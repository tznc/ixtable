# Release checklist

PRD §6.1 makes Windows, macOS and Linux all release-blocking. PRD Phase 5 asks
for signed installers on every platform. In-app update channels are deferred
past the MVP: users download new versions from GitHub Releases. A release ships
only when every gate below is checked for all three platforms. The map of
PRD gates to automated checks and human sign-offs is
[../release-checklist.md](../release-checklist.md)
(`node scripts/ci/release-gates.mjs`).

## 1. Prepare

- [ ] The version is bumped in `src-tauri/tauri.conf.json`, `package.json`
      and `src-tauri/Cargo.toml` (the `plan` job fails when they differ).
- [ ] The tag matches: `vX.Y.Z` for stable, `vX.Y.Z-beta.N` for beta.
- [ ] `Desktop` CI is green on the tagged commit. It includes the archive
      upgrade fixtures (`tests/fixtures/archives/`) and the forced-termination
      autosave test (`src-tauri/src/durability_tests/`) on all three OSes.
      Each golden job uploads its journey evidence as `golden-evidence-<os>`.
- [ ] `Cloud contracts` CI (`.github/workflows/cloud.yml`) is green for the
      last change to `supabase/**` or the cloud clients.
- [ ] `CI` (`.github/workflows/ci.yml`: website typecheck and Playwright
      end-to-end tests) is green for the last change to `web/**` or
      `supabase/**`. Its E2E job starts the stack with
      `node scripts/cloud/up.mjs`, like `cloud.yml`. A bare `supabase start`
      boots the Edge Functions without their npm dependencies.
- [ ] `Deploy Web` (`.github/workflows/deploy-web.yml`) published the
      website and its Lighthouse preview report. The Lighthouse step runs a
      pinned `lighthouse@13.5.0` on Node 24 against the runner's Chrome and
      retries the launch up to three times, because Chrome sometimes fails to
      start on a cold runner. A Cloudflare API error on the deploy step is an
      account or token problem, not a code failure.
- [ ] If this release bumps the archive `FORMAT_VERSION`, a new
      `tests/fixtures/archives/format-<N>/` is committed
      (`node scripts/ci/write-archive-fixtures.mjs`) and no older fixture
      directory changed.
- [ ] The 500 MB boundary test passed on all three OSes: the `test` job's
      "500 MB archive boundary" step (locally,
      `IXTABLE_HEAVY_TESTS=1 cargo test --lib durability_tests::heavy` in
      `src-tauri`). PostgreSQL conformance passed on all three (`postgres`
      and `postgres-native`).
- [ ] Release notes are written in the draft release body before step 4.

## 2. Build (automatic: `.github/workflows/release.yml`)

Pushing the tag runs these gates. Any failure stops the release.

| Gate | Windows | macOS | Linux |
|---|---|---|---|
| Lint | | | `gates` |
| Rust tests (`cargo test --lib`) | `gates` | `gates` | `gates` |
| Golden application suites (`ui::golden`) | `gates` | `gates` | `gates` |
| DuckDB sqlite + postgres scanners fetched and hash-checked | `build` | `build` (arm64 + x64) | `build` |
| Automated release gates (`scripts/ci/release-gates.mjs`) | | | `plan` |
| Signing secrets present (beta/stable fail closed) | `build` | `build` | `build` |
| Production cloud public key set, not a dev/test key | `build` | `build` | `build` |
| Binary pins the release cloud key | `build` | `build` | `build` |
| Installers signed | Authenticode `Valid` on exe/msi | `codesign --verify --deep --strict`, `spctl` | — |
| Notarized and stapled | — | `stapler validate` on each dmg | — |
| Universal binary | — | `lipo` shows arm64 + x86_64 | — |
| Smoke launch (app stays up 20 s with a fresh state dir) | `build` | `build` | `build` (xvfb) |

## 3. Manual sign-off (before publishing)

Download the installers from the draft release, then on each OS:

- [ ] **Windows:** run the NSIS installer on a clean VM. SmartScreen shows the
      ixtable publisher. Open a golden `.ixt`, then a `.ixtr` bundle.
- [ ] **macOS (Apple silicon and Intel):** open the dmg with no Gatekeeper
      warning. Open a golden `.ixt`. Connect a PostgreSQL datasource, which
      loads the postgres scanner under the hardened runtime.
- [ ] **Linux:** install the `.deb` (Ubuntu) and the `.rpm` (Fedora), and run
      the AppImage. Opening `.ixt` from the file manager uses ixtable.

## 4. Publish

- [ ] Publish the GitHub draft release. Mark a beta as a prerelease. Publish a
      stable release as a full release.
- [ ] Check the published release: the version is right and the installers
      for all three OSes are attached.

## Rollback

To pull a bad release, unpublish it on GitHub, or mark it as a prerelease so
it is no longer the latest. Then ship a fixed `X.Y.Z+1`.
