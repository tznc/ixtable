/** Turns saved-query results into KPI values and chart series. Pure functions. */
import { fromDataValue } from "../automation/values";
import { evaluate, formatValue } from "../expr";
import type { QueryResult } from "../lib/types";
import { MAX_SERIES, OTHER_LABEL } from "./charts/palette";
import type { DashboardComponent, DashboardFilter } from "./types";

export type Row = Record<string, unknown>;

export const resultRows = (result: QueryResult): Row[] =>
  result.rows.map((row) =>
    Object.fromEntries(result.columns.map((column, i) => [column, fromDataValue(row[i])])),
  );

/** A finite number from a query value, else null (text that looks numeric counts). */
export function toNumber(value: unknown): number | null {
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (typeof value === "boolean") return value ? 1 : 0;
  if (typeof value === "string" && value.trim() !== "") {
    const n = Number(value);
    return Number.isFinite(n) ? n : null;
  }
  return null;
}

export const categoryLabel = (value: unknown) =>
  value === null || value === undefined || value === "" ? "(blank)" : String(value);

/** Formats a value with an optional src/expr pattern; bad patterns fall back to plain text. */
export function formatNumber(value: unknown, pattern?: string | null): string {
  if (value === null || value === undefined) return "—";
  try {
    return formatValue(value, pattern || null);
  } catch {
    return String(value);
  }
}

export type Series = { name: string; values: (number | null)[] };
export type CategoryData = { categories: string[]; series: Series[] };
export type Point = { x: number; y: number; label: string };
export type ScatterData = { series: { name: string; points: Point[] }[] };

const add = (a: number | null, b: number | null) => (b === null ? a : (a ?? 0) + b);

/** Keeps the first `MAX_SERIES - 1` series and sums the rest into "Other". */
export function foldSeries(series: Series[]): Series[] {
  if (series.length <= MAX_SERIES) return series;
  const kept = series.slice(0, MAX_SERIES - 1);
  const other = series.slice(MAX_SERIES - 1).reduce<(number | null)[]>(
    (sum, s) => sum.map((value, i) => add(value, s.values[i])),
    series[0].values.map(() => null),
  );
  return [...kept, { name: OTHER_LABEL, values: other }];
}

/** The columns chart data reads; dashboards and report charts both supply them. */
export type SeriesSpec = Pick<DashboardComponent, "x" | "y" | "groupBy">;

/**
 * Category chart data. Without `groupBy`, each y column is a series and rows with the
 * same x are summed. With `groupBy`, the first y column is split into one series per
 * group value. Categories and series keep first-appearance order.
 */
export function chartData(component: SeriesSpec, rows: Row[]): CategoryData {
  const x = component.x ?? "";
  const ys = (component.y ?? []).filter(Boolean);
  const categories: string[] = [];
  const index = new Map<string, number>();
  const categoryOf = (row: Row) => {
    const label = categoryLabel(row[x]);
    if (!index.has(label)) {
      index.set(label, categories.length);
      categories.push(label);
    }
    return index.get(label) ?? 0;
  };
  if (component.groupBy && ys.length) {
    const groups = new Map<string, (number | null)[]>();
    const cells: Array<[number, string, number | null]> = rows.map((row) => [
      categoryOf(row),
      categoryLabel(row[component.groupBy ?? ""]),
      toNumber(row[ys[0]]),
    ]);
    for (const [, group] of cells) if (!groups.has(group)) groups.set(group, []);
    for (const values of groups.values()) values.push(...categories.map(() => null));
    for (const [category, group, value] of cells) {
      const values = groups.get(group) ?? [];
      values[category] = add(values[category], value);
    }
    return {
      categories,
      series: foldSeries([...groups].map(([name, values]) => ({ name, values }))),
    };
  }
  const positions = rows.map(categoryOf);
  const series = ys.map((name) => {
    const values: (number | null)[] = categories.map(() => null);
    rows.forEach((row, i) => {
      values[positions[i]] = add(values[positions[i]], toNumber(row[name]));
    });
    return { name, values };
  });
  return { categories, series: foldSeries(series) };
}

