---
name: service-qa
description: >-
  Verify the ixtable Cloud contract (Supabase Auth, PostgREST/RLS, Storage,
  Edge Functions) with real calls against the local Supabase CLI stack, never
  a mocked client, and capture authenticated website pages from the Docusaurus
  production build. Use on /service-qa, when asked to QA cloud, auth, billing
  or Supabase flows, and proactively after changing supabase/**, scripts/cloud/**
  or code that calls the cloud (web/src auth and cloud pages, desktop cloud
  client), before reporting done. Not for desktop-only changes.
---

# Service QA (ixtable Cloud against the local Supabase stack)

## When to use

- The user invokes `/service-qa`, optionally naming a contract
  ("service-qa key grant", "service-qa publish conflict", "service-qa billing webhook").
- Proactively, right after you change the cloud contract: a migration, RLS
  policy, SQL helper, Edge Function or `_shared` module under `supabase/`,
  `scripts/cloud/`, a website page that calls Supabase, or the desktop cloud
  client's request shapes. Do not wait to be asked.

## Relationship to the other QA skills

| Skill | Proves | Stack |
|---|---|---|
| `/app-qa` | desktop Studio/Runtime UI | headless GPUI harness + real Rust backend, no cloud |
| `/web-qa` | public website pages, layout, accessibility | Docusaurus production build in Chromium |
| `/service-qa` | Auth, RLS, Storage, Edge Functions, and the authenticated website pages that consume them | real local Supabase CLI stack + Docusaurus production build |

Do not substitute one for another. A green `/app-qa` run with a stubbed cloud
client does not prove a migration or function works. A green contract spec
does not prove the website page wires it up; capture the page too. Never use
`/service-qa` as evidence for a desktop-only change.

## Ground rules

- **Real local Supabase only.** Every call goes to the CLI stack at
  `http://127.0.0.1:54321` through `web/e2e/service-qa/clients.ts`. The
  harness refuses non-local URLs. Never point at a hosted project
  (`*.supabase.co`), and never mock `@supabase/supabase-js`, PostgREST, Auth
  or Storage.
- **Email/password users only.** Create users with `createTestUser` /
  `cloud.user()` (public `signUp`; confirmations are off locally). Do not use
  OAuth or hard-code user ids or tokens.
- **Seed with the service role, test as the user.** Seed rows the contract
  under test does not create with `getServiceClient()` (seed.ts). Call the
  contract the way the website or desktop does: the user's client (RLS
  applies) or `callFunction(name, { jwt })`.
- **Outbound providers only may be faked.** Billing runs with
  `BILLING_PROVIDER=fake`; post Stripe-shaped events signed with
  `devSecret("STRIPE_WEBHOOK_SECRET")`. Never fake Supabase itself.
- **Clean up.** Use the `cloud` fixture (fixture.ts); it deletes apps, orgs and
  users after every test, pass or fail. Audit rows stay (append-only).
- **No secrets in output.** `recordOutcome` redacts secret-looking keys and
  JWTs; still do not put DEKs, tokens or keys in `details` on purpose.

## Prerequisites

- Docker (the script starts `dockerd` when run as root without one), the
  Supabase CLI 2.x, Node 20+.
- `npm run service-qa:up --prefix web` (= `node scripts/cloud/up.mjs`): generates local
  secrets in `supabase/functions/.env.local` (and `.env`, which
  `supabase start` loads), installs the functions' npm deps into
  `supabase/functions/node_modules`, starts the stack without studio,
  realtime, imgproxy, vector, logflare, supavisor and postgres-meta, runs
  `supabase db reset`, and waits until `functions/v1/health` returns
  `{ok:true}`. Flags: `--no-reset`, `--restart` (needed after editing
  `config.toml` or secrets).
- After a migration-only change with the stack up: `supabase db reset`.
- Ports: API 54321, DB 54322 (`postgresql://postgres:postgres@127.0.0.1:54322/postgres`),
  Mailpit 54324 (invitation and recovery mail).

## Harness (`web/e2e/service-qa/`)

| File | Provides |
|---|---|
| `clients.ts` | `getAnonClient()`, `getServiceClient()`, `callFunction(name, {jwt, body, method, headers})` → `{status, body, headers}`, `functionsUrl(name)`, `devSecret(name)`, `localStack()` |
| `seed.ts` | `createTestUser()`, `signIn()`, `deleteTestUser()`, `createOrg()`, `createApp()`, `createRole()`, `addMember()`, `grantSubscription()`, `createVersion()`, `createInstallation()`, `sha256Hex()` |
| `fixture.ts` | Playwright `test` with a `cloud` fixture: `cloud.user()`, `cloud.org(owner)`, `cloud.app(org, owner)`, `cloud.trackApp()`, guaranteed teardown; `withCloudFixture(fn)` |
| `record.ts` | `recordOutcome(name, {expectations, details})` → `.generated/<name>.json`; `captureOutcome(page, name, {...})` → `.generated/<name>.png` + JSON |
| `contract.ts` + `specs/contract.spec.ts` | recorded client-facing replies in `fixtures/contract/*.json`: the spec fails when a live reply changes shape; `CONTRACT_RECORD=1` re-records after an intended change (then fix the clients: `src-tauri/src/cloud/contract.rs`, `web/src/lib/cloud/types.ts`, and run `node scripts/cloud/contract-doc.mjs`) |
| `fixtures/crm.ixt` | a real CRM archive from the desktop's Rust code (`cd src-tauri && cargo test --lib write_service_qa_crm_fixture -- --ignored`) for UI journeys |
| `health.ts` / `global-setup.ts` | health gate (Auth, PostgREST, `functions/v1/health`) before any spec, then the deterministic e2e user for UI specs |

Specs: contract specs (no browser) in `web/e2e/service-qa/specs/*.spec.ts`
(project `contracts`); UI specs in `web/e2e/service-qa/ui/*.spec.ts` and
`web/e2e/service-qa.spec.ts` (project `ui`, Chromium against the production
build on `http://127.0.0.1:3001`). Worked examples:
`specs/rls-private-by-default.spec.ts` (RLS, storage, immutability) and
`specs/health.spec.ts` (Edge Function over HTTP).

## Spec shape

```ts
import { callFunction } from "../clients";
import { expect, test } from "../fixture";
import { recordOutcome } from "../record";
import { createRole, grantSubscription } from "../seed";

test("key-grant refuses a revoked member", async ({ cloud }) => {
  const owner = await cloud.user();
  const member = await cloud.user();
  const app = await cloud.app(await cloud.org(owner), owner);
  await grantSubscription(app.id);
  // ...seed membership, revoke it...

  const result = await callFunction("key-grant", {
    jwt: member.jwt,
    body: { appId: app.id, installationId, datasourceId: "main" },
  });
  expect(result.status).toBe(403);
  expect(result.body).toEqual({ error: { code: "REVOKED", message: expect.any(String) } });

  recordOutcome("key-grant-03-revoked-member", {
    expectations: [
      "A revoked Runtime User gets 403 REVOKED from key-grant.",
      "No key_grants row is written for the refused request.",
    ],
    details: { status: result.status, body: result.body },
  });
});
```

`recordOutcome` requires 1–3 plain-English expectations. More than 3 means
the outcome claims too much: split it into a second outcome with its own
name. Names are kebab-case `<slug>-<NN>-<what-it-shows>` and become the file
names. Expectations are the reviewer checklist; keep the real `expect` calls
in the spec body too.

## Workflow

1. **Identify the contract.** From the request or the changed files, name the
   table/policy/function, the caller (anon, unrelated user, Runtime User,
   owner, org admin, operator), the expected result, the error code from the
   API contract (`docs/decisions/cloud-architecture.md`), and the side
   effects (rows, audit events, storage objects).
2. **Write or extend a spec** in `web/e2e/service-qa/specs/<contract>.spec.ts`.
   One spec file per contract; extend an existing file rather than duplicate
   its seeding. Cover the denial paths as well as the happy path, and add a
   positive control so a denial is not just an empty table.
3. **Run it.**
   - First run in a session, or after `config.toml`/secret changes:
     `npm run service-qa:up --prefix web` (add `--restart` if the stack was already up).
   - Contracts: `npm run service-qa:contracts --prefix web`, or one file:
     `cd web && npx playwright test --config playwright.service-qa.config.ts --project=contracts specs/<file>`.
   - `_shared` Deno unit tests and type checks: `npm run service-qa:deno --prefix web`.
4. **Verify every outcome against DB state.** Read each
   `web/e2e/service-qa/.generated/<name>.json` and confirm or refute each
   expectation from the test output and a follow-up query (service client, or
   `docker exec supabase_db_ixtable psql -U postgres -c "<sql>"`). Passing
   `expect` calls can still leave a wrong row shape, a missing audit event or
   a leaked fixture.
5. **Capture the UI** when a website page consumes the contract: a spec in
   `web/e2e/service-qa/ui/` logs in with email/password, drives the page by
   role/label, asserts the result, and calls `captureOutcome(page, name, …)`.
   Run `npm run service-qa:ui --prefix web` (builds and serves the production site; needs
   a Playwright Chromium or `PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH`). Open every
   PNG and confirm it shows the authenticated result, not an error or a
   loading state.
6. **Report.** Which contracts passed, any expectation that did not hold
   (quoted), the spec files added or extended, and the PNG/JSON paths under
   `web/e2e/service-qa/.generated/` for the reviewer to open.
7. **Lint and typecheck last:** `cd web && npx tsc && npx eslint e2e/service-qa && npx oxlint e2e/service-qa`,
   and `npm run service-qa:deno --prefix web` for function code.

## Keep specs around

`web/e2e/service-qa/specs/` is the cloud contract library. Do not delete a
spec after using it; extend it when the contract changes.

## What not to do

- Do not mock supabase-js, PostgREST, Auth, Storage or Edge Functions.
- Do not hit a hosted Supabase project or real Stripe.
- Do not use OAuth providers or the admin API to create the user under test
  (admin API is fine for teardown).
- Do not put business logic in the harness: only clients, seeding, fixtures,
  health and outcome recording.
- Do not grant broad table privileges or disable RLS to make a spec pass.
  Fix the policy or the function.
- Do not start the desktop app or the Rust bridge here; that is `/app-qa`.
- Do not report a capture or outcome you have not opened and checked.
