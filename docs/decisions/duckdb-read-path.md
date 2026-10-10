# DuckDB read path and pinned extensions

Status: accepted on Linux. macOS and Windows are wired into CI
(`.github/workflows/desktop.yml`) and become proven when that matrix passes.
Covers PRD §3.2 (file import), §9.4, §10, and the Phase 0 DuckDB spikes.

## Context

The PRD splits reads from writes. Every list, detail, selector, saved query,
report, and dashboard dataset reads through DuckDB. Every create, update, and
delete goes straight to the RecordStore. DuckDB reaches SQLite and PostgreSQL
through loadable extensions, and those must be a fixed, verified set on all
three platforms. Arbitrary extension installation is deferred.

## Decision

### One reader per session

`data::ReadRuntime` owns an in-memory DuckDB connection per open document. It
attaches the datasource as the `data` catalog in `READ_ONLY` mode. For SQLite
that is `<workspace>/data.db` through `sqlite_scanner`. For PostgreSQL it is
the configured schema through `postgres_scanner`, with the password redacted
from any error text.

App SQL never runs with external access. The reader opens DuckDB with external
access on only to load the bundled extensions and attach `data`, then runs
`SET allowed_paths=['<workspace>/data.db']`, `SET enable_external_access=false`
and `SET lock_configuration=true`. After that DuckDB itself refuses
`read_csv`, `read_text`, `glob`, file replacement scans (`FROM '/etc/passwd'`),
`COPY TO`, `ATTACH`, `INSTALL`, and loading new extensions, and no `SET` can
undo the lock. Attached databases keep working. The lock is applied even when
the attach fails. The reader then remembers the error, and every read fails
with `Datasource unavailable: ...` instead of falling back to the embedded
file.

After every write, the manager calls `mark_data_dirty`, which runs
`ReadRuntime::refresh`. For SQLite, refresh detaches and reattaches `data`
in place (the file is in `allowed_paths`). For PostgreSQL the attachment
stays: postgres_scanner reads rows live, in a new PostgreSQL transaction for
each DuckDB transaction, so committed rows are already visible, and only its
catalog cache (tables and columns) can be stale. Refresh runs
`CALL pg_clear_cache()` to drop it. DuckDB refuses a PostgreSQL `ATTACH` once
external access is off, so a datasource switch, a failed clear, or a retry
after a failed attach builds a fresh locked database instead. Either way the
next read sees committed rows and DDL. Because writes
commit before the refresh returns, a workflow that writes and then reads gets
its own write back. Config edits re-attach only when `datasource` changed, and
build the new reader outside the sessions lock.

Refresh takes the exclusive side of the reader's gate (`data::gate`, keyed by
`<workspace>/data.db`) for both targets, and every read takes the shared side,
including PostgreSQL reads and the cloned connections `read_connection` hands
to long queries. For SQLite the gate also keeps reads off the file while a
RecordStore write is open. For PostgreSQL it orders the cache clear against
reads. Cloned connections share one DuckDB database and so one attached
catalog; before the in-place clear, a rebuild swapped in a new database and
clones kept the old one. In the pinned postgres_scanner (commit `41223e5`,
built for DuckDB 1.5.5) the clear is memory-safe on its own: each
PostgreSQL transaction holds a `shared_ptr` to every catalog entry it looked
up (`ReferenceEntry`), so a running query keeps its entries alive. It is not
ordered, though: `ClearEntries` takes only the entry lock, while a read that
is loading the catalog holds the separate load lock, so a load that started
before the clear can store the pre-DDL catalog after it and mark it loaded,
and the writer's next read misses its new table or column. An ungated stress
run (8 readers, 3,000 refreshes) showed no failure, but the window exists.
The gate keeps the in-place clear (no rebuild, so the refresh stays cheap) and
makes it wait for running reads. A refresh behind a long query waits up to
the gate timeout (30 s) and then fails as busy, as on SQLite.
`data::race_tests::postgres_refresh_never_races_reads_on_cloned_connections`
runs refreshes after DDL and writes against four threads reading through
clones, in the PostgreSQL CI jobs.

