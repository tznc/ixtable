import { ArrowDown, ArrowUp } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { humanize } from "../design/generate";
import type { DesignControl, DesignForm } from "../design/schema";
import { useDocumentConfig } from "../lib/config-store";
import type { Filter, Sort, TableSchema } from "../lib/types";
import {
  cachedPage,
  loadPage,
  type PageRequest,
  type RecordPage,
  recordIdFor,
  sourceParams,
  tableSchema,
} from "./data";
import type { RecordCursor } from "./cursor";
import { pageLabel, TRUNCATED_NOTICE, toneClass, toneFor } from "./conditions";
import { cellText } from "./formState";
import { BooleanCell } from "./BooleanCell";
import { useLookupLabels } from "./lookups";
import { useRuntimeNavigation } from "./navigation";
import { can } from "./rbac";
import { isDesignedForm, resolveForm } from "./registry";
import { isBooleanColumn } from "./values";

type Props = {
  form: DesignForm;
  /** Opens a row; `cursor` is its place in the list's order (for record navigation). */
  onOpen: (recordId: unknown, cursor: RecordCursor) => void;
  onCreate: () => void;
  /** Page parameters (navigation or dashboard), in scope for query source bindings. */
  params?: Record<string, unknown>;
  /** Position (across pages) of the row shown as selected (split forms). */
  selected?: number | null;
  /** Changing it re-reads the current page (split forms, after a save in the detail pane). */
  refresh?: number;
};

const NO_PARAMS: Record<string, unknown> = {};

/**
 * List mode: DuckDB-backed paging with sort and a contains-filter, rows open the detail form.
 * The form's `filter` expression runs on fetched rows (see `loadPage`); cells take the
 * conditional tone of the control bound to their column.
 */
