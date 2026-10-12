import { describe, expect, it } from "vitest";
import {
  cellMatches,
  cellText,
  nextMatch,
  replaceText,
  type FindOptions,
} from "../../src/data/sheet/find";
import { addFilter, describeFilter, selectionFilter } from "../../src/data/sheet/filters";
import {
  EMPTY_LAYOUT,
  freezeThrough,
  hideColumn,
  readLayout,
  visibleColumns,
  writeLayout,
  type DatasheetLayout,
} from "../../src/data/sheet/layout";
import {
  isMultiCell,
  parseClipboardGrid,
  pasteWrites,
  pastedMessage,
  planPaste,
} from "../../src/data/sheet/paste";
import { nextSorts } from "../../src/data/sheet/sorts";
import { totalsFor } from "../../src/data/sheet/totals";
import type { DataValue, DbColumn, DbPage, Filter } from "../../src/lib/types";

const col = (name: string, logicalType: string, extra: Partial<DbColumn> = {}): DbColumn => ({
  name,
  declaredType: logicalType.toUpperCase(),
  nullable: true,
  defaultValue: null,
  primaryKeyPosition: null,
  generated: false,
  logicalType,
  ...extra,
});

const text = (value: string): DataValue => ({ type: "text", value });
const int = (value: number): DataValue => ({ type: "integer", value });
const nil: DataValue = { type: "null" };
const blob: DataValue = { type: "blob", value: "AAAA" };

const opts = (find: string, extra: Partial<FindOptions> = {}): FindOptions => ({
  find,
  matchCase: false,
  wholeField: false,
  ...extra,
});

describe("parseClipboardGrid", () => {
  it("parses tab separated rows with LF", () => {
    expect(parseClipboardGrid("a\tb\nc\td")).toEqual([
      ["a", "b"],
      ["c", "d"],
    ]);
  });
  it("handles Excel CRLF and a trailing line end", () => {
    expect(parseClipboardGrid("a\tb\r\nc\td\r\n")).toEqual([
      ["a", "b"],
      ["c", "d"],
    ]);
  });
  it("adds no empty row for a trailing newline", () => {
    expect(parseClipboardGrid("a\n")).toEqual([["a"]]);
  });
  it("returns no rows for empty text", () => {
    expect(parseClipboardGrid("")).toEqual([]);
  });
  it("keeps empty cells", () => {
    expect(parseClipboardGrid("a\t\tc\n\tb\t")).toEqual([
      ["a", "", "c"],
      ["", "b", ""],
    ]);
  });
  it("reads quoted fields with tabs, newlines and doubled quotes", () => {
    expect(parseClipboardGrid('"a\tb"\t"line1\nline2"\t"say ""hi"""\r\nx\ty\tz\r\n')).toEqual([
      ["a\tb", "line1\nline2", 'say "hi"'],
      ["x", "y", "z"],
    ]);
  });
  it("treats a quote inside an unquoted field literally", () => {
    expect(parseClipboardGrid('ab"c\td')).toEqual([['ab"c', "d"]]);
  });
});

describe("isMultiCell", () => {
  it("is false for a single value", () => {
    expect(isMultiCell("hello")).toBe(false);
  });
  it("is false for a single value with trailing newlines", () => {
    expect(isMultiCell("hello\n")).toBe(false);
    expect(isMultiCell("hello\r\n")).toBe(false);
  });
  it("is true for tabs and for several lines", () => {
    expect(isMultiCell("a\tb")).toBe(true);
    expect(isMultiCell("a\nb")).toBe(true);
    expect(isMultiCell("a\r\nb\r\n")).toBe(true);
  });
});

describe("planPaste", () => {
  const grid = [
    ["a", "b", "c"],
    ["d", "e", "f"],
  ];

  it("maps cells onto the given column order, skipping columns not listed", () => {
    const plan = planPaste(grid, [0, 2, 3, 5], { row: 1, column: 2 }, 10);
    expect(plan.inserts).toEqual([]);
    expect(plan.updates.map((u) => u.row)).toEqual([1, 2]);
    expect([...plan.updates[0].cells]).toEqual([
      [2, "a"],
      [3, "b"],
      [5, "c"],
    ]);
  });
  it("drops cells past the last column", () => {
    const plan = planPaste(grid, [0, 1], { row: 0, column: 1 }, 5);
    expect([...plan.updates[0].cells]).toEqual([[1, "a"]]);
    expect([...plan.updates[1].cells]).toEqual([[1, "d"]]);
  });
  it("turns rows past rowCount into inserts", () => {
    const plan = planPaste(grid, [0, 1, 2], { row: 2, column: 0 }, 3);
    expect(plan.updates.map((u) => u.row)).toEqual([2]);
    expect(plan.inserts).toHaveLength(1);
    expect([...plan.inserts[0]]).toEqual([
      [0, "d"],
      [1, "e"],
      [2, "f"],
    ]);
  });
  it("inserts every row when the start is the new-record row", () => {
    const plan = planPaste(grid, [0, 1, 2], { row: 4, column: 0 }, 4);
    expect(plan.updates).toEqual([]);
    expect(plan.inserts).toHaveLength(2);
  });
  it("returns an empty plan when the start column is not editable", () => {
    expect(planPaste(grid, [0, 1], { row: 0, column: 9 }, 5)).toEqual({
      updates: [],
      inserts: [],
    });
  });
});

