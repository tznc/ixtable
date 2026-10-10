import { Plus, Rows3, Trash2 } from "lucide-react";
import { type ClipboardEvent, useEffect, useLayoutEffect, useRef, useState } from "react";
import { asTauriError } from "../../lib/api";
import { deleteRecord, insertRecord, updateRecord, writeRecordBatch } from "../../lib/records";
import type { DbPage, Filter, Sort } from "../../lib/types";
import { logicalOf, valueFromText } from "../../schema/logical";
import { showValue } from "../format";
import { addFilter, selectionFilter } from "./filters";
import type { CellAt } from "./find";
import { FindReplace } from "./FindReplace";
import { freezeThrough, hideColumn, visibleColumns } from "./layout";
import { ColumnMenu } from "./ColumnMenu";
import { isMultiCell, parseClipboardGrid, pastedMessage, pasteWrites, planPaste } from "./paste";
import { isEditable, originalValues, typedValues } from "./rows";
import { navigateDraft } from "./navigation";
import { nextSorts } from "./sorts";
import { SheetToolbar } from "./SheetToolbar";
import { TotalsRow } from "./TotalsRow";
import { useSheetLayout } from "./useSheetLayout";
import "./sheet.css";

/**
 * The Data mode datasheet: in-place editing, a new-record row, multi-column sort,
 * filter by selection, find and replace, hidden and frozen columns, a totals row,
 * and multi-cell paste from spreadsheets. Hidden, frozen and totals settings are
 * saved per table in the document's navigation state.
 */
