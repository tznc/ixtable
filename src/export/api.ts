import { call } from "../lib/api";
import type { Filter, NamedValue, Sort } from "../lib/types";
import type { QueryParameter } from "../query/types";
import type { ExportFormat, ExportSummary } from "./types";

/** Exports every row of a table matching `filters`, in `sorts` order. */
export const exportTable = (
  table: string,
  options: { sorts?: Sort[]; filters?: Filter[] },
  format: ExportFormat,
  path: string,
) =>
  call<ExportSummary>("export_table", {
    table,
    sorts: options.sorts ?? [],
    filters: options.filters ?? [],
    format,
    path,
  });

/** Exports a saved query's rows for the given parameter values. */
export const exportSavedQuery = (
  id: string,
  options: { params?: NamedValue[]; sorts?: Sort[]; filters?: Filter[] },
  format: ExportFormat,
  path: string,
) =>
  call<ExportSummary>("export_saved_query", {
    id,
    params: options.params ?? [],
    sorts: options.sorts ?? [],
    filters: options.filters ?? [],
    format,
    path,
  });

/** Exports ad hoc read-only SQL (Query mode; developer only). */
export const exportSqlQuery = (
  sql: string,
  options: { params?: NamedValue[]; parameters?: QueryParameter[] | null },
  format: ExportFormat,
  path: string,
) =>
  call<ExportSummary>("export_sql_query", {
    sql,
    params: options.params ?? [],
    parameters: options.parameters ?? null,
    format,
    path,
  });
