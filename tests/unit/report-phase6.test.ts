import { describe, expect, it } from "vitest";
import { layoutReport, type PositionedItem, type ReportDocument } from "../../src/reports/engine";
import { arcOps, parsePath, wedgePath } from "../../src/reports/engine/chart-paths";
import { RUNNING_NOT_NUMBER } from "../../src/reports/engine/values";
import { writePdf } from "../../src/reports/pdf";
import type {
  CalculatedComponent,
  ChartComponent,
  FieldComponent,
  ReportGroup,
} from "../../src/reports/types";
import { band, baseReport, orders } from "./report-fixtures";

type Text = Extract<PositionedItem, { kind: "text" }>;
type Chart = Extract<PositionedItem, { kind: "chart" }>;

const items = (doc: ReportDocument) => doc.pages.flatMap((p) => p.items);
const textsOf = (doc: ReportDocument, id: string) =>
  items(doc).filter((i): i is Text => i.kind === "text" && i.componentId === id);
const chartOf = (doc: ReportDocument, id = "ch") =>
  items(doc).find((i): i is Chart => i.kind === "chart" && i.componentId === id);

const field = (
  id: string,
  expression: string,
  patch: Partial<FieldComponent> = {},
): FieldComponent => ({
  id,
  kind: "field",
  x: 0,
  y: 0,
  w: 120,
  h: 14,
  expression,
  ...patch,
});

const group = (patch: Partial<ReportGroup> = {}): ReportGroup => ({
  id: "g",
  groupBy: "record.region",
  header: band(0),
  footer: band(0),
  ...patch,
});

describe("running sums", () => {
  it("accumulate over all rows or restart at each group", () => {
    const report = baseReport({
      groups: [group()],
      detail: band(14, [
        field("all", "record.amount", { runningSum: "all", x: 0 }),
        field("grp", "record.amount", { runningSum: "group", x: 130 }),
        field("plain", "record.amount", { x: 260 }),
      ]),
    });
    const doc = layoutReport(report, orders);
    expect(textsOf(doc, "all").map((t) => t.text)).toEqual(["80", "90", "390", "510.5", "552.75"]);
    expect(textsOf(doc, "grp").map((t) => t.text)).toEqual(["80", "90", "300", "120.5", "162.75"]);
    expect(textsOf(doc, "plain").map((t) => t.text)).toEqual(["80", "10", "300", "120.5", "42.25"]);
    expect(doc.diagnostics).toEqual([]);
  });

  it("sum group footers across groups, restarting at the enclosing group", () => {
    const total: CalculatedComponent = {
      id: "tot",
      kind: "calculated",
      x: 0,
      y: 0,
      w: 120,
      h: 14,
      expression: "sum(rows.amount)",
      runningSum: "group",
    };
    const outer = group({ id: "o", groupBy: "record.qty > 1" });
    const inner = group({ id: "i", footer: band(14, [total]) });
    const doc = layoutReport(baseReport({ groups: [outer, inner] }), orders);
    expect(textsOf(doc, "tot").map((t) => t.text)).toEqual(["80", "380", "10", "172.75"]);
  });

  it("apply formats, skip nulls and report non-numbers", () => {
    const rows = [{ v: 1 }, { v: null }, { v: 2.5 }, { v: "x" }, { v: 3 }];
    const report = baseReport({
      detail: band(14, [field("s", "record.v", { runningSum: "all", format: "0.00" })]),
    });
    const doc = layoutReport(report, rows);
    expect(textsOf(doc, "s").map((t) => t.text)).toEqual([
      "1.00",
      "1.00",
      "3.50",
      "#Error",
      "#Error",
    ]);
    expect(doc.diagnostics).toEqual([{ componentId: "s", message: RUNNING_NOT_NUMBER }]);
  });

  it("print the plain value outside detail and group bands", () => {
    const report = baseReport({
      reportFooter: band(14, [field("f", "sum(rows.amount)", { runningSum: "all" })]),
    });
    expect(textsOf(layoutReport(report, orders), "f")[0].text).toBe("552.75");
  });
});

