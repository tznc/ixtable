# Field formats: rich text, attachments, multiple choices, and input masks

Status: accepted. Implements the PRD Phase 6 item "attachment, rich-text memo,
multi-select fields, and input masks". Extends the
[type matrix](./recordstore-type-matrix.md) without adding logical types, and
narrows PRD §18's "record-linked attachments are deferred".

## Context

Access users expect four field kinds that ixtable did not have: a memo that
keeps bold, italics, and lists; an attachment field that holds files per
record; a field that holds several choices from a list; and an input mask
such as `(000) 000-0000` that shapes what is typed.

Logical types are the contract between the RecordStore, DuckDB, and the
designer. A new logical type would need a physical type on both stores, a
DuckDB read mapping, a CHECK constraint on SQLite, and a table rebuild to
change. None of the four kinds needs new storage semantics: they are text or
JSON with rules about entry and display. Attachment bytes are the exception,
and they must follow the same write and read paths as records.

## Decision

**Field settings sit on the entity, keyed by column.** `EntitySettings.fields`
(`src-tauri/src/recordstore/model.rs`, `src/fields/types.ts`) holds one
`FieldSettings` per column: a stable `id`, the `column`, an optional `format`
(`richText`, `attachment`, `multiSelect`), an optional `inputMask`, and the
`options` of a multi-select. The table designer edits them in its Field
settings section (`src/fields/FieldSettingsEditor.tsx`) as ordinary definition
edits; they never change the column. `commands::alter_table` renames a field's
column with the column and drops it with the column (`follow_columns`).
`recordstore::validate` flags a duplicate column, an unknown format, and a
multi-select without choices.

| Format | Column logical type | Stored value |
|---|---|---|
| `richText` | `text` | sanitized HTML subset |
| `attachment` | `text` or `json` | JSON array of `{id, name, mime, size, sha256}` |
| `multiSelect` | `text` or `json` | JSON array of chosen strings |
| input mask | `text` | typed characters only, or with literals when the mask's second section is `0` |

**Forms inherit from fields.** Three control kinds, `richText`, `attachment`,
and `multiSelect`, join `ControlKind` in Rust and TypeScript, and a control has
an optional `inputMask`. The generator picks the kind and mask from the field
settings. At run time `withFieldDefaults` (`src/fields/defaults.ts`) upgrades a
plain text control bound to a formatted field to that kind, fills a text
control's mask from the field when it has none, and fills a multi-select's
choices. A mask set on the control wins.

**Rich text is sanitized twice.** `sanitizeRichText` (`src/fields/richtext.ts`)
keeps `p`, `br`, `div`, `b`, `strong`, `i`, `em`, `u`, `s`, `ul`, `ol`, `li`,
`h1` to `h3`, `blockquote`, and `a` with an `http`, `https`, or `mailto` link.
It drops every other attribute, and drops `script`, `style`, and embedded
content with their text. The editor sanitizes what it emits, and the read-only
view sanitizes again before rendering, because a value can also come from an
import, an action, or another PostgreSQL client. Lists, the datasheet,
reports, and the required check use `richTextPlain`.

**Attachment bytes live in the record store.** `record_attachments.rs` writes
each file through the session's `RecordStore` into the hidden
`_ixtable_attachments` table (BLOB on SQLite, BYTEA on PostgreSQL), created on
first use with `execute_internal`. Binds are text on both stores, so content
travels as hex (`unhex` on SQLite, `decode(..., 'hex')` on PostgreSQL). Reads
go through DuckDB like every other read and check the SHA-256 recorded at
upload. The commands are:

- `upload_record_attachment`: needs create or update permission on the table,
  refuses a column that is not an attachment field, caps a file at 20 MB, and
  cleans the file name with `assets::safe_file_name`.
- `read_record_attachment` and `save_record_attachment`: need read permission
  on the table the file was uploaded for.
- `remove_unused_record_attachments`: Studio only. Deletes files no attachment
  column refers to, keeping files younger than an hour because an open form may
  not have saved them yet.

**Masks follow Access syntax.** `src/fields/mask.ts` parses the three
sections (pattern, store literals, placeholder) and the slot characters
`0 9 # L ? A a & C`, case markers `> <`, `\` escapes, and quoted literals. It
skips `!` and does not support the `Password` mask. Like Access, a mask
governs entry only: forms check it in `validateControl`, and the datasheet
checks it before the logical type (`datasheetValue`). Writes from actions,
imports, and other clients are not checked.

## Consequences

- No DDL, table rebuild, or new DuckDB mapping is needed to adopt a format, and
  an older build that does not know `fields` drops them without breaking the
  table.
- Attachments travel with embedded SQLite data, so a runtime installation's
  files stay with its records through definition updates, and PostgreSQL
  users share them. They count toward the `.ixt` archive size for SQLite apps.
- A file uploaded from a form that is then cancelled stays stored until
  cleanup runs. Cleanup is a manual Studio action.
- The datasheet shows formatted fields as text and edits them only in forms.
- A query-sourced report sees the stored HTML or JSON. Only a table-sourced
  report turns formatted fields into text (`fieldTextRows`).
- Expressions see the stored value: HTML for rich text and JSON text for
  attachments and multiple choices.

## Evidence

- `src-tauri/src/record_attachments_tests.rs`: hex and checksum, the stored
  bytes on SQLite, and store, read, checksum, and unused-file detection through
  DuckDB on both stores (`postgres::` variants run in the PostgreSQL job).
- `src-tauri/src/recordstore/fields_tests.rs`: serde shape, column renames and
  drops, and validation.
- `tests/unit/field-mask.test.ts`, `tests/unit/field-formats.test.ts`,
  `tests/unit/field-inputs.test.tsx`: masks, sanitizing, value parsing,
  inheritance, generation, validation, and the inputs.
- `tests/integration/field-formats.test.tsx`: upload and read through the
  bridge, setting formats in the table designer, entering each kind in a
  generated form, and the datasheet mask check.
