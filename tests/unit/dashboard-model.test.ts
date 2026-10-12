import { describe, expect, it } from "vitest";
import {
  chartData,
  dashboardParams,
  defaultFilterValues,
  filterParams,
  foldSeries,
  kpiValue,
  resultRows,
  scatterData,
} from "../../src/dashboards/data";
import {
  dashboardIssues,
  duplicateDashboard,
  embeddableModes,
  newComponent,
  newDashboard,
  newFilter,
  normalizeDashboard,
  withLayout,
} from "../../src/dashboards/model";
import type { DashboardComponent, DashboardFilter } from "../../src/dashboards/types";
import { newForm, upgradeDesign } from "../../src/design/schema";
import { defaultGridLayout, normalizeLayout } from "../../src/grid/engine";
import type { DocumentConfig } from "../../src/lib/types";

const chart = (patch: Partial<DashboardComponent>): DashboardComponent => ({
  id: "c",
  kind: "chart",
  title: "Chart",
  placement: { column: 1, row: 1, columnSpan: 6, rowSpan: 2, region: null },
  chartType: "bar",
  x: "region",
  y: ["amount"],
  ...patch,
});

const rows = [
  { region: "East", year: 2025, amount: 10, cost: 4 },
  { region: "West", year: 2025, amount: 5, cost: 1 },
  { region: "East", year: 2026, amount: 7, cost: null },
  { region: null, year: 2026, amount: "3", cost: 2 },
];

describe("query results to chart data", () => {
  it("converts DataValues to plain row objects", () => {
    expect(
      resultRows({
        columns: ["a", "b", "c"],
        rows: [[{ type: "integer", value: 2 }, { type: "text", value: "x" }, { type: "null" }]],
      }),
    ).toEqual([{ a: 2, b: "x", c: null }]);
  });

  it("sums rows per category with one series per value field", () => {
    expect(chartData(chart({ y: ["amount", "cost"] }), rows)).toEqual({
      categories: ["East", "West", "(blank)"],
      series: [
        { name: "amount", values: [17, 5, 3] },
        { name: "cost", values: [4, 1, 2] },
      ],
    });
  });

  it("splits the first value field by group", () => {
    expect(chartData(chart({ groupBy: "year" }), rows)).toEqual({
      categories: ["East", "West", "(blank)"],
      series: [
        { name: "2025", values: [10, 5, null] },
        { name: "2026", values: [7, null, 3] },
      ],
    });
  });

  it("folds series past the palette into Other", () => {
    const series = Array.from({ length: 10 }, (_, i) => ({ name: `s${i}`, values: [i, null] }));
    const folded = foldSeries(series);
    expect(folded.map((s) => s.name)).toEqual(["s0", "s1", "s2", "s3", "s4", "s5", "s6", "Other"]);
    expect(folded[7].values).toEqual([7 + 8 + 9, null]);
  });

  it("builds scatter points from numeric x and y, skipping blanks", () => {
    expect(scatterData(chart({ chartType: "scatter", x: "amount", y: ["cost"] }), rows)).toEqual({
      series: [
        {
          name: "cost",
          points: [
            { x: 10, y: 4, label: "10" },
            { x: 5, y: 1, label: "5" },
            { x: 3, y: 2, label: "3" },
          ],
        },
      ],
    });
    expect(
      scatterData(
        chart({ chartType: "scatter", x: "amount", y: ["cost"], groupBy: "region" }),
        rows,
      ).series.map((s) => [s.name, s.points.length]),
    ).toEqual([
      ["East", 1],
      ["West", 1],
      ["(blank)", 1],
    ]);
  });
});

