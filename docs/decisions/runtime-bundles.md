# Runtime-only bundles: signing, password protection, and trust

Status: accepted for local manual distribution. Cloud personalized bundles and
envelope-encrypted credential delivery are a separate design, recorded in
[cloud security model](./cloud-security-model.md) and implemented in
`src-tauri/src/cloud/`; they reuse this record's installation flow but not the
`.ixtr` file format. Covers PRD §4.1, §21.2, §22.3, §27.2, and the Phase 0
runtime-bundle spike.

## Context

A developer can hand a recipient a locked, runtime-only copy of an
application. The recipient runs it without Studio, and later installs updates
by hand. The PRD requires signed bundles that fail closed, optional password
protection, no custom cryptography, and no claim of protection beyond what the
design gives. There is no cloud identity in this flow.

## Decision

### File format

A bundle (`.ixtr`) wraps a complete `.ixt` archive. All integers are
little-endian:

```text
"IXTRBNDL" | u16 format | u32 header_len | header JSON | u64 payload_len | payload | Ed25519 signature (64 bytes)
```

The header holds the bundle id (the document id), name, semver version,
release notes, minimum runtime version, creation time, signer public key,
payload SHA-256, and the encryption parameters when present. The signature
covers every byte before it.

### Primitives

| Purpose | Primitive | Crate |
|---|---|---|
| Signature | Ed25519, verified with `verify_strict` | `ed25519-dalek` 2 |
| Password key derivation | Argon2id, 19 MiB, 2 passes, 1 lane, random salt | `argon2` 0.5 |
| Payload encryption | XChaCha20-Poly1305, random 24-byte nonce, header bytes as associated data | `chacha20poly1305` 0.10 |
| Payload integrity | SHA-256 | `sha2` |

The Argon2id parameters are the OWASP baseline. They are stored in the header,
so a later build can raise them without breaking older bundles. The header is
untrusted until the password checks out, so the Runtime refuses parameters
above 256 MiB, 4 passes, or 4 lanes with `BUNDLE_INCOMPATIBLE`. It also refuses
bundle files larger than 600 MB (`BUNDLE_TOO_LARGE`) before reading them,
because verification happens in memory.

### Keys and trust

Each developer gets one Ed25519 signing key, created on first export at
`<state>/keys/runtime-bundle-signing.key` with mode `0600` on Unix. It is a
local key, not an identity. The Runtime pins the signer of the first bundle
it installs for a bundle id, in `installations/<bundleId>/bundle.json`. A
later bundle for the same id with any other key fails with
`BUNDLE_SIGNER_MISMATCH` and is not opened. This is trust on first use.

### Verification order

1. Check the magic, format version, and header size limit.
2. Verify the Ed25519 signature over the whole file. Any change fails here.
3. Check the encryption flags against the header.
4. Check the signer against the key pinned for this bundle id.
5. Check the semver version and `minRuntimeVersion` against this Runtime.
6. Decrypt when protected. A wrong password fails with `BUNDLE_PASSWORD`.
7. Check the payload SHA-256 before the archive is opened.

### Installations and updates

