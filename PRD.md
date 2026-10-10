# ixtable — Commercial MVP Product Requirements Document

**Status:** Scoped for implementation  
**Product:** ixtable  
**Category:** Local-first relational application builder  
**License:** Apache-2.0 desktop core  
**Business model:** Free open-source desktop product + paid ixtable Cloud  
**Launch model:** Public self-service commercial release  
**Planning model:** Phase gates without calendar estimates in this PRD

---

## 1. Executive Summary

ixtable is a generic desktop application builder for creating relational business applications without assembling a conventional application stack.

Users model data, create queries, design forms, compose reports and dashboards, define actions and triggers, and run the resulting application from the same desktop product.

The free Apache-2.0 edition supports complete, editable, single-user local applications. The paid cloud service monetizes private application distribution, authenticated runtime access, application publishing, archive backup and versioning, credential delivery, RBAC, audit history, and commercial operations.

The commercial MVP is deliberately **not** a hosted database platform or browser app:

- Studio and Runtime are desktop-only.
- Local SQLite data remains local.
- ixtable Cloud does not provide managed PostgreSQL.
- PostgreSQL connections are supplied and operated by the application developer.
- Cloud stores versioned application archives and distributes personalized runtime bundles.
- Browser runtime, cloud SQLite execution, record synchronization, Access interoperability beyond one-way import, AI, and self-hosted cloud features are excluded. One-way import of Access databases and templates into a new document is in scope ([decision record](docs/decisions/access-import.md)).

> **Build complete relational applications locally. Privately distribute them when ready.**

---

## 2. Product Thesis

Microsoft Access proved that one technically capable person can model relational data, build interfaces, encode business rules, produce reports, and deliver useful business software from one environment.

ixtable modernizes that workflow through:

- cross-platform desktop operation;
- standard relational databases;
- portable, self-contained application archives;
- a shared layout system for forms and dashboards;
- DuckDB-backed reads and analytics;
- private authenticated application distribution;
- recoverable publishing and versioning; and
- an open-source local core.

ixtable is application-first rather than spreadsheet-first and database-first rather than frontend-first.

---

## 3. Primary Customer and Jobs

### 3.1 MVP beachhead

The primary customer is a technical operations team whose developer or technically capable operator needs to build internal relational applications.

The **Developer** builds and publishes the application. **Runtime Users** use the distributed application through authenticated desktop Runtime.

### 3.2 Core jobs

- Replace fragile spreadsheet workflows with constrained relational data.
- Build data-entry and operational interfaces without a frontend codebase.
- Query SQLite, PostgreSQL, and supported external sources.
- Produce operational reports and dashboards.
- Encode validation, actions, and record-triggered behavior.
- Run a complete application locally without an account.
- Privately distribute a controlled runtime application to trusted users.
- Update and recover distributed application versions.

### 3.3 Reference domains, not product opinions

ixtable remains a generic builder. It does not impose a CRM, inventory, or work-order domain model.

Three separately implemented golden applications prove that the generic primitives are sufficient:

1. Lightweight CRM
2. Inventory management
3. Work-order management

These are editable examples and automated QA fixtures, not hard-coded product modes.

---

## 4. Product Editions and Commercial Boundary

### 4.1 Free desktop edition

The free Apache-2.0 product includes:

- Desktop Studio and local Runtime on Windows, macOS, and Linux
- creation and editing of `.ixt` applications
- local single-user application execution
- SQLite and PostgreSQL RecordStores
- DuckDB read/query engine
- schema, query, form, report, and dashboard designers
- expressions, actions, and local triggers
- application-managed attachments
- local autosave, checkpoints, and recovery
- editable sharing of complete `.ixt` projects outside ixtable Cloud
- versioned runtime-only bundle export
- optional password protection for runtime-only bundles
- manual distribution and update of runtime-only bundles

Free users may distribute either editable projects or locked runtime-only bundles. Manual distribution has no cloud identity, user-specific personalization, automatic update delivery, remote revocation, centralized audit history, or managed recovery.

“Single-user” means ixtable provides no shared SQLite record state or synchronization. Manually distributed recipients may run independent embedded SQLite copies or connect to a developer-provided PostgreSQL database.

A password-protected runtime bundle protects the bundle at rest and against casual unauthorized opening. It does not prevent an authorized recipient from extracting displayed data or runtime-accessible credentials.

Because the desktop code is Apache-2.0, code-level gates may be removed by forks. The durable commercial boundary is the official cloud service and its operational capabilities.

### 4.2 Paid ixtable Cloud

Paid plans are priced per cloud application and include a runtime-user allowance.

ixtable Cloud provides:

- accounts and organizations;
- exactly one Developer/Owner role per cloud application;
- authenticated Runtime Users and custom runtime roles;
- private-by-default application distribution;
- explicit publishing of application checkpoints;
- automatic runtime update delivery on sync;
- signed and user-fingerprinted runtime bundles;
- encrypted datasource credential delivery;
- S3-backed archive backup and versioning;
- checkpoint restoration;
- cloud audit history;
- subscriptions, entitlements, and plan enforcement; and
- commercial monitoring and support tooling.

### 4.3 Paid cloud is not

- a managed PostgreSQL service;
- a cloud SQLite runtime;
- a browser runtime;
- a multi-developer editor;
- a row-level SQLite synchronization engine; or
- a self-hostable cloud control plane.

