# ixtable

ixtable is a desktop app for building small business applications, in the
spirit of Microsoft Access. One `.ixt` file holds the application's tables,
records, queries, forms, reports, dashboards, actions, and assets. Studio
designs the application and the local Runtime runs it, on Windows, macOS, and
Linux, with no account. Records live in an embedded SQLite file or in a
PostgreSQL database you provide. Every read goes through DuckDB. The product
requirements are in [`PRD.md`](./PRD.md), and the architecture decisions are
in [`docs/decisions/`](./docs/decisions/README.md).

## Prerequisites

- Rust stable, through [rustup](https://rustup.rs)
- Node.js 20 or newer, for the helper scripts under `scripts/` (the desktop
  app has no JavaScript dependencies)
- The system libraries GPUI and Tauri need for your OS. On Debian or Ubuntu:

  ```bash
  sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev librsvg2-dev libssl-dev pkg-config \
    libxkbcommon-dev libxkbcommon-x11-dev libxcb1-dev libfontconfig1-dev \
    libfreetype-dev libwayland-dev libvulkan-dev mesa-vulkan-drivers
  ```

- `bash`, `curl`, and `gzip` for the DuckDB extension script. On Windows, use
  Git Bash.

## Setup

```bash
bash scripts/prepare-duckdb-artifacts.sh linux-x64   # or macos-universal, windows-x64
```

The script downloads the official prebuilt libduckdb (the app links it
instead of compiling DuckDB) and the DuckDB `sqlite_scanner` and
`postgres_scanner` extensions that match the pinned DuckDB version. It checks each file against
a SHA-256 in the script and writes them to
`src-tauri/resources/duckdb/lib/` (the library) and
`src-tauri/resources/duckdb/<platform>/` (the extensions), both compressed (what the app
bundles) and unpacked (what dev builds load). The app checks the hash again
before loading an extension and refuses to start reads on a mismatch. The
files are gitignored, so run the script once per checkout. Builds fail until
it has run, because `tauri.conf.json` bundles the compressed files.

## Develop

```bash
node scripts/tauri.mjs dev     # the desktop window
node scripts/tauri.mjs build   # release build and installers
```

The script runs a pinned Tauri CLI through `npx`. `cargo tauri` also works if
you have it installed.

## Test

| Command | What it runs |
|---|---|
| `cd src-tauri && cargo test --lib [filter]` | every Rust test: unit tests, the RecordStore conformance suite, headless UI tests, and the golden suites |
| `cd src-tauri && cargo check --lib --tests` | compile check |
| `cd src-tauri && cargo screenshot [flow]` | renders real UI flows headlessly and captures PNGs under `scripts/screenshot/.generated/` |
| `npx --yes --package @ast-grep/cli@0.38.7 ast-grep scan --warning` | ast-grep lint rules |
| `node scripts/ci/unused-pub-rust.mjs` | finds unused `pub` functions |
| `node --test "scripts/tests/*.test.mjs"` | release-script tests |
| `node scripts/ci/release-gates.mjs` | release gates |

UI tests are `#[gpui_kit::test]` functions that drive the real GPUI app
through `ui::testing::Harness` with no window and no GPU. The golden suites
live in `ui::golden`.

PostgreSQL, heavy, performance, and live-cloud tests are `#[ignore]`d. The
PostgreSQL tests need a server. Any empty database works, because each run
creates and drops its own schema:

```bash
docker run --rm -d -p 5432:5432 -e POSTGRES_PASSWORD=ixtable postgres:16
export IXTABLE_TEST_POSTGRES_URL=postgresql://postgres:ixtable@localhost:5432/postgres?sslmode=disable
(cd src-tauri && cargo test --lib -- --include-ignored recordstore)
```

Screenshots need a Vulkan driver on Linux. Mesa's lavapipe works without a
GPU (`mesa-vulkan-drivers`).

## Architecture

The desktop app is a Tauri 2 shell with a native Rust UI. GPUI (through
`tauri-plugin-gpui` and `gpui-kit`) renders Studio and the Runtime from
`src-tauri/src/ui/`, and Tauri hosts no webview. The UI calls the backend
modules in `src-tauri/src/` directly. See
[`docs/decisions/gpui-ui.md`](./docs/decisions/gpui-ui.md).

### Modes

Studio has one mode per area. The registry is `src-tauri/src/ui/shell/modes.rs`.

| Mode | Directory | Purpose |
|---|---|---|
| Data | `ui/data/`, `ui/schema/` | tables, schema designer, relationships, record grid |
| Query | `ui/query/` | SQL and visual query builder, parameters, saved queries |
| Design | placeholder; `ui/grid/` holds the shared grid | form designer on the shared grid |
| Reports | `ui/reports/` | freeform report designer, pagination, print and PDF |
| Dashboards | placeholder | dashboards on the shared grid |
| Automation | `ui/automation/` | actions, record triggers, background jobs |
| Settings | `ui/shell/settings.rs` | assets, release, datasource, entities, migrations, roles, YAML, problems, logs |
| Runtime | `ui/runtime/`, `ui/release/` | runs the application, opens runtime-only bundles |

Shared code, all under `src-tauri/src/`:

| Path | Contents |
|---|---|
| `ui/{prelude,widgets,events,dialogs}.rs` | common imports, widgets, change notices, native dialogs |
| `ui/store.rs` | `ConfigStore`: every definition edit, with undo and redo |
| `records.rs` | `insert`, `update`, `delete`, `write_batch`, `run_action_query`: the only record write path |
| `expr/` | the expression language |
| `forms/`, `report_engine/`, `automation_runner/` | form state, report layout, and the automation runner |
| `ui/autosave.rs`, `ui/persistence/` | autosave, recovery, checkpoints UI |

### Rust modules

| Module | Responsibility |
|---|---|
| `manager` | open sessions, save, autosave, the per-session DuckDB reader |
| `archive`, `archive_io` | `DocumentConfig` and the `.ixt` SQLite archive format |
| `recovery`, `checkpoints`, `storage` | crash recovery, local checkpoints, recent files |
| `assets`, `logging` | application assets, redacted diagnostic logs |
| `data` | DuckDB `ReadRuntime`, logical types, DDL |
| `recordstore`, `postgres`, `migrations` | writes to SQLite or PostgreSQL, capabilities, migrations |
| `queries` | parameterized saved queries over DuckDB |
| `design`, `reports`, `dashboards`, `automation`, `roles` | definition types and validation per feature |
| `jobs` | durable queue for async triggers |
| `bundle`, `bundle_export`, `installation*` | signed runtime-only bundles and Runtime installations |
| `validation` | `validate_document`, which aggregates every module's checks |

## Rules for changes

- Write records only through `crate::records`. Triggers and the Runtime cache
  hook into every write there. Writes block, so call them inside
  `ui::task::spawn_call`.
- Change definitions only through `ConfigStore::update(label, mutate, cx)`.
  Anything else bypasses undo.
- Read through DuckDB. Never read records through a RecordStore connection.
- Give every definition object a stable uuid. Names are labels, not identity.
- There are no Tauri commands. UI code calls backend `pub fn`s directly.
- Add tests for every change: Rust unit tests in the module and
  `#[gpui_kit::test]` UI tests for UI flows.
- Record architecture decisions in `docs/decisions/`.

## Continuous integration

`.github/workflows/desktop.yml` runs on pushes to `main` and on pull requests
that touch the desktop app:

- `lint`: ast-grep rules, unused `pub` functions, release-script tests, and
  release gates
- `test`: `cargo test --lib` on Ubuntu, macOS (arm64), and Windows
- `postgres`: the RecordStore conformance suite and PostgreSQL datasource
  tests against a `postgres:16` service on Ubuntu
- `golden`: the golden application suites in `ui::golden` on all three OSes
- `build`: a debug `tauri build` on Ubuntu that proves the binary links

Failed jobs upload test reports and screenshot output as artifacts.

## Website

`web/` is the Docusaurus site with the product docs and the future ixtable
Cloud account pages. `supabase/` holds the local Supabase stack those pages
use. Neither is part of the desktop app.

```bash
cd web && npm ci && npm start    # docs site on :3001
supabase start                   # from the repo root: local Postgres, Auth, Mailpit
```

`.github/workflows/ci.yml` and `deploy-web.yml` test and deploy the site.
Use `.claude/skills/writing/SKILL.md` for docs, and check a page with
`python3 .claude/skills/writing/scripts/readability.py <file>`.
