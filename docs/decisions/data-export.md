# Data export

Status: accepted. Implements the first PRD §28 Phase 6 item: CSV, XLSX and
JSON export from tables, saved queries, ad hoc query results and runtime
list forms. Report output to XLSX is not built yet.

## Context

Access users export data to Excel and CSV every day. Before this change the
only export was report PDF. Screens show one page of up to 1,000 rows, so the
browser cannot build an export from what it has. Exports must also follow
the rules every read follows: rows come from DuckDB, and a runtime role
reads only what it is allowed to read.

## Decision

**Rust streams the rows.** `export_table`, `export_saved_query` and
`export_sql_query` (`src-tauri/src/export/`) run the same SQL a page read
runs, without LIMIT and OFFSET, on a fresh read connection
(`Manager::read_connection`). The session lock is not held while rows
stream. A table export reuses `ReadRuntime::page_plan`, so it has the page's
filters, sort and stable row order. A query export wraps the saved SQL with
`queries::filtered_sql`, which `page_on` also builds on. Each row goes
straight to a file writer, so memory use does not grow with row count.

**Exports honor what the screen shows.** The current sort and filters are
sent with the export, and every matching row is written, not only the
visible page. A runtime list form's row filter is an expression evaluated in
TypeScript. When `pushdown` can turn all of it into DuckDB filters, the
export sends those filters. When it cannot, the Export button is disabled,
so a file never holds rows the list hides.

**Raw typed values.** Files hold stored values, not display formatting.

| Format | Shape |
|---|---|
| CSV | UTF-8 with BOM (so Excel reads non-ASCII text), RFC 4180 quoting, CRLF, null as an empty field. No formula-escaping prefix, which would change the data. |
| JSON | One array of objects keyed by column name. A duplicate name gets `_2`, `_3`. Decimals are strings, so no precision is lost. JSON columns are embedded as JSON. |
| XLSX | `rust_xlsxwriter` in constant-memory mode, one sheet with a bold frozen header. Numbers, booleans, dates, times and timestamps are native cells. Integers above 2^53, decimals over 15 significant digits and text that does not parse stay strings. Cells are cut at Excel's 32,767-character limit. More than 1,048,575 rows is an error. |

**Atomic files.** The writer fills a hidden temp file beside the destination
and renames it into place only after it finishes. A failed or cancelled
export leaves an existing file untouched. Exports register with
`cancel_query` like query runs.

**Permissions.** A table or saved query export needs read access to that
object (`authz::check`), the same as reading a page. Any role that can read
an object can export it. Ad hoc SQL export needs developer access, like
running ad hoc SQL.

## Consequences

- No new DuckDB extension. DuckDB's own `COPY ... TO` would need the excel
  extension for XLSX and could not apply the per-type rules above.
- An XLSX export over the row limit fails. It does not split rows across sheets.
- Runtime list forms whose row filter cannot be pushed down cannot export.
  Extending `pushdown` widens what can be exported.

## Evidence

- `src-tauri/src/export/writers_tests.rs`: exact CSV and JSON bytes, XLSX
  round trip through calamine, the row limit, empty exports.
- `src-tauri/src/export/run_tests.rs`: streaming order, filters and sorts,
  existing file kept on failure, no temp file left behind.
- `tests/unit/export-menu.test.tsx`, `tests/unit/export-names.test.ts`: the
  Export menu, the save dialog and file names.
- `tests/integration/export.test.tsx`: table CSV and saved query JSON
  exports through the real commands.