describe("pasteWrites", () => {
  const columns = [
    col("id", "integer", { primaryKeyPosition: 1 }),
    col("qty", "integer"),
    col("note", "text"),
  ];
  const page: DbPage = {
    columns,
    rows: [
      [int(1), int(5), text("old")],
      [int(2), int(6), nil],
    ],
    identities: [[int(1)], [int(2)]],
    total: 2,
    offset: 0,
    limit: 100,
  };

  it("builds updates with expected values and null for empty text", () => {
    const plan = planPaste([["9", ""]], [1, 2], { row: 0, column: 1 }, 2);
    const [write] = pasteWrites("items", page, plan);
    expect(write.operation).toBe("update");
    expect(write.table).toBe("items");
    expect(write.identity).toEqual([int(1)]);
    expect(write.values).toEqual([
      { column: "qty", value: int(9) },
      { column: "note", value: nil },
    ]);
    expect(write.meta?.expected).toEqual([
      { column: "id", value: int(1) },
      { column: "qty", value: int(5) },
      { column: "note", value: text("old") },
    ]);
  });
  it("builds inserts that omit empty text", () => {
    const plan = planPaste([["", "hi"]], [1, 2], { row: 2, column: 1 }, 2);
    const [write] = pasteWrites("items", page, plan);
    expect(write.operation).toBe("insert");
    expect(write.identity).toBeNull();
    expect(write.values).toEqual([{ column: "note", value: text("hi") }]);
    expect(write.meta).toBeUndefined();
  });
  it("lists updates before inserts", () => {
    const plan = planPaste([["1"], ["2"], ["3"]], [1], { row: 1, column: 1 }, 2);
    expect(pasteWrites("items", page, plan).map((w) => w.operation)).toEqual([
      "update",
      "insert",
      "insert",
    ]);
  });
  it("throws naming the pasted row when an integer column gets text", () => {
    const plan = planPaste([["1"], ["abc"]], [1], { row: 0, column: 1 }, 2);
    expect(() => pasteWrites("items", page, plan)).toThrow(Error);
    expect(() => pasteWrites("items", page, plan)).toThrow(/Pasted row 2: qty requires an integer/);
  });
  it("numbers inserts after the updates in errors", () => {
    const plan = planPaste([["1"], ["x"]], [1], { row: 1, column: 1 }, 2);
    expect(() => pasteWrites("items", page, plan)).toThrow(/Pasted row 2:/);
  });
});

describe("pastedMessage", () => {
  it("describes updates, additions and both", () => {
    expect(pastedMessage(1, 0)).toBe("Pasted into 1 record.");
    expect(pastedMessage(3, 0)).toBe("Pasted into 3 records.");
    expect(pastedMessage(0, 1)).toBe("Added 1 record.");
    expect(pastedMessage(0, 4)).toBe("Added 4 records.");
    expect(pastedMessage(2, 1)).toBe("Pasted into 2 records and added 1 record.");
  });
});

