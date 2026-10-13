//! Path helpers for report charts: rectangles, circles and arcs as Bézier curves.

use super::{r2, PathOp};
use std::f64::consts::PI;

/// Ops of a path from the chart geometry, moved by (dx, dy).
pub fn offset_path(ops: &[PathOp], dx: f64, dy: f64) -> Vec<PathOp> {
    ops.iter()
        .map(|op| match op {
            PathOp::M(x, y) => PathOp::M(r2(x + dx), r2(y + dy)),
            PathOp::L(x, y) => PathOp::L(r2(x + dx), r2(y + dy)),
            other => other.clone(),
        })
        .collect()
}

pub fn rect_path(x: f64, y: f64, w: f64, h: f64) -> Vec<PathOp> {
    vec![
        PathOp::M(r2(x), r2(y)),
        PathOp::L(r2(x + w), r2(y)),
        PathOp::L(r2(x + w), r2(y + h)),
        PathOp::L(r2(x), r2(y + h)),
        PathOp::Z,
    ]
}

pub fn line_path(x1: f64, y1: f64, x2: f64, y2: f64) -> Vec<PathOp> {
    vec![PathOp::M(r2(x1), r2(y1)), PathOp::L(r2(x2), r2(y2))]
}

fn at(cx: f64, cy: f64, r: f64, a: f64) -> (f64, f64) {
    (cx + r * a.cos(), cy + r * a.sin())
}

/// Cubic Bézier segments along a circle from angle `a0` to `a1` (radians, clockwise on
/// the page, either direction), at most a quarter turn each. The pen must already be at
/// the start point.
pub fn arc_ops(cx: f64, cy: f64, r: f64, a0: f64, a1: f64) -> Vec<PathOp> {
    let n = (((a1 - a0).abs() / (PI / 2.0) - 1e-9).ceil() as i64).max(1);
    let step = (a1 - a0) / n as f64;
    let k = (4.0 / 3.0) * (step / 4.0).tan();
    (0..n)
        .map(|i| {
            let s = a0 + i as f64 * step;
            let e = s + step;
            let (x0, y0) = at(cx, cy, r, s);
            let (x3, y3) = at(cx, cy, r, e);
            PathOp::C(
                r2(x0 - k * r * s.sin()),
                r2(y0 + k * r * s.cos()),
                r2(x3 + k * r * e.sin()),
                r2(y3 - k * r * e.cos()),
                r2(x3),
                r2(y3),
            )
        })
        .collect()
}

fn move_to(p: (f64, f64)) -> PathOp {
    PathOp::M(r2(p.0), r2(p.1))
}

fn line_to(p: (f64, f64)) -> PathOp {
    PathOp::L(r2(p.0), r2(p.1))
}

pub fn circle_path(cx: f64, cy: f64, r: f64) -> Vec<PathOp> {
    let mut ops = vec![move_to(at(cx, cy, r, 0.0))];
    ops.extend(arc_ops(cx, cy, r, 0.0, 2.0 * PI));
    ops.push(PathOp::Z);
    ops
}

/// A pie wedge (inner 0) or donut segment from `a0` to `a1`. A full turn is a circle, with
/// the hole drawn in the opposite direction so nonzero filling leaves it empty.
pub fn wedge_path(cx: f64, cy: f64, r: f64, inner: f64, a0: f64, a1: f64) -> Vec<PathOp> {
    if a1 - a0 >= 2.0 * PI - 1e-9 {
        let mut outer = vec![move_to(at(cx, cy, r, a0))];
        outer.extend(arc_ops(cx, cy, r, a0, a1));
        outer.push(PathOp::Z);
        if inner <= 0.0 {
            return outer;
        }
        outer.push(move_to(at(cx, cy, inner, a0)));
        outer.extend(arc_ops(cx, cy, inner, a0, a0 - 2.0 * PI));
        outer.push(PathOp::Z);
        return outer;
    }
    if inner <= 0.0 {
        let mut ops = vec![PathOp::M(r2(cx), r2(cy)), line_to(at(cx, cy, r, a0))];
        ops.extend(arc_ops(cx, cy, r, a0, a1));
        ops.push(PathOp::Z);
        return ops;
    }
    let mut ops = vec![move_to(at(cx, cy, r, a0))];
    ops.extend(arc_ops(cx, cy, r, a0, a1));
    ops.push(line_to(at(cx, cy, inner, a1)));
    ops.extend(arc_ops(cx, cy, inner, a1, a0));
    ops.push(PathOp::Z);
    ops
}

/// Mixes a `#rrggbb` color with white; `amount` 0 keeps it, 1 gives white.
pub fn tint(color: &str, amount: f64) -> String {
    let channel = |i: usize| {
        let v = u8::from_str_radix(color.get(1 + i * 2..3 + i * 2).unwrap_or("00"), 16).unwrap_or(0) as f64;
        format!("{:02x}", super::js_round(v + (255.0 - v) * amount) as i64)
    };
    format!("#{}{}{}", channel(0), channel(1), channel(2))
}
