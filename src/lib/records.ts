import { call } from "./api";
import type { DataValue, NamedValue } from "./types";
import { newId } from "./utils";

export type RecordOperation = "insert" | "update" | "delete";

export interface RecordWrite {
  operation: RecordOperation;
  table: string;
  values: NamedValue[];
  identity: DataValue[] | null;
  /** Workflow context supplied by the caller (automation uses it for trigger semantics). */
  meta?: RecordWriteMeta;
}

export interface RecordWriteMeta {
  /** Trigger nesting depth of the workflow issuing this write (recursion guard). */
  triggerDepth?: number;
  /** Previous values of the updated row, when the caller knows them (`old` in trigger scopes). */
  old?: Record<string, unknown>;
  /** Original values the edit started from; optimistic entities reject the write with CONFLICT if they changed. */
  expected?: NamedValue[];
  /**
   * Identity of this write call (a uuid set by records.ts when absent). A caller
   * retrying the same write passes the same id, so async triggers dedupe it.
   */
  writeId?: string;
  /**
   * Write straight to the store even when the entity's policy is `customAction`.
   * Set by the steps of a running custom action, so they do not re-route to it.
   */
  direct?: boolean;
  /** Marks the write as a step of an app-mode trigger (see src-tauri/src/trigger_auth.rs). */
  trigger?: TriggerStepAuth;
  /** Set on the one write that stands for a whole action query (see `runActionQuery`). */
  bulk?: BulkChange;
}

/** The rows an action query created, updated or deleted, for firing triggers once per row. */
export interface BulkChange {
  created: DataValue[][];
  updated: { identity: DataValue[]; old: NamedValue[] }[];
  /** Deleted rows with every column's value before the statement. */
  deleted?: { identity: DataValue[]; values: NamedValue[] }[];
  /** Sync app-mode trigger grants Rust issued for each event; shared by its rows. */
  createdGrant?: string | null;
  updatedGrant?: string | null;
  deletedGrant?: string | null;
}

/** A row an action query is about to create or update, for before-change triggers. */
export interface PendingRow {
  identity: DataValue[];
  /** Every column's new value. */
  values: NamedValue[];
  /** Every column's value before an update; null for a created row. */
  old: NamedValue[] | null;
}

/** Fields before-change triggers set on one pending row, by its position in `pending.rows`. */
export interface RowOverride {
  row: number;
  values: NamedValue[];
}

/** What `run_action_query` returns (src-tauri/src/queries/action.rs). */
export interface ActionQueryRun extends BulkChange {
  /** Rows the statement matched (for `replace`: rows inserted). */
  changed: number;
  /** Rows a `replace` query removed first. */
  removed: number;
  dryRun: boolean;
  table: string;
  /** Rows awaiting before-change triggers: nothing was written yet (see `runActionQuery`). */
  pending?: { fingerprint: string; rows: PendingRow[] };
}

/** What an app-mode trigger presents to Rust: its sync grant, or its job lease. */
export interface TriggerAuth {
  triggerId: string;
  grant?: string;
  jobId?: string;
  leaseToken?: string;
}
export type TriggerStepAuth = TriggerAuth & { stepId: string };

/** Extra outcome of a committed write: the grant for its app-mode sync triggers. */
export interface WriteExtra {
  triggerGrant?: string;
}

/**
 * Thrown when the write committed but an after hook (a sync trigger) failed.
 * The record changes are saved; `results` holds the write results.
 */
export class CommittedWriteError extends Error {
  readonly committed = true;
  constructor(
    message: string,
    readonly results: unknown[],
  ) {
    super(message);
  }
}

/**
 * Takes over an update or delete instead of writing it (custom-action entities).
 * Returns undefined to let the write go to the store.
 */
export type RecordRouter = (write: RecordWrite) => Promise<number> | undefined;
let router: RecordRouter | null = null;

/** Installs the update/delete router (one at a time); returns its uninstall function. */
export function setRecordRouter(next: RecordRouter): () => void {
  router = next;
  return () => {
    if (router === next) router = null;
  };
}

