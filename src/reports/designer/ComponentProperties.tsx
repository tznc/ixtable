import { Plus, Trash2 } from "lucide-react";
import { newId } from "../../lib/utils";
import type { SavedQuery } from "../../query/types";
import type { AssetSummary } from "../api";
import { fieldExpression, KIND_LABELS } from "../model";
import type { ComponentStyle, ReportComponent, RunningSum, TableColumn, TextAlign } from "../types";
import { ChartProperties } from "./ChartProperties";
import { ConditionsEditor } from "./ConditionsEditor";
import { ExpressionInput, PointInput } from "./fields";
import { FILLS } from "./styles";

interface Props {
  component: ReportComponent;
  /** Page header or footer: their height is fixed, so text can't grow there. */
  pageBand?: boolean;
  columns: string[];
  queries: SavedQuery[];
  assets: AssetSummary[];
  onChange: (patch: Partial<ReportComponent>) => void;
  onDelete: () => void;
}

/** Property editor for the selected report component. */
export function ComponentProperties({
  component: c,
  pageBand = false,
  columns,
  queries,
  assets,
  onChange,
  onDelete,
}: Props) {
  const style = c.style ?? {};
  const setStyle = (patch: Partial<ComponentStyle>) => onChange({ style: { ...style, ...patch } });
  const isText = c.kind === "staticText" || c.kind === "field" || c.kind === "calculated";
  return (
    <>
      <b>{KIND_LABELS[c.kind]} properties</b>
      {c.kind === "staticText" && (
        <label>
          Text
          <textarea
            rows={2}
            value={c.text}
            onChange={(e) => onChange({ text: e.target.value } as Partial<ReportComponent>)}
          />
        </label>
      )}
      {(c.kind === "field" || c.kind === "calculated") && (
        <>
          {c.kind === "field" && columns.length > 0 && (
            <label>
              Bound field
              <select
                value=""
                onChange={(e) =>
                  e.target.value &&
                  onChange({
                    expression: fieldExpression(e.target.value),
                  } as Partial<ReportComponent>)
                }
              >
                <option value="">Choose a field…</option>
                {columns.map((col) => (
                  <option key={col} value={col}>
                    {col}
                  </option>
                ))}
              </select>
            </label>
          )}
          <ExpressionInput
            label="Expression"
            value={c.expression}
            placeholder={c.kind === "field" ? "record.amount" : "sum(rows.amount)"}
            onChange={(expression) => onChange({ expression } as Partial<ReportComponent>)}
          />
          <label>
            Format
            <input
              value={c.format ?? ""}
              placeholder="#,##0.00 or MMM d, yyyy"
              onChange={(e) => onChange({ format: e.target.value } as Partial<ReportComponent>)}
            />
          </label>
          {!pageBand && (
            <label>
              Running sum
              <select
                value={c.runningSum ?? ""}
                onChange={(e) =>
                  onChange({
                    runningSum: (e.target.value || undefined) as RunningSum | undefined,
                  } as Partial<ReportComponent>)
                }
              >
                <option value="">No</option>
                <option value="group">Over group</option>
                <option value="all">Over all</option>
              </select>
            </label>
          )}
        </>
      )}
      {c.kind === "chart" && (
        <ChartProperties
          chart={c}
          columns={columns}
          queries={queries}
          onChange={(patch) => onChange(patch as Partial<ReportComponent>)}
        />
      )}
      {c.kind === "image" && (
        <label>
          Image asset
          <select
            value={c.assetId}
            onChange={(e) => onChange({ assetId: e.target.value } as Partial<ReportComponent>)}
          >
            <option value="">None</option>
            {assets
              .filter((a) => a.mediaType.startsWith("image/"))
              .map((a) => (
                <option key={a.id} value={a.id}>
                  {a.displayName}
                </option>
              ))}
          </select>
        </label>
      )}
      {c.kind === "line" && (
        <label>
          Direction
          <select
            value={c.orientation}
            onChange={(e) => onChange({ orientation: e.target.value } as Partial<ReportComponent>)}
          >
            <option value="horizontal">Horizontal</option>
            <option value="vertical">Vertical</option>
          </select>
        </label>
      )}
      {c.kind === "table" && (
        <TableProperties
          queryId={c.queryId ?? ""}
          tableColumns={c.columns}
          columns={columns}
          queries={queries}
          onChange={(patch) => onChange(patch as Partial<ReportComponent>)}
        />
      )}
      <fieldset>
        <legend>Position and size (pt)</legend>
        <div className="grid2">
          <PointInput label="X" value={c.x} onChange={(x) => onChange({ x })} />
          <PointInput label="Y" value={c.y} onChange={(y) => onChange({ y })} />
          <PointInput label="Width" value={c.w} min={1} onChange={(w) => onChange({ w })} />
          <PointInput label="Height" value={c.h} min={1} onChange={(h) => onChange({ h })} />
        </div>
      </fieldset>
      <fieldset>
        <legend>Style</legend>
        {(isText || c.kind === "table") && (
          <div className="grid2">
            <PointInput
              label="Font size"
              min={4}
              value={style.fontSize ?? (c.kind === "table" ? 9 : 10)}
              onChange={(fontSize) => setStyle({ fontSize })}
            />
            <label>
              Alignment
              <select
                value={style.align ?? "left"}
                onChange={(e) => setStyle({ align: e.target.value as TextAlign })}
              >
                <option value="left">Left</option>
                <option value="center">Center</option>
                <option value="right">Right</option>
              </select>
            </label>
          </div>
        )}
        {isText && (
          <label className="inline">
            <input
              type="checkbox"
              checked={!!style.bold}
              onChange={(e) => setStyle({ bold: e.target.checked })}
            />
            Bold
          </label>
        )}
        {isText && !pageBand && (
          <label className="inline">
            <input
              type="checkbox"
              checked={!!c.canGrow}
              onChange={(e) =>
                onChange({ canGrow: e.target.checked || undefined } as Partial<ReportComponent>)
              }
            />
            Can grow
          </label>
        )}
        <PointInput
          label={c.kind === "line" ? "Line width" : "Border width"}
          value={style.borderWidth ?? (c.kind === "line" || c.kind === "rectangle" ? 1 : 0)}
          onChange={(borderWidth) => setStyle({ borderWidth })}
        />
        {c.kind !== "line" && c.kind !== "table" && (
          <label>
            Fill
            <select
              value={String(style.fill ?? "")}
              onChange={(e) =>
                setStyle({ fill: e.target.value === "" ? null : Number(e.target.value) })
              }
            >
              {FILLS.map(([name, value]) => (
                <option key={name} value={value === null ? "" : String(value)}>
                  {name}
                </option>
              ))}
            </select>
          </label>
        )}
      </fieldset>
      {isText && (
        <ConditionsEditor
          conditions={c.conditions ?? []}
          onChange={(conditions) =>
            onChange({
              conditions: conditions.length ? conditions : undefined,
            } as Partial<ReportComponent>)
          }
        />
      )}
      <button type="button" onClick={onDelete}>
        <Trash2 /> Delete component
      </button>
    </>
  );
}

