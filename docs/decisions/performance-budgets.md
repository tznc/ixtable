# Performance budgets

Status: accepted, report-only. Covers PRD §27.3.

## Context

PRD §27.3 sets performance targets and says they are measured against
published reference hardware and fixture sizes. Until this record there was
no timing harness, no reference hardware, and no fixture. Three hot spots
were known: every save rewrote and recompressed the whole archive, every
PostgreSQL write rebuilt the DuckDB reader, and a filtered list scanned up to
50,000 rows in TypeScript.

## Decision

### Reference hardware and fixture

The reference hardware is the GitHub-hosted `ubuntu-latest` runner (4 vCPU,
16 GB RAM, SSD). Numbers from a developer machine are useful for comparing
before and after, but only the CI numbers are published.

The fixture is built by `src-tauri/src/perf_tests/fixture.rs`:

| Part | Size |
|---|---|
| Definitions | the CRM golden template (forms, queries, reports, dashboards) |
| `deals` | 50,000 rows, plus the template's seed rows |
| `companies` | 1,000 extra rows |
| Assets | one 8 MiB incompressible file |
| Deals list filter | `record.amount >= 99000` (about 500 of 50,000 rows) |

### Budgets

| Target (PRD §27.3) | Budget | Measured by |
|---|---|---|
| Application open after warm start | 3 s | `open-warm` (Rust: open, list objects, first page) and `open-ui` (UI: open event to Runtime shown) |
| Local field edit acknowledgement | 100 ms | `field-edit` (UI: change event to rendered value) |
| Autosave completion | 2 s | `autosave-definition` and `autosave-record` (Rust) |
| Runtime navigation between loaded pages | 200 ms | `navigate-loaded` (UI: Companies and Contacts after both loaded) |
| Report and dashboard progress and cancellation after 2 s | progress and Cancel at 2 s | `query-cancel` (Rust: time from cancel to stop for a query cancelled 2 s in) |

Informational rows have no budget: `write-read-sqlite` and
`write-read-postgres` (a write, the read refresh, and the next read) and
`filtered-list` (a cold filtered Deals list). The 30-minute
install-to-working target is a user study, not a timing, and is not measured.

Progress and Cancel at 2 s are already in the UI: `PROGRESS_DELAY_MS` in
`src/reports/ReportPreview.tsx`, `SLOW_QUERY_MS` in
`src/dashboards/useDashboardData.ts`, and `RunStatus` for queries, with
`cancel_query` interrupting DuckDB. Their behavior is covered by the report,
dashboard, and query mode integration tests. The harness measures how fast a
cancel stops the query.

### Harness

- Rust: `src-tauri/src/perf_tests/` tests are `#[ignore]`d and run only with
  `--include-ignored` and `IXTABLE_PERF=1` (under CI a missing variable fails
  the test; the `perf` job runs them through
  `scripts/ci/run-ignored-rust-tests.mjs`, which checks each one passed). Each
  scenario runs once untimed, then seven timed samples, and records median,
  p95, and samples in `reports/perf/rust.json` (`IXTABLE_PERF_OUT` moves it).
  It also writes `reports/perf/perf-fixture.ixt`. The PostgreSQL row needs
  `IXTABLE_TEST_POSTGRES_URL`.
- UI: the React UI harness (`tests/perf/ui.perf.test.tsx`, jsdom over the
  NAPI test bridge) was removed with the React frontend. No native GPUI perf
  harness exists yet, so only the Rust half runs.
- `scripts/ci/perf-report.mjs` prints both files as a Markdown table, marks
  each budget within or over by p95, writes `reports/perf/summary.md`, and
  appends it to the GitHub job summary.

Run it locally:

```bash
cd src-tauri && IXTABLE_PERF=1 cargo test --release --lib perf_tests -- --include-ignored --test-threads=1
cd .. && node scripts/ci/perf-report.mjs
```

### CI

The `perf` job in `.github/workflows/desktop.yml` runs on pushes to `main`
(not on pull requests, to keep PR runners free). It runs both halves in release
mode on `ubuntu-latest` with a PostgreSQL 16 service, publishes the table to
the job summary, and uploads `reports/perf/*.json` and `summary.md` as the
`performance-budgets` artifact. The job has `continue-on-error: true` and no
step compares against a budget, so a miss is visible but never fails CI.

### Hot spots fixed

- Saves copy unchanged payloads from the previous archive instead of
  compressing them again ([archive format](./archive-format.md)).
- A PostgreSQL refresh clears postgres_scanner's catalog cache instead of
  rebuilding the reader ([DuckDB read path](./duckdb-read-path.md)).
- List forms over tables push the safe part of their row filter into DuckDB
  ([expression language](./expression-language.md)).

### Before and after

Medians of seven samples on a 4 vCPU Linux container, release build, on
the fixture (October 2026). Published CI numbers replace these.

| Scenario | Before | After |
|---|---:|---:|
| `autosave-definition` | 138 ms | 43 ms |
| `autosave-record` | 146 ms | 104 ms |
| `write-read-postgres` | 267 ms | 32 ms |
| `filtered-list` | 6,088 ms | 119 ms |
| `open-warm` | 211 ms | 234 ms |
| `open-ui` | 193 ms | 210 ms |
| `navigate-loaded` | 37 ms | 37 ms |
| `field-edit` | 2.9 ms | 2.9 ms |
| `query-cancel` | 0.6 ms | 0.7 ms |

Open, navigation, field edit, and cancel did not change; the open
difference is noise between runs.

## Consequences

- Budgets are reported, not enforced. A regression shows up in the job
  summary and the artifact, and a reviewer has to look.
- Shared runners are noisy. Compare medians across several runs before
  calling a change a regression.
- The release build adds a cold compile of the crate and DuckDB to the job
  when its cache is empty.
- A record edit still compresses all of `data.db` on save, so autosave after
  a record edit grows with data size.

## Evidence

- `src-tauri/src/perf_tests/`: the Rust harness and fixture.
- `tests/perf/ui.perf.test.tsx`: the removed UI harness (see above).
- `src-tauri/src/archive_io/reuse_tests.rs`,
  `src-tauri/src/durability_tests/incremental.rs`: incremental saves.
- `src-tauri/src/recordstore/conformance.rs`: PostgreSQL refresh keeps the
  attachment and sees writes and DDL.
- `tests/unit/pushdown.test.ts`, `tests/unit/filtered-paging.test.ts`: filter
  pushdown keeps the same rows and reads only matching ones.

## Audit log

- 2026-10-05: checked against main after it landed in #36. The code and
  test paths it names exist; `reports/perf/*.json` are generated by the
  harness, not committed. No changes.
