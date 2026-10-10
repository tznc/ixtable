# ixtable Cloud architecture

Status: accepted and implemented: the control-plane foundation (schema,
RLS, shared function code, local stack, service-qa harness), the
distribution, credential, desktop sign-in, billing and account functions,
and the desktop client. Covers PRD §4.2, §20–§25,
§27.2, §27.5, Phase 4 and Phase 5. Security model:
[cloud-security-model.md](./cloud-security-model.md).

## Context

ixtable Cloud is the paid layer over the free desktop app: accounts and
organizations, one Developer/Owner per cloud app, Runtime Users with custom
roles, explicit publishing, signed and fingerprinted bundles, encrypted
credential delivery with 24-hour key grants, archive backup and restore,
audit history, and per-app plans. It is not a managed database, a browser
runtime or a sync engine (PRD §4.3). Apps are private by default, with no
public links or anonymous sessions (PRD §21.1).

## Decision

### Supabase is the control plane

| Concern | Supabase piece |
|---|---|
| Metadata | Postgres with row-level security on every table |
| Accounts | Auth: email/password, Google, Microsoft (`azure` provider), invitations |
| Archives | Storage bucket `app-archives` (private, 500 MiB per object) |
| Privileged operations | Edge Functions (Deno) using the service role |

The desktop and the website use supabase-js with the anon key and the user's
JWT, and call `functions/v1/<name>`. The service role key exists only inside
Edge Functions.

### Schema

Migrations: `supabase/migrations/20261003000000_cloud_core.sql` (tables),
`…000100_cloud_policies.sql` (helpers, RLS, grants),
`…000200_cloud_storage_plans.sql` (bucket, storage lockdown, plans),
`20261003100000_distribution_functions.sql` and
`20261003100100_distribution_withdraw.sql` (see Distribution functions),
`20261003200000_credentials_desktop_auth.sql` (envelope supersede and erase,
`credential_envelope_put`), `20261003300000_commercial_billing.sql`
(checkout sessions, webhook ordering and outcome columns) and
`20261010000000_org_app_access_billing.sql` (per-app access, capabilities,
annual intervals, org billing customers).

| Table | Purpose |
|---|---|
| `profiles` | one row per auth user (trigger `handle_new_user`), `is_operator` |
| `organizations`, `org_members` | tenancy; roles owner, admin, billing, member; creator becomes owner (trigger); the last owner cannot leave |
| `cloud_apps` | `org_id`, single `owner_id` (NOT NULL), `document_id`, `datasource_kind` sqlite/postgres, `backups_enabled`, `retention_versions`, `retention_days`, `head_version_id`, soft delete `deleted_at` |
| `app_roles` | custom runtime roles, key `(app_id, id)` where `id` is the desktop role id; `permissions` mirrors `roles.rs` |
| `app_members` | `(app_id, user_id)`, `role_id`, status active/revoked |
| `app_collaborators` | per-app console role (admin, billing, viewer) of an org member, PRD §20.3; written only by `app-access-update`; rows go when the user leaves the org (trigger) |
| `invitations` | org or app invitation; only `token_hash` (sha256 hex) is stored |
| `app_versions` | published checkpoints; immutable except `status` pending→published/withdrawn and published→withdrawn (trigger `app_versions_immutable`) |
| `archive_uploads` | signed upload sessions (expected size and sha256) |
| `installations` | Runtime installation per device, `revoked_at` |
| `installation_backups` | per-installation backup stream |
| `credential_envelopes` | datasource ciphertext + KEK-wrapped DEK, scope shared or per user |
| `key_grants` | one row per issued key grant (no key material) |
| `plans`, `subscriptions`, `billing_events` | catalog (starter 5, team 25, business 100 runtime users) with monthly and annual prices (`annual_price_cents` = 10 × monthly, enforced by a check), one subscription per app with `billing_interval` month/year, webhook idempotency on `event_id` |
| `org_billing_customers` | function-only; one billing (Stripe) customer per organization, PRD §4.5 |
| `billing_checkout_sessions` | function-only; checkout sessions handed out by `billing-checkout`, consumed once by `billing-fake-complete` |
| `audit_events` | append-only (trigger blocks UPDATE and DELETE for every role) |
| `rate_limits`, `service_metrics`, `desktop_auth_requests` | function-only operational tables |

SQL helpers (security definer, `search_path = ''`):
`is_org_member(org, roles[] default null)`, `is_app_owner(app)`,
`app_capabilities_for(app, user)` (service role) and `app_capabilities(app)`
→ sorted `text[]` of `owner`, `admin`, `billing`, `view`, the union of the
app owner, the org role (owner/admin → admin+billing+view, billing →
billing) and the per-app role (admin → admin+billing+view, billing →
billing, viewer → view); `is_app_admin(app)`, `can_view_app(app)` and
`can_manage_app_billing(app)` test one capability; `is_app_member(app)`
(active Runtime User), `can_view_profile(user)`, `app_entitlement(app) → jsonb
{allowed, reason, allowance, used, status, planId}`, and the service-role-only
`audit(action, actor, org, app, target, details, ip_hash)`,
`rate_limit(bucket, max, window_seconds) → boolean`,
`metric_increment(name, by)`.

