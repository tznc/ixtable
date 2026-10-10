import { Download, Eye, FileUp, Trash2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { asTauriError, type TauriError } from "../lib/api";
import { useDocumentConfig } from "../lib/config-store";
import { useShell } from "../shell/context";
import {
  cleanupOrphanAssets,
  exportAsset,
  importAsset,
  listAssets,
  listOrphanAssets,
  removeAsset,
} from "./api";
import { ArchiveSizePanel } from "./ArchiveSizePanel";
import { AssetPreview } from "./AssetPreview";
import { isTextAsset } from "./textSections";
import { CheckpointsPanel } from "./CheckpointsPanel";
import { chooseAssetDestination, chooseAssetToImport } from "./dialog";
import { formatBytes } from "./format";
import type { Attachment } from "./types";

const plural = (count: number, word: string) => `${count} ${word}${count === 1 ? "" : "s"}`;

/** Application assets (PRD §18): import, export, remove, unused-asset cleanup, archive size. */
export function AssetsTab() {
  const { applySession } = useShell();
  const { config } = useDocumentConfig();
  const [assets, setAssets] = useState<Attachment[] | null>(null);
  const [orphans, setOrphans] = useState<Attachment[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<TauriError | null>(null);
  const [status, setStatus] = useState("");
  const [revision, setRevision] = useState(0);
  const [viewing, setViewing] = useState<Attachment | null>(null);

  const refresh = useCallback(async () => {
    const [all, unused] = await Promise.all([listAssets(), listOrphanAssets()]);
    setAssets(all);
    setOrphans(unused);
  }, []);
  useEffect(() => {
    refresh().catch((reason: unknown) => setError(asTauriError(reason)));
  }, [refresh, config]);

  const act = async (action: () => Promise<string>) => {
    setBusy(true);
    setError(null);
    setStatus("");
    try {
      setStatus(await action());
      await refresh();
      setRevision((value) => value + 1);
    } catch (reason) {
      setError(asTauriError(reason));
    } finally {
      setBusy(false);
    }
  };
  const importOne = () =>
    act(async () => {
      const path = await chooseAssetToImport();
      if (!path) return "Import canceled.";
      const result = await importAsset(path);
      applySession(result.state);
      return result.deduplicated
        ? `Same content as ${result.asset.displayName}; reused the existing asset.`
        : `Imported ${result.asset.displayName}.`;
    });
  const exportOne = (asset: Attachment) =>
    act(async () => {
      const path = await chooseAssetDestination(asset.displayName);
      if (!path) return "Export canceled.";
      await exportAsset(asset.id, path);
      return `Exported ${asset.displayName}.`;
    });
  const removeOne = (asset: Attachment) =>
    act(async () => {
      if (!window.confirm(`Remove ${asset.displayName} from this application?`)) return "";
      applySession(await removeAsset(asset.id));
      return `Removed ${asset.displayName}.`;
    });
  const cleanup = () =>
    act(async () => {
      const result = await cleanupOrphanAssets();
      applySession(result.state);
      return `Removed ${plural(result.removed.length, "unused asset")}.`;
    });

  return (
    <div className="settings-panel assets-panel">
      <h2>Assets</h2>
      <p>
        Files stored inside this application's archive, such as logos and report images. Identical
        files are stored once.
      </p>
      <div className="settings-actions">
        <button className="save" disabled={busy} onClick={importOne}>
          <FileUp aria-hidden /> Import asset
        </button>
        <span>{assets ? plural(assets.length, "asset") : "Loading assets…"}</span>
      </div>
      {error && (
        <div className="error" role="alert">
          <b>{error.code}</b>
          <span>{error.message}</span>
        </div>
      )}
      {status && <p role="status">{status}</p>}
      {!!assets?.length && (
        <table className="asset-table" aria-label="Assets">
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Type</th>
              <th scope="col">Size</th>
              <th scope="col">Checksum (SHA-256)</th>
              <th scope="col">Actions</th>
            </tr>
          </thead>
          <tbody>
            {assets.map((asset) => (
              <tr key={asset.id}>
                <td>{asset.displayName}</td>
                <td>{asset.mediaType}</td>
                <td>{formatBytes(asset.size)}</td>
                <td>
                  <code title={asset.checksum}>{asset.checksum.slice(0, 12)}…</code>
                </td>
                <td className="asset-actions">
                  {isTextAsset(asset) && (
                    <button
                      aria-label={`View ${asset.displayName}`}
                      aria-pressed={viewing?.id === asset.id}
                      onClick={() => setViewing(viewing?.id === asset.id ? null : asset)}
                    >
                      <Eye aria-hidden />
                    </button>
                  )}
                  <button
                    disabled={busy}
                    aria-label={`Export ${asset.displayName}`}
                    onClick={() => exportOne(asset)}
                  >
                    <Download aria-hidden />
                  </button>
                  <button
                    disabled={busy}
                    aria-label={`Remove ${asset.displayName}`}
                    onClick={() => removeOne(asset)}
                  >
                    <Trash2 aria-hidden />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {viewing && assets?.some((a) => a.id === viewing.id) && (
        <AssetPreview asset={viewing} onClose={() => setViewing(null)} />
      )}
      <section className="settings-section" aria-labelledby="unused-assets">
        <h3 id="unused-assets">Unused assets</h3>
        {orphans.length ? (
          <>
            <p>
              No form, report, or setting refers to {plural(orphans.length, "asset")}:{" "}
              {orphans.map((asset) => asset.displayName).join(", ")}.
            </p>
            <button disabled={busy} onClick={cleanup}>
              Remove unused assets
            </button>
          </>
        ) : (
          <p>Every asset is referenced by the application.</p>
        )}
      </section>
      <ArchiveSizePanel revision={revision} />
      <CheckpointsPanel />
    </div>
  );
}
