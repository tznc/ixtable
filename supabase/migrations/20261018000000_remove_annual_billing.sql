-- Remove annual billing (PRD §4.5): every plan is sold monthly only.
-- Reverses the annual parts of 20261010000000_org_app_access_billing.sql;
-- per-app access and org billing customers stay.

-- Same rules as 20261010000000, without the billing interval.
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
    'planId', v_sub.plan_id
  );
end;
$$;

alter table public.plans drop constraint plans_annual_two_months_free;
drop index public.plans_stripe_annual_price_idx;
alter table public.plans drop column annual_price_cents;
alter table public.plans drop column stripe_annual_price_id;
alter table public.subscriptions drop column billing_interval;
alter table public.billing_checkout_sessions drop column billing_interval;
comment on column public.plans.interval is 'Billing interval of price_cents (always month).';
