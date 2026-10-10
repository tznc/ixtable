import { describe, expect, it } from "vitest";
import {
  filterItems,
  itemKey,
  openCount,
  readMigration,
  toCsv,
  toMarkdown,
  withReviewed,
} from "../../src/access/migration";

const settings = {
  theme: "keep",
  accessImport: {
    source: "Orders.accdb",
    format: "ace",
    importedAt: "2026-10-10T12:00:00Z",
    warnings: ["page 9 could not be read"],
    report: [
      { kind: "table", name: "Orders", status: "converted", notes: [] },
      {
        kind: "form",
        name: "Order Entry",
        status: "partial",
        notes: ["the On Load VBA procedure does not run"],
        target: { kind: "form", id: "f1" },
      },
      {
        kind: "query",
        name: 'Pass, "Through"',
        status: "skipped",
        notes: ["pass-through | server", "Access SQL: SELECT 1"],
      },
      { kind: "macro", name: "AutoExec", status: "partial", notes: ["Beep was left out"] },
      { kind: "bogus" },
    ],
  },
};

const all = { status: "all", kind: "", search: "", hideReviewed: false } as const;

describe("Access migration report", () => {
  it("reads the report from settings and ignores anything malformed", () => {
    const m = readMigration(settings)!;
    expect(m.source).toBe("Orders.accdb");
    expect(m.report).toHaveLength(4);
    expect(m.reviewed).toEqual([]);
    expect(readMigration({})).toBeNull();
    expect(readMigration(null)).toBeNull();
    expect(readMigration({ accessImport: { report: "x" } })).toBeNull();
  });

  it("filters worst first and searches names and notes", () => {
    const m = readMigration(settings)!;
    expect(filterItems(m, { ...all, status: "attention" }).map((i) => i.name)).toEqual([
      'Pass, "Through"',
      "Order Entry",
      "AutoExec",
    ]);
    expect(filterItems(m, { ...all, kind: "form" }).map((i) => i.name)).toEqual(["Order Entry"]);
    expect(filterItems(m, { ...all, search: "beep" }).map((i) => i.name)).toEqual(["AutoExec"]);
    expect(filterItems(m, { ...all, status: "converted" }).map((i) => i.name)).toEqual(["Orders"]);
  });

  it("marks items reviewed without touching other settings", () => {
    const key = itemKey({ kind: "macro", name: "AutoExec" });
    const next = withReviewed(settings, key, true) as typeof settings & {
      accessImport: { reviewed: string[] };
    };
    expect(next.theme).toBe("keep");
    expect(next.accessImport.reviewed).toEqual(["macro:AutoExec"]);
    const m = readMigration(next)!;
    expect(openCount(m)).toBe(2);
    expect(filterItems(m, { ...all, status: "attention", hideReviewed: true })).toHaveLength(2);
    const cleared = readMigration(withReviewed(next, key, false))!;
    expect(cleared.reviewed).toEqual([]);
  });

  it("renders Markdown and CSV", () => {
    const m = readMigration(withReviewed(settings, "form:Order Entry", true))!;
    const md = toMarkdown(m, "Order Desk");
    expect(md).toContain("# Access migration report: Order Desk");
    expect(md).toContain("| Forms | 0 | 1 | 0 |");
    expect(md).toContain("### Order Entry (form): Partly converted (reviewed)");
    expect(md).toContain("- pass-through \\| server");
    expect(md).toContain("## Reading problems");
    const csv = toCsv(m).split("\r\n");
    expect(csv[0]).toBe("Kind,Name,Status,Reviewed,Note");
    expect(csv).toContain('query,"Pass, ""Through""",Not converted,no,pass-through | server');
    expect(csv).toContain(
      "form,Order Entry,Partly converted,yes,the On Load VBA procedure does not run",
    );
    expect(csv).toContain("table,Orders,Converted,no,");
  });
});
