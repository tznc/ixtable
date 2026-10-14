# Pages: one Design page for forms, reports, and dashboards

## Status

Accepted (2026-10-13). Covers PRD §13 to §16 and §16.1. Config version 8.

## Context

Studio had three modes for screens: Design (forms), Reports, and Dashboards. A dashboard
was already a grid that held forms, reports, charts, filters, and buttons, but forms and
reports were built and navigated to on their own. Users asked for one Design page where a
page is built from any of these parts.

## Decision

- **Page model.** `pages::Page` (formerly `dashboards::Dashboard`) is a grid layout,
  filters, and components. `DocumentConfig.pages` holds them and still reads the old
  `dashboards` key. `NavKind::Page` and the role object kind `page` replace `dashboard`;
  `NavKind` still reads `dashboard`.
- **Form and report definitions stay.** `design.forms` and `reports` keep their models and
  every feature (form events, continuous, split, and popup views, nested subforms, report
  groups, running sums, charts, preview, PDF). A page component points at one by id. A form
  or report may sit on several pages, or on none.
- **Migration** (`pages::upgrade`). `DocumentConfig::upgrade` turns a config older than
  version 8 into pages. Each dashboard keeps its id. Each form and report that no page
  shows gets a page holding it full width, with an id derived from the definition id
  (UUIDv5 bytes stamped as version 7, like every other id), so loading the same file twice gives the same ids. Navigation items for forms
  and reports point at those pages and keep their form mode. Roles that read a form or a
  report also read its page. Stored `dashboard` role kinds become `page`.
- **Keeping pages in step.** `pages::upgrade::sync` drops a page whose only component
  shows a deleted form or report, and gives new forms and reports a page. It runs after
  the generators ("Generate pages from table", "Generate app"), Access import, and the
  New/Duplicate/Delete form and report commands. A new document's main form is on a page.
- **Studio.** `ModeId` loses Reports and Dashboards; stored `reports`/`dashboards` active
  modes open Design. `ui::pages::DesignMode` lists the pages, the navigation editor, the
  generators, and any form or report on no page. It shows the page designer
  (`PagePanel`), or, after "Edit form" or "Edit report" on a component, the form designer
  (`DesignStudio`) or report designer (`ReportPanel`) with a link back to the page. Every
  Design reveal (Problems links, edit buttons) goes through `DesignMode`.
- **Page parameters.** `Page.parameters` declares named, typed inputs with an optional
  default and a required flag, edited under "Page settings" in the page designer. In the
  Runtime the values arrive as `params`, merged with the page filters and the defaults
  (`page_engine::links::with_defaults`).
- **Links between pages.** Three things open a page and pass parameter expressions:
  navigation items (`NavigationItem.params`, evaluated over the app scope), page buttons
  (`PageComponent.target_page_id` and `page_params`, evaluated over the page's `params`;
  a button with an action opens the page after the action succeeds), and `openDashboard`
  steps. `page_engine::links::page_links` lists them, and the navigation editor shows the
  list as "Page links". `link_issues` (run by validation) reports a missing target page,
  a required parameter with no value, and a value for a parameter the page does not
  declare.
- **Runtime.** Navigation opens pages through `PageBuilders::page`. A page whose only
  component is a form or report is shown through the form renderer or the report run
  (`forms::navigation::solo_page`), so migrated apps behave as before; the navigation
  item's form mode wins over the component's. Such a page also opens for a role that
  reads its form or report (`forms::navigation::can_open`), since it shows nothing more.

## Consequences

- The automation step `openDashboard` keeps its stored name and `dashboardId` field; the
  step editor calls it "Open page".
- A form that is on no page is not in navigation unless a navigation item points at the
  form itself. Older `form` and `report` navigation items still open in the Runtime.
- Wrapping every form gives subforms and detail forms a page too. They are listed with the
  other pages.

## Evidence

- `src-tauri/src/pages/upgrade_tests.rs`: migration, ids, navigation, roles.
- `src-tauri/src/ui/pages/tests_design_mode.rs`: page list, Edit form / Edit report and
  back, new forms and reports, navigation, loose definitions, runtime full-page forms,
  page parameters, and button links that open a page with values.
- `src-tauri/src/page_engine/links_tests.rs`: link listing, issues, defaults.
