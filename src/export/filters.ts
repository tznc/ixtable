import type { Filter } from "../lib/types";

/** The table browser's "Filter first column" box as a `contains` filter (none when empty). */
export const firstColumnFilter = (column: string | undefined, text: string): Filter[] =>
  text && column ? [{ column, operator: "contains", value: { type: "text", value: text } }] : [];
