# Datasheet tools

Status: accepted. Implements the PRD Phase 6 datasheet tools: filter by
selection, multi-column sort, find and replace, freeze and hide columns, a
totals row, and multi-cell paste from spreadsheets.

## Context

Access users expect the table datasheet to behave like a spreadsheet. The
tools have to keep the existing rules: reads go through DuckDB, record writes
go through `src/lib/records.ts` so triggers fire, and definition edits go
through the config store.

## Decision

The Data mode grid moved from `DatabaseWorkbench` into `src/data/sheet/`.

- **Filters.** Filter by selection and filter excluding selection turn the
  focused cell into an `eq`, `ne`, `is_null` or `is_not_null` filter
  (`filters.ts`). They combine with the search box as one AND list, which the
  page read, the totals row and export all use. Filters are session state per
  table and are not saved in the document.
- **Sort.** Clicking a header sorts by it. Shift-click adds the column as the
  next sort key (`sorts.ts`). The column menu also sorts.
- **Find and replace** (`FindReplace.tsx`, `find.ts`) matches the text a cell
  shows, skipping nulls and blobs. Find next walks the filtered, sorted table
  one page at a time, wraps once, and moves the grid to the match. Replace all
  reads every matching row, asks for confirmation, and sends one
  `writeRecordBatch` with `expected` values.
- **Hidden and frozen columns and the totals row** are saved per table in
  `navigationState.datasheetLayouts` through `useDocumentConfig().update`, like
  the relationship layout (`layout.ts`). Each change is one undo step. Frozen
  columns are the leftmost visible columns, made `position: sticky` at offsets
  measured from the rendered headers. Hiding a column never reorders columns.
- **Totals** come from the `read_table_totals` command
  (`src-tauri/src/data/totals.rs`). It runs `sum`, `avg`, `count`, `min`,
  `max`, `stddev_samp` or `var_samp` in DuckDB over the same FROM and WHERE as
  the page query, so totals cover every filtered row, not only the page. Sum,
  average, standard deviation and variance need a numeric column.
- **Paste** (`paste.ts`) takes over a paste that spans more than one cell.
  It parses tab-separated clipboard text with quoted fields, maps it onto the
  visible editable columns from the focused cell, and sends one
  `writeRecordBatch`. Rows that land on existing records update them (an empty
  cell becomes null). Rows past the last record, or pasted into the new-record
  row, are inserted (an empty cell keeps the default). A cell that does not
  parse rejects the whole paste before anything is written.

## Consequences

- Paste and replace all are one batch: a failure writes nothing, and a
  failing trigger after commit reports a committed write as usual.
- A paste that starts on existing records and runs past the current page is
  refused, because the rows after the page are not on screen.
- Find and replace reads the whole table when replacing all, one page of up
  to 1000 rows at a time.
- Filter by selection on text is case-sensitive (DuckDB `=`), unlike the
  search box, which uses `ILIKE`.

## Evidence

- `src-tauri/src/data/totals_tests.rs`: aggregates over filtered rows, type
  checks, unknown columns.
- `tests/unit/datasheet-tools.test.ts`: clipboard parsing, paste plans and
  writes, find and replace matching, selection filters, layout storage, sort.
- `tests/integration/datasheet-tools.test.tsx`: each tool end to end against
  the Rust bridge.