### Private by default

- The first migration revokes default table, sequence and function
  privileges from `anon` and `authenticated` (and PUBLIC execute on new
  functions). **A later migration must GRANT explicitly** whatever clients
  may do, and enable RLS with policies. A new table without grants is
  invisible to clients, which is the intended failure mode.
- `anon` has no privileges on any cloud table.
- `authenticated` has `SELECT` on the tables the website reads, filtered by
  RLS, and column-level `UPDATE` only for settings the website edits (app
  name, backups, retention; org name; member role; profile display name;
  invitation revocation). The only client inserts and deletes are on
  organizations (any user creates one and becomes its owner; an owner
  deletes it, blocked while apps exist) and `org_members` (leave, or an
  owner/admin removes a member). App creation and deletion, owner transfer,
  publishing, Runtime User membership changes and every other write that
  needs auditing go through Edge Functions.
- Column grants hide secrets: `invitations.token_hash` and the envelope
  ciphertext, nonce, AAD and wrapped DEK are never selectable by clients.
- Storage: a restrictive policy on `storage.objects` denies `anon` and
  `authenticated` any access to `app-archives`. Functions mint signed upload
  and download URLs with the service role.

### Edge Functions

Every function is JSON over `POST` (health also `GET`) with
`Authorization: Bearer <user JWT>`, except `health`, `stripe-webhook`,
`desktop-auth-exchange` and `retention-sweep`, which set `verify_jwt = false`
in `supabase/config.toml` and authenticate in code (`health` is public). Errors are
`{ error: { code, message, details? } }`:

| Code | HTTP |
|---|---|
| UNAUTHENTICATED | 401 |
| ENTITLEMENT_REQUIRED | 402 |
| FORBIDDEN, REVOKED | 403 |
| NOT_FOUND | 404 |
| METHOD_NOT_ALLOWED | 405 |
| VERSION_CONFLICT | 409 |
| TOO_LARGE | 413 |
| VALIDATION | 422 |
| PENDING | 428 |
| RATE_LIMITED | 429 |
| INTERNAL | 500 (no detail leaked) |

Shared code in `supabase/functions/_shared/`:

| Module | Exports |
|---|---|
| `http.ts` | `handler(fn, {methods})`, `HttpError(code, message, details?)`, `json`, `errorResponse`, `preflight`, `corsHeaders`, `readJson`, `bearerToken`, `requireUser(req) → {user, jwt}`, `ERROR_STATUS` |
| `db.ts` | `serviceClient()`, `userClient(jwt)`, `anonClient()`, `env`, `optionalEnv`, `ARCHIVE_BUCKET`, `MAX_ARCHIVE_BYTES` |
| `audit.ts` | `audit({action, actorId, orgId, appId, target, details, req})`, `redactSecrets`, `ipHash` |
| `rateLimit.ts` | `RATE_LIMITS`, `enforceNamedRateLimit`, `rateLimit`, `enforceRateLimit` (throws RATE_LIMITED), `incrementMetric` |
| `entitlements.ts` | `getEntitlement(appId)`, `requireEntitlement(appId, {adding})` (throws ENTITLEMENT_REQUIRED) |
| `crypto.ts` | Ed25519 sign/verify, `canonicalJson`, `bundleFingerprint`, AES-256-GCM `wrapDek`/`unwrapDek`, `hmacSha256Hex`, `sha256Hex`, `pkceChallenge`, `randomToken`, `timingSafeEqual`, base64 helpers |
| `billing.ts` | `billingProvider()` (`stripe` or `fake`), `fakeBillingAllowed`, `verifyStripeSignature`, `signStripePayload`, `buildFakeEvent`, `decideSubscriptionEvent`, `accessChangeAction` |
| `validate.ts` | `str`, `uuid`, `int`, `bool`, `oneOf`, `arr`, `record`, `sha256`, `semver`, `email` (throw VALIDATION) |
| `distribution.ts` | paths, manifest, security summary, `mapDbError`, `publicUrl`, access checks for the distribution functions |
| `credentials.ts`, `credentialAccess.ts` | envelope and PKCE input rules, `envelopeAad`, grant expiry, desktop sign-in state; live app and membership loading for the credential functions |
| `commercial.ts` | billing app access, subscription rows, `cancelSubscriptionNow` |
| `production.ts` | production-only guards (`IXTABLE_ENV=production`), `requireEmailConfirmations` |

CORS echoes only the website origins (`SITE_URL`, local 3001,
`CORS_ALLOWED_ORIGINS`) and the Tauri origins `tauri://localhost`,
`http(s)://tauri.localhost`. The local Kong gateway rewrites the header to
`*`; hosted Supabase passes the function's header through.

npm dependencies of functions are declared in `supabase/functions/package.json`
(bare imports, `deno.json` `nodeModulesDir: "manual"`), and only `db.ts`
imports supabase-js.

### Archives, versions and bundles

