/**
 * Declarative action runner (PRD §17.2). Every expression goes through src/expr,
 * every record write through src/lib/records.ts (so triggers see it), and every
 * read through the DuckDB read path.
 *
 * Failure behavior (`ActionDef.onError`):
 * - stop: the first failing step ends the action; earlier writes stay committed.
 * - continue: failing steps are logged and skipped; the action reports ok.
 * - rollback: a transactional action. Record writes are collected and applied at
 *   the end as one RecordStore transaction (`writeRecordBatch`); a failing step or a
 *   failing commit saves none of them. Steps read the data as it was before the
 *   action (pending writes are not visible to later lookups, and `storeAs` on a
 *   created record holds its values, not its generated key). Navigation, messages
 *   and state changes are held back until the commit succeeds.
 * A declined `confirm` always ends the action (saving nothing in rollback mode).
 * A `fail` step always ends the action with its message as the error, even under
 * onError=continue (saving nothing in rollback mode).
 * Nested runAction steps join the caller's transaction when there is one.
 * Updates and deletes send the matched rows' original values (`expected`), so
 * optimistic entities reject them if the row changed since it was read. A row
 * written earlier in the same run carries its post-write values forward as the
 * next write's `expected` (rollback mode, where lookups see the data as it was
 * before the action, and snapshot rows); a row deleted earlier fails the step. On a
 * `customAction` entity they run the entity's action instead (src/automation/custom.ts),
 * joining the caller's transaction like a nested runAction.
 * If the writes commit but a sync trigger on them fails, the action reports that
 * failure with the changes saved; held-back effects and the refresh still run.
 */
import { evaluate, evaluateBoolean } from "../expr";
import { call, executeReadQuery } from "../lib/api";
import {
  CommittedWriteError,
  deleteRecord,
  insertRecord,
  type RecordWrite,
  type RecordWriteMeta,
  runActionQuery,
  type TriggerAuth,
  type TriggerStepAuth,
  updateRecord,
  writeRecordBatch,
} from "../lib/records";
import type { DataValue, DocumentConfig, NamedValue, QueryResult } from "../lib/types";
import * as queryApi from "../query/api";
import { ACTION_QUERY_OPS, type ActionSpec } from "../query/types";
import { customActionFor, customScope } from "./custom";
import { afterWrite, currentRow, type FoundRow, matchRows, rowKey, withKeys } from "./rows";
import { STEP_LABELS } from "./steps";
import {
  type ActionDef,
  BEFORE_CHANGE_STEPS,
  type MatchSpec,
  type OnError,
  type Step,
  type StepLog,
  type ValueMap,
} from "./types";
import { rowToObject, toNamedValues } from "./values";

export { ActionPicker } from "./ActionPicker";

export interface NavigationTarget {
  kind: "form" | "report" | "dashboard" | "table";
  id: string;
  mode?: string;
  recordId?: unknown;
  params?: Record<string, unknown>;
}

export interface ActionContext {
  config: DocumentConfig;
  record?: Record<string, unknown>;
  form?: Record<string, unknown>;
  app: Record<string, unknown>;
  params?: Record<string, unknown>;
  navigate(target: NavigationTarget): void;
  setState(scope: "app" | "form", key: string, value: unknown): void;
  confirm(message: string): Promise<boolean>;
  notify(message: string, tone?: "info" | "error"): void;
  authorize?(kind: string, id: string, op: string): boolean;
  refresh?(): void;
  /** Previous values of `record` (update triggers). */
  old?: Record<string, unknown>;
  /**
   * Set when an app-mode trigger runs this action: record steps present it to Rust
   * (with their step id) instead of the user's role.
   */
  triggerAuth?: TriggerAuth;
  /** Trigger nesting depth of this run; writes carry it so triggers can stop recursion. */
  triggerDepth?: number;
  /** Clock for today()/now(); defaults to the current time. */
  now?: Date;
  /** Set while a custom concurrency action runs: its record steps write to the store directly. */
  directWrites?: boolean;
  /** The current record as loaded (before unsaved edits); `expected` for a keyless current row. */
  snapshot?: Record<string, unknown>;
  /** Original values a write of this row must match (custom actions: what the caller edited). */
  current?: { table: string; identity: DataValue[]; expected: NamedValue[] };
  /**
   * Set while a before-change trigger runs: setField steps collect the record's
   * new field values in `set`, and steps that write or touch the UI are refused.
   */
  beforeChange?: { set: Record<string, unknown> };
}