export function ListView({
  form,
  onOpen,
  onCreate,
  params = NO_PARAMS,
  selected = null,
  refresh = 0,
}: Props) {
  const { config } = useDocumentConfig();
  const { roleId, app } = useRuntimeNavigation();
  const [sorts, setSorts] = useState<Sort[]>([]);
  const [offset, setOffset] = useState(0);
  const [search, setSearch] = useState("");
  const [searchColumn, setSearchColumn] = useState("");
  const [schema, setSchema] = useState<TableSchema | null>(null);
  const [error, setError] = useState("");
  const table = form.source?.kind === "table" ? (form.source.table ?? null) : null;
  const detail = (form.detailFormId && resolveForm(config, form.detailFormId)) || form;
  const limit = Math.max(1, form.pageSize || 25);

  const filters: Filter[] = useMemo(
    () =>
      search.trim() && searchColumn
        ? [
            {
              column: searchColumn,
              operator: "contains",
              value: { type: "text", value: search.trim() },
            },
          ]
        : [],
    [search, searchColumn],
  );
  const request: PageRequest = useMemo(
    () => ({ offset, limit, sorts, filters }),
    [offset, limit, sorts, filters],
  );
  // Bound query parameters; JSON keeps the effect below from rerunning on equal values.
  const bound = useMemo(() => {
    try {
      return { json: JSON.stringify(sourceParams(form, { app, params })), error: "" };
    } catch (reason) {
      return { json: "{}", error: reason instanceof Error ? reason.message : String(reason) };
    }
  }, [form, app, params]);
  // New parameter values start again at the first page.
  const [boundFor, setBoundFor] = useState(bound.json);
  if (boundFor !== bound.json) {
    setBoundFor(bound.json);
    if (offset !== 0) setOffset(0);
  }
  // Row filter scope.
  const scope = useMemo(() => ({ app, params }), [app, params]);
  const [page, setPage] = useState<RecordPage | null>(() =>
    cachedPage(form, request, scope, JSON.parse(bound.json)),
  );

  useEffect(() => {
    let live = true;
    if (bound.error) {
      setError(bound.error);
      setPage(null);
      return;
    }
    const values = JSON.parse(bound.json) as Record<string, unknown>;
    const hit = cachedPage(form, request, scope, values);
    if (hit) setPage(hit);
    const timer = setTimeout(
      () => {
        loadPage(form, request, scope, values)
          .then((next) => {
            if (!live) return;
            setPage(next);
            setError("");
          })
          .catch(
            (reason) => live && setError(reason instanceof Error ? reason.message : String(reason)),
          );
      },
      search ? 200 : 0,
    );
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [config, form, request, search, bound, scope, refresh]);

  useEffect(() => {
    if (table)
      tableSchema(table)
        .then(setSchema)
        .catch(() => setSchema(null));
  }, [table]);

  const controlFor = (column: string): DesignControl | undefined =>
    [...form.controls, ...detail.controls].find((c) => c.binding?.column === column);
  const columns = form.listColumns.length
    ? form.listColumns
    : form.controls.flatMap((c) => (c.binding?.column ? [c.binding.column] : [])).length
      ? form.controls.flatMap((c) => (c.binding?.column ? [c.binding.column] : []))
      : (page?.columns ?? []);
  const label = (column: string) => controlFor(column)?.label ?? humanize(column);
  const lookup = useLookupLabels(table, columns, controlFor, page?.rows);
  const booleans = new Set(
    columns.filter((column) =>
      isBooleanColumn(
        [...form.controls, ...detail.controls].filter((c) => c.binding?.column === column),
        schema?.columns.find((c) => c.name === column),
      ),
    ),
  );
  const cell = (record: Record<string, unknown>, column: string) =>
    booleans.has(column) ? (
      <BooleanCell value={record[column]} />
    ) : (
      (lookup(column, record) ?? cellText(record[column], controlFor(column)))
    );
  const subject = isDesignedForm(config, detail)
    ? { kind: "form", id: detail.id }
    : { kind: "table", id: table ?? "" };
  const canCreate =
    !!table &&
    detail.modes.includes("create") &&
    can(config, roleId, subject.kind, subject.id, "create");
  const effectiveColumn = searchColumn || columns[0] || "";

  const toggleSort = (column: string) => {
    setOffset(0);
    setSorts((current) => {
      const existing = current.find((s) => s.column === column);
      if (!existing) return [{ column, descending: false }];
      if (!existing.descending) return [{ column, descending: true }];
      return [];
    });
  };
  const open = (index: number, row: Element) => {
    // Rows are not native controls, so a disabled (inert) container must be checked here.
    if (!page || row.closest("[inert], [aria-disabled='true']")) return;
    const record = page.rows[index];
    const cursor: RecordCursor = { formId: form.id, index: offset + index, sorts, filters, params };
    if (!table || !page.identities) return onOpen(record, cursor);
    const identity = page.identities[index];
    if (schema) return onOpen(recordIdFor(schema, record, identity), cursor);
    // The schema can still be loading after a database change cleared the cache; never open by row.
    tableSchema(table)
      .then((def) => onOpen(recordIdFor(def, record, identity), cursor))
      .catch(() => undefined);
  };
  const total = page?.total ?? 0;
  const tone = (record: Record<string, unknown>, column: string) =>
    toneClass(
      toneFor(controlFor(column)?.styles, { record, form: {}, app, value: record[column] }),
    ) || undefined;

  return (
    <section className="rt-list" aria-label={form.name}>
      <div className="rt-record-head">
        <h2>{form.name}</h2>
        <div className="rt-actions">
          <label className="rt-search">
            <span className="sr-only">Search column</span>
            <select
              aria-label="Search column"
              value={effectiveColumn}
              onChange={(e) => {
                setSearchColumn(e.target.value);
                setOffset(0);
              }}
            >
              {columns.map((column) => (
                <option key={column} value={column}>
                  {label(column)}
                </option>
              ))}
            </select>
          </label>
          <input
            type="search"
            aria-label={`Search ${form.name}`}
            placeholder="Search…"
            value={search}
            onChange={(e) => {
              setSearchColumn(effectiveColumn);
              setSearch(e.target.value);
              setOffset(0);
            }}
          />
          {canCreate && (
            <button type="button" className="primary" onClick={onCreate}>
              New {detail.name.toLowerCase()}
            </button>
          )}
        </div>
      </div>
      {error && (
        <p className="rt-error" role="alert">
          {error}
        </p>
      )}
      {page?.truncated && (
        <p className="rt-notice" role="status">
          {TRUNCATED_NOTICE}
        </p>
      )}
      <table className="rt-table">
        <thead>
          <tr>
            {columns.map((column) => {
              const sort = sorts.find((s) => s.column === column);
              return (
                <th
                  key={column}
                  scope="col"
                  aria-sort={sort ? (sort.descending ? "descending" : "ascending") : "none"}
                >
                  <button type="button" onClick={() => toggleSort(column)}>
                    {label(column)}
                    {sort &&
                      (sort.descending ? (
                        <ArrowDown aria-hidden="true" />
                      ) : (
                        <ArrowUp aria-hidden="true" />
                      ))}
                  </button>
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {page?.rows.map((record, index) => (
            <tr
              key={index}
              tabIndex={0}
              className={selected === offset + index ? "rt-row rt-row-selected" : "rt-row"}
              aria-current={selected === offset + index ? "true" : undefined}
              aria-label={`Open ${record[columns[0]] == null ? `row ${offset + index + 1}` : (lookup(columns[0], record) ?? String(record[columns[0]]))}`}
              onClick={(event) => open(index, event.currentTarget)}
              onKeyDown={(event) => {
                if (event.key === "Enter" || event.key === " ") {
                  event.preventDefault();
                  open(index, event.currentTarget);
                }
              }}
            >
              {columns.map((column) => (
                <td key={column} className={tone(record, column)}>
                  {cell(record, column)}
                </td>
              ))}
            </tr>
          ))}
          {page && !page.rows.length && (
            <tr>
              <td colSpan={Math.max(1, columns.length)} className="rt-muted">
                No records.
              </td>
            </tr>
          )}
        </tbody>
      </table>
      <div className="rt-pager">
        <span role="status">{page ? pageLabel(offset, page.rows.length, total) : "Loading…"}</span>
        <button
          type="button"
          disabled={offset === 0}
          onClick={() => setOffset(Math.max(0, offset - limit))}
        >
          Previous page
        </button>
        <button
          type="button"
          disabled={offset + limit >= total}
          onClick={() => setOffset(offset + limit)}
        >
          Next page
        </button>
      </div>
    </section>
  );
}
