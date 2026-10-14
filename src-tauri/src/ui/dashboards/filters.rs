//! Dashboard filters: name, query parameter, control, choices and default value (port of
//! `FiltersEditor.tsx`).
//!
//! Test ids: `add-filter`; per filter (`<fid>` is the filter's id) the group
//! `filter-group:<fid>` (label "Filter <name>") with `filter-name:<fid>`,
//! `filter-param:<fid>`, `filter-control:<fid>`, `filter-type:<fid>`,
//! `filter-choices:<fid>`, `filter-options:<fid>` (a `QueryPicker`), `filter-default:<fid>`,
//! `filter-place:<fid>` and `filter-remove:<fid>`; `filters-hint` while there are none.

use super::editor::DashboardEditor;
use crate::{
    dashboard_engine::model::{FILTER_CONTROLS, control_key, control_label, dashboard_query_parameters, new_filter},
    dashboards::{ComponentKind, Dashboard, DashboardFilter, FilterControl},
    ui::{
        prelude::*,
        controls::{choice_field, input},
    },
};
use serde_json::Value;

const PARAMETER_TYPES: [&str; 6] = ["text", "integer", "number", "boolean", "date", "timestamp"];

fn title(param: &str) -> String {
    let mut chars = param.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Filter".into(),
    }
}

