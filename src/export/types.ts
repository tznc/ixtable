export const EXPORT_FORMATS = ["csv", "xlsx", "json"] as const;
export type ExportFormat = (typeof EXPORT_FORMATS)[number];

/** What the Rust export commands return. */
export interface ExportSummary {
  rows: number;
  path: string;
}

/** Runs an export of the current view into `path`; supplied by each entry point. */
export type ExportRunner = (format: ExportFormat, path: string) => Promise<ExportSummary>;