Paths: `apps/<appId>/versions/<versionId>.ixt` (developer stream) and
`apps/<appId>/installations/<userId>/<installationId>/<backupId>.ixt`
(installation stream, never merged). Publishing is explicit and carries
`expectedHeadVersionId`; a mismatch is `VERSION_CONFLICT`, resolved by
explicit overwrite or fork (`app_versions.resolution`, `parent_version_id`).
Bundle manifests are `canonicalJson` strings signed with Ed25519; see the
security model for formats.

### Billing

Per-app plans with a runtime-user allowance. `app_entitlement` allows
`active` and `trialing`, and `past_due` for 7 days after the period end;
active Runtime Users must not exceed the allowance (the owner is not
counted). Activating a member (invitation accept, re-activation) checks room
for one more inside the SQL function `distribution_activate_member`, under
the app row lock. Functions call `requireEntitlement` on publish, overwrite
and fork, archive upload URLs, bundle download, key grant and backup commit. `stripe-webhook` verifies the
`Stripe-Signature` HMAC and is idempotent through `billing_events.event_id`.
`BILLING_PROVIDER=fake` (local/QA, and only with
`IXTABLE_ALLOW_FAKE_BILLING=1`) returns website URLs; the flow completes
with a Stripe-shaped event signed by the same secret.

Billing model (PRD §4.5): the organization is the customer and each app has
its own subscription. `billing-checkout` creates the org's provider customer
on first use (`org_billing_customers`) and reuses it for every app, so the
portal and invoices are per organization. It takes `interval` month or year
and uses `stripe_price_id` or `stripe_annual_price_id`; buying the same plan
at the other interval is a change, buying the same plan and interval again
is 422. Checkout never sets a trial and always collects a payment method.
The webhook resolves plan and interval from the price first (portal
switches change the price, not the metadata), then from
`metadata.plan_id`/`billing_interval`; an interval switch is audited as
`billing.plan_changed`. Billing functions require the `billing` capability.

Per-app access (PRD §20.3): `app-access-update {appId, userId, role|null}`
grants, changes or removes an org member's per-app role. Only org
owners/admins may call it; the target must be an org member and not the app
owner. Audited `access.grant|change|remove`. Edge Function admin checks
(`isAppAdmin` in `distribution.ts` and `credentialAccess.ts`) call
`app_capabilities_for`, so SQL and functions share one rule. Credentials and
publishing stay owner-only.

Webhook ordering (`_shared/billing.ts` `decideSubscriptionEvent`): the
subscription row keeps `provider_event_at` (the newest applied
`event.created`); older events are recorded as `stale` and not applied, and
the update is a compare-and-set on that column. Events about a subscription
other than the app's current one are ignored, except a new purchase, which
replaces it (the old one is canceled with the provider). `invoice.payment_failed`
moves active to `past_due` (grace), `invoice.paid` recovers it. Each event's
`outcome` (applied, ignored, stale, failed) is stored on `billing_events`;
a failed one returns 500 so Stripe retries, and a processed one is
acknowledged as a duplicate. Access-relevant changes are audited as
`billing.subscription_activated|past_due|canceled|inactive`,
`billing.payment_recovered`, `billing.plan_changed`, `billing.cancel_scheduled|reverted`.
A downgrade below the active Runtime Users is allowed and reported as
`over_allowance` until members are revoked. The fake checkout records a
`billing_checkout_sessions` row that `billing-fake-complete` consumes once.
Account deletion purges the caller's apps (soft delete and audit first,
then the rows, since `cloud_apps.owner_id` is `ON DELETE RESTRICT`).
Per-endpoint limits live in `RATE_LIMITS` (`_shared/rateLimit.ts`).
`account-export` returns the caller's own data without secrets, and
`admin-support` gives operators (`profiles.is_operator`) diagnostics built
from explicit field lists, audited as `admin.lookup`. Operations:
`docs/ops/`.

Evidence: `web/e2e/service-qa/specs/{billing,app-access,account,admin,ratelimit}.spec.ts`,
`supabase/functions/_shared/billing_events_test.ts` (event ordering and
outcomes) and `billing_test.ts`.

### Local stack and secrets

`node scripts/cloud/up.mjs` (`npm run service-qa:up`) starts the stack with
only the services ixtable uses, resets the database, and gates on
`functions/v1/health`. `scripts/cloud/dev-secrets.mjs` writes throwaway
secrets to `supabase/functions/.env.local` (mirrored to `.env`, which
`supabase start` loads; both gitignored). `.env.example` lists every
variable. Google and Microsoft sign-in are configured but disabled locally;
`config.toml` documents how to enable them.

## Consequences

- Clients can never write cloud state directly, so every access-relevant
  change has one audited code path. The cost is an Edge Function per
  mutation.
- New tables and functions are inaccessible until a migration grants them,
  which makes forgotten policies fail closed instead of open.
- Audit rows outlive apps and accounts (no foreign keys), so they hold user
  ids after account deletion; they hold no secrets and IPs only as keyed
  hashes.
- `app_versions` rows cannot be edited even by operators; fixing a bad
  publish means withdrawing it and publishing a new version.
