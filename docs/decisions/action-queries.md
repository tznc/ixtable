# Action queries

Status: accepted. Implements PRD §12.1 (action queries), which extends saved
queries with queries that change rows. Supersedes the "reads cannot write"
consequence of the [DuckDB read path](./duckdb-read-path.md) for action
queries only: the reader itself stays locked read-only.

## Context

A saved query used to be one read-only DuckDB statement. Access applications
rely on action queries (append, update, delete, make-table) run from buttons
and macros, and the Access importer had to leave them out. Users also want
set-based changes in their own documents without writing one record at a
time.

Three rules constrain the design. Writes go through the session's store and
then refresh the reader. Record triggers fire once per created or updated
row. Role permissions are checked per table operation. An action query has
to respect all three while changing many rows in one statement.

## Decision

**One dialect.** An action query is DuckDB SQL, like a read query, so the same
SQL runs whether the document stores data in the embedded SQLite file or in
PostgreSQL. `SavedQuery.action` (`{ kind, table }`) marks it: `insert`,
`update` or `delete` run `sql` as that statement on `table`. `replace` runs
`sql` as a SELECT whose rows replace every row of `table` (the Access
make-table query). Adding this nested field moved the config version to 4,
as [the archive format](./archive-format.md) requires.

**The guard.** `queries::action::target` accepts exactly one statement of the
declared kind writing to the declared table, optionally qualified by `data`
or the datasource schema, with no DDL, catalog, settings or file-reading
word (`sqlite_query` among them, since it would run SQLite statements on the
writable attachment). It works lexically on masked SQL (`data::sqltext::mask`), like the read
guard, because the DuckDB crate does not expose statement types.

**The writer.** `data::write::open_writer` opens a short-lived in-memory DuckDB
for each run. It loads only the pinned extensions, attaches the datasource
read-write as `data`, then locks down like the reader (`enable_external_access`
off, `lock_configuration` on). For the embedded file, the run holds
`data::gate` exclusively from before the attach until the connection is
dropped, because `sqlite_scanner`'s copy of SQLite is one more writer. All
statements of a run share one transaction. A dry run rolls it back and
reports counts.

Two `sqlite_scanner` limits shape the SQLite path:

