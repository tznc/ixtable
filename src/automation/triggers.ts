/**
 * Record triggers (PRD §17.3), hooked into src/lib/records.ts.
 *
 * Before-change triggers run before a create or update is sent: their actions
 * may set fields of the record (setField) or reject the save (a fail step or
 * any failing step), and cannot write records, so a rejection writes nothing.
 * Action queries hand them each pending row through `beforeBulk`.
 *
 * Created, updated and deleted triggers run after the write commits. Sync
 * triggers run their action after the initiating write commits, inside the
 * same workflow: the caller's insertRecord/updateRecord awaits them and a failing
 * trigger makes that call reject. The write itself is already committed then;
 * rollback-mode actions only undo their own writes.
 * Async triggers enqueue a job on the durable queue (jobs.rs); the worker runs it.
 * Writes carry their trigger depth; a chain deeper than MAX_TRIGGER_DEPTH fails.
 *
 * Execution identity (`Trigger.runAs`): app-mode triggers (the default) run with
 * the grant Rust returned for the initiating write (sync) or their job lease
 * (async), limited to the writes they declare; user-mode triggers run under the
 * signed-in user's role, and Rust refuses the initiating write up front when the
 * role could not run them.
 */
import { evaluate, evaluateBoolean } from "../expr";
import { inspectTable, readTablePage } from "../lib/api";
import {
  type BulkChange,
  type PendingRow,
  type RecordWrite,
  type RowOverride,
  registerRecordHook,
  type WriteExtra,
} from "../lib/records";
import { activeRoleId } from "../runtime/rbac";
import type { DataValue, DocumentConfig, Filter } from "../lib/types";
import { enqueueJob, notifyJobsChanged, releaseTriggerGrant } from "./api";
import { type ActionContext, runAction } from "./runner";
import type { Trigger, TriggerEvent } from "./types";
import {
  fromDataValue,
  namedToObject,
  rowToObject,
  stableHash,
  toDataValue,
  toNamedValues,
} from "./values";

export const MAX_TRIGGER_DEPTH = 5;

/** What an async job carries to the worker. */
export interface JobPayload {
  table: string;
  event: TriggerEvent;
  record: Record<string, unknown>;
  old: Record<string, unknown> | null;
  identity: unknown[];
  triggerDepth: number;
  /** Role active when the job was created (null: developer); the worker never exceeds it. */
  roleId?: string | null;
}

export interface TriggerEnv {
  getConfig(): DocumentConfig;
  /**
   * Builds the interactive context sync trigger and custom actions run with.
   * `triggerAuth` is set for app-mode triggers: the context must then skip role checks.
   */
  context(base: Partial<ActionContext>): ActionContext;
  app?(): Record<string, unknown>;
}

const enabledFor = (config: DocumentConfig, table: string, event: TriggerEvent) =>
  (config.triggers ?? []).filter((t) => t.enabled && t.table === table && t.event === event);

/** Thrown when a before-change trigger rejects a save; nothing was written. */
export class BeforeChangeRejected extends Error {}

/**
 * Registers the trigger hook on record writes; returns its unregister function.
 * Before an update with `updated` or `beforeChange` triggers, or a delete with
 * `deleted` triggers, it reads the row so `old` is available even when the
 * caller did not pass `meta.old`. Before-change triggers then decide the write.
 */
export function installTriggers(env: TriggerEnv): () => void {
  const previous = new WeakMap<RecordWrite, Record<string, unknown> | null>();
  return registerRecordHook({
    before: async (write) => {
      const config = env.getConfig();
      const has = (event: TriggerEvent) => enabledFor(config, write.table, event).length > 0;
      const needsOld =
        write.operation === "update"
          ? has("updated") || has("beforeChange")
          : write.operation === "delete" && has("deleted");
      const old =
        write.meta?.old ??
        (needsOld && write.identity ? await readRow(write.table, write.identity) : null);
      const next =
        write.operation === "delete" || !has("beforeChange")
          ? write
          : await beforeChange(write, old, env);
      if (needsOld && !write.meta?.old) previous.set(next, old);
      return next;
    },
    beforeBulk: (table, rows, depth) => beforeChangeBulk(table, rows, depth, env),
    after: (write, result, extra) =>
      write.meta?.bulk
        ? dispatchBulk(write, write.meta.bulk, env)
        : dispatchTriggers(write, result, env, previous.get(write) ?? undefined, extra),
  });
}

/**
 * Default idempotency key: `${triggerId}:${table}:${pk}:${event}:${hash(values)}:${writeId}`.
 * The write id (one per records.ts write call) makes it dedupe retries of the
 * same write only; a later identical write is a new event and enqueues again.
 */
