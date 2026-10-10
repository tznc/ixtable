import {
  Database,
  DatabaseZap,
  FilePlus2,
  FileText,
  FolderOpen,
  PackageOpen,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import {
  asTauriError,
  listRecentFiles,
  newDocument,
  openDocument,
  type TauriError,
} from "../lib/api";
import { AccessImportWizard } from "../access";
import { CloudApps } from "../cloud";
import { chooseDocumentToOpen } from "../lib/dialog";
import { isRuntimeBundle, type OpenRequest, useEachRequest } from "../lib/launch";
import type { RecentFile, SessionState } from "../lib/types";
import { RecoveryList } from "../persistence";
import { BundleFileFlow } from "../release";
import { openRuntimeBundle } from "../release/api";
import { TemplatePicker } from "./TemplatePicker";

const RECENT_PREVIEW = 5;
const fileName = (path: string) => path.split(/[\\/]/).pop() || path;

export function StartScreen({
  onOpened,
  initialNotice = "",
  openRequest,
  onRequestHandled,
}: {
  onOpened: (state: SessionState) => void;
  initialNotice?: string;
  /** A file the OS asked to open (see `useOpenRequests`). */
  openRequest?: OpenRequest | null;
  onRequestHandled?: (id: number) => void;
}) {
  const [pending, setPending] = useState("");
  const [error, setError] = useState<TauriError | null>(null);
  const [notice, setNotice] = useState(initialNotice);
  const [recents, setRecents] = useState<RecentFile[]>([]);
  const [showAll, setShowAll] = useState(false);
  const [bundleRequest, setBundleRequest] = useState<OpenRequest | null>(null);
  const [importingAccess, setImportingAccess] = useState(false);

  useEffect(() => {
    listRecentFiles()
      .then(setRecents)
      .catch(() => setRecents([]));
  }, []);

  const run = async (label: string, action: () => Promise<SessionState | null>) => {
    setPending(label);
    setError(null);
    setNotice("");
    try {
      const state = await action();
      if (state) onOpened(state);
    } catch (reason) {
      setError(asTauriError(reason));
    } finally {
      setPending("");
    }
  };
  const create = () => run("Creating document…", newDocument);
  const open = () =>
    run("Choosing document…", async () => {
      const path = await chooseDocumentToOpen();
      if (!path) {
        setNotice("Open canceled.");
        return null;
      }
      return openDocument(path);
    });
  const openRecent = (path: string) => run("Opening document…", () => openDocument(path));
  useEachRequest(openRequest, (request) => {
    onRequestHandled?.(request.id);
    if (isRuntimeBundle(request.path)) setBundleRequest(request);
    else run("Opening document…", () => openDocument(request.path)).catch(() => undefined);
  });
  const visible = showAll ? recents : recents.slice(0, RECENT_PREVIEW);

  return (
    <div className="start-screen">
      <div className="start-brand">
        <span>ix</span>
        <b>ixtable</b>
      </div>
      <main className="start-main">
        <p className="kicker">DOCUMENT DATABASE</p>
        <h1>Your data, in one portable file.</h1>
        <p className="intro">
          Create or open an ixtable document. Every database, setting, and attachment stays together
          inside its .ixt archive.
        </p>
        <div className="start-actions">
          <button className="hero-action" disabled={!!pending} onClick={create}>
            <FilePlus2 />
            <span>
              <b>New document</b>
              <small>Start with an empty database</small>
            </span>
          </button>
          <button disabled={!!pending} onClick={open}>
            <FolderOpen />
            <span>
              <b>Open document</b>
              <small>Choose an .ixt file</small>
            </span>
          </button>
          <button disabled={!!pending} onClick={() => setImportingAccess(true)}>
            <DatabaseZap />
            <span>
              <b>Import Access database…</b>
              <small>Convert an .accdb, .mdb, or .accdt file</small>
            </span>
          </button>
          <BundleFileFlow
            disabled={!!pending}
            act={openRuntimeBundle}
            request={bundleRequest}
            onDone={(state) => onOpened(state)}
            onCancel={() => setNotice("Open canceled.")}
          >
            <PackageOpen />
            <span>
              <b>Open runtime bundle…</b>
              <small>Run an .ixtr application</small>
            </span>
          </BundleFileFlow>
        </div>
        {pending && (
          <div className="progress" role="status">
            {pending}
          </div>
        )}
        {error && (
          <div className="error" role="alert">
            <b>{error.code}</b>
            <span>{error.message}</span>
            <button aria-label="Dismiss error" onClick={() => setError(null)}>
              <X />
            </button>
          </div>
        )}
        {notice && (
          <div className="notice">
            <span className="cloud-update-notice">{notice}</span>
            <button aria-label="Dismiss" onClick={() => setNotice("")}>
              <X />
            </button>
          </div>
        )}
        <CloudApps disabled={!!pending} onOpened={onOpened} onNotice={setNotice} />
        <RecoveryList disabled={!!pending} run={run} onError={setError} />
        <TemplatePicker disabled={!!pending} run={run} />
        <section className="start-section" aria-labelledby="recent-documents">
          <div>
            <h2 id="recent-documents">Recent documents</h2>
            {recents.length > RECENT_PREVIEW && (
              <button className="text-button" onClick={() => setShowAll((value) => !value)}>
                {showAll ? "Show fewer" : "View all"}
              </button>
            )}
          </div>
          {recents.length ? (
            <ul className="recent-list">
              {visible.map((recent) => (
                <li key={recent.path}>
                  <button
                    disabled={!!pending}
                    aria-label={`Open recent ${fileName(recent.path)}`}
                    title={recent.path}
                    onClick={() => openRecent(recent.path)}
                  >
                    <FileText />
                    <span>
                      <b>{fileName(recent.path)}</b>
                      <small>{recent.path}</small>
                    </span>
                    <time dateTime={recent.openedAt}>{recent.openedAt}</time>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <div className="empty-recent">
              <Database />
              <b>No recent documents</b>
              <span>Documents you open will appear here.</span>
            </div>
          )}
        </section>
      </main>
      {importingAccess && (
        <AccessImportWizard
          onClose={() => setImportingAccess(false)}
          onOpened={(state) => {
            setImportingAccess(false);
            onOpened(state);
          }}
        />
      )}
      <footer>
        ixtable <span>Local-first document database</span>
      </footer>
    </div>
  );
}
