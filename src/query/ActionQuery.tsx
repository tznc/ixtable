import { Eye, LoaderCircle, Play } from "lucide-react";
import { useState } from "react";
import { asTauriError } from "../lib/api";
import { CommittedWriteError, runActionQuery } from "../lib/records";
import type { DbObject } from "../lib/types";
import { isEmptyModel, normalizeModel } from "./builder/model";
import { toNamedValues } from "./api";
import { describeRun, KIND_LABELS } from "./actionText";
import type { ActionQueryKind, SavedQuery } from "./types";

const TEMPLATES: Record<ActionQueryKind, (table: string) => string> = {
  insert: (t) => `INSERT INTO ${t} BY NAME\nSELECT ...`,
  update: (t) => `UPDATE ${t}\nSET ... \nWHERE ...`,
  delete: (t) => `DELETE FROM ${t}\nWHERE ...`,
  replace: () => "SELECT ...",
};

const quote = (name: string) =>
  /^[a-z_][a-z0-9_]*$/.test(name) ? name : `"${name.replaceAll('"', '""')}"`;

/**
 * Query type and target table. Switching to an action query leaves the visual
 * builder: action queries are DuckDB SQL (docs/decisions/action-queries.md).
 */
export function QueryTypeFields({
  query,
  objects,
  onChange,
}: {
  query: SavedQuery;
  objects: DbObject[];
  onChange: (query: SavedQuery) => void;
}) {
  const kind = query.action?.kind ?? "read";
  const tables = objects.filter((o) => o.objectType === "table").map((o) => o.name);
  const setKind = (next: "read" | ActionQueryKind) => {
    if (next === "read") {
      onChange({ ...query, action: null });
      return;
    }
    if (
      query.builder &&
      !isEmptyModel(normalizeModel(query.builder)) &&
      !window.confirm("Action queries are written in SQL. Leave the visual builder?")
    )
      return;
    const table = query.action?.table ?? "";
    const sql = query.sql.trim() || !table ? query.sql : TEMPLATES[next](quote(table));
    onChange({ ...query, builder: null, sql, action: { kind: next, table } });
  };
  return (
    <div className="query-type">
      <label>
        Query type
        <select value={kind} onChange={(e) => setKind(e.target.value as "read" | ActionQueryKind)}>
          {Object.entries(KIND_LABELS).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </label>
      {query.action && (
        <label>
          Target table
          <select
            value={query.action.table}
            onChange={(e) => {
              const table = e.target.value;
              const sql = query.sql.trim()
                ? query.sql
                : TEMPLATES[query.action!.kind](quote(table));
              onChange({ ...query, sql, action: { ...query.action!, table } });
            }}
          >
            <option value="">Choose a table</option>
            {!tables.includes(query.action.table) && query.action.table && (
              <option value={query.action.table}>{query.action.table} (missing)</option>
            )}
            {tables.map((t) => (
              <option key={t} value={t}>
                {t}
              </option>
            ))}
          </select>
        </label>
      )}
    </div>
  );
}

/**
 * Preview (a dry run that rolls back) and Run for a saved action query. Both
 * run the saved definition, so unsaved edits must be saved first.
 */
export function ActionRunButtons({
  query,
  dirty,
  params,
  onResult,
}: {
  query: SavedQuery;
  dirty: boolean;
  params: Record<string, unknown>;
  onResult: (result: { text: string; error: boolean } | null) => void;
}) {
  const [busy, setBusy] = useState(false);
  const action = query.action;
  if (!action) return null;
  const go = async (dryRun: boolean) => {
    if (
      !dryRun &&
      !window.confirm(`Run ${query.name}? It changes rows in ${action.table} and cannot be undone.`)
    )
      return;
    setBusy(true);
    onResult(null);
    try {
      const run = await runActionQuery(query.id, toNamedValues(params), { dryRun });
      onResult({ text: describeRun(run, action.kind), error: false });
    } catch (e) {
      const saved = e instanceof CommittedWriteError ? "The changes were saved, but " : "";
      onResult({ text: saved + asTauriError(e).message, error: true });
    } finally {
      setBusy(false);
    }
  };
  const blocked = busy || dirty || !action.table;
  const why = dirty
    ? "Save the query to run it"
    : !action.table
      ? "Choose a target table"
      : undefined;
  return (
    <>
      <button disabled={blocked} title={why} onClick={() => void go(true)}>
        {busy ? <LoaderCircle className="spin" /> : <Eye />}
        Preview changes
      </button>
      <button className="save" disabled={blocked} title={why} onClick={() => void go(false)}>
        <Play />
        Run
      </button>
    </>
  );
}