describe("KPI values", () => {
  const kpi = (patch: Partial<DashboardComponent>): DashboardComponent => ({
    ...chart({}),
    kind: "kpi",
    ...patch,
  });

  it("evaluates an expression over rows and params with a format", () => {
    const value = kpiValue(
      kpi({ expression: "sum(rows.amount) * params.rate", format: "#,##0.00" }),
      rows.slice(0, 3),
      {
        params: { rate: 100 },
      },
    );
    expect(value).toEqual({ value: 2200, text: "2,200.00" });
  });

  it("reads a field from the first row and compares", () => {
    const value = kpiValue(
      kpi({ valueField: "amount", comparison: { valueField: "cost", label: "cost" } }),
      rows,
    );
    expect(value.text).toBe("10");
    expect(value.comparison).toEqual({ value: 4, delta: 6, label: "cost", text: "+6" });
    expect(kpiValue(kpi({ valueField: "amount" }), []).text).toBe("—");
  });

  it("throws expression errors for the widget to show", () => {
    expect(() => kpiValue(kpi({ expression: "sum(rows.amount" }), rows)).toThrow();
  });
});

describe("filters to query parameters", () => {
  const filter = (patch: Partial<DashboardFilter>): DashboardFilter => ({
    ...newFilter("Region", "region"),
    ...patch,
  });

  it("maps blank to null so the query default applies", () => {
    expect(filterParams(filter({}), "")).toEqual({ region: null });
    expect(filterParams(filter({}), "East")).toEqual({ region: "East" });
    expect(filterParams(filter({ param: "" }), "East")).toEqual({});
  });

  it("converts numbers and splits date ranges", () => {
    expect(
      filterParams(filter({ param: "n", control: "number", logicalType: "integer" }), "4.7"),
    ).toEqual({ n: 4 });
    expect(
      filterParams(filter({ param: "n", control: "text", logicalType: "number" }), "abc"),
    ).toEqual({ n: null });
    expect(
      filterParams(filter({ param: "period", control: "dateRange" }), {
        from: "2026-01-01",
        to: "",
      }),
    ).toEqual({ periodFrom: "2026-01-01", periodTo: null });
  });

  it("starts from defaults and merges all filters", () => {
    const filters = [
      filter({ id: "a", default: "West" }),
      filter({ id: "b", param: "period", control: "dateRange" }),
    ];
    const values = defaultFilterValues(filters);
    expect(values).toEqual({ a: "West", b: {} });
    expect(dashboardParams(filters, values)).toEqual({
      region: "West",
      periodFrom: null,
      periodTo: null,
    });
  });
});