describe("find", () => {
  it("cellText skips nulls and blobs and shows other values", () => {
    expect(cellText(nil)).toBeNull();
    expect(cellText(blob)).toBeNull();
    expect(cellText(int(42))).toBe("42");
    expect(cellText(text("abc"))).toBe("abc");
  });
  it("matches substrings case-insensitively by default", () => {
    expect(cellMatches(text("Hello World"), opts("lo wo"))).toBe(true);
    expect(cellMatches(text("Hello World"), opts("xyz"))).toBe(false);
  });
  it("honours match case", () => {
    expect(cellMatches(text("Hello"), opts("hello", { matchCase: true }))).toBe(false);
    expect(cellMatches(text("Hello"), opts("Hello", { matchCase: true }))).toBe(true);
  });
  it("honours whole field", () => {
    expect(cellMatches(text("Hello"), opts("ell", { wholeField: true }))).toBe(false);
    expect(cellMatches(text("Hello"), opts("hello", { wholeField: true }))).toBe(true);
  });
  it("matches numbers by their shown text", () => {
    expect(cellMatches(int(1234), opts("23"))).toBe(true);
  });
  it("never matches an empty search", () => {
    expect(cellMatches(text("abc"), opts(""))).toBe(false);
  });
  it("treats regex special characters literally", () => {
    expect(cellMatches(text("a.c"), opts("a.c"))).toBe(true);
    expect(cellMatches(text("abc"), opts("a.c"))).toBe(false);
    expect(cellMatches(text("price (usd)"), opts("(usd)"))).toBe(true);
    expect(cellMatches(text("a+b"), opts("a+b"))).toBe(true);
    expect(cellMatches(text("aab"), opts("a+b"))).toBe(false);
    expect(cellMatches(text("[x]"), opts("[x]", { wholeField: true }))).toBe(true);
    expect(cellMatches(text("back\\slash"), opts("\\"))).toBe(true);
  });
  it("skips nulls and blobs", () => {
    expect(cellMatches(nil, opts("null"))).toBe(false);
    expect(cellMatches(blob, opts("blob"))).toBe(false);
  });

  it("replaces every match", () => {
    expect(replaceText("a.b.c", "-", opts("."))).toBe("a-b-c");
    expect(replaceText("Foo foo FOO", "x", opts("foo"))).toBe("x x x");
    expect(replaceText("Foo foo", "x", opts("foo", { matchCase: true }))).toBe("Foo x");
  });
  it("replaces the whole text when matching whole fields", () => {
    expect(replaceText("abc", "z", opts("ABC", { wholeField: true }))).toBe("z");
    expect(replaceText("abcd", "z", opts("abc", { wholeField: true }))).toBe("abcd");
  });
  it("inserts replacements literally", () => {
    expect(replaceText("cost", "$&$1", opts("cost"))).toBe("$&$1");
  });

  describe("nextMatch", () => {
    const rows: DataValue[][] = [
      [text("apple"), text("pear"), nil],
      [text("grape"), blob, text("apple pie")],
      [text("plum"), text("fig"), text("Apple")],
    ];
    const all = [0, 1, 2];

    it("starts at the first cell without a position", () => {
      expect(nextMatch(rows, all, opts("apple"), null)).toEqual({ row: 0, column: 0 });
    });
    it("starts after the given cell in reading order", () => {
      expect(nextMatch(rows, all, opts("apple"), { row: 0, column: 0 })).toEqual({
        row: 1,
        column: 2,
      });
      expect(nextMatch(rows, all, opts("apple"), { row: 1, column: 2 })).toEqual({
        row: 2,
        column: 2,
      });
    });
    it("returns null after the last match", () => {
      expect(nextMatch(rows, all, opts("apple"), { row: 2, column: 2 })).toBeNull();
    });
    it("returns null when nothing matches", () => {
      expect(nextMatch(rows, all, opts("zzz"), null)).toBeNull();
    });
    it("searches only the given columns", () => {
      expect(nextMatch(rows, [0, 1], opts("apple"), { row: 0, column: 0 })).toBeNull();
      expect(nextMatch(rows, [2], opts("apple"), null)).toEqual({ row: 1, column: 2 });
    });
    it("respects the given column order", () => {
      expect(nextMatch(rows, [2, 0], opts("apple"), { row: 1, column: 2 })).toEqual({
        row: 2,
        column: 2,
      });
    });
    it("skips nulls and blobs", () => {
      expect(nextMatch(rows, all, opts("blob"), null)).toBeNull();
      expect(nextMatch(rows, all, opts("null"), null)).toBeNull();
    });
  });
});

