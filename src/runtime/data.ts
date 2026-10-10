import { runQuery } from "../automation/runner";
import { runSavedQueryPage } from "../query/api";
import type { DesignForm, Relationship } from "../design/schema";
import { evaluate, evaluateBoolean, parse } from "../expr";
import { inspectTable, listDatabaseObjects, readTablePage } from "../lib/api";
import { registerRecordHook } from "../lib/records";
import type { DataValue, DocumentConfig, Filter, Sort, TableSchema } from "../lib/types";
import {
  pagedScan,
  type RowPredicate,
  rowFilter,
  SCAN_CHUNK,
  type ScanResult,
  scanFiltered,
} from "./conditions";
import { pushdown } from "./pushdown";
import { fromDataValue, type RecordValues, rowObject } from "./values";

/**
 * One page of records for a list. `identities` is null for read-only (query) sources.
 * `truncated` is true when a row filter stopped at `SCAN_LIMIT` rows: matches past that
 * point are missing and `total` counts only the rows scanned.
 */
export type RecordPage = {
  columns: string[];
  rows: RecordValues[];
  identities: DataValue[][] | null;
  total: number;
  truncated?: boolean;
};
export type PageRequest = { offset: number; limit: number; sorts: Sort[]; filters: Filter[] };

const schemas = new Map<string, Promise<TableSchema>>();
const pages = new Map<string, RecordPage>();
type Matched = { record: RecordValues; identity: DataValue[] | null };
// One filtered scan per (source, bound params, sorts, search, filter, inputs); pages slice it.
type Scanned = ScanResult<Matched> & { columns: string[]; keyed: boolean };
const scans = new Map<string, Promise<Scanned>>();
const snapshots = new Map<string, { record: RecordValues; identity: DataValue[] }>();

/** Drops cached schemas and pages; runs after every record write and schema change. */
export function invalidateRuntimeCache(schemaToo = false) {
  pages.clear();
  scans.clear();
  if (schemaToo) schemas.clear();
}
registerRecordHook({ after: () => invalidateRuntimeCache() });
if (typeof window !== "undefined")
  window.addEventListener("ixtable:database-changed", () => invalidateRuntimeCache(true));

export function tableSchema(table: string): Promise<TableSchema> {
  let hit = schemas.get(table);
  if (!hit) {
    hit = inspectTable(table);
    hit.catch(() => schemas.delete(table));
    schemas.set(table, hit);
  }
  return hit;
}

/**
 * A table with what generated forms need around it: the tables its foreign keys point at
 * (lookup display columns) and the tables pointing at it (one-level related lists).
 */
export async function tableContext(table: string) {
  const schema = await tableSchema(table);
  const objects = await listDatabaseObjects();
  const known = (
    await Promise.all(
      objects
        .filter((object) => object.objectType === "table")
        .map((object) => tableSchema(object.name).catch(() => null)),
    )
  ).filter((other): other is TableSchema => !!other);
  return {
    schema,
    targets: Object.fromEntries(known.map((other) => [other.name, other])),
    children: known.filter((other) => other.foreignKeys.some((fk) => fk.targetTable === table)),
  };
}

const pageKey = (
  form: DesignForm,
  request: PageRequest,
  scope: Record<string, unknown>,
  bound: Record<string, unknown>,
) =>
  JSON.stringify([form.source, bound, form.filter?.trim() ? [form.filter, scope] : null, request]);

/** A cached page for instant re-navigation (null when not loaded yet). */
export const cachedPage = (
  form: DesignForm,
  request: PageRequest,
  scope: Record<string, unknown> = {},
  bound: Record<string, unknown> = {},
) => pages.get(pageKey(form, request, scope, bound)) ?? null;

/** Query sources page in DuckDB (Rust wraps the saved SQL), so totals are exact. */
async function queryPage(
  queryId: string,
  request: PageRequest,
  bound: Record<string, unknown>,
): Promise<RecordPage> {
  const result = await runSavedQueryPage(queryId, bound, request);
  const columns = result.columns.map((name) => ({ name }));
  return {
    columns: result.columns,
    rows: result.rows.map((row) => rowObject(columns, row)),
    identities: null,
    total: result.total,
  };
}

async function tablePage(table: string, request: PageRequest): Promise<RecordPage> {
  const result = await readTablePage(table, request);
  return {
    columns: result.columns.map((c) => c.name),
    rows: result.rows.map((row) => rowObject(result.columns, row)),
    identities: result.identities,
    total: result.total,
  };
}