A Runtime installation lives in `<state>/installations/<bundleId>/`.
`active/app.ixt` holds the definition and `data.db` holds the recipient's
records, which the bundle seeds only on first open. An update stages the new
definition with a copy of the records. It applies pending migrations and
health checks to the copy, then swaps it in. The previous version is kept in
`previous/`, and any failure restores it. An older version never installs:
`installation::plan` refuses it with `BUNDLE_DOWNGRADE` ("Installed: 2.0.0.
This file: 1.0.0."), already
at inspect, so no confirm step opens. The installation's records may use a
schema that only the newer version understands, and there are no down
migrations. Manual and cloud installations follow the same rule. (Until
2026-10-14 a confirmed downgrade was allowed; Zn removed it.)

A manual update stops at a confirm step first (PRD §22.3 step 7).
It shows the release notes (line breaks kept) and the migrations that would
run on this installation's records, with their SQL.
`inspect_runtime_bundle` checks Runtime compatibility before reading the
archive, then fills `pendingMigrations` for an unencrypted bundle. The preview
is best effort: if it fails, inspect still succeeds with
`migrationsUnavailable` set to the reason, and the confirm step says so. A
protected bundle needs its password first, so the UI calls
`preview_runtime_update(path, password)` after the password prompt. Both read
`data.db` read-only: the ids in `_ixtable_migrations` are compared with the
incoming definition's supported migrations.

Inspect returns the file's `sha256`. The file flow passes it back as
`expectedSha256` (required by `update_runtime_installation`, optional for
`open_runtime_bundle`), and Rust refuses a file whose hash changed since
inspection (`BUNDLE_CHANGED`), so the applied bundle is the one confirmed.

`bundle.json` records what the last apply did: `lastAction` (`install`,
`open`, `update`; installations from before the change can still read
`downgrade`) and `appliedMigrations`, the names of the migrations that ran
(empty for a fresh install and for opening the installed version). Cloud
auto-sync shows the post-update notice only when `lastAction` is `update`.

Studio and installation updates share one migration preflight
(`migrations::preflight`). Before any SQL runs it rejects an applied migration
whose `up` SQL changed (checksum) and any `depends_on` that is unknown, unapplied,
or ordered after its dependent, so a bad update fails and activation is blocked.
The migration log records real start and finish times and the health-check
outcome; the preview splits statements with SQLite's own completeness rules.
After Studio applies or rolls back migrations it re-validates the document so
forms and queries bound to dropped columns surface as problems at once.

A `bundle.json` that exists but cannot be read or parsed fails with
`INSTALLATION_CORRUPT`. It is never treated as "not installed", because that
would drop the pinned signer. An installation folder with no `bundle.json` at
all is moved aside to `<bundleId>.corrupt-<timestamp>-<random>` before a
fresh install, never deleted.

Cloud installations (`src-tauri/src/cloud/install.rs`) go through the same
`installation::apply_bundle`, under `<state>/cloud-installations/<appId>/`.
Instead of a `.ixtr` file, the archive is authenticated by a manifest signed
with the pinned cloud key, and its SHA-256 must match the manifest. The
installation pins the cloud key and stores the verified manifest in
`cloud.json`.

### Runtime window

A runtime-only window shows the application, not Studio. Its title is the
application name. Its ribbon has only Records (Refresh) and the role; it has no
Studio commands (create, undo, redo), no mode switch,
no Save buttons, no `PROJECT /` breadcrumb, and no "Preview as role" switch.
A manual bundle runs with full access, because the document config has no
default runtime role. A cloud installation runs as the role in its signed
manifest and shows it as `Role: <name>`. The role lives in the Rust session
(`authz.rs`) and is checked at each record command, so a direct `invoke`
gets `FORBIDDEN` just like the UI. Studio's Run App window (see
`gpui-ui.md`) sets the same Rust role for the role it runs as, so a preview
behaves like the installed runtime. The role is enforced only while that
window is in front, and closing the window ends it. Runtime sessions keep the ids the
bundle was published with; only Studio rewrites the legacy `main` form id
(`design/upgrade.rs`, `rekey_legacy_ids`).

### Database login

