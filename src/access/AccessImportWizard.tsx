import { DatabaseZap, FileUp } from "lucide-react";
import { useId, useState } from "react";
import { DialogFrame } from "../components/DialogFrame";
import { asTauriError, type TauriError } from "../lib/api";
import type { SessionState } from "../lib/types";
import { importAccessFile, inspectAccessFile } from "./api";
import { chooseAccessFile } from "./dialog";
import {
  FORMAT_LABELS,
  itemsNeedingAttention,
  kindLabel,
  statusCounts,
  totalRows,
} from "./summary";
import type { AccessImport, AccessInventory } from "./types";
import "./access.css";

const fileName = (path: string) => path.split(/[\\/]/).pop() || path;
const plural = (n: number, word: string) => `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;

/**
 * Converts a Microsoft Access database or template into a new document: tables
 * with their data, relationships, queries, forms, reports and macros
 * (docs/decisions/access-import.md). Shows what the file holds first, then a
 * report of what each object became.
 */
export function AccessImportWizard({
  onClose,
  onOpened,
}: {
  onClose: () => void;
  onOpened: (state: SessionState) => void;
}) {
  const titleId = useId();
  const [path, setPath] = useState("");
  const [inventory, setInventory] = useState<AccessInventory | null>(null);
  const [includeData, setIncludeData] = useState(true);
  const [busy, setBusy] = useState("");
  const [error, setError] = useState<TauriError | null>(null);
  const [result, setResult] = useState<AccessImport | null>(null);

  const choose = async () => {
    setError(null);
    const picked = await chooseAccessFile();
    if (typeof picked !== "string") return;
    setPath(picked);
    setInventory(null);
    setBusy("Reading the Access file…");
    try {
      setInventory(await inspectAccessFile(picked));
    } catch (reason) {
      setError(asTauriError(reason));
    } finally {
      setBusy("");
    }
  };

  const run = async () => {
    setError(null);
    setBusy("Importing… large databases take a while");
    try {
      setResult(await importAccessFile(path, { includeData }));
    } catch (reason) {
      setError(asTauriError(reason));
    } finally {
      setBusy("");
    }
  };

  // Once imported, the new document is open: every way out shows it.
  const close = () => (result ? onOpened(result.state) : onClose());

  return (
    <div className="schema-dialog-backdrop">
      <DialogFrame
        className="schema-dialog import-wizard access-import"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onClose={close}
        busy={!!busy}
      >
        <h2 id={titleId}>Import Access database</h2>
        {!result && (
          <div className="settings-actions">
            <button type="button" disabled={!!busy} onClick={() => void choose()} data-autofocus>
              <FileUp aria-hidden /> Choose file…
            </button>
            <span>{path ? fileName(path) : ".accdb, .mdb, or an Access template (.accdt)"}</span>
          </div>
        )}
        {busy && <p role="status">{busy}</p>}
        {inventory && !result && (
          <InventoryView
            inventory={inventory}
            includeData={includeData}
            onIncludeData={setIncludeData}
            disabled={!!busy}
          />
        )}
        {error && (
          <div className="error" role="alert">
            <b>{error.code}</b> <span>{error.message}</span>
          </div>
        )}
        {result && <ReportView result={result} />}
        <div className="settings-actions">
          {result ? (
            <button type="button" className="save" onClick={close}>
              Open document
            </button>
          ) : (
            <>
              <button
                type="button"
                className="save"
                disabled={!!busy || !inventory}
                onClick={() => void run()}
              >
                <DatabaseZap aria-hidden /> Import
              </button>
              <button type="button" disabled={!!busy} onClick={onClose}>
                Cancel
              </button>
            </>
          )}
        </div>
      </DialogFrame>
    </div>
  );
}

function InventoryView({
  inventory,
  includeData,
  onIncludeData,
  disabled,
}: {
  inventory: AccessInventory;
  includeData: boolean;
  onIncludeData: (value: boolean) => void;
  disabled: boolean;
}) {
  const rows = totalRows(inventory);
  const objects: [string, number][] = [
    ["Queries", inventory.queries.length],
    ["Forms", inventory.forms.length],
    ["Reports", inventory.reports.length],
    ["Macros", inventory.macros.length],
    ["Modules", inventory.modules.length],
  ];
  const compiled = inventory.compiled.filter(([kind]) => kind === "form" || kind === "report");
  return (
    <section aria-label="Access file contents">
      <p>
        {FORMAT_LABELS[inventory.format]}: {plural(inventory.tables.length, "table")},{" "}
        {plural(rows, "row")}, {plural(inventory.relationships, "relationship")}.
      </p>
      <ul className="access-object-counts">
        {objects
          .filter(([, n]) => n > 0)
          .map(([label, n]) => (
            <li key={label}>
              {label}: {n}
            </li>
          ))}
      </ul>
      {compiled.length > 0 && (
        <p className="notice">
          This file stores its {compiled.length} forms and reports compiled, so they cannot be read.
          ixtable creates a list and a detail form for each table instead.
        </p>
      )}
      <div className="import-preview">
        <table className="asset-table" aria-label="Access tables">
          <thead>
            <tr>
              <th scope="col">Table</th>
              <th scope="col">Fields</th>
              <th scope="col">Rows</th>
            </tr>
          </thead>
          <tbody>
            {inventory.tables.map((t) => (
              <tr key={t.name}>
                <td>{t.name}</td>
                <td>{t.columns}</td>
                <td>{t.rows === null ? "—" : t.rows.toLocaleString()}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {inventory.warnings.length > 0 && (
        <ul aria-label="Reading problems">
          {inventory.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
      <fieldset className="import-options" disabled={disabled}>
        <legend>Options</legend>
        <label>
          <input
            type="checkbox"
            checked={includeData}
            onChange={(e) => onIncludeData(e.target.checked)}
          />{" "}
          Import the data ({rows.toLocaleString()} rows)
        </label>
      </fieldset>
    </section>
  );
}

function ReportView({ result }: { result: AccessImport }) {
  const { report } = result;
  const counts = statusCounts(report);
  const attention = itemsNeedingAttention(report);
  return (
    <section aria-label="Access import report">
      <p role="status">
        Created a document with {plural(report.tables, "table")} and {plural(report.rows, "row")}.
      </p>
      <p>Settings › Access migration keeps this report, with links, review marks and export.</p>
      <table className="asset-table" aria-label="Converted objects">
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
      {attention.length > 0 && (
        <details>
          <summary>What did not convert fully ({attention.length})</summary>
          <div className="import-preview">
            <table className="asset-table" aria-label="Conversion notes">
              <thead>
                <tr>
                  <th scope="col">Object</th>
                  <th scope="col">Notes</th>
                </tr>
              </thead>
              <tbody>
                {attention.map((item) => (
                  <tr key={`${item.kind}:${item.name}`}>
                    <td>
                      {item.name} <small>{item.kind}</small>
                    </td>
                    <td>
                      <ul>
                        {item.notes.map((n) => (
                          <li key={n}>{n}</li>
                        ))}
                      </ul>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </details>
      )}
      {report.warnings.length > 0 && (
        <ul aria-label="Reading problems">
          {report.warnings.map((w) => (
            <li key={w}>{w}</li>
          ))}
        </ul>
      )}
    </section>
  );
}