function TableProperties({
  queryId,
  tableColumns,
  columns,
  queries,
  onChange,
}: {
  queryId: string;
  tableColumns: TableColumn[];
  columns: string[];
  queries: SavedQuery[];
  onChange: (patch: { queryId?: string; columns?: TableColumn[] }) => void;
}) {
  const setColumn = (id: string, patch: Partial<TableColumn>) =>
    onChange({ columns: tableColumns.map((col) => (col.id === id ? { ...col, ...patch } : col)) });
  const add = (name: string) =>
    onChange({
      columns: [
        ...tableColumns,
        {
          id: newId(),
          header: name || "Column",
          expression: name ? fieldExpression(name) : "",
          width: 100,
        },
      ],
    });
  return (
    <fieldset>
      <legend>Table rows and columns</legend>
      <label>
        Rows from
        <select value={queryId} onChange={(e) => onChange({ queryId: e.target.value })}>
          <option value="">This band&apos;s rows</option>
          {queries.map((q) => (
            <option key={q.id} value={q.id}>
              Query: {q.name}
            </option>
          ))}
        </select>
      </label>
      {tableColumns.map((col, i) => (
        <fieldset key={col.id}>
          <legend>Column {i + 1}</legend>
          <label>
            Column {i + 1} header
            <input
              value={col.header}
              onChange={(e) => setColumn(col.id, { header: e.target.value })}
            />
          </label>
          <ExpressionInput
            label={`Column ${i + 1} expression`}
            value={col.expression}
            onChange={(expression) => setColumn(col.id, { expression })}
          />
          <div className="grid2">
            <PointInput
              label={`Column ${i + 1} width`}
              min={1}
              value={col.width}
              onChange={(width) => setColumn(col.id, { width })}
            />
            <label>
              Column {i + 1} format
              <input
                value={col.format ?? ""}
                onChange={(e) => setColumn(col.id, { format: e.target.value })}
              />
            </label>
          </div>
          <button
            type="button"
            onClick={() => onChange({ columns: tableColumns.filter((x) => x.id !== col.id) })}
          >
            <Trash2 /> Remove column {i + 1}
          </button>
        </fieldset>
      ))}
      <label>
        Add column for field
        <select value="" onChange={(e) => e.target.value && add(e.target.value)}>
          <option value="">Choose a field…</option>
          {columns.map((col) => (
            <option key={col} value={col}>
              {col}
            </option>
          ))}
        </select>
      </label>
      <button type="button" onClick={() => add("")}>
        <Plus /> Add column
      </button>
    </fieldset>
  );
}