export interface ActionResult {
  ok: boolean;
  error?: string;
  /** True when a confirm step was declined. */
  cancelled?: boolean;
  /** True when a fail step ended the action (`error` is its message). */
  aborted?: boolean;
  steps: StepLog[];
  /** Values stored by steps with `storeAs`. */
  results: Record<string, unknown>;
}

/** Maximum nesting of runAction steps. */
export const MAX_ACTION_DEPTH = 10;

class Cancelled extends Error {
  constructor() {
    super("Cancelled by user");
  }
}
class StepFailure extends Error {}
/** A `fail` step: a business-rule abort whose message is the action's error. */
class Aborted extends StepFailure {}

/** Writes and UI effects held back until a rollback-mode action commits. */
interface Transaction {
  writes: RecordWrite[];
  effects: (() => void)[];
}
interface Frame {
  ctx: ActionContext;
  scope: Record<string, unknown> & { results: Record<string, unknown> };
  logs: StepLog[];
  onError: OnError;
  tx: Transaction | null;
  stack: string[];
  wrote: { value: boolean };
  /** Rows written in this run: their `expected` from now on, or null once deleted. */
  seen: Map<string, NamedValue[] | null>;
}

const message = (e: unknown) =>
  e instanceof Error ? e.message : typeof e === "string" ? e : JSON.stringify(e);

/** Runs an action (or the action with this id) against `ctx`. Never throws. */
export async function runAction(
  action: ActionDef | string,
  ctx: ActionContext,
): Promise<ActionResult> {
  const results: Record<string, unknown> = {};
  const def = typeof action === "string" ? ctx.config.actions.find((a) => a.id === action) : action;
  if (!def)
    return { ok: false, error: `Action ${String(action)} does not exist`, steps: [], results };
  if (ctx.authorize && !ctx.authorize("action", def.id, "execute"))
    return { ok: false, error: "Not permitted", steps: [], results };
  const frame: Frame = {
    ctx,
    scope: {
      record: ctx.record ?? null,
      old: ctx.old ?? null,
      form: { ...ctx.form },
      app: { ...ctx.app },
      params: { ...ctx.params },
      results,
      steps: results,
    },
    logs: [],
    onError: def.onError ?? "stop",
    tx: null,
    stack: [],
    wrote: { value: false },
    seen: new Map(
      ctx.current ? [[rowKey(ctx.current.table, ctx.current.identity), ctx.current.expected]] : [],
    ),
  };
  const outcome = await execute(def, frame, "");
  if (frame.wrote.value) ctx.refresh?.();
  return { ...outcome, steps: frame.logs, results };
}

async function execute(
  action: ActionDef,
  parent: Frame,
  prefix: string,
): Promise<{ ok: boolean; error?: string; cancelled?: boolean; aborted?: boolean }> {
  const own: Transaction | null =
    (action.onError ?? "stop") === "rollback" && !parent.tx ? { writes: [], effects: [] } : null;
  const frame: Frame = {
    ...parent,
    onError: action.onError ?? "stop",
    tx: parent.tx ?? own,
    stack: [...parent.stack, action.id],
  };
  try {
    await runSteps(action.steps ?? [], frame, prefix);
  } catch (e) {
    const unsaved = own?.writes.length ? "; no record changes were saved" : "";
    return {
      ok: false,
      error: message(e) + unsaved,
      ...(e instanceof Cancelled && { cancelled: true }),
      ...(e instanceof Aborted && { aborted: true }),
    };
  }
  if (!own) return { ok: true };
  let triggerError: string | null = null;
  if (own.writes.length) {
    try {
      await writeRecordBatch(own.writes);
      frame.wrote.value = true;
    } catch (e) {
      if (!(e instanceof CommittedWriteError))
        return {
          ok: false,
          error: `Transaction failed: ${message(e)}; no record changes were saved`,
        };
      frame.wrote.value = true;
      triggerError = `The record changes were saved, but ${lowerFirst(e.message)}`;
    }
  }
  try {
    for (const effect of own.effects) effect();
  } catch (e) {
    return { ok: false, error: `${message(e)} (after the record changes were saved)` };
  }
  return triggerError ? { ok: false, error: triggerError } : { ok: true };
}

