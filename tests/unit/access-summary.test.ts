import { describe, expect, it } from "vitest";
import {
  itemsNeedingAttention,
  kindLabel,
  statusCounts,
  totalRows,
} from "../../src/access/summary";
import type { AccessImportReport, AccessInventory } from "../../src/access/types";

const inventory: AccessInventory = {
  format: "ace",
  tables: [
    { name: "Customers", columns: 4, rows: 3 },
    { name: "Orders", columns: 6, rows: 10 },
    { name: "Linked", columns: 2, rows: null },
  ],
  relationships: 1,
  queries: [],
  forms: [],
  reports: [],
  macros: [],
  modules: [],
  compiled: [],
  warnings: [],
};

const report: AccessImportReport = {
  tables: 2,
  rows: 13,
  warnings: [],
  items: [
    { kind: "module", name: "Helpers", status: "skipped", notes: ["VBA"] },
    { kind: "form", name: "Orders", status: "partial", notes: ["button left out"] },
    { kind: "table", name: "Orders", status: "converted", notes: [] },
    { kind: "form", name: "Customers", status: "converted", notes: [] },
    { kind: "query", name: "Totals", status: "skipped", notes: ["action query"] },
    { kind: "form", name: "Contacts", status: "partial", notes: ["unbound box"] },
  ],
};

describe("Access import summary", () => {
  it("adds up rows and ignores tables whose rows are unknown", () => {
    expect(totalRows(inventory)).toBe(13);
  });

  it("counts statuses per kind in report order", () => {
    expect(statusCounts(report)).toEqual([
      { kind: "table", converted: 1, partial: 0, skipped: 0 },
      { kind: "query", converted: 0, partial: 0, skipped: 1 },
      { kind: "form", converted: 1, partial: 2, skipped: 0 },
      { kind: "module", converted: 0, partial: 0, skipped: 1 },
    ]);
  });

  it("lists skipped items before partial ones, then by kind and name", () => {
    expect(itemsNeedingAttention(report).map((i) => `${i.kind}:${i.name}`)).toEqual([
      "query:Totals",
      "module:Helpers",
      "form:Contacts",
      "form:Orders",
    ]);
  });

  it("labels kinds in the plural and passes unknown kinds through", () => {
    expect(kindLabel("macro")).toBe("Macros");
    expect(kindLabel("other")).toBe("other");
  });
});
