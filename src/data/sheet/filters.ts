import type { DataValue, Filter } from "../../lib/types";
import { showValue } from "../format";

/**
 * Access's "Filter by selection" for one cell: rows whose `column` equals the
 * value (or, with `exclude`, differs from it). A null cell filters on nullness.
 * Blobs cannot be compared and give no filter.
 */
export function selectionFilter(column: string, value: DataValue, exclude = false): Filter | null {
  if (value.type === "blob") return null;
  if (value.type === "null") return { column, operator: exclude ? "is_not_null" : "is_null" };
  return { column, operator: exclude ? "ne" : "eq", value };
}

/** Adds `filter`, replacing an existing filter on the same column and operator. */
export const addFilter = (filters: Filter[], filter: Filter): Filter[] => [
  ...filters.filter((f) => !(f.column === filter.column && f.operator === filter.operator)),
  filter,
];

const OPERATOR_LABELS: Partial<Record<Filter["operator"], string>> = {
  eq: "=",
  ne: "≠",
  contains: "contains",
};

/** One-line description of a filter for its chip, e.g. `Region = North`. */
export function describeFilter(filter: Filter): string {
  if (filter.operator === "is_null") return `${filter.column} is empty`;
  if (filter.operator === "is_not_null") return `${filter.column} is not empty`;
  const value = filter.value ? showValue(filter.value) : "";
  return `${filter.column} ${OPERATOR_LABELS[filter.operator] ?? filter.operator} ${value}`;
}
