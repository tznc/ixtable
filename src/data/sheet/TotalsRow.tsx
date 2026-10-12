import { useEffect, useState } from "react";
import { asTauriError } from "../../lib/api";
import type { DataValue, DbColumn, Filter } from "../../lib/types";
import { readTableTotals } from "../api";
import { showValue } from "../format";
import { totalsFor } from "./totals";
import type { TotalFunction } from "./types";

const LABELS: Record<TotalFunction, string> = {
  sum: "Sum",
  avg: "Average",
  count: "Count",
  min: "Minimum",
  max: "Maximum",
  stdev: "Std dev",
  var: "Variance",
};

const showTotal = (value: DataValue) =>
  value.type === "real"
    ? Number(value.value).toLocaleString(undefined, { maximumFractionDigits: 4 })
    : showValue(value);

/**
 * Access's Total row: a per-column aggregate picker over every row the current
 * filters select (not just this page), computed in DuckDB.
 */
export function TotalsRow({
  table,
  columns,
  visible,
  totals,
  filters,
  revision,
  cellProps,
  trailingCell,
  onChange,
}: {
  table: string;
  columns: DbColumn[];
  visible: number[];
  totals: Record<string, TotalFunction>;
  filters: Filter[];
  revision: number;
  /** Frozen-column class and offset for the cell of column index `j`. */
  cellProps: (j: number) => { className?: string; style?: React.CSSProperties };
  trailingCell: boolean;
  onChange: (column: string, total: TotalFunction | null) => void;
}) {
  const [values, setValues] = useState<Record<string, DataValue>>({});
  const [error, setError] = useState("");
  const specs = Object.entries(totals)
    .filter(([column]) => columns.some((c) => c.name === column))
    .map(([column, fn]) => ({ column, function: fn }));
  const key = JSON.stringify([table, specs, filters, revision]);
  useEffect(() => {
    let live = true;
    readTableTotals(table, filters, specs)
      .then((result) => {
        if (!live) return;
        setError("");
        setValues(Object.fromEntries(specs.map((s, i) => [s.column, result[i]])));
      })
      .catch((e) => live && setError(asTauriError(e).message));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return (
    <tfoot>
      <tr className="totals-row">
        <th className="rownum" scope="row" title={error || undefined}>
          Total
        </th>
        {visible.map((j) => {
          const column = columns[j];
          const chosen = totals[column.name];
          const value = values[column.name];
          return (
            <td key={column.name} {...cellProps(j)}>
              <select
                aria-label={`Total for ${column.name}`}
                value={chosen ?? ""}
                onChange={(e) =>
                  onChange(column.name, (e.target.value || null) as TotalFunction | null)
                }
              >
                <option value="">None</option>
                {totalsFor(column).map((fn) => (
                  <option key={fn} value={fn}>
                    {LABELS[fn]}
                  </option>
                ))}
              </select>
              {chosen && value && <output>{showTotal(value)}</output>}
            </td>
          );
        })}
        {trailingCell && <td />}
      </tr>
      {error && (
        <tr>
          <td colSpan={visible.length + 2} role="alert" className="error">
            {error}
          </td>
        </tr>
      )}
    </tfoot>
  );
}
