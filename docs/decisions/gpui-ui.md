# Native GPUI user interface

## Context

The Studio and Runtime UI was a React app in a Tauri webview. Its logic was
split across two languages: commands lived in Rust, while expressions,
trigger hooks, autosave and undo history lived in TypeScript. Tests needed a
NAPI bridge so that Vitest could reach the Rust commands.

## Decision

The UI is native Rust, rendered with GPUI through
[tauri-plugin-gpui](https://github.com/Ziinc/tauri-gpui) and built from
[gpui-kit](https://crates.io/crates/gpui-kit) components. No JavaScript
frontend remains, and Tauri hosts no webview.

- `src-tauri/src/ui/` holds every view. `ui::launch` initializes the plugin
  and opens the `main` window with `AppView` at its root.
- UI code calls the command functions directly. A long call runs on a
  background thread through `ui::task::spawn_call` and updates the view when
  it returns. Commands carry no `#[tauri::command]` attribute, and
  `generate_handler!` is gone.
- Logic that used to be TypeScript-only is now Rust:
  - `src/expr` became `expr/` (see `expression-language.md`).
  - `src/lib/records.ts` became `records.rs`, which holds the hooks, the
    router, batches and action queries.
  - The config store with undo/redo became `ui/store.rs`.
  - Autosave became `ui/autosave.rs`.

## Conventions

- **Registries:**
  - Modes: `ui/shell/modes.rs` (`build_view`, `build_sidebar`).
  - Settings tabs: `ui/shell/settings.rs` (`build_tab`).
  - Start screen actions and panels: `ui/start_sections.rs`.
  - Extra Lucide icons: `ui/assets.rs`. gpui-kit embeds only about 100 icons by default.
- **Definition edits** go only through `ConfigStore::update(label, mutate, cx)`.
  Edits with the same label within one second are merged into one undo step.
- **Record writes** go only through `records::{insert, update, delete,
  write_batch, run_action_query}`. They block, so they are called inside
  `spawn_call`. The caller then announces the change with
  `Shell::records_changed`.
- **Change notices:**
  - DDL changes announce through `Shell::database_changed`.
  - Record changes announce through `Shell::records_changed`.
  - Both go out over the `DocBus` entity.
- **Dialogs** go through `ui::dialogs` (`pick_file`, `save_file`, `confirm`, and
  so on). The `TestDialogs` provider queues answers for tests.
- **Imports:** UI files import `ui::prelude::*`.
- **Element ids:** every interactive element gets a stable kebab-case
  element id. Tests find elements by these ids.
  - Tabs from `widgets::tab_strip` use `tab-<key>`.
  - Mode buttons use `mode-<key>`.
- **Text that tests read** is rendered through `widgets::text` or
  `widgets::styled_text`. These register the element with its string as an
  accessibility label.
  - A custom `div` that tests must find ends its builder chain with
    `.test_support()`.

## Tests

Every former `tests/integration/*.test.tsx` flow is a `#[gpui_kit::test]` in
a `tests.rs` beside the view it covers. These tests drive the UI through
`ui::testing::Harness`, which wraps a headless `TestAppContext` over a real
session. Unit tests of former TypeScript modules live with their Rust ports.

Two rules apply to test modules:

- Test modules import names explicitly and do not glob-import `gpui_kit::*`.
  That glob shadows `#[test]`.
- Button `disabled` and `selected` states do not appear in snapshots, so
  tests assert on the model state instead.

Run the tests with `cd src-tauri && cargo test --lib`.

## Dashboards

`dashboard_engine/` holds the non-UI logic of the former `src/dashboards/` (model,
filter-to-parameter mapping, KPI and table slices, chart figures, run state). Views live in
`ui/dashboards/`. Charts reuse `report_engine`'s chart geometry and drawing through
`chart_item`, so a dashboard chart and a report chart cannot drift apart. Embedded forms are a
detached `ui::runtime::FormRenderer`; query pickers are `ui::query::QueryPicker`. The Runtime
shows a dashboard through the `PageBuilders` hook that `ui::dashboards::register` installs.

Differences from the React app:

- A chart has a fixed 250 x 140 pt box (zoom 1.5), not a measured one. SVG tooltips become
  labelled elements (`chart-row:*`).
- The grid settings are columns, padding, gaps and alignment. Tracks, named regions and
  breakpoints are edited in YAML until the Design studio's layout editor is shared.
- The Runtime list of a form restarts, and clears search and sort, when dashboard
  parameters change.
- A set-state step in a dashboard button shows a notice, since there is no form to hold state.
- Queries run through `QueryRunner` (a global that tests replace) and are polled on a 50 ms
  timer, so tests step the clock in 50 ms ticks.

## Status

Accepted (2026-10-13).