- Local CORS is looser than hosted (gateway `*`). The origin list is unit
  tested instead.

## Evidence

- `web/e2e/service-qa/specs/rls-private-by-default.spec.ts`: anon and
  unrelated users read nothing, Runtime User scope, no client writes or
  privileged RPCs, append-only audit, immutable versions, signed-URL-only
  storage.
- `web/e2e/service-qa/specs/health.spec.ts`: health and error contract.
- `supabase/functions/_shared/*_test.ts` (`npm run service-qa:deno`): crypto
  vectors and Node interoperability, Stripe signatures, CORS, error mapping,
  validators.

## Distribution functions

Status: accepted. Functions `apps-create`, `apps-delete`, `apps-transfer`,
`roles-sync`, `invitations-create`, `invitations-accept`, `members-update`,
`archive-upload-url`, `publish-checkpoint`, `versions-resolve`,
`restore-url`, `bundle-manifest`, `sync-check`, `backup-commit`,
`retention-sweep`. Shared code: `supabase/functions/_shared/distribution.ts`
(pure helpers unit tested in `distribution_test.ts`). Migrations
`20261003100000_distribution_functions.sql` and
`20261003100100_distribution_withdraw.sql` add service-role-only SQL
functions that run each check and write in one transaction under a row lock
on the app: `distribution_commit_version`, `distribution_commit_backup`,
`distribution_accept_invitation`, `distribution_activate_member`,
`distribution_update_member`, `distribution_withdraw_version`. They raise
`IXnnn` SQLSTATEs that `mapDbError` turns into the error contract (IX404
NOT_FOUND, IX409 VERSION_CONFLICT with `details.headVersionId`, IX402
ENTITLEMENT_REQUIRED with `details.reason`, IX403 FORBIDDEN, IX410/IX422
VALIDATION, IX423 VALIDATION with `details {requiresConfirm, installations}`).

### Rules

| Function | Who | Gates | Audit |
|---|---|---|---|
| apps-create `{orgId, name, documentId, datasourceKind?}` → `{app}` | org owner/admin/member (billing 403, outsider 404) | one live app per (org, documentId), else 422 with `details.appId` | app.create |
| apps-delete `{appId, confirm}` → `{appId, deletedAt, subscriptionStatus}` | app owner or org owner | `confirm` = app name; soft delete; revokes members, installations, key grants, pending invitations | app.delete (`billingCancellationRequired`) |
| apps-transfer `{appId, newOwnerId, confirm}` → `{app}` | current owner | new owner is an org owner/admin/member; their Runtime User row is removed | app.transfer |
| roles-sync `{appId, roles:[{id,name,permissions}]}` → `{roles, kept}` | owner | upsert by desktop id; absent roles deleted unless a member or pending invitation uses them (`kept`) | role.sync |
| invitations-create `{kind:"app", appId, email, roleId}` \| `{kind:"org", orgId, email, role}` → `{invitation, acceptUrl, delivery}` | app admin / org owner-admin | revokes older pending invitations for the same email and target | invitation.create |
| invitations-accept `{token}` → `{membership}` | invitee with the verified invited email | single use, 7 days; app: entitlement with room for one more (402) | invitation.accept + member.add / org_member.add |
| members-update `{appId, userId, roleId?, status?}` → `{member}` | app admin | re-activation needs allowance (402) | member.role_change / member.revoke / member.activate |
| app-access-update `{appId, userId, role \| null}` → `{collaborator \| null}` | org owner/admin | target is an org member and not the app owner (422) | access.grant / access.change / access.remove |
| archive-upload-url `{appId, kind, size, sha256, installationId?}` → `{uploadId, path, signedUrl, token, expiresAt}` | version: owner; backup: owner or active member, backups enabled | ≤ 500 MB (413), entitlement; backup registers the installation | archive.upload |
| publish-checkpoint (contract fields) → `{version}` | owner | entitlement, head precondition (409), version > head (422), security summary, stored object size | version.publish |
| versions-resolve overwrite / fork / withdraw | owner | see below | version.overwrite / version.fork (+ app.create) / version.withdraw |
| restore-url `{appId, versionId \| backupId}` → `{signedUrl, sha256, size, isPostgres, warning, kind, id, expiresAt}` | versions: owner; backups: owner or the backup's user | 15-minute URL | version.restore / backup.restore |
| bundle-manifest `{appId, installationId, deviceName?}` → `{manifest, signature, archiveUrl, archiveUrlExpiresAt}` | owner or active member (403 FORBIDDEN / REVOKED) | entitlement, installation not revoked or foreign | bundle.generate |
| sync-check `{appId, installedVersionId, installationId}` → `{upToDate, latest}` | same as bundle | records `installed_version_id`, `last_seen_at` | none |
| backup-commit `{appId, uploadId, installationId}` → `{backup}` | owner or active member | backups enabled, entitlement, the caller's own installation and upload | backup.upload |
| retention-sweep `{appId?}` → `{deleted, versions, backups, uploads, desktopAuthRequests}` | service role key (Bearer) or `x-cron-secret` = `CRON_SECRET` | `verify_jwt = false` | retention.sweep |

