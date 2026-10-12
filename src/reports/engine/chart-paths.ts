/** Path helpers for report charts: SVG path parsing, rectangles, circles and arcs as Bézier curves. */
import { type PathOp, r2 } from "./document";

/** Ops of an `M`/`L`/`Z` path from the dashboard geometry, moved by (dx, dy). */
export function parsePath(d: string, dx: number, dy: number): PathOp[] {
  const ops: PathOp[] = [];
  for (const [, cmd, args] of d.matchAll(/([MLZ])([^MLZ]*)/g)) {
    if (cmd === "Z") {
      ops.push(["Z"]);
      continue;
    }
    const [x, y] = args.split(",").map(Number);
    ops.push([cmd as "M" | "L", r2(x + dx), r2(y + dy)]);
  }
  return ops;
}

export const rectPath = (x: number, y: number, w: number, h: number): PathOp[] => [
  ["M", r2(x), r2(y)],
  ["L", r2(x + w), r2(y)],
  ["L", r2(x + w), r2(y + h)],
  ["L", r2(x), r2(y + h)],
  ["Z"],
];

export const linePath = (x1: number, y1: number, x2: number, y2: number): PathOp[] => [
  ["M", r2(x1), r2(y1)],
  ["L", r2(x2), r2(y2)],
];

const at = (cx: number, cy: number, r: number, a: number): [number, number] => [
  cx + r * Math.cos(a),
  cy + r * Math.sin(a),
];

/**
 * Cubic Bézier segments along a circle from angle `a0` to `a1` (radians,
 * clockwise on the page, either direction), at most a quarter turn each.
 * The pen must already be at the start point.
 */
export function arcOps(cx: number, cy: number, r: number, a0: number, a1: number): PathOp[] {
  const n = Math.max(1, Math.ceil(Math.abs(a1 - a0) / (Math.PI / 2) - 1e-9));
  const step = (a1 - a0) / n;
  const k = (4 / 3) * Math.tan(step / 4);
  const ops: PathOp[] = [];
  for (let i = 0; i < n; i++) {
    const s = a0 + i * step;
    const e = s + step;
    const [x0, y0] = at(cx, cy, r, s);
    const [x3, y3] = at(cx, cy, r, e);
    ops.push([
      "C",
      r2(x0 - k * r * Math.sin(s)),
      r2(y0 + k * r * Math.cos(s)),
      r2(x3 + k * r * Math.sin(e)),
      r2(y3 - k * r * Math.cos(e)),
      r2(x3),
      r2(y3),
    ]);
  }
  return ops;
}

const move = (p: [number, number], op: "M" | "L"): PathOp => [op, r2(p[0]), r2(p[1])];

export const circlePath = (cx: number, cy: number, r: number): PathOp[] => [
  move(at(cx, cy, r, 0), "M"),
  ...arcOps(cx, cy, r, 0, 2 * Math.PI),
  ["Z"],
];

/**
 * A pie wedge (inner 0) or donut segment from `a0` to `a1`. A full turn is a
 * circle, with the hole drawn in the opposite direction so nonzero filling
 * leaves it empty.
 */
export function wedgePath(
  cx: number,
  cy: number,
  r: number,
  inner: number,
  a0: number,
  a1: number,
): PathOp[] {
  if (a1 - a0 >= 2 * Math.PI - 1e-9) {
    const outer: PathOp[] = [move(at(cx, cy, r, a0), "M"), ...arcOps(cx, cy, r, a0, a1), ["Z"]];
    if (inner <= 0) return outer;
    return [
      ...outer,
      move(at(cx, cy, inner, a0), "M"),
      ...arcOps(cx, cy, inner, a0, a0 - 2 * Math.PI),
      ["Z"],
    ];
  }
  if (inner <= 0)
    return [
      ["M", r2(cx), r2(cy)],
      move(at(cx, cy, r, a0), "L"),
      ...arcOps(cx, cy, r, a0, a1),
      ["Z"],
    ];
  return [
    move(at(cx, cy, r, a0), "M"),
    ...arcOps(cx, cy, r, a0, a1),
    move(at(cx, cy, inner, a1), "L"),
    ...arcOps(cx, cy, inner, a1, a0),
    ["Z"],
  ];
}

/** Mixes a `#rrggbb` color with white; `amount` 0 keeps it, 1 gives white. */
export function tint(color: string, amount: number): string {
  const channel = (i: number) => {
    const v = Number.parseInt(color.slice(1 + i * 2, 3 + i * 2), 16);
    return Math.round(v + (255 - v) * amount)
      .toString(16)
      .padStart(2, "0");
  };
  return `#${channel(0)}${channel(1)}${channel(2)}`;
}
