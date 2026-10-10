# `.ixt` archive format, atomic writes, and crash recovery

Status: accepted. Covers PRD §7, §27.1, and the Phase 0 archive and atomic
checkpoint spikes.

## Context

An ixtable application is one `.ixt` file that holds the definition, the
embedded SQLite records, and application assets. It must survive crashes,
power loss, and interrupted writes without losing the last good save. It must
also open files written by older builds, and keep data written by newer ones.

A zip of loose files would need a custom writer for atomic updates and would
hold every payload in memory. SQLite already gives transactions, checksummed
pages, and a schema that later builds can extend.

## Decision

The archive is a SQLite database in the spirit of
[SQLite Archive](https://sqlite.org/sqlar.html), with `PRAGMA application_id`
set to `ixtb`. Format 2 has these tables:

| Table | Contents |
|---|---|
| `archive_metadata` | format version, document id, timestamps, app version |
| `data_payload` | zstd-compressed `data.db` with SHA-256 and uncompressed size |
| `document_config` | `DocumentConfig` JSON and its config version |
| `attachments` | application assets with media type, checksum, and size |
| `payload_chunks` | continuation chunks for payloads over 4 MiB |

Payloads stream through zstd in 4 MiB chunks. No payload sits in memory whole,
and none hits SQLite's 1 GB blob limit. Format 1 archives open unchanged and
are rewritten as format 2 on the next save. A newer format fails with a message
that asks the user to update ixtable.

`DocumentConfig.version` is checked the same way. Older configs upgrade on
load. A newer config version fails with `UNSUPPORTED_VERSION` and a message
that asks the user to update ixtable, not with `INVALID_ARCHIVE`. Top-level
config fields this build does not know (a newer build with the same config
version) are kept in `DocumentConfig.extra` and written back unchanged by
saves, `document.json`, and the `config.yaml` projection. This covers top-level
fields only. An unknown field inside a nested object (a form, a query, a
report) is dropped on save, so a build that adds nested fields must bump the
config version; older builds then refuse the document instead of losing data.

Ordinary tables that this build does not know are copied, with rows and
indexes, from the previous archive of the same document. Extra columns that a
newer build adds to a known archive table (`archive_metadata`, `attachments`,
and the rest) are ignored on read and dropped on save, so a newer build that
needs such a column kept must bump `FORMAT_VERSION`. Views, triggers,
virtual tables, and tables from a different document are never copied. The
one exception is Restore as copy: the new archive gets a new document id but
deliberately copies the checkpoint, so it keeps the checkpoint's unknown
tables too.

### Save path

`archive_io::write` writes a complete archive to a hidden temp sibling
(`.<name>.<uuid>.tmp`), calls `fsync`, and reopens it to verify every checksum.
Only then does it rename the temp file over the destination and sync the
parent directory. Any failure removes the temp file and leaves the old archive
in place. Leftover temp files from a killed process are never read and are
removed on the next successful save.

Saves are incremental. When the destination's previous archive is the
session's own file (`Snapshot::reuse_from`: the file the session opened or
last saved, with an unchanged fingerprint, current format, same document id),
`archive_io::write` copies unchanged payloads into the temp archive as
compressed rows (`archive_io/reuse.rs`) instead of compressing them again:

- an attachment whose id, SHA-256, and size match its row in that archive;
- the data payload when `data.db` has the same `DataStamp` as when that
  archive's payload was taken (`Payload::Reuse`). The stamp is SQLite's file
  change counter, page count, schema cookie, and version-valid-for number
  from the file header, plus file size and modification time, read under the
  shared read gate. In rollback-journal mode every commit bumps the counter,
  whichever connection made it. A WAL-mode file or a leftover journal has no
  stamp, so it is always packed again. The stamp is taken right after open
  extracts the archive and before each save's `VACUUM INTO`, so a commit that
  races the save makes the next save pack the data again.

Everything else is unchanged: the result is still a complete temp file that
is synced, verified, and renamed over the old archive, so an interrupted save
still leaves the last valid archive. Verification skips decompressing reused
payloads only: they are byte copies of rows that were verified when that
archive was opened (extraction checks every checksum) or written. A reuse
failure (the file moved or lost a row) falls back to a full write.

Before saving, the manager compares the file's fingerprint with the one it
recorded at open. A file changed by another program blocks the save with
`EXTERNAL_CONFLICT` until the user reloads or uses Save As.

### Working session and recovery