/**
 * A page with a row filter. The source is scanned once (up to `SCAN_LIMIT` rows) per
 * bound params, sort, search, filter and filter inputs; every page is then a slice of the
 * cached matches, so paging never rescans and `total` is the real match count.
 */
async function filteredPage(
  read: (request: PageRequest) => Promise<RecordPage>,
  request: PageRequest,
  keep: RowPredicate,
  key: string,
): Promise<RecordPage> {
  let scan = scans.get(key);
  if (!scan) {
    let columns: string[] = [];
    let keyed = false;
    scan = pagedScan(
      async (offset, limit) => {
        const chunk = await read({ ...request, offset, limit });
        columns = chunk.columns;
        keyed = chunk.identities !== null;
        return chunk.rows.map((record, i) => ({
          record,
          identity: chunk.identities?.[i] ?? null,
        }));
      },
      (row) => keep(row.record),
    ).then((found) => ({ ...found, columns, keyed }));
    scan.catch(() => scans.delete(key));
    scans.set(key, scan);
  }
  const found = await scan;
  const shown = found.matches.slice(request.offset, request.offset + request.limit);
  return {
    columns: found.columns,
    rows: shown.map((row) => row.record),
    identities: found.keyed ? shown.map((row) => row.identity ?? []) : null,
    total: found.matches.length,
    truncated: found.truncated,
  };
}

/** The scope a form's row filter sees; each row is added as `record` (see `rowFilter`). */
const filterScope = (scope: Record<string, unknown>) => ({
  form: {},
  params: {},
  parent: null,
  ...scope,
});

/**
 * A table source's reader with the pushable part of the row filter applied in DuckDB
 * (`pushdown`), and the predicate for the rest (null when DuckDB applies all of it).
 * Null when nothing can be pushed; the whole filter then runs per row as before.
 */
async function pushedReader(
  form: DesignForm,
  read: (request: PageRequest) => Promise<RecordPage>,
  scope: Record<string, unknown>,
) {
  const table = form.source?.kind === "table" ? form.source.table : null;
  if (!table || !form.filter?.trim()) return null;
  const split = pushdown(parse(form.filter), scope, (await tableSchema(table)).columns);
  if (!split.filters.length) return null;
  const rest = split.rest;
  const keep: RowPredicate | null = rest
    ? (record) => {
        try {
          return evaluateBoolean(rest, { ...scope, record });
        } catch {
          return false;
        }
      }
    : null;
  return {
    read: (request: PageRequest) =>
      read({ ...request, filters: [...request.filters, ...split.filters] }),
    keep,
  };
}

/** Reads pages of a form's source (table page or saved query page), or null without one. */
function sourceReader(form: DesignForm, bound: Record<string, unknown>) {
  const source = form.source;
  const queryId = source?.kind === "query" ? source.queryId : null;
  const table = source?.kind === "table" ? source.table : null;
  if (queryId) return (request: PageRequest) => queryPage(queryId, request, bound);
  if (table) return (request: PageRequest) => tablePage(table, request);
  return null;
}

/**
 * Reads one page of a form's source through DuckDB (table page or saved query). `bound`
 * are the query source's bound parameter values (see `sourceParams`). A form `filter`
 * expression is applied to the fetched rows with `scope` (`app`, `params`).
 */
export async function loadPage(
  form: DesignForm,
  request: PageRequest,
  scope: Record<string, unknown> = {},
  bound: Record<string, unknown> = {},
): Promise<RecordPage> {
  let keep = rowFilter(form.filter, filterScope(scope));
  let read = sourceReader(form, bound);
  const pushed = read && keep ? await pushedReader(form, read, filterScope(scope)) : null;
  if (pushed) ({ read, keep } = pushed);
  let page: RecordPage;
  if (!read) page = { columns: [], rows: [], identities: null, total: 0 };
  else if (keep) {
    const scanKey = pageKey(form, { ...request, offset: 0, limit: 0 }, scope, bound);
    page = await filteredPage(read, request, keep, scanKey);
  } else page = await read(request);
  pages.set(pageKey(form, request, scope, bound), page);
  return page;
}

/**
 * Whether `record` passes the form's row filter, evaluated with `scope` as in `loadPage`
 * (true without a filter). A filter that does not parse throws.
 */
export function passesFilter(
  form: DesignForm,
  record: RecordValues,
  scope: Record<string, unknown> = {},
): boolean {
  const keep = rowFilter(form.filter, filterScope(scope));
  return !keep || keep(record);
}