A manual bundle on PostgreSQL carries no password, so the recipient enters a
database login. When the bundle opens, `DatabaseLogin` asks
`runtime_datasource_login_status`. It opens the "Database login" dialog when
no login is stored (`missing`) or the server refused the stored one
(`rejected`, from the reader's attach error). The username starts as the
datasource's user and may be changed. `set_runtime_datasource_login` checks
the username, requires a confirmed transport, and connects (`postgres::probe`)
before storing anything; a refusal comes back as `AUTH_FAILED` (SQLSTATE class
28) with a redacted message. A verified login goes to the local secret store
under `installation:<bundleId>:datasource:<datasourceId>`, sealed and bound to
`login::installation_target`: the datasource target plus its sslmode and
`insecureTransportConfirmed`. The reader then re-attaches. "Database login…"
in the runtime sidebar re-enters the login or forgets it
(`clear_runtime_datasource_login`).

The recipient owns this password, so the bundle author's transport choices
cannot decide alone where it is sent. Only `verify-full` checks the server's
certificate and name (`DatasourceConfig::verifies_server`); `require` and
`verify-ca` accept a server that anyone on the network can impersonate, and
the plaintext modes send the password in the clear. The dialog shows the
effective sslmode. For any mode but `verify-full` it shows a warning and a
checkbox, "I accept sending my password over a connection that does not verify
the server", and Connect stays disabled until the recipient ticks it. The
command refuses with `UNVERIFIED_TRANSPORT` before connecting unless
`acceptUnverified` is true (`login::ensure_recipient_transport`). The
author's `insecureTransportConfirmed` is still required for plaintext modes
but never stands in for the recipient's acceptance. The acceptance is stored
with the login (`unverified_accepted`), and because the sslmode and insecure
flag are part of the login's target, an update that changes the transport
reads the stored login as none and Runtime asks again.

We chose this over requiring `verify-full` for every Runtime login. Many
PostgreSQL servers on office networks run with self-signed certificates or
none, and refusing them would push developers to share one application
password instead, which is worse. The acceptance keeps those setups possible
while making the risk the recipient's informed choice. It also keeps the
PostgreSQL integration tests meaningful: CI servers run without TLS, and the
tests now tick the acceptance as a recipient would.

The installation id reaches the credential lookup through
`DatasourceConfig.installation`, a `serde(skip)` field set by
`open_runtime_session` only for a manual install, so Studio, other
installations, and cloud sessions never see the login. A cloud installation
reuses the document id as its bundle id, so it would otherwise share the
manual install's key; `open_runtime_session` recognizes a directory under the
cloud root (`cloud::install::is_cloud_dir`) and sets `grantScope` (the
window label) instead. `secrets::datasource_login` picks a login in this
order: a key grant held by the datasource's cloud session, the manual
installation's login, then the developer's `passwordRef`. Grants are consulted
only when `grantScope` is set (see `cloud-architecture.md`). An update that
moves the datasource to another host, port, database, or user, or changes its
transport, changes the target, so the stored login no longer matches and
Runtime asks again. Cloud installations get their login from key grants and
never show the dialog; a cloud user whose grant is revoked gets no login.

The runtime sidebar has a "Diagnostics…" button for the background job queue
(status, attempts, retry, cancel; the Studio `JobsPanel`) and the local log
(`LogsTab`). Neither changes the definition.

## Threat model limits

The release tab shows the first limit next to the password option, in the
words of PRD §4.1.

- A password protects the bundle at rest and against casual opening. Anyone
  with the password can open it, read every record it shows, and extract the
  archive.
- The password protects the distributed `.ixtr` file only. Once installed,
  `installations/<bundleId>/active/app.ixt` and `data.db` are plaintext at
  rest, protected only by the user account's file permissions. During
  install, the decrypted archive touches disk only as a `0600` scratch file
  that is deleted as soon as it has been read.
- Trust on first use does not protect the first install. A recipient must get
  the first bundle, or the signer fingerprint, from the developer over a
  channel they trust.
- A lost signing key cannot be rotated for existing installations. They must
  reinstall from a bundle signed by the new key.
- The signing key file is protected only by file permissions. Malware running
  as the developer can sign bundles.
- Bundles carry no PostgreSQL password. `DatasourceConfig` stores only
  `passwordRef`, and the secret stays in the developer's local secret store.
  Recipients enter their own login in Runtime. It is sealed in their local
  secret store under the machine key, which the OS account's file
  permissions protect. Anyone who can read that account's files can open it.
- A fork of the Apache-2.0 desktop code can remove any of these checks for
  its own users. The checks protect honest Runtimes from tampered files.

Export is also refused while any expression has an error
(`EXPRESSION_ERRORS`). Rust cannot evaluate expressions, so Studio runs that
check in TypeScript before calling `export_runtime_bundle`; see
[expression-language.md](expression-language.md).

## Consequences

- No account or network is needed to export, verify, or update a bundle.
- Verification fails closed before any archive byte is parsed.
- Cloud distribution needs per-user personalization, key grants, and
  revocation (PRD §21.3). Those are built on signed cloud manifests
  (`cloud/manifest.rs`, `cloud/envelope.rs`, `cloud/grants.rs`), not on new
  `.ixtr` header fields. The `.ixtr` format is unchanged at format version 1.

## Evidence

- `src-tauri/src/bundle/tests.rs`: signed round trip, password round trip and
  wrong password, tampering fails the signature, runtime version rules, and a
  persistent private signing key.
- `src-tauri/src/installation/tests.rs`: first open seeds data and pins the
  signer, updates keep installation records, an older version is refused, a
  different signer is rejected, migrations on update, a failing migration or
  health check reverts, restore of the previous version, reset, unsafe
  bundle ids, a corrupt `bundle.json` failing closed, and a folder without one
  moved aside.
- `src-tauri/src/recordstore/runtime_login_tests.rs`: logins sealed per
  installation and target, an unverified transport needs the recipient's
  acceptance, a transport downgrade asks again, a manual install never uses a
  cloud grant, the `missing`/`rejected`/lookup-error reasons, a moved
  datasource asks again, username validation, auth failures told apart from
  unreachable servers, and `AUTH_FAILED` from a real PostgreSQL server.
- `src-tauri/src/manager_config_tests.rs`: a cloud session of the same app
  ignores a manual installation's login.
- `tests/integration/runtime-database-login.test.tsx`: the prompt opens for a
  PostgreSQL bundle with no login and shows its sslmode; Connect stays
  disabled and the command refuses (`UNVERIFIED_TRANSPORT`) until the
  recipient accepts an unverified server; an unverifiable login is not stored; with
  `IXTABLE_TEST_POSTGRES_URL` a wrong password is refused, the right one
  connects and reads, and "Forget saved login" asks again.
- `src-tauri/src/installation_commands.rs` (unit test): oversized bundles fail
  with `BUNDLE_TOO_LARGE` before reading.
- `tests/integration/bundle.test.tsx`: export from Studio, open runtime-only
  without Studio chrome, update while keeping records, password prompt, and a tampered bundle
  rejected before opening.
- `tests/integration/runtime-update-diagnostics.test.tsx`: a protected
  update's release notes and migrations in the confirm step, then the runtime
  jobs and logs diagnostics.
- `tests/integration/cloud-install.test.tsx`: a signed cloud bundle installs
  through the same flow, tampering is rejected, and updates keep records.

## Audit log

- 2026-10-05: Status and consequences now point to the built cloud
  installation path (`src-tauri/src/cloud/`), which reuses `apply_bundle`.
  Added the cloud installation paragraph, the moved-aside folder suffix, and
  missing evidence files.
