//! Edge routes and the view transform of the relationship diagram.
//!
//! A relationship is drawn as straight horizontal and vertical segments. It leaves the
//! referenced column's row on the card's right edge, enters the referencing column's row on
//! the card's left edge, and goes around every card on the way. Routes are found with a
//! shortest-path search over the grid of lines that run along the card margins, so a route
//! prefers few bends and never crosses a card.

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

pub type Point = (f32, f32);

/// A rectangle in diagram coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn inflate(&self, by: f32) -> Rect {
        Rect {
            x: self.x - by,
            y: self.y - by,
            w: self.w + 2.0 * by,
            h: self.h + 2.0 * by,
        }
    }

    fn strictly_contains(&self, p: Point) -> bool {
        p.0 > self.x && p.0 < self.x + self.w && p.1 > self.y && p.1 < self.y + self.h
    }

    pub fn contains(&self, p: Point) -> bool {
        p.0 >= self.x && p.0 <= self.x + self.w && p.1 >= self.y && p.1 <= self.y + self.h
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect {
            x,
            y,
            w: (self.x + self.w).max(other.x + other.w) - x,
            h: (self.y + self.h).max(other.y + other.h) - y,
        }
    }
}

/// The gap a route keeps from every card.
const CLEARANCE: f32 = 16.0;
/// The straight run out of the source card and into the target card.
const STUB: f32 = 24.0;
/// What a bend costs, in pixels of route length.
const BEND: f32 = 40.0;
/// How far outside the endpoints' bounding box a route may wander.
const REACH: f32 = 400.0;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

impl Dir {
    fn reverse(self) -> Dir {
        match self {
            Dir::Left => Dir::Right,
            Dir::Right => Dir::Left,
            Dir::Up => Dir::Down,
            Dir::Down => Dir::Up,
        }
    }
}

#[derive(PartialEq)]
struct Open {
    cost: f32,
    state: (usize, usize, Dir),
}
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.cost.total_cmp(&self.cost)
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn sorted_unique(mut values: Vec<f32>) -> Vec<f32> {
    values.sort_by(f32::total_cmp);
    values.dedup_by(|a, b| (*a - *b).abs() < 0.5);
    values
}

/// The route from `from` (a source card's right edge) to `to` (a target card's left edge)
/// around `cards`, as the corner points of its segments, `from` and `to` included.
pub fn route(from: Point, to: Point, cards: &[Rect]) -> Vec<Point> {
    let start = (from.0 + STUB, from.1);
    let end = (to.0 - STUB, to.1);
    // Blocking uses a slightly smaller margin so the margin lines themselves stay free.
    let blocks: Vec<Rect> = cards.iter().map(|c| c.inflate(CLEARANCE - 1.0)).collect();
    let free = |p: Point| !blocks.iter().any(|b| b.strictly_contains(p));
    if !free(start) || !free(end) {
        return simplify(fallback(from, to, start, end));
    }
    let (lo_x, hi_x) = (start.0.min(end.0) - REACH, start.0.max(end.0) + REACH);
    let (lo_y, hi_y) = (start.1.min(end.1) - REACH, start.1.max(end.1) + REACH);
    let mut xs = vec![start.0, end.0];
    let mut ys = vec![start.1, end.1];
    for card in cards {
        let margin = card.inflate(CLEARANCE);
        xs.extend([margin.x, margin.x + margin.w].into_iter().filter(|x| (lo_x..=hi_x).contains(x)));
        ys.extend([margin.y, margin.y + margin.h].into_iter().filter(|y| (lo_y..=hi_y).contains(y)));
    }
    let xs = sorted_unique(xs);
    let ys = sorted_unique(ys);
    let index = |values: &[f32], v: f32| values.iter().position(|&x| (x - v).abs() < 0.5);
    let (Some(sx), Some(sy), Some(ex), Some(ey)) =
        (index(&xs, start.0), index(&ys, start.1), index(&xs, end.0), index(&ys, end.1))
    else {
        return simplify(fallback(from, to, start, end));
    };

    let step = |(ix, iy): (usize, usize), dir: Dir| -> Option<(usize, usize)> {
        let next = match dir {
            Dir::Left => (ix.checked_sub(1)?, iy),
            Dir::Right => (ix + 1, iy),
            Dir::Up => (ix, iy.checked_sub(1)?),
            Dir::Down => (ix, iy + 1),
        };
        if next.0 >= xs.len() || next.1 >= ys.len() {
            return None;
        }
        let (a, b) = ((xs[ix], ys[iy]), (xs[next.0], ys[next.1]));
        let middle = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
        (free(b) && free(middle)).then_some(next)
    };

    let mut best: HashMap<(usize, usize, Dir), f32> = HashMap::new();
    let mut came: HashMap<(usize, usize, Dir), (usize, usize, Dir)> = HashMap::new();
    let mut open = BinaryHeap::new();
    let first = (sx, sy, Dir::Right);
    best.insert(first, 0.0);
    open.push(Open { cost: 0.0, state: first });
    let mut found = None;
    while let Some(Open { cost, state }) = open.pop() {
        if best.get(&state).is_some_and(|&b| cost > b) {
            continue;
        }
        let (ix, iy, dir) = state;
        if (ix, iy) == (ex, ey) {
            // Arriving at the target stub heading left needs a turn back, so it costs a bend.
            let total = cost + if dir == Dir::Right { 0.0 } else { BEND };
            if found.is_none_or(|(c, _)| total < c) {
                found = Some((total, state));
            }
            continue;
        }
        if found.is_some_and(|(c, _)| cost >= c) {
            break;
        }
        for next_dir in [Dir::Left, Dir::Right, Dir::Up, Dir::Down] {
            if next_dir == dir.reverse() {
                continue;
            }
            let Some((nx, ny)) = step((ix, iy), next_dir) else {
                continue;
            };
            let length = (xs[nx] - xs[ix]).abs() + (ys[ny] - ys[iy]).abs();
            let next_cost = cost + length + if next_dir == dir { 0.0 } else { BEND };
            let next = (nx, ny, next_dir);
            if best.get(&next).is_none_or(|&b| next_cost < b) {
                best.insert(next, next_cost);
                came.insert(next, state);
                open.push(Open { cost: next_cost, state: next });
            }
        }
    }
    let Some((_, mut state)) = found else {
        return simplify(fallback(from, to, start, end));
    };
    let mut corners = vec![to, end];
    while let Some(&previous) = came.get(&state) {
        corners.push((xs[previous.0], ys[previous.1]));
        state = previous;
    }
    corners.push(from);
    corners.reverse();
    simplify(corners)
}