describe("conditional formatting", () => {
  const rows = [{ amount: -5 }, { amount: 10 }, { amount: 500 }];
  const report = () =>
    baseReport({
      detail: band(14, [
        field("a", "record.amount", {
          conditions: [
            { id: "neg", when: "value < 0", style: { gray: 0.5, fill: 0.9 } },
            { id: "big", when: "record.amount >= 100", style: { bold: true } },
            { id: "never", when: "value > 1000", style: { bold: false } },
          ],
        }),
      ]),
    });

  it("applies the first matching rule's style", () => {
    const doc = layoutReport(report(), rows);
    const texts = textsOf(doc, "a");
    expect(texts.map((t) => [t.text, t.gray, t.bold])).toEqual([
      ["-5", 0.5, false],
      ["10", 0, false],
      ["500", 0, true],
    ]);
    const fills = items(doc).filter((i) => i.kind === "rect" && i.componentId === "a");
    expect(fills).toHaveLength(1);
    expect(fills[0]).toMatchObject({ fill: 0.9, y: texts[0].y });
  });

  it("measures grown text with the conditional style", () => {
    const words = (amount: number) => [{ amount, label: "Mmmmm Mmmmm" }];
    const report = baseReport({
      detail: band(14, [
        field("a", "record.label", {
          canGrow: true,
          w: 92,
          conditions: [{ id: "b", when: "record.amount >= 100", style: { bold: true } }],
        }),
      ]),
    });
    expect(textsOf(layoutReport(report, words(1)), "a")[0].lines).toHaveLength(1);
    const bold = textsOf(layoutReport(report, words(100)), "a")[0];
    expect(bold.bold).toBe(true);
    expect(bold.lines).toHaveLength(2);
  });

  it("reports a failing condition and keeps the base style", () => {
    const doc = layoutReport(
      baseReport({
        detail: band(14, [
          field("a", "record.amount", {
            conditions: [{ id: "x", when: "value + 'a'", style: { bold: true } }],
          }),
        ]),
      }),
      [{ amount: 1 }],
    );
    expect(textsOf(doc, "a")[0].bold).toBe(false);
    expect(doc.diagnostics[0].componentId).toBe("a");
    expect(doc.diagnostics[0].message).toMatch(/^Condition: /);
  });
});

const chart = (patch: Partial<ChartComponent> = {}): ChartComponent => ({
  id: "ch",
  kind: "chart",
  x: 10,
  y: 0,
  w: 300,
  h: 160,
  chartType: "bar",
  xField: "region",
  yFields: ["amount"],
  ...patch,
});

