import { evaluate, formatValue } from "../../expr";
import type {
  CalculatedComponent,
  ComponentStyle,
  FieldComponent,
  ReportComponent,
  StaticTextComponent,
  SubreportComponent,
  TableComponent,
} from "../types";
import { type PositionedItem, type Row, r2 } from "./document";
import { BASELINE, LINE_HEIGHT, measureText, normalizeText, wrapText } from "./text";

/** Evaluation context for one band instance. */
export interface RenderContext {
  scope: Record<string, unknown>;
  now?: Date;
  tables: Record<string, Row[]>;
  assets: Record<string, { mediaType: string }>;
  diagnose: (componentId: string, message: string) => void;
}

export const DEFAULT_FONT_SIZE = 10;
export const TABLE_FONT_SIZE = 9;
const TEXT_PAD = 2;
const CELL_PAD_X = 3;
const CELL_PAD_Y = 2;
const HEADER_FILL = 0.9;
const CELL_LINE = 0.5;
const ERROR_TEXT = "#Error";

/** Evaluates `expression` and formats the result; errors become `#Error` plus a diagnostic. */
export function expressionText(
  componentId: string,
  expression: string,
  format: string | undefined,
  ctx: RenderContext,
  scope = ctx.scope,
): string {
  if (!expression.trim()) return "";
  try {
    const value = evaluate(expression, scope, { now: ctx.now });
    return formatValue(value, format || undefined);
  } catch (error) {
    ctx.diagnose(componentId, error instanceof Error ? error.message : String(error));
    return ERROR_TEXT;
  }
}

export interface Box {
  id: string;
  x: number;
  y: number;
  w: number;
  h: number;
  style?: ComponentStyle;
}

/** Optional background/border rectangle for a box. */
function frame(box: Box, ox: number, oy: number): PositionedItem[] {
  const lineWidth = box.style?.borderWidth ?? 0;
  const fill = box.style?.fill ?? null;
  if (!lineWidth && fill === null) return [];
  return [
    {
      kind: "rect",
      componentId: box.id,
      x: r2(ox + box.x),
      y: r2(oy + box.y),
      w: r2(box.w),
      h: r2(box.h),
      lineWidth,
      gray: box.style?.gray ?? 0,
      fill,
    },
  ];
}

/**
 * A text item: wrapped with the fixed metrics, clipped to the lines that fit
 * the box (at least one), aligned horizontally, and centered vertically when
 * `middle` is set.
 */
export function textItem(
  box: Box,
  raw: string,
  ox: number,
  oy: number,
  options: { padX?: number; middle?: boolean; bold?: boolean; fontSize?: number } = {},
): PositionedItem {
  const fontSize = options.fontSize ?? box.style?.fontSize ?? DEFAULT_FONT_SIZE;
  const bold = options.bold ?? box.style?.bold ?? false;
  const padX = options.padX ?? TEXT_PAD;
  const align = box.style?.align ?? "left";
  const lineHeight = fontSize * LINE_HEIGHT;
  const text = normalizeText(raw);
  const all = text ? wrapText(text, Math.max(1, box.w - 2 * padX), fontSize, bold) : [];
  const shown = all.slice(0, Math.max(1, Math.floor((box.h + 1e-6) / lineHeight)));
  const x = ox + box.x;
  const top = oy + box.y + (options.middle ? (box.h - shown.length * lineHeight) / 2 : 0);
  return {
    kind: "text",
    componentId: box.id,
    x: r2(x),
    y: r2(oy + box.y),
    w: r2(box.w),
    h: r2(box.h),
    text,
    lines: shown.map((line, i) => {
      const width = measureText(line, fontSize, bold);
      const lx =
        align === "center"
          ? x + (box.w - width) / 2
          : align === "right"
            ? x + box.w - padX - width
            : x + padX;
      return {
        text: line,
        x: r2(lx),
        y: r2(top + i * lineHeight + fontSize * BASELINE),
        width: r2(width),
      };
    }),
    fontSize,
    bold,
    gray: box.style?.gray ?? 0,
  };
}

/** Height a text box needs to show all of `raw` at its style, in points. */
export function textHeight(box: Box, raw: string): number {
  const fontSize = box.style?.fontSize ?? DEFAULT_FONT_SIZE;
  const text = normalizeText(raw);
  const width = Math.max(1, box.w - 2 * TEXT_PAD);
  const lines = text ? wrapText(text, width, fontSize, box.style?.bold ?? false).length : 1;
  return lines * fontSize * LINE_HEIGHT;
}

/** The text a static text, field or calculated component prints. */
export function componentText(
  c: StaticTextComponent | FieldComponent | CalculatedComponent,
  ctx: RenderContext,
): string {
  return c.kind === "staticText" ? c.text : expressionText(c.id, c.expression, c.format, ctx);
}

function placeholder(box: Box, label: string, ox: number, oy: number): PositionedItem[] {
  return [
    {
      kind: "rect",
      componentId: box.id,
      x: r2(ox + box.x),
      y: r2(oy + box.y),
      w: r2(box.w),
      h: r2(box.h),
      lineWidth: CELL_LINE,
      gray: 0.5,
      fill: null,
    },
    textItem({ ...box, style: { fontSize: 8, align: "center", gray: 0.4 } }, label, ox, oy, {
      middle: true,
    }),
  ];
}

