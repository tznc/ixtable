# CLAUDE.md

ixtable is a Tauri 2 desktop app (Studio + local Runtime) for Access-style
business applications stored in one `.ixt` file. The whole app is Rust in
`src-tauri/src/`. The UI is native GPUI (tauri-plugin-gpui + gpui-kit) in
`src-tauri/src/ui/`, with no JavaScript frontend. Product spec: `PRD.md`.
Architecture decisions: `docs/decisions/` (read the relevant record before
changing a subsystem, and update it in the same change; start with
`gpui-ui.md` for UI work).

## Setup and commands

```bash
bash scripts/prepare-duckdb-artifacts.sh linux-x64   # pinned prebuilt libduckdb + extensions, once per checkout
node scripts/tauri.mjs dev                           # run the app (`build` for installers)
```

There is no root `package.json`; `scripts/*.mjs` run on plain Node.

| Task | Command |
|---|---|
| All Rust tests, including headless UI and golden tests | `cd src-tauri && cargo test --lib [filter]` |
| Compile check | `cd src-tauri && cargo check --lib --tests` |
| Screenshots | `cd src-tauri && cargo screenshot [flow]` (see `.claude/skills/app-qa/SKILL.md`) |
| Lint | `npx --yes --package @ast-grep/cli@0.38.7 ast-grep scan --warning` and `node scripts/ci/unused-pub-rust.mjs` |
| Release-script tests / gates | `node --test "scripts/tests/*.test.mjs"` / `node scripts/ci/release-gates.mjs` |
| Regenerate icons | `node scripts/release/icons.mjs generate` |

PostgreSQL, heavy, perf and live-cloud tests are `#[ignore]`d. Run them with
`cargo test --lib -- --include-ignored <filter>`. PostgreSQL tests need
`IXTABLE_TEST_POSTGRES_URL=postgresql://user:pass@host:5432/db?sslmode=disable`;
under `CI` a missing variable panics (`src-tauri/src/test_env.rs`), and CI runs
the ignored tests through `scripts/ci/run-ignored-rust-tests.mjs`, which
checks each one ran and passed. Cloud tests against a local Supabase stack:
`cargo test --lib -- --ignored cloud_live` with `SUPABASE_URL`,
`SUPABASE_ANON_KEY` and `IXTABLE_CLOUD_PUBLIC_KEY_RAW` set.
`IXTABLE_DUCKDB_SQLITE_EXTENSION` / `IXTABLE_DUCKDB_POSTGRES_EXTENSION`
override extension paths (hashes still checked). `IXTABLE_STATE_DIR` moves
the state directory (tests set it to a temp dir).

## How UI tests work

UI tests are `#[gpui_kit::test]` functions in a `tests.rs` beside the view.
They drive the real `AppView` headlessly through `ui::testing::Harness` over
a real session and a real backend; only native dialogs are faked
(`TestDialogs`). Find elements by their kebab-case element ids.

- Button `disabled` and `selected` states are not in snapshots, so assert the
  model state.
- A custom `div` that tests must find ends its builder chain with
  `.test_support()`.
- Test modules import names explicitly. Never glob-import `gpui_kit::*`; it
  shadows `#[test]`.

## Map

- UI: `src-tauri/src/ui/<feature>/` (data, query, schema, forms, reports,
  automation, settings, runtime and so on). Modes are registered in
  `ui/shell/modes.rs`, settings tabs in `ui/shell/settings.rs`.
- Shared UI: `ui/{prelude,widgets,store,events,dialogs,testing,screenshot}.rs`.
- Logic: `expr/` (expression language), `records.rs` (record write path and
  hooks), `forms/`, `report_engine/`, `automation_runner/`.
- Backend: `manager.rs` (sessions, save), `archive*.rs` (`.ixt` format),
  `data/` (DuckDB reader), `recordstore/` + `postgres/` (writes),
  `queries/`, `design/`, `reports.rs`, `dashboards/`, `automation.rs`,
  `jobs.rs` (async trigger queue), `bundle*.rs` + `installation*.rs`
  (runtime bundles), `validation.rs`.

## Rules

- Reads go through DuckDB (`ReadRuntime`). Writes go through a RecordStore,
  then `mark_data_dirty` refreshes the reader. Action queries are the one
  exception: `queries::action` runs them on a separate writable DuckDB
  connection (`docs/decisions/action-queries.md`).
- Record writes only via `crate::records` (`insert`, `update`, `delete`,
  `write_batch`, `run_action_query`) so hooks and triggers run. They block,
  so call them inside `ui::task::spawn_call`, then announce the change with
  `Shell::records_changed` (DDL: `Shell::database_changed`).
- Definition edits only via `ConfigStore::update(label, mutate, cx)`
  (`ui/store.rs`). Same-label edits within one second merge into one undo step.
- There is no `#[tauri::command]` layer. UI code calls backend `pub fn`s
  directly. Native dialogs go through `ui::dialogs`; UI files import
  `ui::prelude::*`.
- Every interactive element gets a stable kebab-case element id. Text that
  tests read goes through `widgets::text` or `widgets::styled_text`.
- Every definition object has a stable uuid id. Expressions are evaluated
  only by `src-tauri/src/expr`.
- Keep Rust files under 2000 non-comment lines (ast-grep warns).
  Single-line comments inside code blocks.
- Every change ships with tests: Rust unit tests and `#[gpui_kit::test]` UI
  tests for UI flows.

## CI

`.github/workflows/desktop.yml`: lint (ast-grep, unused pub fns, script
tests, release gates), then `cargo test --lib` on ubuntu/macos/windows (with
the 500 MB archive test, `IXTABLE_HEAVY_TESTS=1`), PostgreSQL conformance
(ubuntu service container; on `main` also macOS/Windows with a native server
via `scripts/ci/start-postgres.mjs`), golden suites (`ui::golden`; PRs ubuntu
only, `main` all three), perf budgets (`main` only), and a debug `tauri build`
on ubuntu. Shared setup is `.github/actions/setup-desktop`. Keep commands
cross-platform: use Node scripts under `scripts/ci/` instead of POSIX shell.

## Website

`web/` is the Docusaurus docs site (`cd web && npm start`, port 3001) and
`supabase/` its local Supabase stack. Neither is part of the desktop app.
Use the `writing` skill for docs and run
`python3 .claude/skills/writing/scripts/readability.py <file>`.