/// A plain elbow, for when the endpoints sit inside other cards.
fn fallback(from: Point, to: Point, start: Point, end: Point) -> Vec<Point> {
    if end.0 >= start.0 {
        let middle = (start.0 + end.0) / 2.0;
        vec![from, (middle, from.1), (middle, to.1), to]
    } else {
        let middle = (from.1 + to.1) / 2.0;
        vec![from, start, (start.0, middle), (end.0, middle), end, to]
    }
}

/// Drops repeated points and the middle point of three in a line.
fn simplify(points: Vec<Point>) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(points.len());
    for p in points {
        if out.last().is_some_and(|q| (q.0 - p.0).abs() < 0.01 && (q.1 - p.1).abs() < 0.01) {
            continue;
        }
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            let vertical = (a.0 - b.0).abs() < 0.01 && (b.0 - p.0).abs() < 0.01;
            let horizontal = (a.1 - b.1).abs() < 0.01 && (b.1 - p.1).abs() < 0.01;
            if vertical || horizontal {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 1.4;
/// The share of the view left empty around the diagram on fit-to-view.
const FIT_PADDING: f32 = 0.12;

/// How the diagram maps onto the screen: `screen = pan + diagram * zoom`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub zoom: f32,
    pub pan: Point,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: 1.0, pan: (0.0, 0.0) }
    }
}

impl View {
    pub fn to_screen(&self, p: Point) -> Point {
        (self.pan.0 + p.0 * self.zoom, self.pan.1 + p.1 * self.zoom)
    }

    pub fn to_diagram(&self, p: Point) -> Point {
        ((p.0 - self.pan.0) / self.zoom, (p.1 - self.pan.1) / self.zoom)
    }

    /// Zooms by `factor`, keeping the diagram point under `anchor` (a screen point) in place.
    pub fn zoom_at(&mut self, factor: f32, anchor: Point) {
        let fixed = self.to_diagram(anchor);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = (anchor.0 - fixed.0 * self.zoom, anchor.1 - fixed.1 * self.zoom);
    }

    /// Moves the view so the diagram point `p` sits in the middle of a `size` viewport.
    pub fn center_on(&mut self, p: Point, size: Point) {
        self.pan = (size.0 / 2.0 - p.0 * self.zoom, size.1 / 2.0 - p.1 * self.zoom);
    }

    /// The view that shows all of `content` in a `size` viewport, centred.
    pub fn fit(content: Rect, size: Point) -> View {
        let pad = 1.0 + 2.0 * FIT_PADDING;
        let zoom = (size.0 / (content.w * pad))
            .min(size.1 / (content.h * pad))
            .clamp(MIN_ZOOM, MAX_ZOOM);
        let mut view = View { zoom, pan: (0.0, 0.0) };
        view.center_on((content.x + content.w / 2.0, content.y + content.h / 2.0), size);
        view
    }

