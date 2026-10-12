/**
 * Report charts (PRD §15, Phase 6). Data and geometry come from the dashboard
 * charts (`src/dashboards/data.ts`, `src/dashboards/charts/`); this module
 * turns them into one `chart` item of paths and text marks in page points,
 * so preview, print and PDF draw the same chart.
 */
import {
  type CategoryData,
  chartData,
  formatNumber,
  type Row,
  scatterData,
} from "../../dashboards/data";
import {
  barGeometry,
  type Frame,
  lineGeometry,
  pieGeometry,
  plotArea,
  scatterGeometry,
} from "../../dashboards/charts/geometry";
import {
  AXIS,
  GRID,
  MAX_SERIES,
  OTHER_COLOR,
  OTHER_LABEL,
  seriesColor,
} from "../../dashboards/charts/palette";
import { linearScale, type Ticks } from "../../dashboards/charts/scale";
import type { ChartComponent, ReportChartType } from "../types";
import { circlePath, linePath, parsePath, rectPath, tint, wedgePath } from "./chart-paths";
import { type PathItem, type PathOp, type PositionedItem, r2, type TextItem } from "./document";
import type { RenderContext } from "./render";
import { textItem } from "./render";
import { LINE_HEIGHT, measureText } from "./text";

export const CHART_NO_DATA = "No data";
export const CHART_QUERY_NOT_LOADED = "Chart query data is not loaded";

const TYPE_NAMES: Record<ReportChartType, string> = {
  bar: "Bar chart",
  line: "Line chart",
  area: "Area chart",
  pie: "Pie chart",
  donut: "Donut chart",
  scatter: "Scatter plot",
};

const LABEL_SIZE = 7;
const TITLE_SIZE = 9;
const LABEL_LINE = LABEL_SIZE * LINE_HEIGHT;
const TITLE_HEIGHT = TITLE_SIZE * LINE_HEIGHT + 4;
const LEGEND_ROW = LABEL_LINE + 3;
const SWATCH = 7;
/** Label and axis text gray, close to the dashboard's muted ink. */
const LABEL_GRAY = 0.36;
const TITLE_GRAY = 0.18;

type Legend = { name: string; color: string }[];

/** Collects marks for one chart in page coordinates. */
class Marks {
  readonly marks: (PathItem | TextItem)[] = [];
  constructor(
    readonly id: string,
    readonly ox: number,
    readonly oy: number,
  ) {}
  path(ops: PathOp[], fill: string | null, stroke: string | null = null, lineWidth = 0) {
    if (ops.length) this.marks.push({ kind: "path", ops, fill, stroke, lineWidth });
  }
  /** One line of text in a box at chart-relative (x, y), clipped to the box width. */
  text(
    raw: string,
    x: number,
    y: number,
    w: number,
    options: {
      align?: "left" | "center" | "right";
      size?: number;
      bold?: boolean;
      gray?: number;
    } = {},
  ) {
    const size = options.size ?? LABEL_SIZE;
    const box = {
      id: this.id,
      x,
      y,
      w: Math.max(1, w),
      h: size * LINE_HEIGHT,
      style: {
        fontSize: size,
        bold: options.bold,
        align: options.align ?? "left",
        gray: options.gray ?? LABEL_GRAY,
      },
    };
    const item = textItem(box, raw, this.ox, this.oy, { padX: 0 });
    if (item.kind === "text" && item.lines.length) this.marks.push(item);
  }
}

/** Legend rows: swatch and name entries flowing left to right, wrapping at `width`. */
function legendRows(legend: Legend, width: number): Legend[] {
  const rows: Legend[] = [];
  let used = Number.POSITIVE_INFINITY;
  for (const entry of legend) {
    const w = SWATCH + 3 + measureText(entry.name, LABEL_SIZE, false) + 10;
    if (used + w > width && rows.length < 3) {
      rows.push([]);
      used = 0;
    }
    if (used + w > width && rows[rows.length - 1].length) continue;
    rows[rows.length - 1].push(entry);
    used += w;
  }
  return rows;
}