/** Items for one component other than a table or subreport, placed with its band origin at (ox, oy). */
export function componentItems(
  c: Exclude<ReportComponent, TableComponent | SubreportComponent>,
  ox: number,
  oy: number,
  ctx: RenderContext,
): PositionedItem[] {
  switch (c.kind) {
    case "staticText":
    case "field":
    case "calculated":
      return [...frame(c, ox, oy), textItem(c, componentText(c, ctx), ox, oy)];
    case "line": {
      const lineWidth = c.style?.borderWidth ?? 1;
      if (lineWidth <= 0) return [];
      const horizontal = c.orientation !== "vertical";
      return [
        {
          kind: "line",
          componentId: c.id,
          x: r2(ox + c.x + (horizontal ? 0 : c.w / 2)),
          y: r2(oy + c.y + (horizontal ? c.h / 2 : 0)),
          w: horizontal ? r2(c.w) : 0,
          h: horizontal ? 0 : r2(c.h),
          lineWidth,
          gray: c.style?.gray ?? 0,
        },
      ];
    }
    case "rectangle":
      return [
        {
          kind: "rect",
          componentId: c.id,
          x: r2(ox + c.x),
          y: r2(oy + c.y),
          w: r2(c.w),
          h: r2(c.h),
          lineWidth: c.style?.borderWidth ?? 1,
          gray: c.style?.gray ?? 0,
          fill: c.style?.fill ?? null,
        },
      ];
    case "image": {
      const asset = c.assetId ? ctx.assets[c.assetId] : undefined;
      if (!c.assetId) return placeholder(c, "Image", ox, oy);
      if (!asset) {
        ctx.diagnose(c.id, `Image asset ${c.assetId} not found`);
        return placeholder(c, "Missing image", ox, oy);
      }
      if (!/^image\/(jpeg|png)$/.test(asset.mediaType))
        return placeholder(c, "Unsupported image", ox, oy);
      return [
        {
          kind: "image",
          componentId: c.id,
          x: r2(ox + c.x),
          y: r2(oy + c.y),
          w: r2(c.w),
          h: r2(c.h),
          assetId: c.assetId,
          mediaType: asset.mediaType,
        },
        ...frame({ ...c, style: { ...c.style, fill: null } }, ox, oy),
      ];
    }
  }
}

/** Measured table: column positions, cell texts, and row heights. */
export interface TableGeometry {
  columns: { x: number; w: number }[];
  header: string[];
  cells: string[][];
  headerHeight: number;
  rowHeights: number[];
  height: number;
}

/** Evaluates and measures every cell of a table component. */
export function measureTable(t: TableComponent, ctx: RenderContext): TableGeometry {
  const fontSize = t.style?.fontSize ?? TABLE_FONT_SIZE;
  const lineHeight = fontSize * LINE_HEIGHT;
  let rows: Row[];
  if (t.queryId) {
    rows = ctx.tables[t.queryId] ?? [];
    if (!ctx.tables[t.queryId]) ctx.diagnose(t.id, "Table query data is not loaded");
  } else {
    rows = Array.isArray(ctx.scope.rows) ? (ctx.scope.rows as Row[]) : [];
  }
  const total = t.columns.reduce((sum, col) => sum + Math.max(0, col.width), 0) || 1;
  let cx = 0;
  const columns = t.columns.map((col) => {
    const w = (Math.max(0, col.width) / total) * t.w;
    const out = { x: cx, w };
    cx += w;
    return out;
  });
  const linesIn = (text: string, i: number, bold: boolean) =>
    text ? wrapText(text, Math.max(1, columns[i].w - 2 * CELL_PAD_X), fontSize, bold).length : 1;
  const heightOf = (texts: string[], bold: boolean) =>
    Math.max(1, ...texts.map((s, i) => linesIn(s, i, bold))) * lineHeight + 2 * CELL_PAD_Y;
  const header = t.columns.map((col) => normalizeText(col.header));
  const cells = rows.map((record) =>
    t.columns.map((col) =>
      expressionText(t.id, col.expression, col.format, ctx, { ...ctx.scope, record }),
    ),
  );
  const headerHeight = t.columns.length ? heightOf(header, true) : 0;
  const rowHeights = t.columns.length ? cells.map((row) => heightOf(row, false)) : [];
  return {
    columns,
    header,
    cells,
    headerHeight,
    rowHeights,
    height: headerHeight + rowHeights.reduce((a, b) => a + b, 0),
  };
}

/** Items for one table row (`index` -1 is the header) whose top edge is at `top`. */
export function tableRowItems(
  t: TableComponent,
  geo: TableGeometry,
  index: number,
  left: number,
  top: number,
): PositionedItem[] {
  const header = index < 0;
  const texts = header ? geo.header : geo.cells[index];
  const h = header ? geo.headerHeight : geo.rowHeights[index];
  const fontSize = t.style?.fontSize ?? TABLE_FONT_SIZE;
  return t.columns.flatMap((col, i) => {
    const cell = {
      id: t.id,
      x: geo.columns[i].x,
      y: 0,
      w: geo.columns[i].w,
      h,
      style: { align: col.align ?? (header ? "left" : t.style?.align), gray: t.style?.gray },
    };
    return [
      {
        kind: "rect" as const,
        componentId: t.id,
        x: r2(left + cell.x),
        y: r2(top),
        w: r2(cell.w),
        h: r2(h),
        lineWidth: t.style?.borderWidth ?? CELL_LINE,
        gray: t.style?.gray ?? 0,
        fill: header ? HEADER_FILL : null,
      },
      textItem(cell, texts[i], left, top, {
        padX: CELL_PAD_X,
        middle: true,
        bold: header,
        fontSize,
      }),
    ];
  });
}
