import type { QueryResult } from "../lib/types";
import type { BuilderModel } from "./builder/model";

/** Parameter logical types accepted by `queries.rs` (`logical_type`). */
export const PARAMETER_TYPES = [
  "text",
  "integer",
  "number",
  "boolean",
  "date",
  "timestamp",
] as const;
export type ParameterType = (typeof PARAMETER_TYPES)[number];

export interface QueryParameter {
  // Placeholder name: `$name` in SQL.
  name: string;
  logicalType: ParameterType | string;
  defaultValue?: unknown;
  // A required parameter without a default must be supplied at run time.
  required?: boolean;
}

/**
 * What an action query changes (docs/decisions/action-queries.md). `insert`,
 * `update` and `delete` run `sql` as that DuckDB statement on `table`;
 * `replace` runs `sql` as a SELECT whose rows replace every row of `table`.
 */
export type ActionQueryKind = "insert" | "update" | "delete" | "replace";
export interface ActionSpec {
  kind: ActionQueryKind;
  table: string;
}
/** The table operations an action query performs: what the role needs (trigger_auth.rs). */
export const ACTION_QUERY_OPS: Record<ActionQueryKind, ("create" | "update" | "delete")[]> = {
  insert: ["create"],
  update: ["update"],
  delete: ["delete"],
  replace: ["delete", "create"],
};

export interface SavedQuery {
  id: string;
  name: string;
  // Read-only SQL with `$name` placeholders. Compiled from `builder` when present.
  sql: string;
  filterState?: unknown;
  parameters?: QueryParameter[];
  // Visual builder model; absent for SQL-authored queries.
  builder?: BuilderModel | null;
  // Set for an action query: `sql` changes rows instead of reading them.
  action?: ActionSpec | null;
}

/** Action queries change rows and return none, so they are never a data source. */
export const readQueries = <T extends Pick<SavedQuery, "action">>(queries: T[] = []) =>
  queries.filter((q) => !q.action);

/** `QueryResult` plus run metadata from `execute_parameterized_query` / `run_saved_query`. */
export interface QueryRunResult extends QueryResult {
  truncated: boolean;
  rowLimit: number;
  elapsedMs: number;
}