export function defaultIdempotencyKey(
  trigger: Trigger,
  identity: unknown[],
  values: Record<string, unknown>,
  writeId?: string,
): string {
  const key = `${trigger.id}:${trigger.table}:${identity.map(String).join(",")}:${trigger.event}:${stableHash(values)}`;
  return writeId ? `${key}:${writeId}` : key;
}

export async function dispatchTriggers(
  write: RecordWrite,
  result: unknown,
  env: TriggerEnv,
  oldRow?: Record<string, unknown>,
  extra?: WriteExtra,
): Promise<void> {
  const grant = extra?.triggerGrant;
  try {
    await dispatch(write, result, env, oldRow, grant);
  } finally {
    // The grant is single use: it ends with this trigger run.
    if (grant) await releaseTriggerGrant(grant).catch(() => undefined);
  }
}

/**
 * Runs the before-change triggers of a create or update: returns the write with
 * the fields they set, or throws BeforeChangeRejected.
 */
async function beforeChange(
  write: RecordWrite,
  old: Record<string, unknown> | null,
  env: TriggerEnv,
): Promise<RecordWrite> {
  const values = namedToObject(write.values);
  const record = write.operation === "update" ? { ...old, ...values } : values;
  const isNew = write.operation === "insert";
  const set = await decide(write.table, record, isNew ? null : old, write.meta?.triggerDepth, env);
  if (!Object.keys(set).length) return write;
  return {
    ...write,
    values: [...write.values.filter((v) => !(v.column in set)), ...toNamedValues(set)],
  };
}

/** Runs the before-change triggers on each row an action query is about to write. */
async function beforeChangeBulk(
  table: string,
  rows: PendingRow[],
  depth: number,
  env: TriggerEnv,
): Promise<RowOverride[]> {
  if (!enabledFor(env.getConfig(), table, "beforeChange").length) return [];
  const overrides: RowOverride[] = [];
  for (const [i, row] of rows.entries()) {
    const old = row.old ? namedToObject(row.old) : null;
    const set = await decide(table, namedToObject(row.values), old, depth, env);
    if (Object.keys(set).length) overrides.push({ row: i, values: toNamedValues(set) });
  }
  return overrides;
}

/**
 * Runs a table's before-change triggers in order on `record` (each sees the
 * fields earlier ones set) and returns the fields they set. A failing action
 * rejects: a fail step with its own message, anything else naming the trigger.
 */
async function decide(
  table: string,
  record: Record<string, unknown>,
  old: Record<string, unknown> | null,
  triggerDepth: number | undefined,
  env: TriggerEnv,
): Promise<Record<string, unknown>> {
  const set: Record<string, unknown> = {};
  const app = env.app?.() ?? {};
  for (const trigger of enabledFor(env.getConfig(), table, "beforeChange")) {
    const current = { ...record, ...set };
    if (
      trigger.condition?.trim() &&
      !evaluateBoolean(trigger.condition, { record: current, old, app })
    )
      continue;
    const outcome = await runAction(
      trigger.actionId,
      env.context({
        record: current,
        old: old ?? undefined,
        triggerDepth: triggerDepth ?? 0,
        beforeChange: { set },
        // App mode skips the role's checks; it holds no grant, since the action cannot write.
        ...(trigger.runAs !== "user" && { triggerAuth: { triggerId: trigger.id, grant: "" } }),
      }),
    );
    if (!outcome.ok)
      throw new BeforeChangeRejected(
        `Not saved: ${
          outcome.aborted
            ? outcome.error
            : `trigger "${trigger.name}" failed: ${outcome.error ?? "unknown error"}`
        }`,
      );
  }
  return set;
}

/**
 * Fires an action query's triggers once per created, updated or deleted row, in order,
 * as if each row had been written alone. The rows of one event share Rust's
 * grant, released when all of them have run. Failures are collected so one
 * bad row does not stop the others' triggers.
 */