- **Uploads.** The upload id is also the version or backup id, so the
  storage path is fixed when the URL is minted. Commits check the stored
  object's size (Storage list metadata) against the declared size and mark
  the upload `committed`; a consumed, expired or mismatched upload is 422.
  The signed upload URL refuses a second PUT (no upsert). The server does not
  re-hash archives; the desktop verifies sha256 against the signed manifest.
  The desktop makes up to 4 upload attempts, waiting 1 s, 2 s and 4 s
  between them, after connect errors, timeouts, 5xx and 429. It never
  retries other 4xx replies. Because a retry keeps `x-upsert: false`, it
  cannot overwrite an object. If an earlier attempt landed but its reply was
  lost, the retry fails with 409 and the user uploads again.
- **Runtime backups.** Before a backup, the desktop shows the archive size
  report (`archive_size_report`) and blocks an archive over 500 MB. A runtime
  window lists its own installation's backups from `installation_backups`
  (RLS: the installing user or app admins). It restores one through
  `restore-url {appId, backupId}` into a new local copy. When `isPostgres` is
  set, the window shows the server's warning first.
- **Desktop auth.** "Forgot password?" calls `resetPasswordForEmail` with
  `redirectTo` = `<siteUrl>/reset-password`, the same flow the website uses.
  "Accept an invitation…" takes the emailed link (or bare token) and calls
  `invitations-accept {token}`.
- **Post-update notice.** After auto-sync installs a newer version, the
  runtime shows the version, the applied migrations (`appliedMigrations` in
  `bundle.json`, shown only when its `lastAction` is an update or downgrade)
  and the release notes (line breaks kept) (`app_versions.release_notes`, readable
  by members for published versions). `sync-check` and `bundle-manifest` do
  not carry release notes.
- **Security summary.** Stored normalized on the version: `{store,
  credentialMode, tls, sslmode, insecureTransportConfirmed(At),
  sharedCredentialAcknowledged, concurrencyPoliciesResolved,
  unresolvedEntities}`. Desktop preflight names are accepted as aliases
  (`insecureOverrideConfirmed`, `sharedCredentialWarningAcknowledged`,
  `entityPoliciesResolved`). A PostgreSQL summary without `credentialMode
  "perUser"` counts as shared. Concurrency policies are required when the
  app has more than one active Runtime User or the plan allows more than one.
  Publishing sets `cloud_apps.datasource_kind` from `store`.
- **versions-resolve.** `overwrite` takes the publish fields plus
  `fromVersionId` (the head from the 409) and publishes with resolution
  `overwrite`. `fork` creates a new app in the same org owned by the caller
  (roles copied; no members or subscription; `documentId` defaults to
  `<documentId>:fork:<newAppId>` because one document links to one live app
  per org) from either the caller's pending upload (moved to the new app's
  path) or a copy of a published `fromVersionId`; returns `{app, version}`.
  `withdraw {versionId, confirm?}` marks a published version withdrawn and,
  when it was the head, moves the head to the most recently published
  remaining version (null when none); withdrawing the last published version
  that installations run needs `confirm: true`. Returns `{version,
  headVersionId, dependentInstallations}`. Withdraw is not entitlement-gated.
- **Manifest.** Fields: `format` (`ixtable-cloud-bundle/1`), `appId`,
  `appName`, `versionId`, `version`, `archiveSha256`, `archiveSize`,
  `minRuntimeVersion`, `userId`, `roleId`, `roleName`, `rolePermissions`,
  `owner`, `installationId`, `fingerprint`, `issuedAt`, `expiresAt`
  (`buildManifest` in `_shared/distribution.ts`). The owner gets `owner: true` and `roleId`, `roleName` and
  `rolePermissions` null. A Runtime User always has a role. `expiresAt` is issue time
  plus 24 hours; the archive URL lives 15 minutes.
- **Invitation delivery.** An email with no account gets a Supabase Auth
  invitation (`inviteUserByEmail`, redirect to the accept link); an existing
  account gets a sign-in link (`signInWithOtp`, `shouldCreateUser: false`)
  to the accept link. The reply's `delivery` is always `"sent"` (and the
  audit records only `emailSent`), so it does not reveal whether the address
  has an account.
- **Signed URLs** minted inside the Edge runtime use its internal API host
  locally (`http://kong:8000`); `publicUrl` rewrites them to
  `IXTABLE_PUBLIC_API_URL` when set, else (only when `SUPABASE_URL` is a
  local stack) to `http://127.0.0.1:54321`. Request headers are never
  trusted for this. (Supabase refuses secret names starting `SUPABASE_`.)
- **Retention.** Per app: versions beyond `retention_versions` or older than
  `retention_days` are deleted (storage object first, then the row), never
  the head or a version an installation reports as installed. Each
  installation's backup stream follows the same rule but always keeps its
  newest backup. Pending uploads past expiry become `expired`. A full sweep
  also deletes `desktop_auth_requests` expired over an hour ago.
