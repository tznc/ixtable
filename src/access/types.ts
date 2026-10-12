/** Mirrors `access::Inventory` and `access::convert::report` in src-tauri. */
import type { SessionState } from "../lib/types";

export type AccessFormat = "template" | "jet3" | "jet4" | "ace";

export interface TableSummary {
  name: string;
  columns: number;
  rows: number | null;
}

export interface AccessInventory {
  format: AccessFormat;
  tables: TableSummary[];
  relationships: number;
  queries: string[];
  forms: string[];
  reports: string[];
  macros: string[];
  modules: string[];
  // Objects a binary file stores compiled: [kind, name].
  compiled: [string, string][];
  warnings: string[];
}

export type ItemStatus = "converted" | "partial" | "skipped";

/** The ixtable object an Access object became. */
export interface ImportTarget {
  kind: "form" | "report" | "action" | "query";
  id: string;
}

export interface ImportItem {
  kind: string;
  name: string;
  status: ItemStatus;
  notes: string[];
  target?: ImportTarget;
}

/** `settings.accessImport` of an imported document: the migration report. */
export interface AccessMigration {
  source: string;
  format: AccessFormat;
  report: ImportItem[];
  warnings: string[];
  importedAt: string | null;
  // `kind:name` keys of items the developer marked as reviewed.
  reviewed: string[];
}

export interface AccessImportReport {
  items: ImportItem[];
  warnings: string[];
  tables: number;
  rows: number;
}

export interface AccessImport {
  state: SessionState;
  report: AccessImportReport;
}

export interface AccessImportOptions {
  includeData: boolean;
}