### 4.4 Manual versus managed distribution

| Capability | Free/manual | Paid cloud |
|---|---|---|
| Editable `.ixt` sharing | Yes | Yes |
| Runtime-only bundle | Yes | Yes |
| Versioned bundle export | Yes | Yes |
| Password protection | Yes | Optional |
| Manual file delivery | Yes | Optional |
| Authenticated private delivery | No | Yes |
| User fingerprinting | No | Yes |
| Per-user entitlement/revocation | No | Yes |
| Automatic update delivery | No | Yes |
| Cloud archive backup/history | No | Yes |
| Central audit history | No | Yes |

---

## 5. Commercial MVP Contract

A commercially complete MVP must allow this end-to-end workflow:

1. A developer installs ixtable Desktop.
2. The developer creates a generic application or clones a golden application.
3. The developer defines SQLite or PostgreSQL data storage.
4. The developer models tables, constraints, and relationships.
5. The developer creates DuckDB-backed queries.
6. The developer builds forms, reports, dashboards, actions, and triggers.
7. The developer tests the application locally.
8. The developer creates a cloud application and runtime roles.
9. The developer explicitly publishes a checkpoint.
10. Invited users authenticate in ixtable Desktop Runtime.
11. Each user receives a signed, personalized application bundle.
12. Runtime users use the application within assigned RBAC permissions.
13. Published updates download automatically during sync and activate safely.
14. The developer can inspect audit history and restore an eligible checkpoint.
15. Billing and plan enforcement work without manual operator intervention.

If this loop is incomplete, the product has not reached commercial MVP.

The equivalent free workflow ends with a versioned, optionally password-protected runtime bundle that the developer distributes and updates manually.

---

## 6. Platforms and Runtime Model

### 6.1 Supported platforms

Commercial launch supports:

- Windows
- macOS
- Linux

All three are release-blocking platforms and participate in CI.

### 6.2 Desktop only

Desktop contains both Studio and Runtime.

- Studio authors applications.
- Local Runtime previews and executes applications.
- Paid Runtime executes personalized cloud-distributed applications.

There is no browser Studio or Runtime in the MVP. The website is the ixtable Cloud account and management dashboard. It uses Supabase authentication. It does not open or edit `.ixt` applications.

### 6.3 Future rendering portability

Application layouts use a platform-agnostic grid model. The desktop renderer translates that model to CSS Grid.

The schema must avoid storing browser-specific CSS strings as the canonical layout. A future renderer may target React Native, but native mobile rendering and authoring are non-goals for the MVP.

---

## 7. `.ixt` Application Archive

### 7.1 Format

An application is a SQLite Archive file with the `.ixt` extension. The format follows the SQLite archive idea. One SQLite database holds named payload objects, not a zip of loose files. See https://sqlite.org/sqlar.html.

The current schema is the contract. Extend it. Do not replace it with a second archive layout.

Versioned tables today:

```text
archive_metadata     format version, document id, timestamps, app version
data_payload         compressed embedded record-store bytes (data.db)
document_config      JSON DocumentConfig (source of structured settings)
attachments          application-asset blobs with checksums and MIME metadata
```

A working session extracts those objects to:

```text
data.db
document.json
config.yaml
attachments/<id>/
```

`data.db` is the embedded SQLite RecordStore while a session is open. Writes go there. A successful save packs the extracted store back into `data_payload`.

Later objects (migrations, extra assets, unknown tables) must be added as additional archive tables or rows. Unknown future tables must be preserved where safe. Unsupported `format_version` values fail with a clear compatibility message.

### 7.2 Autosave, extraction, and crash recovery

The `.ixt` file is the source of truth after a successful save.

Extraction exists so in-progress edits can use ordinary files and database connections. Extracted files are WIP. They are not a second published copy of the application.

Required behavior:

- debounced automatic saving into the `.ixt` file when a path exists
- atomic replacement of the archive (temp file, validate, rename)
- no partially written archive becoming authoritative
- crash recovery of extracted WIP, then a checkpoint back into the `.ixt` file
- visible dirty, saving, saved, and error state

If recovery cannot reconstruct a valid archive, keep the last valid `.ixt` and surface the failure.

### 7.3 Portability

A local `.ixt` application must remain usable without an ixtable account or cloud service.

### 7.4 Cloud size limit

The MVP cloud-synchronized archive limit is **500 MB**.

Before publishing or backup, Studio reports the archive size and largest entries. Over-limit applications remain usable locally but cannot sync until reduced.

---

## 8. Application Configuration

`DocumentConfig` is JSON stored in `document_config`. That is the structured application catalog. It is not a second SQLite catalog of definitions.

The same document also keeps a YAML projection (`config.yaml` in the working session). YAML is the code-first editing surface. Loading YAML replaces `DocumentConfig`. Saving or updating config rewrites YAML so the two stay in sync.

`DocumentConfig` includes:

- document name and active mode
- navigation state and settings
- saved queries and parameters
- design schema (forms, shared grid layouts, navigation)
- and later reports, dashboards, expressions, actions, triggers, roles, migrations, datasource definitions, and dependency metadata as fields on this same object

ixtable owns the configuration schema and migrates it during product upgrades and downgrades where supported.

Stable object IDs are mandatory. Display names are not identity.