### External files

DuckDB reads CSV, JSON, and Parquet with its own readers. The official
prebuilt libduckdb includes them, so they are part of the pinned library and
not loadable extensions: the extension
allowlist stays `sqlite_scanner` and `postgres_scanner`, and nothing is
installed at runtime. XLSX is read in Rust with `calamine`. Global external
access stays off in both uses below; each connection may open only the files
listed in its `allowed_paths`, set before the lock.

**Import** (`import/`, `data::files::sandbox`). The wizard parses the chosen
file in a fresh in-memory DuckDB whose `allowed_paths` is that one file, with
external access off and the configuration locked. Files over 1 GB (100 MB for
XLSX, whose shared strings stay in memory) are refused before reading. The
preview reads `read_csv` (header and delimiter options), `read_json`, or
`read_parquet` with types sniffed from the whole file (`sample_size=-1`), and
returns the columns, DuckDB's types mapped to logical types by
`logical_from_duckdb` (lists, structs, maps, and unions are JSON, intervals
are text), the first 50 rows, and the row count. XLSX is read with calamine's
streaming cell reader: the preview reads the sheet once for the narrowest
logical type that holds every cell (whole numbers are integers, Excel dates
are dates, or timestamps when any has a time, and mixed columns are text),
the first 50 rows, and the count. Only cells that exist are read, so a huge
declared used range costs nothing; a sheet whose cells span more than 1600
columns (PostgreSQL's table limit) is refused.

The sniffed types are only suggestions for the mapping. The import reads CSV
with `all_varchar=true` and streams rows from the file; it never holds the
file in memory and never writes through DuckDB. Each mapped value is
converted with `LogicalType::normalize` for its target field, the same check
every store write uses, so a value that does not fit (say `n/a` at row 30,002
of a column sniffed as integer) skips only its row, which the report lists.
A row that leaves a required field empty is skipped the same way. The
remaining rows go to the RecordStore (SQLite or PostgreSQL) in batches of 500,
one transaction each. A batch the store rejects with a constraint or
validation error is retried row by row, so only the rows at fault are
reported; any other store error (a lost connection, a busy or full database)
or a file read error stops the import. A stop after anything was written
(including a new table being created) is not a command error: the report
comes back with `aborted` and the counts so far, the committed batches stay,
and the wizard reloads the configuration and tables after every attempt.
After writing, `sync_identity` moves PostgreSQL key sequences past the
imported keys (see `recordstore-capabilities.md`). A new-table import creates
the table through the usual `create_table` path (with an `id` integer key
unless a file column is chosen as the key). Record triggers do not run for
imported rows, and runtime roles cannot import.

**File sources** (`import::sources`, `data::files::create_views`). A bundled
source is an application asset plus a `fileSources` entry in the config
(`{id, name, assetId, format, csv}`), so the file travels in the archive's
`attachments` table and in runtime bundles without an archive format change.
A runtime installation keeps no extracted assets, so opening one writes the
source files to `<installation>/attachments/<id>/content` first.
When the reader is built, it attaches an in-memory `files` catalog and
creates `files.main.<name>` as a view over the asset's extracted file
(`attachments/<id>/content`) while external access is still on. View types
are sniffed from the whole file, like the preview, so a late value that does
not fit a type sniffed from the first rows cannot fail a query. The lockdown
then adds exactly those files to `allowed_paths` next to `data.db`. Queries,
reports, and dashboards read `files.<name>`; `read_only_guard` still rejects
writes and direct `read_*` calls, and DuckDB refuses any other file even
when the guard is bypassed. A config change to `fileSources` (like a
datasource change) builds a new reader outside the sessions lock. A source
whose file is missing or unreadable is skipped and its error is shown in
Settings, File sources. Asset ids in the config must be plain ids, so a
crafted archive cannot point a view outside `attachments/`. XLSX stays
import-only.

### Writes and reads of one file never overlap

rusqlite and `sqlite_scanner` each link their own SQLite. SQLite coordinates
connections with `fcntl` advisory locks, which belong to the process, so one
copy cannot see the locks the other holds. Picture a DuckDB read that starts
while a RecordStore transaction is open. It finds the writer's rollback
journal, sees no lock on it, and takes it for a crashed writer's hot journal.
Since `data` is attached `READ_ONLY`, it fails with
`attempt to write a readonly database`. Some hosts export their own SQLite
symbols: the `cargo test` binary, and the Linux app because of `build.rs`.
There the extension binds to rusqlite's copy instead. The writer and the
scanner's several handles can then deadlock on SQLite's locks until both fail
with `database is locked`.

`data::gate` arbitrates in process, per file. A RecordStore connection holds
the gate exclusively for its whole life. Every `ReadRuntime` read and every
connection from `DocumentManager::read_connection` holds it shared. The
SQLite refresh after a write holds it exclusively too, because it detaches the
`data` catalog that cloned query connections are using. Waiting writers block
new readers, and when a writer finishes, the readers already waiting enter
before the next writer, so neither a stream of reads nor a stream of writes
starves the other side. A thread that already holds the gate passes through,
and a wait longer than 30 seconds fails with `BUSY`. The order stays write, commit,
refresh, read, so read-your-writes holds. An ad-hoc read error is `READ_ONLY`
only when `read_only_guard` rejected the SQL.

User SQL also passes `read_only_guard`, as defense in depth. It ignores string
literals, quoted identifiers, and comments (`data::sqltext::mask`), allows one
statement with an optional trailing `;` that starts with `SELECT`, `WITH`,
`VALUES`, `SHOW`, or `DESCRIBE`, and rejects writes, DDL, `ATTACH`,
`INSTALL`, `LOAD`, `COPY`, `PRAGMA`, `SET`, and file or scanner table
functions such as `read_csv_auto`, any `read_*(...)` or `*_scan(...)` call,
`postgres_query`, and `duckdb_databases` (which would show the PostgreSQL
connection string). Saved queries
use `$name` placeholders. `queries::params` rewrites them to DuckDB
parameters and binds the values, so values are never spliced into SQL text.

Forms whose source is a saved query page through `run_saved_query_page`
(`queries::page`). Like `run_saved_query`, it first checks that a runtime role
may read the query (`authz::check`). The saved SQL runs as a subquery
(`SELECT * FROM (<sql>) AS ixt_page`), and the filters, sort, `LIMIT`/`OFFSET`
wrap it in DuckDB, and a `count(*) OVER ()` column returns the exact total with
the page, so the saved query runs once per page (a separate count runs only
for an empty page past the first). There is no row cap, and the total counts
every matching row. Sort and filter columns must be result columns of the
query and are quoted; an unknown name is reported after a `LIMIT 0` probe.
An `in` filter binds each candidate (`col IN ($n, …)`) and an empty list
matches nothing, as on table pages.
Text search on tables and saved queries is the same case-insensitive `ILIKE`,
with `\`, `%` and `_` in the typed text escaped so they match literally. Filter values, limit and offset bind as
further positional parameters after the query's own, so the same guard and
binding rules apply. Without a sort, the order is whatever the saved query
produces. The form binds each query parameter to an expression over `app` and
the page `params`, which TypeScript evaluates (`sourceParams` in
`src/runtime/data.ts`).

### Pinned extensions

- `duckdb` is pinned to `=1.10505.0`, which binds DuckDB 1.5.5. The app
  links the official prebuilt shared libduckdb 1.5.5 instead of compiling
  DuckDB from source (the `bundled` feature took most of a clean build).
  `.cargo/config.toml` sets `DUCKDB_LIB_DIR` to
  `src-tauri/resources/duckdb/lib`. Extensions are ABI-specific, so the
  crate, library, and extension versions move together.
- Bundles ship the library where the loader finds it (`build.rs` sets the
  rpaths): Linux in the resource dir `../lib/ixtable` next to `bin/`
  (`tauri.linux.conf.json`), macOS in `Contents/Frameworks`
  (`tauri.macos.conf.json`), Windows next to the exe
  (`tauri.windows.conf.json`). On Linux the binary links with
  `--exclude-libs,ALL`: WebKitGTK references `sqlite3_*`, so the linker would
  export rusqlite's bundled SQLite, and `sqlite_scanner`, which carries its
  own SQLite, crashes on close when some of its calls bind to ours. The NAPI
  test bridge skips the flag because node needs its registration symbol.
- `scripts/prepare-duckdb-artifacts.sh <linux-x64|macos-universal|windows-x64>`
  downloads `sqlite_scanner` and `postgres_scanner` v1.5.5 from
  `extensions.duckdb.org`, and the platform's libduckdb archive from the
  DuckDB GitHub release into `resources/duckdb/lib`. It checks each download
  against a pinned SHA-256 and
  writes `src-tauri/resources/duckdb/<platform>/`: the archive as downloaded
  and an unpacked copy for dev builds and tests. The binaries are gitignored.
  `resources/duckdb/manifest.json` records both hashes, and the Rust code
  compiles it in (`data::extensions`), so it is the single runtime pin.
- App bundles carry only the archives (`bundle.resources` is
  `resources/duckdb/*/*.duckdb_extension.gz`). On macOS that keeps the
  extensions' ad-hoc-signed Mach-O out of notarization, which rejects it.
  Re-signing them is not an option: it would change their hashes and break
  DuckDB's own signature check.
- `data::sqlite_extension_path` and `data::postgres_extension_path` resolve
  the file before every `LOAD`. They take an unpacked copy in a resource
  directory if there is one (dev) and check its uncompressed SHA-256.
  Otherwise they check the archive's compressed SHA-256, unpack it, check the
  uncompressed SHA-256, and write it to
  `<state>/duckdb-extensions/<uncompressed sha>/<name>.duckdb_extension`.
  The directories are 0700 and the file 0600. The write goes to a unique temp
  file that is renamed into place, so concurrent processes never see a
  partial file. A cached file is reused while its hash matches and replaced
  when it does not. Any mismatch fails with `EXTENSION_STARTUP` and the app
  does not read.
- Autoload and autoinstall are off. DuckDB's own extension signature check
  stays on, because nothing sets `allow_unsigned_extensions`.
- `IXTABLE_DUCKDB_SQLITE_EXTENSION` and `IXTABLE_DUCKDB_POSTGRES_EXTENSION`
  point at an unpacked file elsewhere. They do not change the pinned hash.
- The macOS build bundles both `arm64` and `x64` archives for a universal
  app.

The DuckDB extensions on Linux and Windows link DuckDB statically, so they do
not import symbols from the host. `build.rs` still exports dynamic symbols on
Linux for extensions that do.

## Consequences

- Reads cannot write. Writes cannot skip the RecordStore and its
  capabilities, constraint mapping, and concurrency checks.
- An import holds the parsed rows in memory before writing them, which is
  fine for spreadsheet-sized files. A streaming import is a later change.
- File sources are read in place on every query. A large Parquet file
  costs no memory until a query scans it; a large CSV is parsed on each
  scan.
- Reattaching after each SQLite write is simple and correct, and cheap for a
  local file. A PostgreSQL write costs one cache clear instead of loading the
  extension, connecting, and reading the catalog again. Measured in
  [performance budgets](./performance-budgets.md).
- Upgrading DuckDB means a new crate pin, new extension hashes in two places
  (script and manifest), and a CI run on all three OSes. Unpacked copies of
  old versions stay in the state directory under their old hash.
- The first read after install unpacks about 75 MB per platform into the
  state directory. Later starts only hash the cached file.
- Offline builds need the extension files fetched once. CI caches them by the
  script's hash.

## Evidence

- `src-tauri/src/data/extensions_tests.rs`: an archive is verified, unpacked
  0600 into 0700 directories, and reused; a tampered archive (either hash) is
  rejected; a tampered cache file is replaced; eight concurrent unpacks agree
  and leave no temp files; override paths are still verified; every platform
  has both pins in the manifest; an unpacked dev copy is preferred and still
  verified; the official archive for the host platform
  unpacks to a file DuckDB loads.
- `src-tauri/src/data/files_tests.rs`: the import sandbox reads the chosen
  file and refuses other files, `COPY TO`, `glob`, `INSTALL`, and `SET`;
  the reader serves CSV, JSON, and Parquet views (joined with each other),
  keeps them across a refresh, skips a missing file, and refuses writes and
  other files even past the guard.
- `src-tauri/src/import/tests.rs`: type inference for CSV, JSON, Parquet,
  and XLSX (fixture `tests/fixtures/imports/people.xlsx`), header and
  delimiter options, field mapping checks, per-row type and required-field
  errors, unique-constraint failures isolated by the row-by-row retry, and
  file source validation (no path escape).
- `src-tauri/src/manager_config_tests.rs`: a runtime installation session
  reads a bundled CSV source.
- `tests/integration/file-import.test.tsx`: the wizard imports a CSV into a
  new table, an XLSX worksheet, and a CSV into an existing table with a row
  error report; a bundled CSV source is queried, refuses writes, and still
  reads after save and reopen.
- `src-tauri/src/data/tests.rs`: autoload is rejected, values convert
  losslessly to canonical forms, `read_only_guard` rejects writes and scanner
  functions but accepts keywords inside literals and identifiers, and file
  reads, replacement scans, `COPY TO`, `ATTACH`, `INSTALL`/`LOAD`, `glob` and
  `SET` fail on the reader even when the guard is bypassed. Table-page search
  escapes `LIKE` wildcards and is case-insensitive, and `in` filters match
  nothing when the list is empty.
- `src-tauri/src/recordstore/conformance.rs`: every scenario reads through
  DuckDB after writing through the store, as `sqlite::<scenario>` and as the
  ignored `postgres::<scenario>`, which needs `IXTABLE_TEST_POSTGRES_URL` (the
  PostgreSQL CI jobs run it, see [RecordStore capabilities](./recordstore-capabilities.md)).
  A PostgreSQL refresh keeps the same DuckDB database and still sees new rows
  and tables.
- `src-tauri/src/data/race_tests.rs`: concurrent SQLite writes and reads, and
  PostgreSQL refreshes against reads on cloned connections (ignored, PostgreSQL
  CI jobs).
- `src-tauri/src/queries/tests.rs`: named placeholders rewritten outside
  literals, typed binding, injection attempts bound, mutating SQL rejected,
  cancellation.
- `src-tauri/src/queries/tests_page.rs`: saved-query pages run in DuckDB with
  exact totals, bound filters and checked column names, `in` filters that
  match nothing when empty, and literal case-insensitive search.
- `src-tauri/src/data/gate.rs` tests: writers exclude readers and each other,
  and a reader queued behind two writers that write back to back enters after
  the current write (with writer-only priority it waited for the whole stream,
  which on a Windows runner outlasted the 30 second wait).
- `src-tauri/src/data/race_tests.rs` and `tests/integration/read-write-race.test.tsx`:
  concurrent writes and reads on one file and one session never fail, and
  each write is visible to the next read. Without the gate the integration
  test fails with `attempt to write a readonly database`.
- `tests/integration/sql-and-metadata.test.tsx` and
  `tests/integration/query-mode.test.tsx`: read SQL, write rejection,
  parameters, and cancel through the UI and the real bridge.
- [RecordStore and DuckDB type matrix](./recordstore-type-matrix.md).

## Audit log

- 2026-10-06: Link the official prebuilt libduckdb instead of the `bundled`
  source build, and ship it in each bundle.
- 2026-10-05: Added the failed-attach behavior and the guard's allowed leading
  keywords, and added `queries/tests_page.rs` and the paging, search, and dev
  copy tests to Evidence. Status unchanged: the macOS and Windows CI jobs have
  not passed on main.
- 2026-10-05 (after merging #36): re-checked the PostgreSQL refresh (`pg_clear_cache` in `data/read.rs`) that #36 added; it matches the code. No changes.
- 2026-10-05 (later): the data gate now lets waiting readers in after each write (cherry-picked a487178); the gate paragraph and Evidence describe it.
