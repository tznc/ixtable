import { useEffect, useState } from "react";
import type { DesignForm } from "../design/schema";
import { exportSavedQuery, exportTable } from "../export/api";
import { ExportMenu } from "../export/ExportMenu";
import type { Filter, Sort } from "../lib/types";
import { toNamedValues } from "../query/api";
import { exportFilters, type SourceScope } from "./data";

/**
 * Exports a list form's whole source (table or saved query) with the list's current sort,
 * search and row filter. Disabled when the row filter cannot run in DuckDB, so an export
 * never holds rows the list hides. The export commands enforce the role's read permission.
 */
export function ListExport({
  form,
  sorts,
  filters,
  bound,
  scope,
}: {
  form: DesignForm;
  sorts: Sort[];
  filters: Filter[];
  bound: Record<string, unknown>;
  scope: SourceScope;
}) {
  const [rowFilters, setRowFilters] = useState<Filter[] | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    setRowFilters(undefined);
    exportFilters(form, scope).then(
      (found) => live && setRowFilters(found),
      () => live && setRowFilters(null),
    );
    return () => {
      live = false;
    };
  }, [form, scope]);
  const source = form.source;
  const table = source?.kind === "table" ? source.table : null;
  const queryId = source?.kind === "query" ? source.queryId : null;
  if (!table && !queryId) return null;
  const all = [...filters, ...(rowFilters ?? [])];
  return (
    <ExportMenu
      name={form.name}
      disabled={!rowFilters}
      disabledReason={
        rowFilters === null ? "This list's filter can't be applied to an export" : undefined
      }
      onExport={(format, path) =>
        queryId
          ? exportSavedQuery(
              queryId,
              { params: toNamedValues(bound), sorts, filters: all },
              format,
              path,
            )
          : exportTable(table ?? "", { sorts, filters: all }, format, path)
      }
    />
  );
}
