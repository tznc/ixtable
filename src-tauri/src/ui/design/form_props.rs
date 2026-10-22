//! Properties of the selected form (port of `FormProperties.tsx`): source, modes, list
//! settings, validation rules, events and the form grid; and the grid of a selected
//! section or tab group.
//!
//! Test ids: `form-name`, `source-kind` ("Source kind"), `data-table`, `source-query`,
//! `param-<name>` (a query parameter binding; problem `param-<name>-problem`),
//! `orphan-<name>` and `remove-binding:<name>`, the mode checkboxes `mode-<key>` and
//! `navigation-bar`, `page-size`, `detail-form`, `list-filter`, `list-column:<column>`,
//! `rule-<n>`, `rule-<n>-message`, `remove-rule-<n>`, `add-rule`, `event-<name>` (the action
//! pickers `event-onLoad`, ...). The grid sections are in `layout_settings.rs`.

use super::studio::DesignStudio;
use crate::archive::SavedQuery;
use crate::design::{Control, ControlKind, Form, FormEvents, FormMode, FormRule, FormSource, GridLayout, SourceKind};
use crate::forms::{
    conditions::filter_names,
    operations::set_layout,
    schema::{FORM_MODES, form_mode_key, form_mode_label, form_table, is_collection_mode, new_id},
};
use crate::ui::{automation::action_picker, grid::engine::GridItemRef, prelude::*};

const EVENTS: [(&str, &str); 4] = [
    ("onLoad", "On load"),
    ("onCurrent", "On current"),
    ("beforeUpdate", "Before update"),
    ("afterUpdate", "After update"),
];

fn event_slot<'a>(events: &'a mut FormEvents, name: &str) -> &'a mut Option<String> {
    match name {
        "onLoad" => &mut events.on_load,
        "onCurrent" => &mut events.on_current,
        "beforeUpdate" => &mut events.before_update,
        _ => &mut events.after_update,
    }
}

/// Controls placed directly on a grid (one tab page for tab groups), for layout checks.
fn grid_items(form: &Form, parent: Option<&str>, tab: Option<&str>) -> Vec<(GridItemRef, String)> {
    form.controls
        .iter()
        .filter(|c| {
            c.parent.as_ref().map(|p| p.id.as_str()) == parent
                && tab.is_none_or(|t| c.parent.as_ref().and_then(|p| p.tab.as_deref()) == Some(t))
        })
        .map(|c| (GridItemRef { id: c.id.clone(), placement: c.placement.clone() }, c.label.clone()))
        .collect()
}

