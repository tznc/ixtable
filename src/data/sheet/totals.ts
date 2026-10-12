import type { DbColumn } from "../../lib/types";
import { logicalOf } from "../../schema/logical";
import type { TotalFunction } from "./types";

/** The totals a column offers, matching the checks in src-tauri/src/data/totals.rs. */
export function totalsFor(column: DbColumn): TotalFunction[] {
  const base = logicalOf(column).split("(")[0];
  if (["integer", "real", "decimal"].includes(base))
    return ["sum", "avg", "count", "min", "max", "stdev", "var"];
  if (["blob", "json", "boolean"].includes(base)) return ["count"];
  return ["count", "min", "max"];
}
