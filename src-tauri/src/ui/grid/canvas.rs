//! The grid canvas (port of `src/grid/GridCanvas.tsx`): renders items on a layout's tracks,
//! picks the breakpoint from the measured width, and, when `editable`, lets the user select,
//! resize and move items.
//!
//! GPUI grids have uniform tracks only (`grid_cols(n)`), so the layout's `fr` weights become
//! integer sub-columns: a track of weight `w` covers `w` sub-columns and an item spans the
//! sum of its tracks' weights. Fixed and content tracks are treated as `1fr`; column gaps
//! are padding on items (none on the outer edges). Row gaps are the container's `gap_y`.
//!
//! Test ids (editable canvases): the canvas has the id given to `GridCanvas::new` and the
//! accessibility label `label`; `<id>:breakpoint` is labelled with the active breakpoint
//! index (`-1` for the base columns). Each item is `<item id>` (a labelled group). The
//! selected item shows buttons `narrow:<id>`, `widen:<id>`, `shorter:<id>`, `taller:<id>`
//! (accessibility labels "Narrow X", "Widen X", "Make X shorter", "Make X taller") and
//! every editable item has a drag grip `grip:<id>`.

use super::engine::{
    GridDelta, ItemCss, SpanConstraints, move_placement, placements_to_css, resize_placement,
    resolve_breakpoint, resolve_placements, resolve_regions,
};
use crate::design::{GridAlign, GridLayout, GridTrack, TrackKind};
use crate::design::Placement;
use crate::ui::prelude::*;
use gpui_kit::component::{h_flex, v_flex};
use std::rc::Rc;

pub type SelectHandler = Rc<dyn Fn(&str, &mut Window, &mut App)>;
pub type PlacementHandler = Rc<dyn Fn(&str, Placement, &mut Window, &mut App)>;

/// Most sub-columns a layout may expand to; heavier weights are scaled down.
const MAX_SUBCOLUMNS: u32 = 96;

/// One item on a [`GridCanvas`].
pub struct GridItem {
    id: SharedString,
    placement: Placement,
    label: Option<SharedString>,
    constraints: SpanConstraints,
    child: AnyElement,
}

impl GridItem {
    pub fn new(id: impl Into<SharedString>, placement: Placement, child: impl IntoElement) -> Self {
        Self {
            id: id.into(),
            placement,
            label: None,
            constraints: SpanConstraints::default(),
            child: child.into_any_element(),
        }
    }

    /// Accessible name of the item and its resize controls; defaults to the id.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn constraints(mut self, constraints: SpanConstraints) -> Self {
        self.constraints = constraints;
        self
    }
}

#[derive(IntoElement)]
pub struct GridCanvas {
    id: ElementId,
    layout: GridLayout,
    width: Option<f64>,
    editable: bool,
    selected: Option<SharedString>,
    show_regions: Option<bool>,
    label: Option<SharedString>,
    on_select: Option<SelectHandler>,
    on_resize: Option<PlacementHandler>,
    on_move: Option<PlacementHandler>,
    items: Vec<GridItem>,
}

impl GridCanvas {
    pub fn new(id: impl Into<ElementId>, layout: GridLayout) -> Self {
        Self {
            id: id.into(),
            layout,
            width: None,
            editable: false,
            selected: None,
            show_regions: None,
            label: None,
            on_select: None,
            on_resize: None,
            on_move: None,
            items: vec![],
        }
    }

    /// Container width in pixels; when omitted the canvas measures itself.
    pub fn width(mut self, width: f64) -> Self {
        self.width = Some(width);
        self
    }

    /// Design-time mode: items are focusable, selectable and resizable.
    pub fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    pub fn selected(mut self, id: Option<SharedString>) -> Self {
        self.selected = id;
        self
    }

