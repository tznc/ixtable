import { Download } from "lucide-react";
import { useId, useState } from "react";
import { asTauriError, type TauriError } from "../lib/api";
import { useDocumentConfig } from "../lib/config-store";
import { type RevealTarget, useShell } from "../shell/context";
import { writeAccessReport } from "./api";
import { chooseReportDestination } from "./dialog";
import {
  filterItems,
  itemKey,
  type MigrationFilter,
  openCount,
  readMigration,
  type StatusFilter,
  statusLabel,
  toCsv,
  toMarkdown,
  withReviewed,
} from "./migration";
import { FORMAT_LABELS, KIND_ORDER, kindLabel, statusCounts } from "./summary";
import type { ImportTarget } from "./types";
import "./access.css";

const revealFor = (t: ImportTarget): RevealTarget | null => {
  switch (t.kind) {
    case "form":
      return { mode: "design", objectId: t.id };
    case "report":
      return { mode: "reports", objectId: t.id };
    case "action":
      return { mode: "automation", tab: "actions", objectId: t.id };
    default:
      return null;
  }
};

const OPEN_LABEL: Record<ImportTarget["kind"], string> = {
  form: "Open form",
  report: "Open report",
  action: "Open action",
  query: "",
};

/**
 * Settings › Access migration: everything an Access import converted, with
 * what each object lost, links to the result, review marks, and export
 * (docs/decisions/access-import.md, "Migration report").
 */
export function AccessMigrationTab() {
  const { config, update } = useDocumentConfig();
  const { requestReveal } = useShell();
  const migration = readMigration(config.settings);
  const searchId = useId();
  const [filter, setFilter] = useState<MigrationFilter>({
    status: "attention",
    kind: "",
    search: "",
    hideReviewed: false,
  });
  const [error, setError] = useState<TauriError | null>(null);
  const [notice, setNotice] = useState("");

  if (!migration) {
    return (
      <div className="settings-panel">
        <h2>Access migration</h2>
        <p>This document was not imported from Microsoft Access.</p>
      </div>
    );
  }
  const items = filterItems(migration, filter);
  const reviewed = new Set(migration.reviewed);
  const counts = statusCounts({ items: migration.report, warnings: [], tables: 0, rows: 0 });
  const kinds = KIND_ORDER.filter((k) => migration.report.some((i) => i.kind === k));
  const open = openCount(migration);

  const mark = (key: string, on: boolean) =>
    update(
      (draft) => ({ ...draft, settings: withReviewed(draft.settings, key, on) }),
      on ? "Mark Access item reviewed" : "Unmark Access item reviewed",
    );

  const exportAs = async (format: "md" | "csv") => {
    setError(null);
    setNotice("");
    const base = `${config.name || "Access"} migration report`;
    const path = await chooseReportDestination(base, format);
    if (typeof path !== "string") return;
    try {
      await writeAccessReport(
        path,
        format === "md" ? toMarkdown(migration, config.name) : toCsv(migration),
      );
      setNotice(`Saved ${path.split(/[\\/]/).pop()}`);
    } catch (reason) {
      setError(asTauriError(reason));
    }
  };

  return (
    <div className="settings-panel access-migration">
      <h2>Access migration</h2>
      <p>
        Imported from {migration.source || "an Access file"} (
        {FORMAT_LABELS[migration.format] ?? migration.format})
        {migration.importedAt ? ` on ${migration.importedAt.slice(0, 10)}` : ""}.{" "}
        {open === 0
          ? "Every item that needs attention is reviewed."
          : `${open} item${open === 1 ? "" : "s"} still need${open === 1 ? "s" : ""} review.`}
      </p>
      <table className="asset-table" aria-label="Migration summary">
        <thead>
          <tr>
            <th scope="col">Objects</th>
            <th scope="col">Converted</th>
            <th scope="col">Partly converted</th>
            <th scope="col">Not converted</th>
          </tr>
        </thead>
        <tbody>
          {counts.map((c) => (
            <tr key={c.kind}>
              <th scope="row">{kindLabel(c.kind)}</th>
              <td>{c.converted}</td>
              <td>{c.partial}</td>
              <td>{c.skipped}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="settings-actions access-migration-filters">
        <label>
          Show{" "}
          <select
            value={filter.status}
            onChange={(e) => setFilter({ ...filter, status: e.target.value as StatusFilter })}
          >
            <option value="attention">Needs attention</option>
            <option value="skipped">Not converted</option>
            <option value="partial">Partly converted</option>
            <option value="converted">Converted</option>
            <option value="all">Everything</option>
          </select>
        </label>
        <label>
          Kind{" "}
          <select
            value={filter.kind}
            onChange={(e) => setFilter({ ...filter, kind: e.target.value })}
          >
            <option value="">All kinds</option>
            {kinds.map((k) => (
              <option key={k} value={k}>
                {kindLabel(k)}
              </option>
            ))}
          </select>
        </label>
        <label htmlFor={searchId}>Search</label>
        <input
          id={searchId}
          type="search"
          value={filter.search}
          onChange={(e) => setFilter({ ...filter, search: e.target.value })}
        />
        <label>
          <input
            type="checkbox"
            checked={filter.hideReviewed}
            onChange={(e) => setFilter({ ...filter, hideReviewed: e.target.checked })}
          />{" "}
          Hide reviewed
        </label>
        <button type="button" onClick={() => void exportAs("md")}>
          <Download aria-hidden /> Export Markdown…
        </button>
        <button type="button" onClick={() => void exportAs("csv")}>
          <Download aria-hidden /> Export CSV…
        </button>
      </div>
      {notice && <p role="status">{notice}</p>}
      {error && (
        <div className="error" role="alert">
          <b>{error.code}</b> <span>{error.message}</span>
        </div>
      )}
      {items.length === 0 ? (
        <p>No items match.</p>
      ) : (
        <table className="asset-table" aria-label="Migration items">
          <thead>
            <tr>
              <th scope="col">Object</th>
              <th scope="col">Status</th>
              <th scope="col">Notes</th>
              <th scope="col">Reviewed</th>
            </tr>
          </thead>
          <tbody>
            {items.map((item) => {
              const key = itemKey(item);
              const target = item.target ? revealFor(item.target) : null;
              return (
                <tr key={key}>
                  <td>
                    {item.name} <small>{item.kind}</small>
                    {item.target && target && (
                      <div>
                        <button
                          type="button"
                          className="problem-link"
                          onClick={() => requestReveal(target)}
                          aria-label={`${OPEN_LABEL[item.target.kind]} ${item.name}`}
                        >
                          {OPEN_LABEL[item.target.kind]}
                        </button>
                      </div>
                    )}
                  </td>
                  <td>{statusLabel(item.status)}</td>
                  <td>
                    {item.notes.length > 0 && (
                      <ul>
                        {item.notes.map((n) => (
                          <li key={n}>{n}</li>
                        ))}
                      </ul>
                    )}
                  </td>
                  <td>
                    <input
                      type="checkbox"
                      aria-label={`Reviewed ${item.kind} ${item.name}`}
                      checked={reviewed.has(key)}
                      onChange={(e) => mark(key, e.target.checked)}
                    />
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      )}
      {migration.warnings.length > 0 && (
        <ul aria-label="Reading problems">
          {migration.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
    </div>
  );
}
