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

**Rich text is sanitized twice.** `sanitize_rich_text` (`forms/richtext.rs`)
keeps the elements Markdown writes: `p`, `br`, `div`, `b`, `strong`, `i`, `em`,
`u`, `s`, `del`, `code`, `pre`, `ul`, `ol` (with `start`), `li`, `h1` to `h6`,
`blockquote`, `hr`, `table`, `thead`, `tbody`, `tr`, `th` and `td` (with
`align`), a disabled `input type="checkbox"` for task lists, `a` with an `http`,
`https`, or `mailto` link, and `img` with an `http`, `https`, or `attachment:`
source plus `alt` and `title`. `code` keeps a `language-*` class. It drops every
other attribute, and drops `script`, `style`, and embedded content with their
text. The editor sanitizes what it emits, and every view sanitizes again before
rendering, because a value can also come from an import, an action, or another
PostgreSQL client. Lists, the datasheet, reports, and the required check use
`rich_text_plain`.

**The GPUI editor edits rich text as Markdown.** GPUI has no content-editable
box, so the form's rich-text control (`ui/runtime/field_richtext.rs`) shows the
stored HTML as Markdown and renders the result in a preview under it.
`forms/richtext_markup.rs` converts both ways. Markdown to HTML parses
CommonMark with the GitHub extensions (tables, task lists, `~~strike~~`, bare
links) through the `markdown` crate, which gpui-kit's text view already builds.
Footnotes and single-tilde strike are off. A single line break inside a
paragraph stays a line break, as people expect in a text box. Underline has no
Markdown syntax, so it is written `<u>…</u>`, which Markdown passes through as
inline HTML. HTML to Markdown builds a tree from the sanitized HTML and writes
it back, escaping text that would read as Markdown, so a stored value survives
being opened and saved unchanged. The toolbar edits the Markdown around the
selection (`forms/richtext_edit.rs`): bold, italic, underline, strike, code,
heading, quote, bulleted, numbered and task lists, link, image, table, code
block and divider. The stored value is still a sanitized HTML subset, so other
readers do not change. Values saved by the earlier line-per-paragraph editor
open as Markdown with a blank line between paragraphs.

**Rich-text images reference the record's attachments.** Zn chose this over
embedding images in the value (2026-10-20). An image is
`![description](attachment:<file name>)`, matched by file id first, then by
name, against the files of every attachment field of the same record. Pasting
an image (or a copied image file), or the Image button, uploads it to the
form's first attachment field, adds it to that field's value and inserts the
link at the caret. A pasted image is named `pasted-image-<date>-<time>.png`. A
form without an attachment field has no Image button, and a paste explains that
one is needed. Web images (`http`, `https`) are allowed too. The preview loads
attachment images through `read_record_attachment` and downloads web images
itself (10 MB cap, image bytes only), because the embedded GPUI app has no HTTP
client for its image loader. Both reach the text view as data URLs. Until an
image has loaded, or when its file is gone, the view shows its description.
Removing the file from the attachment field leaves a link that shows only its
description.

**The attachment control adds, saves, and removes files.** Add file uploads a
picked file through `upload_record_attachment_file`. Save writes a copy through
`save_record_attachment`. Remove drops the file from the field's JSON array, and
its bytes stay stored until cleanup, like a cancelled upload. A read-only
control offers Save only.

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
  Its edits, inserts, multi-cell paste and find and replace all parse through
  `datasheetValue`, so a mask applies to each of them, and find matches the
  masked or formatted text a cell shows (`src/data/sheet/rows.ts`, `find.ts`).
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
  generated form, the datasheet mask check, and masked and formatted cells in
  the datasheet.
- `tests/unit/datasheet-fields.test.ts`: masks and formats in datasheet
  edits, paste, and find.
- `src-tauri/src/forms/richtext_markup_tests.rs`, `richtext_edit_tests.rs` and
  the tests in `richtext_images.rs`: Markdown to HTML for every construct, safe
  link and image targets, HTML to Markdown with escaping, round trips both ways,
  toolbar edits, and attachment image lookup.
- `src-tauri/src/ui/runtime/field_richtext_tests.rs`: toolbar blocks, a picked
  and a pasted image stored in the attachment field and linked, the paste error
  without an attachment field, and the preview loading a record's image.
