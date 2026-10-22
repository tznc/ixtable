//! The band canvas of the designer (port of `BandCanvas.tsx`): bands stacked vertically in
//! points (1 pt = 1 px), components positioned absolutely. Pointer drags move and resize
//! components and band bottoms; a focused component takes arrow keys.

use super::designer::{Drag, DragKind, ReportDesigner};
use super::extra;
use crate::report_engine::model::{band_min_height, BandKey};
use crate::reports::{Component, Report};
use crate::ui::prelude::*;
use gpui_kit::hsla;

fn gray(level: f64) -> Hsla {
    hsla(0.0, 0.0, level.clamp(0.0, 1.0) as f32, 1.0)
}

/// A component with the geometry of a drag in progress applied.
fn live(c: &Component, drag: &Option<Drag>) -> Component {
    let mut out = c.clone();
    if let Some(Drag::Component { id, now, .. }) = drag {
        if *id == c.id {
            (out.x, out.y, out.w, out.h) = (now.x, now.y, now.w, now.h);
        }
    }
    out
}

impl ReportDesigner {
    /// Continues a drag to the pointer position.
    fn drag_to(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(report) = self.report(cx) else {
            return;
        };
        let min = match &self.drag {
            Some(Drag::Band { key, .. }) => crate::report_engine::model::get_band(&report, key).map_or(0.0, band_min_height),
            _ => 0.0,
        };
        self.drag_move(pos, min, report.page.content_width(), cx);
    }

    fn component_box(&mut self, c: &Component, key: &BandKey, band_height: f64, selected: bool, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (accent, border) = (theme.primary, theme.border);
        let handle = self.handle(&c.id, cx);
        let (id, label) = (c.id.clone(), extra::canvas_label(c));
        let border_width = extra::style_num(c, "borderWidth");
        let fill = extra::style_num(c, "fill");
        let bold = extra::style_flag(c, "bold");
        let align = extra::style_text(c, "align").unwrap_or_else(|| "left".into());
        let body: AnyElement = match c.kind.as_str() {
            "line" => {
                let width = border_width.unwrap_or(1.0) as f32;
                let vertical = extra::text(c, "orientation") == "vertical";
                let line = div().absolute().bg(gpui_kit::black());
                (if vertical {
                    line.left(px(c.w as f32 / 2.0)).top_0().h_full().w(px(width))
                } else {
                    line.top(px(c.h as f32 / 2.0)).left_0().w_full().h(px(width))
                })
                .into_any_element()
            }
            _ => div().ty(Type::Caption).child(extra::canvas_text(c)).into_any_element(),
        };
        let this_id = id.clone();
        let (down_id, down_key, down_c) = (id.clone(), key.clone(), c.clone());
        let mut el = div()
            .id(SharedString::from(format!("component-{id}")))
            .aria_label(label)
            .track_focus(&handle)
            .absolute()
            .left(px(c.x as f32))
            .top(px(c.y as f32))
            .w(px(c.w as f32))
            .h(px(c.h as f32))
            .overflow_hidden()
            .when(bold, |d| d.font_weight(FontWeight::BOLD))
            .when(align == "center", |d| d.text_center())
            .when(align == "right", |d| d.text_right())
            .when_some(fill.filter(|_| c.kind != "line"), |d, f| d.bg(gray(f)))
            .border_1()
            .border_color(if selected { accent } else { border.opacity(0.5) });
        if c.kind == "rectangle" {
            el = el.border_color(if selected { accent } else { gpui_kit::black() });
        }
        el.on_key_down(cx.listener(move |this, e: &KeyDownEvent, _, cx| {
            if this.component_key(&this_id, e, cx) {
                cx.stop_propagation();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, e: &MouseDownEvent, window, cx| {
                this.select(&down_id, down_key.clone(), cx);
                this.begin_component_drag(DragKind::Move, &down_c, e.position, band_height);
                let handle = this.handle(&down_id, cx);
                window.focus(&handle, cx);
                cx.stop_propagation();
            }),
        )
        .child(body)
        .when(selected, |d| {
            let (resize_id, resize_key, resize_c) = (id.clone(), key.clone(), c.clone());
            d.child(
                widgets::observed(
                    div()
                        .id(SharedString::from(format!("resize-{id}")))
                        .aria_label("Resize")
                        .absolute()
                        .right_0()
                        .bottom_0()
                        .size(px(8.))
                        .bg(accent)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                this.select(&resize_id, resize_key.clone(), cx);
                                this.begin_component_drag(DragKind::Resize, &resize_c, e.position, band_height);
                                cx.stop_propagation();
                            }),
                        ),
                ),
            )
        })
        .test_support()
        .into_any_element()
    }

    pub(super) fn render_canvas(&mut self, report: &Report, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width = report.page.content_width() as f32;
        let active = self.active_band(report);
        let selected = self.selected.clone();
        let drag = self.drag.clone();
        let theme = cx.theme();
        let (border, muted, background, primary) = (theme.border, theme.muted_foreground, theme.background, theme.primary);
        let mut bands = vec![];
        for entry in self.bands_of(report) {
            let min = band_min_height(entry.band);
            let height = match &drag {
                Some(Drag::Band { key, now, .. }) if *key == entry.key => *now,
                _ => entry.band.height,
            };
            let key_id = entry.key.id();
            let (pick_key, grip_key, area_key) = (entry.key.clone(), entry.key.clone(), entry.key.clone());
            let components: Vec<AnyElement> = entry
                .band
                .components
                .iter()
                .map(|c| {
                    let shown = live(c, &drag);
                    let is_selected = selected.as_deref() == Some(c.id.as_str());
                    self.component_box(&shown, &entry.key, height, is_selected, cx)
                })
                .collect();
            let band_height = f64::max(height, min) as f32;
            let label = format!("{} · {} pt", entry.label, height.round());
            bands.push(
                v_flex()
                    .id(SharedString::from(format!("band-group-{key_id}")))
                    .child(
                        Button::new(SharedString::from(format!("band-{key_id}")))
                            .ghost()
                            .xsmall()
                            .start_label(None, label)
                            .selected_tint(entry.key == active, cx)
                            .on_click(cx.listener(move |this, _, _, cx| this.select_band(pick_key.clone(), cx))),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("band-area-{key_id}")))
                            .relative()
                            .w(px(width))
                            .h(px(band_height))
                            .bg(background)
                            .border_1()
                            .border_color(if entry.key == active { primary } else { border })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| this.select_band(area_key.clone(), cx)),
                            )
                            .child(div().children(components)),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("band-grip-{key_id}")))
                            .w(px(width))
                            .h(px(5.))
                            .bg(muted.opacity(0.25))
                            .cursor_row_resize()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                    let height = this
                                        .report(cx)
                                        .and_then(|r| crate::report_engine::model::get_band(&r, &grip_key).map(|b| b.height))
                                        .unwrap_or(0.0);
                                    this.begin_band_drag(grip_key.clone(), height, e.position);
                                    cx.stop_propagation();
                                }),
                            ),
                    ),
            );
        }
        v_flex()
            .id("report-sheet")
            .w(px(width))
            .gap_1()
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if this.drag.is_some() {
                    this.drag_to(e.position, cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, cx| this.drag_end(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _: &MouseUpEvent, _, cx| this.drag_end(cx)))
            .children(bands)
    }
}
