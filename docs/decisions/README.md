# Decision records

Each record explains one architecture decision for the ixtable desktop app:
the context, the decision, its consequences, its status, and the tests that
prove it. PRD §28 Phase 0 requires a written record and automated proof for
every risk-retirement spike. This index maps each spike to its record.

| Record | Phase 0 spike | Status |
|---|---|---|
| [Archive format and recovery](./archive-format.md) | `.ixt` read, write, autosave. Crash-safe atomic checkpoint with large assets. Upgrade fixtures in `tests/fixtures/archives/`, forced-termination and 500 MB tests in `src-tauri/src/durability_tests/` | accepted |
| [Shared grid model](./grid-model.md) | grid schema and CSS Grid renderer | accepted (model, engine, forms, and dashboards) |
| [DuckDB read path](./duckdb-read-path.md) | DuckDB reads over SQLite and PostgreSQL on three OSes. Reproducible extension packaging | accepted on Linux; macOS and Windows wired into CI, no run has reached a runner yet |
| [RecordStore capabilities](./recordstore-capabilities.md) | PostgreSQL write path and read-after-write consistency | accepted |
| [Type matrix](./recordstore-type-matrix.md) | RecordStore and DuckDB logical-type compatibility | accepted |
| [Report engine](./report-engine.md) | freeform report pagination and PDF | accepted |
| [Runtime bundles](./runtime-bundles.md) | password-protected manual runtime bundle | accepted for manual bundles; cloud bundles covered by the cloud security model |
| [Async trigger queue](./async-trigger-queue.md) | local async trigger queue | accepted |
| [Expression language](./expression-language.md) | none. Records the PRD §17.1 language design | accepted |
| [Cloud architecture](./cloud-architecture.md) | none. ixtable Cloud control plane: schema, RLS, functions, local stack | accepted, implemented (control plane, distribution, credentials, desktop sign-in, billing, desktop client) |
| [Cloud security model](./cloud-security-model.md) | signed personalized bundle and envelope-encryption threat model | accepted, implemented, external security review pending |
| [Desktop signing and webview CSP](./desktop-updates.md) | none. Signed installers, webview CSP (PRD Phase 5). In-app updates removed from the MVP (2026-10-06) | accepted; release key gate in CI, production key ceremony pending |
| [Performance budgets](./performance-budgets.md) | none. PRD §27.3 targets, reference hardware, fixture, and the report-only harness | accepted, report-only |
| [Windows support](./windows-support.md) | none. Windows-only failures in Rust tests and the NAPI test bridge | accepted, pending Windows CI (no run has reached a runner yet) |
| [Action queries](./action-queries.md) | none. Saved queries that change rows: DuckDB SQL through a writable attach, per-row triggers | accepted |
| [Access import](./access-import.md) | none. One-way import of Access databases and templates. Format spec in [`docs/access-format.md`](../access-format.md) | accepted |

The Phase 0 item "signed personalized bundle and envelope-encryption threat
model" is covered by the [cloud security model](./cloud-security-model.md)
and implemented in `src-tauri/src/cloud/` and `supabase/functions/`. The
[runtime bundles](./runtime-bundles.md) record covers the local manual-bundle
design. Release gates, including the external review of that design, are
mapped in [the release checklist](../release-checklist.md).

## Writing a record

Name the file after the decision, in kebab case, and use these sections:

| Section | Contents |
|---|---|
| Status | `accepted`, `in progress`, or `superseded by <record>`, with the PRD sections it covers |
| Context | the problem and the constraints from the PRD |
| Decision | what the code does, with file and function names |
| Consequences | what the decision costs and what it rules out |
| Evidence | the test files that prove it. CI runs them on Windows, macOS, and Linux through `.github/workflows/desktop.yml` |

Update the record in the same change that alters the decision. A record that
no longer matches the code is a bug.

## Audit log

- 2026-10-05: every record was checked against main. Each record's own audit
  log says what changed. Statuses above were updated to match.
