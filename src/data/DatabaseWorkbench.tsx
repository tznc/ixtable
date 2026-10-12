import { Columns3, FileUp, GitBranch, Plus, Search, Table2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { asTauriError, inspectTable, readTablePage } from "../lib/api";
import { CreateFormsOffer } from "../design/CreateFormsOffer";
import { firstColumnFilter } from "../export/filters";
import { TableExport } from "../export/TableExport";
import { ImportWizard } from "../import";
import { useDocumentConfig } from "../lib/config-store";
import type { CreateTableSpec, DbPage, Filter, Sort, TableSchema } from "../lib/types";
import { storeCapabilities } from "../schema/api";
import type { StoreCapabilities } from "../schema/types";
import { useShell } from "../shell/context";
import { createDatabaseTable } from "./api";
import { CreateTableForm } from "./CreateTableForm";
import { RelateDialog } from "./RelateDialog";
import { Datasheet } from "./sheet/Datasheet";
import {
  type DrawnRelationship,
  type NodePositions,
  RelationshipBrowser,
} from "./RelationshipBrowser";
import { TableSchemaDesigner } from "./TableSchemaDesigner";

const PAGE_SIZE = 100;
const NO_FILTERS: Filter[] = [];

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
  // Filters applied from the datasheet (filter by selection), on top of the search box.
  const [applied, setApplied] = useState<{ table: string; filters: Filter[] }>({
    table: "",
    filters: [],
  });
  const sheetFilters = applied.table === selected ? applied.filters : NO_FILTERS;
  const setSheetFilters = (filters: Filter[]) => setApplied({ table: selected, filters });
  const readFilters = useMemo(
    () => [...firstColumnFilter(page?.columns[0]?.name, filter), ...sheetFilters],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [page?.columns[0]?.name, filter, sheetFilters],
  );
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
    readTablePage(selected, { offset, limit: PAGE_SIZE, sorts, filters: readFilters })
      .then(setPage)
      .catch((e) => setError(asTauriError(e).message))
      .finally(() => setLoading(false));
  }, [selected, offset, sorts, readFilters, revision]);
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
                    <TableExport table={selected} sorts={sorts} filters={readFilters} />
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
                {page && (
                  <Datasheet
                    table={selected}
                    page={page}
                    pageSize={PAGE_SIZE}
                    readOnly={readOnly}
                    revision={revision}
                    sorts={sorts}
                    onSorts={setSorts}
                    readFilters={readFilters}
                    filters={sheetFilters}
                    onFilters={(next) => {
                      setSheetFilters(next);
                      setOffset(0);
                    }}
                    onOffset={setOffset}
                    onWritten={() => {
                      markDirty();
                      refresh();
                    }}
                    onRefresh={refresh}
                    onError={setError}
                  />
                )}
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
