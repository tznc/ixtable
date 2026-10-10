import { fieldTextRows } from "../fields/values";
import { asTauriError, readTablePage } from "../lib/api";
import type { DataValue, DocumentConfig, QueryResult } from "../lib/types";
import { cancelQuery, runSavedQuery, runQuerySql } from "../query/api";
import type { QueryRunResult, SavedQuery } from "../query/types";
import { readReportAssets } from "./api";
import { newId } from "../lib/utils";
import { bandEntries } from "./model";
import type { Row } from "./engine";
import type { Report } from "./types";

/** Converts a backend cell to the plain JS value expressions see. */
export function cellValue(cell: DataValue | undefined): unknown {
  if (!cell || cell.type === "null" || cell.value === undefined) return null;
  if (cell.type === "integer" || cell.type === "real") {
    const n = Number(cell.value);
    return Number.isNaN(n) ? cell.value : n;
  }
  if (cell.type === "boolean") return Boolean(cell.value);
  return String(cell.value);
}

export const resultRows = (result: QueryResult): Row[] =>
  result.rows.map((row) =>
    Object.fromEntries(result.columns.map((column, i) => [column, cellValue(row[i])])),
  );

const quoteIdent = (name: string) => `"${name.replace(/"/g, '""')}"`;

/** Rows a report reads per query (Rust caps a run at 100,000). */
export const REPORT_ROW_LIMIT = 100_000;

export class ReportCancelled extends Error {
  constructor() {
    super("Report loading cancelled");
    this.name = "ReportCancelled";
  }
}

/** Only the parameters a saved query declares (all of them when it declares none). */
function queryParams(query: SavedQuery, params: Record<string, unknown>) {
  const declared = query.parameters ?? [];
  if (!declared.length) return params;
  return Object.fromEntries(
    declared.filter((p) => p.name in params).map((p) => [p.name, params[p.name]]),
  );
}

/**
 * Rows of the report's dataset: its saved query (parameters bound by Rust,
 * never interpolated), else its table, else none.
 */
export async function loadDataset(
  report: Report,
  config: DocumentConfig,
  params: Record<string, unknown>,
  runId?: string,
  limit = REPORT_ROW_LIMIT,
): Promise<QueryRunResult | null> {
  const options = { limit, runId };
  if (report.datasetQueryId) {
    const query = config.savedQueries.find((q) => q.id === report.datasetQueryId);
    if (!query) throw new Error("The report's saved query no longer exists");
    return runSavedQuery(query.id, queryParams(query, params), options);
  }
  if (report.table) return tableDataset(report.table, options);
  return null;
}

/**
 * All rows of a table, up to `limit`. A runtime role may not run ad hoc SQL
 * (FORBIDDEN), so it reads the table page by page, which Rust authorizes.
 */
async function tableDataset(
  table: string,
  options: { limit: number; runId?: string },
): Promise<QueryRunResult> {
  try {
    return await runQuerySql(`SELECT * FROM ${quoteIdent(table)}`, {}, options);
  } catch (error) {
    if (asTauriError(error).code !== "FORBIDDEN") throw error;
  }
  const started = Date.now();
  const first = await readTablePage(table, { limit: Math.min(1000, options.limit) });
  const total = first.total;
  const rows: DataValue[][] = [...first.rows];
  while (rows.length < Math.min(total, options.limit)) {
    const page = await readTablePage(table, {
      offset: rows.length,
      limit: Math.min(1000, options.limit - rows.length),
    });
    if (!page.rows.length) break;
    rows.push(...page.rows);
  }
  return {
    columns: first.columns.map((c) => c.name),
    rows,
    truncated: total > rows.length,
    rowLimit: options.limit,
    elapsedMs: Date.now() - started,
  };
}

export interface ReportData {
  rows: Row[];
  tables: Record<string, Row[]>;
  assets: Record<string, { mediaType: string; dataBase64: string }>;
  /** True when a query returned only the first `REPORT_ROW_LIMIT` rows. */
  truncated: boolean;
}

/**
 * Loads everything a report needs: dataset rows, rows of table components'
 * saved queries, and image assets. Aborting `signal` interrupts the running
 * query in DuckDB (`cancelQuery`) and rejects with `ReportCancelled`.
 */
export async function loadReportData(
  report: Report,
  config: DocumentConfig,
  params: Record<string, unknown>,
  signal?: AbortSignal,
): Promise<ReportData> {
  const runId = newId();
  const onAbort = () => {
    cancelQuery(runId).catch(() => undefined);
  };
  signal?.addEventListener("abort", onAbort);
  const step = async <T>(work: () => Promise<T>): Promise<T> => {
    if (signal?.aborted) throw new ReportCancelled();
    const value = await work();
    if (signal?.aborted) throw new ReportCancelled();
    return value;
  };
  try {
    let truncated = false;
    const rowsOf = (result: QueryRunResult) => {
      truncated ||= result.truncated;
      return resultRows(result);
    };
    const dataset = await step(() => loadDataset(report, config, params, runId));
    const components = bandEntries(report).flatMap((entry) => entry.band.components);
    const tables: Record<string, Row[]> = {};
    for (const c of components) {
      if (c.kind !== "table" || !c.queryId || tables[c.queryId]) continue;
      const query = config.savedQueries.find((q) => q.id === c.queryId);
      if (!query) continue;
      const options = { limit: REPORT_ROW_LIMIT, runId };
      tables[c.queryId] = rowsOf(
        await step(() => runSavedQuery(query.id, queryParams(query, params), options)),
      );
    }
    const ids = [
      ...new Set(components.flatMap((c) => (c.kind === "image" && c.assetId ? [c.assetId] : []))),
    ];
    const assets: ReportData["assets"] = {};
    if (ids.length)
      for (const a of await step(() => readReportAssets(ids)))
        assets[a.id] = { mediaType: a.mediaType, dataBase64: a.dataBase64 };
    const table = report.datasetQueryId ? null : report.table;
    const rows = dataset ? fieldTextRows(rowsOf(dataset), config, table) : [];
    return { rows, tables, assets, truncated };
  } finally {
    signal?.removeEventListener("abort", onAbort);
  }
}

export function base64Bytes(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}
