//! Can-grow layout of a band (PRD §15): a text box with `can_grow` grows to fit its
//! wrapped text, components below it move down, and the band grows with them.

use super::component::{Band, Comp};
use super::render::{text_height, BoxSpec, RenderContext};
use super::values::styled_component;
use std::collections::HashMap;

const EPS: f64 = 1e-6;

pub const GROW_WITH_TABLE: &str = "Can grow is ignored in a band with a table";

/// Grown positions of one band instance: component boxes that moved or grew (by
/// component id, as `(y, h)`), and the extra band height.
#[derive(Debug, Clone, Default)]
pub struct Growth {
    pub boxes: HashMap<String, (f64, f64)>,
    pub extra: f64,
}

fn can_grow(c: &Comp) -> bool {
    c.is_text() && c.can_grow
}

/// Can-grow layout of a band. A text box with `can_grow` grows to fit its wrapped text;
/// every component that starts at or below a box's designed bottom edge moves down by
/// that box's growth plus its own shift; the band grows by the largest shift plus growth.
/// Returns `None` when nothing grows, so bands without growing text lay out exactly as
/// designed.
///
/// Text is measured with the band's row scope before pagination, so `page` and `pages`
/// don't make a box grow. Bands with a table don't grow text.
pub fn grow_band(band: &Band, ctx: &RenderContext) -> Option<Growth> {
    let growing: Vec<usize> = (0..band.components.len())
        .filter(|&i| can_grow(&band.components[i]))
        .collect();
    if growing.is_empty() {
        return None;
    }
    if band.components.iter().any(Comp::is_table) {
        for &i in &growing {
            ctx.diagnose(&band.components[i].id, GROW_WITH_TABLE);
        }
        return None;
    }
    let mut growth: Vec<(usize, f64)> = vec![];
    for &i in &growing {
        let c = &band.components[i];
        let (component, text) = styled_component(c, ctx);
        let need = text_height(&BoxSpec::of(&component), &text);
        if need > c.h + EPS {
            growth.push((i, need - c.h));
        }
    }
    if growth.is_empty() {
        return None;
    }
    // Top to bottom (stable), so every box above a component has its shift already.
    let mut order: Vec<usize> = (0..band.components.len()).collect();
    order.sort_by(|&a, &b| {
        band.components[a]
            .y
            .partial_cmp(&band.components[b].y)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    let mut shift: HashMap<usize, f64> = HashMap::new();
    let mut boxes = HashMap::new();
    let mut extra: f64 = 0.0;
    for i in order {
        let c = &band.components[i];
        let mut s: f64 = 0.0;
        for &(g, grow) in &growth {
            let gc = &band.components[g];
            if g != i && gc.y + gc.h <= c.y + EPS {
                s = s.max(shift.get(&g).copied().unwrap_or(0.0) + grow);
            }
        }
        shift.insert(i, s);
        let grow = growth
            .iter()
            .find(|(g, _)| *g == i)
            .map_or(0.0, |(_, grow)| *grow);
        if s != 0.0 || grow != 0.0 {
            boxes.insert(c.id.clone(), (c.y + s, c.h + grow));
        }
        extra = extra.max(s + grow);
    }
    Some(Growth { boxes, extra })
}

/// A component at its grown position.
pub fn grown(c: &Comp, growth: Option<&Growth>) -> Comp {
    match growth.and_then(|g| g.boxes.get(&c.id)) {
        Some(&(y, h)) => Comp {
            y,
            h,
            ..c.clone()
        },
        None => c.clone(),
    }
}
