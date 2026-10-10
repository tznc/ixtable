import { call } from "../lib/api";
import type { AccessImport, AccessImportOptions, AccessInventory } from "./types";

/** The tables, queries, forms, reports, macros and modules of an Access file. */
export const inspectAccessFile = (path: string) =>
  call<AccessInventory>("inspect_access_file", { path });

/** A new untitled document converted from an Access file, with the import report. */
export const importAccessFile = (path: string, options: AccessImportOptions) =>
  call<AccessImport>("import_access_file", { path, options });

/** Saves migration report text (Markdown or CSV) to `path`. */
export const writeAccessReport = (path: string, text: string) =>
  call<void>("write_access_report", { path, text });