Record schema (tables, columns, constraints) lives in the RecordStore, not in `DocumentConfig`.

---

## 9. Record Storage

### 9.1 MVP RecordStores

The commercial MVP supports:

- `SQLiteRecordStore`
- `PostgresRecordStore`

There is no managed PostgreSQL offering in the MVP.

### 9.2 SQLite

SQLite is the default local transactional store. Embedded records live in `data.sqlite` inside the `.ixt` archive.

ixtable exposes real SQLite capabilities:

- primary and composite keys;
- foreign keys;
- unique constraints;
- check constraints;
- defaults;
- cascading actions;
- indexes; and
- transactions.

SQLite applications are independent local applications. Cloud archive upload provides backup and distribution, not concurrent multi-user SQLite record synchronization.

For a runtime-only SQLite bundle, `data.sqlite` initializes that installation on first open. Thereafter, the installation's records and record attachments are independent local state. Application-definition updates preserve that state and apply declared migrations; they do not replace it with the Developer's bundled copy.

### 9.3 PostgreSQL

The application developer supplies PostgreSQL hosting, networking, credentials, lifecycle, backups, and availability.

Runtime clients connect directly to PostgreSQL. ixtable does not proxy queries in the MVP.

Supported credential modes:

- one shared application credential; or
- separate least-privileged credentials per runtime user.

Studio warns that shared credentials reduce revocation and database-level attribution.

### 9.4 Capability contract and read/write split

`RecordStore` is the generic name for the current split:

- DuckDB reads every list, detail, selector, saved query, report, and dashboard dataset.
- Record writes from forms, tables, actions, triggers, and imports go directly to the selected store (SQLite or PostgreSQL). They never go through DuckDB.
- User-written action queries (§12.1) are the one exception. They run through a separate, short-lived writable DuckDB connection to the same store. The shared reader stays read-only.

Studio UI must speak store capabilities (in-place change vs rebuild), not SQLite-only alter-table language.

Every RecordStore publishes capabilities for:

- logical types;
- DDL operations;
- constraints and indexes;
- transactions;
- parameter binding;
- generated values;
- migration behavior;
- error mapping; and
- concurrency facilities.

SQLite and PostgreSQL must pass the same conformance suite. Backend-specific capability differences remain visible rather than being silently emulated.

---

## 10. DuckDB Read Architecture

DuckDB handles every application read path.

Reads include:

- record lists and details;
- related-record selectors;
- saved queries;
- report datasets;
- dashboard datasets; and
- supported external files and sources.

DuckDB accesses data through curated, bundled extensions, including SQLite and PostgreSQL integration. The MVP extension allowlist is fixed and signed; arbitrary extension installation is deferred.

Record creates, updates, and deletes go directly to the selected RecordStore. Set-based changes that the user writes as action queries (§12.1) go through a separate writable DuckDB connection.

```text
Read
  -> DuckDB (read-only)
     -> SQLite / PostgreSQL / allowed external source

Record write
  -> RecordStore
     -> SQLite / PostgreSQL

Action query (user-written)
  -> DuckDB writer (short-lived, one transaction)
     -> SQLite / PostgreSQL
```

### 10.1 Consistency requirements

The runtime must define and test:

- writes commit before dependent reads execute;
- DuckDB connections/views refresh after mutation where required;
- read-your-writes behavior within the application workflow;
- transaction error propagation;
- parameter and logical-type equivalence across engines; and
- deterministic null, date/time, decimal, and collation behavior where promised.

Cross-engine conformance failures block commercial release.

---

## 11. Schema Designer

Users visually define:

- tables and fields;
- logical types;
- required/nullability rules;
- primary and composite keys;
- foreign keys;
- uniqueness;
- defaults;
- check constraints;
- indexes; and
- update/delete behavior.

A relationship diagram visualizes the schema.

All destructive schema actions require impact preview. Operations that cannot be expressed safely through the designer may be performed through an explicit migration.

---

## 12. Queries

Queries are first-class application objects.

The visual query builder supports:

- source selection;
- joins;
- fields and aliases;
- filters;
- grouping and aggregation;
- sorting;
- parameters; and
- preview.

Advanced users may write SQL directly.

Every saved read query executes through DuckDB. Schema changes belong to migrations, not queries.

Query results may source forms, tables, reports, charts, and dashboards.

### 12.1 Action queries

Users may save action queries that change rows in bulk, matching Access append, update, delete, and make-table queries. An action query declares its kind (`insert`, `update`, `delete`, or `replace`) and one target table, and is written in the same DuckDB SQL dialect as read queries, so it runs unchanged on SQLite and PostgreSQL.

- The DuckDB write path is allowed only for these user-written queries. Product features never generate DuckDB writes in place of RecordStore writes.
- A guard accepts exactly one statement of the declared kind against the declared table. DDL, catalog, settings, and file access are rejected.
- Each run is one transaction on a short-lived writer connection that is locked down like the reader. A dry run reports affected row counts and rolls back.
- Role permissions are checked per table operation, record triggers fire for each created or updated row, and the store's constraints, including foreign keys, are enforced as for RecordStore writes.
- After commit, the reader refreshes as after any other write (§10.1).

Action queries may be run from Studio, from manual actions (§17.2), and by imported Access applications. Design details: [action queries decision record](docs/decisions/action-queries.md).

