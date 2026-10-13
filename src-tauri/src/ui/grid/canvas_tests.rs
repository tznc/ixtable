//! Port of tests/unit/grid-canvas.test.tsx. A small host view holds the items; the tests
//! drive the canvas through real pointer and key events.

use super::{GridCanvas, GridItem, track_weights};
use crate::design::{Breakpoint, GridLayout, GridTrack, Placement};
use crate::ui::grid::engine::{SpanConstraints, default_grid_layout};
use crate::ui::widgets;
use gpui_kit::component::v_flex;
use gpui_kit::test::{ElementSnapshot, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, Context, Entity, IntoElement, Point, Render,
    ParentElement, SharedString, Styled, TestAppContext, Window, WindowBounds, WindowOptions, point, px, size,
};

fn at(column: u16, row: u16, column_span: u16, row_span: u16) -> Placement {
    Placement { column, row, column_span, row_span, region: None }
}

fn layout() -> GridLayout {
    GridLayout {
        breakpoints: vec![
            Breakpoint { min_width: 0, columns: vec![GridTrack::fr(1.0); 4] },
            Breakpoint { min_width: 900, columns: default_grid_layout().columns },
        ],
        ..default_grid_layout()
    }
}

struct Item {
    id: &'static str,
    label: &'static str,
    placement: Placement,
    constraints: SpanConstraints,
}

struct Host {
    items: Vec<Item>,
    selected: Option<SharedString>,
    width: Option<f64>,
    resized: Vec<(String, Placement)>,
}

impl Host {
    fn update(&mut self, id: &str, placement: Placement) {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            item.placement = placement;
        }
    }
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let host = cx.entity();
        let (select, resize, mv) = (host.clone(), host.clone(), host.clone());
        let editable = self.width.is_none();
        let mut canvas = GridCanvas::new("canvas", layout())
            .label(if editable { "Form layout" } else { "Narrow layout" })
            .editable(editable)
            .selected(self.selected.clone())
            .on_select(move |id, _, cx| {
                let id = SharedString::from(id.to_string());
                select.update(cx, |h, cx| {
                    h.selected = Some(id);
                    cx.notify();
                });
            })
            .on_resize(move |id, placement, _, cx| {
                resize.update(cx, |h, cx| {
                    h.resized.push((id.to_string(), placement.clone()));
                    h.update(id, placement);
                    cx.notify();
                });
            })
            .on_move(move |id, placement, _, cx| {
                mv.update(cx, |h, cx| {
                    h.update(id, placement);
                    cx.notify();
                });
            });
        if let Some(width) = self.width {
            canvas = canvas.width(width);
        }
        for item in &self.items {
            let p = &item.placement;
            let text = format!(
                "{} at {},{} size {}x{}",
                item.label, p.column, p.row, p.column_span, p.row_span
            );
            canvas = canvas.item(
                GridItem::new(item.id, p.clone(), widgets::text(format!("text:{}", item.id), text))
                    .label(item.label)
                    .constraints(item.constraints),
            );
        }
        v_flex().size_full().child(canvas)
    }
}

struct Win<'a> {
    cx: &'a mut TestAppContext,
    window: AnyWindowHandle,
    host: Entity<Host>,
}

fn open(cx: &mut TestAppContext, items: Vec<Item>, width: Option<f64>, window_width: f32) -> Win<'_> {
    let host = Host { items, selected: None, width, resized: vec![] };
    let (window, host) = cx.update(|cx| {
        gpui_kit::init(cx);
        let bounds = Bounds { origin: Point::default(), size: size(px(window_width), px(700.0)) };
        gpui_kit::open_window(
            WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() },
            cx,
            |_, cx| cx.new(|_| host),
        )
        .expect("open window")
    });
    let mut win = Win { cx, window, host };
    win.settle();
    win
}

impl Win<'_> {
    fn update<R>(&mut self, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        self.cx.update_window(self.window, |_, w, cx| f(w, cx)).expect("window")
    }
    fn settle(&mut self) {
        for _ in 0..3 {
            self.cx.run_until_parked();
            self.update(|w, cx| w.render_frame(cx));
        }
    }
    fn click(&mut self, id: &str) {
        let id = SharedString::from(id.to_string());
        self.update(|w, cx| {
            w.render_frame(cx);
            w.click(id, cx)
        });
        self.settle();
    }
    fn press(&mut self, key: &str) {
        self.update(|w, cx| w.press(key, cx));
        self.settle();
    }
    fn find(&mut self, id: &str) -> ElementSnapshot {
        let id = SharedString::from(id.to_string());
        self.update(|w, _| w.find(id))
    }
    fn label(&mut self, id: &str) -> String {
        self.find(id).label().unwrap_or_default().to_string()
    }
    fn placement(&mut self, id: &str) -> Placement {
        let id = id.to_string();
        self.cx.read(|cx| {
            self.host.read(cx).items.iter().find(|i| i.id == id).expect("item").placement.clone()
        })
    }
    fn resized(&mut self) -> Vec<(String, Placement)> {
        self.cx.read(|cx| self.host.read(cx).resized.clone())
    }
}