impl DashboardEditor {
    /// Applies `mutate` to the filter `id` as one undo step.
    fn change_filter(&mut self, id: &str, label: &str, mutate: impl FnOnce(&mut DashboardFilter) + 'static, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.edit(
            label,
            move |d| {
                if let Some(filter) = d.filters.iter_mut().find(|f| f.id == id) {
                    mutate(filter);
                }
            },
            cx,
        );
    }

    pub(super) fn on_filter_edit(&mut self, key: &str, text: &str, cx: &mut Context<Self>) -> bool {
        let Some((field, id)) = key.split_once(':').filter(|(f, _)| f.starts_with("filter-")) else { return false };
        let (field, id, text) = (field.to_string(), id.to_string(), text.to_string());
        let Some(current) = self.dashboard(cx).and_then(|d| d.filters.into_iter().find(|f| f.id == id)) else { return true };
        let apply = move |f: &mut DashboardFilter| match field.as_str() {
            "filter-name" => f.name = text.clone(),
            "filter-param" => f.param = text.trim().to_string(),
            "filter-choices" => f.options = text.split(',').map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect(),
            "filter-default" => f.default_value = (!text.is_empty()).then(|| Value::String(text.clone())),
            _ => {}
        };
        let mut next = current.clone();
        apply(&mut next);
        if next != current {
            self.change_filter(&id, "Edit filter", apply, cx);
        }
        true
    }

    fn add_filter(&mut self, cx: &mut Context<Self>) {
        let Some(dashboard) = self.dashboard(cx) else { return };
        let declared = dashboard_query_parameters(&dashboard, &self.shell.read(cx).config(cx).saved_queries);
        let next = declared.iter().find(|(name, _)| !dashboard.filters.iter().any(|f| &f.param == name));
        let mut filter = new_filter(&title(next.map_or("", |(n, _)| n.as_str())), next.map_or("", |(n, _)| n.as_str()));
        if let Some((_, logical)) = next {
            filter.logical_type = logical.clone();
            filter.control = match logical.as_str() {
                "date" => FilterControl::Date,
                "integer" | "number" => FilterControl::Number,
                _ => FilterControl::Select,
            };
        }
        self.edit("Add filter", move |d| d.filters.push(filter), cx);
    }

    fn remove_filter(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.edit(
            "Remove filter",
            move |d| {
                d.filters.retain(|f| f.id != id);
                d.components.retain(|c| c.kind != ComponentKind::Filter || c.filter_id.as_deref().is_none_or(|f| f.is_empty() || f != id));
            },
            cx,
        );
    }

    fn place_filter(&mut self, filter: DashboardFilter, cx: &mut Context<Self>) {
        let name = filter.name.clone();
        self.add(ComponentKind::Filter, move |c| {
            c.filter_id = Some(filter.id);
            c.title = name;
        }, cx);
    }

    pub(super) fn render_filters(&mut self, dashboard: &Dashboard, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let mut panel = v_flex().gap_2().child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child("Filters"));
        if dashboard.filters.is_empty() {
            panel = panel.child(widgets::styled_text("filters-hint", "Filters pass their value to every query parameter with the same name.", move |d| {
                d.text_xs().text_color(muted)
            }));
        }
        for filter in &dashboard.filters {
            let fid = filter.id.clone();
            let name = if !filter.name.is_empty() { filter.name.clone() } else if !filter.param.is_empty() { filter.param.clone() } else { "Filter".into() };
            let name_input = self.inputs.get(format!("filter-name:{fid}"), &filter.name, "", window, cx);
            let param_input = self.inputs.get(format!("filter-param:{fid}"), &filter.param, "", window, cx);
            let controls = FILTER_CONTROLS.iter().map(|c| (control_key(*c).to_string(), control_label(*c).to_string())).collect();
            let id_for_control = fid.clone();
            let control = choice_field(
                format!("filter-control:{fid}"),
                "Control",
                control_key(filter.control),
                controls,
                "Text box",
                Self::on_pick(cx, move |this, v, cx| {
                    let control = FILTER_CONTROLS.iter().copied().find(|c| control_key(*c) == v).unwrap_or(FilterControl::Text);
                    this.change_filter(&id_for_control, "Edit filter", move |f| f.control = control, cx)
                }),
                cx,
            );
            let id_for_type = fid.clone();
            let types = PARAMETER_TYPES.iter().map(|t| (t.to_string(), t.to_string())).collect();
            let logical = choice_field(
                format!("filter-type:{fid}"),
                "Value type",
                &filter.logical_type,
                types,
                "text",
                Self::on_pick(cx, move |this, v, cx| {
                    let v = v.to_string();
                    this.change_filter(&id_for_type, "Edit filter", move |f| f.logical_type = v, cx)
                }),
                cx,
            );
            let mut group = v_flex()
                .id(SharedString::from(format!("filter-group:{fid}")))
                .aria_label(format!("Filter {name}"))
                .gap_2()
                .p_2()
                .border_1()
                .border_color(border)
                .rounded_md()
                .child(div().text_sm().font_weight(FontWeight::MEDIUM).child(name.clone()))
                .child(widgets::field("Filter name", input(&name_input, format!("filter-name:{fid}"), "Filter name", false), cx))
                .child(widgets::field("Parameter", input(&param_input, format!("filter-param:{fid}"), "Parameter", true), cx))
                .child(control)
                .child(logical);
            if filter.control == FilterControl::Select {
                let choices = self.inputs.get(format!("filter-choices:{fid}"), &filter.options.join(", "), "East, West", window, cx);
                let picker = self.picker(
                    &format!("filter-options:{fid}"),
                    "Choices from query",
                    filter.options_query_id.as_deref().unwrap_or(""),
                    |this, key, id, cx| {
                        let fid = key.trim_start_matches("filter-options:").to_string();
                        this.change_filter(&fid, "Edit filter", move |f| f.options_query_id = (!id.is_empty()).then_some(id), cx)
                    },
                    cx,
                );
                group = group
                    .child(widgets::field("Choices", input(&choices, format!("filter-choices:{fid}"), "Choices", false), cx))
                    .child(picker);
            }
            if filter.control != FilterControl::DateRange {
                let shown = match &filter.default_value {
                    None | Some(Value::Null) => String::new(),
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                };
                let default = self.inputs.get(format!("filter-default:{fid}"), &shown, "", window, cx);
                group = group.child(widgets::field("Default value", input(&default, format!("filter-default:{fid}"), "Default value", false), cx));
            }
            let (place_filter, remove_id) = (filter.clone(), fid.clone());
            group = group.child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(SharedString::from(format!("filter-place:{fid}")))
                            .outline()
                            .small()
                            .label(format!("Place {name} on canvas"))
                            .on_click(cx.listener(move |this, _, _, cx| this.place_filter(place_filter.clone(), cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("filter-remove:{fid}")))
                            .outline()
                            .small()
                            .icon(IconName::Trash)
                            .tooltip(format!("Remove filter {name}"))
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_filter(&remove_id, cx))),
                    ),
            );
            panel = panel.child(group.test_support());
        }
        panel.child(
            Button::new("add-filter")
                .outline()
                .small()
                .icon(IconName::Plus)
                .label("Add filter")
                .on_click(cx.listener(|this, _, _, cx| this.add_filter(cx))),
        )
    }
}
