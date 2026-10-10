# Record triggers and the local async job queue

Status: accepted. Covers PRD §17.3 (including the Phase 7 before-change and
record-deleted triggers) and the Phase 0 local async trigger queue spike.

## Context

The MVP supports record-created and record-updated triggers. Phase 7 adds
before-change triggers, which can set fields of a record or reject its save,
and record-deleted triggers. A trigger runs an action either inside the
initiating workflow or later through a durable local queue. The queue only runs while the desktop app is open, and it must provide
retries, status, attempt history, cancellation, and idempotency keys.
Schedules, webhooks, and always-on workers are deferred.

## Decision

### Where triggers fire

`src/lib/records.ts` is the only frontend write path. Automation registers a
record hook there (`registerRecordHook`), so every insert and update from
forms, grids, dashboards, and actions passes through trigger matching in
`src/automation/triggers.ts`.

- **Sync triggers** run their action after the write commits, and the caller
  awaits them. A failing sync trigger makes the caller's `insertRecord` or
  `updateRecord` reject with a `CommittedWriteError`: the record write itself
  has already committed. An action in rollback mode only undoes its own
  writes. An action whose writes committed reports "The record changes were
  saved, but trigger X failed: reason", and still runs its held-back effects
  and refresh.
- Sync trigger actions run with a browser context: `app` is the Runtime's app
  state, and navigation and `setState(app)` steps are window events that Run
  mode follows. Outside Run mode a navigation is reported as a message.
- **Async triggers** enqueue a job and return at once.
- Each write carries its trigger depth. A chain deeper than 5 fails, which
  stops trigger loops.
- **Deleted triggers** (`event: "deleted"`) run after a delete commits, sync or
  async. The hook reads the row before the delete (or takes `meta.old`), and
  `record` and `old` are that row. Rust issues a grant for sync app-mode
  deleted triggers like it does for created and updated ones.

### Before-change triggers

A trigger with `event: "beforeChange"` runs before a create or update is sent
to Rust, from the `before` record hook. It is always sync (validation refuses
async). `record` is the record about to be saved: the supplied values for a
create, the stored row merged with the changed values for an update. `old` is
the stored row, or null for a create. Triggers run in order, and each sees the
fields earlier ones set.

- Its action may only use `setField` (sets a field of the pending record;
  later steps see it in `record`), `condition`, `fail`, `runQuery` on a read
  query, and `runAction` with the same limits. This follows Access's Before
  Change data macros. Validation flags other steps, and the runner fails them.
  `setField` outside a before-change trigger fails.
- A failing action rejects the save with `BeforeChangeRejected`: "Not saved:"
  and the fail step's message, or the trigger's name and error. Nothing has
  been sent, and the action cannot write, so a rejection never leaves a
  partial write. In `writeRecordBatch` it aborts the whole batch.
- The fields it sets replace or join the write's values. Rust applies the
  same validation and permissions to them as to the caller's own values.
- `runAs` keeps its meaning. App mode skips the role's checks in TypeScript
  (reads still go through Rust's role checks) and needs no grant, since the
  action writes nothing. User mode checks the role, and Rust refuses an
  insert or update up front when the role could not run a user-mode
  before-change trigger (`check_user_triggers` with `BeforeChange`).
- Action queries hand each row they would create or update to the triggers
  through the `beforeBulk` hook (see [action queries](./action-queries.md)).

### Execution identity

Each trigger has `runAs`: `app` (the default when absent) or `user`.

- **app** (definer context): the trigger's steps may write tables the user's
  role cannot, but only the writes its action declares. When Rust commits an
  insert or update (`insert_row`, `update_row`, `execute_write_batch`) on a table with
  enabled sync app-mode triggers for that event, it returns a grant: a random
  token bound to the window, the trigger ids, the table and the event,
  valid for 2 minutes. The runner passes the grant, trigger id and
  step id with each step write and with the lookups the step makes
  (`read_table_page`, `inspect_table`). Rust (`src-tauri/src/trigger_auth.rs`)
  accepts it without role checks only when the grant is live for that
  window, the trigger is enabled and runs as the app, and the action
  declares a step with that id, table and operation (condition branches,
  called actions, and the custom action that an update or delete of a
  `customAction` entity is routed to included). The runner releases the grant when the trigger
  finishes (`release_trigger_grant`), so it is single use. Async app-mode
  jobs present their job id and current lease token instead; Rust checks the
  lease is live (`JobStore::active_lease`) and belongs to that trigger.
  Validation and concurrency rules still apply.
- **user**: steps run under the user's role, as before. Rust refuses the
  initiating insert or update before it commits when the role lacks a
  permission the trigger's steps need (action execute, table operations,
  saved query reads, and the routed custom actions of `customAction`
  entities), naming the trigger and the missing grants. The
  trigger's condition is not evaluated in Rust, so the check applies even
  when the condition would skip the trigger. The Roles settings tab warns
  about user-mode triggers a role cannot run.

### The queue

`src-tauri/src/jobs.rs` keeps jobs in `<state>/data/jobs.db`, a SQLite file
in WAL mode next to the global store. Jobs are keyed by the record store they
belong to (`store_key`): `studio:<documentId>` for Studio sessions of a
document, `runtime:<bundleId>` for an installed runtime bundle, whose
`data.db` is separate even though it has the same document id. Jobs survive
restarts and stay with their records. A queue created before `store_key`
existed is rebuilt in place on open; its rows move to the Studio queue of
their document.

