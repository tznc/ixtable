# ixtable Cloud security model

Status: accepted and implemented as the design baseline; external security
review pending. The envelope-encryption design and the rest of this model
must pass an independent security review before public launch (PRD §21.3,
Phase 4 exit criteria). Covers PRD §20.2, §21, §23, §25, §27.2. Architecture:
[cloud-architecture.md](./cloud-architecture.md).

## Context

ixtable distributes desktop applications to trusted Runtime Users. The
Runtime runs on machines the developer does not control, and with PostgreSQL
it talks to the developer's database directly. The cloud therefore cannot
promise secrecy against an authorized user. It can promise that unauthorized
people get nothing, that authorized access is attributable, and that revoked
users cannot obtain new bundles or keys.

## Threat model

| Actor | Can | Cannot (by design) |
|---|---|---|
| Anonymous internet | call `health`, `stripe-webhook` (signature required), `desktop-auth-exchange` (needs the PKCE verifier), `retention-sweep` (needs the service role key or `CRON_SECRET`) | read any table or storage object, discover apps |
| Signed-in user with no membership | read own profile and the plan catalog | read or infer other orgs, apps, versions, members, envelopes, grants, audit, billing |
| Runtime User | read app basics, own role, own membership, own installations and backups, published versions; request bundles and key grants while active and entitled | read other users, envelopes, grants, subscriptions, audit; change any cloud row directly |
| Malicious Runtime User (authorized) | copy the archive, read displayed data, extract the decrypted datasource credential from process memory, keep using a credential they already obtained | evade fingerprint attribution in bundles they downloaded; obtain new grants after revocation |
| App owner / org admin | manage their apps, members, invitations, settings; read audit for their apps | read envelope ciphertext or wrapped DEKs, forge audit rows, edit published versions |
| Per-app admin / org billing / per-app billing / per-app viewer | act within their `app_capabilities` on the granted app only (admin: manage; billing: plan; viewer: read) | see other apps in the org, publish, manage credentials, grant access (org owners/admins only) |
| Operator (`profiles.is_operator`) | diagnose through `admin-support`, which returns no secrets | read secrets through PostgREST (operators get no extra RLS) |
| Compromised website session | act as that user within the rules above | escalate beyond the user's RLS scope |

### Trusted-user limits (PRD §20.2, §21.2, §21.3)

- Runtime RBAC protects navigation, queries, forms, reports, dashboards
  and actions in the official Runtime. Rust enforces it at every command
  entry point that
  reads or writes records (`src-tauri/src/authz.rs`): row and batch writes,
  table pages, saved queries, report PDF export and attachment export. A
  role gets `FORBIDDEN` for anything its permissions (or the forms,
  reports and dashboards it may open) do not grant. No role may run ad hoc
  SQL or export attachments. Read on a form, report or dashboard implies
  read on the saved queries it shows. For a dashboard that is the queries
  of its KPI, table and chart components and of its filter options, never
  those of an embedded form or report: an embedded form, report or button
  action still needs its own grant, and the form grant is what opens the
  form's queries. A `queryId` on any other component kind grants nothing.
  Table listing and schema inspection show only tables the role may read.
- Dashboard filters, columns, `visibleWhen` and fixed filter options are
  presentation, not row or column security. `run_saved_query` and
  `run_saved_query_page` take parameters, filters, sorts and limits from
  the caller, so a role that may read a query can read every row and
  column it returns. Row- and field-level rules are deferred (PRD §20.1).
  To limit what a role sees, grant it a narrower saved query.
  Checkpoints, restoring a checkpoint as a copy and resetting installation
  data copy or replace every table, so they need unrestricted access. The
  readable-table set is cached per session and role and recomputed after a
  config or data change.
