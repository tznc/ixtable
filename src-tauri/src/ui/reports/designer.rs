//! The band designer: palette, band canvas and properties panel for one report (port of
//! `ReportDesigner.tsx`). Edits go through `ConfigStore::update` with the same labels the
//! React designer used, so undo steps and coalescing behave alike.
//!
//! Test ids:
//! - palette `add-<kind>` (`add-staticText`, `add-field`, `add-calculated`, `add-image`,
//!   `add-line`, `add-rectangle`, `add-table`, `add-subreport`, `add-chart`) and the
//!   hint `table-hint`;
//! - canvas: band label buttons `band-<band key>` (`band-detail`, `band-groupHeader:<id>`),
//!   bands `band-area-<key>`, components `component-<id>` (label "<Kind> <summary>"),
//!   resize handles `resize-<id>`, band grips `band-grip-<key>`;
//! - `report-problems` and `report-problem-N`;
//! - the properties panel is in `props.rs`, `props_kinds.rs` and `settings.rs`.

use super::extra;
use super::fields::Inputs;
use crate::archive::Attachment;
use crate::report_engine::model::{
    band_entries, clamp_geometry, embeddable_reports, find_component, get_band, is_page_band, new_component,
    report_problems, with_band, BandKey, Geometry,
};
use crate::reports::{Component, Report};
use crate::ui::{prelude::*, shell::Shell};
use serde_json::Value;
use std::collections::HashMap;

/// Component kinds in palette order: (kind, button label).
pub const PALETTE: [(&str, &str); 9] = [
    ("staticText", "Add text"),
    ("field", "Add field"),
    ("calculated", "Add calculated"),
    ("image", "Add image"),
    ("line", "Add line"),
    ("rectangle", "Add rectangle"),
    ("table", "Add table"),
    ("subreport", "Add subreport"),
    ("chart", "Add chart"),
];

/// Kinds that page headers and footers can't hold.
pub const BODY_ONLY: [&str; 2] = ["table", "subreport"];

pub const TABLE_HINT: &str = "Tables and subreports are not supported in page headers or footers.";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragKind {
    Move,
    Resize,
}

/// A pointer drag of a component or of a band's bottom edge.
#[derive(Clone, Debug)]
pub enum Drag {
    Component {
        kind: DragKind,
        id: String,
        start: (f32, f32),
        origin: Geometry,
        now: Geometry,
        band_height: f64,
    },
    Band {
        key: BandKey,
        start_y: f32,
        origin: f64,
        now: f64,
    },
}

pub struct ReportDesigner {
    pub(super) shell: Entity<Shell>,
    pub report_id: String,
    pub band_key: BandKey,
    pub selected: Option<String>,
    /// Columns of the report's dataset, for the field pickers.
    pub columns: Vec<String>,
    pub(super) assets: Vec<Attachment>,
    assets_loaded: bool,
    /// A component to select on the first render (a Problems link).
    pending_focus: Option<String>,
    pub(super) inputs: Inputs<ReportDesigner, String>,
    handles: HashMap<String, FocusHandle>,
    pub drag: Option<Drag>,
    _subscription: Subscription,
}

pub fn parse_number(text: &str, min: f64) -> Option<f64> {
    let n: f64 = text.trim().parse().ok()?;
    (text.trim() != "" && n.is_finite() && n >= min).then_some(n)
}