export interface RecordHook {
  // Runs before the write; throwing aborts it, returning a write replaces it (before-change triggers).
  before?: (write: RecordWrite) => void | RecordWrite | Promise<void | RecordWrite>;
  // Runs before an action query writes its rows (before-change triggers); throwing aborts the query.
  beforeBulk?: (table: string, rows: PendingRow[], triggerDepth: number) => Promise<RowOverride[]>;
  // Runs after the write commits, with the command result.
  after?: (write: RecordWrite, result: unknown, extra?: WriteExtra) => void | Promise<void>;
}

const hooks = new Set<RecordHook>();

/** Fired once per tick after record writes commit (narrower than `ixtable:database-changed`). */
export const RECORDS_CHANGED_EVENT = "ixtable:records-changed";
let announcing = false;

function announceChange() {
  if (announcing || typeof window === "undefined") return;
  announcing = true;
  setTimeout(() => {
    announcing = false;
    window.dispatchEvent(new Event(RECORDS_CHANGED_EVENT));
  }, 0);
}

/** Registers a hook on every record write and returns its unregister function. */
export function registerRecordHook(hook: RecordHook): () => void {
  hooks.add(hook);
  return () => {
    hooks.delete(hook);
  };
}

/** Gives the write a `meta.writeId` (kept when the caller supplied one). */
function withWriteId(record: RecordWrite): RecordWrite {
  return record.meta?.writeId ? record : { ...record, meta: { ...record.meta, writeId: newId() } };
}

interface Outcome {
  changed: number;
  identity?: DataValue[] | null;
  triggerGrant?: string;
}

const toOp = ({ operation, table, values, identity, meta }: RecordWrite) =>
  operation === "insert"
    ? { op: "insert", table, values }
    : operation === "update"
      ? { op: "update", table, values, identity, expected: meta?.expected ?? null }
      : { op: "delete", table, identity, expected: meta?.expected ?? null };

const resultOf = (record: RecordWrite, outcome: Outcome | undefined) =>
  record.operation === "insert" ? (outcome?.identity ?? []) : (outcome?.changed ?? 0);

/**
 * Runs after hooks for committed writes, each with its result and trigger grant;
 * a failure becomes a CommittedWriteError (the writes stay saved).
 */
async function afterCommit(
  writes: RecordWrite[],
  outcomes: (Outcome | undefined)[],
  active: RecordHook[],
): Promise<unknown[]> {
  const results = writes.map((record, i) => resultOf(record, outcomes[i]));
  const failures: string[] = [];
  for (const [i, record] of writes.entries()) {
    const grant = outcomes[i]?.triggerGrant;
    for (const hook of active) {
      try {
        await hook.after?.(record, results[i], grant ? { triggerGrant: grant } : undefined);
      } catch (e) {
        failures.push(e instanceof Error ? e.message : String(e));
      }
    }
  }
  if (failures.length) throw new CommittedWriteError(failures.join("; "), results);
  return results;
}

async function write<T>(
  input: RecordWrite,
  send: (record: RecordWrite) => Promise<Outcome>,
): Promise<T> {
  const record = withWriteId(input);
  if (record.operation !== "insert" && !record.meta?.direct) {
    const routed = router?.(record);
    if (routed) return (await routed) as T;
  }
  const active = [...hooks];
  const final = await beforeAll(record, active);
  const outcome = await send(final);
  announceChange();
  const [result] = await afterCommit([final], [outcome], active);
  return result as T;
}

/** Runs the before hooks in turn; each may replace the write. */
async function beforeAll(record: RecordWrite, active: RecordHook[]): Promise<RecordWrite> {
  let current = record;
  for (const hook of active) current = (await hook.before?.(current)) ?? current;
  return current;
}

const trigger = (record: RecordWrite) => record.meta?.trigger ?? null;

