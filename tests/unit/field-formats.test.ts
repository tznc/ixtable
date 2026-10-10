import { describe, expect, it } from "vitest";
import { generateCrudForms } from "../../src/design/generate";
import { newControl, newForm } from "../../src/design/schema";
import { datasheetText, datasheetValue } from "../../src/fields/datasheet";
import { withFieldDefaults } from "../../src/fields/defaults";
import { richTextPlain, sanitizeRichText } from "../../src/fields/richtext";
import {
  attachmentsValue,
  choicesValue,
  fieldSettingsFor,
  fieldText,
  fieldTextRows,
  fileSize,
  parseAttachments,
  parseChoices,
} from "../../src/fields/values";
import type { DbColumn, TableSchema } from "../../src/lib/types";
import { cellText, validateControl } from "../../src/runtime/formState";
import type { EntitySettings } from "../../src/schema/types";

const file = { id: "f1", name: "a.pdf", mime: "application/pdf", size: 2048, sha256: "x" };
const entities: EntitySettings[] = [
  {
    id: "e",
    table: "docs",
    fields: [
      { id: "1", column: "notes", format: "richText" },
      { id: "2", column: "files", format: "attachment" },
      { id: "3", column: "tags", format: "multiSelect", options: ["red", "blue"] },
      { id: "4", column: "phone", inputMask: "000-0000" },
    ],
  },
];
const col = (name: string, extra: Partial<DbColumn> = {}): DbColumn => ({
  name,
  declaredType: "TEXT",
  logicalType: "text",
  nullable: true,
  defaultValue: null,
  primaryKeyPosition: 0,
  generated: false,
  ...extra,
});
const scope = { record: {}, form: {}, app: {} };

describe("rich text", () => {
  it("keeps formatting tags and drops scripts, handlers, and unsafe links", () => {
    const html =
      '<p onclick="x()">Hi <b>bold</b> <script>alert(1)</script><a href="javascript:x">j</a> <a href="https://e.com">e</a><img src=x onerror=y></p>';
    expect(sanitizeRichText(html)).toBe(
      '<p>Hi <b>bold</b> <a>j</a> <a href="https://e.com" rel="noopener noreferrer" target="_blank">e</a></p>',
    );
    expect(sanitizeRichText('<span style="x">t</span>')).toBe("t");
  });

  it("reads as plain text with block breaks", () => {
    expect(richTextPlain("<p>One</p><ul><li>a</li><li>b</li></ul>")).toBe("One\na\nb");
    expect(richTextPlain("plain & simple")).toBe("plain & simple");
    expect(richTextPlain(null)).toBe("");
  });
});

describe("field values", () => {
  it("parses and writes attachment and choice arrays", () => {
    expect(parseAttachments(JSON.stringify([file, { name: "no id" }]))).toEqual([file]);
    expect(parseAttachments("not json")).toEqual([]);
    expect(attachmentsValue([])).toBeNull();
    expect(parseChoices('["a","b"]')).toEqual(["a", "b"]);
    expect(parseChoices("legacy")).toEqual(["legacy"]);
    expect(parseChoices("[broken")).toEqual([]);
    expect(choicesValue([])).toBeNull();
    expect(choicesValue(["a"])).toBe('["a"]');
  });

  it("shows each format as text", () => {
    expect(fieldText("<p>Hi</p><p>there</p>", "richText")).toBe("Hi there");
    expect(fieldText(JSON.stringify([file]), "attachment")).toBe("a.pdf");
    expect(fieldText(JSON.stringify([file, file]), "attachment")).toBe("2 files");
    expect(fieldText('["red","blue"]', "multiSelect")).toBe("red, blue");
    expect(fieldText(5, null)).toBe("5");
    expect(fileSize(2048)).toBe("2 KB");
    expect(fileSize(20_000_000)).toBe("20.0 MB");
  });

  it("finds field settings and turns report rows into text", () => {
    expect(fieldSettingsFor({ entities }, "docs", "phone")?.inputMask).toBe("000-0000");
    expect(fieldSettingsFor({ entities }, "other", "phone")).toBeUndefined();
    const rows = [{ notes: "<b>x</b>", tags: '["red"]', id: 1 }];
    expect(fieldTextRows(rows, { entities }, "docs")).toEqual([{ notes: "x", tags: "red", id: 1 }]);
    expect(fieldTextRows(rows, { entities }, null)).toBe(rows);
  });
});

describe("forms", () => {
  it("fills field formats, masks, and choices into a form's controls", () => {
    const form = newForm("Docs", { kind: "table", table: "docs" });
    for (const column of ["notes", "tags", "phone", "plain"]) {
      const control = newControl("text", form);
      control.binding = { column };
      form.controls.push(control);
    }
    const filled = withFieldDefaults(form, { entities });
    expect(filled.controls.map((c) => [c.kind, c.inputMask ?? null])).toEqual([
      ["richText", null],
      ["multiSelect", null],
      ["text", "000-0000"],
      ["text", null],
    ]);
    expect(filled.controls[1].options).toEqual([
      { value: "red", label: "red" },
      { value: "blue", label: "blue" },
    ]);
    expect(withFieldDefaults(form, { entities: [] })).toBe(form);
  });

  it("generates controls from field settings", () => {
    const table: TableSchema = {
      name: "docs",
      columns: [
        col("id", { declaredType: "INTEGER", logicalType: "integer", primaryKeyPosition: 1 }),
        col("notes"),
        col("files", { logicalType: "json" }),
        col("phone"),
      ],
      foreignKeys: [],
      withoutRowid: false,
    };
    const { detail } = generateCrudForms(table, { entities });
    const byColumn = Object.fromEntries(detail.controls.map((c) => [c.binding?.column, c]));
    expect(byColumn.notes.kind).toBe("richText");
    expect(byColumn.files.kind).toBe("attachment");
    expect(byColumn.files.placement.columnSpan).toBe(12);
    expect(byColumn.phone.inputMask).toBe("000-0000");
  });

  it("validates masks and treats empty rich text as blank", () => {
    const form = newForm("Docs");
    const phone = { ...newControl("text", form), label: "Phone", inputMask: "000-0000" };
    expect(validateControl(phone, "555", scope)).toBe("Phone must match ___-____.");
    expect(validateControl(phone, "5551234", scope)).toBeNull();
    const notes = {
      ...newControl("richText", form),
      label: "Notes",
      validation: { required: true },
    };
    expect(validateControl(notes, "<p> </p>", scope)).toBe("Notes is required.");
    expect(validateControl(notes, "<p>x</p>", scope)).toBeNull();
    expect(cellText('["a","b"]', { ...newControl("multiSelect", form) })).toBe("a, b");
    expect(cellText("<i>x</i>", undefined, "richText")).toBe("x");
  });
});

describe("datasheet", () => {
  it("applies the mask before the logical type and shows formatted cells as text", () => {
    const phone = entities[0].fields?.[3];
    expect(datasheetValue("555-1234", col("phone"), phone)).toEqual({
      type: "text",
      value: "5551234",
    });
    expect(() => datasheetValue("12", col("phone"), phone)).toThrow("phone must match");
    expect(datasheetValue("NULL", col("phone"), phone)).toEqual({ type: "null" });
    expect(datasheetText({ type: "text", value: "5551234" }, phone)).toBe("555-1234");
    expect(datasheetText({ type: "text", value: '["red"]' }, entities[0].fields?.[2])).toBe("red");
    expect(datasheetText({ type: "text", value: "x" })).toBeNull();
  });
});