describe("filters", () => {
  it("builds eq and ne filters for values", () => {
    expect(selectionFilter("Region", text("North"))).toEqual({
      column: "Region",
      operator: "eq",
      value: text("North"),
    });
    expect(selectionFilter("Region", text("North"), true)).toEqual({
      column: "Region",
      operator: "ne",
      value: text("North"),
    });
  });
  it("filters nulls on nullness", () => {
    expect(selectionFilter("Region", nil)).toEqual({ column: "Region", operator: "is_null" });
    expect(selectionFilter("Region", nil, true)).toEqual({
      column: "Region",
      operator: "is_not_null",
    });
  });
  it("gives no filter for blobs", () => {
    expect(selectionFilter("Photo", blob)).toBeNull();
    expect(selectionFilter("Photo", blob, true)).toBeNull();
  });

  const eq = (column: string, value: string): Filter => ({
    column,
    operator: "eq",
    value: text(value),
  });

  it("appends a filter on a new column or operator", () => {
    const base = [eq("a", "1")];
    expect(addFilter(base, eq("b", "2"))).toEqual([eq("a", "1"), eq("b", "2")]);
    const ne: Filter = { column: "a", operator: "ne", value: text("1") };
    expect(addFilter(base, ne)).toEqual([eq("a", "1"), ne]);
  });
  it("replaces the same column and operator and moves it last", () => {
    const result = addFilter([eq("a", "1"), eq("b", "2")], eq("a", "3"));
    expect(result).toEqual([eq("b", "2"), eq("a", "3")]);
  });
  it("does not mutate its input", () => {
    const base = [eq("a", "1")];
    addFilter(base, eq("a", "2"));
    expect(base).toEqual([eq("a", "1")]);
  });

  it("describes filters", () => {
    expect(describeFilter(eq("Region", "North"))).toBe("Region = North");
    expect(describeFilter({ column: "Region", operator: "ne", value: text("North") })).toBe(
      "Region ≠ North",
    );
    expect(describeFilter({ column: "Name", operator: "contains", value: text("x") })).toBe(
      "Name contains x",
    );
    expect(describeFilter({ column: "Qty", operator: "gt", value: int(3) })).toBe("Qty gt 3");
    expect(describeFilter({ column: "Region", operator: "is_null" })).toBe("Region is empty");
    expect(describeFilter({ column: "Region", operator: "is_not_null" })).toBe(
      "Region is not empty",
    );
  });
});

describe("layout", () => {
  const columns = [col("a", "text"), col("b", "text"), col("c", "text"), col("d", "text")];
  const layout = (extra: Partial<DatasheetLayout> = {}): DatasheetLayout => ({
    ...EMPTY_LAYOUT,
    ...extra,
  });

  it("reads defaults when nothing is saved", () => {
    expect(readLayout({ navigationState: null }, "t")).toEqual(EMPTY_LAYOUT);
    expect(readLayout({ navigationState: {} }, "t")).toEqual(EMPTY_LAYOUT);
  });
  it("sanitises malformed saved values", () => {
    const config = {
      navigationState: {
        datasheetLayouts: { t: { hidden: "x", frozen: -2, totals: "sum" } },
      },
    };
    expect(readLayout(config, "t")).toEqual(EMPTY_LAYOUT);
  });
  it("round-trips through writeLayout", () => {
    const saved = layout({ hidden: ["b"], frozen: 2, totals: { a: "count" } });
    const config = writeLayout({ navigationState: null }, "t", saved);
    expect(readLayout(config, "t")).toEqual(saved);
    expect(readLayout(config, "other")).toEqual(EMPTY_LAYOUT);
  });
  it("keeps other tables and other navigation keys", () => {
    const start = { navigationState: { tab: "data", datasheetLayouts: { u: { frozen: 1 } } } };
    const config = writeLayout(start, "t", layout({ frozen: 3 }));
    expect(config.navigationState).toMatchObject({ tab: "data" });
    expect(readLayout(config, "u").frozen).toBe(1);
    expect(readLayout(config, "t").frozen).toBe(3);
  });
  it("removes the table entry for an empty layout", () => {
    const start = writeLayout({ navigationState: { tab: "data" } }, "t", layout({ frozen: 1 }));
    const config = writeLayout(start, "t", EMPTY_LAYOUT);
    const layouts = (config.navigationState as { datasheetLayouts: Record<string, unknown> })
      .datasheetLayouts;
    expect("t" in layouts).toBe(false);
    expect(config.navigationState).toMatchObject({ tab: "data" });
  });
  it("does not mutate the input config", () => {
    const start = { navigationState: { datasheetLayouts: {} } };
    writeLayout(start, "t", layout({ frozen: 1 }));
    expect(start.navigationState.datasheetLayouts).toEqual({});
  });

  it("lists visible column indexes in table order", () => {
    expect(visibleColumns(columns, layout())).toEqual([0, 1, 2, 3]);
    expect(visibleColumns(columns, layout({ hidden: ["b", "d"] }))).toEqual([0, 2]);
  });

  describe("hideColumn", () => {
    it("hides a column", () => {
      expect(hideColumn(layout(), columns, "c").hidden).toEqual(["c"]);
    });
    it("lowers the frozen count when a frozen column is hidden", () => {
      expect(hideColumn(layout({ frozen: 2 }), columns, "a").frozen).toBe(1);
      expect(hideColumn(layout({ frozen: 2 }), columns, "b").frozen).toBe(1);
    });
    it("keeps the frozen count when hiding a column past the frozen ones", () => {
      expect(hideColumn(layout({ frozen: 2 }), columns, "c").frozen).toBe(2);
    });
    it("counts frozen columns among visible ones only", () => {
      const result = hideColumn(layout({ hidden: ["a"], frozen: 1 }), columns, "c");
      expect(result.frozen).toBe(1);
      expect(result.hidden).toEqual(["a", "c"]);
    });
    it("refuses to hide the last visible column", () => {
      const only = layout({ hidden: ["a", "b", "c"] });
      expect(hideColumn(only, columns, "d")).toBe(only);
    });
    it("ignores columns that are unknown or already hidden", () => {
      const l = layout({ hidden: ["a"] });
      expect(hideColumn(l, columns, "zzz")).toBe(l);
      expect(hideColumn(l, columns, "a")).toBe(l);
    });
  });

  describe("freezeThrough", () => {
    it("freezes up to and including the column", () => {
      expect(freezeThrough(layout(), columns, "c").frozen).toBe(3);
    });
    it("counts visible columns only", () => {
      expect(freezeThrough(layout({ hidden: ["a"] }), columns, "c").frozen).toBe(2);
    });
    it("ignores hidden or unknown columns", () => {
      const l = layout({ hidden: ["a"], frozen: 1 });
      expect(freezeThrough(l, columns, "a")).toBe(l);
      expect(freezeThrough(l, columns, "zzz")).toBe(l);
    });
  });
});