Opening a document extracts it to `recovery/<sessionId>/`: `data.db`,
`document.json`, `config.yaml`, `archive.json`, and
`attachments/<id>/{content,metadata.json}`. Record writes go to that `data.db`.
The global store registers each session with a `dirty` flag.

The JSON and YAML files in the workspace are each written atomically: a hidden
temp sibling, `fsync`, rename, then a directory sync. A crash leaves the old
or the new file, never a truncated one. `document.json` and `config.yaml`
hold the same config, so recovery reads `document.json` and falls back to
`config.yaml` when it is missing or unreadable.

On the next start, a dirty session that never closed is offered for recovery.
Recovery checks `PRAGMA integrity_check` on `data.db`, loads the config, and
verifies every asset checksum. It opens `data.db` read-write (never creating
it): a crash in the middle of a transaction leaves a hot rollback journal, and
only a writable connection can roll it back. A read-only open fails with
`SQLITE_READONLY_ROLLBACK` and would reject work that is recoverable. Rolling
back drops only the uncommitted transaction, which is the correct crash
outcome. Valid work is reopened, a local checkpoint of the current `.ixt` is
taken (`before-recovery`), and the work is saved back into the `.ixt`.
Invalid work never touches the archive, and `RECOVERY_FAILED` is shown
instead. When the `.ixt` is unreadable or the checkpoint fails, the work opens
with `RECOVERY_NEEDS_SAVE_AS` and the file is left untouched. When the file
now holds a different document or a newer format, the work opens with
`EXTERNAL_CONFLICT`. Untitled work opens dirty and needs Save As. Clean
leftover sessions hold nothing new and are removed. A live session holds an OS
lock on `<workspace>.lock`, so another ixtable process never lists, cleans
up, or discards its workspace.

The frontend autosaves 1.5 s after the last change, and at most 10 s after the
first unsaved one. It shows dirty, saving, saved, and error states. Local
checkpoints are validated archive copies under
`<state>/data/checkpoints/<documentId>/`, and the newest 20 are kept.

## Consequences

- A crash during a save cannot corrupt the last valid archive. Rename is
  atomic on all three platforms when source and target share a directory.
- Any SQLite tool can inspect an archive. Editing one by hand changes its
  fingerprint and triggers the conflict check.
- A save still writes a whole new file, but only changed payloads are
  compressed. A definition edit copies the data payload and every asset; a
  record edit compresses `data.db` again and copies the assets. Save time
  after a record edit grows with `data.db` size, which the 500 MB cloud limit
  bounds. Measured in [performance budgets](./performance-budgets.md).
- Unknown-table preservation only works for plain tables. A future format
  that needs views or triggers must bump `FORMAT_VERSION`.
- Every released format stays openable. `tests/fixtures/archives/format-<N>/`
  holds one `.ixt` per golden app (CRM, inventory with its asset, work
  orders) with seeded data, plus a `manifest.json` of the ids, row counts, and
  asset checksums each must contain. A change that bumps `FORMAT_VERSION` adds
  a new `format-<N+1>/` with `node scripts/ci/write-archive-fixtures.mjs` and
  never rewrites an existing directory. Each manifest entry records the
  sha256 of its `.ixt`, checked by a Rust test, and the lint job runs
  `scripts/ci/check-archive-fixtures.mjs`, which fails a pull request that
  modifies or deletes a fixture file present on the base branch. `format-1/`
  holds the same documents as `format-2/` in the legacy layout (no
  `application_id`, no `payload_chunks`, config version 2), derived by
  `node scripts/ci/write-archive-fixtures.mjs --format-1`. `.gitattributes`
  marks `*.ixt` binary.
- Archives from a newer build are fixtures too. `tests/fixtures/archives/newer-build/`
  holds three variants of `format-2/crm.ixt`, written once by
  `node scripts/ci/write-archive-fixtures.mjs --newer-build` and listed with
  their sha256 and expected outcome in `expectations.json`: format 99 and
  config version 99 are refused with `UNSUPPORTED_VERSION` and leave the file
  unchanged; `unknown-entries.ixt` (current versions plus an unknown table,
  index and view, extra columns in `archive_metadata` and `attachments`, and
  unknown top-level config fields) opens, and a save keeps the table, its rows
  and index, and the config fields while it drops the view and the extra
  columns. The fixture-change check covers every directory under
  `tests/fixtures/archives/`, this one included.

## Evidence

