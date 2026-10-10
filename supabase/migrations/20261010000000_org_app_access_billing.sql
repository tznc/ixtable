-- ixtable Cloud: per-app console access for organization members and
-- organization-level billing with monthly or annual intervals (PRD §4.5,
-- §20.3). Default privileges are revoked (20261003000000_cloud_core.sql), so
-- every grant here is explicit.

-- Per-app access ------------------------------------------------------------

-- One console role per organization member per app, on top of their org
-- role. Granted, changed and removed only through app-access-update (audited).
create table public.app_collaborators (
  app_id uuid not null references public.cloud_apps (id) on delete cascade,
  user_id uuid not null references auth.users (id) on delete cascade,
  role text not null check (role in ('admin', 'billing', 'viewer')),
  granted_by uuid references auth.users (id) on delete set null,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  primary key (app_id, user_id)
);
create index app_collaborators_user_idx on public.app_collaborators (user_id);
create trigger app_collaborators_updated_at before update on public.app_collaborators
  for each row execute function public.set_updated_at();

-- A collaborator must belong to the app's organization.
create or replace function public.app_collaborators_require_org_member()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  if not exists (
    select 1 from public.cloud_apps a
    join public.org_members m on m.org_id = a.org_id
    where a.id = new.app_id and m.user_id = new.user_id
  ) then
    raise exception 'user % is not a member of the app''s organization', new.user_id
      using errcode = 'foreign_key_violation';
  end if;
  return new;
end;
$$;
create trigger app_collaborators_org_member before insert or update on public.app_collaborators
  for each row execute function public.app_collaborators_require_org_member();

-- Leaving or being removed from an organization drops its per-app grants.
create or replace function public.org_members_drop_app_access()
returns trigger
language plpgsql
security definer
set search_path = ''
as $$
begin
  delete from public.app_collaborators c
  using public.cloud_apps a
  where a.id = c.app_id and a.org_id = old.org_id and c.user_id = old.user_id;
  return null;
end;
$$;
create trigger org_members_drop_app_access after delete on public.org_members
  for each row execute function public.org_members_drop_app_access();

-- Capabilities of a user on a live app, the union of every source:
--   owner   app Developer/Owner (publish, credentials)
--   admin   manage Runtime Users, roles, settings (owner, org owner/admin, app admin)
--   billing manage the plan (admin sources, org billing, app billing)
--   view    read the console (admin sources, app viewer)
-- Sorted, distinct; empty for strangers and deleted apps.
create or replace function public.app_capabilities_for(p_app_id uuid, p_user_id uuid)
returns text[]
language sql
stable
security definer
set search_path = ''
as $$
  select coalesce((
    select array(
      select distinct cap from unnest(
        case when a.owner_id = p_user_id then array['owner', 'admin', 'billing', 'view'] else '{}'::text[] end
        || case om.role
             when 'owner' then array['admin', 'billing', 'view']
             when 'admin' then array['admin', 'billing', 'view']
             when 'billing' then array['billing']
             else '{}'::text[]
           end
        || case c.role
             when 'admin' then array['admin', 'billing', 'view']
             when 'billing' then array['billing']
             when 'viewer' then array['view']
             else '{}'::text[]
           end
      ) as cap
      order by cap
    )
    from public.cloud_apps a
    left join public.org_members om on om.org_id = a.org_id and om.user_id = p_user_id
    left join public.app_collaborators c on c.app_id = a.id and c.user_id = p_user_id
    where a.id = p_app_id and a.deleted_at is null and p_user_id is not null
  ), '{}'::text[]);
$$;

-- The caller's capabilities (website: which tabs to show).
create or replace function public.app_capabilities(p_app_id uuid)
returns text[]
language sql
stable
security definer
set search_path = ''
as $$
  select public.app_capabilities_for(p_app_id, (select auth.uid()));
$$;

