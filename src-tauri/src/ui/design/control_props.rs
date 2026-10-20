//! Properties of the selected control (port of `ControlProperties.tsx`), including
//! expression fields with live diagnostics.
//!
//! Test ids: `prop-label` ("Label"), `prop-text`, `bound-column` ("Bound column"),
//! `prop-container` ("Container"), the placement drafts `prop-column`, `prop-row`,
//! `prop-column-span`, `prop-row-span` (disabled in a region; problems `<id>-error`),
//! `prop-region` ("Region"), `prop-options` and `prop-options-query`, the lookup
//! `lookup-table`, `lookup-value`, `lookup-display`, `lookup-filter`, `prop-action`
//! (the button action), validation `prop-required`, `prop-min`, `prop-max`, `prop-mask`
//! (hint `prop-mask-hint`), `prop-pattern`, `prop-validation-rule`, `prop-error-message`,
//! behavior `prop-visible-when`, `prop-enabled-when`, `clear-enabled-when`, `prop-computed`,
//! `prop-default`, `prop-format`, `prop-read-only`, `prop-switch` and `delete-control`.
//! Expression problem lines are `<id>-problem`.

use super::studio::DesignStudio;
use crate::design::{Control, ControlKind, ControlParent, Form, GridLayout, Placement, Relationship, SelectOption};
use crate::forms::{
    conditions::filter_names,
    constraints::control_constraints,
    operations::{clamp_placement, move_to_container, remove_control},
    schema::{container_layout, control_kind_label, has_enabled_state, is_input_kind},
    state::known_names,
};
use crate::forms::mask::{mask_issue, mask_template, parse_mask};
use crate::ui::{
    automation::action_picker,
    grid::engine::{GridDelta, SpanConstraints, resize_placement},
    prelude::*,
};

fn parent_key(parent: Option<&ControlParent>) -> String {
    parent.map(|p| format!("{}|{}", p.id, p.tab.as_deref().unwrap_or(""))).unwrap_or_default()
}

/// A column of the placement a draft edits.
#[derive(Clone, Copy)]
enum Span {
    Column,
    Row,
    ColumnSpan,
    RowSpan,
}

impl Span {
    fn get(self, p: &Placement) -> u16 {
        match self {
            Span::Column => p.column,
            Span::Row => p.row,
            Span::ColumnSpan => p.column_span,
            Span::RowSpan => p.row_span,
        }
    }

    fn set(self, p: &mut Placement, value: u16) {
        match self {
            Span::Column => p.column = value,
            Span::Row => p.row = value,
            Span::ColumnSpan => p.column_span = value,
            Span::RowSpan => p.row_span = value,
        }
    }
}

/// Width choices for a grid of `columns`: only the shares that divide it evenly, plus the
/// current span when it is none of them.
pub(super) fn width_options(columns: u16, current: u16) -> Vec<(String, String)> {
    let mut options: Vec<(u16, &str)> = vec![(columns, "Full")];
    for (div, num, label) in [(2, 1, "½"), (3, 1, "⅓"), (4, 1, "¼"), (3, 2, "⅔")] {
        if columns > 1 && columns % div == 0 {
            let span = columns / div * num;
            if !options.iter().any(|(s, _)| *s == span) {
                options.push((span, label));
            }
        }
    }
    if !options.iter().any(|(s, _)| *s == current) {
        options.push((current, "Custom"));
    }
    options.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
    options
        .into_iter()
        .map(|(span, label)| {
            let text = if label == "Custom" { format!("{span} of {columns} columns") } else { label.to_string() };
            (span.to_string(), text)
        })
        .collect()
}

fn options_text(options: &[SelectOption]) -> String {
    options
        .iter()
        .map(|o| if !o.label.is_empty() && o.label != o.value { format!("{}={}", o.value, o.label) } else { o.value.clone() })
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_options(text: &str) -> Vec<SelectOption> {
    text.split('\n')
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (value, label) = line.split_once('=').unwrap_or((line, ""));
            let (value, label) = (value.trim().to_string(), label.trim().to_string());
            SelectOption { label: if label.is_empty() { value.clone() } else { label }, value }
        })
        .collect()
}