- **Rate limits** (fixed window): every function except `health`,
  `stripe-webhook` and `retention-sweep` calls
  `enforceNamedRateLimit("<function>", subject)`; the numbers are the
  `RATE_LIMITS` table in `_shared/rateLimit.ts` (listed in the Contract
  appendix). The subject is the caller's user id, `<userId>:<appId>` for
  key-grant, and both `ip:<hash>` and `state:<state>` for
  desktop-auth-exchange. `_shared/rateLimit_test.ts` fails when a function
  uses ad-hoc numbers or a name missing from the table.
- **Deleting an app** mirrors account deletion: while its subscription still
  bills, `apps-delete` answers 403 with `details.reason:
  "active_subscription"` unless `cancelSubscription: true`, which cancels it
  with the provider at once (audited `billing.subscription_canceled`,
  reason `app_delete`) before the soft delete.

Evidence: `web/e2e/service-qa/specs/{apps-journey,apps,invitations,members,publish,versions,bundle,backup,retention}.spec.ts`
(helpers in `distribution-fixtures.ts`).

## Desktop client

Studio and the Runtime talk to the cloud from two places
(`src/cloud`, `src-tauri/src/cloud`):

- **Sign-in.** Email and password go through supabase-js in the webview.
  Google and Microsoft use a browser hand-off: the desktop makes a PKCE
  verifier and opens `<site>/desktop-auth?code_challenge=…&state=…`; the
  signed-in website calls `desktop-auth-approve`; the desktop polls
  `desktop-auth-exchange {state, codeVerifier}` (428 `PENDING` until
  approved) and adopts the returned session with `setSession`. supabase-js
  stores the session through `cloud_auth_storage_*`, which seals it in the
  local secret store; it is never in `localStorage`, archives or logs.
- **Build configuration.** Release builds take the cloud from build-time
  environment variables: `IXTABLE_CLOUD_BUILD_URL`,
  `IXTABLE_CLOUD_BUILD_ANON_KEY`, `IXTABLE_CLOUD_BUILD_SITE_URL`, and the
  pinned bundle-signing key `IXTABLE_CLOUD_PUBLIC_KEY_RAW` (raw 32-byte
  Ed25519 key, base64; or `IXTABLE_CLOUD_PUBLIC_KEY`, SPKI). The
  `IXTABLE_CLOUD_URL`, `IXTABLE_CLOUD_ANON_KEY` and `IXTABLE_CLOUD_SITE_URL`
  environment variables, then the `cloud.*` preferences, override the URL
  defaults (`src-tauri/src/cloud/config.rs`); the key has no runtime
  override in release builds. Without a key every cloud install is refused.
  `release.yml` takes the key from the repository variable and refuses
  beta/stable builds when it is missing or a known dev/test key
  (`scripts/release/keys.mjs`). Debug builds default to the local stack and
  accept the key from the runtime environment (tests).
- **Install and update.** `bundle-manifest` returns the signed manifest and
  a 15-minute archive URL. Rust verifies the Ed25519 signature over the
  canonical JSON, expiry, app, user and installation, then the archive's
  sha256 and size, before using any byte. The archive installs through the
  same path as a runtime bundle (`installation::apply_bundle`: staging,
  kept `data.db`, migrations, health check, atomic switch, revert). The
  archive's document id names the installation directory, so it must be a
  safe id. The manifest is stored next to the installation for attribution.
- **Role.** The manifest's role becomes the only runtime role. A manifest
  with `owner: true` (the Developer/Owner) gives developer access; any other
  manifest without a role allows nothing.
- **Credentials.** `key-grant` returns a DEK and the envelope; Rust opens it
  (XChaCha20-Poly1305) and keeps the password, and a per-user envelope's
  database username, in memory only, for at most 24 hours. Revocation,
  sign-out, closing the session and closing the app clear it. A grant is
  keyed by the cloud session's window and the datasource target
  (`cloud/grants.rs`), and only a datasource whose `grantScope` names that
  window uses it (`DatasourceConfig.grant_scope`, set by
  `open_runtime_session` for a cloud installation). Studio, other windows,
  and manual installs of the same app keep their own identity, so a per-user
  grant cannot change Studio's database user, and `cloud_upload_credential`
  without a password seals only the developer's stored password. A cloud
  session never reads a login entered for a manual install, so a revoked
  user gets no credential.
- **Offline.** An installed app opens without the cloud. `sync-check`
  failures that mean "cannot reach the cloud" let the installed version run;
  an expired grant means a PostgreSQL datasource stays detached until a new
  grant succeeds.
- **Replies are decoded.** `src/cloud/contract.ts` and
  `src-tauri/src/cloud/contract.rs` decode every reply the desktop reads; a
  missing field fails with `CLOUD_CONTRACT` instead of an empty value.

Evidence: `src-tauri/src/cloud/tests.rs` (manifest verification and
tampering, envelopes, a v2 per-user envelope through `open_credential`,
grants scoped to one cloud window, PKCE, config resolution, upload retry,
tampered `cloud.json`), `tests/integration/cloud-{install,auth,distribution}.test.tsx`,
`tests/unit/cloud-{contract,errors,rbac,recipient}.test.ts(x)` and
`web/e2e/service-qa/ui/*.spec.ts` (website pages).

