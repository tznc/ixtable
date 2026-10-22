# Design system

Status: accepted (2026-10-22). The visual direction (density, surfaces, corners) is
pending Zn's pick from eight options; the structure below doesn't depend on it.

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
  every component with knobs and named states, in light and dark. The PNGs under
  `crates/design/styleguide/` are committed, and CI re-renders and compares them.
  A visual change shows up in the PR diff as changed snapshots.

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
| Nothing jumps | `Toast` is drawn top right by the shell; `Section` collapses in place; inline messages slide in (`widgets::slide_in`) |
| Tabs are not buttons | `Tabs` is Bootstrap's underline nav |
| No raw sizes | `Type` has six roles (`.ty(Type::Title)`); the crate never calls `text_sm` and friends |

## Tokens

`ixtable_design::theme::theme(mode)` starts from Bootstrap 5.3 (`Platform::Web`) and sets:

- **Accent:** primary `#0DB9B6` (app icon teal) with white text. White on teal is
  2.4:1, below WCAG AA. Zn chose it for the brand anyway, so `min_contrast_ratio` is
  2.2. Warning and info fills stay below that threshold and keep dark text (tested).
- **Links:** `#0A7C7A` on light (4.5:1) and `#5FD8D5` on dark.
- **Dark:** brand navy `#10171D` is Bootstrap's `dark`; the dark surfaces are navy-tinted.
- **Type:** 14px base, 12px small, 16px large, 1.45 line height. Headings are
  30/24/20/16/14/12, which matches `Type`.
- **Controls:** medium padding is 5×12px, a little denser than Bootstrap's.
- **Info tone:** this uses primary (teal), not Bootstrap's cyan, so the app has one
  cool accent.

Use `ixtable_design::change_mode`, not `Theme::change_mode`. Bootstrap's mode switch
resets link colors.

## Components

| Component | Built on | Job |
|---|---|---|
| `ActionButton` (+ `Emphasis`) | `Button` | Any labelled action: Primary, Secondary (outline), Quiet (link), Danger (outline, filled only with `.confirm()`) |
| `PageHeader` | `Breadcrumb` slot | Screen title, state sentence, actions |
| `Callout` | `Alert` | Inline message about this screen |
| `Toast` | `CloseButton` | The result of an action (the shell places and times it) |
| `StatusBadge` | `Badge` (subtle, pill) | A state in one or two words |
| `Field` | none | Label, Optional, control, hint or error |
| `Section` | none | Titled, optionally collapsible group with a closed-state summary |
| `KeyValue` | none | Facts about one thing |
| `DataTable` (+ `Column`, `Align`) | none | Read-only record list with selection, footer count, and empty state |
| `EmptyState` | none | What goes here and how to add the first one |
| `Tabs` | `Nav` underline | Views of the same thing |

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
| Re-render snapshots after a deliberate change | `cd crates/design && cargo run -p ixtable-design --example styleguide -- render` |
| Check snapshots (CI) | `cd crates/design && cargo run -p ixtable-design --example styleguide -- check` |

Rendering needs a Vulkan driver (`mesa-vulkan-drivers` on Linux; CI installs it).

## Rollout

1. This change adds the crate, the styleguide, and the CI check. The app doesn't depend
   on the crate yet, so screens are unchanged.
2. The next change applies the chosen direction's tokens, adds `ixtable-design` to the
   app, and replaces `ui::theme::apply` with `ixtable_design::init`. That moves
   gpui-kit's theme onto the Bootstrap tokens.
3. After that, screens migrate one area per PR, in this order: runtime, data, design,
   automation, settings and cloud. Each PR replaces `widgets::callout`,
   `display::data_table`, `forms::primary` and the like with the components, and works
   through `/mnt/project-files/qa/pr-design-7/drift-findings.md`. The old helpers are
   deleted once nothing uses them.

## Gaps (add a component before a screen needs a one-off)

- `Dialog`: a thin layer over `Modal` with a sticky footer and the one-primary rule.
- Ribbon and sidebar navigation.
- Datasheet (stays an app widget; it is an editable grid, not a `DataTable`).
