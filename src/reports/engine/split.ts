import { moveChart, type PositionedItem, r2 } from "./document";
import { BASELINE, LINE_HEIGHT } from "./text";

/** Coordinates are rounded to 0.01 pt, so comparisons allow that much. */
const TOL = 0.01;

type TextItem = Extract<PositionedItem, { kind: "text" }>;
type Line = TextItem["lines"][number];
const lineTop = (item: TextItem, line: Line) => line.y - item.fontSize * BASELINE;
const lineBottom = (item: TextItem, line: Line) =>
  lineTop(item, line) + item.fontSize * LINE_HEIGHT;

/**
 * Where a piece of a band that starts at `from` and may reach `limit` ends
 * (coordinates relative to the band top). The cut falls between text lines;
 * an image, chart or line that crosses it moves it above that item. Rectangles and
 * text frames don't move it: they are clipped. Returns `from` when nothing
 * fits.
 */
export function cutAt(items: PositionedItem[], from: number, limit: number): number {
  let cut = limit;
  for (let changed = true; changed; ) {
    changed = false;
    for (const item of items) {
      const top = item.y;
      const bottom = item.y + item.h;
      if (top >= cut - TOL || bottom <= cut + TOL || item.kind === "rect") continue;
      let next = cut;
      if (item.kind === "text") {
        const crossing = item.lines.find((line) => lineBottom(item, line) > cut + TOL);
        if (crossing) next = lineTop(item, crossing);
      } else next = top;
      next = Math.max(from, next);
      if (next < cut - TOL) {
        cut = next;
        changed = true;
      }
    }
  }
  return cut;
}

/**
 * The items of one piece of a band, from `from` (inclusive) to `to`
 * (exclusive), moved down by `dy`. Text keeps the lines that start in the
 * piece; rectangles and vertical lines are clipped to it; images and
 * horizontal lines go to the piece their top is in.
 */
export function sliceItems(
  items: PositionedItem[],
  from: number,
  to: number,
  dy: number,
): PositionedItem[] {
  const inside = (y: number) => y >= from - TOL && y < to - TOL;
  const out: PositionedItem[] = [];
  for (const item of items) {
    const top = Math.max(item.y, from);
    const h = Math.min(item.y + item.h, to) - top;
    if (item.kind === "text") {
      const lines = item.lines.filter((line) => inside(lineTop(item, line)));
      if (!lines.length && !(item.lines.length === 0 && inside(item.y))) continue;
      out.push({
        ...item,
        y: r2(top + dy),
        h: r2(h),
        lines: lines.map((line) => ({ ...line, y: r2(line.y + dy) })),
      });
    } else if (item.kind === "chart") {
      if (inside(item.y)) out.push(moveChart(item, dy));
    } else if (item.kind === "image" || (item.kind === "line" && item.h === 0)) {
      if (inside(item.y)) out.push({ ...item, y: r2(item.y + dy) });
    } else if (h > TOL) out.push({ ...item, y: r2(top + dy), h: r2(h) });
  }
  return out;
}
