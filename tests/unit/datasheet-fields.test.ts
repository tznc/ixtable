import { describe, expect, it } from "vitest";
import { cellMatches, cellText, nextMatch } from "../../src/data/sheet/find";
import { pasteWrites, planPaste } from "../../src/data/sheet/paste";
import { isEditable, typedValues } from "../../src/data/sheet/rows";
import type { FieldSettings } from "../../src/fields/types";
import type { DataValue, DbColumn, DbPage } from "../../src/lib/types";

const col = (name: string, logicalType: string): DbColumn => ({
  name,
  declaredType: logicalType.toUpperCase(),
  nullable: true,
  defaultValue: null,
  primaryKeyPosition: null,
  generated: false,
  logicalType,
});
const text = (value: string): DataValue => ({ type: "text", value });
const int = (value: number): DataValue => ({ type: "integer", value });
const phone: FieldSettings = { id: "f1", column: "phone", inputMask: "000-0000" };
const tags: FieldSettings = { id: "f2", column: "tags", format: "multiSelect" };
const opts = (find: string) => ({ find, matchCase: false, wholeField: false });

describe("datasheet field settings", () => {
  const columns = [col("id", "integer"), col("phone", "text"), col("tags", "text")];
  const fields = [undefined, phone, tags];

  it("leaves formatted fields to forms", () => {
    expect(isEditable(columns[1], phone)).toBe(true);
    expect(isEditable(columns[2], tags)).toBe(false);
    expect(isEditable(columns[2])).toBe(true);
  });

  it("applies input masks before the logical type", () => {
    const cells = new Map([[1, "555-1234"]]);
    expect(typedValues(columns, cells, false, fields)).toEqual([
      { column: "phone", value: text("5551234") },
    ]);
    expect(() => typedValues(columns, new Map([[1, "55"]]), false, fields)).toThrow(
      /phone must match/,
    );
  });

  it("pastes through masks and names the bad row", () => {
    const page: DbPage = {
      columns,
      rows: [[int(1), text("5550000"), text('["a"]')]],
      identities: [[int(1)]],
      total: 1,
      offset: 0,
      limit: 100,
    };
    const plan = planPaste([["555-1234"], ["12"]], [1], { row: 0, column: 1 }, 1);
    expect(() => pasteWrites("people", page, plan, fields)).toThrow(/Pasted row 2:/);
    const [write] = pasteWrites(
      "people",
      page,
      planPaste([["555-1234"]], [1], { row: 0, column: 1 }, 1),
      fields,
    );
    expect(write.values).toEqual([{ column: "phone", value: text("5551234") }]);
    expect(write.meta?.expected).toHaveLength(3);
  });

  it("finds the masked and formatted text a cell shows", () => {
    expect(cellText(text("5551234"), phone)).toBe("555-1234");
    expect(cellText(text('["red","blue"]'), tags)).toBe("red, blue");
    expect(cellMatches(text("5551234"), opts("5-12"), phone)).toBe(true);
    const rows = [[int(1), text("5551234"), text('["red","blue"]')]];
    expect(nextMatch(rows, [0, 1, 2], opts("red, b"), null, fields)).toEqual({
      row: 0,
      column: 2,
    });
    expect(nextMatch(rows, [0, 1, 2], opts("red, b"), null)).toBeNull();
  });
});