---

## 13. Shared Grid Layout System

ixtable defines a renderer-agnostic grid schema based on the concepts of CSS Grid without persisting raw CSS as the application model.

Required primitives:

- rows and columns;
- fixed, content-sized, and fractional tracks;
- gaps and padding;
- row/column spans;
- minimum and maximum sizes;
- alignment;
- named regions;
- breakpoints and wrapping rules; and
- interactive resizing within declared constraints.

Forms and dashboards use this same grid system and renderer.

The grid engine must have serialized-layout tests independent of UI snapshots.

---

## 14. Forms

Forms are first-class application objects, not alternate table skins.

The form designer supports:

- resizable responsive grid layout;
- labels and text;
- text, number, boolean, date, time, and select controls;
- relationship selectors;
- validation messages;
- computed display values;
- conditional visibility and enabled state;
- sections and tabs;
- buttons and declarative actions;
- record list, detail, create, and edit modes;
- one-level master/detail forms; and
- related-record lists.

Arbitrarily nested subforms, absolute pixel layouts, and custom scripted controls are excluded.

Generated CRUD forms must use the same public form primitives as manually designed forms.

---

## 15. Reports

Commercial MVP includes a freeform visual report canvas.

Supported components are limited to:

- static text;
- bound fields;
- images;
- lines and rectangles;
- query-backed tables;
- grouping;
- totals and calculated expressions;
- report and page headers/footers; and
- pagination controls.

Outputs:

- preview;
- print; and
- PDF.

The report engine must use a deterministic layout and pagination model across all supported platforms.

Nested subreports, report scripts, barcodes, label-specific tooling, and arbitrary HTML/CSS are deferred.

---

## 16. Charts and Dashboards

The MVP includes a full dashboard canvas built on the shared grid system.

Dashboard components include:

- KPI/summary values;
- tables;
- filters;
- forms;
- action buttons; and
- bar, line, area, pie/donut, scatter, and summary charts.

Charts consume saved query results and do not implement independent datasource logic.

Dashboard components must reuse form, table, query, expression, and action primitives. A second dashboard-only layout or state engine is prohibited.

---

## 17. Expressions, Actions, and Triggers

### 17.1 Expression language

The MVP provides a declarative expression language for:

- validation;
- computed values;
- conditional visibility;
- conditional enabled state;
- filters;
- formatting; and
- action conditions.

Arbitrary JavaScript/TypeScript is excluded.

### 17.2 Manual actions

Buttons may invoke declarative actions such as:

- create/update/delete a record;
- run a query;
- navigate;
- open a form/report/dashboard;
- set application or form state;
- show a confirmation/message; and
- compose multiple actions with explicit failure behavior.

### 17.3 Record triggers

The MVP supports record-created and record-updated triggers.

Triggers may be:

- synchronous, inside the initiating workflow; or
- asynchronous, through a durable local queue.

The asynchronous queue runs only while the desktop application is running. It provides retries, status, attempt history, cancellation, and idempotency keys.

Schedules, webhooks, cloud workers, email integrations, and always-on execution are deferred.

---

## 18. Application assets

Application assets are individual files stored in the archive `attachments` table. They belong to the application, not to a business record.

Requirements:

- stable asset IDs
- original filename and MIME metadata
- checksum validation
- deduplication by content hash where practical
- safe filename handling
- streaming import/export so the whole file need not sit in memory
- orphan detection and cleanup
- inclusion in archive checkpoints and cloud versions

Studio must show how much of archive size comes from assets.

Record-linked or object-storage attachments are deferred.

---

## 19. Application Concurrency

ixtable does not impose one universal record-conflict policy.

The developer selects a policy per entity:

- optimistic version check and reject;
- last-write-wins; or
- custom transactional action.

Generated entities default to optimistic rejection. Publishing validation fails if an entity exposed to multiple Runtime Users has no resolved policy.

RecordStore transactions provide atomicity but are not themselves a conflict policy.

SQLite applications remain single-user in the MVP. PostgreSQL application concurrency is determined by the selected policy and database behavior.

---

## 20. Permissions and Roles

### 20.1 Cloud application roles

Each cloud application has exactly one Developer/Owner.

The Developer defines custom Runtime User roles controlling:

- page/navigation visibility;
- object access;
- action execution; and
- create/read/update/delete operations.

Field-level and row-level permission rules are deferred.

### 20.2 Enforcement boundary

Runtime enforces ixtable RBAC in navigation, queries, forms, reports, dashboards, and actions.

For direct PostgreSQL connections, ixtable RBAC is not a defense against a malicious authorized user who extracts valid database credentials. Strong database-level isolation requires separate least-privileged credentials and database permissions supplied by the developer.

This limitation must be documented in product UI and security documentation.

---

## 21. Authentication and Private Distribution

### 21.1 Account authentication

Commercial launch supports:

- email/password authentication;
- password recovery;
- invitations;
- Google sign-in; and
- Microsoft sign-in.

Enterprise OIDC is deferred.

Cloud applications are private by default. There are no public links or anonymous runtime sessions.

### 21.2 Personalized bundles

Publishing creates signed application material from the selected checkpoint. Each authorized user downloads a user-fingerprinted runtime bundle.

Fingerprinting provides attribution and traceability; it does not prevent copying by an authorized user.