    /// The part of the diagram a `size` viewport shows.
    pub fn visible(&self, size: Point) -> Rect {
        let (x, y) = self.to_diagram((0.0, 0.0));
        Rect { x, y, w: size.0 / self.zoom, h: size.1 / self.zoom }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    fn crosses(route: &[Point], card: &Rect) -> bool {
        route.windows(2).any(|w| {
            let (a, b) = (w[0], w[1]);
            let steps = 50;
            (1..steps).any(|i| {
                let t = i as f32 / steps as f32;
                card.strictly_contains((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t))
            })
        })
    }

    fn axis_aligned(route: &[Point]) -> bool {
        route.windows(2).all(|w| (w[0].0 - w[1].0).abs() < 0.01 || (w[0].1 - w[1].1).abs() < 0.01)
    }

    #[test]
    fn connects_neighbouring_cards_with_straight_segments() {
        let cards = [card(0.0, 0.0, 240.0, 112.0), card(300.0, 0.0, 240.0, 112.0)];
        let r = route((240.0, 52.0), (300.0, 76.0), &cards);
        assert_eq!(r.first(), Some(&(240.0, 52.0)));
        assert_eq!(r.last(), Some(&(300.0, 76.0)));
        assert!(axis_aligned(&r));
        assert!(r.len() <= 4, "{r:?}");
        assert!(cards.iter().all(|c| !crosses(&r, c)));
    }

    #[test]
    fn a_level_route_is_one_straight_line() {
        let cards = [card(0.0, 0.0, 240.0, 112.0), card(300.0, 0.0, 240.0, 112.0)];
        assert_eq!(route((240.0, 52.0), (300.0, 52.0), &cards), vec![(240.0, 52.0), (300.0, 52.0)]);
    }

    #[test]
    fn goes_around_a_card_in_the_way() {
        // The shop diagram: Products (bottom left) references Order Items (top right), and
        // Orders sits between them.
        let products = card(30.0, 265.0, 240.0, 112.0);
        let orders = card(330.0, 25.0, 240.0, 112.0);
        let items = card(630.0, 25.0, 240.0, 136.0);
        let cards = [products, orders, items];
        let r = route((270.0, 317.0), (630.0, 113.0), &cards);
        assert!(axis_aligned(&r), "{r:?}");
        assert!(!crosses(&r, &orders), "{r:?}");
        assert!(cards.iter().all(|c| !crosses(&r, c)), "{r:?}");
    }

    #[test]
    fn a_self_reference_loops_around_its_own_card() {
        let only = card(0.0, 0.0, 240.0, 112.0);
        let r = route((240.0, 52.0), (0.0, 76.0), &[only]);
        assert!(axis_aligned(&r));
        assert!(!crosses(&r, &only), "{r:?}");
        assert_eq!(r.last(), Some(&(0.0, 76.0)));
    }

    #[test]
    fn a_child_left_of_its_parent_is_reached_from_the_left() {
        let parent = card(400.0, 0.0, 240.0, 112.0);
        let child = card(0.0, 200.0, 240.0, 112.0);
        let r = route((640.0, 52.0), (0.0, 252.0), &[parent, child]);
        assert!(axis_aligned(&r));
        assert!(!crosses(&r, &parent) && !crosses(&r, &child), "{r:?}");
        let before_end = r[r.len() - 2];
        assert!(before_end.0 < 0.0, "enters the child from its left: {r:?}");
    }

    #[test]
    fn zooming_keeps_the_point_under_the_cursor() {
        let mut view = View { zoom: 1.0, pan: (10.0, 20.0) };
        let under = view.to_diagram((200.0, 100.0));
        view.zoom_at(1.25, (200.0, 100.0));
        assert_eq!(view.zoom, 1.25);
        let after = view.to_diagram((200.0, 100.0));
        assert!((after.0 - under.0).abs() < 0.001 && (after.1 - under.1).abs() < 0.001);
        view.zoom_at(10.0, (0.0, 0.0));
        assert_eq!(view.zoom, MAX_ZOOM);
        view.zoom_at(0.01, (0.0, 0.0));
        assert_eq!(view.zoom, MIN_ZOOM);
    }

    #[test]
    fn fit_centres_the_diagram_within_the_zoom_limits() {
        let content = card(100.0, 50.0, 800.0, 400.0);
        let view = View::fit(content, (1240.0, 620.0));
        assert!((view.zoom - 1.25).abs() < 0.001, "{view:?}");
        let centre = view.to_screen((500.0, 250.0));
        assert!((centre.0 - 620.0).abs() < 0.01 && (centre.1 - 310.0).abs() < 0.01);
        assert_eq!(View::fit(card(0.0, 0.0, 100.0, 100.0), (2000.0, 2000.0)).zoom, MAX_ZOOM);
        assert_eq!(View::fit(card(0.0, 0.0, 9000.0, 100.0), (500.0, 500.0)).zoom, MIN_ZOOM);
    }
}
