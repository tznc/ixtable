//! Relationship diagram (port of `src/data/RelationshipBrowser.tsx`). Tables are cards
//! that can be dragged; their positions persist through `Arrange`. Relationships are
//! curves from a referenced column to the referencing column, labelled `1 — ∞` or
//! `1 — 1`. To create a relationship, press a referenced column's right handle and
//! release on a referencing column's left handle (or click one handle, then the other).
//!
//! Not ported: wheel zoom, the minimap and the fit-to-view on load (the diagram scrolls
//! instead).
//!
//! Test ids: `node:<table>` (label = table name; selected when active), `node-head:<table>`
//! (the drag handle), `field:<table>:<column>`, `handle:<table>:<index>:source|target`,
//! `edge-label:<n>` and `diagram-empty`.

use super::{
    layout::{default_layout, node_height, LayoutNode, Position},
    relationships::{drawn_from, handle_id, relationship_edges, Connection, Direction, DrawnRelationship},
};
use crate::archive::DocumentConfig;
use crate::data::TableSchema;
use crate::ui::prelude::*;
use gpui_kit::{canvas, point, Bounds, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};

pub const NODE_WIDTH: f32 = 240.0;
const HEADER: f32 = 40.0;
const FIELD: f32 = 24.0;
const MARGIN: f32 = 60.0;

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

