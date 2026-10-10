import { useId } from "react";
import type { SavedQuery } from "../../query/types";
import { type ChartComponent, REPORT_CHART_TYPES, type ReportChartType } from "../types";

const TYPE_LABELS: Record<ReportChartType, string> = {
  bar: "Bar",
  line: "Line",
  area: "Area",
  pie: "Pie",
  donut: "Donut",
  scatter: "Scatter",
};

type Patch = Partial<
  Pick<
    ChartComponent,
    "chartType" | "queryId" | "xField" | "yFields" | "groupBy" | "stacked" | "format" | "title"
  >
>;

/** Data and display settings of a report chart. Column inputs suggest the dataset's columns. */
export function ChartProperties({
  chart: c,
  columns,
  queries,
  onChange,
}: {
  chart: ChartComponent;
  columns: string[];
  queries: SavedQuery[];
  onChange: (patch: Patch) => void;
}) {
  const listId = useId();
  const pie = c.chartType === "pie" || c.chartType === "donut";
  return (
    <fieldset>
      <legend>Chart</legend>
      <datalist id={listId}>
        {columns.map((col) => (
          <option key={col} value={col} />
        ))}
      </datalist>
      <label>
        Chart type
        <select
          value={c.chartType}
          onChange={(e) => onChange({ chartType: e.target.value as ReportChartType })}
        >
          {REPORT_CHART_TYPES.map((t) => (
            <option key={t} value={t}>
              {TYPE_LABELS[t]}
            </option>
          ))}
        </select>
      </label>
      <label>
        Chart title
        <input
          value={c.title ?? ""}
          onChange={(e) => onChange({ title: e.target.value || undefined })}
        />
      </label>
      <label>
        Chart rows from
        <select value={c.queryId ?? ""} onChange={(e) => onChange({ queryId: e.target.value })}>
          <option value="">This band&apos;s rows</option>
          {queries.map((q) => (
            <option key={q.id} value={q.id}>
              Query: {q.name}
            </option>
          ))}
        </select>
      </label>
      <label>
        {c.chartType === "scatter" ? "X column" : "Category column"}
        <input
          list={listId}
          value={c.xField}
          onChange={(e) => onChange({ xField: e.target.value })}
        />
      </label>
      <label>
        {pie ? "Value column" : "Value columns"}
        <input
          list={listId}
          value={c.yFields.join(", ")}
          placeholder={pie ? "amount" : "amount, cost"}
          onChange={(e) =>
            onChange({
              yFields: e.target.value
                .split(",")
                .map((s) => s.trim())
                .filter((s, i, all) => s !== "" || i < all.length - 1),
            })
          }
        />
      </label>
      {!pie && (
        <label>
          Series by column
          <input
            list={listId}
            value={c.groupBy ?? ""}
            onChange={(e) => onChange({ groupBy: e.target.value || undefined })}
          />
        </label>
      )}
      {(c.chartType === "bar" || c.chartType === "area") && (
        <label className="inline">
          <input
            type="checkbox"
            checked={!!c.stacked}
            onChange={(e) => onChange({ stacked: e.target.checked || undefined })}
          />
          Stacked
        </label>
      )}
      <label>
        Value format
        <input
          value={c.format ?? ""}
          placeholder="#,##0"
          onChange={(e) => onChange({ format: e.target.value || undefined })}
        />
      </label>
    </fieldset>
  );
}
