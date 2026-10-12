# Signing and release secrets

`.github/workflows/release.yml` signs every platform. When a `beta` or
`stable` run is missing any secret below, `scripts/release/signing.mjs` fails
the build and names the missing secrets (never their values). A `draft` run
warns and builds whatever it can sign.

## macOS (Developer ID + notarization)

| Secret | Use |
|---|---|
| `APPLE_CERTIFICATE` | base64 of the Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | `.p12` password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: ixtable Ltd (TEAMID)` |

Notarization takes one of two credential sets. When any API key secret is
set, `signing.mjs` requires the API key set and ignores the Apple ID set.

| Secret | Use |
|---|---|
| `APPLE_API_ISSUER` | App Store Connect API issuer ID (preferred set) |
| `APPLE_API_KEY` | API key ID, e.g. `ABC123DEF4` |
| `APPLE_API_PRIVATE_KEY` | text of the `AuthKey_<id>.p8` file, Developer role |
| `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID` | fallback set: Apple ID, app-specific password, team |

The API key is not tied to a person's Apple ID and survives staff changes.
`signing.mjs` writes it to `$RUNNER_TEMP/AuthKey_<id>.p8` (mode 600) and
exports only `APPLE_API_KEY_PATH`, never the key text.

The Tauri CLI imports the certificate into a temporary keychain. It signs with
the hardened runtime (`bundle.macOS.hardenedRuntime`) and the entitlements in
`src-tauri/entitlements.plist`, then notarizes and staples. The build is
universal (`--target universal-apple-darwin`). CI then runs `codesign
--verify --deep --strict`, `spctl --assess`, `stapler validate`, and `lipo`.

The DuckDB extensions are not in the bundle as Mach-O files. Upstream ships
them with only ad-hoc (linker) signatures, which notarization rejects, and
re-signing them would break both their SHA-256 pins and DuckDB's own extension
signature. The bundle holds the official `.duckdb_extension.gz` archives
instead (`bundle.resources` in `tauri.conf.json`). The app checks each archive
against its pinned hash, unpacks it into the per-user state directory on first
use, and checks the pinned hash of the result before `LOAD` (see
[the DuckDB read path record](../decisions/duckdb-read-path.md)).
`disable-library-validation` in the entitlements lets the hardened runtime
load the unpacked files. The release workflow fails if an uncompressed
`*.duckdb_extension` ends up in `ixtable.app`.

## Windows (Azure Trusted Signing)

Since June 2023, publicly trusted code-signing keys must be kept in hardware,
so a `.pfx` in a GitHub secret is no longer an option. ixtable uses
[Azure Trusted Signing](https://learn.microsoft.com/azure/trusted-signing/)
through Tauri's `bundle.windows.signCommand`, which runs
`trusted-signing-cli` on every binary and installer:

| Secret | Use |
|---|---|
| `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET`, `AZURE_TENANT_ID` | service principal with the *Trusted Signing Certificate Profile Signer* role |
| `AZURE_SIGNING_ENDPOINT` | e.g. `https://eus.codesigning.azure.net` |
| `AZURE_SIGNING_ACCOUNT` | Trusted Signing account name |
| `AZURE_CERTIFICATE_PROFILE` | certificate profile name |

`signCommand` is set only in the release `--config` override, so local
Windows builds do not need Azure. `tauri.conf.json` sets SHA-256 digests and a
timestamp server. CI checks `Get-AuthenticodeSignature` on `ixtable.exe` and
every `.exe` and `.msi` it builds.

An EV or OV certificate on a hardware token or cloud HSM can be used instead:
set `bundle.windows.certificateThumbprint` on a self-hosted runner where the
certificate is installed, and change `REQUIRED.windows` in
`scripts/release/signing.mjs` to match.

## Linux

AppImage, `.deb` and `.rpm` are built on Ubuntu 22.04, which keeps the glibc
requirement low. Linux needs no signing secret.

## Publishing

A release run ends as a draft GitHub release. A maintainer publishes it by
hand after [the release checklist](./checklist.md). Beta releases are GitHub
prereleases. Stable releases are full releases. There is no in-app updater,
so users download new versions from GitHub Releases.

## ixtable Cloud build values

The repository variables `IXTABLE_CLOUD_BUILD_URL`,
`IXTABLE_CLOUD_BUILD_ANON_KEY`, `IXTABLE_CLOUD_BUILD_SITE_URL` and
`IXTABLE_CLOUD_PUBLIC_KEY_RAW` (the bundle-signing public key) are compiled
into release builds (`cloud/config.rs`). `IXTABLE_CLOUD_PUBLIC_KEY` (SPKI
DER base64) is the fallback when the RAW variable is unset or blank. Beta and
stable runs fail when the public key is missing or a development or test key,
both in `Signing setup` and in `build.rs` (`IXTABLE_RELEASE=1`). The
`keys.mjs build` wrapper then checks that the built binary pins it, before
tauri-action uploads anything. The URL's origin is also added to the
release CSP (see [security.md](./security.md)).

## Local builds

A local `tauri build` without `IXTABLE_RELEASE=1` is a development build: it
may pin no cloud key. Only release.yml sets the opt-in. A local
`npm run tauri build` needs no signing secret.
