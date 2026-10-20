# Design language

Status: accepted (2026-10-14). Covers the rules every Studio and Runtime screen
follows, so that screens built at different times look and behave alike. It came
from a "Don't Make Me Think" audit of the whole app. It is built on gpui-kit's
theme tokens (`theme.primary`, `muted_foreground`, `border`, `danger`, `warning`,
`success`) and needs no new dependency. The shared helpers in
`ui/{dialog,display,toast,widgets,forms,controls}.rs` implement it; see `gpui-ui.md` for the
rest of the UI conventions.

Components marked *(planned)* are specified here but not built yet. Until they
exist, use the closest existing helper rather than a one-off style.

## 1. Principles

1. **Answer first.** Every screen leads with its state in one line: what this is, whether anything is wrong, and the next step. Details come after.
2. **Users speak Access, not SQL or CSS.** Use the words *table, field, record, form, report, query, macro*. Keep SQL, enums, error codes, UUIDs and CSS grid terms behind a "Details" disclosure.
3. **One component per job.** A new screen composes the shared components. A one-off style is a bug.
4. **Calm by default, loud only when it matters.** The UI is greyscale plus one accent color. Color carries meaning (selection, status, danger) and is never decoration.
5. **Nothing jumps.** Feedback never pushes content around abruptly. Use a toast, or let an inline message slide in (`widgets::slide_in`) so the form moves gradually. Don't reserve empty space for a message that may never come.

## 2. Type scale

Five steps, with weight used sparingly. Feature code sizes text only with `.ty(Type::X)` (`widgets::{Type, TypeScale}`, in the prelude), which sets size and weight; color stays with the caller. Raw `text_xs()`..`text_3xl()` calls are banned outside `widgets.rs` by an ast-grep rule. `Type::Display` (`text_3xl` semibold) is reserved for dashboard metric values and the start-screen wordmark. Zoom-scaled canvases and user-set report fonts keep `text_size(px(..))`.

| Step | Use | Size / weight |
|---|---|---|
| Title | Mode and page title (one per view) | `text_xl` semibold |
| Section | Card, dialog and panel-section titles | `text_base` semibold |
| Body | Content, inputs, table cells | `text_sm` regular |
| Label | Field labels, column headers, nav items | `text_sm` medium, `muted_foreground` |
| Caption | Hints, counts, timestamps, help | `text_xs` regular, `muted_foreground` |

- Monospace (`font_family("monospace")`) is only for code, SQL, hashes, paths and log lines, and is always `Body` or `Caption` size.
- Kicker and eyebrow labels ("PROJECT / DATA", "DASHBOARD SETTINGS") are `Caption`, uppercase, with tracking. Use at most one per view.

## 3. Spacing and layout

- **Spacing:** use the 4 px base gpui steps (`gap_1` 4 px, `gap_2` 8 px, `gap_3` 12 px, `gap_4` 16 px, `gap_6` 24 px):
  - inside a control group, `gap_1`
  - between fields, `gap_3`
  - between sections, `gap_6`
  - inside a card, padding `p_4`
- **Page frame:** a content column with `max_w` 960 px for forms, settings and dialogs. Tables, canvases and grids can go full-bleed. Prose and descriptions are capped at about 70 characters.
- **Properties panels:** a fixed 320 px width. Use a two-column grid for paired numbers (X/Y, W/H, min/max). Labels go above inputs, and every input in a panel has the same width.
- **Alignment:** everything is left-aligned. The only centered elements are empty states and dialogs. A centered link inside a left-aligned card (as in "What did not convert fully" and "Sign in to ixtable Cloud") is a bug.

## 4. Color

| Role | Token | Use |
|---|---|---|
| Text | `foreground` | Body, values |
| Secondary text | `muted_foreground` | Labels, captions, placeholders |
| Lines | `border` | Card borders, dividers, table rules |
| Accent | `primary`: the app icon's teal `#0DB9B6`, with navy `#10171D` text on it (`ui/theme.rs`); links use a darker teal `#0A7C7A` | Selection, focus ring, the one primary button, links |
| Success | `success` | Toasts, "Converted" and "Applied" badges |
| Warning | `warning`, darkened for text to ≥ 4.5:1 | "Partly converted", "Pending", non-blocking issues |
| Danger | `danger` | Errors, destructive buttons, "Not converted" |