/** Expression scope for a query source's parameter bindings. */
export type SourceScope = { app: Record<string, unknown>; params: Record<string, unknown> };

/**
 * Evaluates a query source's parameter bindings (`source.params`: name to
 * expression over `app` and `params`). Throws with the parameter name when one fails.
 */
export function sourceParams(form: DesignForm, scope: SourceScope): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  if (form.source?.kind !== "query") return out;
  for (const [name, src] of Object.entries(form.source.params ?? {})) {
    if (!src?.trim()) continue;
    try {
      out[name] = evaluate(src, scope);
    } catch (error) {
      throw new Error(
        `Parameter $${name}: ${error instanceof Error ? error.message : String(error)}`,
        { cause: error },
      );
    }
  }
  return out;
}

/**
 * The record id of the first row of a form's source that passes its row filter (the row
 * itself for query sources), or null when there is none. `bound` and `scope` are as for
 * `loadPage`; the scan stops at the first match. Used to open detail/edit views that have
 * no record of their own (design preview, dashboard-embedded forms).
 */
export async function firstRecordId(
  form: DesignForm,
  bound: Record<string, unknown> = {},
  scope: Record<string, unknown> = {},
): Promise<unknown> {
  let read = sourceReader(form, bound);
  if (!read) return null;
  let keep = rowFilter(form.filter, filterScope(scope));
  const pushed = keep ? await pushedReader(form, read, filterScope(scope)) : null;
  if (pushed) ({ read, keep } = pushed);
  const scan = await scanFiltered(
    async (offset, limit) => {
      const chunk = await read({ offset, limit, sorts: [], filters: [] });
      return chunk.rows.map((record, i) => ({ record, identity: chunk.identities?.[i] ?? null }));
    },
    (row) => !keep || keep(row.record),
    1,
    keep ? SCAN_CHUNK : 1,
  );
  const first = scan.matches[0];
  if (!first) return null;
  if (form.source?.kind !== "table" || !form.source.table || !first.identity) return first.record;
  return recordIdFor(await tableSchema(form.source.table), first.record, first.identity);
}

/** Primary key columns in key order. */
export const keyColumns = (schema: TableSchema) =>
  schema.columns
    .filter((c) => c.primaryKeyPosition > 0)
    .sort((a, b) => a.primaryKeyPosition - b.primaryKeyPosition)
    .map((c) => c.name);

/** The id passed to FormRenderer for a row: the key value, a key object, or a rowid snapshot key. */
export function recordIdFor(
  schema: TableSchema,
  record: RecordValues,
  identity: DataValue[],
): unknown {
  const keys = keyColumns(schema);
  if (keys.length === 1) return record[keys[0]];
  if (keys.length > 1) return Object.fromEntries(keys.map((k) => [k, record[k]]));
  const id = `rowid:${schema.name}:${String(fromDataValue(identity[0]))}`;
  snapshots.set(id, { record, identity });
  return id;
}

export type LoadedRecord = { record: RecordValues; identity: DataValue[] };

/** Loads one record by id through DuckDB. */
export async function loadRecord(table: string, recordId: unknown): Promise<LoadedRecord | null> {
  if (typeof recordId === "string" && snapshots.has(recordId))
    return snapshots.get(recordId) ?? null;
  const schema = await tableSchema(table);
  const keys = keyColumns(schema);
  const keyValues: RecordValues =
    recordId && typeof recordId === "object" ? (recordId as RecordValues) : { [keys[0]]: recordId };
  const columns = Object.keys(keyValues).filter((c) => schema.columns.some((x) => x.name === c));
  if (!columns.length) return null;
  const filters: Filter[] = columns.map((column) => {
    const declared = schema.columns.find((c) => c.name === column)?.declaredType ?? "";
    const raw = keyValues[column];
    const numeric =
      /INT|REAL|NUM|DEC|FLOA|DOUB/i.test(declared) && raw !== "" && Number.isFinite(Number(raw));
    const value: DataValue =
      raw == null
        ? { type: "null" }
        : numeric
          ? { type: Number.isInteger(Number(raw)) ? "integer" : "real", value: Number(raw) }
          : { type: "text", value: String(raw) };
    return { column, operator: "eq", value };
  });
  const page = await readTablePage(table, { limit: 1, filters });
  if (!page.rows.length) return null;
  return { record: rowObject(page.columns, page.rows[0]), identity: page.identities[0] };
}