- Record triggers run as the app by default (definer context, see
  [async-trigger-queue.md](./async-trigger-queue.md)): Rust lets a trigger
  step write without the role's grants only with a short-lived, single-use
  grant it issued for the initiating write (or the job's live lease), and
  only for the table and operation of a step the trigger declares. Values
  are computed by TypeScript expressions, so a modified client can write
  arbitrary values, but only into those declared tables and operations.
  Triggers set to run as the signed-in user keep the role's limits, and the
  initiating save is refused before it commits when the role could not run
  them. The role
  comes only from the signed manifest, re-verified against the pinned cloud
  key every time the installation opens or `cloud.json` is used: an edited
  record (for example `owner: true`) fails with `INSTALLATION_TAMPERED`.
  A cloud session whose role was never set is allowed nothing. It is not a
  defense against a user who extracts a
  valid PostgreSQL credential and connects directly. Strong isolation needs
  per-user, least-privileged database credentials and database permissions
  set by the developer. Per-user envelopes (`scope = 'user'`) exist for this;
  shared credentials require an acknowledged warning recorded in the version's
  `security` summary.
- Fingerprinting (HMAC over user, version, installation and issue time, in a
  signed manifest) gives attribution. It does not prevent copying.
- Revocation stops future bundles and key grants. It cannot erase a
  credential or archive a malicious user already obtained; the developer
  must rotate the database credential.
- Non-TLS PostgreSQL is allowed only after an explicit developer override,
  recorded in the version's security summary and shown before publishing.

## Decision

### Authorization

- RLS on every table, explicit grants only (see the architecture record).
  Security-definer helpers use `search_path = ''` and read only
  `auth.uid()`; the service-role-only functions (`audit`, `rate_limit`,
  `metric_increment`) have execute revoked from clients.
- Every privileged action is an Edge Function that checks, in order:
  authentication (`requireUser`), input (`validate.ts`), rate limit, role
  (owner/admin/member and status), installation not revoked, app not deleted,
  entitlement (`requireEntitlement`), then acts and writes an audit event.
- `audit_events` is append-only for every role, including the service role.

### Bundle signing

- Ed25519 via WebCrypto. The private key is the function secret
  `IXTABLE_CLOUD_SIGNING_KEY` (PKCS8 DER, base64). The desktop pins the
  public key at build time (raw 32 bytes = last 32 bytes of the SPKI DER),
  from `IXTABLE_CLOUD_PUBLIC_KEY_RAW` or `IXTABLE_CLOUD_PUBLIC_KEY` (SPKI
  base64) in the build environment. Only debug builds accept a runtime
  override from those environment variables. Release builds take the key
  from the repository variable, and `release.yml` refuses beta/stable builds
  when it is missing or equals a known dev/test key
  (`scripts/release/keys.mjs`).
- The manifest is serialized with `canonicalJson` (sorted keys, no
  whitespace) and the signature covers exactly those UTF-8 bytes. The
  manifest string is transmitted as is and the verifier checks those bytes;
  Rust also accepts the canonical re-serialization of the same JSON value,
  which is how a stored `cloud.json` record is re-checked. The Runtime verifies signature, expiry, and the archive
  sha256 before using any byte, and fails closed.
- A build pins exactly one key. Rotation (`docs/ops/production-config.md`):
  ship a desktop release that pins the new public key, then switch the
  secret. Desktops that still pin the old key refuse new bundles until they
  update, and installation records signed by the old key fail re-verification
  under the new build. A leaked signing key lets an attacker forge manifests
  for builds that pin it; rotation requires a desktop update.

### Credential envelopes

- Studio encrypts the datasource secret with a random 256-bit DEK
  (XChaCha20-Poly1305, Rust). The owner-only `credential-envelope` function
  checks the inputs (base64; 24-byte nonce, ciphertext of 17 bytes to
  64 KiB, aad up to 1 KiB, 32-byte DEK) and wraps the DEK with the KEK:
  AES-256-GCM, 96-bit random IV, AAD `appId|datasourceId|scope|userId`
  (`userId` empty for shared), stored as `base64(iv || ciphertext || tag)`
  plus `kek_version`. A row copied to another target fails to unwrap. The
  plaintext DEK is not stored or logged.
- A target (app, datasource, user or shared) has one active envelope
  (partial unique index). A new upload runs `credential_envelope_put`, which
  marks the old row `superseded_at`/`superseded_by` and erases its ciphertext,
  nonce, aad and wrapped DEK in the same transaction; a check constraint
  keeps retired rows empty. The metadata stays so key grants remain
  attributable to the credential they delivered.
- The envelope plaintext is `{v, kind, password, target}`. A per-user
  credential may also carry `user`, its own database username (`v: 2`). The
  username sits inside the ciphertext, so the cloud never sees it. Studio
  accepts a username only with `scope = user` and an explicit password.
  Runtime checks it like any login username and connects as it. `target`
  still names the datasource's configured user, so the binding to host,
  port, and database is unchanged. A runtime older than v2 ignores `user`
  and tries that password with the configured user, which the server
  refuses unless both roles share a password.
- Per-user envelopes (`scope = 'user'`) need the target to be an active
  member (or the owner). `key-grant` prefers the caller's per-user envelope
  over the shared one.
- `credential-delete` (owner) revokes the active envelopes of a datasource
  (or one scope/user), erases their secrets and revokes their live grants;
  later grants for it return `NOT_FOUND`.
- KEKs are function secrets `IXTABLE_KEK_V<n>` (32 random bytes, base64) with
  `IXTABLE_KEK_CURRENT_VERSION` for new wraps. Each row records its
  `kek_version`, so rotation adds a version and old envelopes still unwrap.
  No re-wrap job exists yet; old versions stay configured until every
  envelope that uses them is re-wrapped or replaced. The KEK never leaves
  the function runtime.
- Clients can read only envelope metadata, and only the app owner. Every
  client role gets `42501` selecting `ciphertext`, `nonce`, `aad`,
  `wrapped_dek` or `*`.

### Key grants

- `key-grant` checks, in order: session, input, rate limit (30 per hour per
  user and app, `429 RATE_LIMITED`), live app (`NOT_FOUND` when missing or
  deleted), owner or membership (`NOT_FOUND` without one, `REVOKED` when
  revoked), the caller's own installation (`NOT_FOUND`, or `REVOKED` when
  revoked), entitlement (`402 ENTITLEMENT_REQUIRED` with the reason), then the
  envelope (`NOT_FOUND`).