function drawLegend(m: Marks, rows: Legend[], top: number, width: number) {
  rows.forEach((row, r) => {
    const widths = row.map((e) => SWATCH + 3 + measureText(e.name, LABEL_SIZE, false));
    const total = widths.reduce((a, b) => a + b, 0) + 10 * (row.length - 1);
    let x = Math.max(0, (width - total) / 2);
    const y = top + r * LEGEND_ROW;
    row.forEach((entry, i) => {
      m.path(rectPath(m.ox + x, m.oy + y + 1, SWATCH, SWATCH), entry.color);
      m.text(entry.name, x + SWATCH + 3, y, widths[i] - SWATCH - 3 + 1);
      x += widths[i] + 10;
    });
  });
}

/** Gridlines and labels for a value axis on the left of the plot. */
function valueAxis(m: Marks, ticks: Ticks, plot: ReturnType<typeof plotArea>, format?: string) {
  const y = linearScale([ticks.min, ticks.max], [plot.bottom, plot.top]);
  for (const tick of ticks.ticks) {
    const ty = y(tick);
    m.path(linePath(m.ox + plot.left, m.oy + ty, m.ox + plot.right, m.oy + ty), null, GRID, 0.5);
    m.text(formatNumber(tick, format), 0, ty - LABEL_LINE / 2, plot.left - 4, { align: "right" });
  }
}

/** Category labels under the plot, thinned so they don't overlap. */
function categoryAxis(
  m: Marks,
  labels: { text: string; x: number }[],
  plot: ReturnType<typeof plotArea>,
) {
  if (!labels.length) return;
  const band = (plot.right - plot.left) / labels.length;
  const widest = Math.max(...labels.map((l) => measureText(l.text, LABEL_SIZE, false)));
  const step = Math.max(1, Math.ceil((widest + 4) / Math.max(1, band)));
  labels.forEach((label, i) => {
    if (i % step) return;
    const w = band * step;
    m.text(label.text, label.x - w / 2, plot.bottom + 3, w, { align: "center" });
  });
}

const valueTicksWidth = (ticks: Ticks, format?: string) =>
  Math.max(...ticks.ticks.map((t) => measureText(formatNumber(t, format), LABEL_SIZE, false)));

function frameFor(w: number, h: number, left: number): Frame {
  return {
    width: w,
    height: h,
    margin: { top: LABEL_LINE / 2 + 2, right: 8, bottom: LABEL_LINE + 5, left },
  };
}

const hasValues = (data: CategoryData) =>
  data.categories.length > 0 &&
  data.series.some((s) => s.values.some((v) => v !== null && Number.isFinite(v)));

function categoryChart(
  m: Marks,
  c: ChartComponent,
  data: CategoryData,
  w: number,
  h: number,
  top: number,
) {
  const kind = c.chartType;
  const stacked = !!c.stacked;
  const geometry = (frame: Frame) =>
    kind === "bar"
      ? barGeometry(data, { stacked, frame })
      : lineGeometry(data, { area: kind === "area", stacked, frame });
  const first = geometry(frameFor(w, h, 40));
  const frame = frameFor(w, h, Math.min(w / 3, valueTicksWidth(first.ticks, c.format) + 6));
  const plot = plotArea(frame);
  const dy = m.oy + top;
  const shifted = new Marks(m.id, m.ox, dy);
  valueAxis(shifted, first.ticks, plot, c.format);
  const color = (s: number) => seriesColor(s, data.series[s]?.name);
  if (kind === "bar") {
    const g = barGeometry(data, { stacked, frame });
    for (const bar of g.bars)
      shifted.path(rectPath(m.ox + bar.x, dy + bar.y, bar.width, bar.height), color(bar.series));
    shifted.path(
      linePath(m.ox + plot.left, dy + g.zeroY, m.ox + plot.right, dy + g.zeroY),
      null,
      AXIS,
      0.75,
    );
    categoryAxis(shifted, g.xLabels, plot);
  } else {
    const g = lineGeometry(data, { area: kind === "area", stacked, frame });
    g.series.forEach((s, i) => {
      if (s.area) shifted.path(parsePath(s.area, m.ox, dy), tint(color(i), 0.6));
    });
    g.series.forEach((s, i) => {
      shifted.path(parsePath(s.path, m.ox, dy), null, color(i), 1.5);
      if (kind === "line")
        for (const p of s.points) shifted.path(circlePath(m.ox + p.x, dy + p.y, 1.75), color(i));
    });
    shifted.path(
      linePath(m.ox + plot.left, dy + g.zeroY, m.ox + plot.right, dy + g.zeroY),
      null,
      AXIS,
      0.75,
    );
    categoryAxis(shifted, g.xLabels, plot);
  }
  m.marks.push(...shifted.marks);
}

