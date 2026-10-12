# Desktop signing and webview CSP

## Status

Accepted. Covers PRD §6.1, §27.2 ("Signed bundles and updates must fail
closed") and the Phase 5 item "signed desktop installers for all platforms".
The release pipeline takes the production cloud public key from a CI variable
and refuses beta and stable builds with a missing or development key.
Generating the production key is a human gate (`key-ceremony` in
[the release gates](../release-checklist.md)) and is still pending.

On 2026-10-06, in-app updates were removed from the MVP. Users download new
versions from GitHub Releases. See the audit log.

## Context

ixtable ships on Windows, macOS and Linux, and every one blocks a release.
Installers must be signed so each OS trusts them. Cloud bundles must be
refused unless they carry a signature from the pinned cloud key. The webview
can call every Tauri command, so what it may load has to be limited too.

## Decision

- **No in-app updater in the MVP:** a release run ends as a draft GitHub
  release from tauri-action. A maintainer publishes it by hand after the
  release checklist. Beta is a GitHub prerelease, stable a full release.
- **Release key gate (fail closed):** `releaseKeyProblems` in
  `scripts/release/keys.mjs` runs in the `Signing setup` step. A beta or
  stable build stops before compiling when the cloud bundle-signing key is
  missing, malformed, or a known development or test key. Known keys are in
  `scripts/release/dev-cloud-keys.json`: the RFC 8032 test vectors, the
  all-zero key, and every Ed25519 test key committed to the repository (a
  unit test fails when one is missing). The local Supabase stack key is
  random per checkout (`dev-secrets.mjs`) and never shared, so the list does
  not depend on `.env.local`; when that file exists it is checked too. Draft
  runs only warn.
- **Fail closed in the build itself:** release.yml sets `IXTABLE_RELEASE=1`
  for beta and stable. With it, `src-tauri/build.rs`
  (`src/release_keys.rs`) fails the compile when the cloud key is missing,
  blank, malformed or in `dev-cloud-keys.json`. A local `tauri build`
  without the opt-in is a development build.
- **Binary check before upload:** tauri-action runs `tauri build` through
  `tauriScript: node scripts/release/keys.mjs`. The `build` wrapper checks
  that the binary contains the release cloud key, so a failure stops the job
  before tauri-action uploads to the draft release.
- **Installers:** `.github/workflows/release.yml` runs tauri-action with a
  universal macOS build (Developer ID, notarized), Windows signed with Azure
  Trusted Signing through `signCommand`, and Linux AppImage, deb and rpm.
  Beta and stable runs fail when any signing secret is missing.
- **CSP:** `script-src 'self'`, no eval, and IPC plus a single cloud origin in
  `connect-src`. Monaco is bundled instead of loaded from a CDN.

Details: `docs/release-checklist.md`, `docs/release/signing.md`,
`docs/release/security.md`, `docs/release/checklist.md`.

## Secrets and variables

Private keys live only in GitHub secrets and an offline backup. Public keys
are repository variables, because they are not secret and maintainers need to
read them.

| Name | Kind | Holds |
|---|---|---|
| `IXTABLE_CLOUD_PUBLIC_KEY_RAW` | variable | public half of the Edge Function secret `IXTABLE_CLOUD_SIGNING_KEY`, raw 32-byte Ed25519 key in base64 (`IXTABLE_CLOUD_PUBLIC_KEY`, SPKI DER base64, also works) |

Platform signing secrets (Apple, Azure) are listed in
`docs/release/signing.md`.

## Rotation

- **Cloud bundle-signing key:** set `IXTABLE_CLOUD_PUBLIC_KEY_RAW` to the new
  public key and ship a desktop release first. Then switch
  `IXTABLE_CLOUD_SIGNING_KEY` on the cloud (`docs/ops/production-config.md`).
  Desktops that have not installed the new release refuse new bundles until
  they do (fail closed).
- **Compromised key:** rotate as above at once, and record the date, key
  fingerprints and operators in the incident log (`docs/ops/incidents.md`).

## Consequences

- Users find and install new versions themselves. A fixed release reaches
  only the users who download it, so key rotations and urgent fixes need an
  announcement.
- The DuckDB extensions ship as `.duckdb_extension.gz` archives and are
  unpacked, hash-checked, at first use, so notarization never sees the
  ad-hoc-signed Mach-O files. `release.yml` fails if an uncompressed
  extension ends up in `ixtable.app`.
- `style-src` keeps `'unsafe-inline'` because React, React Flow and Monaco
  set inline styles.

## Evidence

- `tests/unit/release-scripts.test.ts`: release plan, signing-secret gates,
  and CSP rules.
- `tests/unit/release-keys.test.ts`: the release key gate (missing,
  malformed, and development keys in every encoding) and the embedded key
  check.
- `tests/unit/release-gates.test.ts`: `scripts/ci/release-gates.mjs` and its
  match with `docs/release-checklist.md`.

## Audit log

- 2026-10-05: production public keys now come from CI variables with a
  fail-closed key gate, probe and embedded check. Added secret names and
  rotation. The DuckDB notarization item was already fixed (compressed
  extensions), so it moved out of the pending list.
- 2026-10-06: in-app updates removed from the MVP. Users download new
  versions from GitHub Releases. Removed tauri-plugin-updater
  (`src-tauri/src/updater/`), Settings → Updates (`src/updates/`), the
  updater key and its probe, update manifests, and the
  `publish-update-manifest` job with its S3 host. The `updates-fail-closed`
  gate is gone. Releases end as a draft GitHub release that a maintainer
  publishes. Signing, the cloud key gate and the CSP are unchanged. Retitled
  from "Desktop updates, signing, and webview CSP"; the filename is kept.
- 2026-10-10: macOS notarization accepts an App Store Connect API key
  (`APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_PRIVATE_KEY`) as the
  preferred credential set, with the Apple ID set as the fallback
  (`NOTARIZATION` in `scripts/release/signing.mjs`). The key text is written
  to a file, never exported.