fn number_or_none(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok().filter(|n| n.is_finite())
}

impl DesignStudio {
    #[allow(clippy::too_many_arguments)]
    fn span_field(
        &mut self,
        which: Span,
        id: &str,
        label: &str,
        control: &Control,
        grid: &GridLayout,
        limits: SpanConstraints,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let (cid, placement, grid) = (control.id.clone(), control.placement.clone(), grid.clone());
        let columns = grid.columns.len();
        let value = which.get(&control.placement).to_string();
        // Applied on blur or Enter, so typing "10" is not clamped at "1" first.
        self.draft(
            id,
            label,
            &value,
            "",
            control.placement.region.is_some(),
            move |this, text, cx| {
                let Some(parsed) = number_or_none(&text) else {
                    return "Enter a whole number.".to_string();
                };
                let mut next = placement.clone();
                which.set(&mut next, parsed.round().clamp(1.0, f64::from(u16::MAX)) as u16);
                let next = resize_placement(&clamp_placement(&next, columns), GridDelta::default(), &grid, limits);
                this.edit_control(&cid, "Move control", move |c| c.placement = next, cx);
                String::new()
            },
            window,
            cx,
        )
    }

    /// "Width": the column span as a share of the form grid (Full, ½, ⅓, ¼, ⅔).
    fn width_field(&mut self, control: &Control, grid: &GridLayout, limits: SpanConstraints, cx: &mut Context<Self>) -> Div {
        let columns = grid.columns.len() as u16;
        let current = control.placement.column_span;
        let options = width_options(columns, current);
        let (cid, placement, grid) = (control.id.clone(), control.placement.clone(), grid.clone());
        self.pick(
            "prop-width",
            "Width",
            &current.to_string(),
            options,
            "Full",
            move |this, value, cx| {
                let Ok(span) = value.parse::<u16>() else { return };
                let mut next = placement.clone();
                next.column_span = span;
                let next = resize_placement(&clamp_placement(&next, columns as usize), GridDelta::default(), &grid, limits);
                this.edit_control(&cid, "Move control", move |c| c.placement = next, cx);
            },
            cx,
        )
    }

