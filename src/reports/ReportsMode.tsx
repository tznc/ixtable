import { Copy, FilePlus2, FileText, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { inspectTable } from "../lib/api";
import { readQueries } from "../query/types";
import { useDocumentConfig } from "../lib/config-store";
import type { DocumentConfig } from "../lib/types";
import { useShell } from "../shell/context";
import { useReveal } from "../shell/reveal";
import { loadDataset } from "./data";
import { DatasetPicker } from "./designer/ReportSettings";
import { ReportDesigner } from "./designer/ReportDesigner";
import { duplicateReport, newReport } from "./model";
import { ReportPreview } from "./ReportPreview";
import type { Report } from "./types";
import "./reports.css";

const uniqueName = (base: string, taken: string[]) => {
  let n = 1;
  while (taken.includes(`${base} ${n}`)) n++;
  return `${base} ${n}`;
};

/** Column names of the report's dataset (for field pickers). */
function useDatasetColumns(report: Report | undefined, config: DocumentConfig) {
  const [columns, setColumns] = useState<string[]>([]);
  const queryId = report?.datasetQueryId ?? "";
  const table = report?.table ?? "";
  const sql = config.savedQueries.find((q) => q.id === queryId)?.sql ?? "";
  useEffect(() => {
    let live = true;
    const load = async () => {
      if (!report || (!queryId && !table)) return [];
      if (!queryId && table) return (await inspectTable(table)).columns.map((c) => c.name);
      return (await loadDataset(report, config, report.params, undefined, 1))?.columns ?? [];
    };
    load()
      .then((names) => live && setColumns(names))
      .catch(() => live && setColumns([]));
    return () => {
      live = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- reload only when the dataset changes
  }, [queryId, table, sql]);
  return columns;
}

/** Reports mode (PRD §15): report list, band designer, preview, print and PDF export. */
export function ReportsMode() {
  const { config, update, reload } = useDocumentConfig();
  const { objects } = useShell();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [tab, setTab] = useState<"design" | "preview">("design");
  const [focus, setFocus] = useState({ id: undefined as string | undefined, seq: 0 });
  useReveal("reports", (target) => {
    setSelectedId(target.objectId);
    setTab("design");
    setFocus((f) => ({ id: target.elementId, seq: f.seq + 1 }));
  });
  const reports = config.reports;
  const report = reports.find((r) => r.id === selectedId) ?? reports[0];
  const columns = useDatasetColumns(report, config);

  const [synced, setSynced] = useState(false);
  useEffect(() => {
    reload()
      .catch(() => undefined)
      .finally(() => setSynced(true));
  }, [reload]);

  const change = useCallback(
    (fn: (r: Report) => Report, label = "Edit report") => {
      if (!report) return;
      const id = report.id;
      update(
        (draft) => ({ ...draft, reports: draft.reports.map((r) => (r.id === id ? fn(r) : r)) }),
        label,
      ).catch(() => undefined);
    },
    [report, update],
  );

  const create = () => {
    const next = newReport(
      uniqueName(
        "Report",
        reports.map((r) => r.name),
      ),
    );
    setSelectedId(next.id);
    setTab("design");
    update((draft) => ({ ...draft, reports: [...draft.reports, next] }), "New report").catch(
      () => undefined,
    );
  };
  const duplicate = () => {
    if (!report) return;
    const copy = duplicateReport(report, `${report.name} copy`);
    setSelectedId(copy.id);
    update((draft) => ({ ...draft, reports: [...draft.reports, copy] }), "Duplicate report").catch(
      () => undefined,
    );
  };
  const remove = () => {
    if (!report) return;
    const id = report.id;
    setSelectedId(null);
    update(
      (draft) => ({ ...draft, reports: draft.reports.filter((r) => r.id !== id) }),
      "Delete report",
    ).catch(() => undefined);
  };

  return (
    <>
      <header className="titlebar">
        <div>
          <p>PROJECT / REPORTS</p>
          <h1>{report ? report.name : "Reports"}</h1>
        </div>
      </header>
      <section className="reports-mode" aria-label="Reports">
        <nav className="report-list" aria-label="Report list">
          <small>REPORTS</small>
          <button type="button" disabled={!synced} onClick={create}>
            <FilePlus2 /> New report
          </button>
          <ul>
            {reports.map((r) => (
              <li key={r.id}>
                <button
                  type="button"
                  aria-current={r.id === report?.id}
                  onClick={() => setSelectedId(r.id)}
                >
                  <FileText /> {r.name}
                </button>
              </li>
            ))}
          </ul>
          {!reports.length && <p className="report-hint">No reports yet.</p>}
        </nav>
        {!synced ? (
          <div className="report-loading" role="status">
            Loading reports…
          </div>
        ) : report ? (
          <div className="report-editor">
            <div className="report-toolbar">
              <label>
                Report name
                <input
                  value={report.name}
                  onChange={(e) => change((r) => ({ ...r, name: e.target.value }), "Rename report")}
                />
              </label>
              <DatasetPicker
                report={report}
                queries={readQueries(config.savedQueries)}
                objects={objects}
                change={change}
              />
              <button type="button" onClick={duplicate}>
                <Copy /> Duplicate report
              </button>
              <button type="button" onClick={remove}>
                <Trash2 /> Delete report
              </button>
              <div role="tablist" aria-label="Report view">
                <button
                  type="button"
                  role="tab"
                  aria-selected={tab === "design"}
                  onClick={() => setTab("design")}
                >
                  Design
                </button>
                <button
                  type="button"
                  role="tab"
                  aria-selected={tab === "preview"}
                  onClick={() => setTab("preview")}
                >
                  Preview
                </button>
              </div>
            </div>
            {tab === "design" ? (
              <ReportDesigner
                key={`${report.id}:${focus.seq}`}
                focusId={focus.id}
                report={report}
                reports={config.reports}
                queries={readQueries(config.savedQueries)}
                columns={columns}
                change={change}
              />
            ) : (
              <ReportPreview reportId={report.id} />
            )}
          </div>
        ) : (
          <div className="report-loading">Create a report to start designing.</div>
        )}
      </section>
    </>
  );
}