-- Owner, org owner/admin, or per-app admin (was: owner or org owner/admin).
create or replace function public.is_app_admin(p_app_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select 'admin' = any (public.app_capabilities_for(p_app_id, (select auth.uid())));
$$;

create or replace function public.can_view_app(p_app_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select 'view' = any (public.app_capabilities_for(p_app_id, (select auth.uid())));
$$;

create or replace function public.can_manage_app_billing(p_app_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select 'billing' = any (public.app_capabilities_for(p_app_id, (select auth.uid())));
$$;

-- Org owners/admins see profiles of their org members; anyone who can view
-- an app sees its owner, Runtime Users and collaborators.
create or replace function public.can_view_profile(p_user_id uuid)
returns boolean
language sql
stable
security definer
set search_path = ''
as $$
  select p_user_id = (select auth.uid())
    or exists (
      select 1 from public.org_members target
      join public.org_members me on me.org_id = target.org_id
      where target.user_id = p_user_id
        and me.user_id = (select auth.uid())
        and me.role in ('owner', 'admin')
    )
    or exists (
      select 1 from public.cloud_apps a
      where a.owner_id = p_user_id and public.can_view_app(a.id)
    )
    or exists (
      select 1 from public.app_members target
      where target.user_id = p_user_id and public.can_view_app(target.app_id)
    )
    or exists (
      select 1 from public.app_collaborators target
      where target.user_id = p_user_id and public.can_view_app(target.app_id)
    );
$$;

-- Same entitlement rules; billing-only and viewer access may now ask too.
create or replace function public.app_entitlement(p_app_id uuid)
returns jsonb
language plpgsql
stable
security definer
set search_path = ''
as $$
declare
  v_app public.cloud_apps;
  v_sub public.subscriptions;
  v_allowance integer := 0;
  v_used integer := 0;
  v_reason text;
  v_allowed boolean;
  -- JWT role of the PostgREST request; empty for direct database sessions.
  v_role text := coalesce(nullif(current_setting('request.jwt.claims', true), '')::jsonb ->> 'role', '');
begin
  select * into v_app from public.cloud_apps where id = p_app_id;
  if not found
    or (
      v_role not in ('', 'service_role')
      and cardinality(public.app_capabilities(p_app_id)) = 0
      and not public.is_app_member(p_app_id)
    )
  then
    return jsonb_build_object('allowed', false, 'reason', 'not_found', 'allowance', 0, 'used', 0);
  end if;

  select count(*) into v_used from public.app_members
  where app_id = p_app_id and status = 'active';

  if v_app.deleted_at is not null then
    return jsonb_build_object('allowed', false, 'reason', 'app_deleted', 'allowance', 0, 'used', v_used);
  end if;

  select * into v_sub from public.subscriptions where app_id = p_app_id;
  if not found then
    return jsonb_build_object('allowed', false, 'reason', 'no_subscription', 'allowance', 0, 'used', v_used);
  end if;

  select runtime_user_allowance into v_allowance from public.plans where id = v_sub.plan_id;

  if v_sub.status in ('active', 'trialing') then
    v_allowed := true; v_reason := 'ok';
  elsif v_sub.status = 'past_due'
    and coalesce(v_sub.current_period_end, now()) + interval '7 days' > now() then
    v_allowed := true; v_reason := 'grace';
  else
    v_allowed := false; v_reason := 'subscription_inactive';
  end if;

  if v_allowed and v_used > v_allowance then
    v_allowed := false; v_reason := 'over_allowance';
  end if;

  return jsonb_build_object(
    'allowed', v_allowed,
    'reason', v_reason,
    'allowance', v_allowance,
    'used', v_used,
    'status', v_sub.status,
    'planId', v_sub.plan_id,
    'interval', v_sub.billing_interval
  ) ;
end;
$$;

-- Billing --------------------------------------------------------------------

-- Every plan sells monthly and annually; annual is two months free.
alter table public.plans add column annual_price_cents integer;
alter table public.plans add column stripe_annual_price_id text;
update public.plans set annual_price_cents = price_cents * 10 where annual_price_cents is null;
alter table public.plans alter column annual_price_cents set not null;
alter table public.plans add constraint plans_annual_two_months_free
  check (annual_price_cents = price_cents * 10);
create unique index plans_stripe_price_idx on public.plans (stripe_price_id);
create unique index plans_stripe_annual_price_idx on public.plans (stripe_annual_price_id);
comment on column public.plans.interval is
  'Base interval of price_cents (always month). Annual pricing is annual_price_cents.';

alter table public.subscriptions add column billing_interval text not null default 'month'
  check (billing_interval in ('month', 'year'));
alter table public.billing_checkout_sessions add column billing_interval text not null default 'month'
  check (billing_interval in ('month', 'year'));

-- The organization is the billing customer: one Stripe customer per org,
-- one subscription per app. Function-only: no client grants, no policies.
create table public.org_billing_customers (
  org_id uuid primary key references public.organizations (id) on delete cascade,
  provider text not null check (provider in ('stripe', 'fake')),
  stripe_customer_id text not null unique,
  created_at timestamptz not null default now()
);
alter table public.org_billing_customers enable row level security;
revoke all on public.org_billing_customers from anon, authenticated;
grant all on public.org_billing_customers to service_role;

-- Grants and policies ----------------------------------------------------------

alter table public.app_collaborators enable row level security;
grant select on public.app_collaborators to authenticated;
grant all on public.app_collaborators to service_role;
create policy app_collaborators_select on public.app_collaborators for select to authenticated
  using (user_id = (select auth.uid()) or public.can_view_app(app_id));

revoke all on function public.app_capabilities_for(uuid, uuid) from public, anon, authenticated;
revoke all on function public.app_capabilities(uuid) from public, anon;
revoke all on function public.can_view_app(uuid) from public, anon;
revoke all on function public.can_manage_app_billing(uuid) from public, anon;
revoke all on function public.app_collaborators_require_org_member() from public, anon, authenticated;
revoke all on function public.org_members_drop_app_access() from public, anon, authenticated;
grant execute on function public.app_capabilities_for(uuid, uuid) to service_role;
grant execute on function public.app_capabilities(uuid) to authenticated, service_role;
grant execute on function public.can_view_app(uuid) to authenticated, service_role;
grant execute on function public.can_manage_app_billing(uuid) to authenticated, service_role;

drop policy cloud_apps_select on public.cloud_apps;
create policy cloud_apps_select on public.cloud_apps for select to authenticated
  using (
    public.can_view_app(id)
    or public.can_manage_app_billing(id)
    or (deleted_at is null and public.is_app_member(id))
    or public.is_org_member(org_id, array['owner', 'admin', 'billing'])
  );

drop policy app_roles_select on public.app_roles;
create policy app_roles_select on public.app_roles for select to authenticated
  using (
    public.can_view_app(app_id)
    or exists (
      select 1 from public.app_members m
      where m.app_id = app_roles.app_id and m.role_id = app_roles.id
        and m.user_id = (select auth.uid()) and m.status = 'active'
    )
  );

drop policy app_members_select on public.app_members;
create policy app_members_select on public.app_members for select to authenticated
  using (user_id = (select auth.uid()) or public.can_view_app(app_id));

drop policy app_versions_select on public.app_versions;
create policy app_versions_select on public.app_versions for select to authenticated
  using (
    public.can_view_app(app_id)
    or (status = 'published' and public.is_app_member(app_id))
  );

drop policy installations_select on public.installations;
create policy installations_select on public.installations for select to authenticated
  using (user_id = (select auth.uid()) or public.can_view_app(app_id));

drop policy subscriptions_select on public.subscriptions;
create policy subscriptions_select on public.subscriptions for select to authenticated
  using (
    public.can_view_app(app_id)
    or public.can_manage_app_billing(app_id)
    or exists (
      select 1 from public.cloud_apps a
      where a.id = subscriptions.app_id
        and public.is_org_member(a.org_id, array['owner', 'admin', 'billing'])
    )
  );

drop policy audit_events_select on public.audit_events;
create policy audit_events_select on public.audit_events for select to authenticated
  using (
    (app_id is not null and (
      public.can_view_app(app_id)
      or exists (
        select 1 from public.cloud_apps a
        where a.id = audit_events.app_id and a.owner_id = (select auth.uid())
      )
    ))
    or (org_id is not null and public.is_org_member(org_id, array['owner', 'admin']))
  );