pub struct RelationshipBrowser {
    schemas: Vec<TableSchema>,
    selected: String,
    saved: NodePositions,
    // Positions being dragged, until the saved ones catch up.
    local: HashMap<String, Position>,
    drag: Option<Drag>,
    connecting: Option<(String, usize)>,
    relatable: bool,
}
impl EventEmitter<DiagramEvent> for RelationshipBrowser {}

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

    fn move_drag(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let dx: f32 = (event.position.x - drag.mouse.x).into();
        let dy: f32 = (event.position.y - drag.mouse.y).into();
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

fn handle_point(at: Position, index: usize, direction: Direction) -> (f32, f32) {
    let x = match direction {
        Direction::Source => at.x + NODE_WIDTH,
        Direction::Target => at.x,
    };
    (x, at.y + HEADER + index as f32 * FIELD + FIELD / 2.0)
}

impl Render for RelationshipBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let positions = self.positions();
        let edges = relationship_edges(&self.schemas);
        let theme = cx.theme();
        let (border, muted, accent, fg, bg, secondary) = (
            theme.border,
            theme.muted_foreground,
            theme.primary,
            theme.foreground,
            theme.background,
            theme.secondary,
        );
        let mut width = 0.0f32;
        let mut height = 0.0f32;
        for schema in &self.schemas {
            if let Some(at) = positions.get(&schema.name) {
                width = width.max(at.x + NODE_WIDTH);
                height = height.max(at.y + node_height(schema.columns.len()));
            }
        }
        let (width, height) = (width + MARGIN, height + MARGIN);

        // Edge geometry in diagram coordinates: (start, end).
        let curves: Vec<((f32, f32), (f32, f32))> = edges
            .iter()
            .filter_map(|edge| {
                let parent = positions.get(&edge.source)?;
                let child = positions.get(&edge.target)?;
                let parse = |h: &str| h.trim_start_matches('c').split('-').next()?.parse::<usize>().ok();
                let from = handle_point(*parent, parse(&edge.source_handle)?, Direction::Source);
                let to = handle_point(*child, parse(&edge.target_handle)?, Direction::Target);
                Some((from, to))
            })
            .collect();
        let labels: Vec<(usize, String, (f32, f32))> = edges
            .iter()
            .zip(&curves)
            .enumerate()
            .filter_map(|(n, (edge, (from, to)))| {
                edge.label
                    .clone()
                    .map(|label| (n, label, ((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0)))
            })
            .collect();
        let paint_curves = curves.clone();
        let lines = canvas(
            |_, _, _| (),
            move |bounds: Bounds<Pixels>, _, window, _| {
                for (from, to) in &paint_curves {
                    let at = |p: (f32, f32)| point(bounds.origin.x + px(p.0), bounds.origin.y + px(p.1));
                    let reach = ((to.0 - from.0).abs() / 2.0).max(40.0);
                    let mut path = PathBuilder::stroke(px(1.5));
                    path.move_to(at(*from));
                    path.cubic_bezier_to(at(*to), at((from.0 + reach, from.1)), at((to.0 - reach, to.1)));
                    if let Ok(path) = path.build() {
                        window.paint_path(path, accent);
                    }
                }
            },
        )
        .absolute()
        .size_full();

        let entity = cx.entity();
        let nodes: Vec<AnyElement> = self
            .schemas
            .iter()
            .map(|schema| {
                let name = schema.name.clone();
                let at = positions[&name];
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
                        let dot = |id: String| {
                            div()
                                .id(SharedString::from(id))
                                .size(px(10.))
                                .flex_none()
                                .rounded_full()
                                .border_1()
                                .border_color(accent)
                                .cursor_pointer()
                        };
                        h_flex()
                            .id(SharedString::from(format!("field:{name}:{}", column.name)))
                            .h(px(FIELD))
                            .px_1()
                            .gap_2()
                            .items_center()
                            .text_sm()
                            .child(
                                dot(format!("handle:{name}:{i}:target"))
                                    .bg(bg)
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
                            .child(div().text_xs().text_color(muted).child(tag))
                            .child(
                                dot(format!("handle:{name}:{i}:source"))
                                    .bg(if hot_source { accent } else { bg })
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
                    .left(px(at.x))
                    .top(px(at.y))
                    .w(px(NODE_WIDTH))
                    .rounded_md()
                    .border_1()
                    .border_color(if active { accent } else { border })
                    .bg(bg)
                    .shadow_sm()
                    .child(
                        h_flex()
                            .id(SharedString::from(format!("node-head:{head_name}")))
                            .aria_label(head_name.clone())
                            .h(px(HEADER))
                            .px_2()
                            .gap_2()
                            .items_center()
                            .rounded_t_md()
                            .bg(secondary)
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                                let table = head_name.clone();
                                head_entity.update(cx, |this, cx| this.start_drag(&table, event, cx));
                            })
                            .child(gpui_kit::component::Icon::new(IconName::Table2).small())
                            .child(div().font_weight(FontWeight::SEMIBOLD).child(name.clone()))
                            .test_support(),
                    )
                    .children(fields)
                    .on_click(move |_, _, cx| {
                        // A click outside the handles and header (a field row) selects the table.
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

        let edge_labels = labels.into_iter().map(|(n, label, (x, y))| {
            div()
                .absolute()
                .left(px(x - 24.))
                .top(px(y - 9.))
                .px_1()
                .rounded_sm()
                .bg(bg)
                .text_color(fg)
                .text_xs()
                .child(widgets::text(SharedString::from(format!("edge-label:{n}")), label))
        });
        let up_entity = entity.clone();
        let move_entity = entity.clone();
        let empty = self.schemas.is_empty();
        let _ = self.relatable;
        div()
            .id("flow-browser")
            .size_full()
            .min_h_0()
            .overflow_scroll()
            .border_1()
            .border_color(border)
            .rounded_md()
            .on_mouse_move(move |event, _, cx| move_entity.update(cx, |this, cx| this.move_drag(event, cx)))
            .on_mouse_up(MouseButton::Left, move |_: &MouseUpEvent, _, cx| {
                up_entity.update(cx, |this, cx| {
                    this.end_drag(cx);
                    if this.connecting.take().is_some() {
                        cx.notify();
                    }
                })
            })
            .child(
                div()
                    .relative()
                    .w(px(width))
                    .h(px(height))
                    .child(lines)
                    .children(nodes)
                    .children(edge_labels),
            )
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
