import { ChevronLeft, ChevronRight, FileDown, Printer } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { createPortal } from "react-dom";
import { asTauriError } from "../lib/api";
import { useDocumentConfig } from "../lib/config-store";
import { choosePdfDestination } from "../lib/dialog";
import { writeReportPdf } from "./api";
import { loadReportData, REPORT_ROW_LIMIT, type ReportData, ReportCancelled } from "./data";
import { layoutReport, type ReportDocument } from "./engine";
import { reportPdfBytes } from "./export";
import { PageView } from "./PageView";
import type { Report } from "./types";
import "./reports.css";

/** Progress and Cancel appear once loading takes this long (PRD §27.3). */
export const PROGRESS_DELAY_MS = 2000;
const ZOOMS = [0.5, 0.75, 1, 1.25, 1.5, 2];

type Loaded = { doc: ReportDocument; data: ReportData; generatedAt: string };
type Status =
  | { kind: "loading"; startedAt: number }
  | { kind: "ready"; result: Loaded }
  | { kind: "cancelled" }
  | { kind: "error"; message: string };

export interface ReportPreviewProps {
  reportId: string;
  /** Parameter values; override the report's defaults. */
  params?: Record<string, unknown>;
  /** Show Print and Export PDF (default true). */
  actions?: boolean;
}

/**
 * Paginated preview of a saved report with page navigation, zoom, print and
 * PDF export. Embeddable by dashboards and the runtime.
 */
export function ReportPreview({ reportId, params, actions = true }: ReportPreviewProps) {
  const { config } = useDocumentConfig();
  const report = config.reports.find((r) => r.id === reportId);
  if (!report) return <p role="alert">Report not found.</p>;
  return <LoadedPreview report={report} params={params} actions={actions} />;
}

