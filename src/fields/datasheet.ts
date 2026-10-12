import type { DataValue, DbColumn } from "../lib/types";
import { logicalOf, valueFromText } from "../schema/logical";
import { applyMask, maskedText, parseMask } from "./mask";
import type { FieldSettings } from "./types";
import { fieldText } from "./values";

/** A datasheet cell's typed value: the input mask applied first, then the logical type. */
export function datasheetValue(text: string, column: DbColumn, field?: FieldSettings): DataValue {
  const masked =
    field?.inputMask && text !== "NULL" ? maskedText(field.inputMask, text, column.name) : text;
  return valueFromText(masked, column.name, logicalOf(column));
}

/** A cell's text for a formatted field (read-only; edited in a form) or a masked one, else null. */
export function datasheetText(value: DataValue, field?: FieldSettings): string | null {
  if (field?.format) return fieldText(value.type === "null" ? null : value.value, field.format);
  if (field?.inputMask && value.type !== "null")
    return applyMask(parseMask(field.inputMask), String(value.value ?? "")).display;
  return null;
}
