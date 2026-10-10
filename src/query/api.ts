import { call } from "../lib/api";
import type { DataValue, Filter, NamedValue, Sort } from "../lib/types";
import type { ActionSpec, QueryParameter, QueryRunResult } from "./types";

export interface RunOptions {
  // Maximum rows returned (default 10,000 in Rust). `truncated` reports whether more exist.
  limit?: number;
  // Identifies the run so `cancelQuery(runId)` can interrupt it.
  runId?: string;
}

const DATA_TYPES = new Set([
  "null",
  "integer",
  "real",
  "text",
  "blob",
  "boolean",
  "date",
  "timestamp",
]);

/** Converts a plain JS value (or a tagged `DataValue`) to the value sent to Rust. */
export function toDataValue(value: unknown): DataValue {
  if (value === null || value === undefined) return { type: "null" };
  if (typeof value === "boolean") return { type: "boolean", value };
  if (typeof value === "number")
    return Number.isInteger(value) ? { type: "integer", value } : { type: "real", value };
  if (typeof value === "bigint") return { type: "integer", value: Number(value) };
  if (value instanceof Date) return { type: "timestamp", value: value.toISOString() };
  if (typeof value === "object" && "type" in value && DATA_TYPES.has(String(value.type)))
    return value as DataValue;
  return { type: "text", value: String(value) };
}

export const toNamedValues = (params: Record<string, unknown> = {}): NamedValue[] =>
  Object.entries(params).map(([column, value]) => ({ column, value: toDataValue(value) }));

/** Runs a saved query by id. Missing parameters fall back to their defaults. */
export const runSavedQuery = (
  queryId: string,
  params?: Record<string, unknown>,
  options: RunOptions = {},
) =>
  call<QueryRunResult>("run_saved_query", {
    id: queryId,
    params: toNamedValues(params),
    limit: options.limit ?? null,
    runId: options.runId ?? null,
  });

/** One page of a saved query's rows and the exact filtered total (paged in DuckDB). */
export interface QueryPageResult {
  columns: string[];
  rows: DataValue[][];
  total: number;
  offset: number;
  limit: number;
}

/** Reads one page of a saved query: filter, sort, LIMIT/OFFSET and COUNT run in DuckDB. */
export const runSavedQueryPage = (
  queryId: string,
  params: Record<string, unknown>,
  page: { offset: number; limit: number; sorts?: Sort[]; filters?: Filter[] },
) =>
  call<QueryPageResult>("run_saved_query_page", {
    id: queryId,
    params: toNamedValues(params),
    offset: page.offset,
    limit: page.limit,
    sorts: page.sorts ?? [],
    filters: page.filters ?? [],
    runId: null,
  });

/**
 * Runs read-only SQL with `$name` placeholders bound to `params`. Pass `parameters`
 * to type, default, and require values the way a saved query declares them.
 */
export const runQuerySql = (
  sql: string,
  params?: Record<string, unknown>,
  options: RunOptions & { parameters?: QueryParameter[] } = {},
) =>
  call<QueryRunResult>("execute_parameterized_query", {
    sql,
    params: toNamedValues(params),
    parameters: options.parameters ?? null,
    limit: options.limit ?? null,
    runId: options.runId ?? null,
  });

/** Interrupts one run (`runId`) or every running query in this window. Returns the count. */
export const cancelQuery = (runId?: string) =>
  call<number>("cancel_query", { runId: runId ?? null });

/** Checks read-only SQL without running it (DuckDB prepare). Resolves to the `$name` placeholders. */
export const checkQuerySql = (sql: string) => call<string[]>("check_query_sql", { sql });

/** Checks an action query's SQL against the action-query guard before it is saved. */
export const checkActionQuerySql = (sql: string, action: ActionSpec) =>
  call<void>("check_action_query_sql", { sql, action });
