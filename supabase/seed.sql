-- Local development seed. Runs after `supabase db reset`.
-- Nothing user-specific: service-qa creates throwaway users per spec
-- (web/e2e/service-qa/seed.ts) and web e2e seeds its own user in
-- web/e2e/global-setup.ts.

-- Local plan prices for the `fake` billing provider (BILLING_PROVIDER=fake).
update public.plans set stripe_price_id = 'price_fake_' || id where stripe_price_id is null;
update public.plans set stripe_annual_price_id = 'price_fake_' || id || '_annual'
  where stripe_annual_price_id is null;
