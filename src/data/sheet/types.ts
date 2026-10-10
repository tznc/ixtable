/** Aggregates the datasheet totals row offers (src-tauri/src/data/totals.rs). */
export type TotalFunction = "sum" | "avg" | "count" | "min" | "max" | "stdev" | "var";
export type TotalSpec = { column: string; function: TotalFunction };
