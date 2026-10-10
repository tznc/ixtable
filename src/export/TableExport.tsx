import type { Sort } from "../lib/types";
import { exportTable } from "./api";
import { ExportMenu } from "./ExportMenu";
import { firstColumnFilter } from "./filters";

/** Data mode: exports the whole table with the browser's sort and first-column filter. */
export function TableExport({
  table,
  sorts,
  filterText,
  firstColumn,
}: {
  table: string;
  sorts: Sort[];
  filterText: string;
  firstColumn?: string;
}) {
  return (
    <ExportMenu
      name={table}
      onExport={(format, path) =>
        exportTable(
          table,
          { sorts, filters: firstColumnFilter(firstColumn, filterText) },
          format,
          path,
        )
      }
    />
  );
}
