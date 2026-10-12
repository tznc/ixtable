/** Field formats layered on a column's logical type (`docs/decisions/field-formats.md`). */
export type FieldFormat = "richText" | "attachment" | "multiSelect";

export const FIELD_FORMATS: Array<{ value: FieldFormat; label: string; types: string[] }> = [
  { value: "richText", label: "Rich text", types: ["text"] },
  { value: "attachment", label: "Attachments", types: ["text", "json"] },
  { value: "multiSelect", label: "Multiple choices", types: ["text", "json"] },
];

/** Per-column settings, stored on the table's entity settings and keyed by column name. */
export interface FieldSettings {
  id: string;
  column: string;
  format?: FieldFormat | (string & {}) | null;
  /** Access-style input mask (see `mask.ts`); applies to entry only. */
  inputMask?: string | null;
  /** Choices of a multi-select field. */
  options?: string[];
}

/** One file of an attachment field, as stored in the field's JSON array. */
export interface AttachmentRef {
  id: string;
  name: string;
  mime: string;
  size: number;
  sha256: string;
}
