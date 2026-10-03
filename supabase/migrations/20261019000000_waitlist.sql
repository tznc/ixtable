-- Waitlist for ixtable Cloud, written by the marketing site's waitlist form.
--
-- Visitors can add an email. Nobody outside service_role can read, change, or
-- delete rows, so the public API never reveals who is on the list.

create table public.waitlist (
  id bigint generated always as identity primary key,
  email text not null,
  source text not null default 'website'
    constraint waitlist_source_length check (char_length(source) between 1 and 64),
  created_at timestamptz not null default now()
);

-- One row per address, case-insensitive. The form treats a unique violation
-- (23505) as success, so a duplicate submission looks the same as a new one.
create unique index waitlist_email_lower_key on public.waitlist (lower(email));

alter table public.waitlist enable row level security;

-- Supabase grants anon and authenticated full table access by default.
-- Replace that with insert on the two client-writable columns only, so callers
-- cannot set id or backdate created_at.
revoke all on table public.waitlist from anon, authenticated;
grant insert (email, source) on table public.waitlist to anon, authenticated;

create policy waitlist_insert_only
  on public.waitlist
  for insert
  to anon, authenticated
  with check (
    char_length(email) between 3 and 254
    and email ~* '^[^@[:space:]]+@[^@[:space:]]+\.[^@[:space:]]+$'
    and char_length(source) between 1 and 64
  );

-- No select, update, or delete policies: with RLS enabled those are denied.

-- Daily go-to-market funnel for the external reporting job.
--
-- Aggregates only, no PII: the function returns per-day counts and never
-- exposes an email, user id, or any other row-level value.
--
-- Future columns get added as the Cloud tables land: cloud_apps_created,
-- checkpoints_published, invites_sent, invited_users_activated,
-- paid_conversions. Add them at the end of the returned table so existing
-- clients that read columns by name keep working.
--
-- Days are UTC calendar days. The range is inclusive and every day in it gets
-- a row, zero-filled when nothing happened.
--
-- The function is plpgsql rather than sql because a sql function cannot raise
-- an exception, and an oversized range must fail loudly.
create function public.gtm_funnel_daily(start_date date, end_date date)
returns table (
  day date,
  waitlist_signups integer,
  account_signups integer,
  accounts_confirmed integer
)
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  range_start timestamptz;
  range_end timestamptz;
begin
  if start_date is null or end_date is null then
    raise exception 'gtm_funnel_daily: start_date and end_date are required'
      using errcode = '22004';
  end if;

  if end_date < start_date then
    raise exception 'gtm_funnel_daily: end_date % is before start_date %', end_date, start_date
      using errcode = '22023';
  end if;

  if end_date - start_date + 1 > 400 then
    raise exception 'gtm_funnel_daily: range of % days exceeds the 400 days limit',
      end_date - start_date + 1
      using errcode = '22023';
  end if;

  range_start := start_date::timestamp at time zone 'UTC';
  range_end := (end_date + 1)::timestamp at time zone 'UTC';

  return query
  with days as (
    select series.d::date as d
    from pg_catalog.generate_series(
      start_date::timestamp,
      end_date::timestamp,
      interval '1 day'
    ) as series(d)
  ),
  waitlist_daily as (
    select (w.created_at at time zone 'UTC')::date as d, pg_catalog.count(*) as n
    from public.waitlist as w
    where w.created_at >= range_start and w.created_at < range_end
    group by 1
  ),
  signup_daily as (
    select (u.created_at at time zone 'UTC')::date as d, pg_catalog.count(*) as n
    from auth.users as u
    where u.created_at >= range_start and u.created_at < range_end
    group by 1
  ),
  confirmed_daily as (
    select (u.email_confirmed_at at time zone 'UTC')::date as d, pg_catalog.count(*) as n
    from auth.users as u
    where u.email_confirmed_at >= range_start and u.email_confirmed_at < range_end
    group by 1
  )
  select
    days.d,
    coalesce(waitlist_daily.n, 0)::integer,
    coalesce(signup_daily.n, 0)::integer,
    coalesce(confirmed_daily.n, 0)::integer
  from days
  left join waitlist_daily on waitlist_daily.d = days.d
  left join signup_daily on signup_daily.d = days.d
  left join confirmed_daily on confirmed_daily.d = days.d
  order by days.d;
end;
$$;

comment on function public.gtm_funnel_daily(date, date) is
  'Daily GTM funnel counts for an inclusive UTC date range of at most 400 days. '
  'Aggregates only, no PII. Future columns (cloud_apps_created, checkpoints_published, '
  'invites_sent, invited_users_activated, paid_conversions) get added as Cloud tables land. '
  'Callable by service_role only.';

revoke execute on function public.gtm_funnel_daily(date, date) from public, anon, authenticated;
grant execute on function public.gtm_funnel_daily(date, date) to service_role;