| Column | Meaning |
|---|---|
| `status` | `queued`, `running`, `succeeded`, `failed`, or `cancelled` |
| `idempotency_key` | unique per store key. Enqueueing a duplicate returns the existing job |
| `attempts`, `max_attempts` | default 3 attempts |
| `backoff_ms`, `next_run_at` | retry delay `backoff · 2^(attempts − 1)`, default 1 s, capped at 1 hour |
| `lease_until` | a claimed job's lease, default 5 minutes |
| `lease_token` | `<attempt>:<instance>:<uuid>`, new on every claim. `instance` names the store instance (app process) that claimed it |

`job_attempts` stores one row per attempt with start and finish times, the
outcome, the error, and the step log.

A claim runs in an `IMMEDIATE` transaction. It picks the oldest due `queued`
job, marks it `running`, and sets the lease, so two workers cannot claim the
same job. A job whose lease expired, because the app crashed or quit mid-run,
goes back to `queued` on the next claim or start. Opening the store (exactly
once per app process, under a lock, even when the first calls race; the app
runs as a single instance) also requeues every
`running` job whose lease token names another instance at once, so a job
interrupted by a quit or crash retries promptly instead of waiting out its
5-minute lease. Cancel and retry are explicit commands. Like completion and
failure, they run in an `IMMEDIATE` transaction with a status guard: only a
`queued` or `running` job can be cancelled, so a job that succeeds at the same
moment stays succeeded, and a cancelled job's late result is refused. `complete_job` and `fail_job` take the claim's lease token
and check it, with `status = 'running'`, in one `IMMEDIATE` transaction. A
worker whose lease expired, or whose job was cancelled and claimed again, gets
`STALE_LEASE` and its result is ignored.

The default idempotency key combines the trigger id, table, record identity,
event, a hash of the record values, and the write id that `records.ts` gives
each write call (`meta.writeId`). It dedupes retries of the same write only; a
later identical write is a new event. A trigger can supply its own key as an
expression instead.

### The worker

`src/automation/worker.ts` polls `claim_next_job` for the open document and
runs the action through the same `runAction` the UI uses. It polls every second
while an async trigger is enabled or the queue has queued or running jobs
(`AutomationHost` rechecks this on queue changes and every 30 s), and it also
wakes on each queue change event, so jobs of a trigger
that was later disabled or deleted still run (their action id is stored on the
job). A job whose action no longer exists fails with "Action … does not exist"
and follows the normal retry and failure path. It runs with a
headless context, where navigation, confirmation, and form state fail with a
clear message. An app-mode job presents its lease. A user-mode job is
authorized against both the active role and the role it was enqueued under
(`roleId` in the job payload), so it never exceeds either. A worker that gets
`STALE_LEASE` drops the result without recording a failure. It reports `complete_job` or `fail_job` with the step log.

## Consequences

- Jobs persist across restarts but run only while ixtable is open. Users see
  queued jobs waiting until they reopen the app.
- Delivery is at least once. A crash after the action's writes but before
  `complete_job` runs the job again after the next start. Actions that must
  not repeat need an idempotent design or a key that a second run detects.
- Sync trigger failures surface to the user, but they do not undo the
  initiating write.
- App-mode trigger values are computed by TypeScript expressions. A modified
  client can write arbitrary values, but only to the tables and operations
  the trigger declares, and only while it holds a live grant or job lease.
  `runQuery` steps in app mode still need the role's read access to the
  query.
- Actions run in TypeScript, so the queue needs the app's frontend. A Rust-only
  worker would need a second action runner, which this design avoids.

## Evidence

- `src-tauri/src/automation.rs` tests: before-change triggers limited to
  their step kinds and to sync, `setField` refused elsewhere.
- `tests/unit/before-change-triggers.test.ts`: fields set on inserts and
  updates, rejections with no write (single and batch), refused steps,
  chained triggers, deleted triggers sync and async, and the action query
  two-call flow.
- `tests/integration/before-change-triggers.test.tsx`: the same through the
  real bridge, including a date set on every row of an embedded-SQLite update
  query and a rejection that leaves the table unchanged.
- `src-tauri/src/jobs.rs` tests: idempotent enqueue per store, atomic and
  exclusive claims, exponential backoff until failed, completion with log,
  cancel and retry, expired leases recovered after restart, `active_lease`
  accepting only the current unexpired lease, unexpired leases of
  a previous process reclaimed on open, guarded cancel and result transitions
  (including a cancel racing a completion), stale lease tokens refused,
  separate Studio and runtime queues, v1 queue migration, concurrent first
  opens initialising the store once.
- `tests/unit/automation-write-path.test.ts`: truthful messages when a sync
  trigger fails after a commit, browser contexts handing navigation to Run
  mode, polling for queued jobs.
- `tests/integration/automation-runtime.test.tsx`: a sync trigger's navigate
  and setState steps followed in the Runtime.
- `tests/unit/automation-triggers.test.ts`: sync triggers inside the write,
  old values on update, failure propagation, recursion depth limit.
- `tests/unit/automation-runner.test.ts`: step behavior, stop, continue, and
  rollback failure modes, headless contexts.
- `src-tauri/src/trigger_auth_tests.rs`: grant issue, verify, expiry,
  single use, wrong table, operation, step or trigger, user mode, forged
  tokens, job leases, and the up-front refusal for user-mode triggers.
- `tests/integration/runtime-rbac-triggers.test.tsx`: a restricted role whose
  app-mode trigger updates tables it cannot write, a user-mode trigger that
  refuses the save with no row committed, and forged trigger writes refused.
- `tests/integration/automation.test.tsx`: sync triggers, async triggers with
  retry after failure and cancel, deduplication by idempotency key, and
  rollback-mode actions as one RecordStore transaction.

## Audit log

- 2026-10-05: Stated the worker's polling and wake conditions, the role check
  for user-mode jobs, and silent `STALE_LEASE` handling; added the
  `active_lease` test to Evidence.
- 2026-10-10: Phase 7 before-change and record-deleted triggers.