async function runSteps(steps: Step[], frame: Frame, prefix: string): Promise<void> {
  for (const [index, step] of steps.entries()) {
    const path = prefix ? `${prefix}.${index}` : String(index);
    const log: StepLog = { stepIndex: index, path, kind: step.kind, ok: true, durationMs: 0 };
    frame.logs.push(log);
    const started = performance.now();
    try {
      if (step.kind !== "condition" && step.when?.trim() && !condition(step.when, frame)) {
        log.skipped = true;
        continue;
      }
      await runStep(step, frame, path);
    } catch (e) {
      log.ok = false;
      log.error = message(e);
      if (e instanceof Cancelled || e instanceof Aborted) throw e;
      if (frame.onError !== "continue")
        throw e instanceof StepFailure
          ? e
          : new StepFailure(`Step ${Number(index) + 1} (${step.kind}) failed: ${log.error}`);
    } finally {
      log.durationMs = Math.round(performance.now() - started);
    }
  }
}

const opts = (frame: Frame) => (frame.ctx.now ? { now: frame.ctx.now } : {});

function expr(src: string | undefined, frame: Frame, what: string): unknown {
  if (!src?.trim()) throw new Error(`${what} has no expression`);
  return evaluate(src, frame.scope, opts(frame));
}
function condition(src: string, frame: Frame): boolean {
  return evaluateBoolean(src, frame.scope, opts(frame));
}
function evalMap(map: ValueMap | undefined, frame: Frame): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(map ?? {}).map(([key, src]) => [key, expr(src, frame, `Value for ${key}`)]),
  );
}
function authorize(frame: Frame, kind: string, id: string, op: string) {
  if (frame.ctx.authorize && !frame.ctx.authorize(kind, id, op)) throw new Error("Not permitted");
}
/** An app-mode trigger's proof for one step's writes and lookups (src-tauri/src/trigger_auth.rs). */
const stepAuth = (frame: Frame, step: Step): TriggerStepAuth | undefined =>
  frame.ctx.triggerAuth && { ...frame.ctx.triggerAuth, stepId: step.id };
const writeMeta = (frame: Frame, step: Step, extra: RecordWriteMeta = {}): RecordWriteMeta => {
  const trigger = stepAuth(frame, step);
  return {
    triggerDepth: frame.ctx.triggerDepth ?? 0,
    ...(frame.ctx.directWrites && { direct: true }),
    ...extra,
    ...(trigger && { trigger }),
  };
};
const lowerFirst = (text: string) => text.charAt(0).toLowerCase() + text.slice(1);
/** Runs a direct (non-transactional) write; a committed write counts even if a trigger failed. */
async function direct(frame: Frame, run: () => Promise<unknown>) {
  try {
    await run();
    frame.wrote.value = true;
  } catch (e) {
    if (!(e instanceof CommittedWriteError)) throw e;
    frame.wrote.value = true;
    throw new Error(`The record was saved, but ${lowerFirst(e.message)}`, { cause: e });
  }
}
/** Runs a custom-action entity's action for one row, joining the caller's transaction. */
async function runCustom(
  action: ActionDef,
  frame: Frame,
  path: string,
  request: Parameters<typeof customScope>[0],
  identity: DataValue[],
) {
  if (frame.stack.length >= MAX_ACTION_DEPTH)
    throw new Error(`Actions nested deeper than ${MAX_ACTION_DEPTH}`);
  // Like a form save routed to it (src/automation/custom.ts), the role must be allowed to run it.
  authorize(frame, "action", action.id, "execute");
  const scope = customScope(request);
  const child: Frame = {
    ...frame,
    ctx: { ...frame.ctx, directWrites: true, snapshot: request.old },
    scope: { ...frame.scope, ...scope, form: {}, results: {} },
  };
  child.scope.steps = child.scope.results;
  if (request.expected) frame.seen.set(rowKey(request.table, identity), request.expected);
  const outcome = await execute(action, child, `${path}.custom`);
  if (outcome.cancelled) throw new Cancelled();
  if (outcome.aborted) throw new Aborted(outcome.error ?? `Action ${action.name} failed`);
  if (!outcome.ok) throw new Error(outcome.error ?? `Action ${action.name} failed`);
}
/** The custom action an update or delete step routes to (none inside a custom action). */
const routedTo = (frame: Frame, table: string) =>
  frame.ctx.directWrites ? null : customActionFor(frame.ctx.config, table);
