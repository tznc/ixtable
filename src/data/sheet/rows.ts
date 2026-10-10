import type { DataValue, DbColumn, NamedValue } from "../../lib/types";
import { logicalOf, valueFromText } from "../../schema/logical";

/**
 * The row as read, sent back as `expected` so optimistic entities detect
 * concurrent edits. Generated and blob columns are left out.
 */
export const originalValues = (columns: DbColumn[], row: DataValue[]): NamedValue[] =>
  columns
    .map((c, j) => ({ column: c.name, value: row[j] }))
    .filter((_, j) => !columns[j].generated && logicalOf(columns[j]) !== "blob");

/** Whether the datasheet lets the user type into `column`. */
export const isEditable = (column: DbColumn) => !column.generated && logicalOf(column) !== "blob";

/**
 * Typed values for `cells` (column index to text). Empty text becomes null when
 * `emptyAsNull` (pasting over a cell), and is left out otherwise (a new record
 * keeps the column default).
 */
export const typedValues = (
  columns: DbColumn[],
  cells: Map<number, string>,
  emptyAsNull: boolean,
): NamedValue[] =>
  [...cells].flatMap(([j, text]): NamedValue[] => {
    const column = columns[j];
    if (text === "") return emptyAsNull ? [{ column: column.name, value: { type: "null" } }] : [];
    return [{ column: column.name, value: valueFromText(text, column.name, logicalOf(column)) }];
  });