- It returns the unwrapped DEK and the envelope over TLS, records a
  `key_grants` row (`kind` issue or renew, `used_at` = delivery time,
  `expires_at` = issue + 24 h, `renewed_from`) and audits `key.issue`, or
  `key.renew` when a live grant already existed for the installation and
  datasource. Grant ids are single-use: a renewal is a new grant. The
  Runtime decrypts in memory and renews with a refreshed session.
- `devices-revoke` lets app admins revoke any installation and a Runtime User
  revoke their own. It sets `revoked_at`/`revoked_by`, revokes the
  installation's live grants and audits `credential.revoke`. Installation ids
  come from the desktop, so a revoked person could register a new one;
  revoking the membership (`members-update`) is what cuts a person off.
  `bundle-manifest` and `sync-check` refuse revoked installations with
  `403 REVOKED`.

### Desktop sign-in

Email/password runs in the desktop webview. OAuth uses a browser hand-off with
PKCE (S256):

1. The desktop opens `<site>/desktop-auth?code_challenge=…&state=…`.
2. The signed-in website calls `desktop-auth-approve` `{codeChallenge,
   state}`. This stores a `desktop_auth_requests` row bound to the user,
   approved now, expiring in 5 minutes (audit `auth.desktop_approve`).
   Approving the same state and challenge again is idempotent; any other
   reuse of a state is refused.
3. The desktop polls `desktop-auth-exchange` `{state, codeVerifier}` (no
   JWT). It gets `428 PENDING` until approval, then one session. The
   function checks `base64url(sha256(verifier)) == challenge` in constant
   time, consumes the row with a conditional update (single use), and mints
   the session without the user's password: Auth admin `generateLink`
   (magic link) followed by a server-side `verifyOtp` with the token hash.
   Errors: `404` with reason `consumed` or `expired`, `403` with reason
   `verifier_mismatch` (five failures invalidate the request), `429` (120
   requests per minute, counted per IP hash and per state). Without
   `IXTABLE_FINGERPRINT_SECRET` the function fails closed with `500`. Audit
   `auth.desktop_exchange`.

