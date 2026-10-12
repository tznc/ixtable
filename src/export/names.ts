import type { ExportFormat } from "./types";

export const FORMAT_LABELS: Record<ExportFormat, string> = {
  csv: "CSV",
  xlsx: "Excel (.xlsx)",
  json: "JSON",
};

export const FILTER_NAMES: Record<ExportFormat, string> = {
  csv: "CSV files",
  xlsx: "Excel workbooks",
  json: "JSON files",
};

// Characters invalid in file names on Windows, macOS or Linux, plus control characters.
const INVALID = /[<>:"/\\|?*]/g;
const isControl = (char: string) => char.charCodeAt(0) < 32 || char.charCodeAt(0) === 127;

/** A file name stem from an object name; "export" when nothing usable is left. */
export function safeFileStem(name: string): string {
  // Windows rejects trailing dots and spaces.
  const stem = [...name.replace(INVALID, "")]
    .filter((c) => !isControl(c))
    .join("")
    .trim()
    .replace(/[. ]+$/, "")
    .trim();
  return stem || "export";
}

/** `name` with `.format` appended unless it already ends with it (case-insensitive). */
export function withExtension(name: string, format: ExportFormat): string {
  return name.toLowerCase().endsWith(`.${format}`) ? name : `${name}.${format}`;
}

/** Default file name offered by the save dialog for an object. */
export const defaultExportName = (name: string, format: ExportFormat) =>
  withExtension(safeFileStem(name), format);

/** "Exported 1,234 rows" */
export const exportedMessage = (rows: number) =>
  `Exported ${rows.toLocaleString("en-US")} ${rows === 1 ? "row" : "rows"}`;