/**
 * A runQuery step on an action query: it writes at once, so it cannot join a
 * rollback-mode action's batch. `storeAs` holds `{ changed, removed }`.
 */
async function runActionQueryStep(
  frame: Frame,
  step: Extract<Step, { kind: "runQuery" }>,
  action: ActionSpec,
) {
  if (frame.tx)
    throw new Error(
      "Action queries write at once, so they cannot run inside an action that rolls back on error",
    );
  for (const op of ACTION_QUERY_OPS[action.kind]) authorize(frame, "table", action.table, op);
  const params = toNamedValues(evalMap(step.params, frame));
  let run: { changed: number; removed: number } = { changed: 0, removed: 0 };
  await direct(frame, async () => {
    run = await runActionQuery(step.queryId, params, {
      triggerDepth: frame.ctx.triggerDepth ?? 0,
    });
  });
  if (step.storeAs?.trim())
    frame.scope.results[step.storeAs] = { changed: run.changed, removed: run.removed };
}
/** Runs a UI effect now, or after the commit inside a transaction. */
function effect(frame: Frame, run: () => void) {
  if (frame.tx) frame.tx.effects.push(run);
  else run();
}

async function runStep(step: Step, frame: Frame, path: string): Promise<void> {
  const { ctx, scope } = frame;
  if (ctx.beforeChange) refuseInBeforeChange(step, ctx.config);
  switch (step.kind) {
    case "createRecord": {
      authorize(frame, "table", step.table, "create");
      const values = evalMap(step.values, frame);
      const named = toNamedValues(values);
      if (frame.tx) {
        frame.tx.writes.push(write("insert", step.table, named, null, writeMeta(frame, step)));
        if (step.storeAs) scope.results[step.storeAs] = values;
        return;
      }
      let identity: DataValue[] = [];
      await direct(frame, async () => {
        identity = await insertRecord(step.table, named, writeMeta(frame, step));
      });
      if (step.storeAs)
        scope.results[step.storeAs] = await withKeys(
          step.table,
          values,
          identity,
          stepAuth(frame, step),
        );
      return;
    }
    case "updateRecord": {
      authorize(frame, "table", step.table, "update");
      const rows = await findRows(step.table, step.match, frame, stepAuth(frame, step));
      const values = evalMap(step.values, frame);
      const named = toNamedValues(values);
      const custom = routedTo(frame, step.table);
      for (const row of rows) {
        const meta = writeMeta(frame, step, { old: row.object, expected: row.expected });
        if (custom)
          await runCustom(
            custom,
            frame,
            path,
            {
              operation: "update",
              table: step.table,
              values: named,
              old: row.object,
              expected: row.expected,
            },
            row.identity,
          );
        else {
          if (frame.tx)
            frame.tx.writes.push(write("update", step.table, named, row.identity, meta));
          else await direct(frame, () => updateRecord(step.table, named, row.identity, meta));
          remember(frame, step.table, row, afterWrite(row.expected, named));
        }
      }
      if (step.match === "current" && scope.record && typeof scope.record === "object")
        scope.record = { ...(scope.record as Record<string, unknown>), ...values };
      return;
    }
    case "deleteRecord": {
      authorize(frame, "table", step.table, "delete");
      const rows = await findRows(step.table, step.match, frame, stepAuth(frame, step));
      const custom = routedTo(frame, step.table);
      for (const row of rows) {
        const meta = writeMeta(frame, step, { old: row.object, expected: row.expected });
        if (custom)
          await runCustom(
            custom,
            frame,
            path,
            {
              operation: "delete",
              table: step.table,
              values: [],
              old: row.object,
              expected: row.expected,
            },
            row.identity,
          );
        else {
          if (frame.tx) frame.tx.writes.push(write("delete", step.table, [], row.identity, meta));
          else await direct(frame, () => deleteRecord(step.table, row.identity, meta));
          remember(frame, step.table, row, null);
        }
      }
      return;
    }
    case "runQuery": {
      const action = ctx.config.savedQueries.find((q) => q.id === step.queryId)?.action;
      if (action) {
        await runActionQueryStep(frame, step, action);
        return;
      }
      authorize(frame, "query", step.queryId, "read");
      const params = evalMap(step.params, frame);
      const result = await runQuery(ctx.config, step.queryId, params);
      const rows = result.rows.map((row) => rowToObject(result.columns, row));
      if (step.storeAs?.trim()) scope.results[step.storeAs] = rows;
      return;
    }
    case "navigate": {
      const target = step.target;
      if (!target?.id) throw new Error("Navigation target is not set");
      exists(ctx.config, target.kind, target.id);
      const to: NavigationTarget = {
        kind: target.kind,
        id: target.id,
        ...(target.mode && { mode: target.mode }),
        ...(target.recordId?.trim() && { recordId: expr(target.recordId, frame, "Record id") }),
      };
      effect(frame, () => ctx.navigate(to));
      return;
    }
    case "openForm": {
      exists(ctx.config, "form", step.formId);
      const to: NavigationTarget = {
        kind: "form",
        id: step.formId,
        ...(step.mode && { mode: step.mode }),
        ...(step.recordId?.trim() && { recordId: expr(step.recordId, frame, "Record id") }),
      };
      effect(frame, () => ctx.navigate(to));
      return;
    }
    case "openReport": {
      exists(ctx.config, "report", step.reportId);
      const params = evalMap(step.params, frame);
      effect(frame, () => ctx.navigate({ kind: "report", id: step.reportId, params }));
      return;
    }
    case "openDashboard": {
      exists(ctx.config, "dashboard", step.dashboardId);
      const params = evalMap(step.params, frame);
      effect(frame, () => ctx.navigate({ kind: "dashboard", id: step.dashboardId, params }));
      return;
    }
    case "setState": {
      if (!step.key?.trim()) throw new Error("State key is not set");
      const value = expr(step.value, frame, `State ${step.key}`);
      effect(frame, () => ctx.setState(step.scope, step.key, value));
      const bucket = step.scope === "form" ? "form" : "app";
      scope[bucket] = { ...(scope[bucket] as Record<string, unknown>), [step.key]: value };
      return;
    }
    case "confirm": {
      const text = expr(step.message, frame, "Confirmation message");
      if (!(await ctx.confirm(String(text ?? "")))) throw new Cancelled();
      return;
    }
    case "message": {
      const text = String(expr(step.text, frame, "Message") ?? "");
      effect(frame, () => ctx.notify(text, step.tone ?? "info"));
      return;
    }
    case "condition": {
      if (!step.when?.trim()) throw new Error("Condition has no expression");
      const branch = condition(step.when, frame) ? "then" : "else";
      await runSteps(step[branch] ?? [], frame, `${path}.${branch}`);
      return;
    }
    case "runAction": {
      const child = ctx.config.actions.find((a) => a.id === step.actionId);
      if (!child) throw new Error(`Action ${step.actionId} does not exist`);
      if (frame.stack.includes(child.id))
        throw new Error(
          `Recursive action call: ${[...frame.stack, child.id]
            .map((id) => ctx.config.actions.find((a) => a.id === id)?.name ?? id)
            .join(" → ")}`,
        );
      if (frame.stack.length >= MAX_ACTION_DEPTH)
        throw new Error(`Actions nested deeper than ${MAX_ACTION_DEPTH}`);
      authorize(frame, "action", child.id, "execute");
      const outcome = await execute(child, frame, `${path}.action`);
      if (outcome.cancelled) throw new Cancelled();
      if (outcome.aborted) throw new Aborted(outcome.error ?? `Action ${child.name} failed`);
      if (!outcome.ok) throw new Error(outcome.error ?? `Action ${child.name} failed`);
      return;
    }
    case "fail": {
      const text = expr(step.message, frame, "Failure message");
      throw new Aborted(String(text ?? "") || "The action failed");
    }
    case "setField": {
      if (!ctx.beforeChange) throw new Error("Set field steps only run in before-change triggers");
      if (!step.field?.trim()) throw new Error("Field is not set");
      const value = expr(step.value, frame, `Value for ${step.field}`);
      ctx.beforeChange.set[step.field] = value;
      scope.record = { ...(scope.record as Record<string, unknown> | null), [step.field]: value };
      return;
    }
    default:
      throw new Error(`Unknown step kind ${(step as { kind: string }).kind}`);
  }
}