export function Datasheet({
  table,
  page,
  pageSize,
  readOnly,
  revision,
  sorts,
  onSorts,
  readFilters,
  filters,
  onFilters,
  onOffset,
  onWritten,
  onRefresh,
  onError,
}: {
  table: string;
  page: DbPage;
  pageSize: number;
  readOnly: boolean;
  revision: number;
  sorts: Sort[];
  onSorts: (sorts: Sort[]) => void;
  /** Every filter the page was read with: the search box plus `filters`. */
  readFilters: Filter[];
  /** The filters applied from the datasheet (filter by selection). */
  filters: Filter[];
  onFilters: (filters: Filter[]) => void;
  onOffset: (offset: number) => void;
  /** A write committed: mark the document dirty and reload the page. */
  onWritten: () => void;
  onRefresh: () => void;
  onError: (message: string) => void;
}) {
  const [layout, saveLayout] = useSheetLayout(table, onError);
  const [active, setActive] = useState<CellAt | null>(null);
  const [finding, setFinding] = useState(false);
  const [pending, setPending] = useState<{ offset: number; cell: CellAt } | null>(null);
  const [draft, setDraft] = useState<Record<number, string>>({});
  const [draftError, setDraftError] = useState("");
  const [notice, setNotice] = useState("");
  const [lefts, setLefts] = useState<number[]>([]);
  const heads = useRef<(HTMLTableCellElement | null)[]>([]);
  const offset = page.offset;
  const visible = visibleColumns(page.columns, layout);
  const frozen = visible.slice(0, layout.frozen);
  const frozenKey = frozen.join(",");
  const editable = (j: number) => !readOnly && isEditable(page.columns[j]);
  const editableVisible = visible.filter(editable);

  // Frozen columns stick at the summed widths of the row number and earlier frozen columns.
  useLayoutEffect(() => {
    let left = heads.current[0]?.offsetWidth ?? 0;
    const next: number[] = [];
    for (const j of frozenKey ? frozenKey.split(",").map(Number) : []) {
      next[j] = left;
      left += heads.current[j + 1]?.offsetWidth ?? 0;
    }
    setLefts((old) => (JSON.stringify(old) === JSON.stringify(next) ? old : next));
  }, [frozenKey, page]);
  const cellProps = (j: number) => {
    const at = frozen.indexOf(j);
    return at < 0
      ? {}
      : {
          className: `frozen${at === frozen.length - 1 ? " frozen-edge" : ""}`,
          style: { left: lefts[j] ?? 0 },
        };
  };

  const focusCell = (cell: CellAt) => {
    const target = document.querySelector<HTMLElement>(`[data-cell="${cell.row}:${cell.column}"]`);
    target?.focus();
    if (target instanceof HTMLInputElement) target.select();
    setActive(cell);
  };
  useEffect(() => {
    if (pending && pending.offset === page.offset) {
      focusCell(pending.cell);
      setPending(null);
    }
  }, [pending, page]);
  useEffect(() => setActive(null), [table]);

  const fail = (e: unknown) => {
    const failure = asTauriError(e);
    onError(failure.message);
    if (failure.code === "CONFLICT") onRefresh();
  };
  const commit = async (row: number, column: number, text: string) => {
    const meta = page.columns[column];
    try {
      const value = valueFromText(text, meta.name, logicalOf(meta));
      await updateRecord(table, [{ column: meta.name, value }], page.identities[row], {
        expected: originalValues(page.columns, page.rows[row]),
      });
      onError("");
      onWritten();
    } catch (e) {
      fail(e);
    }
  };
  const remove = async (row: number) => {
    if (!window.confirm("Delete this row?")) return;
    try {
      await deleteRecord(table, page.identities[row], {
        expected: originalValues(page.columns, page.rows[row]),
      });
      if (page.rows.length === 1 && offset) onOffset(Math.max(0, offset - pageSize));
      onWritten();
    } catch (e) {
      onError(asTauriError(e).message);
    }
  };
  const insert = async () => {
    setDraftError("");
    try {
      const cells = new Map(Object.entries(draft).map(([j, text]) => [Number(j), text]));
      await insertRecord(table, typedValues(page.columns, cells, false));
      setDraft({});
      onWritten();
    } catch (e) {
      setDraftError(asTauriError(e).message);
    }
  };

  /** Pastes a block copied from a spreadsheet, starting at `start`, as one batch. */
  const paste = async (event: ClipboardEvent<HTMLInputElement>, start: CellAt) => {
    const text = event.clipboardData.getData("text/plain");
    if (!isMultiCell(text)) return;
    event.preventDefault();
    const plan = planPaste(parseClipboardGrid(text), editableVisible, start, page.rows.length);
    const lastPage = offset + page.rows.length >= page.total;
    if (plan.inserts.length && start.row < page.rows.length && !lastPage) {
      setNotice(
        "The pasted rows run past this page. Paste into the new record row to add records, or start higher up.",
      );
      return;
    }
    try {
      const writes = pasteWrites(table, page, plan);
      if (!writes.length) return;
      await writeRecordBatch(writes);
      setDraft({});
      setNotice(pastedMessage(plan.updates.length, plan.inserts.length));
      onError("");
      onWritten();
    } catch (e) {
      setNotice("");
      onError(`${asTauriError(e).message} Nothing was pasted.`);
    }
  };

  const filterSelection = (exclude: boolean) => {
    if (!active) return;
    const value = page.rows[active.row]?.[active.column];
    const filter = value && selectionFilter(page.columns[active.column].name, value, exclude);
    if (filter) onFilters(addFilter(filters, filter));
  };
  const activeValue = active && page.rows[active.row]?.[active.column];

  return (
    <>
      <SheetToolbar
        canFilter={!!activeValue && activeValue.type !== "blob"}
        onFilterSelection={filterSelection}
        filters={filters}
        onRemoveFilter={(i) => onFilters(filters.filter((_, k) => k !== i))}
        onClearFilters={() => onFilters([])}
        onFind={() => setFinding(true)}
        totalsShown={!!layout.totals}
        onToggleTotals={() =>
          void saveLayout(
            { ...layout, totals: layout.totals ? null : {} },
            layout.totals ? "Hide totals row" : "Show totals row",
          )
        }
        hidden={layout.hidden.filter((name) => page.columns.some((c) => c.name === name))}
        onShow={(names) =>
          void saveLayout(
            { ...layout, hidden: layout.hidden.filter((n) => !names.includes(n)) },
            "Show columns",
          )
        }
        frozen={layout.frozen}
        onUnfreeze={() => void saveLayout({ ...layout, frozen: 0 }, "Unfreeze columns")}
      />
      {finding && (
        <FindReplace
          table={table}
          page={page}
          pageSize={pageSize}
          readOnly={readOnly}
          read={{ sorts, filters: readFilters }}
          visible={visible}
          editable={editable}
          active={active}
          onFound={(at, cell) => {
            setPending({ offset: at, cell });
            if (at !== offset) onOffset(at);
          }}
          onWritten={onWritten}
          onClose={() => setFinding(false)}
        />
      )}
      {notice && (
        <p className="sheet-notice" role="status">
          {notice}
        </p>
      )}
      {draftError && (
        <div className="error" role="alert">
          {draftError}
        </div>
      )}
      <div
        className="data-grid"
        onKeyDown={(e) => {
          if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
            e.preventDefault();
            setFinding(true);
          }
        }}
      >
        <table>
          <thead>
            <tr>
              <th
                className="rownum frozen"
                style={{ left: 0 }}
                ref={(el) => {
                  heads.current[0] = el;
                }}
              >
                #
              </th>
              {visible.map((j) => {
                const c = page.columns[j];
                const at = sorts.findIndex((s) => s.column === c.name);
                const props = cellProps(j);
                return (
                  <th
                    key={c.name}
                    className={props.className}
                    style={props.style}
                    ref={(el) => {
                      heads.current[j + 1] = el;
                    }}
                    aria-sort={
                      at === 0 ? (sorts[0].descending ? "descending" : "ascending") : undefined
                    }
                  >
                    <button
                      type="button"
                      title="Click to sort, Shift-click to add to the sort"
                      onClick={(e) => onSorts(nextSorts(sorts, c.name, e.shiftKey))}
                    >
                      {c.name} {at >= 0 && (sorts[at].descending ? "↓" : "↑")}
                      {at >= 0 && sorts.length > 1 && <sup>{at + 1}</sup>}
                    </button>
                    <ColumnMenu
                      name={c.name}
                      frozen={frozen.includes(j)}
                      canHide={visible.length > 1}
                      onSort={(descending) => onSorts([{ column: c.name, descending }])}
                      onHide={() =>
                        void saveLayout(hideColumn(layout, page.columns, c.name), "Hide column")
                      }
                      onFreeze={() =>
                        void saveLayout(
                          frozen.includes(j)
                            ? { ...layout, frozen: 0 }
                            : freezeThrough(layout, page.columns, c.name),
                          frozen.includes(j) ? "Unfreeze columns" : "Freeze columns",
                        )
                      }
                    />
                    <small>{logicalOf(c)}</small>
                  </th>
                );
              })}
              {!readOnly && <th />}
            </tr>
          </thead>
          <tbody>
            {page.rows.map((row, i) => (
              <tr key={JSON.stringify(page.identities[i])}>
                <td className="rownum frozen" style={{ left: 0 }}>
                  {offset + i + 1}
                </td>
                {visible.map((j) => {
                  const v = row[j];
                  const label = `${page.columns[j].name}, row ${offset + i + 1}`;
                  const props = cellProps(j);
                  const primary = page.columns[j].primaryKeyPosition ? "primary" : "";
                  return (
                    <td
                      key={j}
                      className={[primary, props.className].filter(Boolean).join(" ")}
                      style={props.style}
                    >
                      {editable(j) ? (
                        <input
                          key={`${revision}:${showValue(v)}`}
                          data-cell={`${i}:${j}`}
                          defaultValue={showValue(v)}
                          aria-label={label}
                          onFocus={() => setActive({ row: i, column: j })}
                          onPaste={(e) => void paste(e, { row: i, column: j })}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") e.currentTarget.blur();
                          }}
                          onBlur={(e) => {
                            if (e.currentTarget.value !== showValue(v))
                              void commit(i, j, e.currentTarget.value);
                          }}
                        />
                      ) : (
                        <span
                          role="textbox"
                          aria-readonly="true"
                          tabIndex={0}
                          data-cell={`${i}:${j}`}
                          aria-label={label}
                          onFocus={() => setActive({ row: i, column: j })}
                        >
                          {showValue(v)}
                        </span>
                      )}
                    </td>
                  );
                })}
                {!readOnly && (
                  <td>
                    <button
                      aria-label={`Delete row ${offset + i + 1}`}
                      onClick={() => void remove(i)}
                    >
                      <Trash2 />
                    </button>
                  </td>
                )}
              </tr>
            ))}
            {!readOnly && (
              <tr className="draft-row">
                <td className="rownum frozen" style={{ left: 0 }}>
                  +
                </td>
                {visible.map((j) => {
                  const column = page.columns[j];
                  const props = cellProps(j);
                  return (
                    <td key={column.name} {...props}>
                      {column.generated ? (
                        <span>Generated</span>
                      ) : (
                        <input
                          data-draft-index={visible.indexOf(j)}
                          aria-label={`New ${column.name}`}
                          placeholder={column.defaultValue ? "Default" : "Enter value"}
                          value={draft[j] ?? ""}
                          onChange={(e) => setDraft((x) => ({ ...x, [j]: e.target.value }))}
                          onPaste={(e) => void paste(e, { row: page.rows.length, column: j })}
                          onKeyDown={(e) => {
                            navigateDraft(e, visible.indexOf(j));
                            if (e.key === "Enter") void insert();
                            if (e.key === "Escape") {
                              setDraft({});
                              setDraftError("");
                            }
                          }}
                        />
                      )}
                    </td>
                  );
                })}
                <td>
                  <button aria-label="Insert row" onClick={() => void insert()}>
                    <Plus />
                  </button>
                </td>
              </tr>
            )}
          </tbody>
          {layout.totals && (
            <TotalsRow
              table={table}
              columns={page.columns}
              visible={visible}
              totals={layout.totals}
              filters={readFilters}
              revision={revision}
              cellProps={cellProps}
              trailingCell={!readOnly}
              onChange={(column, total) => {
                const totals = { ...layout.totals };
                if (total) totals[column] = total;
                else delete totals[column];
                void saveLayout({ ...layout, totals }, "Change total");
              }}
            />
          )}
        </table>
        {!page.rows.length && (
          <div className="empty-recent">
            <Rows3 />
            <b>No records yet</b>
            <span>
              {readOnly
                ? "This view returned no records."
                : "Use the new row above to add the first record."}
            </span>
          </div>
        )}
      </div>
    </>
  );
}
