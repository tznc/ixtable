import { Columns3, FileUp, GitBranch, Plus, Rows3, Search, Table2, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { asTauriError, inspectTable, readTablePage } from "../lib/api";
import { CreateFormsOffer } from "../design/CreateFormsOffer";
import { firstColumnFilter } from "../export/filters";
import { TableExport } from "../export/TableExport";
import { ImportWizard } from "../import";
import { useDocumentConfig } from "../lib/config-store";
import { deleteRecord, insertRecord, updateRecord } from "../lib/records";
import type { CreateTableSpec, DbPage, NamedValue, Sort, TableSchema } from "../lib/types";
import { storeCapabilities } from "../schema/api";
import { logicalOf, valueFromText } from "../schema/logical";
import type { StoreCapabilities } from "../schema/types";
import { useShell } from "../shell/context";
import { createDatabaseTable } from "./api";
import { CreateTableForm } from "./CreateTableForm";
import { RelateDialog } from "./RelateDialog";
import { showValue } from "./format";
import {
  type DrawnRelationship,
  type NodePositions,
  RelationshipBrowser,
} from "./RelationshipBrowser";
import { TableSchemaDesigner } from "./TableSchemaDesigner";

const PAGE_SIZE = 100;

export function DatabaseWorkbench() {
  const { objects, selection: active, select: onSelect, reloadMetadata, markDirty } = useShell();
  const { config, update, reload: reloadConfig, settled } = useDocumentConfig();
  const [capabilities, setCapabilities] = useState<StoreCapabilities | null>(null);
  // The relationship editor, opened by drawing on the diagram or by the New relationship button.
  const [relating, setRelating] = useState<{ drawn: DrawnRelationship | null } | null>(null);
  const [importing, setImporting] = useState(false);
  const positions = useMemo(
    () =>
      ((config.navigationState as Record<string, unknown> | null)?.relationshipLayout ??
        {}) as NodePositions,
    [config.navigationState],
  );
  const arrange = (next: NodePositions) =>
    update(
      (draft) => ({
        ...draft,
        navigationState: {
          ...((draft.navigationState ?? {}) as Record<string, unknown>),
          relationshipLayout: next,
        },
      }),
      "Arrange relationship diagram",
    ).catch((e) => setError(asTauriError(e).message));
  // `schemasFor` is the object list the schemas were inspected for. The designer
  // waits until they match before it opens, so it never opens on (and is not then
  // remounted from) a schema that a DDL change has just made stale. Once open
  // (`designedTable`), it stays mounted through a reload: it remounts only when the
  // reloaded schema differs (its key), so a reload that changes nothing keeps drafts.
  const [schemasFor, setSchemasFor] = useState<typeof objects | null>(null);
  const [designedTable, setDesignedTable] = useState("");
  const [schemas, setSchemas] = useState<TableSchema[]>([]),
    [page, setPage] = useState<DbPage | null>(null),
    [, setLoading] = useState(false),
    [error, setError] = useState(""),
    [offset, setOffset] = useState(0),
    [revision, setRevision] = useState(0);
  const selected = active?.kind === "table" || active?.kind === "view" ? active.id : "";
  const readOnly = active?.kind === "view";
  const creating = active?.kind === "new-table";
  const [sorts, setSorts] = useState<Sort[]>([]),
    [filter, setFilter] = useState(""),
    [designingSelected, setDesigningSelected] = useState(false),
    [created, setCreated] = useState<string | null>(null);
  const [draft, setDraft] = useState<Record<number, string>>({}),
    [draftError, setDraftError] = useState("");
  const refresh = () => setRevision((x) => x + 1);
  useEffect(() => {
    let live = true;
    Promise.all(objects.filter((x) => x.objectType === "table").map((x) => inspectTable(x.name)))
      .then((next) => {
        if (!live) return;
        setSchemas(next);
        setSchemasFor(objects);
      })
      .catch((e) => setError(asTauriError(e).message));
    storeCapabilities()
      .then(setCapabilities)
      .catch(() => setCapabilities(null));
    return () => {
      live = false;
    };
  }, [objects]);
  useEffect(() => {
    if (!selected) {
      setPage(null);
      return;
    }
    setLoading(true);
    const filters = firstColumnFilter(page?.columns[0]?.name, filter);
    readTablePage(selected, { offset, limit: PAGE_SIZE, sorts, filters })
      .then(setPage)
      .catch((e) => setError(asTauriError(e).message))
      .finally(() => setLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, offset, sorts, filter, revision]);
  /**
   * After DDL: entities may have changed in the backend config, and metadata is stale.
   * The reload adds no undo step: undo cannot revert the schema, so it must not
   * revert the config that describes it either.
   */
  const afterSchemaChange = async (table: string | null) => {
    markDirty();
    await reloadConfig();
    await reloadMetadata();
    onSelect(table ? { kind: "table", id: table } : null);
    setDesigningSelected(false);
    setOffset(0);
    refresh();
  };
  const createTable = async (spec: CreateTableSpec) => {
    await settled();
    await createDatabaseTable(spec);
    await afterSchemaChange(spec.name);
    setCreated(spec.name);
  };
  /** The row as read, sent back as `expected` so optimistic entities detect concurrent edits. */
  const original = (row: number): NamedValue[] =>
    page
      ? page.columns
          .map((c, j) => ({ column: c.name, value: page.rows[row][j] }))
          .filter((_, j) => !page.columns[j].generated && logicalOf(page.columns[j]) !== "blob")
      : [];
  const commit = async (row: number, column: number, text: string) => {
    if (!page) return;
    const meta = page.columns[column];
    try {
      const value = valueFromText(text, meta.name, logicalOf(meta));
      await updateRecord(selected, [{ column: meta.name, value }], page.identities[row], {
        expected: original(row),
      });
      setError("");
      markDirty();
      refresh();
    } catch (e) {
      const failure = asTauriError(e);
      setError(failure.message);
      if (failure.code === "CONFLICT") refresh();
    }
  };
  const remove = async (row: number) => {
    if (!page || !window.confirm("Delete this row?")) return;
    try {
      await deleteRecord(selected, page.identities[row], { expected: original(row) });
      markDirty();
      if (page.rows.length === 1 && offset) setOffset(Math.max(0, offset - PAGE_SIZE));
      else refresh();
    } catch (e) {
      setError(asTauriError(e).message);
    }
  };
  const insert = async () => {
    if (!page) return;
    setDraftError("");
    try {
      const values = Object.entries(draft)
        .filter(([, text]) => text !== "")
        .map(([index, text]) => {
          const column = page.columns[Number(index)];
          return {
            column: column.name,
            value: valueFromText(text, column.name, logicalOf(column)),
          };
        });
      await insertRecord(selected, values);
      setDraft({});
      markDirty();
      refresh();
    } catch (e) {
      setDraftError(asTauriError(e).message);
    }
  };
  const selectedSchema = schemas.find((schema) => schema.name === selected);
  const schemasFresh = schemasFor === objects;
  const designerOpen = designingSelected && designedTable === selected;
  useEffect(() => {
    if (!designingSelected) setDesignedTable("");
    else if (schemasFresh && selectedSchema) setDesignedTable(selectedSchema.name);
  }, [designingSelected, schemasFresh, selectedSchema]);
  const inspectorVisible = creating || !!selected;
  return (
    <section className={`workbench ${inspectorVisible ? "" : "relationship-only"}`}>
      <div className="pane relationship-pane">
        <div className="pane-title">
          <div>
            <GitBranch />
            <span>Relationship Browser</span>
            <b>
              {schemas.length} tables · {schemas.reduce((n, s) => n + s.foreignKeys.length, 0)}{" "}
              relationships
            </b>
          </div>
          <span className="canvas-help">Drag to arrange · Scroll to zoom</span>
          <button onClick={() => setImporting(true)}>
            <FileUp />
            Import records
          </button>
          {schemas.length > 0 && (
            <button onClick={() => setRelating({ drawn: null })}>
              <Plus />
              New relationship
            </button>
          )}
        </div>
        <RelationshipBrowser
          objects={objects}
          schemas={schemas}
          selected={selected}
          onSelect={(name) => {
            onSelect({ kind: "table", id: name });
            setOffset(0);
          }}
          positions={positions}
          onArrange={arrange}
          onRelate={(drawn) => setRelating({ drawn })}
        />
        {importing && (
          <ImportWizard
            tables={schemas.filter((s) => s.objectType !== "view")}
            onClose={() => setImporting(false)}
            onImported={(table) =>
              afterSchemaChange(table ?? (active?.kind === "table" ? active.id : null))
            }
          />
        )}
        {relating && (
          <RelateDialog
            schemas={schemas}
            capabilities={capabilities}
            drawn={relating.drawn}
            childTable={
              selected && schemas.some((s) => s.name === selected)
                ? selected
                : (schemas[0]?.name ?? "")
            }
            onCancel={() => setRelating(null)}
            onApplied={async (table) => {
              setRelating(null);
              await afterSchemaChange(table);
            }}
          />
        )}
      </div>
      {inspectorVisible && (
        <>
          <div className="split-handle">
            <span>•••</span>
          </div>
          <div className="pane data-pane">
            {designingSelected && !schemasFresh && !designerOpen ? (
              <p role="status">Loading table design…</p>
            ) : designingSelected && selectedSchema ? (
              // Disabled while reloading: a remount on new schema would drop staged changes.
              <fieldset className="contents" disabled={!schemasFresh} aria-busy={!schemasFresh}>
                {!schemasFresh && <p role="status">Refreshing schema…</p>}
                <TableSchemaDesigner
                  key={JSON.stringify(selectedSchema)}
                  schema={selectedSchema}
                  tables={schemas}
                  capabilities={capabilities}
                  onChanged={(name) => afterSchemaChange(name)}
                  onDropped={() => afterSchemaChange(null)}
                  onCancel={() => setDesigningSelected(false)}
                />
              </fieldset>
            ) : creating ? (
              <CreateTableForm
                tables={schemas}
                capabilities={capabilities}
                onCreate={createTable}
                onCancel={() => onSelect(null)}
              />
            ) : !selected ? (
              <div className="empty-recent select-table">
                <Table2 />
                <b>Select a table</b>
                <span>Choose a table or view from the object browser.</span>
              </div>
            ) : (
              <>
                <div className="pane-title">
                  <div>
                    <Table2 />
                    <span>{selected}</span>
                    {readOnly && <b>Read-only view</b>}
                    <b>{page?.total.toLocaleString() ?? "—"} records</b>
                    <label className="grid-search">
                      <Search />
                      <input
                        placeholder="Filter first column"
                        aria-label="Filter first column"
                        value={filter}
                        onChange={(e) => {
                          setFilter(e.target.value);
                          setOffset(0);
                        }}
                      />
                    </label>
                    <button onClick={refresh}>
                      <Search />
                      Refresh
                    </button>
                    <TableExport
                      table={selected}
                      sorts={sorts}
                      filterText={filter}
                      firstColumn={page?.columns[0]?.name}
                    />
                    {!readOnly && (
                      <button onClick={() => setDesigningSelected(true)}>
                        <Columns3 />
                        Design table
                      </button>
                    )}
                  </div>
                </div>
                {created === selected && (
                  <CreateFormsOffer table={selected} onDismiss={() => setCreated(null)} />
                )}
                {error && (
                  <div className="error" role="alert">
                    {error}
                  </div>
                )}
                {draftError && (
                  <div className="error" role="alert">
                    {draftError}
                  </div>
                )}
                <div className="data-grid">
                  <table>
                    <thead>
                      <tr>
                        <th className="rownum">#</th>
                        {page?.columns.map((c) => (
                          <th key={c.name}>
                            <button
                              onClick={() =>
                                setSorts((old) => [
                                  {
                                    column: c.name,
                                    descending:
                                      old[0]?.column === c.name ? !old[0].descending : false,
                                  },
                                ])
                              }
                            >
                              {c.name}{" "}
                              {sorts[0]?.column === c.name ? (sorts[0].descending ? "↓" : "↑") : ""}
                            </button>
                            <small>{logicalOf(c)}</small>
                          </th>
                        ))}
                        {!readOnly && <th />}
                      </tr>
                    </thead>
                    <tbody>
                      {page?.rows.map((row, i) => (
                        <tr key={JSON.stringify(page.identities[i])}>
                          <td className="rownum">{offset + i + 1}</td>
                          {row.map((v, j) => (
                            <td
                              key={j}
                              className={page.columns[j].primaryKeyPosition ? "primary" : ""}
                            >
                              {readOnly || page.columns[j].generated ? (
                                <span>{showValue(v)}</span>
                              ) : (
                                <input
                                  key={`${revision}:${showValue(v)}`}
                                  defaultValue={showValue(v)}
                                  aria-label={`${page.columns[j].name}, row ${offset + i + 1}`}
                                  onKeyDown={(e) => {
                                    navigateDraft(e, j);
                                    if (e.key === "Enter") {
                                      e.currentTarget.blur();
                                    }
                                  }}
                                  onBlur={(e) => {
                                    if (e.currentTarget.value !== showValue(v))
                                      void commit(i, j, e.currentTarget.value);
                                  }}
                                />
                              )}
                            </td>
                          ))}
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
                      {page && !readOnly && (
                        <tr className="draft-row">
                          <td className="rownum">+</td>
                          {page.columns.map((column, j) => (
                            <td key={column.name}>
                              {column.generated ? (
                                <span>Generated</span>
                              ) : (
                                <input
                                  data-draft-index={j}
                                  aria-label={`New ${column.name}`}
                                  placeholder={column.defaultValue ? "Default" : "Enter value"}
                                  value={draft[j] ?? ""}
                                  onChange={(e) => setDraft((x) => ({ ...x, [j]: e.target.value }))}
                                  onKeyDown={(e) => {
                                    navigateDraft(e, j);
                                    if (e.key === "Enter") void insert();
                                    if (e.key === "Escape") {
                                      setDraft({});
                                      setDraftError("");
                                    }
                                  }}
                                />
                              )}
                            </td>
                          ))}
                          <td>
                            <button aria-label="Insert row" onClick={() => void insert()}>
                              <Plus />
                            </button>
                          </td>
                        </tr>
                      )}
                    </tbody>
                  </table>
                  {page && !page.rows.length && (
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
                <div className="grid-footer">
                  <span>
                    {page?.total
                      ? `${offset + 1}–${Math.min(offset + (page?.rows.length || 0), page.total)} of ${page.total}`
                      : "0 records"}
                  </span>
                  <div>
                    <button
                      aria-label="Previous page"
                      disabled={!offset}
                      onClick={() => setOffset((x) => Math.max(0, x - PAGE_SIZE))}
                    >
                      ‹
                    </button>
                    <button
                      aria-label="Next page"
                      disabled={!page || offset + PAGE_SIZE >= page.total}
                      onClick={() => setOffset((x) => x + PAGE_SIZE)}
                    >
                      ›
                    </button>
                  </div>
                </div>
              </>
            )}
          </div>
        </>
      )}
    </section>
  );
}

function navigateDraft(event: React.KeyboardEvent<HTMLInputElement>, index: number) {
  const input = event.currentTarget;
  let next = index;
  if (event.key === "ArrowRight" && input.selectionStart === input.value.length) next = index + 1;
  if (event.key === "ArrowLeft" && input.selectionStart === 0) next = index - 1;
  if (event.key === "ArrowDown") next = index + 1;
  if (event.key === "ArrowUp") next = index - 1;
  if (next === index) return;
  const target = document.querySelector<HTMLInputElement>(`[data-draft-index="${next}"]`);
  if (target) {
    event.preventDefault();
    target.focus();
    target.select();
  }
}
