# Continuous, split and popup forms

## Status

Accepted. Covers PRD §14 and Phase 6: continuous and split form modes, the record
navigation bar, and popup forms opened by an action that return values to the caller.

## Context

Access users expect forms that show many editable records at once (continuous forms),
a datasheet with the current record under it (split forms), first/previous/next/last/new
buttons on a record, and modal dialogs that hand a value back to the code that opened
them. Phase 6 requires these to reuse the shared grid and form primitives (§13), to read
through DuckDB, and to write through the RecordStore so triggers run.

## Decision

- **Modes.** `FormMode` gains `continuous` and `split` (`src/design/schema.ts`,
  `src-tauri/src/design/mod.rs`). They are opt-in: `DEFAULT_FORM_MODES` and Rust
  `default_modes()` keep the four classic modes, so stored forms are unchanged.
  `checks.rs` warns when a form without a source offers either mode.
- **Continuous** (`src/runtime/ContinuousView.tsx`, `ContinuousRow.tsx`). One page
  from `loadPage`, the list reader, with each record drawn by `ControlGrid` on the
  form's own layout. A row keeps its own record, errors and form state, validates with
  `validateForm`, and writes with `src/runtime/rowWrites.ts`, which mirrors
  `RecordView.save` (computed inputs written, disabled fields kept, `expected` values
  for optimistic entities) and goes through `src/lib/records.ts`. A changed row saves on
  Save, Enter, or when focus leaves it. A save re-reads the page. Rows render with
  `embedded`, so related lists stay one level deep.
- **Split** (`src/runtime/SplitView.tsx`). `ListView` with a `selected` row and a
  `refresh` counter above a `RecordView` of the detail form. Saving or deleting in the
  pane bumps `refresh`.
- **Navigation bar** (`src/runtime/RecordNavBar.tsx`, `cursor.ts`). `DesignForm.navigationBar`
  turns it on; `newForm()` and the new-document form set it, stored forms without the
  field keep it off. `ListView.onOpen` passes a `RecordCursor` (list form, absolute index,
  sorts, search filters, params) that `FormStack` keeps on the view. `recordAt` reads
  the record at an index with a one-row `loadPage` in the same order, so the bar walks
  what the list showed. Without a cursor (a record opened directly) only first, last
  and new work. The bar shows in detail mode and is disabled while the record is dirty.
- **Popup forms** (`src/runtime/popup.ts`, `PopupHost.tsx`, `src/automation/runner.ts`).
  `openForm` takes `popup` and `storeAs`. The runner awaits `ctx.openPopup`, else
  `requestPopup`, a cancelable `ixtable:open-popup` window event that Run mode's
  `PopupHost` answers. The host renders an embedded `FormRenderer` in a modal
  `DialogFrame` and resolves with the record `RecordView.onSaved` reports, with a
  `closeForm` step's value (`ixtable:close-form`, topmost popup), or with null on
  dismiss. With no host the form opens as a page and the result is null. A popup
  opens immediately even in a rollback-mode action, since later steps need its value;
  `closeForm` is an effect and waits for the commit like navigation.

## Consequences

- Continuous rows remount after any save, so unsaved edits in other rows are dropped;
  leaving a changed row saves it first, which keeps this rare.
- The bar re-reads one row per move through DuckDB. Cached pages make repeat moves cheap.
- Only modal popups exist. Non-modal floating windows are out of scope.
- Saves inside a popup are not part of the opener's rollback transaction.

## Evidence

- `tests/integration/form-views.test.tsx`: continuous edit, add, delete; split selection,
  bar stepping, refresh after save; bar order after sorting; popup create return value,
  `closeForm` value, dismiss returns null.
- `tests/unit/form-views.test.ts`: runner popup and `closeForm` steps, event fallbacks,
  rollback hold-back, schema defaults and upgrade.
- `src-tauri/src/design/tests.rs` (`continuous_and_split_modes_round_trip_and_need_a_source`)
  and `src-tauri/src/automation.rs` (`accepts_popup_open_form_and_close_form_steps`).