/** Scatter data: numeric x against each y column, or the first y split by `groupBy`. */
export function scatterData(component: SeriesSpec, rows: Row[]): ScatterData {
  const x = component.x ?? "";
  const ys = (component.y ?? []).filter(Boolean);
  const point = (row: Row, y: string): Point | null => {
    const px = toNumber(row[x]);
    const py = toNumber(row[y]);
    return px === null || py === null ? null : { x: px, y: py, label: categoryLabel(row[x]) };
  };
  if (component.groupBy && ys.length) {
    const groups = new Map<string, Point[]>();
    for (const row of rows) {
      const name = categoryLabel(row[component.groupBy]);
      const p = point(row, ys[0]);
      if (!groups.has(name)) groups.set(name, []);
      if (p) groups.get(name)?.push(p);
    }
    const series = [...groups].map(([name, points]) => ({ name, points }));
    if (series.length <= MAX_SERIES) return { series };
    const other = series.slice(MAX_SERIES - 1).flatMap((s) => s.points);
    return { series: [...series.slice(0, MAX_SERIES - 1), { name: OTHER_LABEL, points: other }] };
  }
  return {
    series: ys.slice(0, MAX_SERIES).map((name) => ({
      name,
      points: rows.map((row) => point(row, name)).filter((p): p is Point => p !== null),
    })),
  };
}

export type KpiValue = {
  value: unknown;
  text: string;
  comparison?: { value: unknown; delta: number | null; text: string; label: string };
};

/** Evaluates a field or expression against the rows (`rows`, `params`, `app` scope). */
function measure(
  field: string | null | undefined,
  expression: string | null | undefined,
  rows: Row[],
  scope: Record<string, unknown>,
): unknown {
  if (expression?.trim()) return evaluate(expression, { ...scope, rows });
  if (field) return rows[0]?.[field] ?? null;
  return null;
}

/** KPI value and optional comparison (value − comparison). Expression errors throw. */
export function kpiValue(
  component: DashboardComponent,
  rows: Row[],
  scope: Record<string, unknown> = {},
): KpiValue {
  const value = measure(component.valueField, component.expression, rows, scope);
  const result: KpiValue = { value, text: formatNumber(value, component.format) };
  const comparison = component.comparison;
  if (comparison && (comparison.valueField || comparison.expression?.trim())) {
    const other = measure(comparison.valueField, comparison.expression, rows, scope);
    const a = toNumber(value);
    const b = toNumber(other);
    const delta = a === null || b === null ? null : Number((a - b).toPrecision(15));
    const sign = delta !== null && delta > 0 ? "+" : "";
    result.comparison = {
      value: other,
      delta,
      label: comparison.label || "comparison",
      text: delta === null ? "—" : `${sign}${formatNumber(delta, component.format)}`,
    };
  }
  return result;
}

export type FilterValue = unknown;
export type DateRange = { from?: string | null; to?: string | null };

const blank = (value: unknown) =>
  value === null || value === undefined || (typeof value === "string" && value.trim() === "");

/** Converts one filter's raw control value to query parameters (blank → null → query default). */
export function filterParams(filter: DashboardFilter, value: FilterValue): Record<string, unknown> {
  if (!filter.param) return {};
  if (filter.control === "dateRange") {
    const range = (value ?? {}) as DateRange;
    return {
      [`${filter.param}From`]: blank(range.from) ? null : range.from,
      [`${filter.param}To`]: blank(range.to) ? null : range.to,
    };
  }
  if (blank(value)) return { [filter.param]: null };
  const numeric = ["integer", "number", "decimal", "real"].includes(filter.logicalType);
  if (numeric || filter.control === "number") {
    const n = toNumber(value);
    return {
      [filter.param]: n === null ? null : filter.logicalType === "integer" ? Math.trunc(n) : n,
    };
  }
  return { [filter.param]: value };
}

/** Initial control values: each filter's default. */
export const defaultFilterValues = (filters: DashboardFilter[]): Record<string, FilterValue> =>
  Object.fromEntries(
    filters.map((f) => [f.id, f.default ?? (f.control === "dateRange" ? {} : "")]),
  );

/** The parameters every component query receives, keyed by parameter name. */
export const dashboardParams = (
  filters: DashboardFilter[],
  values: Record<string, FilterValue>,
): Record<string, unknown> =>
  Object.assign({}, ...filters.map((f) => filterParams(f, values[f.id] ?? f.default)));