    /// Shows named region outlines; defaults to `editable`.
    pub fn show_regions(mut self, show: bool) -> Self {
        self.show_regions = Some(show);
        self
    }

    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn on_select(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    pub fn on_resize(mut self, f: impl Fn(&str, Placement, &mut Window, &mut App) + 'static) -> Self {
        self.on_resize = Some(Rc::new(f));
        self
    }

    pub fn on_move(mut self, f: impl Fn(&str, Placement, &mut Window, &mut App) + 'static) -> Self {
        self.on_move = Some(Rc::new(f));
        self
    }

    pub fn item(mut self, item: GridItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn items(mut self, items: impl IntoIterator<Item = GridItem>) -> Self {
        self.items.extend(items);
        self
    }
}

/// The width the canvas last measured; changing it re-renders the canvas.
#[derive(Default)]
struct Measured(Option<f64>);

/// Per-item state: the last painted bounds and a drag in progress.
#[derive(Default)]
struct ItemState {
    bounds: Bounds<Pixels>,
    drag: Option<Drag>,
}

struct Drag {
    origin: Point<Pixels>,
    pitch: (f32, f32),
    start: Placement,
    preview: Placement,
}

/// Sub-column weights of the active tracks.
pub fn track_weights(tracks: &[GridTrack]) -> Vec<u16> {
    let raw: Vec<u32> = tracks
        .iter()
        .map(|t| match t.kind {
            TrackKind::Fr => (t.value.unwrap_or(1.0).round().max(1.0)) as u32,
            _ => 1,
        })
        .collect();
    let total: u32 = raw.iter().sum();
    if total <= MAX_SUBCOLUMNS {
        return raw.into_iter().map(|w| w as u16).collect();
    }
    raw.into_iter()
        .map(|w| ((w * MAX_SUBCOLUMNS) / total).max(1) as u16)
        .collect()
}

/// Sub-column line range `[start, end)` (1-based lines) for tracks `column..column+span`.
fn sub_columns(weights: &[u16], column: i64, span: i64) -> (i16, i16) {
    let count = weights.len() as i64;
    let first = (column.clamp(1, count) - 1) as usize;
    let last = ((column - 1 + span.max(1)).clamp(1, count)) as usize;
    let start: u32 = weights[..first].iter().map(|&w| w as u32).sum();
    let width: u32 = weights[first..last.max(first + 1)].iter().map(|&w| w as u32).sum();
    ((start + 1) as i16, (start + 1 + width) as i16)
}

impl RenderOnce for GridCanvas {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let measured = window.use_keyed_state((self.id.clone(), "measured"), cx, |_, _| Measured::default());
        let width = self.width.or(measured.read(cx).0);
        let (tracks, breakpoint) = resolve_breakpoint(&self.layout, width);
        let weights = track_weights(&tracks);
        let total: u16 = weights.iter().sum();
        // A drag in progress shows its preview placement.
        let placements: Vec<Placement> = self
            .items
            .iter()
            .map(|item| {
                if !self.editable {
                    return item.placement.clone();
                }
                let state = window.use_keyed_state((ElementId::from(item.id.clone()), "state"), cx, |_, _| ItemState::default());
                let preview = state.read(cx).drag.as_ref().map(|d| d.preview.clone());
                preview.unwrap_or_else(|| item.placement.clone())
            })
            .collect();
        let css = placements_to_css(&self.layout, &placements, width);
        let resolved = resolve_placements(&self.layout, &placements, width);
        let regions = resolve_regions(&self.layout, width);
        let show_regions = self.show_regions.unwrap_or(self.editable);
        let layout = self.layout.clone();
        let gap = layout.column_gap as f32;
        let last_track = tracks.len() as i64;
        let theme = cx.theme().clone();

        let rect_of = |css: &ItemCss| -> (i64, i64, i64, i64) {
            match css {
                ItemCss::Lines { column, column_span, row, row_span } => {
                    (*column, *column_span, *row, *row_span)
                }
                ItemCss::Area(ident) => regions
                    .iter()
                    .find(|r| &r.ident == ident)
                    .map(|r| {
                        (
                            r.region.column as i64,
                            r.region.column_span as i64,
                            r.region.row as i64,
                            r.region.row_span as i64,
                        )
                    })
                    .unwrap_or((1, 1, 1, 1)),
            }
        };

        let mut container = div()
            .id(self.id.clone())
            .grid()
            .grid_cols(total.max(1))
            .w_full()
            .gap_y(px(layout.row_gap as f32))
            .p(px(layout.padding as f32))
            .when_some(self.label.clone(), |el, label| el.aria_label(label));

        if show_regions {
            for region in regions.iter().filter(|r| r.in_template) {
                let (start, end) = sub_columns(&weights, region.region.column as i64, region.region.column_span as i64);
                container = container.child(
                    div()
                        .col_start(start)
                        .col_end(end)
                        .row_start(region.region.row as i16)
                        .row_end((region.region.row + region.region.row_span) as i16)
                        .border_1()
                        .border_dashed()
                        .border_color(theme.border)
                        .rounded_sm()
                        .ty(Type::Caption)
                        .text_color(theme.muted_foreground)
                        .child(region.region.name.clone()),
                );
            }
        }

        let selected = self.selected.clone();
        let items = self.items;
        for (index, item) in items.into_iter().enumerate() {
            let (column, span, row, row_span) = rect_of(&css[index]);
            let (start, end) = sub_columns(&weights, column, span);
            let left = if column <= 1 { 0.0 } else { gap / 2.0 };
            let right = if column + span - 1 >= last_track { 0.0 } else { gap / 2.0 };
            let id = item.id.clone();
            let name = item.label.clone().unwrap_or_else(|| id.clone());
            let shown = resolved[index].clone();
            let placed = div()
                .flex()
                .flex_col()
                .min_w_0()
                .col_start(start)
                .col_end(end)
                .row_start(row as i16)
                .row_end((row + row_span.max(1)) as i16)
                .pl(px(left))
                .pr(px(right))
                .map(|el| match layout.align_items {
                    GridAlign::Start => el.justify_start(),
                    GridAlign::Center => el.justify_center(),
                    GridAlign::End => el.justify_end(),
                    GridAlign::Stretch => el,
                })
                .map(|el| match layout.justify_items {
                    GridAlign::Start => el.items_start(),
                    GridAlign::Center => el.items_center(),
                    GridAlign::End => el.items_end(),
                    GridAlign::Stretch => el,
                });
            if !self.editable {
                container = container.child(placed.child(item.child));
                continue;
            }
            let is_selected = selected.as_ref() == Some(&id);
            container = container.child(placed.child(editable_item(
                item,
                name,
                shown,
                is_selected,
                &layout,
                width,
                self.on_select.clone(),
                self.on_resize.clone(),
                self.on_move.clone(),
                window,
                cx,
            )));
        }

        let probe = {
            let measured = measured.clone();
            let padding = layout.padding as f64;
            canvas(
                move |bounds, _, cx| {
                    let next = (bounds.size.width.as_f32() as f64 - 2.0 * padding).max(0.0);
                    measured.update(cx, |m, cx| {
                        if m.0.is_none_or(|w| (w - next).abs() > 0.5) {
                            m.0 = Some(next);
                            cx.notify();
                        }
                    });
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full()
        };
        let breakpoint_label = SharedString::from(breakpoint.to_string());
        let breakpoint_id = SharedString::from(format!("{}:breakpoint", element_name(&self.id)));
        let has_label = self.label.is_some();
        v_flex()
            .relative()
            .w_full()
            .child(probe)
            .child(container.test_support())
            .when(has_label, |el| {
                el.child(
                    div()
                        .id(breakpoint_id)
                        .aria_label(breakpoint_label)
                        .size_0()
                        .test_support(),
                )
            })
    }
}

fn element_name(id: &ElementId) -> String {
    match id {
        ElementId::Name(name) => name.to_string(),
        other => format!("{other:?}"),
    }
}

fn same_placement(a: &Placement, b: &Placement) -> bool {
    a == b
}

fn arrow(key: &str) -> Option<GridDelta> {
    Some(match key {
        "right" => GridDelta { columns: 1, rows: 0 },
        "left" => GridDelta { columns: -1, rows: 0 },
        "down" => GridDelta { columns: 0, rows: 1 },
        "up" => GridDelta { columns: 0, rows: -1 },
        _ => return None,
    })
}

#[allow(clippy::too_many_arguments)]
fn editable_item(
    item: GridItem,
    name: SharedString,
    shown: Placement,
    selected: bool,
    layout: &GridLayout,
    width: Option<f64>,
    on_select: Option<SelectHandler>,
    on_resize: Option<PlacementHandler>,
    on_move: Option<PlacementHandler>,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement {
    let id = item.id.clone();
    let placement = item.placement.clone();
    let constraints = item.constraints;
    let focus = window.use_keyed_state((ElementId::from(id.clone()), "focus"), cx, |_, cx| cx.focus_handle());
    let focus = focus.read(cx).clone();
    let state = window.use_keyed_state((ElementId::from(id.clone()), "state"), cx, |_, _| ItemState::default());
    let theme = cx.theme().clone();
    let preview = state.read(cx).drag.as_ref().map(|d| d.preview.clone());
    let layout_for_keys = layout.clone();
    let resized = {
        let layout = layout.clone();
        let placement = placement.clone();
        move |delta: GridDelta| resize_placement(&placement, delta, &layout, constraints)
    };

    let resize = {
        let (on_resize, id, placement, resized) = (on_resize.clone(), id.clone(), placement.clone(), resized.clone());
        move |delta: GridDelta, window: &mut Window, cx: &mut App| {
            let next = resized(delta);
            if !same_placement(&next, &placement) {
                if let Some(f) = &on_resize {
                    f(&id, next, window, cx);
                }
            }
        }
    };
    let move_by = {
        let (on_move, id, placement) = (on_move.clone(), id.clone(), placement.clone());
        move |delta: GridDelta, window: &mut Window, cx: &mut App| {
            let next = move_placement(&placement, delta, &layout_for_keys);
            if !same_placement(&next, &placement) {
                if let Some(f) = &on_move {
                    f(&id, next, window, cx);
                }
            }
        }
    };

    let button = |key: &'static str, tip: String, icon: IconName, delta: GridDelta| {
        let enabled = !same_placement(&resized(delta), &placement);
        let resize = resize.clone();
        Button::new(SharedString::from(format!("{key}:{id}")))
            .ghost()
            .xsmall()
            .icon(Icon::new(icon))
            .tooltip(tip.clone())
            .disabled(!enabled)
            .on_click(move |_, window, cx| resize(delta, window, cx))
    };
    let toolbar = selected.then(|| {
        h_flex()
            .id(SharedString::from(format!("resize:{id}")))
            .aria_label(format!("Resize {name}"))
            .gap_0p5()
            .child(button("narrow", format!("Narrow {name}"), IconName::ArrowLeft, GridDelta { columns: -1, rows: 0 }))
            .child(button("widen", format!("Widen {name}"), IconName::ArrowRight, GridDelta { columns: 1, rows: 0 }))
            .child(button("shorter", format!("Make {name} shorter"), IconName::ArrowUp, GridDelta { columns: 0, rows: -1 }))
            .child(button("taller", format!("Make {name} taller"), IconName::ArrowDown, GridDelta { columns: 0, rows: 1 }))
            .test_support()
    });

    let select = {
        let (on_select, id) = (on_select.clone(), id.clone());
        move |window: &mut Window, cx: &mut App| {
            if let Some(f) = &on_select {
                f(&id, window, cx);
            }
        }
    };
    let grip_down = {
        let (state, select, shown, placement) = (state.clone(), select.clone(), shown.clone(), placement.clone());
        let gap = (layout.column_gap as f32, layout.row_gap as f32);
        move |event: &MouseDownEvent, window: &mut Window, cx: &mut App| {
            if event.button != MouseButton::Left {
                return;
            }
            cx.stop_propagation();
            let bounds = state.read(cx).bounds;
            let pitch = (
                (bounds.size.width.as_f32() + gap.0) / f32::from(shown.column_span.max(1)),
                (bounds.size.height.as_f32() + gap.1) / f32::from(shown.row_span.max(1)),
            );
            state.update(cx, |s, cx| {
                s.drag = Some(Drag {
                    origin: event.position,
                    pitch,
                    start: placement.clone(),
                    preview: placement.clone(),
                });
                cx.notify();
            });
            select(window, cx);
        }
    };
    let dragging = state.read(cx).drag.is_some();
    let probe = {
        let state = state.clone();
        canvas(
            move |bounds, _, cx| {
                state.update(cx, |s, _| s.bounds = bounds);
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    };
    // While a drag is active the canvas listens to the whole window's pointer events.
    let tracker = dragging.then(|| {
        let (state, layout, on_resize, id) = (state.clone(), layout.clone(), on_resize.clone(), id.clone());
        canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let moving = {
                    let (state, layout) = (state.clone(), layout.clone());
                    move |event: &MouseMoveEvent, _: DispatchPhase, _: &mut Window, cx: &mut App| {
                        state.update(cx, |s, cx| {
                            let Some(drag) = s.drag.as_mut() else { return };
                            let dx = (event.position.x - drag.origin.x).as_f32();
                            let dy = (event.position.y - drag.origin.y).as_f32();
                            let delta = GridDelta {
                                columns: if drag.pitch.0 > 0.0 { (dx / drag.pitch.0).round() as i64 } else { 0 },
                                rows: if drag.pitch.1 > 0.0 { (dy / drag.pitch.1).round() as i64 } else { 0 },
                            };
                            let next = resize_placement(&drag.start, delta, &layout, constraints);
                            if next != drag.preview {
                                drag.preview = next;
                                cx.notify();
                            }
                        });
                    }
                };
                let released = {
                    let (state, on_resize, id) = (state.clone(), on_resize.clone(), id.clone());
                    move |_: &MouseUpEvent, _: DispatchPhase, window: &mut Window, cx: &mut App| {
                        let finished = state.update(cx, |s, cx| {
                            cx.notify();
                            s.drag.take()
                        });
                        if let (Some(drag), Some(f)) = (finished, &on_resize) {
                            if !same_placement(&drag.preview, &drag.start) {
                                f(&id, drag.preview, window, cx);
                            }
                        }
                    }
                };
                window.on_mouse_event(moving);
                window.on_mouse_event(released);
            },
        )
        .absolute()
        .size_0()
    });
    let _ = (width, preview);

    let item_focus = focus.clone();
    let key_id = id.clone();
    let key_select = select.clone();
    div()
        .id(id.clone())
        .aria_label(name.clone())
        .test_support()
        .track_focus(&focus)
        .relative()
        .rounded_sm()
        .border_1()
        .border_color(if selected { theme.primary } else { theme.border.opacity(0.0) })
        .on_mouse_down(MouseButton::Left, {
            let select = select.clone();
            move |_, window, cx| select(window, cx)
        })
        .on_key_down(move |event, window, cx| {
            let key = event.keystroke.key.as_str();
            let modifiers = event.keystroke.modifiers;
            if modifiers.alt {
                if let Some(delta) = arrow(key) {
                    cx.stop_propagation();
                    if modifiers.shift {
                        move_by(delta, window, cx);
                    } else {
                        resize(delta, window, cx);
                    }
                    return;
                }
            }
            if item_focus.is_focused(window) && matches!(key, "enter" | "space") {
                cx.stop_propagation();
                key_select(window, cx);
            }
            let _ = &key_id;
        })
        .child(probe)
        .child(item.child)
        .children(toolbar)
        .child(
            div()
                .id(SharedString::from(format!("grip:{id}")))
                .aria_label(format!("Drag to resize {name}"))
                .absolute()
                .right_0()
                .bottom_0()
                .size_3()
                .cursor_nwse_resize()
                .bg(theme.muted_foreground.opacity(0.4))
                .on_mouse_down(MouseButton::Left, grip_down)
                .test_support(),
        )
        .children(tracker)
}

#[cfg(test)]
#[path = "canvas_tests.rs"]
mod tests;
