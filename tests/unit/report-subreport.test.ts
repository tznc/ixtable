import { describe, expect, it } from "vitest";
import {
  layoutReport,
  type ReportDocument,
  type Row,
  SUBREPORT_DEPTH,
  SUBREPORT_EXTRA,
  SUBREPORT_LOOP,
  SUBREPORT_MISSING,
  SUBREPORT_PAGE_BAND,
} from "../../src/reports/engine";
import { linkedRows, splitAtSubreport } from "../../src/reports/engine/subreport";
import { embeddableReports, reportProblems, subreportDepth } from "../../src/reports/model";
import type { Report, ReportComponent, SubreportComponent } from "../../src/reports/types";
import { band, baseReport } from "./report-fixtures";

const field = (id: string, expression: string, y = 0, x = 0): ReportComponent => ({
  id,
  kind: "field",
  expression,
  x,
  y,
  w: 200,
  h: 14,
});
const sub = (
  reportId: string,
  y = 14,
  links = [{ child: "order_id", master: "id" }],
): SubreportComponent => ({
  id: `s-${reportId}`,
  kind: "subreport",
  reportId,
  links,
  x: 40,
  y,
  w: 300,
  h: 20,
});

const invoices: Row[] = [
  { id: 1, customer: "Acme" },
  { id: 2, customer: "Birch" },
  { id: 3, customer: "Cobalt" },
];
const lines: Row[] = [
  { id: 10, order_id: 1, item: "Bolt", qty: 4 },
  { id: 11, order_id: 1, item: "Nut", qty: 8 },
  { id: 12, order_id: 2, item: "Gear", qty: 1 },
  { id: 13, order_id: "1", item: "Washer", qty: 2 },
];

const named = (id: string, bands: Partial<Report["bands"]>): Report => ({
  ...baseReport(bands),
  id,
  name: id,
});
const linesReport = named("lines", {
  reportHeader: band(12, [field("lh", "'Lines of ' & parent.customer")]),
  detail: band(12, [field("ld", "record.item & ' x' & record.qty")]),
  reportFooter: band(12, [field("lf", "sum(rows.qty)")]),
});
const invoiceReport = (detailHeight = 50, extra: ReportComponent[] = []) =>
  named("invoices", {
    detail: band(detailHeight, [
      field("name", "record.customer"),
      sub("lines"),
      field("total", "'End ' & record.id", 34),
      ...extra,
    ]),
  });

const texts = (doc: ReportDocument) =>
  doc.pages.map((p) =>
    p.items.flatMap((i) => (i.kind === "text" ? [[i.componentId, i.text, i.x, i.y]] : [])),
  );

