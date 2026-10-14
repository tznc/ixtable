//! Relationship diagram (port of `src/data/RelationshipBrowser.tsx`). Tables are cards
//! that can be dragged; their positions persist through `Arrange`. Relationships are
//! straight horizontal and vertical lines from a referenced column's row to the
//! referencing column's row, routed around the other cards (`routing.rs`) and labelled
//! `1 — ∞` or `1 — 1`. To create a relationship, press on the right end of a referenced
//! column's row and release on the left end of a referencing column's row (or click one
//! end, then the other). The row ends are invisible grab zones that tint on hover.
//!
//! The view pans by dragging the background and zooms with the mouse wheel around the
//! pointer, between 50% and 140%. It fits the diagram on first show, whenever the set of
//! tables changes, and when the view is resized until the user zooms or pans. Controls in
//! the bottom-left corner zoom and fit. When the view has room, a minimap in the
//! bottom-right corner shows the whole diagram and recentres the view where it is pressed.
//!
//! Test ids: `node:<table>` (label = table name; selected when active), `node-head:<table>`
//! (the drag handle), `field:<table>:<column>`, `handle:<table>:<index>:source|target`,
//! `edge-label:<n>`, `zoom-in`, `zoom-out`, `fit-view`, `minimap` and `diagram-empty`.

use super::{
    layout::{default_layout, LayoutNode, Position},
    relationships::{drawn_from, handle_id, relationship_edges, Connection, Direction, DrawnRelationship},
    routing::{halfway, route, Rect, View},
};
use crate::archive::DocumentConfig;
use crate::data::TableSchema;
use crate::ui::prelude::*;
use gpui_kit::{
    canvas, fill, point, size, Bounds, CursorStyle, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathBuilder, Pixels, ScrollDelta, ScrollWheelEvent,
};
use serde_json::{json, Map, Value};
use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;

pub const NODE_WIDTH: f32 = 240.0;
const HEADER: f32 = 40.0;
const FIELD: f32 = 24.0;
/// Width of the grab zone at each end of a column row.
const GRAB: f32 = 12.0;
/// Background dot spacing at 100%.
const DOT_GAP: f32 = 20.0;
const MINI_W: f32 = 200.0;
const MINI_H: f32 = 140.0;
const INSET: f32 = 12.0;
const ZOOM_STEP: f32 = 1.2;

pub type NodePositions = BTreeMap<String, Position>;

pub enum DiagramEvent {
    Select(String),
    Arrange(NodePositions),
    Relate(DrawnRelationship),
}