- `src-tauri/src/archive_io/tests.rs`: format 1 upgrade with unknown-table
  preservation, newer and ancient format rejection, large chunked payloads with
  checksum checks, interrupted writes that keep the last valid archive, no
  copying from another document, path-like document and attachment ids
  rejected, and schema SQL of unknown tables never executed.
- `src-tauri/src/recovery.rs` tests: valid WIP loads with its assets,
  invalid WIP is reported, not loaded, a truncated or missing `document.json`
  falls back to `config.yaml`, config files leave no temp files, a hot
  journal left by a crash is rolled back instead of rejected, recovery never
  overwrites a file it could not checkpoint, and another process never lists,
  cleans, or discards a live workspace.
- `src-tauri/src/archive_io/reuse_tests.rs`: unchanged payloads are copied
  and read back identical, changed ones are compressed again, nothing is
  reused from another document or a missing archive (and a failed write keeps
  the last archive), and the data stamp changes with every commit and refuses
  WAL files.
- `src-tauri/src/durability_tests/incremental.rs`: definition edits reuse
  data and assets after save and after open, an unannounced commit packs the
  data again, an externally touched file reuses nothing, and Save As reuses
  from the current file.
- `src-tauri/src/archive.rs` tests: a newer config version is
  `UNSUPPORTED_VERSION`, and unknown top-level config fields survive save,
  reopen, and the YAML round trip.
- `src-tauri/src/checkpoints.rs` tests: Restore as copy keeps unknown tables.
- `tests/integration/persistence.test.tsx`: autosave states, autosave failure
  and retry, crash recovery from the start screen, invalid recovered work,
  ignored temp-file leftovers.
- `tests/integration/assets.test.tsx`: asset checksums, archive size report,
  checkpoint create and restore-as-copy.
- `tests/unit/autosave.test.ts`: debounce, max wait, single in-flight save.
- `src-tauri/src/durability_tests/fixtures.rs`: every committed archive
  fixture opens with the current build, matches its manifest (forms, queries,
  reports, dashboards by id, row counts, asset checksums), saves (upgrading
  older formats), and reopens.
- `src-tauri/src/durability_tests/kill.rs`: a real child process saves and
  autosaves a growing document and is killed (`Child::kill`) at eight delays
  and, alternately, while a test-only marker shows an archive write or rename
  is in progress (at least one kill must land mid-write). The archive at the
  path always verifies, and the leftover workspace is recovered into it (saved
  back: not dirty, no error, archive rows equal the workspace's; at least one
  run must grow the archive) or refused with `RECOVERY_FAILED`. The writer is
  killed on drop and stops itself after 512 MB or two minutes.
- `src-tauri/src/durability_tests/newer_build.rs` and
  `tests/integration/newer-build-archives.test.tsx`: the newer-build fixtures
  match their checksums and behave as described above, through the manager
  and through the start screen (the refusal shows the update hint).
- `src-tauri/src/durability_tests/heavy.rs`: an archive just over
  500,000,000 bytes saves and reopens, the size report flags it, and the
  publish preflight blocks it. It is `#[ignore]`d, so run it explicitly:
  `IXTABLE_HEAVY_TESTS=1 cargo test --lib durability_tests::heavy -- --include-ignored`
  in `src-tauri` (about a minute in a debug build on four cores, and 1.5 GB of
  temporary disk). The Desktop workflow's `test` job runs it on every pull
  request and push on Windows, macOS and Linux through
  `scripts/ci/run-ignored-rust-tests.mjs`, which fails unless it ran and
  passed; under CI a missing `IXTABLE_HEAVY_TESTS` fails the test.

## Audit log

- 2026-10-05: Described the recovery outcomes the code has (`before-recovery`
  checkpoint, `RECOVERY_NEEDS_SAVE_AS`, `EXTERNAL_CONFLICT`, workspace lock)
  and added the matching `archive_io` and `recovery.rs` tests to Evidence.
- 2026-10-05 (after merging #36): re-checked the incremental-save text and evidence #36 added (`archive_io/reuse.rs`, `reuse_tests.rs`, `durability_tests/incremental.rs`, `Snapshot::reuse_from`, `DataStamp`); they match the code. No changes.
- 2026-10-09: config version 4 adds `SavedQuery.action` (action queries); version 3 configs upgrade on load.
- 2026-10-10: config version 5 adds `Form.navigationBar` and the `continuous` and `split` form modes (docs/decisions/form-views.md); version 4 configs upgrade on load.