describe("subreports", () => {
  it("prints linked rows at the subreport's x and pushes the band below it down", () => {
    const doc = layoutReport(invoiceReport(), invoices.slice(0, 2), {
      subreports: { lines: { report: linesReport, rows: lines } },
    });
    expect(doc.diagnostics).toEqual([]);
    expect(texts(doc)).toEqual([
      [
        ["name", "Acme", 36, 36],
        ["lh", "Lines of Acme", 76, 50],
        ["ld", "Bolt x4", 76, 62],
        ["ld", "Nut x8", 76, 74],
        ["ld", "Washer x2", 76, 86],
        ["lf", "14", 76, 98],
        ["total", "End 1", 36, 110],
        ["name", "Birch", 36, 126],
        ["lh", "Lines of Birch", 76, 140],
        ["ld", "Gear x1", 76, 152],
        ["lf", "1", 76, 164],
        ["total", "End 2", 36, 176],
      ],
    ]);
  });

  it("prints nothing for a parent row without linked rows", () => {
    const doc = layoutReport(invoiceReport(), [invoices[2]], {
      subreports: { lines: { report: linesReport, rows: lines } },
    });
    expect(texts(doc)).toEqual([
      [
        ["name", "Cobalt", 36, 36],
        ["total", "End 3", 36, 50],
      ],
    ]);
  });

  it("splits across pages between subreport bands", () => {
    const many = Array.from({ length: 80 }, (_, i) => ({ order_id: 1, item: `I${i}`, qty: 1 }));
    const doc = layoutReport(invoiceReport(), [invoices[0]], {
      subreports: { lines: { report: linesReport, rows: many } },
    });
    expect(doc.pages.length).toBe(2);
    const [first, second] = texts(doc);
    expect(first[0]).toEqual(["name", "Acme", 36, 36]);
    expect(Math.max(...first.map((t) => t[3] as number))).toBeLessThanOrEqual(806 - 12);
    expect(second[0][3]).toBe(36);
    expect(second.at(-1)).toEqual(["total", "End 1", 36, (second.at(-2)?.[3] as number) + 12]);
    const items = [...first, ...second].filter((t) => t[0] === "ld").map((t) => t[1]);
    expect(items).toEqual(many.map((r) => `${r.item} x1`));
  });

  it("nests three levels and stops at the fourth or at a loop", () => {
    const level = (id: string, child?: string): Report =>
      named(id, {
        detail: band(30, [
          field(`f-${id}`, `'${id} ' & record.n`),
          ...(child ? [sub(child, 14, [])] : []),
        ]),
      });
    const reports = {
      a: level("a", "b"),
      b: level("b", "c"),
      c: level("c", "d"),
      d: level("d", "e"),
    };
    const rows = [{ n: 1 }];
    const subreports = Object.fromEntries(
      Object.entries(reports).map(([id, report]) => [id, { report, rows }]),
    );
    const doc = layoutReport(reports.a, rows, { subreports });
    expect(texts(doc)[0].map((t) => [t[1], t[2]])).toEqual([
      ["a 1", 36],
      ["b 1", 76],
      ["c 1", 116],
      ["d 1", 156],
    ]);
    expect(doc.diagnostics).toEqual([{ componentId: "s-e", message: SUBREPORT_DEPTH }]);
    const looped = layoutReport(reports.a, rows, {
      subreports: { ...subreports, b: { report: level("b", "a"), rows } },
    });
    expect(looped.diagnostics).toEqual([{ componentId: "s-a", message: SUBREPORT_LOOP }]);
  });

  it("reports missing data, extra subreports and subreports in page bands", () => {
    const detailed = invoiceReport(50, [{ ...sub("other", 0), id: "extra" }]);
    const report = {
      ...detailed,
      bands: { ...detailed.bands, pageFooter: band(20, [sub("lines", 0)]) },
    };
    const doc = layoutReport(report, [invoices[0]]);
    expect(doc.diagnostics).toEqual([
      { componentId: "extra", message: SUBREPORT_EXTRA },
      { componentId: "s-lines", message: SUBREPORT_MISSING },
      { componentId: "s-lines", message: SUBREPORT_PAGE_BAND },
    ]);
  });

  it("splits a band around the subreport and filters linked rows", () => {
    const b = band(60, [
      field("a", "1"),
      field("beside", "2", 20, 300),
      sub("x", 14),
      field("z", "3", 40),
    ]);
    const { head, tail } = splitAtSubreport(b, sub("x", 14));
    expect(head.height).toBe(14);
    expect(head.components.map((c) => c.id)).toEqual(["a", "beside"]);
    expect(tail.height).toBe(26);
    expect(tail.components.map((c) => [c.id, c.y])).toEqual([["z", 6]]);
    expect(linkedRows(sub("x"), lines, { id: 2 })).toEqual([lines[2]]);
    expect(linkedRows(sub("x"), lines, { id: null })).toEqual([]);
    expect(linkedRows(sub("x", 0, []), lines, null)).toEqual(lines);
  });
});

describe("subreport nesting in the designer", () => {
  const level = (id: string, child?: string): Report =>
    named(id, { detail: band(30, child ? [sub(child, 0)] : []) });

  it("measures depth and offers only reports that keep three levels without loops", () => {
    const reports = [level("a", "b"), level("b", "c"), level("c"), level("d"), level("e", "d")];
    expect(subreportDepth(reports, "a")).toBe(2);
    expect(subreportDepth([level("x", "y"), level("y", "x")], "x")).toBe(Number.POSITIVE_INFINITY);
    expect(embeddableReports(reports, reports[2]).map((r) => r.id)).toEqual(["d"]);
    expect(embeddableReports(reports, reports[0]).map((r) => r.id)).toEqual(["b", "c", "d", "e"]);
    expect(embeddableReports(reports, reports[3]).map((r) => r.id)).toEqual(["b", "c"]);
  });

  it("lists nesting and placement problems", () => {
    const bad = named("a", {
      detail: band(30, [sub("b", 0), { ...sub("zzz", 0), id: "s2" }]),
      pageHeader: band(20, [{ ...sub("b", 0), id: "s3" }]),
    });
    const problems = reportProblems(bad, [bad, level("b", "a")]);
    expect(problems).toEqual([
      "its subreports print each other in a loop",
      "Page header component s3: subreports are not supported in page headers or footers",
      "Detail has more than one subreport",
      "Detail component s2: the subreport's report does not exist",
    ]);
  });
});
