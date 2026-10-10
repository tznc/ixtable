import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { Download } from "lucide-react";
import { useState } from "react";
import { asTauriError } from "../lib/api";
import { chooseExportDestination } from "./dialog";
import { exportedMessage, FORMAT_LABELS } from "./names";
import { EXPORT_FORMATS, type ExportFormat, type ExportRunner } from "./types";

/**
 * "Export…" button with a CSV / Excel / JSON menu. Choosing a format opens the save
 * dialog (cancel does nothing), then runs `onExport` and reports the row count or error.
 */
export function ExportMenu({
  name,
  onExport,
  disabled = false,
  disabledReason,
}: {
  /** Object name used for the default file name. */
  name: string;
  onExport: ExportRunner;
  disabled?: boolean;
  /** Tooltip explaining why export is unavailable. */
  disabledReason?: string;
}) {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [error, setError] = useState("");

  const choose = async (format: ExportFormat) => {
    setStatus("");
    setError("");
    try {
      const path = await chooseExportDestination(name, format);
      if (!path) return;
      setBusy(true);
      const summary = await onExport(format, path);
      setStatus(exportedMessage(summary.rows));
    } catch (e) {
      setError(asTauriError(e).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <button
            type="button"
            aria-label="Export"
            aria-busy={busy}
            disabled={disabled || busy}
            title={disabled ? disabledReason : undefined}
          >
            <Download aria-hidden />
            Export…
          </button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content className="view-menu" sideOffset={6} align="end">
            {EXPORT_FORMATS.map((format) => (
              <DropdownMenu.Item key={format} onSelect={() => void choose(format)}>
                {FORMAT_LABELS[format]}
              </DropdownMenu.Item>
            ))}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
      {status && (
        <span className="export-status" role="status">
          {status}
        </span>
      )}
      {error && (
        <span className="export-error" role="alert">
          {error}
        </span>
      )}
    </>
  );
}
