import type {
  Band,
  CalculatedComponent,
  FieldComponent,
  ReportComponent,
  StaticTextComponent,
} from "../types";
import { type RenderContext, textHeight } from "./render";
import { styledComponent } from "./values";

const EPS = 1e-6;

export const GROW_WITH_TABLE = "Can grow is ignored in a band with a table";

/** Grown positions of one band instance: component boxes that moved or grew, and the extra band height. */
export interface Growth {
  boxes: Map<string, { y: number; h: number }>;
  extra: number;
}

const canGrow = (
  c: ReportComponent,
): c is StaticTextComponent | FieldComponent | CalculatedComponent =>
  (c.kind === "staticText" || c.kind === "field" || c.kind === "calculated") && !!c.canGrow;

/**
 * Can-grow layout of a band. A text box with `canGrow` grows to fit its
 * wrapped text; every component that starts at or below a box's designed
 * bottom edge moves down by that box's growth plus its own shift; the band
 * grows by the largest shift plus growth. Returns null when nothing grows,
 * so bands without growing text lay out exactly as designed.
 *
 * Text is measured with the band's row scope before pagination, so `page`
 * and `pages` don't make a box grow. Bands with a table don't grow text.
 */
export function growBand(band: Band, ctx: RenderContext): Growth | null {
  const growing = band.components.filter(canGrow);
  if (!growing.length) return null;
  if (band.components.some((c) => c.kind === "table")) {
    for (const c of growing) ctx.diagnose(c.id, GROW_WITH_TABLE);
    return null;
  }
  const growth = new Map<ReportComponent, number>();
  for (const c of growing) {
    const { component, text } = styledComponent(c, ctx);
    const need = textHeight(component, text);
    if (need > c.h + EPS) growth.set(c, need - c.h);
  }
  if (!growth.size) return null;
  // Top to bottom (stable), so every box above a component has its shift already.
  const order = band.components
    .map((c, i) => ({ c, i }))
    .sort((a, b) => a.c.y - b.c.y || a.i - b.i);
  const shift = new Map<ReportComponent, number>();
  const boxes = new Map<string, { y: number; h: number }>();
  let extra = 0;
  for (const { c } of order) {
    let s = 0;
    for (const [g, grow] of growth)
      if (g !== c && g.y + g.h <= c.y + EPS) s = Math.max(s, (shift.get(g) ?? 0) + grow);
    shift.set(c, s);
    const grow = growth.get(c) ?? 0;
    if (s || grow) boxes.set(c.id, { y: c.y + s, h: c.h + grow });
    extra = Math.max(extra, s + grow);
  }
  return { boxes, extra };
}

/** A component at its grown position. */
export function grown<C extends ReportComponent>(c: C, growth: Growth | null | undefined): C {
  const box = growth?.boxes.get(c.id);
  return box ? { ...c, ...box } : c;
}