/// The saved node positions (`navigationState.relationshipLayout`).
pub fn read_positions(config: &DocumentConfig) -> NodePositions {
    config
        .navigation_state
        .get("relationshipLayout")
        .and_then(Value::as_object)
        .map(|layout| {
            layout
                .iter()
                .filter_map(|(name, p)| {
                    Some((
                        name.clone(),
                        Position {
                            x: p.get("x")?.as_f64()? as f32,
                            y: p.get("y")?.as_f64()? as f32,
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn write_positions(config: &mut DocumentConfig, positions: &NodePositions) {
    let mut navigation = match config.navigation_state.take() {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let layout: Map<String, Value> = positions
        .iter()
        .map(|(name, p)| (name.clone(), json!({"x": p.x, "y": p.y})))
        .collect();
    navigation.insert("relationshipLayout".into(), Value::Object(layout));
    config.navigation_state = Value::Object(navigation);
}


struct Drag {
    table: String,
    mouse: gpui_kit::Point<Pixels>,
    origin: Position,
    moved: bool,
}

struct Pan {
    mouse: gpui_kit::Point<Pixels>,
    origin: (f32, f32),
}

/// Where the minimap draws the diagram: `mini = offset + (diagram - world.origin) * scale`.
#[derive(Clone, Copy)]
struct Minimap {
    world: Rect,
    scale: f32,
    offset: (f32, f32),
}

impl Minimap {
    fn to_mini(&self, p: (f32, f32)) -> (f32, f32) {
        (
            self.offset.0 + (p.0 - self.world.x) * self.scale,
            self.offset.1 + (p.1 - self.world.y) * self.scale,
        )
    }

    fn to_diagram(&self, m: (f32, f32)) -> (f32, f32) {
        (
            self.world.x + (m.0 - self.offset.0) / self.scale,
            self.world.y + (m.1 - self.offset.1) / self.scale,
        )
    }
}

pub struct RelationshipBrowser {
    schemas: Vec<TableSchema>,
    selected: String,
    saved: NodePositions,
    // Positions being dragged, until the saved ones catch up.
    local: HashMap<String, Position>,
    drag: Option<Drag>,
    connecting: Option<(String, usize)>,
    relatable: bool,
    view: View,
    // The tables and viewport size the view was last fitted to. A different set of tables
    // fits again, and so does a resize until the user zooms or pans.
    fitted: Option<(Vec<String>, (f32, f32))>,
    moved_view: bool,
    pan: Option<Pan>,
    minimap_drag: bool,
    // The viewport's bounds in the window, recorded at each paint.
    frame: Rc<Cell<Option<Bounds<Pixels>>>>,
}
impl EventEmitter<DiagramEvent> for RelationshipBrowser {}

fn card_rect(at: Position, columns: usize) -> Rect {
    Rect {
        x: at.x,
        y: at.y,
        w: NODE_WIDTH,
        h: HEADER + columns as f32 * FIELD,
    }
}

fn handle_point(at: Position, index: usize, direction: Direction) -> (f32, f32) {
    let x = match direction {
        Direction::Source => at.x + NODE_WIDTH,
        Direction::Target => at.x,
    };
    (x, at.y + HEADER + index as f32 * FIELD + FIELD / 2.0)
}

impl RelationshipBrowser {
    pub fn new() -> Self {
        Self {
            schemas: vec![],
            selected: String::new(),
            saved: NodePositions::new(),
            local: HashMap::new(),
            drag: None,
            connecting: None,
            relatable: true,
            view: View::default(),
            fitted: None,
            moved_view: false,
            pan: None,
            minimap_drag: false,
            frame: Rc::new(Cell::new(None)),
        }
    }

    pub fn set_data(
        &mut self,
        schemas: Vec<TableSchema>,
        selected: &str,
        saved: NodePositions,
        cx: &mut Context<Self>,
    ) {
        self.schemas = schemas;
        self.selected = selected.to_string();
        self.saved = saved;
        if self.drag.is_none() {
            self.local.clear();
        }
        cx.notify();
    }

    /// Where each table sits: dragged, else saved, else the default layout.
    pub fn positions(&self) -> HashMap<String, Position> {
        let nodes: Vec<LayoutNode> = self.schemas.iter().map(LayoutNode::of).collect();
        let defaults = default_layout(&nodes);
        self.schemas
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let at = self
                    .local
                    .get(&s.name)
                    .or_else(|| self.saved.get(&s.name))
                    .copied()
                    .unwrap_or(defaults[i]);
                (s.name.clone(), at)
            })
            .collect()
    }

    fn cards(&self, positions: &HashMap<String, Position>) -> Vec<(String, Rect)> {
        self.schemas
            .iter()
            .filter_map(|s| Some((s.name.clone(), card_rect(*positions.get(&s.name)?, s.columns.len()))))
            .collect()
    }

    /// Each relationship's route, in diagram coordinates, in `relationship_edges` order.
    fn routes(&self, positions: &HashMap<String, Position>) -> Vec<(Option<String>, Vec<(f32, f32)>)> {
        let cards: Vec<Rect> = self.cards(positions).into_iter().map(|(_, r)| r).collect();
        relationship_edges(&self.schemas)
            .into_iter()
            .filter_map(|edge| {
                let parent = positions.get(&edge.source)?;
                let child = positions.get(&edge.target)?;
                let parse = |h: &str| h.trim_start_matches('c').split('-').next()?.parse::<usize>().ok();
                let from = handle_point(*parent, parse(&edge.source_handle)?, Direction::Source);
                let to = handle_point(*child, parse(&edge.target_handle)?, Direction::Target);
                Some((edge.label, route(from, to, &cards)))
            })
            .collect()
    }

    fn frame_size(&self) -> Option<(f32, f32)> {
        self.frame
            .get()
            .map(|b| (f32::from(b.size.width), f32::from(b.size.height)))
            .filter(|s| s.0 > 0.0 && s.1 > 0.0)
    }

    /// A window position relative to the viewport's top-left corner.
    fn local_point(&self, p: gpui_kit::Point<Pixels>) -> (f32, f32) {
        let origin = self.frame.get().map_or(point(px(0.), px(0.)), |b| b.origin);
        (f32::from(p.x - origin.x), f32::from(p.y - origin.y))
    }

    fn content(&self, positions: &HashMap<String, Position>) -> Option<Rect> {
        self.cards(positions).into_iter().map(|(_, r)| r).reduce(|a, b| a.union(&b))
    }

    /// Shows the whole diagram.
    pub fn fit_view(&mut self, cx: &mut Context<Self>) {
        let (Some(size), Some(content)) = (self.frame_size(), self.content(&self.positions())) else {
            return;
        };
        self.view = View::fit(content, size);
        self.fitted = Some((self.schemas.iter().map(|s| s.name.clone()).collect(), size));
        self.moved_view = false;
        cx.notify();
    }

    /// Zooms by `factor` around the middle of the view.
    pub fn zoom_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        let (w, h) = self.frame_size().unwrap_or((0.0, 0.0));
        self.view.zoom_at(factor, (w / 2.0, h / 2.0));
        self.moved_view = true;
        cx.notify();
    }

    fn wheel(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let dy = match event.delta {
            ScrollDelta::Pixels(p) => f32::from(p.y),
            ScrollDelta::Lines(l) => l.y * 20.0,
        };
        if dy == 0.0 {
            return;
        }
        let anchor = self.local_point(event.position);
        self.view.zoom_at((dy * 0.002).exp(), anchor);
        self.moved_view = true;
        cx.notify();
    }

    #[cfg(test)]
    pub fn view(&self) -> View {
        self.view
    }

    #[cfg(test)]
    pub fn visible(&self) -> Option<Rect> {
        Some(self.view.visible(self.frame_size()?))
    }

    #[cfg(test)]
    pub fn card(&self, table: &str) -> Option<Rect> {
        let positions = self.positions();
        self.cards(&positions).into_iter().find(|(n, _)| n == table).map(|(_, r)| r)
    }

    #[cfg(test)]
    pub fn route_points(&self) -> Vec<Vec<(f32, f32)>> {
        self.routes(&self.positions()).into_iter().map(|(_, r)| r).collect()
    }

    fn minimap(&self, positions: &HashMap<String, Position>, size: (f32, f32)) -> Option<Minimap> {
        let world = self.content(positions)?.union(&self.view.visible(size));
        let world = Rect { x: world.x - 20.0, y: world.y - 20.0, w: world.w + 40.0, h: world.h + 40.0 };
        let scale = (MINI_W / world.w).min(MINI_H / world.h);
        let offset = ((MINI_W - world.w * scale) / 2.0, (MINI_H - world.h * scale) / 2.0);
        Some(Minimap { world, scale, offset })
    }

    /// Centres the view on the diagram point under a window position in the minimap.
    fn minimap_to(&mut self, position: gpui_kit::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(size) = self.frame_size() else {
            return;
        };
        let Some(map) = self.minimap(&self.positions(), size) else {
            return;
        };
        let local = self.local_point(position);
        let mini = (local.0 - (size.0 - MINI_W - INSET), local.1 - (size.1 - MINI_H - INSET));
        self.view.center_on(map.to_diagram(mini), size);
        self.moved_view = true;
        cx.notify();
    }

    fn press_background(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let at = self.view.to_diagram(self.local_point(event.position));
        if self.cards(&self.positions()).iter().any(|(_, r)| r.contains(at)) {
            return;
        }
        self.pan = Some(Pan { mouse: event.position, origin: self.view.pan });
        cx.notify();
    }

    fn start_drag(&mut self, table: &str, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let origin = self.positions().get(table).copied().unwrap_or(Position { x: 0.0, y: 0.0 });
        self.drag = Some(Drag {
            table: table.to_string(),
            mouse: event.position,
            origin,
            moved: false,
        });
        cx.notify();
    }

    fn move_pointer(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.pan = None;
            self.minimap_drag = false;
            return;
        }
        if self.minimap_drag {
            self.minimap_to(event.position, cx);
        } else if let Some(pan) = &self.pan {
            let dx = f32::from(event.position.x - pan.mouse.x);
            let dy = f32::from(event.position.y - pan.mouse.y);
            self.view.pan = (pan.origin.0 + dx, pan.origin.1 + dy);
            self.moved_view = true;
            cx.notify();
        } else {
            self.move_drag(event, cx);
        }
    }

    fn move_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let zoom = self.view.zoom;
        let Some(drag) = &mut self.drag else {
            return;
        };
        let dx = f32::from(event.position.x - drag.mouse.x) / zoom;
        let dy = f32::from(event.position.y - drag.mouse.y) / zoom;
        if dx != 0.0 || dy != 0.0 {
            drag.moved = true;
        }
        let at = Position {
            x: (drag.origin.x + dx).max(0.0),
            y: (drag.origin.y + dy).max(0.0),
        };
        self.local.insert(drag.table.clone(), at);
        cx.notify();
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.moved {
            if let Some(at) = self.local.get(&drag.table).copied() {
                let mut next = self.saved.clone();
                next.insert(drag.table.clone(), at);
                cx.emit(DiagramEvent::Arrange(next));
            }
        } else {
            self.local.remove(&drag.table);
        }
        cx.notify();
    }

    fn finish_connection(&mut self, child: &str, child_index: usize, cx: &mut Context<Self>) {
        let Some((parent, parent_index)) = self.connecting.take() else {
            return;
        };
        let source = handle_id(parent_index, Direction::Source);
        let target = handle_id(child_index, Direction::Target);
        let drawn = drawn_from(
            &Connection {
                source: Some(&parent),
                target: Some(child),
                source_handle: Some(&source),
                target_handle: Some(&target),
            },
            &self.schemas,
        );
        if let Some(drawn) = drawn {
            cx.emit(DiagramEvent::Relate(drawn));
        }
        cx.notify();
    }

    #[cfg(test)]
    pub fn connecting(&self) -> Option<&(String, usize)> {
        self.connecting.as_ref()
    }
}

impl Default for RelationshipBrowser {
    fn default() -> Self {
        Self::new()
    }
}

impl Render for RelationshipBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let names: Vec<String> = self.schemas.iter().map(|s| s.name.clone()).collect();
        if let (Some(size), Some(content)) = (self.frame_size(), self.content(&self.positions())) {
            let refit = match &self.fitted {
                None => true,
                Some((tables, at)) => *tables != names || (*at != size && !self.moved_view),
            };
            if refit {
                self.view = View::fit(content, size);
                self.fitted = Some((names, size));
                self.moved_view = false;
            }
        }
        let positions = self.positions();
        let view = self.view;
        let z = view.zoom;
        let theme = cx.theme();
        let (border, muted, accent, fg, bg, secondary) = (
            theme.border,
            theme.muted_foreground,
            theme.primary,
            theme.foreground,
            theme.background,
            theme.secondary,
        );
        let routes = self.routes(&positions);
        let entity = cx.entity();

        // The background dots, which also record the viewport's bounds for pointer math.
        let frame = self.frame.clone();
        let frame_entity = entity.clone();
        let dots = canvas(
            move |bounds: Bounds<Pixels>, _, cx| {
                let resized = frame.get().map(|b| b.size) != Some(bounds.size);
                frame.set(Some(bounds));
                if resized {
                    // Fit or re-place once the size is known.
                    let entity = frame_entity.clone();
                    cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
                }
            },
            move |bounds: Bounds<Pixels>, _, window, _| {
                let mut gap = DOT_GAP * z;
                while gap < 14.0 {
                    gap *= 2.0;
                }
                let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
                let mut y = view.pan.1.rem_euclid(gap);
                while y < h {
                    let mut x = view.pan.0.rem_euclid(gap);
                    while x < w {
                        let at = point(bounds.origin.x + px(x), bounds.origin.y + px(y));
                        window.paint_quad(fill(Bounds::new(at, size(px(1.5), px(1.5))), border));
                        x += gap;
                    }
                    y += gap;
                }
            },
        )
        .absolute()
        .size_full();

        let paint_routes: Vec<Vec<(f32, f32)>> = routes.iter().map(|(_, r)| r.clone()).collect();
        let lines = canvas(
            |_, _, _| (),
            move |bounds: Bounds<Pixels>, _, window, _| {
                for points in &paint_routes {
                    let at = |p: (f32, f32)| {
                        let s = view.to_screen(p);
                        point(bounds.origin.x + px(s.0), bounds.origin.y + px(s.1))
                    };
                    let mut path = PathBuilder::stroke(px(1.5));
                    path.move_to(at(points[0]));
                    for p in &points[1..] {
                        path.line_to(at(*p));
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, accent);
                    }
                }
            },
        )
        .absolute()
        .size_full();

        let nodes: Vec<AnyElement> = self
            .schemas
            .iter()
            .map(|schema| {
                let name = schema.name.clone();
                let at = positions[&name];
                let screen = view.to_screen((at.x, at.y));
                let active = name == self.selected;
                let head_name = name.clone();
                let click_name = name.clone();
                let fields: Vec<AnyElement> = schema
                    .columns
                    .iter()
                    .enumerate()
                    .map(|(i, column)| {
                        let tag = if column.primary_key_position > 0 {
                            "PK"
                        } else if schema.foreign_keys.iter().any(|k| k.from_columns.contains(&column.name)) {
                            "FK"
                        } else {
                            ""
                        };
                        let hot_source = self.connecting.as_ref() == Some(&(name.clone(), i));
                        let (down_name, up_name) = (name.clone(), name.clone());
                        let source_entity = entity.clone();
                        let (target_up, target_click) = (entity.clone(), entity.clone());
                        let (up_name2, click_name2) = (up_name.clone(), up_name.clone());
                        let tint = accent.opacity(0.35);
                        let grab = |id: String| {
                            div()
                                .id(SharedString::from(id))
                                .w(px(GRAB * z))
                                .h_full()
                                .flex_none()
                                .cursor(CursorStyle::Crosshair)
                                .hover(move |s| s.bg(tint))
                        };
                        h_flex()
                            .id(SharedString::from(format!("field:{name}:{}", column.name)))
                            .h(px(FIELD * z))
                            .gap(px(4. * z))
                            .items_center()
                            .text_size(px(14. * z))
                            .child(
                                grab(format!("handle:{name}:{i}:target"))
                                    .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                                        let child = up_name2.clone();
                                        target_up.update(cx, |this, cx| this.finish_connection(&child, i, cx));
                                        cx.stop_propagation();
                                    })
                                    .on_click(move |_, _, cx| {
                                        let child = click_name2.clone();
                                        target_click.update(cx, |this, cx| this.finish_connection(&child, i, cx));
                                        cx.stop_propagation();
                                    })
                                    .test_support(),
                            )
                            .child(div().flex_1().truncate().child(column.name.clone()))
                            .child(div().text_size(px(12. * z)).text_color(muted).child(tag))
                            .child(
                                grab(format!("handle:{name}:{i}:source"))
                                    .when(hot_source, |el| el.bg(accent.opacity(0.6)))
                                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                        let table = down_name.clone();
                                        source_entity.update(cx, |this, cx| {
                                            this.connecting = Some((table, i));
                                            cx.notify();
                                        });
                                        cx.stop_propagation();
                                    })
                                    .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                                    .on_click(|_, _, cx| cx.stop_propagation())
                                    .test_support(),
                            )
                            .into_any_element()
                    })
                    .collect();
                let (head_entity, card_entity) = (entity.clone(), entity.clone());
                v_flex()
                    .id(SharedString::from(format!("node:{name}")))
                    .aria_label(name.clone())
                    .absolute()
                    .left(px(screen.0))
                    .top(px(screen.1))
                    .w(px(NODE_WIDTH * z))
                    .rounded(px(6. * z))
                    .border_1()
                    .border_color(if active { accent } else { border })
                    .bg(bg)
                    .shadow_sm()
                    .overflow_hidden()
                    .child(
                        h_flex()
                            .id(SharedString::from(format!("node-head:{head_name}")))
                            .aria_label(head_name.clone())
                            .h(px(HEADER * z))
                            .px(px(8. * z))
                            .gap(px(8. * z))
                            .items_center()
                            .bg(secondary)
                            .text_size(px(16. * z))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                                let table = head_name.clone();
                                head_entity.update(cx, |this, cx| this.start_drag(&table, event, cx));
                            })
                            .child(Icon::new(IconName::Table2).with_size(px(14. * z)))
                            .child(div().font_weight(FontWeight::SEMIBOLD).truncate().child(name.clone()))
                            .test_support(),
                    )
                    .children(fields)
                    .on_click(move |_, _, cx| {
                        // A click outside the grab zones and header (a field row) selects the table.
                        let table = click_name.clone();
                        card_entity.update(cx, |this, cx| {
                            if this.drag.as_ref().is_none_or(|d| !d.moved) {
                                cx.emit(DiagramEvent::Select(table));
                            }
                        });
                    })
                    .test_support()
                    .into_any_element()
            })
            .collect();

        let edge_labels = routes.iter().enumerate().filter_map(|(n, (label, points))| {
            let label = label.clone()?;
            let (x, y) = view.to_screen(halfway(points));
            Some(
                div()
                    .absolute()
                    .left(px(x - 18. * z))
                    .top(px(y - 9. * z))
                    .px(px(4. * z))
                    .rounded_sm()
                    .bg(bg)
                    .text_color(fg)
                    .text_size(px(12. * z))
                    .child(widgets::text(SharedString::from(format!("edge-label:{n}")), label)),
            )
        });

        let controls = v_flex()
            .absolute()
            .left(px(INSET))
            .bottom(px(INSET))
            .gap_1()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                Button::new("zoom-in")
                    .outline()
                    .small()
                    .icon(IconName::ZoomIn)
                    .tooltip("Zoom in")
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(ZOOM_STEP, cx))),
            )
            .child(
                Button::new("zoom-out")
                    .outline()
                    .small()
                    .icon(IconName::ZoomOut)
                    .tooltip("Zoom out")
                    .on_click(cx.listener(|this, _, _, cx| this.zoom_by(1.0 / ZOOM_STEP, cx))),
            )
            .child(
                Button::new("fit-view")
                    .outline()
                    .small()
                    .icon(IconName::Scan)
                    .tooltip("Fit to view")
                    .on_click(cx.listener(|this, _, _, cx| this.fit_view(cx))),
            );

        // The minimap shows only when it leaves most of the view free.
        let roomy = self.frame_size().filter(|s| s.0 >= MINI_W * 3.0 && s.1 >= MINI_H * 2.5);
        let minimap = roomy.and_then(|size| {
            let map = self.minimap(&positions, size)?;
            let cards = self.cards(&positions).into_iter().map(|(table, r)| {
                let (x, y) = map.to_mini((r.x, r.y));
                div()
                    .absolute()
                    .left(px(x))
                    .top(px(y))
                    .w(px(r.w * map.scale))
                    .h(px(r.h * map.scale))
                    .rounded_sm()
                    .bg(if table == self.selected { accent } else { border })
            });
            let seen = view.visible(size);
            let (vx, vy) = map.to_mini((seen.x, seen.y));
            let down = entity.clone();
            Some(
                div()
                    .id("minimap")
                    .absolute()
                    .right(px(INSET))
                    .bottom(px(INSET))
                    .w(px(MINI_W))
                    .h(px(MINI_H))
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .bg(bg)
                    .overflow_hidden()
                    .cursor_pointer()
                    .children(cards)
                    .child(
                        div()
                            .absolute()
                            .left(px(vx))
                            .top(px(vy))
                            .w(px(seen.w * map.scale))
                            .h(px(seen.h * map.scale))
                            .border_1()
                            .border_color(accent)
                            .bg(accent.opacity(0.08)),
                    )
                    .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                        cx.stop_propagation();
                        down.update(cx, |this, cx| {
                            this.minimap_drag = true;
                            this.minimap_to(event.position, cx);
                        });
                    })
                    .test_support(),
            )
        });

        let (wheel_entity, down_entity, move_entity, up_entity) =
            (entity.clone(), entity.clone(), entity.clone(), entity.clone());
        let empty = self.schemas.is_empty();
        let _ = self.relatable;
        div()
            .id("flow-browser")
            .relative()
            .size_full()
            .min_h_0()
            .overflow_hidden()
            .border_1()
            .border_color(border)
            .rounded_md()
            .bg(bg)
            .when(self.pan.is_some(), |el| el.cursor(CursorStyle::ClosedHand))
            .on_scroll_wheel(move |event, _, cx| wheel_entity.update(cx, |this, cx| this.wheel(event, cx)))
            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                down_entity.update(cx, |this, cx| this.press_background(event, cx))
            })
            .on_mouse_move(move |event, _, cx| move_entity.update(cx, |this, cx| this.move_pointer(event, cx)))
            .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, _, cx| {
                up_entity.update(cx, |this, cx| {
                    this.end_drag(cx);
                    this.pan = None;
                    this.minimap_drag = false;
                    if this.connecting.take().is_some() {
                        cx.notify();
                    }
                })
            })
            .child(dots)
            .child(lines)
            .children(nodes)
            .children(edge_labels)
            .when(!empty, |el| el.child(controls).children(minimap))
            .when(empty, |el| {
                el.child(widgets::empty_state(
                    "diagram-empty",
                    "No tables yet",
                    "Create a table to see it here.",
                    cx,
                ))
            })
    }
}