function scatterChart(m: Marks, c: ChartComponent, rows: Row[], w: number, h: number, top: number) {
  const data = scatterData(spec(c), rows);
  if (!data.series.some((s) => s.points.length)) return null;
  const first = scatterGeometry(data, frameFor(w, h, 40));
  const frame = frameFor(w, h, Math.min(w / 3, valueTicksWidth(first.yTicks, c.format) + 6));
  const plot = plotArea(frame);
  const g = scatterGeometry(data, frame);
  const dy = m.oy + top;
  const shifted = new Marks(m.id, m.ox, dy);
  valueAxis(shifted, g.yTicks, plot, c.format);
  const sx = linearScale([g.xTicks.min, g.xTicks.max], [plot.left, plot.right]);
  categoryAxis(
    shifted,
    g.xTicks.ticks.map((t) => ({ text: formatNumber(t), x: sx(t) })),
    plot,
  );
  shifted.path(
    linePath(m.ox + plot.left, dy + plot.bottom, m.ox + plot.right, dy + plot.bottom),
    null,
    AXIS,
    0.75,
  );
  g.series.forEach((s, i) => {
    for (const p of s.points)
      shifted.path(circlePath(m.ox + p.cx, dy + p.cy, 2.25), seriesColor(i, data.series[i].name));
  });
  m.marks.push(...shifted.marks);
  return data.series.map((s, i) => ({ name: s.name, color: seriesColor(i, s.name) }));
}

/** Pie or donut on the left, legend with shares on the right. */
function pieChart(
  m: Marks,
  c: ChartComponent,
  data: CategoryData,
  w: number,
  h: number,
  top: number,
) {
  let labels = data.categories;
  let values = data.series[0]?.values ?? [];
  // Categories past the palette fold into "Other" so slice colors never repeat.
  if (labels.length > MAX_SERIES) {
    const rest = values
      .slice(MAX_SERIES - 1)
      .reduce<number>((sum, v) => sum + Math.max(0, v ?? 0), 0);
    labels = [...labels.slice(0, MAX_SERIES - 1), OTHER_LABEL];
    values = [...values.slice(0, MAX_SERIES - 1), rest];
  }
  const slices = pieGeometry(labels, values);
  if (!slices.length) return false;
  const radius = Math.max(4, Math.min(h, w / 2) / 2 - 4);
  const cx = m.ox + radius + 4;
  const cy = m.oy + top + h / 2;
  const inner = c.chartType === "donut" ? radius * 0.58 : 0;
  const color = (s: (typeof slices)[number]) =>
    s.label === OTHER_LABEL && s.index === MAX_SERIES - 1 ? OTHER_COLOR : seriesColor(s.index);
  let angle = -Math.PI / 2;
  for (const s of slices) {
    const end = angle + s.fraction * 2 * Math.PI;
    m.path(
      wedgePath(cx, cy, radius, inner, angle, end),
      color(s),
      "#ffffff",
      slices.length > 1 ? 0.75 : 0,
    );
    angle = end;
  }
  const lx = 2 * radius + 16;
  const rows = Math.max(1, Math.floor(h / LEGEND_ROW));
  const shown = slices.slice(0, rows);
  const ly = top + (h - shown.length * LEGEND_ROW) / 2;
  shown.forEach((s, i) => {
    const y = ly + i * LEGEND_ROW;
    m.path(rectPath(m.ox + lx, m.oy + y + 1, SWATCH, SWATCH), color(s));
    m.text(
      `${s.label} (${Math.round(s.fraction * 100)}%)`,
      lx + SWATCH + 3,
      y,
      w - lx - SWATCH - 3,
    );
  });
  return true;
}