    /// A choice among `columns` (with "—" for none) for one column setting.
    pub(super) fn column_select(
        &self,
        id: &str,
        label: &str,
        value: Option<&str>,
        columns: &[String],
        on_pick: impl Fn(&mut Self, &str, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> Div {
        let mut choices = vec![(String::new(), "—".to_string())];
        choices.extend(columns.iter().map(|c| (c.clone(), c.clone())));
        self.pick(id, label, value.unwrap_or(""), choices, "—", on_pick, cx)
    }

    pub(super) fn render_control_properties(
        &mut self,
        form: &Form,
        control: &Control,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let muted = cx.theme().muted_foreground;
        let cid = control.id.clone();
        let kind = control.kind;
        let input = is_input_kind(kind);
        let columns = self.columns_of(crate::forms::schema::form_table(form), cx);
        let tables = self.tables(cx);
        let grid = container_layout(form, control.parent.as_ref()).clone();
        let limits = control_constraints(kind, grid.columns.len());
        let in_region = control.placement.region.is_some();

        let mut panel = v_flex().gap_2();
        panel = panel
            .child(self.text_field(
                "prop-label",
                "Label",
                &control.label,
                "",
                {
                    let cid = cid.clone();
                    move |this, text, cx| this.edit_control(&cid, "Rename control", move |c| c.label = text, cx)
                },
                window,
                cx,
            ))
            .child(div().ty(Type::Caption).text_color(muted).child(control_kind_label(kind)));

        if kind == ControlKind::Label {
            panel = panel.child(self.text_field(
                "prop-text",
                "Text",
                control.text.as_deref().unwrap_or(""),
                "",
                edit_with!(cid, "Edit control", |c, v| c.text = Some(v)),
                window,
                cx,
            ));
        }
        if input || kind == ControlKind::Computed {
            let mut choices = vec![(String::new(), "Unbound".to_string())];
            choices.extend(columns.iter().map(|c| (c.clone(), c.clone())));
            panel = panel.child(self.pick(
                "bound-column",
                "Column",
                control.binding.as_ref().map_or("", |b| b.column.as_str()),
                choices,
                "Unbound",
                edit_pick!(cid, "Edit control", |c, v| {
                    c.binding = (!v.is_empty()).then(|| crate::design::Binding { table: None, column: v })
                }),
                cx,
            ));
        }

        // Containers the control can move into (itself excluded).
        let mut containers: Vec<(String, String, ControlParent)> = vec![];
        for c in form.controls.iter().filter(|c| c.id != control.id) {
            match c.kind {
                ControlKind::Section => {
                    let parent = ControlParent { id: c.id.clone(), tab: None };
                    containers.push((parent_key(Some(&parent)), c.label.clone(), parent));
                }
                ControlKind::Tabs => {
                    for t in &c.tabs {
                        let parent = ControlParent { id: c.id.clone(), tab: Some(t.id.clone()) };
                        containers.push((parent_key(Some(&parent)), format!("{} › {}", c.label, t.label), parent));
                    }
                }
                _ => {}
            }
        }
        let mut container_options = vec![(String::new(), "Form".to_string())];
        container_options.extend(containers.iter().map(|(k, l, _)| (k.clone(), l.clone())));
        panel = panel.child(self.pick(
            "prop-container",
            "Container",
            &parent_key(control.parent.as_ref()),
            container_options,
            "Form",
            {
                let cid = cid.clone();
                move |this, key, cx| {
                    let parent = containers.iter().find(|(k, _, _)| k == key).map(|(_, _, p)| p.clone());
                    let cid = cid.clone();
                    this.edit_form("Move control", move |f| *f = move_to_container(f, &cid, parent), cx);
                }
            },
            cx,
        ));

        if !in_region {
            panel = panel.child(self.width_field(control, &grid, limits, cx));
        }
        let mut exact = crate::ui::section::section("control-position", "Exact position", false, window, cx)
            .summary(format!(
                "Column {}, row {}, {} × {}",
                control.placement.column, control.placement.row, control.placement.column_span, control.placement.row_span
            ));
        if exact.is_open() {
            let (column, row) = (
                self.span_field(Span::Column, "prop-column", "Column", control, &grid, limits, window, cx),
                self.span_field(Span::Row, "prop-row", "Row", control, &grid, limits, window, cx),
            );
            let (column_span, row_span) = (
                self.span_field(Span::ColumnSpan, "prop-column-span", "Column span", control, &grid, limits, window, cx),
                self.span_field(Span::RowSpan, "prop-row-span", "Row span", control, &grid, limits, window, cx),
            );
            exact = exact
                .child(h_flex().gap_2().child(column).child(row))
                .child(h_flex().gap_2().child(column_span).child(row_span));
        }
        panel = panel.child(exact.render(cx)).when(in_region, |el| {
            el.child(div().ty(Type::Caption).text_color(muted).child("Placed in a region; choose None to set column and row."))
        });
        if let Some(region) = self.region_picker(&grid, control, cx) {
            panel = panel.child(region);
        }

        if matches!(kind, ControlKind::Select | ControlKind::MultiSelect) {
            panel = panel.child(self.area_field(
                "prop-options",
                "Options (one per line, value=label)",
                &options_text(&control.options),
                edit_with!(cid, "Edit control", |c, v| c.options = parse_options(&v)),
                window,
                cx,
            ));
            if kind == ControlKind::Select {
                let mut choices = vec![(String::new(), "None (use the list above)".to_string())];
                choices.extend(
                    self.config(cx).saved_queries.iter().filter(|q| q.action.is_none()).map(|q| (q.id.clone(), q.name.clone())),
                );
                panel = panel.child(self.pick(
                    "prop-options-query",
                    "Options from saved query",
                    control.options_query_id.as_deref().unwrap_or(""),
                    choices,
                    "None (use the list above)",
                    edit_pick!(cid, "Edit control", |c, v| c.options_query_id = (!v.is_empty()).then_some(v)),
                    cx,
                ));
            }
        }
        if kind == ControlKind::Relationship {
            panel = panel.child(self.render_lookup(control, &tables, window, cx));
        }
        if kind == ControlKind::Button {
            let actions = self.config(cx).actions.clone();
            let entity = cx.entity();
            let cid = cid.clone();
            panel = panel.child(action_picker(
                "prop-action",
                "Button action",
                control.action_id.as_deref(),
                None,
                &actions,
                move |id, _, cx| {
                    let cid = cid.clone();
                    entity.update(cx, |this, cx| this.edit_control(&cid, "Edit control", move |c| c.action_id = id, cx));
                },
                cx,
            ));
        }
        if kind == ControlKind::Tabs {
            panel = panel.child(self.render_tabs_properties(form, control, window, cx));
        }
        if kind == ControlKind::RelatedList {
            panel = panel.child(self.render_related_properties(form, control, &columns, &tables, window, cx));
        }
        if kind == ControlKind::Image {
            panel = panel.child(self.render_asset_picker(control, cx));
        }
        if input {
            panel = panel.child(self.render_validation(control, &columns, window, cx));
        }
        panel = panel.child(self.render_behavior(control, &columns, window, cx));
        if input || kind == ControlKind::Computed {
            panel = panel.child(self.render_styles(
                &control.id,
                &control.styles,
                known_names(&columns),
                {
                    let cid = cid.clone();
                    move |this, styles, cx| this.edit_control(&cid, "Edit conditional styles", move |c| c.styles = styles, cx)
                },
                window,
                cx,
            ));
        }
        let delete_id = cid.clone();
        panel.child(
            Button::new("delete-control")
                .danger()
                .outline()
                .small()
                .icon(IconName::Trash)
                .label("Delete control")
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.select(None, cx);
                    let id = delete_id.clone();
                    this.edit_form("Delete control", move |f| *f = remove_control(f, &id), cx);
                })),
        )
    }

    /// Region picker for a placement: "None" keeps the column and row placement.
    pub(super) fn region_picker(&self, layout: &GridLayout, control: &Control, cx: &mut Context<Self>) -> Option<Div> {
        let names: Vec<String> = layout.named_regions.iter().map(|r| r.name.clone()).filter(|n| !n.is_empty()).collect();
        let current = control.placement.region.clone().unwrap_or_default();
        if names.is_empty() && current.is_empty() {
            return None;
        }
        let mut choices = vec![(String::new(), "None (use column and row)".to_string())];
        if !current.is_empty() && !names.contains(&current) {
            choices.push((current.clone(), format!("{current} (missing)")));
        }
        choices.extend(names.iter().map(|n| (n.clone(), n.clone())));
        let cid = control.id.clone();
        let placement = control.placement.clone();
        Some(self.pick(
            "prop-region",
            "Region",
            &current,
            choices,
            "None (use column and row)",
            move |this, value, cx| {
                let next = Placement { region: (!value.is_empty()).then(|| value.to_string()), ..placement.clone() };
                this.edit_control(&cid, "Place control", move |c| c.placement = next, cx);
            },
            cx,
        ))
    }

    fn render_lookup(&mut self, control: &Control, tables: &[String], window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let cid = control.id.clone();
        let relationship = control.relationship.clone();
        let lookup_columns = self.columns_of(relationship.as_ref().map(|r| r.table.as_str()), cx);
        let mut table_options = vec![(String::new(), "—".to_string())];
        table_options.extend(tables.iter().map(|t| (t.clone(), t.clone())));
        let table = self.pick(
            "lookup-table",
            "Lookup table",
            relationship.as_ref().map_or("", |r| r.table.as_str()),
            table_options,
            "—",
            edit_pick!(cid, "Edit control", |c, v| {
                c.relationship = Some(Relationship { table: v, value_column: String::new(), display_column: String::new(), ..Default::default() })
            }),
            cx,
        );
        let value = self.column_select(
            "lookup-value",
            "Value column",
            relationship.as_ref().map(|r| r.value_column.as_str()),
            &lookup_columns,
            {
                let cid = cid.clone();
                move |this, v, cx| {
                    let v = v.to_string();
                    this.edit_control(&cid, "Edit control", move |c| {
                        if let Some(r) = c.relationship.as_mut() {
                            r.value_column = v;
                        }
                    }, cx)
                }
            },
            cx,
        );
        let display = self.column_select(
            "lookup-display",
            "Display column",
            relationship.as_ref().map(|r| r.display_column.as_str()),
            &lookup_columns,
            {
                let cid = cid.clone();
                move |this, v, cx| {
                    let v = v.to_string();
                    this.edit_control(&cid, "Edit control", move |c| {
                        if let Some(r) = c.relationship.as_mut() {
                            r.display_column = v;
                        }
                    }, cx)
                }
            },
            cx,
        );
        let filter = self.expression(
            "lookup-filter",
            "Choice filter",
            relationship.as_ref().and_then(|r| r.filter.as_deref()),
            &[],
            Some(filter_names(&lookup_columns)),
            "record.active and record.region = parent.region",
            {
                let cid = cid.clone();
                move |this, v, cx| {
                    this.edit_control(&cid, "Edit control", move |c| {
                        if let Some(r) = c.relationship.as_mut() {
                            r.filter = v;
                        }
                    }, cx)
                }
            },
            window,
            cx,
        );
        self.group("lookup", "Lookup", cx).child(table).child(value).child(display).child(filter)
    }

    fn render_validation(&mut self, control: &Control, columns: &[String], window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let cid = control.id.clone();
        let validation = control.validation.clone();
        let kind = control.kind;
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let mut group = self.group("validation", "Validation", cx).child(self.toggle(
            "prop-required",
            "Required",
            validation.required,
            {
                let cid = cid.clone();
                move |this, on, cx| this.edit_control(&cid, "Edit validation", move |c| c.validation.required = on, cx)
            },
            cx,
        ));
        if matches!(kind, ControlKind::Number | ControlKind::Decimal) {
            let min = self.text_field(
                "prop-min",
                "Minimum",
                &validation.min.map(|n| n.to_string()).unwrap_or_default(),
                "",
                edit_with!(cid, "Edit validation", |c, v| c.validation.min = number_or_none(&v)),
                window,
                cx,
            );
            let max = self.text_field(
                "prop-max",
                "Maximum",
                &validation.max.map(|n| n.to_string()).unwrap_or_default(),
                "",
                edit_with!(cid, "Edit validation", |c, v| c.validation.max = number_or_none(&v)),
                window,
                cx,
            );
            group = group.child(h_flex().gap_2().child(min).child(max));
        }
        if kind == ControlKind::Text {
            let mask = control.input_mask.clone().unwrap_or_default();
            let issue = (!mask.is_empty()).then(|| mask_issue(&mask)).flatten();
            let hint = (!mask.is_empty()).then(|| match issue {
                Some(issue) => widgets::styled_text("prop-mask-error", issue, move |d| d.ty(Type::Caption).text_color(danger)).into_any_element(),
                None => widgets::styled_text(
                    "prop-mask-hint",
                    format!("Entry looks like {}", mask_template(&parse_mask(&mask))),
                    move |d| d.ty(Type::Caption).text_color(muted),
                )
                .into_any_element(),
            });
            group = group
                .child(self.text_field(
                    "prop-mask",
                    "Input mask",
                    &mask,
                    "e.g. (000) 000-0000",
                    edit_with!(cid, "Set input mask", |c, v| c.input_mask = (!v.is_empty()).then_some(v)),
                    window,
                    cx,
                ))
                .children(hint);
        }
        if matches!(kind, ControlKind::Text | ControlKind::Multiline) {
            group = group.child(self.text_field(
                "prop-pattern",
                "Pattern",
                validation.pattern.as_deref().unwrap_or(""),
                "",
                edit_with!(cid, "Edit validation", |c, v| c.validation.pattern = (!v.is_empty()).then_some(v)),
                window,
                cx,
            ));
        }
        group
            .child(self.expression(
                "prop-validation-rule",
                "Validation rule",
                validation.expression.as_deref(),
                columns,
                None,
                "value > 0",
                edit_with!(cid, "Edit validation", |c, v| c.validation.expression = v),
                window,
                cx,
            ))
            .child(self.text_field(
                "prop-error-message",
                "Error message",
                validation.message.as_deref().unwrap_or(""),
                "",
                edit_with!(cid, "Edit validation", |c, v| c.validation.message = (!v.is_empty()).then_some(v)),
                window,
                cx,
            ))
    }

    fn render_behavior(&mut self, control: &Control, columns: &[String], window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let cid = control.id.clone();
        let kind = control.kind;
        let input = is_input_kind(kind);
        let enabled_state = has_enabled_state(kind);
        let mut group = self.group("behavior", "Behavior", cx).child(self.expression(
            "prop-visible-when",
            "Visible when",
            control.visible_when.as_deref(),
            columns,
            None,
            "",
            edit_with!(cid, "Edit control", |c, v| c.visible_when = v),
            window,
            cx,
        ));
        if enabled_state {
            group = group.child(self.expression(
                "prop-enabled-when",
                "Enabled when",
                control.enabled_when.as_deref(),
                columns,
                None,
                "",
                edit_with!(cid, "Edit control", |c, v| c.enabled_when = v),
                window,
                cx,
            ));
        } else if control.enabled_when.is_some() {
            let cid = cid.clone();
            group = group.child(
                Button::new("clear-enabled-when")
                    .outline()
                    .small()
                    .label("Clear unused \"Enabled when\"")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.edit_control(&cid, "Clear enabled when", |c| c.enabled_when = None, cx)
                    })),
            );
        }
        if input || kind == ControlKind::Computed {
            group = group.child(self.expression(
                "prop-computed",
                if kind == ControlKind::Computed { "Expression" } else { "Computed value" },
                control.computed.as_deref(),
                columns,
                None,
                "",
                edit_with!(cid, "Edit control", |c, v| c.computed = v),
                window,
                cx,
            ));
        }
        if input {
            group = group.child(self.expression(
                "prop-default",
                "Default value",
                control.default_value.as_deref(),
                columns,
                None,
                "",
                edit_with!(cid, "Edit control", |c, v| c.default_value = v),
                window,
                cx,
            ));
        }
        if input || kind == ControlKind::Computed {
            group = group.child(self.text_field(
                "prop-format",
                "Format",
                control.format.as_deref().unwrap_or(""),
                "#,##0.00",
                edit_with!(cid, "Edit control", |c, v| c.format = (!v.is_empty()).then_some(v)),
                window,
                cx,
            ));
        }
        if input {
            group = group.child(self.toggle(
                "prop-read-only",
                "Read only",
                control.read_only,
                edit_with!(cid, "Edit control", |c, v| c.read_only = v),
                cx,
            ));
        }
        if kind == ControlKind::Boolean {
            group = group.child(self.toggle(
                "prop-switch",
                "Show as switch",
                control.variant.as_deref() == Some("toggle"),
                edit_with!(cid, "Edit control", |c, v| c.variant = v.then(|| "toggle".to_string())),
                cx,
            ));
        }
        group
    }
}
