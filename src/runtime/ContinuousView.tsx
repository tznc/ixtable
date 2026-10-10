import { useEffect, useMemo, useState } from "react";
import type { DesignForm } from "../design/schema";
import { useDocumentConfig } from "../lib/config-store";
import { useConfirm } from "./Confirm";
import { pageLabel, TRUNCATED_NOTICE } from "./conditions";
import { ContinuousRow } from "./ContinuousRow";
import { loadPage, type RecordPage, sourceParams } from "./data";
import { useRuntimeNavigation } from "./navigation";
import { can } from "./rbac";
import { isDesignedForm } from "./registry";

type Props = {
  form: DesignForm;
  /** Page parameters (navigation or dashboard), in scope for query source bindings. */
  params?: Record<string, unknown>;
};

const NO_PARAMS: Record<string, unknown> = {};
type Notice = { text: string; tone: "info" | "error" };

/**
 * Continuous mode (PRD §14, Phase 6): one page of records, each drawn with the form's own
 * grid and controls and edited in place, plus a new-record row when the role may create.
 * Reads go through the same DuckDB page reader as list mode.
 */
export function ContinuousView({ form, params = NO_PARAMS }: Props) {
  const { config } = useDocumentConfig();
  const { roleId, app } = useRuntimeNavigation();
  const [offset, setOffset] = useState(0);
  const [page, setPage] = useState<RecordPage | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState<Notice | null>(null);
  // Bumped after a write so the page is read again (rows remount with stored values).
  const [version, setVersion] = useState(0);
  const [dialog, confirm] = useConfirm();
  const table = form.source?.kind === "table" ? (form.source.table ?? null) : null;
  const limit = Math.max(1, form.pageSize || 25);
  const scope = useMemo(() => ({ app, params }), [app, params]);

  useEffect(() => {
    let live = true;
    Promise.resolve()
      .then(() =>
        loadPage(form, { offset, limit, sorts: [], filters: [] }, scope, sourceParams(form, scope)),
      )
      .then((next) => {
        if (!live) return;
        setPage(next);
        setError("");
      })
      .catch(
        (reason) => live && setError(reason instanceof Error ? reason.message : String(reason)),
      );
    return () => {
      live = false;
    };
  }, [config, form, offset, limit, scope, version]);

  const subject = isDesignedForm(config, form)
    ? { kind: "form", id: form.id }
    : { kind: "table", id: table ?? "" };
  const allowed = (op: "create" | "update" | "delete", mode?: "create" | "edit") =>
    !!table &&
    (!mode || form.modes.includes(mode)) &&
    can(config, roleId, subject.kind, subject.id, op);
  const changed = (text: string, tone: Notice["tone"] = "info") => {
    if (text) setNotice({ text, tone });
    setVersion((n) => n + 1);
  };
  const total = page?.total ?? 0;

  return (
    <section className="rt-continuous" aria-label={form.name}>
      <div className="rt-record-head">
        <h2>{form.name}</h2>
      </div>
      {error && (
        <p className="rt-error" role="alert">
          {error}
        </p>
      )}
      {notice && (
        <p className={notice.tone === "error" ? "rt-error" : "rt-status"} role="status">
          {notice.text}
        </p>
      )}
      {page?.truncated && (
        <p className="rt-notice" role="status">
          {TRUNCATED_NOTICE}
        </p>
      )}
      <div className="rt-crows">
        {page?.rows.map((record, index) => (
          <ContinuousRow
            key={`${version}:${offset + index}`}
            form={form}
            table={table}
            record={record}
            identity={page.identities?.[index] ?? null}
            label={`Record ${offset + index + 1}`}
            canUpdate={allowed("update", "edit")}
            canDelete={allowed("delete")}
            confirm={confirm}
            onChanged={changed}
          />
        ))}
        {page && !page.rows.length && <p className="rt-muted">No records.</p>}
        {page && allowed("create", "create") && (
          <ContinuousRow
            key={`new:${version}`}
            form={form}
            table={table}
            label="New record"
            canUpdate={false}
            canDelete={false}
            confirm={confirm}
            onChanged={changed}
          />
        )}
      </div>
      <div className="rt-pager">
        <span>{page ? pageLabel(offset, page.rows.length, total) : "Loading…"}</span>
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
      {dialog}
    </section>
  );
}