/** The dashboard chart-data spec of a report chart. */
const spec = (c: ChartComponent) => ({ x: c.xField, y: c.yFields, groupBy: c.groupBy || null });

/** Rows a chart draws: its saved query's rows, else the band's `rows`. */
function chartRows(c: ChartComponent, ctx: RenderContext): Row[] {
  if (c.queryId) {
    const rows = ctx.tables[c.queryId];
    if (!rows) ctx.diagnose(c.id, CHART_QUERY_NOT_LOADED);
    return rows ?? [];
  }
  return Array.isArray(ctx.scope.rows) ? (ctx.scope.rows as Row[]) : [];
}

/** The chart item of one chart component placed with its band origin at (ox, oy). */
export function chartItems(
  c: ChartComponent,
  ox: number,
  oy: number,
  ctx: RenderContext,
): PositionedItem[] {
  const m = new Marks(c.id, ox + c.x, oy + c.y);
  const rows = chartRows(c, ctx);
  const titleText = c.title?.trim() ?? "";
  let top = 0;
  if (titleText) {
    m.text(titleText, 0, 2, c.w, {
      align: "center",
      size: TITLE_SIZE,
      bold: true,
      gray: TITLE_GRAY,
    });
    top = TITLE_HEIGHT;
  }
  const kind = c.chartType;
  let drawn = false;
  if (kind === "scatter") {
    const legend = scatterLegend(c, rows);
    const legendRowsOf = legend.length > 1 ? legendRows(legend, c.w) : [];
    const plotH = c.h - top - legendRowsOf.length * LEGEND_ROW;
    drawn = !!scatterChart(m, c, rows, c.w, plotH, top);
    if (drawn) drawLegend(m, legendRowsOf, top + plotH, c.w);
  } else {
    const data = chartData(spec(c), rows);
    if (hasValues(data)) {
      drawn = true;
      if (kind === "pie" || kind === "donut") drawn = pieChart(m, c, data, c.w, c.h - top, top);
      else {
        const legend = data.series.map((s, i) => ({ name: s.name, color: seriesColor(i, s.name) }));
        const rowsOf = legend.length > 1 ? legendRows(legend, c.w) : [];
        const plotH = c.h - top - rowsOf.length * LEGEND_ROW;
        categoryChart(m, c, data, c.w, plotH, top);
        drawLegend(m, rowsOf, top + plotH, c.w);
      }
    }
  }
  if (!drawn)
    m.text(CHART_NO_DATA, 0, top + (c.h - top - LABEL_LINE) / 2, c.w, { align: "center" });
  return [
    {
      kind: "chart",
      componentId: c.id,
      x: r2(m.ox),
      y: r2(m.oy),
      w: r2(c.w),
      h: r2(c.h),
      title: titleText || TYPE_NAMES[kind] || "Chart",
      marks: m.marks,
    },
  ];
}

function scatterLegend(c: ChartComponent, rows: Row[]): Legend {
  return scatterData(spec(c), rows).series.map((s, i) => ({
    name: s.name,
    color: seriesColor(i, s.name),
  }));
}
