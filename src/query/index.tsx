import { FileCode2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { asTauriError } from "../lib/api";
import { useDocumentConfig } from "../lib/config-store";
import { newId } from "../lib/utils";
import { useShell } from "../shell/context";
import { checkActionQuerySql, checkQuerySql } from "./api";
import { emptyModel } from "./builder/model";
import { QueryEditor } from "./QueryEditor";
import { QueryList } from "./QueryList";
import { effectiveSql } from "./sql";
import type { SavedQuery } from "./types";

const clone = <T,>(value: T): T => JSON.parse(JSON.stringify(value)) as T;

function uniqueName(base: string, queries: SavedQuery[]) {
  const names = new Set(queries.map((q) => q.name.toLowerCase()));
  if (!names.has(base.toLowerCase())) return base;
  let n = 2;
  while (names.has(`${base} ${n}`.toLowerCase())) n++;
  return `${base} ${n}`;
}

/** Query mode (PRD §12): saved queries, visual builder, SQL, parameters, and results. */
export function QueryMode() {
  const { objects, selection, select } = useShell();
  const { config, update } = useDocumentConfig();
  const queries = config.savedQueries;
  const [draft, setDraft] = useState<SavedQuery | null>(null);
  const [baseline, setBaseline] = useState("");
  const [isNew, setIsNew] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const dirty = !!draft && JSON.stringify(draft) !== baseline;

  const open = (query: SavedQuery, fresh = false) => {
    const copy = clone(query);
    setDraft(copy);
    setBaseline(JSON.stringify(copy));
    setIsNew(fresh);
    setError("");
  };
  const confirmDiscard = () =>
    !dirty || window.confirm(`Discard unsaved changes to ${draft?.name || "this query"}?`);
  const newQuery = () => {
    if (!confirmDiscard()) return;
    open(
      {
        id: newId(),
        name: uniqueName("Untitled query", queries),
        sql: "",
        parameters: [],
        builder: emptyModel(),
      },
      true,
    );
  };

  // Follow the shell selection (Data mode's object browser opens queries here).
  useEffect(() => {
    if (selection?.kind === "new-query") {
      newQuery();
      select(null);
    } else if (selection?.kind === "query" && selection.id !== draft?.id) {
      const query = queries.find((q) => q.id === selection.id);
      if (query && confirmDiscard()) open(query);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selection?.kind, selection?.id]);

  // Keep a clean draft in step with external edits (undo/redo, YAML).
  const saved = useMemo(() => queries.find((q) => q.id === draft?.id), [queries, draft?.id]);
  useEffect(() => {
    if (!draft || isNew || dirty) return;
    if (!saved) setDraft(null);
    else if (JSON.stringify(saved) !== baseline) open(saved);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [saved]);

  const openId = (id: string) => {
    const query = queries.find((q) => q.id === id);
    if (!query || id === draft?.id || !confirmDiscard()) return;
    open(query);
    select({ kind: "query", id });
  };
  const save = async () => {
    if (!draft) return;
    setSaving(true);
    setError("");
    try {
      if (!draft.name.trim()) throw new Error("Give the query a name");
      if (
        queries.some(
          (q) => q.id !== draft.id && q.name.toLowerCase() === draft.name.trim().toLowerCase(),
        )
      )
        throw new Error(`Another query is already named ${draft.name.trim()}`);
      // An unfinished builder model may be saved (Problems lists it); SQL must be
      // one read-only statement DuckDB can prepare.
      const { sql, error: compileError } = effectiveSql(draft);
      if (draft.action) {
        if (!draft.action.table) throw new Error("Choose the table this action query changes");
        await checkActionQuerySql(sql, draft.action);
      } else if (!draft.builder) await checkQuerySql(sql);
      const query: SavedQuery = {
        filterState: null,
        ...draft,
        name: draft.name.trim(),
        sql: compileError ? "" : sql,
      };
      await update(
        (config) => ({
          ...config,
          savedQueries: config.savedQueries.some((q) => q.id === query.id)
            ? config.savedQueries.map((q) => (q.id === query.id ? query : q))
            : [...config.savedQueries, query],
        }),
        `Save query ${query.name}`,
      );
      open(query);
      select({ kind: "query", id: query.id });
    } catch (e) {
      setError(asTauriError(e).message);
    } finally {
      setSaving(false);
    }
  };
  const rename = (id: string, name: string) => {
    if (queries.some((q) => q.id !== id && q.name.toLowerCase() === name.toLowerCase())) {
      setError(`Another query is already named ${name}`);
      return;
    }
    if (draft?.id === id) setDraft({ ...draft, name });
    update(
      (config) => ({
        ...config,
        savedQueries: config.savedQueries.map((q) => (q.id === id ? { ...q, name } : q)),
      }),
      `Rename query ${name}`,
    ).catch((e) => setError(asTauriError(e).message));
  };
  const duplicate = (id: string) => {
    const source = queries.find((q) => q.id === id);
    if (!source || !confirmDiscard()) return;
    const copy: SavedQuery = {
      ...clone(source),
      id: newId(),
      name: uniqueName(`${source.name} copy`, queries),
    };
    update(
      (config) => ({ ...config, savedQueries: [...config.savedQueries, copy] }),
      `Duplicate query ${source.name}`,
    )
      .then(() => {
        open(copy);
        select({ kind: "query", id: copy.id });
      })
      .catch((e) => setError(asTauriError(e).message));
  };
  const remove = (id: string) => {
    const query = queries.find((q) => q.id === id);
    if (!query || !window.confirm(`Delete query ${query.name}?`)) return;
    update(
      (config) => ({
        ...config,
        savedQueries: config.savedQueries.filter((q) => q.id !== id),
      }),
      `Delete query ${query.name}`,
    )
      .then(() => {
        if (draft?.id === id) setDraft(null);
        if (selection?.id === id) select(null);
      })
      .catch((e) => setError(asTauriError(e).message));
  };

  return (
    <section className="query-workspace" aria-label="Query mode">
      <header className="titlebar">
        <div>
          <p>PROJECT / QUERIES</p>
          <h1>Query</h1>
        </div>
      </header>
      <div className="query-mode">
        <QueryList
          queries={queries}
          activeId={draft && !isNew ? draft.id : null}
          draftLabel={draft && isNew ? draft.name || "Untitled query" : null}
          onOpen={openId}
          onNew={newQuery}
          onRename={rename}
          onDuplicate={duplicate}
          onDelete={remove}
        />
        <div className="pane query-pane">
          {error && (
            <div className="error" role="alert">
              {error}
            </div>
          )}
          {draft ? (
            <QueryEditor
              query={draft}
              objects={objects}
              dirty={dirty}
              saving={saving}
              onChange={setDraft}
              onSave={() => void save()}
            />
          ) : (
            <div className="empty-recent">
              <FileCode2 />
              <b>Select or create a query</b>
              <span>
                Build a query visually, write read-only SQL, or write an action query that changes
                rows.
              </span>
              <button onClick={newQuery}>Create a query</button>
            </div>
          )}
        </div>
      </div>
    </section>
  );
}