/** A before-change trigger only computes fields or rejects: it never writes or touches the UI. */
function refuseInBeforeChange(step: Step, config: DocumentConfig) {
  const actionQuery =
    step.kind === "runQuery" && config.savedQueries.some((q) => q.id === step.queryId && q.action);
  if (BEFORE_CHANGE_STEPS.includes(step.kind) && !actionQuery) return;
  const what = actionQuery ? "Run action query" : STEP_LABELS[step.kind];
  throw new Aborted(`A before-change trigger cannot run "${what}" steps; use an after trigger`);
}

function exists(config: DocumentConfig, kind: string, id: string) {
  if (!id) throw new Error(`No ${kind} selected`);
  const list: { id: string }[] | undefined =
    kind === "form"
      ? config.design?.forms
      : kind === "report"
        ? config.reports
        : kind === "dashboard"
          ? config.dashboards
          : undefined;
  if (list && !list.some((item) => item.id === id))
    throw new Error(`The ${kind} ${id} does not exist`);
}

async function findRows(
  table: string,
  match: MatchSpec,
  frame: Frame,
  trigger?: TriggerStepAuth,
): Promise<FoundRow[]> {
  let rows: FoundRow[];
  if (match === "current")
    rows = await currentRow(
      table,
      frame.scope.record as Record<string, unknown> | null,
      frame.ctx.snapshot,
      trigger,
    );
  else {
    const criteria = evalMap(match, frame);
    if (!Object.keys(criteria).length) throw new Error("Match has no columns");
    rows = await matchRows(table, criteria, trigger);
  }
  return rows.map((row) => {
    const known = frame.seen.get(rowKey(table, row.identity));
    if (known === null) throw new Error(`A ${table} row was already deleted by this action`);
    return known ? { ...row, expected: known } : row;
  });
}