describe("dashboard model", () => {
  it("places new components at the first free grid slot with kind sizes", () => {
    const dashboard = newDashboard("Sales");
    const kpi = newComponent("kpi", dashboard);
    dashboard.components.push(kpi);
    const second = newComponent("kpi", dashboard);
    dashboard.components.push(second);
    const bar = newComponent("chart", dashboard);
    expect([kpi.placement, second.placement, bar.placement]).toEqual([
      { column: 1, row: 1, columnSpan: 3, rowSpan: 1, region: null },
      { column: 4, row: 1, columnSpan: 3, rowSpan: 1, region: null },
      { column: 7, row: 1, columnSpan: 6, rowSpan: 2, region: null },
    ]);
    expect([kpi.title, second.title, bar.title, bar.chartType]).toEqual([
      "KPI 1",
      "KPI 2",
      "Chart 1",
      "bar",
    ]);
  });

  it("duplicates with fresh ids and remaps filter components", () => {
    const dashboard = newDashboard("Sales");
    const f = newFilter("Region", "region");
    dashboard.filters.push(f);
    dashboard.components.push(newComponent("filter", dashboard, { filterId: f.id }));
    const copy = duplicateDashboard(dashboard, "Sales copy");
    expect(copy.id).not.toBe(dashboard.id);
    expect(copy.filters[0].id).not.toBe(f.id);
    expect(copy.components[0].id).not.toBe(dashboard.components[0].id);
    expect(copy.components[0].filterId).toBe(copy.filters[0].id);
    expect(copy.components[0].placement).toEqual(dashboard.components[0].placement);
  });

  it("clamps placements when the grid gets narrower", () => {
    const dashboard = newDashboard("Sales");
    dashboard.components.push({
      ...newComponent("chart", dashboard),
      placement: { column: 7, row: 1, columnSpan: 6, rowSpan: 2, region: null },
    });
    const narrow = withLayout(dashboard, {
      ...dashboard.layout,
      columns: dashboard.layout.columns.slice(0, 4),
    });
    expect(narrow.components[0].placement).toEqual({
      column: 1,
      row: 1,
      columnSpan: 4,
      rowSpan: 2,
      region: null,
    });
  });

  it("serializes layouts and placements exactly like a form", () => {
    const layoutJson = {
      columns: [
        { kind: "fixed", value: 200 },
        { kind: "fr", value: 2, min: 120 },
      ],
      rows: [{ kind: "content" }],
      columnGap: 8,
      rowGap: 12,
      padding: 4,
      justifyItems: "start",
      alignItems: "center",
      namedRegions: [{ name: "hero", column: 1, row: 1, columnSpan: 2, rowSpan: 1 }],
      breakpoints: [{ minWidth: 600, columns: [{ kind: "fr", value: 1 }] }],
      gridTemplateColumns: "200px 2fr",
    };
    const placementJson = { column: 2, row: 3, columnSpan: 1, rowSpan: 2, gridArea: "x" };
    const dashboard = normalizeDashboard({
      id: "d",
      name: "Ops",
      layout: layoutJson,
      components: [{ id: "k", kind: "kpi", placement: placementJson }],
    });
    const form = upgradeDesign({
      forms: [
        {
          id: "f",
          name: "F",
          layout: layoutJson,
          controls: [{ id: "k", kind: "text", placement: placementJson }],
        },
      ],
    }).forms[0];
    expect(JSON.stringify(dashboard.layout)).toBe(JSON.stringify(form.layout));
    expect(JSON.stringify(dashboard.components[0].placement)).toBe(
      JSON.stringify(form.controls[0].placement),
    );
    expect(JSON.stringify(dashboard.layout)).not.toContain("gridTemplate");
    expect(normalizeDashboard(JSON.parse(JSON.stringify(dashboard)))).toEqual(dashboard);
    expect(normalizeDashboard({ id: "x", name: "Old" })).toEqual({
      id: "x",
      name: "Old",
      layout: normalizeLayout({}),
      filters: [],
      components: [],
    });
    expect(normalizeLayout({})).toEqual(defaultGridLayout());
  });

  it("reports grid and reference problems", () => {
    const dashboard = newDashboard("Sales");
    dashboard.components.push(
      {
        ...newComponent("kpi", dashboard, { queryId: "missing" }),
        id: "k",
        placement: { column: 11, row: 1, columnSpan: 3, rowSpan: 1, region: null },
      },
      { ...newComponent("filter", dashboard), id: "f", filterId: "nope" },
    );
    const config = {
      savedQueries: [],
      design: { forms: [] },
      reports: [],
      actions: [],
    } as unknown as DocumentConfig;
    expect(
      dashboardIssues(dashboard, config)
        .filter((i) => i.severity === "error")
        .map((i) => i.message),
    ).toEqual([
      "grid placement exceeds declared columns",
      '"KPI 1" uses a saved query that does not exist',
      '"Filter 1" shows a filter that does not exist',
    ]);
  });

  it("flags a table style rule with no column", () => {
    const dashboard = newDashboard("Sales");
    const table = { ...newComponent("table", dashboard), id: "t", title: "Orders" };
    const good = { id: "a", when: "value < 0", tone: "negative" as const, column: "amount" };
    dashboard.components.push({
      ...table,
      styles: [good, { id: "b", when: "value > 9", tone: "positive", column: null }],
    });
    const config = { savedQueries: [] } as unknown as DocumentConfig;
    const messages = dashboardIssues(dashboard, config).map((i) => i.message);
    expect(messages).toContain('"Orders" has a conditional style with no column');
    dashboard.components[0].styles = [good];
    expect(dashboardIssues(dashboard, config).map((i) => i.message)).not.toContain(
      '"Orders" has a conditional style with no column',
    );
  });
});

describe("embeddableModes", () => {
  it("drops create and edit for read-only query forms", () => {
    expect(embeddableModes(newForm("T", { kind: "table", table: "t" }))).toEqual([
      "list",
      "detail",
      "create",
      "edit",
    ]);
    const query = { ...newForm("Q", { kind: "query", queryId: "q" }), modes: ["list", "edit"] };
    expect(embeddableModes(query as ReturnType<typeof newForm>)).toEqual(["list"]);
    expect(embeddableModes(null)).toHaveLength(6);
  });
});
