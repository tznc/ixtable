import type { DataValue, DbColumn, NamedValue } from "../../lib/types";
import { datasheetValue } from "../../fields/datasheet";
import type { FieldSettings } from "../../fields/types";
import { logicalOf } from "../../schema/logical";

/** Field settings per column index of a page (masks and formats), from the document config. */
export type SheetFields = (FieldSettings | undefined)[];

/**
 * The row as read, sent back as `expected` so optimistic entities detect
 * concurrent edits. Generated and blob columns are left out.
 */
export const originalValues = (columns: DbColumn[], row: DataValue[]): NamedValue[] =>
  columns
    .map((c, j) => ({ column: c.name, value: row[j] }))
    .filter((_, j) => !columns[j].generated && logicalOf(columns[j]) !== "blob");

/**
 * Whether the datasheet lets the user type into `column`. Formatted fields (rich
 * text, attachments, multi-select) are edited in a form.
 */
export const isEditable = (column: DbColumn, field?: FieldSettings) =>
  !column.generated && logicalOf(column) !== "blob" && !field?.format;

/**
 * Typed values for `cells` (column index to text). Empty text becomes null when
 * `emptyAsNull` (pasting over a cell), and is left out otherwise (a new record
 * keeps the column default). A column's input mask applies before its type.
 */
export const typedValues = (
  columns: DbColumn[],
  cells: Map<number, string>,
  emptyAsNull: boolean,
  fields: SheetFields = [],
): NamedValue[] =>
  [...cells].flatMap(([j, text]): NamedValue[] => {
    const column = columns[j];
    if (text === "") return emptyAsNull ? [{ column: column.name, value: { type: "null" } }] : [];
    return [{ column: column.name, value: datasheetValue(text, column, fields[j]) }];
  });