The supabase-js session, including the refresh token, is stored in the
local secret store (ChaCha20-Poly1305 under a key file in the state
directory) and bound to the cloud URL, never in `localStorage`, archives or
logs. Like any device-authorization flow, a user can be phished into
approving an attacker's request. The approval page
(`web/src/pages/desktop-auth.tsx`) names the account that will be signed in
and warns to approve only a sign-in the user just started on their own
computer.

### Secrets and logging

- Production secrets live in the Supabase secret store, never in the
  repository; `supabase/functions/.env.example` lists names only. Local
  values come from `scripts/cloud/dev-secrets.mjs` and are gitignored.
- Functions never return internal error text (`INTERNAL` only). Audit
  details pass through `redactSecrets`; client IPs are stored as keyed
  hashes. service-qa records and contract fixtures redact secrets and JWTs
  (`web/e2e/service-qa/record.ts`, `contract.ts`).
- Production guards: the fake billing provider runs only with
  `IXTABLE_ALLOW_FAKE_BILLING=1`, and with `IXTABLE_ENV=production`
  `invitations-accept` refuses while Auth email confirmations are off,
  because the invitation binds to the email address.

## Consequences

- An authorized but malicious user can always leak what they are allowed to
  see. The product and the commercial terms must say so.
- Losing a KEK makes its envelopes unrecoverable; developers must re-upload
  credentials. KEKs need backups in the secret manager.
- A 24-hour grant means revocation takes effect at the next renewal for
  credentials already in memory.
- Storage objects are reachable only through short-lived signed URLs, so a
  leaked URL is usable until it expires.

## Evidence

- `web/e2e/service-qa/specs/rls-private-by-default.spec.ts` (authorization,
  append-only audit, immutable versions, storage lockdown, privileged RPCs).
- `supabase/functions/_shared/crypto_test.ts` (Ed25519 with Node-generated
  keys, AES-GCM wrong key/AAD/tamper rejection, KEK versions, HMAC and PKCE
  vectors) and `billing_test.ts` (Stripe signature tolerance and secrets).
- `web/e2e/service-qa/specs/credentials.spec.ts` (owner-only upload,
  validation, supersede and erase, secrets unreadable for every role),
  `key-grant.spec.ts` (XChaCha20-Poly1305 round trip with the granted DEK,
  24-hour expiry, issue/renew, per-user preference, non-member, revoked
  member, deleted app, entitlement, rate limit), `revocation.spec.ts`
  (device revocation, self-service limits, credential deletion) and
  `desktop-auth.spec.ts` (pending, working session, single use, wrong
  verifier, expiry, approval rules).
- `supabase/functions/_shared/credentials_test.ts` (AAD binding, input
  checks, grant expiry, PKCE vectors) and `production_test.ts` (email
  confirmation guard).
- `web/e2e/service-qa/specs/bundle.spec.ts` (revoked members and
  installations get no bundle or sync), `account.spec.ts` and
  `admin.spec.ts` (exports and diagnostics carry no secrets).
- `src-tauri/src/cloud/tests.rs` (manifest tampering fails closed, tampered
  `cloud.json` grants no role, only debug builds take key overrides,
  credentials released only to their target until expiry),
  `tests/unit/cloud-rbac.test.ts` and
  `tests/integration/cloud-install.test.tsx`.

## Audit log

- 2026-10-05: Status notes implementation and keeps the external review pending; corrected the key override and single-key rotation, KEK re-wrap (no job yet), session storage (local secret store), exchange rate-limit subjects, attachment export, and the now-implemented revoked-installation check and approval-page warning; added production guards, `retention-sweep` and Evidence.
- 2026-10-10: Added per-app console roles to the actor table (PRD §20.3).
