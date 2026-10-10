import type { DocumentConfig } from "../lib/types";
import { richTextPlain } from "./richtext";
import type { AttachmentRef, FieldSettings } from "./types";

/** The field settings of `table.column`, if any. */
export function fieldSettingsFor(
  config: Pick<DocumentConfig, "entities"> | null | undefined,
  table: string | null | undefined,
  column: string | null | undefined,
): FieldSettings | undefined {
  if (!config || !table || !column) return undefined;
  const entity = (config.entities ?? []).find((e) => e.table === table);
  return entity?.fields?.find((f) => f.column === column);
}

const jsonArray = (value: unknown): unknown[] => {
  if (Array.isArray(value)) return value;
  if (typeof value !== "string" || !value.trim().startsWith("[")) return [];
  try {
    const parsed: unknown = JSON.parse(value);
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
};

/** The files of an attachment value; anything but a JSON array of files is none. */
export const parseAttachments = (value: unknown): AttachmentRef[] =>
  jsonArray(value).filter(
    (file): file is AttachmentRef =>
      !!file && typeof file === "object" && typeof (file as AttachmentRef).id === "string",
  );

/** The stored value for a list of files: a JSON array, or null when empty. */
export const attachmentsValue = (files: AttachmentRef[]) =>
  files.length ? JSON.stringify(files) : null;

/** The choices of a multi-select value; a plain string is one choice. */
export function parseChoices(value: unknown): string[] {
  if (value === null || value === undefined || value === "") return [];
  const items = jsonArray(value);
  if (items.length || (typeof value === "string" && value.trim().startsWith("[")))
    return items.map(String);
  return [String(value)];
}

/** The stored value for chosen items: a JSON array, or null when none. */
export const choicesValue = (items: string[]) => (items.length ? JSON.stringify(items) : null);

/** Readable size, e.g. `12 KB` (decimal units, as the size limits are written). */
export function fileSize(bytes: number): string {
  if (bytes < 1000) return `${bytes} B`;
  if (bytes < 1_000_000) return `${Math.round(bytes / 1000)} KB`;
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}

/** Text of a formatted field's value in a list cell, datasheet, or report. */
export function fieldText(value: unknown, format: string | null | undefined): string {
  switch (format) {
    case "richText":
      return richTextPlain(value).replace(/\n/g, " ");
    case "attachment": {
      const files = parseAttachments(value);
      return files.length === 1 ? files[0].name : files.length ? `${files.length} files` : "";
    }
    case "multiSelect":
      return parseChoices(value).join(", ");
    default:
      return value === null || value === undefined ? "" : String(value);
  }
}

/**
 * Rows of a table with its formatted fields as text (rich text as plain text, files by
 * name, choices joined), the way a report shows them.
 */
export function fieldTextRows<T extends Record<string, unknown>>(
  rows: T[],
  config: Pick<DocumentConfig, "entities">,
  table: string | null | undefined,
): T[] {
  const fields = (config.entities ?? []).find((e) => e.table === table)?.fields ?? [];
  const formatted = fields.filter((f) => f.format);
  if (!formatted.length) return rows;
  return rows.map((row) => {
    const next: Record<string, unknown> = { ...row };
    for (const field of formatted)
      if (field.column in next) next[field.column] = fieldText(next[field.column], field.format);
    return next as T;
  });
}