/**
 * Records a row this run wrote. Lookups after an immediate write re-read the row
 * (seeing trigger changes too), so its entry is dropped; rollback-mode and
 * snapshot rows keep their post-write values as the next `expected`.
 */
function remember(frame: Frame, table: string, row: FoundRow, next: NamedValue[] | null) {
  const key = rowKey(table, row.identity);
  if (next === null || frame.tx || !row.fresh) frame.seen.set(key, next);
  else frame.seen.delete(key);
}

const write = (
  operation: RecordWrite["operation"],
  table: string,
  values: RecordWrite["values"],
  identity: DataValue[] | null,
  meta: RecordWriteMeta,
): RecordWrite => ({ operation, table, values, identity, meta });

type QueryModule = {
  runSavedQuery?: (id: string, params?: Record<string, unknown>) => Promise<QueryResult>;
};

/** Runs a saved query: the Queries API when present, else the raw read path. */
export async function runQuery(
  config: DocumentConfig,
  queryId: string,
  params: Record<string, unknown>,
): Promise<QueryResult> {
  const api = queryApi as unknown as QueryModule;
  if (typeof api.runSavedQuery === "function") return api.runSavedQuery(queryId, params);
  const query = config.savedQueries.find((q) => q.id === queryId);
  if (!query) throw new Error(`Query ${queryId} does not exist`);
  if (Object.keys(params).length)
    return call<QueryResult>("execute_parameterized_query", {
      sql: query.sql,
      params: toNamedValues(params),
    });
  return executeReadQuery(query.sql);
}