impl DesignStudio {
    pub(super) fn render_form_properties(&mut self, form: &Form, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let (danger, muted) = (cx.theme().danger, cx.theme().muted_foreground);
        let columns = self.columns_of(form_table(form), cx);
        let source = form.source.clone();
        let is_query = source.as_ref().is_some_and(|s| s.kind == SourceKind::Query);
        let queries: Vec<SavedQuery> = self.config(cx).saved_queries.iter().filter(|q| q.action.is_none()).cloned().collect();
        let bound_query = source
            .as_ref()
            .filter(|_| is_query)
            .and_then(|s| queries.iter().find(|q| Some(&q.id) == s.query_id.as_ref()))
            .cloned();
        // Bindings left behind by a renamed or removed parameter; the design checks flag them.
        let declared: Vec<String> = bound_query.iter().flat_map(|q| q.parameters.iter().map(|p| p.name.clone())).collect();
        let orphans: Vec<String> = source
            .as_ref()
            .filter(|_| is_query)
            .map(|s| s.params.keys().filter(|n| !declared.contains(n)).cloned().collect())
            .unwrap_or_default();

        let mut panel = v_flex().gap_2();
        panel = panel.child(self.text_field(
            "form-name",
            "Form name",
            &form.name,
            "",
            |this, text, cx| this.edit_form("Rename form", move |f| f.name = text, cx),
            window,
            cx,
        ));

        // Source kind and what it reads.
        let first_query = queries.first().map(|q| q.id.clone()).unwrap_or_default();
        panel = panel.child(self.pick(
            "source-kind",
            "Source",
            match source.as_ref().map(|s| s.kind) {
                Some(SourceKind::Query) => "query",
                Some(SourceKind::Table) => "table",
                None => "",
            },
            super::studio::options(&[("", "Unbound"), ("table", "Table"), ("query", "Saved query (read-only)")]),
            "Unbound",
            move |this, kind, cx| {
                let kind = kind.to_string();
                let first_query = first_query.clone();
                this.edit_form(
                    "Edit form",
                    move |f| {
                        f.source = match kind.as_str() {
                            "query" => Some(FormSource { kind: SourceKind::Query, query_id: Some(first_query), ..Default::default() }),
                            "table" => Some(FormSource { kind: SourceKind::Table, table: Some(String::new()), ..Default::default() }),
                            _ => None,
                        };
                        if kind == "query" {
                            f.modes.retain(|m| matches!(m, FormMode::List | FormMode::Detail));
                        }
                    },
                    cx,
                )
            },
            cx,
        ));
        if !is_query {
            let mut choices = vec![(String::new(), "Unbound".to_string())];
            choices.extend(
                self.shell.read(cx).objects.iter().filter(|o| o.object_type == "table" || o.object_type == "view").map(|o| (o.name.clone(), o.name.clone())),
            );
            panel = panel.child(self.pick(
                "data-table",
                "Data table",
                source.as_ref().and_then(|s| s.table.as_deref()).unwrap_or(""),
                choices,
                "Unbound",
                |this, table, cx| {
                    let table = table.to_string();
                    this.edit_form(
                        "Edit form",
                        move |f| {
                            f.source = (!table.is_empty())
                                .then(|| FormSource { kind: SourceKind::Table, table: Some(table), ..Default::default() })
                        },
                        cx,
                    )
                },
                cx,
            ));
        } else {
            let choices = queries.iter().map(|q| (q.id.clone(), q.name.clone())).collect();
            panel = panel.child(self.pick(
                "source-query",
                "Saved query",
                source.as_ref().and_then(|s| s.query_id.as_deref()).unwrap_or(""),
                choices,
                "",
                |this, id, cx| {
                    let id = id.to_string();
                    this.edit_form(
                        "Edit form",
                        move |f| f.source = Some(FormSource { kind: SourceKind::Query, query_id: Some(id), ..Default::default() }),
                        cx,
                    )
                },
                cx,
            ));
        }
        if is_query && (!declared.is_empty() || !orphans.is_empty()) {
            let mut group = self.group("query-parameters", "Query parameters", cx);
            for p in bound_query.iter().flat_map(|q| q.parameters.iter()) {
                let name = p.name.clone();
                let value = source.as_ref().and_then(|s| s.params.get(&p.name)).map(String::as_str);
                group = group.child(self.expression(
                    &format!("param-{}", p.name),
                    &format!("${}", p.name),
                    value,
                    &[],
                    Some(vec!["app".to_string(), "params".to_string()]),
                    if p.required { "Required" } else { "Default value" },
                    move |this, expr, cx| {
                        let name = name.clone();
                        this.edit_form(
                            "Bind query parameter",
                            move |f| {
                                if let Some(s) = f.source.as_mut() {
                                    match expr.filter(|e| !e.trim().is_empty()) {
                                        Some(e) => s.params.insert(name, e),
                                        None => s.params.remove(&name),
                                    };
                                }
                            },
                            cx,
                        )
                    },
                    window,
                    cx,
                ));
            }
            for name in orphans {
                let target = name.clone();
                group = group.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_1().min_w_0().child(widgets::styled_text(
                            SharedString::from(format!("orphan-{name}")),
                            format!("${name} is no longer a parameter of this query."),
                            move |d| d.ty(Type::Caption).text_color(danger),
                        )))
                        .child(
                            Button::new(SharedString::from(format!("remove-binding:{name}")))
                                .danger()
                                .outline()
                                .small()
                                .label("Remove")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let target = target.clone();
                                    this.edit_form(
                                        "Remove query parameter binding",
                                        move |f| {
                                            if let Some(s) = f.source.as_mut() {
                                                s.params.remove(&target);
                                            }
                                        },
                                        cx,
                                    )
                                })),
                        ),
                );
            }
            panel = panel.child(group);
        }

        // Modes.
        let mut modes = self.group("form-modes", "Modes", cx);
        for mode in FORM_MODES {
            let key = form_mode_key(mode);
            let disabled = is_query && matches!(mode, FormMode::Create | FormMode::Edit);
            modes = modes.child(
                self.toggle(
                    &format!("mode-{key}"),
                    &format!("{} mode", form_mode_label(mode)),
                    form.modes.contains(&mode),
                    move |this, on, cx| {
                        this.edit_form(
                            "Edit form",
                            move |f| {
                                f.modes = if on {
                                    FORM_MODES.into_iter().filter(|m| *m == mode || f.modes.contains(m)).collect()
                                } else {
                                    f.modes.iter().copied().filter(|m| *m != mode).collect()
                                }
                            },
                            cx,
                        )
                    },
                    cx,
                )
                .disabled(disabled),
            );
        }
        modes = modes.child(self.toggle(
            "navigation-bar",
            "Record navigation bar",
            form.navigation_bar,
            |this, on, cx| this.edit_form("Toggle navigation bar", move |f| f.navigation_bar = on, cx),
            cx,
        ));
        panel = panel.child(modes);
        let basics = crate::ui::section::section("form-basics", "Basics", true, window, cx).child(panel).render(cx);
        let mut panel = v_flex().gap_2().child(basics);

        if form.modes.iter().copied().any(is_collection_mode) {
            panel = panel.child(self.render_list_settings(form, &columns, window, cx));
        }
        panel = panel.child(self.render_rules(form, &columns, window, cx));

        // Events: a query-sourced form is read-only, so it never saves.
        let set_count = EVENTS.iter().filter(|(name, _)| event_slot(&mut form.events.clone(), name).is_some()).count();
        let mut events = crate::ui::section::section("form-events", "Events", false, window, cx).summary(if set_count == 0 {
            "No actions run on this form.".to_string()
        } else {
            format!("{set_count} of {} events run an action.", EVENTS.len())
        });
        let actions = self.config(cx).actions.clone();
        for (name, label) in EVENTS {
            if is_query && !matches!(name, "onLoad" | "onCurrent") {
                continue;
            }
            let mut stored = form.events.clone();
            let value = event_slot(&mut stored, name).clone();
            let entity = cx.entity();
            events = events.child(action_picker(
                format!("event-{name}"),
                label,
                value.as_deref(),
                None,
                &actions,
                move |action, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.edit_form(
                            &format!("Set {} action", label.to_lowercase()),
                            move |f| *event_slot(&mut f.events, name) = action,
                            cx,
                        )
                    });
                },
                cx,
            ));
        }
        panel = panel.child(events.render(cx));
        let _ = muted;

        let items = grid_items(form, None, None);
        let layout = form.layout.clone();
        panel.child(self.layout_settings(
            "grid",
            "Advanced layout",
            &layout,
            &items,
            |this, layout, renames, cx| {
                this.edit_form("Edit grid", move |f| *f = set_layout(f, None, layout, &renames), cx)
            },
            window,
            cx,
        ))
    }

    fn render_list_settings(&mut self, form: &Form, columns: &[String], window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let page_size = self.draft(
            "page-size",
            "Rows per page",
            &form.page_size.to_string(),
            "",
            false,
            |this, text, cx| {
                let Some(n) = text.trim().parse::<f64>().ok().filter(|n| n.is_finite()) else {
                    return "Enter a whole number.".to_string();
                };
                let size = (n.round().max(1.0) as u32).min(500);
                this.edit_form("Edit form", move |f| f.page_size = size, cx);
                String::new()
            },
            window,
            cx,
        );
        let mut choices = vec![(String::new(), "This form".to_string())];
        choices.extend(self.design(cx).forms.iter().filter(|f| f.id != form.id).map(|f| (f.id.clone(), f.name.clone())));
        let detail = self.pick(
            "detail-form",
            "Row opens",
            form.detail_form_id.as_deref().unwrap_or(""),
            choices,
            "This form",
            |this, id, cx| {
                let id = id.to_string();
                this.edit_form("Edit form", move |f| f.detail_form_id = (!id.is_empty()).then_some(id), cx)
            },
            cx,
        );
        let filter = self.expression(
            "list-filter",
            "Row filter",
            form.filter.as_deref(),
            &[],
            Some(filter_names(columns)),
            "record.status = 'open'",
            |this, filter, cx| this.edit_form("Edit list filter", move |f| f.filter = filter, cx),
            window,
            cx,
        );
        let mut group = crate::ui::section::section("list-settings", "List", true, window, cx).child(page_size).child(detail).child(filter);
        let all: Vec<String> = columns.to_vec();
        for column in columns {
            let (name, all) = (column.clone(), all.clone());
            group = group.child(self.toggle(
                &format!("list-column:{column}"),
                &format!("List column {column}"),
                form.list_columns.contains(column),
                move |this, on, cx| {
                    let (name, all) = (name.clone(), all.clone());
                    this.edit_form(
                        "Edit form",
                        move |f| {
                            f.list_columns = if on {
                                all.iter().filter(|c| **c == name || f.list_columns.contains(c)).cloned().collect()
                            } else {
                                f.list_columns.iter().filter(|c| **c != name).cloned().collect()
                            }
                        },
                        cx,
                    )
                },
                cx,
            ));
        }
        group.render(cx)
    }

    fn render_rules(&mut self, form: &Form, columns: &[String], window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let mut group = crate::ui::section::section("form-rules", "Form validation", false, window, cx)
            .count(form.rules.len())
            .summary(if form.rules.is_empty() { "No rules. Expand to add one." } else { "Rules must pass before a record saves." });
        for (index, rule) in form.rules.iter().enumerate() {
            let n = index + 1;
            let rule_id = rule.id.clone();
            let expression = self.expression(
                &format!("rule-{n}"),
                &format!("Rule {n}"),
                Some(rule.expression.as_str()).filter(|e| !e.is_empty()),
                columns,
                None,
                "record.end >= record.start",
                {
                    let rule_id = rule_id.clone();
                    move |this, text, cx| {
                        let rule_id = rule_id.clone();
                        this.edit_form(
                            "Edit form",
                            move |f| {
                                if let Some(r) = f.rules.iter_mut().find(|r| r.id == rule_id) {
                                    r.expression = text.unwrap_or_default();
                                }
                            },
                            cx,
                        )
                    }
                },
                window,
                cx,
            );
            let message = self.text_field(
                &format!("rule-{n}-message"),
                &format!("Rule {n} message"),
                &rule.message,
                "",
                {
                    let rule_id = rule_id.clone();
                    move |this, text, cx| {
                        let rule_id = rule_id.clone();
                        this.edit_form(
                            "Edit form",
                            move |f| {
                                if let Some(r) = f.rules.iter_mut().find(|r| r.id == rule_id) {
                                    r.message = text;
                                }
                            },
                            cx,
                        )
                    }
                },
                window,
                cx,
            );
            let remove = super::ctl::icon_button(format!("remove-rule-{n}"), IconName::Trash, &format!("Remove rule {n}")).on_click(
                cx.listener(move |this, _, _, cx| {
                    let rule_id = rule_id.clone();
                    this.edit_form("Edit form", move |f| f.rules.retain(|r| r.id != rule_id), cx)
                }),
            );
            group = group.child(v_flex().gap_1().child(expression).child(message).child(remove));
        }
        group.child(
            Button::new("add-rule")
                .outline()
                .small()
                .icon(IconName::Plus)
                .label("Add rule")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.edit_form(
                        "Edit form",
                        |f| f.rules.push(FormRule { id: new_id(), expression: String::new(), message: String::new() }),
                        cx,
                    )
                })),
        )
        .render(cx)
    }

    /// The grid settings of a selected section or tab group.
    pub(super) fn render_container_grid(
        &mut self,
        form: &Form,
        control: &Control,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if !control.kind.is_container() {
            return None;
        }
        let layout: GridLayout = control.layout.clone()?;
        let tab = (control.kind == ControlKind::Tabs).then(|| {
            self.active_tabs.get(&control.id).cloned().or_else(|| control.tabs.first().map(|t| t.id.clone()))
        });
        let items = grid_items(form, Some(&control.id), tab.as_ref().map(|t| t.as_deref().unwrap_or("")));
        let id = control.id.clone();
        Some(self.layout_settings(
            "container",
            "Container layout",
            &layout,
            &items,
            move |this, layout, renames, cx| {
                let id = id.clone();
                this.edit_form("Edit grid", move |f| *f = set_layout(f, Some(&id), layout, &renames), cx)
            },
            window,
            cx,
        ))
    }
}
