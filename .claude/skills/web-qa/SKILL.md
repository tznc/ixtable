---
name: web-qa
description: Verify the ixtable Docusaurus website in a real Chromium browser at desktop and mobile sizes, including navigation, responsive layout, accessibility, and full-page visual captures. Use for /web-qa, website screenshot requests, and after changes under web/.
---

# Web QA

Use this skill after changing the Docusaurus website under `web/`. It tests the production build served locally, never a lookalike fixture or the live site.

## Invariants

- Test the real production Docusaurus build with Playwright Chromium.
- Web screenshots must use the locally served production build. Live-server screenshots are reserved for `/web-qa` and `/service-qa`; `/app-qa` renders the native GPUI app headlessly instead.
- Use accessible roles, names, and labels for interactions and assertions.
- Cover both desktop and mobile viewports for user-facing layout changes.
- Keep DOM assertions alongside screenshots. A passing build alone is not visual proof.
- Never point authentication or service checks at production. Use documented local services only.
- Every screenshot must have a JSON manifest with 1–3 plain-English visual expectations.
- Inspect every generated PNG against its manifest before reporting success.

## Workflow

1. Identify the changed pages, navigation paths, responsive states, and critical user actions.
2. Reuse or extend `web/e2e/web-qa.spec.ts`. Add focused Playwright specs when a flow needs its own setup.
3. Run `npm run web-qa` from the repository root. The command builds Docusaurus, starts the production server, and drives it with Chromium.
4. Read the manifests under `web/web-qa/.generated/` and inspect every matching PNG.
5. Check for clipped content, broken images, unexpected horizontal scrolling, illegible contrast, missing focus states, and mobile navigation problems.
6. Send relevant proof with `treq send web/web-qa/.generated/<capture>.png`.
7. Report the viewports and behaviors verified, plus any mismatch. Do not call a capture successful without inspecting it.
8. Finish with `npm run typecheck --prefix web` and `npm run build --prefix web`.

## Capture naming

Use stable, ordered names such as:

- `landing-01-desktop`
- `landing-02-mobile`
- `docs-01-desktop`
- `auth-01-signed-out`

Generated QA artifacts are intentionally ignored by git.