The local exporter also creates signed, versioned runtime-only bundles without cloud personalization. These may be protected by a developer-selected password and distributed manually.

### 21.3 Credential delivery

Datasource credentials are encrypted by the developer and delivered through an envelope-encryption design using established cryptographic primitives.

After cloud authentication:

1. Runtime verifies the signed personalized bundle.
2. Runtime requests a short-lived one-time key grant from ixtable Cloud.
3. Cloud verifies application membership, role, entitlement, device/session state, and revocation.
4. Runtime decrypts the permitted datasource credential for the authorized session.
5. Refresh-token renewal may obtain a new key grant.

The authorization/key-renewal interval is 24 hours.

Credentials may be shared per application or separate per user. Per-user least-privileged credentials are recommended.

Revocation prevents future legitimate key renewal; it cannot guarantee erasure of credentials previously observed by a malicious authorized user.

The design must pass independent security review before public launch. Custom cryptographic algorithms are prohibited.

### 21.4 TLS

The developer controls PostgreSQL TLS configuration.

Cloud distribution may allow a non-TLS datasource only after a severe warning and explicit Developer confirmation. The application security summary records this override and displays it before publishing.

---

## 22. Publishing, Sync, and Updates

### 22.1 Editable local model

The MVP has no development/staging/production environment system. Applications remain editable locally.

Distribution still requires an explicit **Publish checkpoint** action. Autosave never publishes.

### 22.2 Published checkpoint

A published checkpoint records:

- application/archive version;
- Developer identity;
- timestamp;
- archive checksum;
- schema migrations;
- minimum Runtime version;
- credential/security configuration; and
- release notes.

### 22.3 Runtime update flow

Authorized Runtime Users automatically receive the newest published checkpoint during sync.

Activation sequence:

1. Download into a temporary location.
2. Verify identity, entitlement, checksum, and signature.
3. Create a local recovery checkpoint.
4. Validate archive and Runtime compatibility.
5. Separate incoming application definitions/assets from installation-owned records and attachments.
6. Preserve the installation's `data.sqlite` and record attachments.
7. Preview and apply applicable migrations to installation-owned data.
8. Run application health checks.
9. Atomically activate the new definition version and migrated local state.
10. Revert definition and local state to the prior checkpoint if activation fails.

Replacing or reseeding an existing installation's business data requires a separate explicit destructive action, a clear impact preview, and a recovery checkpoint. Normal application updates never replace business records.

### 22.4 Local authority

Local application/installation state is authoritative. Cloud versions are checkpoints, backups, and distribution sources rather than a live SQLite record service.

When archive histories diverge, supported MVP resolution is explicit overwrite or fork. Automatic row-level merge is excluded.

---

## 23. Cloud Archive Backup and Restoration

ixtable Cloud stores `.ixt` archives in versioned S3-backed storage, colocated with the cloud application's control-plane metadata.

The service keeps logically separate version streams for:

- Developer-published application definitions and bootstrap state; and
- optional backups of each independent Runtime installation's local SQLite state.

Runtime-installation backups are enabled and governed by the Developer. They are keyed by application, user, and installation/device identity. They are never merged with another installation.

Requirements:

- client-side checksum before upload;
- resumable or retryable upload;
- version preconditions;
- encryption in transit and at rest;
- immutable version identity;
- retention policy;
- restore-to-new-local-copy;
- audit event for upload, restore, overwrite, and fork; and
- 500 MB per-archive limit in MVP.

For embedded SQLite applications, restoring a Developer checkpoint restores that Developer archive's definition, embedded data, and attachments together. Restoring a Runtime-installation backup restores only the selected independent installation stream.

For PostgreSQL applications, restoring an archive does **not** restore external PostgreSQL records. This limitation must be shown before restoration.

---

## 24. Migrations

Developers define arbitrary SQL `up` and `down` migrations inside the application.

Migrations apply to the embedded SQLite RecordStore. PostgreSQL schema migrations are out of MVP scope; developers manage external schema themselves.

Requirements:

- target RecordStore declaration;
- explicit ordering and immutable IDs;
- dependency validation;
- SQL preview;
- dry-run validation where feasible;
- mandatory pre-migration archive checkpoint;
- transactional execution where the backend supports it;
- captured logs and errors;
- post-migration health checks; and
- manual-recovery instructions when automatic rollback is unsafe.

A `down` migration is required when the developer claims reversibility, but ixtable does not pretend every schema or data migration is safely reversible.

Migration failure blocks application-version activation.

---

## 25. Revision and Audit Systems

The MVP separates:

1. local autosave/checkpoints;
2. published cloud application versions; and
3. cloud security/audit events.

Cloud audit history records:

- application creation and deletion;
- role and membership changes;
- publishing;
- personalized bundle generation and download;
- authentication;
- credential key issuance and renewal;
- credential/user revocation;
- archive upload;
- version overwrite/fork resolution;
- checkpoint restoration; and
- billing/entitlement changes relevant to access.

Semantic application diffs, branching, merge workflows, and Git export are post-MVP.

---

## 26. Golden Applications and CI

### 26.1 Purpose

The three golden applications are:

- user-facing editable examples;
- cloneable starter templates;
- specification fixtures;
- compatibility fixtures;
- migration fixtures; and
- end-to-end CI scenarios.