- Status colors appear as a **tinted background plus an icon plus text**, never as text color alone. Use 12% opacity for the fill and the full color for the icon and title, with body text staying `foreground`.
- Selection (nav item, list row, tab) is an accent tint plus a medium-weight label: `.selected_tint(selected, cx)` (`widgets::SelectedTint`), never a bare `.selected()`, whose grey reads as hover or disabled.
- Disabled means 40% opacity of the enabled variant. Never use a separate grey fill, which reads as "pressed".
- The app icon (`assets::APP_ICON`, from `branding/app-icon.svg`) marks the Studio sidebar and the start screen. It stays on a white tile in every theme. There is no dark version.

## 5. Components

Each is one function in a shared UI module. **New** marks components that don't exist yet.

### Buttons (`forms::primary`, `secondary`, `danger`)
| Variant | Look | Rule |
|---|---|---|
| Primary | Filled accent | **At most one per view or dialog.** It is the action that completes the task. |
| Secondary | Outline | Everything else |
| Quiet | Ghost or link button | Toolbar actions in dense areas, inline "Show more" |
| Danger | Red text and red outline; filled red only in the final confirm | Delete, remove, drop, reset, roll back |

- A label is a verb plus an object: "Create table", "Delete dashboard", and the singular "New order", not "New orders".
- Add `…` only when the button opens a dialog or picker before acting. Apply this consistently: "Open document…".
- Icon-only buttons need a tooltip. Use **trash** for delete (never ⌫), circular arrows for refresh (never the magnifier), and ✕ only for closing a panel.
- Full-width buttons appear only in empty states and in narrow sidebars.
- A disabled primary button has a caption beside it saying why ("Choose a file first").

### Dialog (`ui::dialog::dialog`)
- A 45% scrim that blocks clicks to the page.
- Centered, at width S 480 px, M 640 px or L 880 px.
- A **fixed header** with the title and ✕.
- A **scrolling body** that shows a top or bottom shadow when it has more content.
- A **sticky footer** with the primary button on the right, Cancel to its left, and destructive actions on the far left.
- Esc and ✕ both mean Cancel, except while busy.
- Titles are questions for confirmations ("Apply 2 changes to orders?") and nouns for tools ("Import from Access").
- Never stack a second close button ("Close Tasks") outside the footer.
- Risky-change dialogs (the schema impact dialog) lead with warning callouts, then the facts, then one acknowledgement that names what is lost. Generated SQL sits behind a "Show SQL" toggle.

### Callout (`widgets::callout`, `widgets::error_banner`, `widgets::error_callout`)
- Tones: info, success, warning and danger, each with an icon, a bold one-line title, optional body text, and an optional action.
- Errors read *what happened. what to do.*, for example "**This file can't be opened.** It isn't an Access database or is damaged. Choose another file."
- The headline is plain language; the message follows; the error code is a muted "Error code X" line for support. A domain with its own codes (runtime bundles) passes its own headline to `error_callout`.

### Toast (`ui::toast`, shown with `Shell::toast` or `Shell::set_notice`)
- For transient success: "Exported orders.pdf · Show in folder" or "Added 2 records". `Toast::info` is for neutral notes such as "Save canceled.".
- Top-right, below the title bar, so it never covers dialog footers or pagers. It dismisses after 6 s, unless it has an action, in which case it stays 10 s.
- It never changes layout. A success message is never an inline line under a toolbar. Errors stay inline, next to what failed.

### DataTable (`display::data_table`, `display::text_table`; the datasheet stays its own grid)
- A header row in `Label` style. Text is left-aligned, and numbers are right-aligned with tabular figures.
- Zero values and "—" show in `muted_foreground`.
- `text_table` is the same table for text-only columns: every column shares the width and is left-aligned (page links).
- Row actions appear on hover or selection at the row end.
- Use it for report summaries, the migration list, problems, assets, archive size, the Access migration summary and the import inventory. Log lines keep the same row shape: time, level badge, area, then the message, which alone wraps.

### KeyValue (`display::key_value`, rows from `display::kv`)
- A two-column grid with a `Label` column and a `Body` value column.
- Hashes, fingerprints and paths use `.code()`: monospace, ellipsized, with a copy button.
- *(planned)* Paths show the file name with a "Show in folder" button. Toasts for exported files already carry it (`dialogs::reveal`).