function LoadedPreview({
  report,
  params,
  actions,
}: {
  report: Report;
  params?: Record<string, unknown>;
  actions: boolean;
}) {
  const { config } = useDocumentConfig();
  const [status, setStatus] = useState<Status>(() => ({
    kind: "loading",
    startedAt: Date.now(),
  }));
  const [attempt, setAttempt] = useState(0);
  const [controller, setController] = useState<AbortController | null>(null);
  const [now, setNow] = useState(() => Date.now());
  const [pageIndex, setPageIndex] = useState(0);
  const [zoom, setZoom] = useState(1);
  const [printing, setPrinting] = useState(false);
  const [notice, setNotice] = useState("");
  const [error, setError] = useState("");
  const definition = JSON.stringify(report);
  const paramKey = JSON.stringify(params ?? null);
  const queriesKey = JSON.stringify(config.savedQueries);
  // Subreports print other reports, so their edits refresh the preview too.
  const reportsKey = JSON.stringify(config.reports);

  useEffect(() => {
    const abort = new AbortController();
    const startedAt = Date.now();
    setController(abort);
    setStatus({ kind: "loading", startedAt });
    setNow(startedAt);
    const values = { ...report.params, ...params };
    const generated = new Date();
    loadReportData(report, config, values, abort.signal)
      .then((data) => {
        if (abort.signal.aborted) return;
        const assets = Object.fromEntries(
          Object.entries(data.assets).map(([id, a]) => [id, { mediaType: a.mediaType }]),
        );
        const doc = layoutReport(report, data.rows, {
          params: values,
          now: generated,
          tables: data.tables,
          assets,
          subreports: data.subreports,
        });
        setStatus({
          kind: "ready",
          result: { doc, data, generatedAt: generated.toISOString() },
        });
        setPageIndex((index) => Math.min(index, doc.pages.length - 1));
      })
      .catch((reason: unknown) => {
        if (abort.signal.aborted || reason instanceof ReportCancelled) return;
        setStatus({ kind: "error", message: asTauriError(reason).message });
      });
    return () => abort.abort();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- the JSON keys capture every input
  }, [definition, paramKey, queriesKey, reportsKey, attempt]);

  const loading = status.kind === "loading";
  useEffect(() => {
    if (!loading) return;
    const timer = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(timer);
  }, [loading]);

  const cancel = () => {
    controller?.abort();
    setStatus({ kind: "cancelled" });
  };

  const result = status.kind === "ready" ? status.result : null;
  const imageUrls = useMemo(() => {
    const urls: Record<string, string> = {};
    for (const [id, a] of Object.entries(result?.data.assets ?? {}))
      urls[id] = `data:${a.mediaType};base64,${a.dataBase64}`;
    return urls;
  }, [result]);
  const imageUrl = useCallback((id: string) => imageUrls[id], [imageUrls]);

  useEffect(() => {
    if (!printing) return;
    const done = () => setPrinting(false);
    window.addEventListener("afterprint", done);
    const timer = window.setTimeout(() => {
      try {
        window.print();
      } finally {
        window.setTimeout(done, 500);
      }
    }, 0);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener("afterprint", done);
    };
  }, [printing]);

  const exportPdf = async () => {
    if (!result) return;
    setError("");
    setNotice("");
    try {
      const path = await choosePdfDestination(report.name);
      if (!path) return;
      const { bytes, warnings } = await reportPdfBytes(result.doc, result.data.assets, {
        title: report.name,
        creationDate: result.generatedAt,
      });
      await writeReportPdf(path, bytes, report.id);
      const placeholders = warnings.length
        ? `. Printed as placeholders: ${warnings.join("; ")}`
        : "";
      setNotice(`Exported PDF to ${path}${placeholders}`);
    } catch (reason) {
      setError(asTauriError(reason).message);
    }
  };

  const doc = result?.doc;
  const total = doc?.pages.length ?? 0;
  const index = Math.min(pageIndex, Math.max(0, total - 1));
  const elapsed = status.kind === "loading" ? now - status.startedAt : 0;

  return (
    <section className="report-preview" aria-label={`${report.name} preview`}>
      <div className="report-preview-toolbar" role="toolbar" aria-label="Report preview">
        <button
          type="button"
          aria-label="Previous page"
          disabled={!doc || index === 0}
          onClick={() => setPageIndex(index - 1)}
        >
          <ChevronLeft />
        </button>
        <span role="status" aria-live="polite">
          {doc ? `Page ${index + 1} of ${total}` : "No pages"}
        </span>
        <button
          type="button"
          aria-label="Next page"
          disabled={!doc || index >= total - 1}
          onClick={() => setPageIndex(index + 1)}
        >
          <ChevronRight />
        </button>
        <label>
          Zoom
          <select value={zoom} onChange={(e) => setZoom(Number(e.target.value))}>
            {ZOOMS.map((z) => (
              <option key={z} value={z}>
                {Math.round(z * 100)}%
              </option>
            ))}
          </select>
        </label>
        {actions && (
          <>
            <button type="button" disabled={!doc} onClick={() => setPrinting(true)}>
              <Printer /> Print
            </button>
            <button type="button" disabled={!doc} onClick={() => exportPdf()}>
              <FileDown /> Export PDF…
            </button>
          </>
        )}
      </div>
      {notice && <p role="status">{notice}</p>}
      {error && <p role="alert">{error}</p>}
      {status.kind === "loading" && (
        <div className="report-loading" role="status">
          <span>Loading report data…</span>
          {elapsed >= PROGRESS_DELAY_MS && (
            <>
              <progress aria-label="Loading report data" />
              <span>{Math.floor(elapsed / 1000)} s</span>
              <button type="button" onClick={cancel}>
                Cancel
              </button>
            </>
          )}
        </div>
      )}
      {status.kind === "cancelled" && (
        <div className="report-loading" role="status">
          <span>Report loading cancelled.</span>
          <button type="button" onClick={() => setAttempt((n) => n + 1)}>
            Retry
          </button>
        </div>
      )}
      {status.kind === "error" && <p role="alert">Could not load report data: {status.message}</p>}
      {result?.data.truncated && (
        <p role="alert">
          The report shows only the first {REPORT_ROW_LIMIT.toLocaleString()} rows of a query.
        </p>
      )}
      {doc && doc.diagnostics.length > 0 && (
        <details className="report-diagnostics">
          <summary>
            {doc.diagnostics.length} expression problem{doc.diagnostics.length === 1 ? "" : "s"}
          </summary>
          <ul>
            {doc.diagnostics.map((d) => (
              <li key={`${d.componentId}:${d.message}`}>{d.message}</li>
            ))}
          </ul>
        </details>
      )}
      {doc && (
        <div className="report-page-scroller">
          <PageView
            doc={doc}
            page={doc.pages[index]}
            zoom={zoom}
            label={`Page ${index + 1} of ${total}`}
            imageUrl={imageUrl}
          />
        </div>
      )}
      {printing &&
        doc &&
        createPortal(
          <div className="report-print-root" data-testid="report-print">
            <style>{`@page { size: ${doc.width}pt ${doc.height}pt; margin: 0; }`}</style>
            {doc.pages.map((page) => (
              <div className="report-print-page" key={page.number}>
                <PageView
                  doc={doc}
                  page={page}
                  unit="pt"
                  label={`Printed page ${page.number} of ${total}`}
                  imageUrl={imageUrl}
                />
              </div>
            ))}
          </div>,
          document.body,
        )}
    </section>
  );
}
