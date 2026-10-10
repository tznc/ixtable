import { describe, expect, it } from "vitest";
import {
  defaultExportName,
  exportedMessage,
  safeFileStem,
  withExtension,
} from "../../src/export/names";
import { firstColumnFilter } from "../../src/export/filters";

describe("export file names", () => {
  it("strips characters invalid on any platform", () => {
    expect(safeFileStem('a<b>c:d"e/f\\g|h?i*j')).toBe("abcdefghij");
    expect(safeFileStem("tab\u0000le\u001f\n")).toBe("table");
  });
  it("trims and falls back to export", () => {
    expect(safeFileStem("  Orders 2026. ")).toBe("Orders 2026");
    expect(safeFileStem("")).toBe("export");
    expect(safeFileStem(' /:*?"<>| ')).toBe("export");
    expect(safeFileStem("...")).toBe("export");
  });
  it("appends the extension unless present", () => {
    expect(withExtension("out", "csv")).toBe("out.csv");
    expect(withExtension("out.CSV", "csv")).toBe("out.CSV");
    expect(withExtension("out.csv", "json")).toBe("out.csv.json");
    expect(defaultExportName("Sales/Q1", "xlsx")).toBe("SalesQ1.xlsx");
  });
  it("formats the row count", () => {
    expect(exportedMessage(1234)).toBe("Exported 1,234 rows");
    expect(exportedMessage(1)).toBe("Exported 1 row");
    expect(exportedMessage(0)).toBe("Exported 0 rows");
  });
  it("builds the first-column contains filter", () => {
    expect(firstColumnFilter("name", "")).toEqual([]);
    expect(firstColumnFilter(undefined, "x")).toEqual([]);
    expect(firstColumnFilter("name", "x")).toEqual([
      { column: "name", operator: "contains", value: { type: "text", value: "x" } },
    ]);
  });
});
