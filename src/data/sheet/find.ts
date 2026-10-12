import { datasheetText } from "../../fields/datasheet";
import type { FieldSettings } from "../../fields/types";
import type { DataValue } from "../../lib/types";
import { showValue } from "../format";

export interface FindOptions {
  find: string;
  matchCase: boolean;
  /** Match the whole cell text rather than any part of it. */
  wholeField: boolean;
}

const escapeRegExp = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

function pattern(options: FindOptions, global: boolean): RegExp {
  const body = escapeRegExp(options.find);
  return new RegExp(
    options.wholeField ? `^${body}$` : body,
    `${options.matchCase ? "" : "i"}${global ? "g" : ""}`,
  );
}

/**
 * The text a datasheet cell shows (masked or formatted by its field), or null for
 * cells find skips (nulls and blobs).
 */
export const cellText = (value: DataValue, field?: FieldSettings): string | null =>
  value.type === "null" || value.type === "blob"
    ? null
    : (datasheetText(value, field) ?? showValue(value));

export function cellMatches(
  value: DataValue,
  options: FindOptions,
  field?: FieldSettings,
): boolean {
  const text = cellText(value, field);
  return !!options.find && text !== null && pattern(options, false).test(text);
}

/** `text` with every match replaced (the whole text when matching whole fields). */
export const replaceText = (text: string, replacement: string, options: FindOptions) =>
  text.replace(pattern(options, true), () => replacement);

export type CellAt = { row: number; column: number };

/**
 * The first matching cell after `after` in reading order (row by row, through
 * `columns` only), or null. Without `after` the search starts at the first cell.
 */
export function nextMatch(
  rows: DataValue[][],
  columns: number[],
  options: FindOptions,
  after: CellAt | null,
  fields: (FieldSettings | undefined)[] = [],
): CellAt | null {
  const startRow = after?.row ?? 0;
  for (let row = startRow; row < rows.length; row++) {
    for (const column of columns) {
      if (row === startRow && after && columns.indexOf(column) <= columns.indexOf(after.column))
        continue;
      if (cellMatches(rows[row][column], options, fields[column])) return { row, column };
    }
  }
  return null;
}
