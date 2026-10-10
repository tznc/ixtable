---
name: app-qa
description: Visually verify ixtable UI and interaction changes with real React rendered in jsdom, user-event interactions, the tauri-test Rust bridge, and Chromium screenshots. Use for /app-qa, screenshot requests, and after any user-facing UI change.
---

# App QA

Use this skill after changing `src/**`, UI-facing `src-tauri/**` commands, or shared styling.

## Invariants

- Render production components in jsdom (`<App />`). Do not build a lookalike fixture.
- Never start Vite, Tauri dev, or another live server for `/app-qa` screenshots. Live-server screenshots belong only to `/service-qa` and `/web-qa`.
- Serialize the jsdom-rendered production DOM with `captureDocument`; Chromium rasterizes that static capture and never navigates to a running app.
- Drive every interaction with `@testing-library/user-event`; never use `fireEvent`.
- Query by accessible role and name. Keep real DOM assertions alongside every screenshot.
- Use the real `tauri-test` bridge for Rust-backed flows. Mock only what is outside the app: native file dialogs (`dialogMock` from `tests/setup.screenshot.ts`), Tauri events, Monaco, `window.print`, `window.confirm`.
- Every capture has 1–3 plain-English visual expectations.

## The app and its flows

Start screen → Studio (sidebar modes: Data, Query, Design, Reports, Dashboards, Automation, Settings, Runtime) or a runtime-only bundle window. Settings tabs: Assets, Release, Datasource, Entities, Migrations, Roles, YAML, Problems, Logs.

One spec per critical local flow lives in `scripts/screenshot/specs/`:

| spec | flow (capture prefix) |
|---|---|
| `app-shell` | start, data grid, table design, canvas, inline record, SQL query, form builder, preview, close (`app-qa-*`, feeds the docs images in `manifest.mjs`) |
| `schema` | create table with keys/FK/unique/check/index, table designer, impact preview, relationship diagram (`schema-*`) |
| `query` | visual builder with join, aggregate, parameter, filter, sort, preview; generated SQL (`query-*`) |
| `forms` | generate CRUD forms, keyboard grid resize, validation rule (`forms-*`); Runtime list, create with validation and relationship selector, master/detail, edit (`runtime-*`) |
| `reports` | band designer, preview, print, PDF export (`reports-*`) |
| `dashboards` | KPI + bar chart + filter on the shared grid, view, filtered (`dashboards-*`) |
| `automation` | Work orders template: action buttons, action/trigger editors, async job (`automation-*`), roles editor and role preview (`roles-*`) |
| `persistence` | save, autosave, recent documents, crash recovery prompt (`persistence-*`), assets + archive size + checkpoints (`assets-*`), logs |
| `release` | protected bundle export, password prompt, runtime-only window, update keeping records (`release-*`) |
| `settings` | datasource, entities, migrations dry run, YAML, problems (`settings-*`) |
| `templates` | start from CRM, Inventory, Work orders (`templates-*`) |
| `access-import` | Access template import: inventory, conversion report, imported app in the Runtime (`access-*`) |

Shared helpers (seeding through the bridge, Save As, settings tabs, templates) are in `specs/fixtures.tsx`.

## Workflow

1. Identify the user-visible behavior and the smallest meaningful before/after path.
2. Extend the flow's spec (or add `specs/<flow>.spec.tsx`). Name captures `<flow>-NN-<what>`.
3. Render the real app, assert the DOM reached the intended state (wait for async lookups and loaded records before capturing), and interact only through `userEvent`.
4. Call `captureDocument(document, { name, expectations })`. Options:
   - `selector: ".flow-browser"` captures and fits a React Flow diagram.
   - `expand: "<scroll container>"` lifts that pane's height/overflow limits so content a user reaches by scrolling is captured (for example `.table-designer`, `.query-tab-panel`).
   - The page viewport grows to the document height, so the fixed Studio sidebar spans the whole capture.
5. Run `npm run screenshot` after Rust, dependency, or CSS changes (it rebuilds the bridge and CSS, cleans, captures every spec, and syncs the docs images to `web/docs/assets`). For quick iteration after CSS is built: `npx vitest run --config vitest.screenshot.config.ts scripts/screenshot/specs/<flow>.spec.tsx`.
6. Read each generated JSON manifest and open its PNG (with the Read tool) against every expectation. Passing assertions do not prove the pixels are correct.
7. Send PNGs to the orchestrator by listing their paths in your report, and report any mismatch.
8. Finish with `npm test` and `npm run build`.

Generated artifacts live under `scripts/screenshot/.generated/` and are ignored. If Playwright reports a missing browser build, run `npx playwright install chromium-headless-shell`.

## Harness notes (jsdom has no layout)

`tests/setup.screenshot.ts` gives the measurements widgets need: React Flow canvases and nodes, shared-grid canvases (`.grid-canvas`, measured at a desktop width so breakpoints match the desktop layout), `getBBox` for SVG text, `matchMedia`, `scrollIntoView`, and a per-run `IXTABLE_STATE_DIR`. Charts measure themselves and fall back to a 480-unit SVG frame that Chromium scales.

If a browser-only widget cannot be captured from serialized jsdom, treat that as a harness gap. Fix the harness or report the mismatch; never replace the App QA proof with a live-server screenshot or a fake widget.
