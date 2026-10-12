import type { Filter, Sort } from "../lib/types";
import { exportTable } from "./api";
import { ExportMenu } from "./ExportMenu";

/** Data mode: exports the whole table with the datasheet's sorts and filters. */
export function TableExport({
  table,
  sorts,
  filters,
}: {
  table: string;
  sorts: Sort[];
  filters: Filter[];
}) {
  return (
    <ExportMenu
      name={table}
      onExport={(format, path) => exportTable(table, { sorts, filters }, format, path)}
    />
  );
}