They may use only public MVP features. Hidden app-specific runtime code is prohibited. Golden applications may churn while kernel contracts settle. A golden fixture is not a compatibility freeze.

### 26.2 Lightweight CRM

Minimum model:

- companies;
- contacts;
- deals;
- activities; and
- deal stages.

Required coverage:

- one-to-many relationships;
- relationship selectors;
- master/detail form;
- filtered and grouped queries;
- pipeline dashboard;
- activity report;
- validation/action rules; and
- record triggers.

### 26.3 Inventory management

Minimum model:

- products;
- locations;
- stock movements;
- suppliers; and
- reorder thresholds.

Required coverage:

- composite/unique constraints;
- transactional stock action;
- aggregation queries;
- low-stock dashboard;
- inventory valuation/position report;
- attachment handling; and
- concurrency-policy configuration.

### 26.4 Work-order management

Minimum model:

- assets;
- work orders;
- tasks;
- statuses/priorities; and
- assignee reference data.

Required coverage:

- nested navigation without nested subforms;
- one-level master/detail;
- status-transition actions;
- synchronous and asynchronous triggers;
- printable work-order report;
- operational dashboard; and
- application migration between fixture versions.

### 26.5 GitHub Actions workflow

CI must run the golden suite on Windows, macOS, and Linux.

Blocking deterministic checks include:

- archive open/save/reopen integrity;
- archive upgrade/downgrade fixtures;
- schema creation and migration;
- seeded CRUD workflows;
- constraints and relationships;
- DuckDB read correctness against SQLite and PostgreSQL;
- form serialization and rendering;
- report dataset, layout, pagination, and PDF snapshots;
- dashboard serialization and query results;
- action and trigger behavior;
- attachment checksums;
- publish/update/rollback simulation;
- preservation of independent Runtime data across definition updates;
- isolation of per-user/per-installation SQLite backup streams;
- authentication/authorization contract tests; and
- corrupted/interrupted archive recovery.

Agent-driven QA executes the same three applications through declared user journeys and emits structured evidence, screenshots, logs, and reproduction steps.

An agent-only judgment may block CI only when it produces a deterministic failing assertion or a reproducible artifact. Exploratory aesthetic judgments remain advisory.

---

## 27. Non-Functional Requirements

### 27.1 Reliability

- No acknowledged save may disappear after a normal restart.
- Interrupted archive writes must preserve the last valid checkpoint.
- Failed application updates must revert atomically.
- Failed migrations must not activate the new application version.

### 27.2 Security

- No plaintext datasource credentials in an unencrypted archive entry.
- No custom cryptographic primitives.
- Signed bundles and updates must fail closed. (In-app updates are deferred post-MVP; this applies to bundles until they return.)
- Cloud applications are private by default.
- Authorization checks must occur at every Runtime object/action entry point.
- Sensitive values are redacted from logs and crash reports.

### 27.3 Performance targets

- Median install-to-working CRUD application: under 30 minutes.
- Typical application open: under 3 seconds after warm start.
- Local field edit acknowledgement: under 100 ms, excluding backend latency.
- Typical autosave completion: under 2 seconds.
- Runtime navigation between already loaded pages: under 200 ms.
- Report/dashboard queries expose cancellation and progress after 2 seconds.

Targets are measured against published reference hardware and fixture sizes.

### 27.4 Accessibility

- Keyboard-accessible Studio and Runtime workflows.
- Visible focus states.
- Semantic labels for generated controls.
- WCAG 2.1 AA contrast target for built-in themes.
- Reports remain readable when printed without color.

### 27.5 Observability and privacy

- Local diagnostic logs are available without an account.
- Optional crash/diagnostic upload requires informed consent.
- Cloud health, authentication, publishing, storage, and key-service metrics are monitored.
- Data collection and retention are documented in product privacy controls.

---

## 28. Multi-Phase Implementation Plan

This PRD intentionally contains no calendar estimates. Delivery is driven by phase exit criteria; schedule and staffing forecasts belong in a separate execution plan after risk retirement. Work may run in parallel only when shared contracts are stable.

Implementation may be accelerated by AI coding and QA agents, but generated work is held to the same review, deterministic test, security, and artifact requirements as human-authored work.

### Phase 0 — Risk Retirement and Contracts

**Objective:** Prove the high-risk architecture before building breadth.

Deliver:

- `.ixt` SQLite archive read/write/autosave spike;
- crash-safe atomic checkpoint spike with large application assets;
- shared grid schema and CSS Grid renderer spike;
- DuckDB reads over SQLite and PostgreSQL on all three OSes;
- PostgreSQL write path and read-after-write consistency spike;
- freeform report pagination/PDF spike;
- password-protected manual runtime-bundle spike;
- signed personalized bundle and envelope-encryption threat model;
- local async trigger queue spike; and
- RecordStore/DuckDB logical-type compatibility matrix.

Exit criteria:

- Each spike has a written decision record and automated proof.
- No unresolved blocker threatens desktop-only architecture.
- Cryptographic design is reviewed before implementation commitment.
- Cross-platform DuckDB extension packaging is reproducible.
- Report pagination is deterministic for the spike fixture.

### Phase 1 — Application Kernel and Archive

**Objective:** Create, persist, reopen, migrate, and recover an application.

Deliver:

