import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { asTauriError } from "../lib/api";
import { readAssetText } from "./api";
import { textSections } from "./textSections";
import type { Attachment } from "./types";

/** Read-only view of a text asset, one block per titled section (VBA modules). */
export function AssetPreview({ asset, onClose }: { asset: Attachment; onClose: () => void }) {
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    setContent(null);
    setError("");
    readAssetText(asset.id)
      .then(setContent)
      .catch((reason: unknown) => setError(asTauriError(reason).message));
  }, [asset.id]);
  const sections = content === null ? [] : textSections(content);
  return (
    <section
      className="settings-section asset-preview"
      aria-label={`Preview of ${asset.displayName}`}
    >
      <div className="asset-preview-title">
        <h3>{asset.displayName}</h3>
        <button aria-label="Close preview" onClick={onClose}>
          <X aria-hidden />
        </button>
      </div>
      {error && (
        <div className="error" role="alert">
          {error}
        </div>
      )}
      {content === null && !error && <p role="status">Loading…</p>}
      {sections.length > 1 && (
        <nav aria-label="Sections">
          {sections.map((s, i) => (
            <a key={`${s.title}-${i}`} href={`#asset-section-${i}`}>
              {s.title || "Untitled"}
            </a>
          ))}
        </nav>
      )}
      {sections.map((s, i) => (
        <section
          key={`${s.title}-${i}`}
          id={`asset-section-${i}`}
          aria-label={s.title || asset.displayName}
        >
          {s.title && <h4>{s.title}</h4>}
          <pre>{s.text}</pre>
        </section>
      ))}
    </section>
  );
}