- It cannot UPDATE a DATE or TIMESTAMP column: the bind hits an internal
  assertion and invalidates the database. So an UPDATE of the embedded file
  runs on a copy of the table (`Plan::Copy`, the target renamed to
  `temp.main.__ixtable_work` with the table's name as its alias). The copy is
  compared with the live table by rowid, which also gives triggers their
  `old` values. A second writer then writes the changed columns in one
  DuckDB transaction: the rows go into a temp table, and one `UPDATE … FROM`
  sets each changed cell. That writer attaches the file with
  `sqlite_all_varchar`, so every column is VARCHAR and the scanner never
  binds a date. Values are written as the text ixtable stores (each
  column's canonical form, booleans as 0 and 1), and SQLite's column
  affinity turns numbers back into numbers. Binary values cannot go through
  text, so an update that changes a binary column uses a typed writer, and
  one that changes both a binary and a date or time column is refused.
  Every statement of a saved query, the write included, runs in DuckDB.
- It evaluates column defaults in DuckDB, and `CURRENT_DATE` and
  `CURRENT_TIME` need the ICU extension, which ixtable does not bundle. The
  writer defines `current_date()` and `get_current_time()` as temporary
  macros that return the same UTC values SQLite would.

- It has no option for `PRAGMA foreign_keys`, which is off by default in the
  copy of SQLite it bundles. On Linux it binds to rusqlite's SQLite, which
  turns it on, but on macOS and Windows it does not. Its `JOURNAL_MODE`
  attach option is run as `PRAGMA journal_mode=<value>` through
  `sqlite3_exec`, so the writer passes the file's own journal mode followed
  by `PRAGMA foreign_keys=ON`. The extension is pinned by hash, and the
  action tests check foreign keys and cascades on all three CI platforms.

INSERT, DELETE and replace run on the attachment directly. Constraints,
foreign keys and `ON DELETE CASCADE` are enforced by SQLite on every
platform. PostgreSQL runs
every kind directly, `postgres_scanner` handles every type.

**Triggers.** When the target has enabled `created`, `updated` or `deleted`
triggers, the run finds the changed rows: inserts compare key sets before and
after, updates compare every column by key, deletes keep every column of the
rows that are gone, and `replace` reports every old row as deleted and every
new row as created. `run_action_query` returns the created identities, the
updated identities with their old values, the deleted rows with their values,
and one sync-trigger grant per event. Rows an SQL cascade removes from other
tables do not fire those tables' triggers.
`src/lib/records.ts` `runActionQuery` calls each record hook once with a
single write whose `meta.bulk` carries those rows, and `src/automation/triggers.ts`
fires the triggers once per row, in order, as if each row had been written
alone. Each use of a grant extends it (`trigger_auth::verify`), so one grant
covers all rows of an event. It is released when they are done. A table with
triggers needs a primary key.

**Before-change triggers.** They run in TypeScript, so a run on a table with
enabled before-change triggers (any kind but delete) takes two calls
(`queries::action_before`). The first runs the statement, collects every row
it would create or update (all new values, and the old values of updated
rows), rolls back, and returns them as `pending` with a fingerprint. The
caller runs the triggers on each row; a rejection ends the run there, with
nothing written. The second call carries `before`: the fingerprint and the
fields set per row, by position. It runs the statement again, refuses with
CONFLICT and rolls back if the rows differ, and applies the fields in the
same transaction: an UPDATE by key on PostgreSQL, an UPDATE of the copy
before it is compared for an embedded-SQLite update, and for rows an
embedded-SQLite insert created, a DELETE and INSERT of the row (the
attachment cannot UPDATE a date column). The fingerprint leaves out new rows'
defaulted and identity columns, whose values can change between the calls,
which is also why fields are matched by position. Trigger fields cannot
change a row's key. A dry run skips the triggers.

**Permissions.** A run needs the role's table operations: `create` for insert,
`update`, `delete`, and both `delete` and `create` for replace
(`trigger_auth::action_query_ops`). The user-mode trigger precheck runs as for
a record write. Tables whose entity routes updates and deletes to a custom
action refuse update and delete queries.

**Where they run.** Query mode authors them (query type and target table),
previews them with a dry run, and runs the saved definition after a
confirmation. Automation runs them with a `runQuery` step, and `storeAs` gets
`{ changed, removed }`. Read paths (`run_saved_query`, pages, forms, reports,
dashboards) refuse them, pickers do not list them, and validation flags any
form, report or dashboard that reads one.

**Access import.** Append, update, delete and make-table queries translate to
action queries (`access/translate/dml.rs`). An UPDATE or DELETE through inner
joins becomes `UPDATE … FROM` or `DELETE … USING` with the join conditions in
WHERE. A make-table query whose target the file lacks gets the table from
migration 002, typed by DuckDB's `DESCRIBE` of its SELECT. `RunSQL` macro
actions become hidden action queries and `OpenQuery` on an action query
becomes a `runQuery` step.

## Consequences

- An action query writes outside the RecordStore's per-row checks: the
  entity concurrency policy (`optimistic` and the rest) does not apply. A
  dry run and the confirmation are the safeguard in Studio.
- A SQLite UPDATE commits in its own transaction after the computation, still
  under the exclusive gate, so no other write can come between them.
- A SQLite UPDATE copies the whole table into memory, and a run with
  triggers copies the keys or rows of the target. Large tables cost memory
  and time in proportion.
- An action query cannot join a rollback-mode action's batch, because it
  writes at once. Validation and the runner refuse that combination.
- Steps of app-mode triggers cannot run action queries without the role's
  table permission, since a grant covers only the writes a trigger declares.
- SQLite column defaults other than literals, `CURRENT_TIMESTAMP`,
  `CURRENT_DATE` and `CURRENT_TIME` must be DuckDB expressions for inserts
  to work.

## Evidence

- `src-tauri/src/queries/action_tests.rs`: the guard, planning, the copy
  path with DATE and TIMESTAMP columns, a changed key and a table without a
  key, inserts with date defaults, dry runs and constraint rollback on a
  real SQLite file, foreign keys and cascades with the journal mode kept,
  validation, and (ignored, CI runs it) the PostgreSQL path.
- `src-tauri/src/queries/action_before_tests.rs`: deleted rows of deletes
  and replaces, pending rows and applied trigger fields for inserts and
  embedded updates (a date column included), refused stale fingerprints, key
  columns and unknown rows, and (ignored, CI runs it) the PostgreSQL path
  with an identity column.
- `src-tauri/src/trigger_auth_tests.rs`: a grant extended by each use.
- `src-tauri/src/access/tests/translate.rs` and `convert.rs`: Access action
  statements translated and run in DuckDB. An imported template's append,
  update and make-table queries and its RunSQL and OpenQuery macro actions.
- `tests/unit/action-queries.test.ts`: per-row triggers, one shared grant,
  hooks, the runQuery step, rollback and permission refusals.
- `tests/integration/action-queries.test.tsx`: authoring, preview and run in
  Query mode, refusals, and triggers through the real bridge.

## Audit log

- 2026-10-09: record created with the feature.
- 2026-10-09: the SQLite UPDATE write-back moved from the RecordStore to
  DuckDB (a text attachment), so every action query runs in DuckDB.
- 2026-10-10: the writer turns on SQLite foreign keys itself, because the
  scanner's own SQLite (macOS and Windows) leaves them off. Both guards
  refuse `sqlite_query`.
- 2026-10-10: PRD §9.4, §10 and §12.1 now allow the DuckDB write path for
  user-written action queries only, and require store constraints, including
  foreign keys, to hold as they do for RecordStore writes.
- 2026-10-10: deleted triggers fire on delete and replace queries, and
  before-change triggers run on each pending row through a second call.
