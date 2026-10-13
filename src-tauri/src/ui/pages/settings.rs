//! The dashboard settings panel, shown while no component is selected: the filters and a
//! compact version of the grid settings (`LayoutSettings`): column count, padding, gaps and
//! item alignment. Tracks, named regions and breakpoints are edited in YAML until the
//! Design studio's layout editor is shared.
//!
//! Test ids: `dash-settings` (the panel), `layout-columns`, `layout-padding`,
//! `layout-column-gap`, `layout-row-gap`, `layout-justify`, `layout-align`.

use super::editor::PageEditor;
use crate::{
    page_engine::model::with_layout,
    pages::Page,
    design::{GridAlign, GridTrack},
    ui::{
        prelude::*,
        controls::{choice_field, input},
        reports::fields::pairs,
    },
};

const MAX_COLUMNS: usize = 24;

fn align_key(align: &GridAlign) -> &'static str {
    match align {
        GridAlign::Stretch => "stretch",
        GridAlign::Start => "start",
        GridAlign::Center => "center",
        GridAlign::End => "end",
    }
}

fn align_of(key: &str) -> GridAlign {
    match key {
        "start" => GridAlign::Start,
        "center" => GridAlign::Center,
        "end" => GridAlign::End,
        _ => GridAlign::Stretch,
    }
}

impl PageEditor {
    pub(super) fn on_layout_edit(&mut self, key: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let Ok(number) = text.trim().parse::<f64>() else { return key.starts_with("layout-") };
        let number = number.round().max(0.0) as usize;
        let key = key.to_string();
        let Some(dashboard) = self.dashboard(cx) else { return true };
        let mut layout = dashboard.layout.clone();
        match key.as_str() {
            "layout-columns" => {
                let count = number.clamp(1, MAX_COLUMNS);
                if count == layout.columns.len() {
                    return true;
                }
                layout.columns = vec![GridTrack::fr(1.0); count];
            }
            "layout-padding" => layout.padding = number.min(200) as u16,
            "layout-column-gap" => layout.column_gap = number.min(200) as u16,
            "layout-row-gap" => layout.row_gap = number.min(200) as u16,
            _ => return false,
        }
        if layout == dashboard.layout {
            return true;
        }
        self.edit("Edit dashboard grid", move |d| *d = with_layout(d, layout, &[]), cx);
        true
    }

    fn set_alignment(&mut self, justify: bool, key: &str, cx: &mut Context<Self>) {
        let align = align_of(key);
        self.edit("Edit dashboard grid", move |d| {
            let mut layout = d.layout.clone();
            if justify {
                layout.justify_items = align;
            } else {
                layout.align_items = align;
            }
            d.layout = layout;
        }, cx);
    }

    pub(super) fn render_settings_panel(&mut self, dashboard: &Page, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let layout = &dashboard.layout;
        let number = |this: &mut Self, id: &str, label: &str, value: usize, window: &mut Window, cx: &mut Context<Self>| {
            let state = this.inputs.get(id.to_string(), &value.to_string(), "", window, cx);
            widgets::field(label, input(&state, id.to_string(), label, false), cx)
        };
        let columns = number(self, "layout-columns", "Columns", layout.columns.len(), window, cx);
        let padding = number(self, "layout-padding", "Padding", layout.padding as usize, window, cx);
        let column_gap = number(self, "layout-column-gap", "Column gap", layout.column_gap as usize, window, cx);
        let row_gap = number(self, "layout-row-gap", "Row gap", layout.row_gap as usize, window, cx);
        let aligns = pairs(&[("stretch", "Stretch"), ("start", "Start"), ("center", "Center"), ("end", "End")]);
        let justify = choice_field(
            "layout-justify",
            "Align items horizontally",
            align_key(&layout.justify_items),
            aligns.clone(),
            "Stretch",
            Self::on_pick(cx, |this, v, cx| this.set_alignment(true, v, cx)),
            cx,
        );
        let align = choice_field(
            "layout-align",
            "Align items vertically",
            align_key(&layout.align_items),
            aligns,
            "Stretch",
            Self::on_pick(cx, |this, v, cx| this.set_alignment(false, v, cx)),
            cx,
        );
        let filters = self.render_filters(dashboard, window, cx);
        v_flex()
            .id("dash-settings")
            .gap_3()
            .child(div().text_xs().text_color(muted).child("DASHBOARD SETTINGS"))
            .child(filters)
            .child(
                crate::ui::reports::fields::section("dash-grid-settings", "Page grid", cx)
                    .child(columns)
                    .child(padding)
                    .child(column_gap)
                    .child(row_gap)
                    .child(justify)
                    .child(align),
            )
            .into_any_element()
    }
}