/** A choice; relationship choices also carry the target record (multi-column keys). */
export type Choice = { value: unknown; label: string; record?: RecordValues };

/**
 * A relationship key: the stored value of a single-column key, or target column → value
 * for a multi-column key. Empty when any part is missing.
 */
export type RelationshipKey = unknown;
const emptyKey = (value: RelationshipKey) =>
  value == null ||
  value === "" ||
  (typeof value === "object" &&
    Object.values(value as RecordValues).some((part) => part == null || part === ""));
const keyText = (value: RelationshipKey) =>
  typeof value === "object" ? JSON.stringify(value) : String(value);

/**
 * Lookup choices for a relationship selector, searched on the display column (DuckDB).
 * A relationship `filter` keeps only choice rows it accepts (`record` is the choice row;
 * `scope` supplies `parent`, `form` and `app`); rows are scanned until `limit` match.
 */
export async function relationshipChoices(
  relationship: Relationship,
  search = "",
  limit = 50,
  scope: Record<string, unknown> = {},
): Promise<Choice[]> {
  const { table, valueColumn, displayColumn } = relationship;
  const filters: Filter[] = search.trim()
    ? [
        {
          column: displayColumn,
          operator: "contains",
          value: { type: "text", value: search.trim() },
        },
      ]
    : [];
  const sorts: Sort[] = [{ column: displayColumn, descending: false }];
  const keep = rowFilter(relationship.filter, { form: {}, app: {}, params: {}, ...scope });
  const toChoice = (record: RecordValues): Choice => ({
    value: record[valueColumn],
    label: String(record[displayColumn] ?? record[valueColumn] ?? ""),
    record,
  });
  if (!keep) {
    const page = await readTablePage(table, { limit, filters, sorts });
    return page.rows.map((row) => toChoice(rowObject(page.columns, row)));
  }
  const scan = await scanFiltered(
    async (offset, size) => {
      const page = await readTablePage(table, { offset, limit: size, filters, sorts });
      return page.rows.map((row) => rowObject(page.columns, row));
    },
    keep,
    limit,
  );
  return scan.matches.slice(0, limit).map(toChoice);
}

/** Display label for one stored relationship key. */
export async function relationshipLabel(
  relationship: Relationship,
  value: RelationshipKey,
): Promise<string> {
  if (emptyKey(value)) return "";
  const keys =
    typeof value === "object" ? (value as RecordValues) : { [relationship.valueColumn]: value };
  const hit = await loadRecord(relationship.table, keys).catch(() => null);
  const shown = hit?.record[relationship.displayColumn];
  const label = shown != null ? String(shown) : typeof value === "object" ? "" : String(value);
  shownLabels.set(labelKey(relationship, value), label);
  return label;
}

/** Last label shown per relationship key, so a reloaded field shows it while it revalidates. */
const shownLabels = new Map<string, string>();
const labelKey = (relationship: Relationship, value: RelationshipKey) =>
  JSON.stringify([
    relationship.table,
    relationship.valueColumn,
    relationship.displayColumn,
    keyText(value),
  ]);

/** The label `relationshipLabel` last returned for this key, if any (stale-while-revalidate). */
export function cachedRelationshipLabel(
  relationship: Relationship,
  value: RelationshipKey,
): string | undefined {
  if (emptyKey(value)) return "";
  return shownLabels.get(labelKey(relationship, value));
}

/** Choices from a saved query: first column is the value, second (optional) the label. */
export async function queryChoices(config: DocumentConfig, queryId: string): Promise<Choice[]> {
  const result = await runQuery(config, queryId, {});
  return result.rows.map((row) => {
    const value = fromDataValue(row[0]);
    return { value, label: String(fromDataValue(row[1] ?? row[0]) ?? "") };
  });
}

/**
 * The DuckDB filters that reproduce a form's row filter, for exports that read the source
 * in Rust: [] without a filter, null when part of it runs only in TypeScript (the export
 * would then include rows the list hides).
 */
export async function exportFilters(
  form: DesignForm,
  scope: SourceScope,
): Promise<Filter[] | null> {
  if (!form.filter?.trim()) return [];
  const table = form.source?.kind === "table" ? form.source.table : null;
  if (!table) return null;
  try {
    const split = pushdown(
      parse(form.filter),
      filterScope(scope),
      (await tableSchema(table)).columns,
    );
    return split.rest || !split.filters.length ? null : split.filters;
  } catch {
    return null;
  }
}