- application lifecycle
- `.ixt` SQLite archive schema and `format_version`
- `DocumentConfig` JSON plus YAML projection
- embedded RecordStore payload (`data.db` / `data_payload`)
- application-asset attachment rows
- autosave and checkpoints into the `.ixt` file
- archive validation and recovery of extracted WIP
- SQLite/PostgreSQL RecordStore contracts
- DuckDB read layer
- schema designer and relationship model
- application dependency validation
- session undo/redo for definition edits

Exit criteria:

- Applications survive forced termination during autosave.
- Schema round-trips without definition loss.
- Read/write conformance passes on all OSes.
- 500 MB boundary behavior is tested.
- Golden-application skeletons open in CI.

### Phase 2 — Complete Local Vertical Slice

**Objective:** Build and operate useful single-user applications locally.

Deliver:

- visual query builder and SQL editor;
- shared grid engine;
- generated CRUD;
- form designer and Runtime rendering;
- one-level master/detail;
- navigation;
- expression language;
- manual actions;
- record-created/updated synchronous triggers;
- versioned runtime-only bundle export;
- password protection and manual runtime-bundle update flow;
- local logs and validation UI; and
- first complete CRM golden application.

Exit criteria:

- CRM golden journey passes without hidden code.
- A new user can produce a working relational CRUD app in under 30 minutes.
- All reads use DuckDB and satisfy read-after-write tests.
- Generated and manually designed forms use identical primitives.
- A recipient can open an authorized password-protected Runtime bundle without Studio access.
- Manual update failure preserves the recipient's last working bundle.

### Phase 3 — Reports, Dashboards, and Local Automation

**Objective:** Complete the promised application-building depth.

Deliver:

- freeform report canvas;
- deterministic print/PDF engine;
- full shared-grid dashboard canvas;
- core chart types;
- local asynchronous trigger queue;
- retry/history/cancellation UI;
- concurrency-policy configuration;
- complete Inventory golden application; and
- complete Work Order golden application.

Exit criteria:

- All three golden applications are fully usable and cloneable.
- Report golden files are deterministic on supported platforms within declared tolerances.
- Dashboard and form layouts share one serialized grid contract.
- Async jobs recover after application restart and respect idempotency.

### Phase 4 — Cloud Control Plane and Private Distribution

**Objective:** Complete the first paid workflow.

Deliver:

- accounts, organizations, and invitations;
- email/password, Google, and Microsoft authentication;
- one Developer/Owner per application;
- custom Runtime User RBAC;
- cloud application creation;
- explicit publish checkpoint;
- signed and fingerprinted personalized bundles;
- encrypted credential/key-grant service;
- 24-hour authorization renewal;
- S3 archive upload, versioning, retention, fork, and restore;
- optional per-installation SQLite backup streams without merging;
- automatic Runtime update flow;
- security warnings for shared credentials and non-TLS PostgreSQL;
- cloud audit history; and
- cloud-plan entitlement enforcement.

Exit criteria:

- Unauthorized users cannot discover or download private applications.
- Revoked users cannot obtain new bundles or key grants.
- Failed updates revert without losing the prior working version.
- Definition updates preserve each Runtime installation's local records and attachments.
- Embedded SQLite checkpoints restore application, data, and attachments.
- PostgreSQL restore limitations are explicit and tested.
- Independent security review has no unresolved critical/high findings.

### Phase 5 — Public Self-Service Commercial Release

**Objective:** Make the paid product operable without manual intervention.

Deliver:

- per-application plans with runtime-user allowances;
- checkout, subscription management, invoices, and cancellation;
- entitlement and quota enforcement;
- account/application deletion and export;
- recovery and support flows;
- admin support tooling;
- abuse/rate controls;
- privacy policy, terms, and security documentation;
- service monitoring, alerting, backups, and incident procedures;
- signed desktop installers for all platforms (in-app update channels are deferred post-MVP; users download new versions from GitHub Releases);
- onboarding using the three golden applications; and
- public documentation.

Exit criteria:

- A new customer can register, pay, publish, invite, run, update, restore, and cancel without operator action.
- Billing state and application access remain consistent under retries/failures.
- Support can diagnose distribution and key-grant failures without viewing secrets.
- Release checklist passes on Windows, macOS, and Linux.
- Commercial terms accurately reflect the trusted-user security model.

---

## 29. Commercial Launch Acceptance Criteria

Commercial launch is blocked unless:

- all three golden applications pass deterministic CI on all supported OSes;
- agent-driven journeys complete with reproducible evidence;
- local app creation and execution require no account;
- every read path goes through DuckDB;
- SQLite and PostgreSQL writes pass conformance tests;
- archive autosave and crash recovery meet reliability requirements;
- report print/PDF output meets golden fixtures;
- cloud applications are private and invite-only;
- personalized bundles verify signatures before execution;
- credential/key delivery passes external security review;
- publishing never occurs implicitly from autosave;
- update failure reverts safely;
- embedded SQLite checkpoint restore is proven;
- PostgreSQL data is never represented as cloud-backed-up when it is not;
- subscriptions and entitlements work end to end; and
- account/data export and deletion workflows are operational.

---

## 30. Success Metrics

### Activation

- Median install to working relational CRUD app under 30 minutes.
- Percentage of new developers who successfully run a golden application.
- Percentage who create a custom table, form, query, and report.

### Product depth

