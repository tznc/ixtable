import Editor from "@monaco-editor/react";
import { Blocks, Code2, Play, Save } from "lucide-react";
import { type KeyboardEvent, useEffect, useMemo, useState } from "react";
import type { DbObject } from "../lib/types";
import { runQuerySql } from "./api";
import { BuilderEditor } from "./builder/BuilderEditor";
import { tryCompile } from "./builder/compile";
import {
  type BuilderModel,
  builderParameterNames,
  emptyModel,
  isEmptyModel,
  normalizeModel,
} from "./builder/model";
import { ActionRunButtons, QueryTypeFields } from "./ActionQuery";
import { ParametersPanel, ParameterValues } from "./ParametersPanel";
import { effectiveSql, placeholderNames, runParams } from "./sql";
import type { SavedQuery } from "./types";
import { RunResults } from "./RunStatus";
import { useQueryRun } from "./useQueryRun";

/** Rows shown by the builder's live preview. */
const PREVIEW_ROWS = 100;
const PREVIEW_DELAY_MS = 400;

type Tab = "builder" | "sql";

export function QueryEditor({
  query,
  objects,
  dirty,
  saving,
  onChange,
  onSave,
}: {
  query: SavedQuery;
  objects: DbObject[];
  dirty: boolean;
  saving: boolean;
  onChange: (query: SavedQuery) => void;
  onSave: () => void;
}) {
  const model = useMemo(
    () => (query.builder ? normalizeModel(query.builder) : null),
    [query.builder],
  );
  const [tab, setTab] = useState<Tab>(model ? "builder" : "sql");
  const [values, setValues] = useState<Record<string, string>>({});
  const run = useQueryRun();
  const [actionResult, setActionResult] = useState<{ text: string; error: boolean } | null>(null);
  const action = query.action ?? null;
  const parameters = useMemo(() => query.parameters ?? [], [query.parameters]);
  const compiled = useMemo(() => effectiveSql(query), [query]);
  const referenced = model ? builderParameterNames(model) : placeholderNames(query.sql);
  const visual = !!model;

  useEffect(() => {
    setTab(query.builder ? "builder" : "sql");
    setValues({});
    setActionResult(null);
    run.reset();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query.id]);

  // Live preview of the builder (debounced, first PREVIEW_ROWS rows).
  const previewKey =
    visual && tab === "builder" && model && !isEmptyModel(model) && compiled.sql
      ? JSON.stringify([compiled.sql, parameters, runParams(values)])
      : "";
  useEffect(() => {
    if (!previewKey) return;
    const timer = setTimeout(() => {
      void run.run((runId) =>
        runQuerySql(compiled.sql, runParams(values), {
          parameters,
          limit: PREVIEW_ROWS,
          runId,
        }),
      );
    }, PREVIEW_DELAY_MS);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [previewKey]);

  const setModel = (next: BuilderModel) =>
    onChange({ ...query, builder: next, sql: tryCompile(next).sql || query.sql });
  const execute = () => {
    if (compiled.error) return;
    void run.run((runId) => runQuerySql(compiled.sql, runParams(values), { parameters, runId }));
  };
  const switchToSql = () => {
    if (
      model &&
      !isEmptyModel(model) &&
      !window.confirm(
        "Switch this query to SQL? The visual builder model is discarded and the query cannot return to the builder.",
      )
    )
      return;
    onChange({ ...query, builder: null, sql: compiled.sql || query.sql });
    setTab("sql");
  };
  const startBuilder = () => {
    if (
      query.sql.trim() &&
      !window.confirm("Start the visual builder? The current SQL is replaced.")
    )
      return;
    onChange({ ...query, builder: emptyModel(), sql: "" });
  };
  const onTabKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const next: Tab = tab === "builder" ? "sql" : "builder";
    setTab(next);
    document.getElementById(`query-tab-${next}`)?.focus();
  };

  return (
    <div className="query-editor">
      <div className="pane-title">
        <div>
          <Code2 />
          <input
            aria-label="Query name"
            value={query.name}
            onChange={(e) => onChange({ ...query, name: e.target.value })}
          />
          <b className="query-mode-badge" title="Authoring mode">
            {action ? "Action query" : visual ? "Visual builder" : "SQL"}
          </b>
          {dirty && <small>Unsaved changes</small>}
          {action ? (
            <ActionRunButtons
              query={query}
              dirty={dirty}
              params={runParams(values)}
              onResult={setActionResult}
            />
          ) : (
            <button className="save" disabled={run.running || !!compiled.error} onClick={execute}>
              <Play />
              Run
            </button>
          )}
          <button disabled={saving} onClick={onSave}>
            <Save />
            Save query
          </button>
        </div>
      </div>
      <QueryTypeFields query={query} objects={objects} onChange={onChange} />
      {!action && (
        <div className="query-tabs" role="tablist" aria-label="Query editor" onKeyDown={onTabKey}>
          {(["builder", "sql"] as const).map((id) => (
            <button
              key={id}
              id={`query-tab-${id}`}
              role="tab"
              aria-selected={tab === id}
              aria-controls="query-tab-panel"
              tabIndex={tab === id ? 0 : -1}
              className={tab === id ? "active" : ""}
              onClick={() => setTab(id)}
            >
              {id === "builder" ? <Blocks /> : <Code2 />}
              {id === "builder" ? "Builder" : "SQL"}
            </button>
          ))}
        </div>
      )}
      <div
        id="query-tab-panel"
        role="tabpanel"
        aria-labelledby={`query-tab-${tab}`}
        className="query-tab-panel"
      >
        {tab === "builder" && !action ? (
          model ? (
            <BuilderEditor
              model={model}
              objects={objects}
              parameters={parameters}
              onChange={setModel}
            />
          ) : (
            <div className="empty-recent">
              <Code2 />
              <b>This query is written in SQL</b>
              <button onClick={startBuilder}>Start visual builder</button>
            </div>
          )
        ) : model && !isEmptyModel(model) ? (
          <div className="query-generated">
            <pre aria-label="Generated SQL">{compiled.sql || compiled.error}</pre>
            <button onClick={switchToSql}>Switch to SQL</button>
          </div>
        ) : (
          <div className="monaco-shell">
            <Editor
              height="200px"
              language="sql"
              value={query.sql}
              onChange={(sql) => onChange({ ...query, builder: null, sql: sql ?? "" })}
              options={{ minimap: { enabled: false }, fontSize: 12, automaticLayout: true }}
            />
          </div>
        )}
      </div>
      {compiled.error && model && !isEmptyModel(model) && (
        <div className="notice" role="note">
          {compiled.error}
        </div>
      )}
      <ParametersPanel
        parameters={parameters}
        referenced={referenced}
        onChange={(next) => onChange({ ...query, parameters: next })}
      />
      <ParameterValues parameters={parameters} values={values} onChange={setValues} />
      {action ? (
        <section className="query-results" aria-label="Action query result">
          {actionResult && (
            <div
              className={actionResult.error ? "error" : "query-status"}
              role={actionResult.error ? "alert" : "status"}
            >
              {actionResult.text}
            </div>
          )}
        </section>
      ) : (
        <section className="query-results" aria-label={visual ? "Preview" : "Results"}>
          <RunResults run={run} />
        </section>
      )}
    </div>
  );
}