fn plain(id: &'static str, label: &'static str, placement: Placement) -> Item {
    Item { id, label, placement, constraints: SpanConstraints::default() }
}

#[gpui_kit::test]
fn resizes_the_selected_item_with_labelled_buttons(cx: &mut TestAppContext) {
    let mut win = open(cx, vec![plain("name", "Name", at(1, 1, 4, 1))], None, 1000.0);
    // Resize controls appear only on the selected item.
    assert!(win.update(|w, _| w.try_find("widen:name")).is_none());
    win.click("name");
    win.click("widen:name");
    win.click("taller:name");
    assert_eq!(win.label("text:name"), "Name at 1,1 size 5x2");
    assert_eq!(win.resized().last(), Some(&("name".to_string(), at(1, 1, 5, 2))));
    win.click("narrow:name");
    win.click("shorter:name");
    assert_eq!(win.label("text:name"), "Name at 1,1 size 4x1");
    // At one row a further "shorter" changes nothing and reports nothing.
    let reports = win.resized().len();
    win.click("shorter:name");
    assert_eq!(win.resized().len(), reports);
    assert_eq!(win.placement("name"), at(1, 1, 4, 1));
}

#[gpui_kit::test]
fn resizes_and_moves_with_alt_and_arrow_keys_inside_declared_constraints(cx: &mut TestAppContext) {
    let notes = Item {
        id: "notes",
        label: "Notes",
        placement: at(1, 1, 2, 1),
        constraints: SpanConstraints {
            min_column_span: Some(2),
            max_column_span: Some(3),
            ..Default::default()
        },
    };
    let mut win = open(cx, vec![notes], None, 1000.0);
    win.click("notes");
    assert_eq!(win.label("resize:notes"), "Resize Notes");
    for _ in 0..3 {
        win.press("alt-right");
    }
    assert_eq!(win.label("text:notes"), "Notes at 1,1 size 3x1");
    for _ in 0..2 {
        win.press("alt-left");
    }
    win.press("alt-down");
    assert_eq!(win.label("text:notes"), "Notes at 1,1 size 2x2");
    win.press("alt-shift-right");
    win.press("alt-shift-down");
    assert_eq!(win.label("text:notes"), "Notes at 2,2 size 2x2");
}

#[gpui_kit::test]
fn applies_the_breakpoint_for_the_measured_width(cx: &mut TestAppContext) {
    let mut wide = open(cx, vec![plain("a", "A", at(1, 1, 12, 1))], None, 1000.0);
    assert_eq!(wide.label("canvas:breakpoint"), "1");
}

#[gpui_kit::test]
fn follows_a_narrower_measured_width(cx: &mut TestAppContext) {
    let mut narrow = open(cx, vec![plain("a", "A", at(1, 1, 12, 1))], None, 600.0);
    assert_eq!(narrow.label("canvas:breakpoint"), "0");
    // The 12-column item wraps onto the four active columns.
    assert!(narrow.find("a").bounds().size.width > px(300.0));
}

#[gpui_kit::test]
fn uses_the_given_width_over_the_measured_one(cx: &mut TestAppContext) {
    let mut win = open(cx, vec![plain("a", "A", at(1, 1, 12, 1))], Some(400.0), 1000.0);
    assert_eq!(win.label("canvas:breakpoint"), "0");
    // Not editable: items are plain children, never labelled groups.
    assert!(win.update(|w, _| w.try_find("a")).is_none());
}

#[gpui_kit::test]
fn snaps_a_pointer_drag_to_whole_tracks(cx: &mut TestAppContext) {
    let mut win = open(cx, vec![plain("city", "City", at(1, 1, 4, 1))], None, 1000.0);
    win.click("city");
    let item = win.find("city").bounds();
    let pitch_x = (item.size.width.as_f32() + 16.0) / 4.0;
    let pitch_y = item.size.height.as_f32() + 16.0;
    let grip = win.find("grip:city").bounds().center();
    let to = point(grip.x + px(pitch_x * 2.0 + 3.0), grip.y + px(pitch_y + 2.0));
    win.update(|w, cx| w.drag(grip, to, cx));
    win.settle();
    assert_eq!(win.resized(), vec![("city".to_string(), at(1, 1, 6, 2))]);
    assert_eq!(win.label("text:city"), "City at 1,1 size 6x2");
}

#[test]
fn weights_follow_fr_values_and_treat_other_tracks_as_one() {
    use crate::design::TrackKind;
    let tracks = vec![
        GridTrack::fr(2.0),
        GridTrack { kind: TrackKind::Fixed, value: Some(120.0), min: None, max: None },
        GridTrack { kind: TrackKind::Content, value: None, min: None, max: None },
        GridTrack::fr(1.0),
    ];
    assert_eq!(track_weights(&tracks), vec![2, 1, 1, 1]);
    assert_eq!(track_weights(&[GridTrack::fr(200.0), GridTrack::fr(200.0)]).iter().sum::<u16>() <= 96, true);
}