- Percentage of active applications using relationships.
- Percentage using custom forms, reports, dashboards, actions, and triggers.
- Golden-application regression pass rate.

### Commercial conversion

- Percentage of active local developers creating a cloud application.
- Publish-to-invitation completion rate.
- Invited-user activation rate.
- Paid application retention.

### Reliability

- Archive corruption incidents.
- Failed autosaves or recoveries.
- Update activation and automatic-revert rates.
- Runtime backup and restore success rate by installation.
- Migration failure rate.
- Credential/key-grant failure rate.

---

## 31. Explicit MVP Non-Goals

The following are not part of commercial MVP:

- browser Studio or Runtime;
- mobile authoring or Runtime;
- managed PostgreSQL;
- cloud-hosted SQLite execution;
- SQLite row-level synchronization;
- offline merging across users/devices;
- multiple application developers;
- developer branches or semantic merge;
- development/staging/production environments;
- self-hosted ixtable Cloud;
- public/anonymous application links;
- field-level or row-level ixtable permissions;
- arbitrary JavaScript/TypeScript scripting;
- arbitrary DuckDB extensions;
- always-on cloud automation;
- schedules or webhooks;
- arbitrary nested subforms;
- nested subreports or report scripting;
- semantic Git export/import;
- MySQL or SQL Server RecordStores;
- Microsoft Access interoperability beyond one-way import (linked tables, export back to Access, VBA execution or conversion);
- AI application building or migration; and
- application-specific hard-coded product modes.

These exclusions are release constraints, not missing acceptance criteria.

---

## 32. Deferred Work Requiring Separate PRDs

Separate PRDs are required before implementation for:

1. Microsoft Access interoperability beyond the one-way import in `docs/decisions/access-import.md`: linked tables, export, and VBA conversion
2. SQLite multi-user synchronization and conflict resolution
3. Browser and mobile runtimes
4. AI-assisted application building and migration
5. Self-hosted cloud/control-plane deployment
6. Multi-developer collaboration, semantic diffs, branching, and merge
7. Additional transactional RecordStores
8. Enterprise identity, OIDC, governance, and advanced audit export

---

## 33. Risk Register and Scope Triggers

| Risk | MVP response | Trigger/action |
|---|---|---|
| DuckDB cannot provide consistent reads across engines/OSes | Mandatory Phase 0 spike and conformance suite | Block architecture progression; do not hide failures with backend-specific behavior |
| Freeform report pagination becomes a platform project | Restrict component set and use golden PDFs | Defer unsupported component behavior rather than expanding the renderer |
| Dashboard becomes a second UI builder | Reuse shared grid and components | Reject dashboard-only layout/state primitives |
| `.ixt` autosave rewrites become slow with attachments | Atomic/debounced checkpoints and size tooling | Redesign checkpoint mechanics before raising 500 MB limit |
| Direct PostgreSQL credentials are extracted | Trusted-user threat model and per-user credentials | Block claims that Runtime protects against malicious authorized users |
| Non-TLS PostgreSQL leaks credentials/data | Severe warning and recorded override | Do not allow silent insecure configuration |
| Transactions are mistaken for conflict handling | Required entity conflict-policy configuration | Publishing validation blocks unresolved shared entities |
| Async triggers expand into workflow SaaS | Local queue only; two record triggers | Defer schedules, webhooks, integrations, and cloud workers |
| Public self-service consumes product roadmap | Dedicated Phase 5 gate | Do not label private manual pilots as public commercial readiness |
| Apache forks remove UI gates | Monetize hosted operations, not artificial lock-in | Do not spend MVP effort on DRM for editable local apps |
| Three-platform matrix slows delivery | Shared contracts and golden CI | A platform is supported only when the full release gate passes |
| Golden apps acquire hidden special cases | Public-feature-only rule | Any required special case becomes either a generic capability or is removed |

---

## 34. Audit of the Previous Draft

The previous PRD contained a strong thesis but mixed MVP, long-term platform strategy, competitive positioning, and enterprise roadmap into one implementation phase.

This revision removes or defers the following noncritical MVP scope:

- Access import and AI conversion;
- browser runtime;
- managed databases;
- self-hosted cloud;
- multiple deployment environments;
- multi-developer collaboration;
- semantic Git workflows;
- general scripting;
- advanced permissions;
- additional database adapters;
- advanced automation; and
- full enterprise governance.

It also replaces broad statements such as “safe versioning,” “collaboration,” and “deployment” with explicit desktop distribution, checkpoint, credential, update, backup, and restoration behavior.

Competitive manifestos and long-term moat claims have been shortened because they do not provide implementation acceptance criteria.

---

## 35. Definition of Done

ixtable reaches commercial MVP when a customer can use the public self-service product to:

- build a complete generic relational application locally;
- prove the builder through three independent golden applications;
- query through DuckDB and write safely to SQLite or PostgreSQL;
- create responsive forms, freeform reports, and grid-based dashboards;
- encode declarative actions and local record triggers;
- publish an explicit application checkpoint;
- privately distribute a personalized desktop Runtime application;
- authorize trusted users through custom roles;
- deliver datasource credentials under the documented threat model;
- update and recover application versions safely;
- back up and restore eligible `.ixt` archives; and
- pay for and operate the cloud service without manual intervention.

Anything not required to complete and safely operate this loop is post-MVP.