/** Inserts a row and resolves with the new row's identity values. */
export const insertRecord = (table: string, values: NamedValue[], meta?: RecordWriteMeta) =>
  write<DataValue[]>(
    { operation: "insert", table, values, identity: null, ...(meta && { meta }) },
    (record) =>
      call<Outcome>("insert_row", { table, values: record.values, trigger: trigger(record) }),
  );

/**
 * Updates one row by identity and resolves with the affected row count. On a
 * `customAction` entity the entity's action runs instead (see src/automation/custom.ts).
 */
export const updateRecord = (
  table: string,
  values: NamedValue[],
  identity: DataValue[],
  meta?: RecordWriteMeta,
) =>
  write<number>({ operation: "update", table, values, identity, ...(meta && { meta }) }, (record) =>
    call<Outcome>("update_row", {
      table,
      values: record.values,
      identity,
      expected: meta?.expected ?? null,
      trigger: trigger(record),
    }),
  );

/** Deletes one row by identity and resolves with the affected row count (custom actions as above). */
export const deleteRecord = (table: string, identity: DataValue[], meta?: RecordWriteMeta) =>
  write<number>(
    { operation: "delete", table, values: [], identity, ...(meta && { meta }) },
    (record) =>
      call<Outcome>("delete_row", {
        table,
        identity,
        expected: meta?.expected ?? null,
        trigger: trigger(record),
      }),
  );

/**
 * Applies several writes as one RecordStore transaction (`execute_write_batch`):
 * all succeed or none do. Before hooks run for every write first (any throw aborts
 * the batch); after hooks run once it commits, each with its own result (identity
 * for inserts, affected count otherwise). A failing after hook rejects with a
 * CommittedWriteError: the batch is saved. Writes are never routed to custom
 * actions here; the action runner routes them before batching.
 */
export async function writeRecordBatch(input: RecordWrite[]): Promise<unknown[]> {
  const active = [...hooks];
  const writes: RecordWrite[] = [];
  for (const record of input.map(withWriteId)) writes.push(await beforeAll(record, active));
  const outcomes = await call<Outcome[]>("execute_write_batch", {
    ops: writes.map(toOp),
    triggers: writes.map((w) => w.meta?.trigger ?? null),
  });
  announceChange();
  return afterCommit(writes, outcomes, active);
}

/**
 * Runs a saved action query (docs/decisions/action-queries.md). With `dryRun`
 * the changes roll back and only the counts come back. On a table with
 * before-change triggers the first call writes nothing and returns the pending
 * rows: the `beforeBulk` hooks decide on them (a rejection ends the run with
 * nothing written), and a second call writes the query with the fields they
 * set. Then every record hook runs once with a single write that stands for
 * the whole query (its `meta.bulk` lists the created, updated and deleted
 * rows); the trigger hook fires the table's triggers once per row. A failing
 * trigger rejects with a CommittedWriteError: the query's changes are saved.
 */
export async function runActionQuery(
  queryId: string,
  params: NamedValue[],
  options: { dryRun?: boolean; triggerDepth?: number } = {},
): Promise<ActionQueryRun> {
  const request = { id: queryId, params, dryRun: options.dryRun ?? false };
  let run = await call<ActionQueryRun>("run_action_query", request);
  if (run.pending) {
    const active = [...hooks];
    const overrides: RowOverride[] = [];
    for (const hook of active)
      overrides.push(
        ...((await hook.beforeBulk?.(run.table, run.pending.rows, options.triggerDepth ?? 0)) ??
          []),
      );
    const before = { fingerprint: run.pending.fingerprint, overrides };
    run = await call<ActionQueryRun>("run_action_query", { ...request, before });
  }
  if (run.dryRun) return run;
  announceChange();
  const operation: RecordOperation = run.updated.length ? "update" : "insert";
  const record: RecordWrite = {
    operation,
    table: run.table,
    values: [],
    identity: null,
    meta: { writeId: newId(), triggerDepth: options.triggerDepth ?? 0, bulk: run },
  };
  await afterCommit([record], [{ changed: run.changed }], [...hooks]);
  return run;
}