describe("charts", () => {
  const layout = (c: ChartComponent, rows = orders, tables?: Record<string, typeof orders>) =>
    layoutReport(baseReport({ reportFooter: band(170, [c]) }), rows, { tables });

  it("draws bars from the band's rows with the dashboard palette", () => {
    const doc = layout(chart({ title: "Sales by region" }));
    const item = chartOf(doc);
    expect(item).toMatchObject({ x: 46, y: 36, w: 300, h: 160, title: "Sales by region" });
    const bars = item?.marks.filter((m) => m.kind === "path" && m.fill === "#2a78d6") ?? [];
    expect(bars).toHaveLength(3);
    const labels = item?.marks.flatMap((m) => (m.kind === "text" ? [m.text] : []));
    expect(labels).toEqual(expect.arrayContaining(["Sales by region", "West", "East", "North"]));
    for (const m of item?.marks ?? [])
      if (m.kind === "path")
        for (const op of m.ops)
          if (op[0] !== "Z") {
            expect(op[1]).toBeGreaterThanOrEqual(46 - 0.01);
            expect(op[1]).toBeLessThanOrEqual(346 + 0.01);
            expect(op[2]).toBeGreaterThanOrEqual(36 - 0.01);
            expect(op[2]).toBeLessThanOrEqual(196 + 0.01);
          }
  });

  it("is deterministic", () => {
    const a = layout(chart({ chartType: "line", groupBy: "customer" }));
    const b = layout(chart({ chartType: "line", groupBy: "customer" }));
    expect(a).toEqual(b);
  });

  it("adds a legend for two or more series", () => {
    const item = chartOf(layout(chart({ yFields: ["amount", "price"] })));
    const labels = item?.marks.flatMap((m) => (m.kind === "text" ? [m.text] : []));
    expect(labels).toEqual(expect.arrayContaining(["amount", "price"]));
  });

  it("reads a saved query's rows when queryId is set", () => {
    const rows = [{ region: "Q", amount: 1 }];
    const item = chartOf(layout(chart({ queryId: "q" }), orders, { q: rows as typeof orders }));
    const labels = item?.marks.flatMap((m) => (m.kind === "text" ? [m.text] : []));
    expect(labels).toContain("Q");
    const missing = layout(chart({ queryId: "other" }));
    expect(missing.diagnostics).toEqual([
      { componentId: "ch", message: "Chart query data is not loaded" },
    ]);
  });

  it("draws pie, donut, area and scatter charts", () => {
    for (const chartType of ["pie", "donut", "area", "scatter"] as const) {
      const c = chart({ chartType, xField: chartType === "scatter" ? "qty" : "region" });
      const item = chartOf(layout(c));
      expect(
        item?.marks.some((m) => m.kind === "path" && m.fill !== null),
        chartType,
      ).toBe(true);
    }
    const pie = chartOf(layout(chart({ chartType: "pie" })));
    const shares = pie?.marks.flatMap((m) => (m.kind === "text" ? [m.text] : []));
    expect(shares).toEqual(["West (29%)", "East (16%)", "North (54%)"]);
  });

  it("says No data when nothing can be drawn", () => {
    const item = chartOf(layout(chart(), []));
    expect(item?.marks).toHaveLength(1);
    expect(item?.marks[0]).toMatchObject({ kind: "text", text: "No data" });
  });

  it("moves to the next page whole instead of splitting", () => {
    const tall = baseReport({
      detail: band(700, []),
      reportFooter: band(170, [chart()]),
    });
    const doc = layoutReport(tall, [{ region: "A", amount: 1 }]);
    expect(doc.pages).toHaveLength(2);
    expect(doc.pages[1].items.find((i) => i.kind === "chart")).toMatchObject({ y: 36 });
  });

  it("writes RGB paths to the PDF", () => {
    const doc = layout(chart());
    const pdf = new TextDecoder("latin1").decode(
      writePdf(doc, { creationDate: "2026-01-01T00:00:00Z" }),
    );
    expect(pdf).toContain("0.16 0.47 0.84 rg");
    expect(pdf).toMatch(/ c\n|\bre f|\bh\n/);
    expect(pdf).toContain(" l");
  });
});

describe("chart paths", () => {
  it("parses dashboard M/L/Z paths with an offset", () => {
    expect(parsePath("M1,2L3,4Z", 10, 20)).toEqual([["M", 11, 22], ["L", 13, 24], ["Z"]]);
  });

  it("approximates arcs with at most quarter-turn Bézier segments", () => {
    expect(arcOps(0, 0, 10, 0, Math.PI)).toHaveLength(2);
    const [seg] = arcOps(0, 0, 10, 0, Math.PI / 2);
    expect(seg.slice(5)).toEqual([0, 10]);
  });

  it("draws full donuts with an opposite-direction hole", () => {
    const ops = wedgePath(0, 0, 10, 5, 0, 2 * Math.PI);
    expect(ops.filter((op) => op[0] === "M")).toHaveLength(2);
    expect(ops.filter((op) => op[0] === "Z")).toHaveLength(2);
  });
});
