# Chaos testing: form designer (`src-tauri/src/ui/design/`)

Driven headlessly through `ui::testing::Harness` on the real `AppView` and backend.
Probes are in `src-tauri/src/ui/design/tests_chaos.rs`; screenshots come from `src-tauri/src/ui/screenshots/chaos.rs` and are saved in `shots/`.

## Findings (most severe first)

### 1. HIGH: a control added after its tab page is removed or undone is orphaned
- Repro: add Tabs → click Tab 2 → select the tab group → Remove tab 2 → add Label.
  A second path: add Tabs → Add tab → click Tab 3 → Undo → select the group → add Text.
- Result: the control is stored with `parent.tab` set to a page that no longer exists. The designer draws it on a phantom page (no tab highlighted) until you click a real tab. After that it is on no canvas, and the Runtime never shows it. The Container picker shows `Missing (01a19371-…)`, a raw UUID.
- Cause: `DesignStudio.active_tabs` is never pruned. `target_container` (`forms/operations.rs`) trusts it, and `structural()` does not check that `parent.tab` exists.
- Shots: `chaos-01a-control-on-removed-tab.png`, `chaos-01b-control-unreachable.png`.
- Options:
  - A: in `target_container`, ignore an active tab that is not in `selected.tabs` and fall back to the first page.
  - B: A, plus prune `active_tabs` on `StoreEvent::Changed`, so the canvas never renders a removed page.
  - C: B, plus a structural or Problems check for an unknown `parent.tab`, and a "Move to Tab 1" fix.
  - **Recommendation: B.** It fixes the cause and the phantom render. Add C's Problems check later for documents that already have orphans.

### 2. MEDIUM: one Undo reverts several distinct adds
- Repro: click Add Text 3 times within 1 s, then Undo once. All 3 controls are gone (shots `chaos-02`, `chaos-03`).
- Cause: `ConfigStore` merges edits by label alone (`COALESCE`, 1 s). That suits typing, but not discrete actions such as "Add control", "Add tab" (labelled "Edit control") or "Duplicate".
- Options:
  - A: give discrete actions a non-coalescing path (`update_discrete`, or `coalesce: bool`).
  - B: coalesce on label plus target key (control id and field), so only repeated keystrokes to the same field merge.
  - C: drop time-based coalescing and commit inputs as drafts on blur or Enter.
  - **Recommendation: A.** It is the smallest change and leaves the typing-merge behaviour alone. B is a follow-up if label collisions across controls show up.

### 3. MEDIUM (plausible): Undo while a live field has focus is re-applied by the next keystroke
- Repro: select a control → type "A" in Label → Undo (store) while the input keeps focus → type "B". The stored label becomes "New text fieldAB", so the undo was lost silently.
- Cause: `Fields::get` does not resync a focused input. Headless tests do not deliver blur, so confirm in the real app whether clicking the ribbon's Undo moves focus.
- Options:
  - A: on `StoreEvent::Changed` from undo or redo, force every field to resync even when focused.
  - B: blur the focused input before undo or redo.
  - C: route Ctrl+Z inside a field to the input's own text undo, and use the ribbon only for store undo.
  - **Recommendation: A.** It is deterministic and does not depend on the platform's focus behaviour.

### 4. LOW: duplicate default form names
- Repro: New form ×2 → delete "Form 2" → New form. You get "Form 3" twice (`chaos-04-duplicate-form-names.png`).
- Cause: `create_form` names the form with `forms.len() + 1`.
- Options:
  - A: pick the lowest free "Form N".
  - B: pick max(N) + 1.
  - C: A, plus a Problems warning for duplicate form names.
  - **Recommendation: A.**

### 5. LOW: hostile form names are stored with no feedback
- `""`, `"   "`, an RTL override (`\u{202e}`), NUL and tab characters are all accepted as form names, and no error banner appears. A newline is stripped. Names round-trip through save and reload unchanged.
- I did not check whether Problems flags an empty name.
- Options:
  - A: trim the name and reject empty, showing a draft error like the grid inputs do.
  - B: A, plus strip control and bidi-override characters.
  - C: allow anything, but warn in Problems.
  - **Recommendation: B.**

### 6. LOW: a tab group can have zero pages
- Removing every page leaves an empty group. Adding while it is selected falls back to the form root, so it is harmless but confusing.
- Options:
  - A: hide Remove when only one page is left.
  - B: ask for confirmation, then delete the group.
  - C: leave it as is and show an empty-state hint.
  - **Recommendation: A.**

## Held up under chaos (no defect found)
- Monkey: 5 seeds, 1,550 steps in total. Each step was a random click or drag in the studio, an Alt or Alt+Shift arrow, Enter, Esc, Tab, Ctrl+Z or Delete, a palette add, hostile text, undo or redo, or Delete control. After every step the stored design still passed validation and had no orphans, with no panics and no error banners.
- An undo/redo storm (30 steps) after deleting the open form while it was previewed, then redo-all, left the design valid.
- Save and reload of an `.ixt` holding sections, tabs and a Unicode/RTL name produced an identical design.
- Bound table: a backend column rename shows `Missing (name)` in the column picker. Dropping the table leaves the preview safe, and Problems lists "uses table items, which does not exist". Bindings not following renames is a known limitation that the change-plan warning documents.
- Expressions: the parser enforces its depth and length caps (100 / 10k), so unbalanced, unterminated or deeply nested input did not panic.
- Typing a min of 50 then a max of 100 stores both correctly. The transient "1" < 50 is rejected silently, with no banner.
- Switching modes mid-typing keeps the half-typed label, and no keys leak afterwards.

## Notes and limits
- In debug headless runs a keystroke costs about 70 ms. Profiling showed this is GPUI layout of the 1500×4200 test window, one frame per keystroke. The backend write is about 2.5 ms. This is not an app issue.
- I did not cover tiny windows: the start screen's New document button is not visible below about 700×420, and the harness cannot resize a window.
- `cargo screenshot <filter>` ORs the filter with `ui::screenshots`, so it runs every spec. To run one, call the test binary with `--exact … --ignored`.
