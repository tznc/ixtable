# Design system

Status: accepted (2026-10-22). Visual direction (Zn, 2026-10-23): **friendly grid on a
native base**. Each OS keeps its own look, and the grid character (gridlines, a tinted
header row, rounded status pills) comes from the components. The Krug walkthrough of every
component lives in the design system doc shared in the project.

`design-language.md` says what ixtable screens should look and sound like. This record
covers how that is codified: the `ixtable-design` crate, its components, and the
styleguide that proves them.

## Decision

- **Primitives come from [gpui-bootstrap](https://github.com/Ziinc/gpui-bootstrap).** It
  provides buttons, alerts, badges, modals, form controls and navs on top of gpui-kit,
  with one token `Theme`. We re-theme it; we don't fork it.
- **App components live in `src-tauri/crates/design` (`ixtable-design`).** Each one is
  composed from Bootstrap primitives and does one job (design-language.md principle 3).
  Screens use these components. They reach for a Bootstrap primitive only when no
  component fits, and then the gap goes on the list below.
- **The styleguide is the spec.** `examples/styleguide.rs` (gpui-styleguidist) renders
  every component with knobs and named states, on every desktop platform in light and
  dark. Snapshots are not committed (Zn, 2026-10-23). Visual QA happens on the published
  styleguide artifact, and CI renders every example and uploads the PNGs as a build
  artifact.

## Principles (Don't Make Me Think)

Each rule is enforced by a component's API, so it doesn't rely on reviewers.

| Rule | Where it is enforced |
|---|---|
| Every screen answers "where am I, what is this, what can I do" first | `PageHeader`: breadcrumb, Title, one-sentence description, actions |
| One obvious next step | `ActionButton` has four emphases; Primary is the only filled one, and the guide shows one per surface |
| Never a dead control | `ActionButton::disabled_because(reason)` is the only way to disable it, and the reason is shown under the button |
| Color means something | `Tone` (neutral, info, success, warning, danger) is the only color input to `Callout`, `Toast` and `StatusBadge` |
| Lead with the answer | `Callout` and `Toast` take a title first. Technical text goes in `details`, never the title |
| Empty is a state, not a blank | `DataTable::empty(EmptyState)` names what goes there and the one action that fills it |
| Required is normal | `Field` marks only the exceptions as "Optional"; errors replace the hint and say how to fix the value |
| Nothing jumps | `Toast` is drawn bottom left by the shell, over the foot of the sidebar and within its width (Zn, 2026-10-23); `Section` collapses in place; inline messages slide in (`widgets::slide_in`) |
| Tabs are not buttons or links | `Tabs` mark the active tab with the teal (underline, pill tint or a raised segment); inactive tabs are muted body text |
| Everything reads | Every text color a component draws reaches 4.5:1 on its fill in all six themes (`contrast_audit.rs`), except white on the teal primary |
| No raw sizes | `Type` has six roles (`.ty(Type::Title)`); the crate never calls `text_sm` and friends |

## Tokens

`ixtable_design::theme::theme(platform, mode)` starts from gpui-bootstrap's platform theme
(macOS, Windows/Fluent, or Linux/libadwaita). That theme supplies the system font, corner
radii, control sizes, and status and surface colors. `native(mode)` picks the OS the app
runs on. The theme then sets:

- **Accent:** primary `#0DB9B6` (app icon teal) with white text. White on teal is
  2.4:1, below WCAG AA. Zn chose it for the brand anyway, so `min_contrast_ratio` is
  2.2. Warning and info fills stay below that threshold and keep dark text (tested).
- **Links:** `#0A7C7A` on light (4.5:1) and `#5FD8D5` on dark.
- **Dark:** brand navy `#10171D` is Bootstrap's `dark`.
- **Type:** 14px base, 12px small and 16px large, on every platform. Headings are
  30/24/20/16/20/12 and semibold, which matches `Type`: h5 is Bootstrap's dialog and
  card title, so it is `Type::Title`. The font family stays the platform's own.
- **Readable color:** `theme::readable(color, bg, 4.5, toward)` darkens (or, on dark,
  lightens) a tone only as far as 4.5:1 needs. The field error and danger buttons use
  it, so they stay red instead of Bootstrap's brown "emphasis" color.
- **Info tone:** this uses primary (teal), not Bootstrap's cyan, so the app has one
  cool accent.

Use `ixtable_design::change_mode`, not `Theme::change_mode`. Bootstrap's mode switch
resets link colors.

## Components

| Component | Built on | Job |
|---|---|---|
| `ActionButton` (+ `Emphasis`, `ButtonLook`) | `Button` | Any labelled action: Primary, Secondary (hairline border, body text), Quiet (link), Danger (red hairline, filled only with `.confirm()`). `ButtonLook` (subtle, soft, faint, raised, outline) is the secondary look; subtle ships, the others are in the guide for comparison |
| `PageHeader` | `Breadcrumb` slot | Screen title, state sentence, actions |
| `Callout` | `Alert` | Inline message about this screen. Neutral sits on the lightest page fill, not Bootstrap's grey tint |
| `Toast` | `CloseButton` | The result of an action, `TOAST_WIDTH` wide so it fits the sidebar (the shell places it, the app times it) |
| `StatusBadge` | `Badge` (subtle, pill) | A state in one or two words |
| `Field` | none | Label, Optional, control, hint or error |
| `Section` | none | Titled, optionally collapsible group with a closed-state summary |
| `KeyValue` | none | Facts about one thing |
| `DataTable` (+ `Column`, `Align`) | none | Read-only record list with selection, footer count, and empty state |
| `EmptyState` | none | What goes here and how to add the first one |
| `Tabs` (+ `TabStyle`) | none | Views of the same thing. Underline for a page's views, pills for filters (with `.counts`), segmented for two to four modes; `.vertical()` for settings sections |

Composites put the building blocks together into whole regions of the screen:

| Composite | Job |
|---|---|
| `AppShell` | Sidebar + Ribbon + PageHeader + content, with toasts bottom left over the sidebar. Passes the trunk test on any screen |
| `Ribbon` (+ `RibbonGroup`, `RibbonCommand`) | Tabs over labelled command groups. Run App is the one primary. A disabled command shows why in its tooltip |
| `Sidebar` (+ `SidebarGroup`, `SidebarItem`) | Objects by kind with counts. The open object is tinted. Each group ends with "New …" |
| `Dialog` (+ `DialogSize`) | One decision: Cancel plus exactly one primary, or a filled danger confirm |
| `RecordForm` | One column of `Field`s and one Save. A failed save shows a count callout |

`PageHeader`, `Ribbon`, `Dialog` and `RecordForm` take the primary action in its own slot
(`.primary(...)`). A primary passed to `.action(...)` is shown as secondary, so a surface
can't end up with two primaries.

Element ids follow the app rule: the id you pass, plus fixed suffixes for parts that
tests read (`<id>-title`, `<id>-reason`, `<id>-error`, `<id>-row-<n>`).

Bootstrap's `Modal`, `FormControl`, `Checkbox`, `Radio`, `Switch`, `Spinner`, `Progress`
and `Pagination` are used directly through `ixtable_design::prelude`.

## Commands

From `src-tauri/`:

| Task | Command |
|---|---|
| Tests | `cargo test -p ixtable-design` |
| Open the styleguide | `cargo run -p ixtable-design --example styleguide` |
| Render PNGs to `crates/design/styleguide/` (gitignored; CI does this) | `cd crates/design && cargo run -p ixtable-design --example styleguide -- render` |
| Web styleguide for QA (wasm) | `cargo styleguide build -p ixtable-design --profile styleguide` writes `target/styleguide/ixtable-design/` (needs `rustup toolchain install nightly --target wasm32-unknown-unknown`, `wasm-bindgen-cli` at the Cargo.lock version, and `cargo-styleguide` from gpui-styleguidist). Publish that folder for QA |

The web build compiles the few Lucide icons it uses into the wasm
(`examples/icons/`), because gpui-kit would otherwise fetch them over HTTP.

Renders cover every desktop platform in light and dark (`macos-light` through
`linux-dark`), so 6 themes for each example. CI renders on Linux, so the platform font falls
back to the CI font there, while colors, radii and sizes match each platform. Rendering
needs a Vulkan driver (`mesa-vulkan-drivers` on Linux; CI installs it).

## Rollout

1. This change adds the crate, the styleguide, and the CI render. The app doesn't depend
   on the crate yet, so screens are unchanged.
2. The next change adds `ixtable-design` to the app and replaces `ui::theme::apply` with
   `ixtable_design::init`. That moves gpui-kit's theme onto the native platform tokens.
3. After that, screens migrate one area per PR, in this order: runtime, data, design,
   automation, settings and cloud. Each PR replaces `widgets::callout`,
   `display::data_table`, `forms::primary` and the like with the components, and works
   through `/mnt/project-files/qa/pr-design-7/drift-findings.md`. The old helpers are
   deleted once nothing uses them.

## Gaps (add a component before a screen needs a one-off)

- Datasheet (stays an app widget; it is an editable grid, not a `DataTable`).
