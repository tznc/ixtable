/** Declarative actions and record triggers (PRD §17). Mirrors src-tauri/src/automation.rs. */

/** An expression in the src/expr language, evaluated by the action runner. */
export type Expr = string;

export type NavigateTarget = {
  kind: "form" | "report" | "dashboard" | "table";
  id: string;
  mode?: string;
  /** Expression for the record to open (forms). */
  recordId?: Expr;
};

/** Column → expression map. */
export type ValueMap = Record<string, Expr>;
/** Column → expression map identifying rows, or "current" for the action's current record. */
export type MatchSpec = ValueMap | "current";

export type StepBody =
  | { kind: "createRecord"; table: string; values: ValueMap; storeAs?: string }
  | { kind: "updateRecord"; table: string; match: MatchSpec; values: ValueMap }
  | { kind: "deleteRecord"; table: string; match: MatchSpec }
  | { kind: "runQuery"; queryId: string; params: ValueMap; storeAs: string }
  | { kind: "navigate"; target: NavigateTarget }
  | { kind: "openForm"; formId: string; mode?: string; recordId?: Expr }
  | { kind: "openReport"; reportId: string; params?: ValueMap }
  /** `params` become the dashboard's initial filter values (by parameter name) and `params` scope. */
  | { kind: "openDashboard"; dashboardId: string; params?: ValueMap }
  | { kind: "setState"; scope: "app" | "form"; key: string; value: Expr }
  | { kind: "confirm"; message: Expr }
  | { kind: "message"; text: Expr; tone?: "info" | "error" }
  /** `when` picks the branch: true → then, false/null → else. */
  | { kind: "condition"; then: Step[]; else: Step[] }
  | { kind: "runAction"; actionId: string }
  /** Business-rule abort: ends the action with ok: false and this message (nothing commits under rollback). */
  | { kind: "fail"; message: Expr }
  /** Before-change triggers only: sets a field of the record about to be saved. */
  | { kind: "setField"; field: string; value: Expr };

export type StepKind = StepBody["kind"];

/** One step. `when` (optional) skips the step when it evaluates to false or null. */
export type Step = StepBody & { id: string; when?: Expr };

export type OnError = "stop" | "continue" | "rollback";

export interface ActionDef {
  id: string;
  name: string;
  description?: string;
  steps: Step[];
  /**
   * stop: abort on the first failing step (earlier writes stay).
   * continue: log the failure and run the remaining steps.
   * rollback: record writes run as one transaction; any failure undoes them all.
   */
  onError: OnError;
}

/**
 * created/updated/deleted run after the write commits. beforeChange runs before a
 * create or update is written (always sync): its action may set fields of the
 * record (setField) or reject the save (fail, or any failing step).
 */
export type TriggerEvent = "created" | "updated" | "deleted" | "beforeChange";

/** The step kinds a before-change trigger's action may use (mirrors automation.rs). */
export const BEFORE_CHANGE_STEPS: readonly StepKind[] = [
  "setField",
  "condition",
  "fail",
  "runQuery",
  "runAction",
];

export interface Trigger {
  id: string;
  name: string;
  table: string;
  event: TriggerEvent;
  /**
   * Optional expression over `record` (and `old`: the row before an update, the
   * deleted row, null on a before-change create); false/null skips the trigger.
   */
  condition?: Expr;
  actionId: string;
  /** sync: runs inside the initiating write workflow. async: enqueued on the durable local job queue. */
  mode: "sync" | "async";
  enabled: boolean;
  /** Expression producing the idempotency key. Default `${triggerId}:${table}:${pk}:${event}:${hash(values)}`. */
  idempotencyKey?: Expr;
  maxAttempts: number;
  backoffMs: number;
  /**
   * Execution identity. "app" (default when absent): steps run as the app, limited
   * to the writes the trigger's action declares. "user": steps run under the
   * signed-in user's role, and a save whose trigger the role could not run is
   * refused before it commits.
   */
  runAs?: "app" | "user";
}

export interface StepLog {
  stepIndex: number;
  /** Nested path for steps inside conditions or called actions, e.g. "2.then.0". */
  path?: string;
  kind: StepKind;
  ok: boolean;
  skipped?: boolean;
  error?: string;
  durationMs: number;
}

export type JobStatus = "queued" | "running" | "succeeded" | "failed" | "cancelled";

export interface Job {
  id: string;
  documentId: string;
  triggerId: string;
  actionId: string;
  payload: unknown;
  idempotencyKey: string;
  status: JobStatus;
  attempts: number;
  maxAttempts: number;
  backoffMs: number;
  nextRunAt: string;
  createdAt: string;
  updatedAt: string;
  lastError?: string | null;
  /** Token of the current lease while running; completeJob/failJob must pass it. */
  leaseToken?: string | null;
}

export interface JobAttempt {
  jobId: string;
  attempt: number;
  startedAt: string;
  finishedAt?: string | null;
  ok?: boolean | null;
  error?: string | null;
  log?: unknown;
}

export const STEP_KINDS: StepKind[] = [
  "createRecord",
  "updateRecord",
  "deleteRecord",
  "runQuery",
  "navigate",
  "openForm",
  "openReport",
  "openDashboard",
  "setState",
  "confirm",
  "message",
  "condition",
  "runAction",
  "fail",
  "setField",
];