describe("nextSorts", () => {
  it("sorts ascending by a new column, replacing existing sorts", () => {
    expect(nextSorts([], "a", false)).toEqual([{ column: "a", descending: false }]);
    expect(nextSorts([{ column: "b", descending: true }], "a", false)).toEqual([
      { column: "a", descending: false },
    ]);
  });
  it("flips the primary sort on repeated clicks", () => {
    expect(nextSorts([{ column: "a", descending: false }], "a", false)).toEqual([
      { column: "a", descending: true },
    ]);
    expect(nextSorts([{ column: "a", descending: true }], "a", false)).toEqual([
      { column: "a", descending: false },
    ]);
  });
  it("collapses to an ascending single sort when clicking a secondary key", () => {
    const sorts = [
      { column: "a", descending: false },
      { column: "b", descending: true },
    ];
    expect(nextSorts(sorts, "b", false)).toEqual([{ column: "b", descending: false }]);
  });
  it("appends a new key with shift", () => {
    const sorts = [{ column: "a", descending: true }];
    expect(nextSorts(sorts, "b", true)).toEqual([
      { column: "a", descending: true },
      { column: "b", descending: false },
    ]);
  });
  it("flips an existing key in place with shift", () => {
    const sorts = [
      { column: "a", descending: false },
      { column: "b", descending: false },
    ];
    expect(nextSorts(sorts, "a", true)).toEqual([
      { column: "a", descending: true },
      { column: "b", descending: false },
    ]);
    expect(nextSorts(sorts, "b", true)).toEqual([
      { column: "a", descending: false },
      { column: "b", descending: true },
    ]);
  });
  it("does not mutate its input", () => {
    const sorts = [{ column: "a", descending: false }];
    nextSorts(sorts, "a", true);
    nextSorts(sorts, "a", false);
    expect(sorts).toEqual([{ column: "a", descending: false }]);
  });
});

describe("totalsFor", () => {
  it("offers every aggregate for numeric columns", () => {
    const all = ["sum", "avg", "count", "min", "max", "stdev", "var"];
    expect(totalsFor(col("n", "integer"))).toEqual(all);
    expect(totalsFor(col("n", "real"))).toEqual(all);
    expect(totalsFor(col("n", "decimal(10,2)"))).toEqual(all);
  });
  it("offers only count for blob, json and boolean", () => {
    expect(totalsFor(col("n", "blob"))).toEqual(["count"]);
    expect(totalsFor(col("n", "json"))).toEqual(["count"]);
    expect(totalsFor(col("n", "boolean"))).toEqual(["count"]);
  });
  it("offers count, min and max for text and dates", () => {
    expect(totalsFor(col("n", "text"))).toEqual(["count", "min", "max"]);
    expect(totalsFor(col("n", "date"))).toEqual(["count", "min", "max"]);
    expect(totalsFor(col("n", "timestamp"))).toEqual(["count", "min", "max"]);
  });
});