## Contract

The table below is generated from the functions
(`node scripts/cloud/contract-doc.mjs`; `--check` fails when stale). Errors
use `{error:{code, message, details?}}`. Reply keys come from exchanges
recorded against the local stack by
`web/e2e/service-qa/specs/contract.spec.ts` into
`web/e2e/service-qa/fixtures/contract/*.json` (`CONTRACT_RECORD=1`
re-records; otherwise the spec fails when a live reply changes shape). The
fixtures feed `tests/unit/cloud-contract.test.ts` (desktop decoders),
`src-tauri/src/cloud/contract.rs` tests (Rust replies) and
`web/e2e/service-qa/contract/website-types.ts` (compile-time check of the
website's `FunctionMap`).

<!-- contract:start (generated by scripts/cloud/contract-doc.mjs) -->

| Function | Request → reply (from the function's header) | Error codes (direct and via shared helpers) | Rate limit | Recorded reply keys |
|---|---|---|---|---|
| `account-delete` | account-delete {confirmEmail, cancelSubscriptions?} → {} | UNAUTHENTICATED, FORBIDDEN, VALIDATION, RATE_LIMITED | 5 / 1 h |  |
| `account-export` | account-export {} → {export, archives: string[]} | UNAUTHENTICATED, RATE_LIMITED | 5 / 1 h |  |
| `admin-support` | admin-support {query:{email?\|appId?}} → {diagnostics} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VERSION_CONFLICT, ENTITLEMENT_REQUIRED, VALIDATION, RATE_LIMITED | 120 / 1 h |  |
| `app-access-update` | POST {appId, userId, role: "admin"\|"billing"\|"viewer"\|null} → {collaborator \| null} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 120 / 1 h |  |
| `apps-create` | POST {orgId, name, documentId, datasourceKind?:"sqlite"\|"postgres"} → {app} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 20 / 1 h | `app` |
| `apps-delete` | POST {appId, confirm:<app name>, cancelSubscription?} → {appId, deletedAt, subscriptionStatus, subscriptionCanceled} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 10 / 1 h | `appId`, `deletedAt`, `subscriptionStatus`, `subscriptionCanceled` |
| `apps-transfer` | POST {appId, newOwnerId, confirm:<app name>} → {app} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VERSION_CONFLICT, VALIDATION, RATE_LIMITED | 10 / 1 h |  |
| `archive-upload-url` | POST {appId, kind, size, sha256, installationId?} → {uploadId, path, signedUrl, token, expiresAt} | UNAUTHENTICATED, FORBIDDEN, REVOKED, NOT_FOUND, TOO_LARGE, ENTITLEMENT_REQUIRED, VALIDATION, RATE_LIMITED | 60 / 1 h | `uploadId`, `path`, `signedUrl`, `token`, `expiresAt` |
| `backup-commit` | POST {appId, uploadId, installationId} → {backup} | UNAUTHENTICATED, FORBIDDEN, REVOKED, NOT_FOUND, ENTITLEMENT_REQUIRED, VALIDATION, RATE_LIMITED | 60 / 1 h | `backup` |
| `billing-cancel` | billing-cancel {appId, atPeriodEnd?=true} → {subscription} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 10 / 10 min | `subscription` |
| `billing-checkout` | billing-checkout {appId, planId, interval?: "month"\|"year"} → {url, overAllowance} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 10 / 10 min | `url`, `overAllowance` |
| `billing-fake-complete` | billing-fake-complete {sessionId, appId, planId} → {ok, status} | UNAUTHENTICATED, NOT_FOUND, VALIDATION, RATE_LIMITED | 10 / 10 min | `ok`, `status` |
| `billing-invoices` | billing-invoices {appId} → {invoices: Invoice[]} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 60 / 10 min | `invoices` |
| `billing-portal` | billing-portal {appId} → {url} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 20 / 10 min | `url` |
| `bundle-manifest` | POST {appId, installationId, deviceName?} → {manifest, signature, archiveUrl, archiveUrlExpiresAt} | UNAUTHENTICATED, FORBIDDEN, REVOKED, NOT_FOUND, ENTITLEMENT_REQUIRED, VALIDATION, RATE_LIMITED | 60 / 1 h | `manifest`, `signature`, `archiveUrl`, `archiveUrlExpiresAt` |
| `credential-delete` | POST {appId, datasourceId, scope?:"shared"\|"user", userId?} scope omitted: every envelope of the datasource. → {revokedEnvelopeIds, revokedGrants} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 60 / 1 h | `revokedEnvelopeIds`, `revokedGrants` |
| `credential-envelope` | POST {appId, datasourceId, scope:"shared"\|"user", userId?, ciphertext, nonce, aad, dek} → {envelopeId, replacedEnvelopeId, kekVersion} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VERSION_CONFLICT, ENTITLEMENT_REQUIRED, VALIDATION, RATE_LIMITED | 60 / 1 h | `envelopeId`, `replacedEnvelopeId`, `kekVersion` |
| `desktop-auth-approve` | POST {codeChallenge, state} → {ok:true, expiresAt} | UNAUTHENTICATED, VALIDATION, RATE_LIMITED | 20 / 10 min | `ok`, `expiresAt` |
| `desktop-auth-exchange` | POST {state, codeVerifier} → {session:{access_token, refresh_token, expires_at, expires_in, token_type, user:{id, email}}} | FORBIDDEN, NOT_FOUND, VALIDATION, PENDING, RATE_LIMITED, INTERNAL | 120 / 1 min | `session` |
| `devices-revoke` | POST {appId, userId, installationId} → {installation:{id, revokedAt}, revokedGrants, alreadyRevoked} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 120 / 1 h | `installation`, `revokedGrants`, `alreadyRevoked` |
| `health` | GET → {ok, db, storage, version} (503 with ok false when a dependency is down) |  | none | `ok`, `db`, `storage`, `version` |
| `invitations-accept` | POST {token} → {membership:{kind, invitationId, orgId, appId?, userId, roleId?\|role, status?}} | UNAUTHENTICATED, FORBIDDEN, VALIDATION, RATE_LIMITED, INTERNAL | 20 / 10 min | `membership` |
| `invitations-create` | POST {kind:"app", appId, email, roleId} \| {kind:"org", orgId, email, role} → {invitation, acceptUrl, delivery:"sent"} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 30 / 1 h | `invitation`, `acceptUrl`, `delivery` |
| `key-grant` | POST {appId, installationId, datasourceId} → {grantId, datasourceId, dek, envelope:{id, scope, ciphertext, nonce, aad}, issuedAt, expiresAt, renewed} | UNAUTHENTICATED, REVOKED, NOT_FOUND, ENTITLEMENT_REQUIRED, VALIDATION, RATE_LIMITED | 30 / 1 h | `grantId`, `datasourceId`, `dek`, `envelope`, `issuedAt`, `expiresAt`, `renewed` |
| `members-update` | POST {appId, userId, roleId?, status?:"active"\|"revoked"} → {member} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 120 / 1 h | `member` |
| `publish-checkpoint` | POST {appId, uploadId, version, releaseNotes, minRuntimeVersion, migrations, security, expectedHeadVersionId} → {version} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VERSION_CONFLICT, TOO_LARGE, ENTITLEMENT_REQUIRED, VALIDATION, RATE_LIMITED | 30 / 1 h | `version` |
| `restore-url` | POST {appId, versionId} \| {appId, backupId} → {signedUrl, sha256, size, isPostgres, warning, kind, id, expiresAt} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 60 / 1 h | `signedUrl`, `sha256`, `size`, `isPostgres`, `warning`, `kind`, `id`, `expiresAt` |
| `retention-sweep` | POST {appId?} → {deleted, versions, backups, uploads, desktopAuthRequests} | UNAUTHENTICATED, FORBIDDEN, VALIDATION | none |  |
| `roles-sync` | POST {appId, roles:[{id, name, permissions}]} → {roles, kept} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VALIDATION, RATE_LIMITED | 60 / 1 h | `roles`, `kept` |
| `stripe-webhook` | POST <Stripe event> (Stripe-Signature header) → {received, outcome} \| {received, duplicate} | FORBIDDEN, NOT_FOUND, VERSION_CONFLICT, ENTITLEMENT_REQUIRED, VALIDATION | none |  |
| `sync-check` | POST {appId, installedVersionId, installationId} → {upToDate, latest:{versionId, version, publishedAt, minRuntimeVersion} \| null} | UNAUTHENTICATED, FORBIDDEN, REVOKED, NOT_FOUND, VALIDATION, RATE_LIMITED | 240 / 1 h | `upToDate`, `latest` |
| `versions-resolve` | POST {appId, action:"withdraw", versionId, confirm?} → {version, headVersionId, dependentInstallations}; POST {appId, action:"overwrite", fromVersionId, uploadId, version, releaseNotes, minRuntimeVersion, migrations, security} → {version}; POST {appId, action:"fork", fromVersionId?, uploadId?, version?, …, name?, documentId?} → {app, version} | UNAUTHENTICATED, FORBIDDEN, NOT_FOUND, VERSION_CONFLICT, TOO_LARGE, ENTITLEMENT_REQUIRED, VALIDATION, RATE_LIMITED | 20 / 1 h | `app`, `version`, `headVersionId`, `dependentInstallations` |

<!-- contract:end -->

## Audit log

- 2026-10-05: Status now covers the implemented functions and desktop client; added the later migrations, `billing_checkout_sessions`, missing shared modules, `retention-sweep` in the `verify_jwt = false` list and the client org writes; corrected member-activation entitlement, upload retry count, manifest fields (dropped the missing PLAN reference), build-config overrides and the release key gate; added billing and desktop Evidence.
- 2026-10-10: Added per-app console access (`app_collaborators`, `app_capabilities`, `app-access-update`), annual billing intervals, org-level billing customers and the no-trial checkout (PRD §4.5, §20.3).
