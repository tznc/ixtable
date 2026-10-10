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

- Node.js 20 or newer, with npm
- Rust stable, through [rustup](https://rustup.rs)
- The [Tauri 2 system dependencies](https://v2.tauri.app/start/prerequisites/)
  for your OS. On Debian or Ubuntu:

  ```bash
  sudo apt-get install libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
    libjavascriptcoregtk-4.1-dev librsvg2-dev libssl-dev pkg-config
  ```

- `bash`, `curl`, and `gzip` for the DuckDB extension script. On Windows, use
  Git Bash.

## Setup

```bash
npm ci
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
npm run tauri:dev     # Vite dev server on :1420 plus the desktop window
npm run tauri:build   # release build and installers
```

## Test

| Command | What it runs |
|---|---|
| `npm test` | builds the test bridge, then unit and integration tests |
| `npx vitest run tests/unit/<file>` | one unit test file (jsdom, no Rust) |
| `npm run pretest` | builds the NAPI test bridge (`scripts/ci/build-test-bridge.mjs`) |
| `npx vitest run --config vitest.integration.config.ts tests/integration/<file>` | one integration file against the real Rust commands |
| `cd src-tauri && cargo test --lib` | Rust unit tests and the RecordStore conformance suite |
| `npm run screenshot` | renders real UI flows and captures PNGs under `scripts/screenshot/.generated/` |
| `npm run lint` | oxlint, eslint, and ast-grep rules |
| `npx tsc --noEmit` | TypeScript type check |
| `npm run format:check` | biome formatting |

Integration tests render the real React UI in jsdom and send each Tauri
command through a native Node module built from `src-tauri` with the
`test-bridge` feature. Rebuild it with `npm run pretest` after any Rust
change. The bridge loads the library from `src-tauri/target/debug`, so keep
the default target directory.

The PostgreSQL tests skip unless you point them at a server. Any empty
database works, because each run creates and drops its own schema:

```bash
docker run --rm -d -p 5432:5432 -e POSTGRES_PASSWORD=ixtable postgres:16
export IXTABLE_TEST_POSTGRES_URL=postgresql://postgres:ixtable@localhost:5432/postgres?sslmode=disable
(cd src-tauri && cargo test --lib recordstore)
npx vitest run --config vitest.integration.config.ts tests/integration/datasource
```

Screenshots need Chromium: run `npx playwright install chromium` once.

## Architecture

The desktop app is a Tauri 2 shell. React and TypeScript in `src/` render
Studio and the Runtime. Rust in `src-tauri/src/` owns files, databases, and
cryptography, and the UI reaches it only through Tauri commands.

### Modes

Studio has one mode per area. The registry is `src/shell/modes.tsx`.

| Mode | Directory | Purpose |
|---|---|---|
| Data | `src/data/`, `src/schema/` | tables, schema designer, relationships, record grid |
| Query | `src/query/` | SQL and visual query builder, parameters, saved queries |
| Design | `src/design/` | form designer on the shared grid |
| Reports | `src/reports/` | freeform report designer, pagination, print and PDF |
| Dashboards | `src/dashboards/` | dashboards on the shared grid |
| Automation | `src/automation/` | actions, record triggers, background jobs |
| Settings | `src/shell/AppSettings.tsx` | assets, release, datasource, entities, migrations, roles, YAML, problems, logs |
| Runtime | `src/runtime/`, `src/release/` | runs the application, opens runtime-only bundles |

Shared frontend code:

| Path | Contents |
|---|---|
| `src/lib/api.ts` | `call()`, the only place that invokes Tauri commands |
| `src/lib/config-store.tsx` | `useDocumentConfig()`: every definition edit, with undo and redo |
| `src/lib/records.ts` | `insertRecord`, `updateRecord`, `deleteRecord`: the only record write path |
| `src/expr/` | the expression language ([reference](./src/expr/README.md)) |
| `src/grid/` | the grid engine and canvas shared by forms and dashboards |
| `src/persistence/` | autosave, recovery, checkpoints, assets, logs |

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

- Write records only through `src/lib/records.ts`. Triggers, autosave, and
  the Runtime cache hook into every write there.
- Change definitions only through `useDocumentConfig().update`. A direct
  command call bypasses undo and is overwritten by the next store update.
- Read through DuckDB. Never read records through a RecordStore connection.
- Call Tauri commands only through `call()` in `src/lib/api.ts`. A lint rule
  enforces it.
- Give every definition object a stable uuid. Names are labels, not identity.
- New Rust commands are `pub fn` with a literal `#[tauri::command]`, take
  `window_label: String` first, and are registered in `src-tauri/src/lib.rs`.
- Add tests for every change: Rust unit tests in the module, TypeScript unit
  tests under `tests/unit/`, and UI flows under `tests/integration/`.
- Record architecture decisions in `docs/decisions/`.

## Continuous integration

`.github/workflows/desktop.yml` runs on pushes to `main` and on pull requests
that touch the desktop app:

- `lint`: type check, lint, and a format check, which stays advisory until
  the tree is formatted
- `test`: Rust unit tests, unit tests, and integration tests on Ubuntu, macOS
  (arm64), and Windows
- `postgres`: the RecordStore conformance suite and PostgreSQL datasource
  tests against a `postgres:16` service on Ubuntu
- `golden`: the golden application suites under
  `tests/integration/golden/` on all three OSes
- `build`: a debug `tauri build` on Ubuntu that proves the binary links

Failed jobs upload JUnit reports and screenshot output as artifacts.

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