impl ReportDesigner {
    pub fn new(shell: Entity<Shell>, report_id: &str, focus: Option<String>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            report_id: report_id.to_string(),
            band_key: BandKey::Detail,
            selected: None,
            columns: vec![],
            assets: vec![],
            assets_loaded: false,
            pending_focus: focus,
            inputs: Inputs::new(Self::on_edit),
            handles: HashMap::new(),
            drag: None,
            _subscription: subscription,
        }
    }

    /// The columns of the dataset (set by the mode when they change).
    pub fn set_columns(&mut self, columns: Vec<String>, cx: &mut Context<Self>) {
        if self.columns != columns {
            self.columns = columns;
            cx.notify();
        }
    }

    pub fn report(&self, cx: &App) -> Option<Report> {
        self.shell
            .read(cx)
            .config(cx)
            .reports
            .iter()
            .find(|r| r.id == self.report_id)
            .cloned()
    }

    pub fn all_reports(&self, cx: &App) -> Vec<Report> {
        self.shell.read(cx).config(cx).reports.clone()
    }

    /// Applies `mutate` to this report as one undo step named `label`.
    pub fn change(&mut self, label: &str, mutate: impl FnOnce(&mut Report), cx: &mut Context<Self>) {
        let id = self.report_id.clone();
        let store = self.shell.read(cx).store.clone();
        let result = store.update(
            cx,
            |s, cx| {
                s.update(
                    label,
                    move |config| {
                        if let Some(report) = config.reports.iter_mut().find(|r| r.id == id) {
                            mutate(report);
                        }
                    },
                    cx,
                )
            },
        );
        if let Err(e) = result {
            self.shell.update(cx, |s, cx| s.set_error(Some(e), cx));
        }
        cx.notify();
    }

    /// Changes one component; its band grows to hold it.
    pub fn update_component(
        &mut self,
        id: &str,
        label: &str,
        mutate: impl FnOnce(&mut Component),
        cx: &mut Context<Self>,
    ) {
        let id = id.to_string();
        self.change(
            label,
            move |report| {
                let Some(key) = find_component(report, &id).map(|(key, _)| key) else {
                    return;
                };
                with_band(report, &key, |band| {
                    if let Some(c) = band.components.iter_mut().find(|c| c.id == id) {
                        mutate(c);
                    }
                    let bottom = band.components.iter().map(|c| c.y + c.h).fold(band.height, f64::max);
                    band.height = bottom;
                });
            },
            cx,
        );
    }

    /// Sets x, y, width or height from a property field, keeping the box inside the band.
    pub fn set_geometry(&mut self, id: &str, field: &str, value: f64, cx: &mut Context<Self>) {
        let Some(report) = self.report(cx) else {
            return;
        };
        let width = report.page.content_width();
        let field = field.to_string();
        self.update_component(
            id,
            "Edit report component",
            move |c| {
                match field.as_str() {
                    "x" => c.x = value,
                    "y" => c.y = value,
                    "w" => c.w = value,
                    _ => c.h = value,
                }
                let g = clamp_geometry(Geometry { x: c.x, y: c.y, w: c.w, h: c.h }, width, f64::INFINITY);
                (c.x, c.y, c.w, c.h) = (g.x, g.y, g.w, g.h);
            },
            cx,
        );
    }

    pub fn select_band(&mut self, key: BandKey, cx: &mut Context<Self>) {
        self.band_key = key;
        self.selected = None;
        cx.notify();
    }

    pub fn select(&mut self, id: &str, key: BandKey, cx: &mut Context<Self>) {
        self.selected = Some(id.to_string());
        self.band_key = key;
        cx.notify();
    }

    /// The band new components go to: the chosen one when the report still has it.
    pub fn active_band(&self, report: &Report) -> BandKey {
        if get_band(report, &self.band_key).is_some() {
            self.band_key.clone()
        } else {
            BandKey::Detail
        }
    }

    /// Whether the palette refuses `kind` in the active band.
    pub fn blocked(&self, report: &Report, kind: &str) -> bool {
        is_page_band(&self.active_band(report)) && BODY_ONLY.contains(&kind)
    }

    /// Adds a component of `kind` to the active band, right of the others when it fits.
    pub fn add(&mut self, kind: &str, cx: &mut Context<Self>) {
        let Some(report) = self.report(cx) else {
            return;
        };
        if self.blocked(&report, kind) {
            return;
        }
        let key = self.active_band(&report);
        let width = report.page.content_width();
        let Some(band) = get_band(&report, &key) else {
            return;
        };
        let right = band.components.iter().map(|c| c.x + c.w).fold(0.0, f64::max);
        let mut component = new_component(kind, width, (right, 0.0));
        if component.x + component.w > width || component.x < right {
            component.x = 0.0;
        }
        self.selected = Some(component.id.clone());
        self.change(
            &format!("Add {kind}"),
            move |r| {
                with_band(r, &key, |b| {
                    b.height = b.height.max(component.y + component.h);
                    b.components.push(component);
                })
            },
            cx,
        );
    }

    pub fn remove(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(report) = self.report(cx) else {
            return;
        };
        let Some((key, _)) = find_component(&report, id) else {
            return;
        };
        self.selected = None;
        let id = id.to_string();
        self.change(
            "Delete report component",
            move |r| with_band(r, &key, |b| b.components.retain(|c| c.id != id)),
            cx,
        );
    }

    /// Arrow keys move the component 1 pt (Shift: 10 pt), Alt+arrows resize it, Delete
    /// removes it. Returns whether the key was handled.
    pub fn component_key(&mut self, id: &str, e: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let key = e.keystroke.key.as_str();
        if key == "delete" || key == "backspace" {
            self.remove(id, cx);
            return true;
        }
        let (dx, dy) = match key {
            "left" => (-1.0, 0.0),
            "right" => (1.0, 0.0),
            "up" => (0.0, -1.0),
            "down" => (0.0, 1.0),
            _ => return false,
        };
        let Some(report) = self.report(cx) else {
            return true;
        };
        let Some((band, c)) = find_component(&report, id) else {
            return true;
        };
        let step = if e.keystroke.modifiers.shift { 10.0 } else { 1.0 };
        let (dx, dy) = (dx * step, dy * step);
        let g = if e.keystroke.modifiers.alt {
            Geometry { x: c.x, y: c.y, w: c.w + dx, h: c.h + dy }
        } else {
            Geometry { x: c.x + dx, y: c.y + dy, w: c.w, h: c.h }
        };
        let height = get_band(&report, &band).map_or(0.0, |b| b.height);
        let g = clamp_geometry(g, report.page.content_width(), height);
        self.update_component(id, "Move report component", move |c| (c.x, c.y, c.w, c.h) = (g.x, g.y, g.w, g.h), cx);
        true
    }

    pub(super) fn handle(&mut self, id: &str, cx: &mut Context<Self>) -> FocusHandle {
        self.handles
            .entry(id.to_string())
            .or_insert_with(|| cx.focus_handle())
            .clone()
    }

    // ---- pointer drags ------------------------------------------------------------

    pub(super) fn begin_component_drag(&mut self, kind: DragKind, c: &Component, pos: Point<Pixels>, band_height: f64) {
        let g = Geometry { x: c.x, y: c.y, w: c.w, h: c.h };
        self.drag = Some(Drag::Component {
            kind,
            id: c.id.clone(),
            start: (pos.x.into(), pos.y.into()),
            origin: g,
            now: g,
            band_height,
        });
    }

    pub(super) fn begin_band_drag(&mut self, key: BandKey, height: f64, pos: Point<Pixels>) {
        self.drag = Some(Drag::Band {
            key,
            start_y: pos.y.into(),
            origin: height,
            now: height,
        });
    }

    pub fn drag_move(&mut self, pos: Point<Pixels>, min_band: f64, width: f64, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        let (x, y): (f32, f32) = (pos.x.into(), pos.y.into());
        match drag {
            Drag::Band { start_y, origin, now, .. } => {
                *now = (*origin + f64::from(y - *start_y)).round().max(min_band);
            }
            Drag::Component { kind, start, origin, now, band_height, .. } => {
                let (dx, dy) = (f64::from(x - start.0), f64::from(y - start.1));
                let next = match kind {
                    DragKind::Move => Geometry { x: origin.x + dx, y: origin.y + dy, ..*origin },
                    DragKind::Resize => Geometry { w: origin.w + dx, h: origin.h + dy, ..*origin },
                };
                *now = clamp_geometry(next, width, *band_height);
            }
        }
        cx.notify();
    }

    pub fn drag_end(&mut self, cx: &mut Context<Self>) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        match drag {
            Drag::Band { key, origin, now, .. } => {
                if now != origin {
                    self.change("Resize band", move |r| with_band(r, &key, |b| b.height = now), cx);
                }
            }
            Drag::Component { id, origin, now, .. } => {
                if now != origin {
                    self.update_component(&id, "Move report component", move |c| {
                        (c.x, c.y, c.w, c.h) = (now.x, now.y, now.w, now.h)
                    }, cx);
                }
            }
        }
        cx.notify();
    }

    /// Reports and queries the properties panel offers.
    pub(super) fn subreport_choices(&self, report: &Report, cx: &App) -> Vec<Report> {
        let all = self.all_reports(cx);
        embeddable_reports(&all, report).into_iter().cloned().collect()
    }

    pub(super) fn problems(&self, report: &Report, cx: &App) -> Vec<String> {
        report_problems(report, &self.all_reports(cx))
    }

    pub(super) fn bands_of<'a>(&self, report: &'a Report) -> Vec<crate::report_engine::model::BandEntry<'a>> {
        band_entries(report)
    }

    /// Loads the application assets for the image picker once.
    fn load_assets(&mut self, cx: &mut Context<Self>) {
        self.assets_loaded = true;
        let label = self.shell.read(cx).label().to_string();
        task::spawn_call(
            cx,
            move || crate::list_attachments(label).unwrap_or_default(),
            |this, assets, cx| {
                this.assets = assets;
                cx.notify();
            },
        );
    }

    /// Selects the component a Problems link asked for.
    fn take_focus(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.pending_focus.take() else {
            return;
        };
        if let Some(report) = self.report(cx) {
            if let Some((key, _)) = find_component(&report, &id) {
                self.select(&id, key, cx);
            }
        }
    }

    pub(super) fn set_extra(&mut self, id: &str, key: &'static str, value: Value, cx: &mut Context<Self>) {
        self.update_component(id, "Edit report component", move |c| extra::set(c, key, value), cx);
    }
}