### StatusBadge (`display::status_badge`)
- A pill with a tone (`Tone::{Neutral, Success, Warning, Danger}`) and `Caption` text.
- One fixed vocabulary per domain: Converted / Partly converted / Not converted; Pending / Applied / Failed / Changed after apply; Policy set / No policy; Saved / Unsaved changes / Not saved / Saving… / Save failed; log levels info / warn / error. Raw codes such as `dry_run_ok` never show.

### Section (`ui::section::section`; `controls::section` remains for fixed groups)
- Collapsible, with a `Section` title, an optional count, and a one-line summary when collapsed ("Primary key: id").
- Panels open with Basics expanded and Advanced collapsed. Optional builder sections (query Having, Group by, Sort; table Relationships, Constraints, Indexes) start collapsed, open once they have content, and then stay open.
- Open state is app-wide per section id, so a section the user opened stays open when its panel comes back.
- Field labels inside a section never repeat the section name ("Field", not "Group 1 field").

### Field
- The label (`Label` style) goes above the control, with "Optional" in the caption rather than "*" marks for required fields. Required is the default assumption.
- A hint goes below in `Caption`. An error replaces the hint in danger color, and the control gets a danger border.
- Validate on blur or submit. Never validate a field the user hasn't touched yet.

### EmptyState (`widgets::empty_state`, extend)
- An icon, a one-line title, one sentence, and at most one primary plus one secondary action, all sized normally.
- Every list, canvas and panel that can be empty has one:
  - Queries: "No queries yet. Create one."
  - Canvas: "Drag a field here."
  - Runtime: "Your app has no pages yet."

### Navigation and tabs
- Mode nav (sidebar): icon plus label. The selected item gets an accent tint and medium weight.
- Tabs: an underline style, with the selected tab in accent and semibold. Use the same component everywhere: Design/Preview, Design/View, settings and diagnostics.
- Beyond 6 tabs, use a grouped left sub-nav. Settings does this (`shell/settings.rs` `GROUPS`: Data, App, Release, Diagnostics), with a count badge on Problems.
- Ribbon (`ui::ribbon`): as in Access, a row of tabs over labelled command groups, at the top of the content column and never over the sidebar. Studio's tabs are Home, Create, External data and Database tools (`shell/ribbon.rs`); the Run App window has Home with Records and Run as. Commands are quiet buttons with an icon and a verb-free noun on Create ("Table", "Form"); the group name is a `Caption` under them. A page keeps its own toolbar for commands on what it shows (a datasheet's filter, a designer's properties); the ribbon holds the document-wide ones.
- Breadcrumbs appear only for drill-in (nested records, form → control). They replace "Back to form properties" links. The component is `widgets::breadcrumb`: earlier crumbs are link buttons (`<id>-<index>`), the last is the current item as plain text (`<id>-current`).

### Long-running work
- Use a determinate progress bar with the current step ("Copying Orders: 12,000 of 80,000 rows") wherever the backend reports progress, and an indeterminate bar with a step name otherwise. Never use static "this may take a while" text.

## 6. Words

- **Glossary, one word per concept:**
  - *document* is the `.ixt` file (not project, archive or database)
  - *app* is what end users run
  - *table, field, record, query, form, report, dashboard, macro*
  - *runtime bundle* is the `.ixtr` file
- Use Access words over builder jargon: "Column widths" and "Row heights" (not tracks), "Flexible (share)" and "Fixed size (px)" (not fr and px), "Areas" (not named regions), "Field in the related table" and "Matches field in this form" (not foreign key and parent column).
- Write singular entity names in actions ("New order", "Add item", via `widgets::singular`), with correct plurals everywhere (`widgets::plural(n, "table")` gives "1 table", "1,204 records").
- Use sentence case for all labels and column headers. Humanize raw field names in runtime headers (`product_name` becomes "Product name").
- Dates are relative within 7 days ("5 min ago") and local date-times after that. Never show ISO nanoseconds.
- Don't use marketing copy inside the app. The start-screen hero line is optional for returning users.

## 7. Enforcement

- `test/ast-grep/rules/type-scale-not-raw-text-size.yml` fails a raw
  `text_xs()`..`text_3xl()` call in `src-tauri/src/ui/` outside `widgets.rs`.
- `test/ast-grep/rules/destructive-buttons-use-danger.yml` fails a delete, remove,
  drop, reset or roll-back action built with `forms::primary` or
  `forms::secondary`.
- Golden tests (`ui::golden`) are the visual regression net while screens move to
  the shared components.
- Screenshot specs carry visual expectations. Each new shared component gets a
  capture, so the specs double as a style guide.
