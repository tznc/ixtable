# CLAUDE.md

ixtable is a Tauri 2 desktop app (Studio + local Runtime) for Access-style
business applications stored in one `.ixt` file. React/TypeScript UI in
`src/`, Rust in `src-tauri/src/`. Product spec: `PRD.md`. Architecture
decisions: `docs/decisions/` (read the relevant record before changing a
subsystem, and update it in the same change).

## Setup and commands

```bash
npm ci
bash scripts/prepare-duckdb-artifacts.sh linux-x64   # pinned prebuilt libduckdb + extensions, once per checkout
npm run tauri:dev                                    # run the app
```

| Task | Command |
|---|---|
| Build test bridge (after any Rust change) | `npm run pretest` |
| All JS tests (builds bridge first) | `npm test` |
| One unit file | `npx vitest run tests/unit/<file>` |
| One integration file | `npx vitest run --config vitest.integration.config.ts tests/integration/<file>` |
| Rust tests | `cd src-tauri && cargo test --lib [module]` |
| Rust compile check | `cd src-tauri && cargo check --lib --features test-bridge` |
| Typecheck / lint / format | `npx tsc --noEmit`, `npm run lint`, `npx biome format --write <files>` |
| Screenshots | `npm run screenshot` (see `.claude/skills/app-qa/SKILL.md`) |

PostgreSQL tests need
`IXTABLE_TEST_POSTGRES_URL=postgresql://user:pass@host:5432/db?sslmode=disable`:
`tests/integration/datasource.test.tsx` skips without it, and the Rust ones
(and the heavy and perf tests) are `#[ignore]`d. Run those with
`cargo test --lib -- --include-ignored <filter>`; under `CI` a missing variable
panics (`src-tauri/src/test_env.rs`), and CI runs them through
`scripts/ci/run-ignored-rust-tests.mjs`, which checks each one ran and passed. `IXTABLE_DUCKDB_SQLITE_EXTENSION` / `IXTABLE_DUCKDB_POSTGRES_EXTENSION`
override extension paths (hashes still checked). `IXTABLE_STATE_DIR` moves
the state directory (tests set it to a temp dir).

## How tests reach Rust

`npm run pretest` runs `scripts/ci/build-test-bridge.mjs`: `cargo build --lib
--features test-bridge`, then copies the tauri-test loader to
`src-tauri/target/index.cjs`. Integration tests mock `@tauri-apps/api/core`
`invoke` with that bridge, so every `#[tauri::command]` runs for real. Drive
the UI with `userEvent`, query by role/label, use `findBy*` with
`{ timeout: 20_000 }` for async UI.

## Map

- Modes (`src/shell/modes.tsx`): data, query, design (forms), reports,
  dashboards, automation, app (settings tabs in `src/shell/settings-tabs.tsx`),
  run (Runtime). Each feature lives in `src/<feature>/` with `index.tsx`,
  `api.ts`, `types.ts`.
- Shared: `src/lib/{api,types,config-store,records}.ts`, `src/expr/`
  (expression language), `src/grid/` (grid engine for forms and dashboards),
  `src/persistence/` (autosave, recovery, checkpoints).
- Rust: `manager.rs` (sessions, save), `archive*.rs` (`.ixt` format),
  `data/` (DuckDB reader), `recordstore/` + `postgres/` (writes),
  `queries/`, `design/`, `reports.rs`, `dashboards/`, `automation.rs`,
  `jobs.rs` (async trigger queue), `bundle*.rs` + `installation*.rs`
  (runtime bundles), `validation.rs`.

## Rules

- Reads go through DuckDB (`ReadRuntime`). Writes go through a RecordStore,
  then `mark_data_dirty` refreshes the reader. Action queries are the one
  exception: `queries::action` runs them on a separate writable DuckDB
  connection (`docs/decisions/action-queries.md`).
- Frontend record writes only via `src/lib/records.ts`
  (`insertRecord`/`updateRecord`/`deleteRecord`, `runActionQuery`). Triggers
  hook in there.
- Definition edits only via `useDocumentConfig().update(mutator, label)`.
  A direct `invoke` config edit is overwritten by the next store update.
- Tauri commands only via `call()` in `src/lib/api.ts`, wrapped per feature in
  `src/<feature>/api.ts`. Lint enforces it.
- New command: `#[tauri::command] pub fn name(window_label: String, ...) ->
  Result<T, AppError>` in the feature module (not in a test module, unique
  name), registered under the module's anchor in `lib.rs` `generate_handler!`.
- Every definition object has a stable uuid id. Expressions are evaluated
  only in TypeScript (`src/expr`), never in Rust.
- Keep Rust files under 2000 and TS files under 500 non-comment lines
  (ast-grep warns). Single-line comments inside code blocks.
- Every change ships with tests: Rust unit tests, `tests/unit/*.test.ts(x)`,
  and `tests/integration/*.test.tsx` for UI flows.

## CI

`.github/workflows/desktop.yml`: lint, then tests on ubuntu/macos/windows
(with the 500 MB archive test, `IXTABLE_HEAVY_TESTS=1`), PostgreSQL
conformance (ubuntu service container; on `main` also macOS/Windows with a
native server via `scripts/ci/start-postgres.mjs`), golden suites
(`tests/integration/golden/`; PRs ubuntu only, `main` all three), perf
budgets (`main` only), and a debug `tauri build` on ubuntu. Shared
setup is `.github/actions/setup-desktop`. Keep commands cross-platform: use
Node scripts under `scripts/ci/` instead of POSIX shell in `package.json`.

## Website

`web/` is the Docusaurus docs site (`cd web && npm start`, port 3001) and
`supabase/` its local Supabase stack. Neither is part of the desktop app.
Use the `writing` skill for docs and run
`python3 .claude/skills/writing/scripts/readability.py <file>`.