impl Render for ReportDesigner {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.assets_loaded {
            self.load_assets(cx);
        }
        self.take_focus(cx);
        let Some(report) = self.report(cx) else {
            return widgets::empty_state("designer-missing", "Report not found.", "It may have been deleted.", cx)
                .into_any_element();
        };
        let active = self.active_band(&report);
        let active_label = self
            .bands_of(&report)
            .into_iter()
            .find(|b| b.key == active)
            .map(|b| b.label)
            .unwrap_or_default();
        let page_band = is_page_band(&active);
        let problems = self.problems(&report, cx);
        let theme = cx.theme();
        let (danger, muted, border) = (theme.danger, theme.muted_foreground, theme.border);
        let entity = cx.entity();
        let palette = PALETTE.iter().map(|&(kind, label)| {
            let blocked = page_band && BODY_ONLY.contains(&kind);
            let entity = entity.clone();
            Button::new(SharedString::from(format!("add-{kind}")))
                .outline()
                .small()
                .label(label)
                .opacity(if blocked { 0.5 } else { 1.0 })
                .on_click(move |_, _, cx| entity.update(cx, |this, cx| this.add(kind, cx)))
        });
        let canvas = self.render_canvas(&report, window, cx);
        let props = self.render_properties(&report, window, cx);
        h_flex()
            .id("report-designer")
            .size_full()
            .min_h_0()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap_2()
                    .p_3()
                    .child(h_flex().id("report-palette").flex_wrap().gap_1().children(palette))
                    .when(page_band, |el| {
                        el.child(widgets::styled_text("table-hint", TABLE_HINT, move |d| {
                            d.text_sm().text_color(muted)
                        }))
                    })
                    .child(widgets::styled_text(
                        "canvas-hint",
                        format!(
                            "Adding to: {active_label}. Click a band label to choose it. Drag components to move them and the corner handle to resize. With a component focused, arrow keys move it 1 pt (Shift: 10 pt), Alt+arrow keys resize it, and Delete removes it."
                        ),
                        move |d| d.text_xs().text_color(muted),
                    ))
                    .when(!problems.is_empty(), |el| {
                        el.child(v_flex().id("report-problems").children(problems.iter().enumerate().map(|(i, p)| {
                            widgets::styled_text(format!("report-problem-{}", i + 1), p.clone(), move |d| {
                                d.text_sm().text_color(danger)
                            })
                        })))
                    })
                    .child(
                        div()
                            .id("report-canvas-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_scroll()
                            .child(canvas),
                    ),
            )
            .child(
                div()
                    .id("report-properties")
                    .w(px(360.))
                    .h_full()
                    .flex_none()
                    .overflow_y_scroll()
                    .border_l_1()
                    .border_color(border)
                    .p_3()
                    .child(props),
            )
            .into_any_element()
    }
}
