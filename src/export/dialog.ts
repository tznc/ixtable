import { save } from "@tauri-apps/plugin-dialog";
import { defaultExportName, FILTER_NAMES, withExtension } from "./names";
import type { ExportFormat } from "./types";

/** Save dialog for a data export. Resolves to the chosen path (extension ensured) or null. */
export async function chooseExportDestination(name: string, format: ExportFormat) {
  const path = await save({
    title: "Export data",
    defaultPath: defaultExportName(name, format),
    filters: [{ name: FILTER_NAMES[format], extensions: [format] }],
  });
  return path ? withExtension(path, format) : null;
}