async function dispatchBulk(write: RecordWrite, bulk: BulkChange, env: TriggerEnv): Promise<void> {
  const failures: string[] = [];
  const base = { triggerDepth: write.meta?.triggerDepth ?? 0 };
  const each = async (row: RecordWrite, result: unknown, grant: string | undefined) => {
    try {
      await dispatch(row, result, env, undefined, grant);
    } catch (e) {
      failures.push(e instanceof Error ? e.message : String(e));
    }
  };
  try {
    for (const [i, identity] of bulk.created.entries())
      await each(
        {
          operation: "insert",
          table: write.table,
          values: [],
          identity: null,
          meta: { ...base, writeId: `${write.meta?.writeId}:c${i}` },
        },
        identity,
        bulk.createdGrant ?? undefined,
      );
    for (const [i, row] of bulk.updated.entries())
      await each(
        {
          operation: "update",
          table: write.table,
          values: [],
          identity: row.identity,
          meta: { ...base, writeId: `${write.meta?.writeId}:u${i}`, old: namedToObject(row.old) },
        },
        1,
        bulk.updatedGrant ?? undefined,
      );
    for (const [i, row] of (bulk.deleted ?? []).entries())
      await each(
        {
          operation: "delete",
          table: write.table,
          values: [],
          identity: row.identity,
          meta: {
            ...base,
            writeId: `${write.meta?.writeId}:d${i}`,
            old: namedToObject(row.values),
          },
        },
        1,
        bulk.deletedGrant ?? undefined,
      );
  } finally {
    for (const grant of [bulk.createdGrant, bulk.updatedGrant, bulk.deletedGrant])
      if (grant) await releaseTriggerGrant(grant).catch(() => undefined);
  }
  if (failures.length) throw new Error(failures.join("; "));
}

async function dispatch(
  write: RecordWrite,
  result: unknown,
  env: TriggerEnv,
  oldRow: Record<string, unknown> | undefined,
  grant: string | undefined,
): Promise<void> {
  const event: TriggerEvent =
    write.operation === "insert" ? "created" : write.operation === "update" ? "updated" : "deleted";
  const triggers = enabledFor(env.getConfig(), write.table, event);
  if (!triggers.length) return;
  const depth = (write.meta?.triggerDepth ?? 0) + 1;
  if (depth > MAX_TRIGGER_DEPTH)
    throw new Error(
      `Trigger recursion limit (${MAX_TRIGGER_DEPTH}) exceeded on ${write.table}; check triggers that write to their own table`,
    );
  const identity =
    (write.operation === "insert" ? (result as DataValue[] | null) : write.identity) ?? [];
  const values = namedToObject(write.values);
  const old = write.meta?.old ?? oldRow ?? null;
  // A deleted row is gone: its triggers see the values it had.
  const record =
    event === "deleted"
      ? (old ?? {})
      : ((await readRow(write.table, identity)) ?? {
          ...values,
          ...(identity.length === 1 && { rowid: fromDataValue(identity[0]) }),
        });
  const app = env.app?.() ?? {};
  for (const trigger of triggers) {
    const scope = { record, old, app };
    if (trigger.condition?.trim() && !evaluateBoolean(trigger.condition, scope)) continue;
    if (trigger.mode === "async") {
      const plainIdentity = identity.map(fromDataValue);
      const key = trigger.idempotencyKey?.trim()
        ? String(evaluate(trigger.idempotencyKey, { ...scope, trigger: { id: trigger.id } }))
        : defaultIdempotencyKey(trigger, plainIdentity, values, write.meta?.writeId);
      const payload: JobPayload = {
        table: write.table,
        event,
        record,
        old,
        identity: plainIdentity,
        triggerDepth: depth,
        roleId: activeRoleId(),
      };
      await enqueueJob({
        triggerId: trigger.id,
        actionId: trigger.actionId,
        payload,
        idempotencyKey: key,
        maxAttempts: trigger.maxAttempts,
        backoffMs: trigger.backoffMs,
      });
      notifyJobsChanged();
      continue;
    }
    const asApp = trigger.runAs !== "user";
    const outcome = await runAction(
      trigger.actionId,
      env.context({
        record,
        old: old ?? undefined,
        triggerDepth: depth,
        ...(asApp && { triggerAuth: { triggerId: trigger.id, grant: grant ?? "" } }),
      }),
    );
    if (!outcome.ok)
      throw new Error(`Trigger "${trigger.name}" failed: ${outcome.error ?? "unknown error"}`);
  }
}

/** Reads one row by identity (primary key values, or rowid); null when it can't. */
export async function readRow(
  table: string,
  identity: DataValue[],
): Promise<Record<string, unknown> | null> {
  try {
    const keys = (await inspectTable(table)).columns
      .filter((c) => c.primaryKeyPosition > 0)
      .sort((a, b) => a.primaryKeyPosition - b.primaryKeyPosition)
      .map((c) => c.name);
    if (!keys.length || keys.length !== identity.length) return null;
    const filters: Filter[] = keys.map((column, i) => ({
      column,
      operator: "eq",
      value: identity[i] ?? toDataValue(null),
    }));
    const page = await readTablePage(table, { filters, limit: 1 });
    return page.rows[0] ? rowToObject(page.columns, page.rows[0]) : null;
  } catch {
    return null;
  }
}
