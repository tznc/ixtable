---
name: app-qa
description: Visually verify ixtable UI and interaction changes by rendering the real GPUI app headlessly to PNGs (ui::screenshot harness, `cargo screenshot`). Use for /app-qa, screenshot requests, and after any user-facing UI change.
---

# App QA

Use this skill after you change `src-tauri/src/ui/**`, a backend command
that the UI shows, or theming.

## How capture works

Screenshot specs use the same `ui::testing::Harness` as the UI tests. The
harness runs on GPUI's `HeadlessAppContext`, which brings the platform's
real text system and its headless GPU renderer: wgpu (Vulkan) on Linux and
Metal on macOS. `Harness::capture` renders the window to
`<name>.png` and writes `<name>.json` next to it. The JSON holds the size and
the visual expectations. See `src-tauri/src/ui/screenshot.rs`.

- Linux needs a Vulkan driver. Mesa's lavapipe works without a GPU:
  `apt-get install mesa-vulkan-drivers`.
- Windows has no headless renderer, so run `/app-qa` on Linux or macOS.
- If several builds share one `CARGO_TARGET_DIR`, another build can overwrite the
  test binary while it runs, and the run dies with SIGSEGV. When builds run in
  parallel, copy the binary out first, or give each its own target dir.

## Invariants

- Render the production `AppView` through `Harness::start(&mut
  screenshot::context())`. Do not build a lookalike view.
- Never launch the app or a dev server for `/app-qa`. Live-server
  screenshots belong only to `/service-qa` and `/web-qa`.
- Drive every interaction through the `Harness` (`click`, `input`, `fill`,
  `press`, and so on). Find elements by their test ids.
- Put real assertions before every capture (`exists`, `label`, model state)
  so the capture shows the intended state.
- Use the real backend. Fake only what lies outside the app: native dialogs
  go through `h.dialogs` (`TestDialogs`).
- Every capture has 1–3 plain-English visual expectations.

## Specs

There is one spec per critical flow, in `src-tauri/src/ui/screenshots/<flow>.rs`.
Each spec is an `#[ignore]`d `#[test]` that builds its own `screenshot::context()`.
Name captures `<flow>-NN-<what>`.

| spec | flow |
|---|---|
| `app_shell` | start screen, shell, settings YAML, Problems (`app-qa-*`) |
| `automation` | step editor, test run, actions list, trigger editor, jobs (`automation-*`) |
| `cloud` | sign-in, apps, publish, conflict, backup size, runtime bar, on mock services (`cloud-*`) |
| `import` | CSV/XLSX import wizard (`import-*`) |
| `extras` | file sources, roles, runtime as role, expression problems (`extras-*`) |

Each area adds its spec as it is ported. Flows still to cover: datasheet, schema,
query, forms, reports, dashboards, automation, persistence, release,
settings, templates, access-import and export.

## Workflow

1. Identify the user-visible behavior and the smallest before-and-after path
   that shows it.
2. Extend the flow's spec, or add `screenshots/<flow>.rs` and a `mod` line in
   `screenshots/mod.rs`.
3. Run the specs from `src-tauri/`:
   - All of them: `cargo screenshot`.
   - One flow: `cargo screenshot app_shell`.
   - Output goes to `scripts/screenshot/.generated/`, or to
     `$IXTABLE_SCREENSHOT_DIR` when that is set.
4. Read each JSON manifest. Open its PNG with the Read tool and check it
   against every expectation. Passing assertions do not prove the pixels are
   right.
5. List the PNG paths in your report, and report any mismatch.
6. Documentation images are listed in `screenshot::DOCS_IMAGES`. Refresh them
   with `IXTABLE_SCREENSHOT_DOCS=1 cargo screenshot`, which copies them into
   `web/docs/assets/`.
7. Finish with `cargo test --lib`.

Captures render at the window size (`Harness::start_sized`; the default is
1280x900 logical pixels at scale 2). To show content below a scroll fold,
start a taller window.
