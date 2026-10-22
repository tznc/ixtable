//! The query editor on the Data page: name, type, builder / SQL tabs, parameters and
//! results. Port of src/query/QueryEditor.tsx.
//!
//! Test ids: `query-name` (input), `query-mode-badge`, `query-unsaved`, `query-run`,
//! `query-save`, `query-duplicate`, `query-delete` (saved queries only), `tab-builder`,
//! `tab-sql`, `generated-sql`, `switch-to-sql` ("Edit as SQL (one-way)"),
//! `start-builder`, `query-note`, `query-results` (labelled Preview or Results).

use super::{QueryMode, Tab, inputs::Field, model::empty_model, sql};
use crate::ui::{dialogs, prelude::*, task, widgets};
use gpui_kit::component::input::Editor;

impl QueryMode {
    /// Keeps the SQL editor's text equal to the draft's SQL.
    pub(super) fn sync_sql_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = self
            .draft
            .as_ref()
            .map(|d| d.query.sql.clone())
            .unwrap_or_default();
        if *self.sql_editor.read(cx).value() != *want {
            self.sql_editor
                .update(cx, |e, cx| e.set_value(want, window, cx));
        }
    }

    pub fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.touched(cx);
    }

    /// Switches a generated query to hand-written SQL (the model is discarded).
    pub fn switch_to_sql(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (sql, _) = self.compiled();
        let apply = move |this: &mut Self, cx: &mut Context<Self>| {
            this.edit_query(
                |q| {
                    q.builder = None;
                    if !sql.is_empty() {
                        q.sql = sql;
                    }
                },
                cx,
            );
            this.tab = Tab::Sql;
            this.touched(cx);
        };
        let answer = dialogs::confirm(
            cx,
            "Edit as SQL",
            "Edit this query as SQL? This is one-way: the visual builder settings are discarded \
             and the query cannot return to the builder.",
        );
        task::after_in(window, cx, answer, move |this, ok, _, cx| {
            if ok {
                apply(this, cx);
            }
        });
    }

    /// Replaces hand-written SQL with an empty visual builder.
    pub fn start_builder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let has_sql = self
            .draft
            .as_ref()
            .is_some_and(|d| !d.query.sql.trim().is_empty());
        let apply = |this: &mut Self, cx: &mut Context<Self>| {
            this.edit_query(
                |q| {
                    q.builder = Some(empty_model().to_value());
                    q.sql = String::new();
                },
                cx,
            );
        };
        if !has_sql {
            return apply(self, cx);
        }
        let answer = dialogs::confirm(
            cx,
            "Start the visual builder",
            "Start the visual builder? The current SQL is replaced.",
        );
        task::after_in(window, cx, answer, move |this, ok, _, cx| {
            if ok {
                apply(this, cx);
            }
        });
    }

    /// Runs the draft's SQL (Run button).
    pub fn execute(&mut self, cx: &mut Context<Self>) {
        let (sql, error) = self.compiled();
        if !error.is_empty() {
            return;
        }
        let Some(d) = &self.draft else {
            return;
        };
        let parameters = d.query.parameters.clone();
        let values = sql::run_params(&self.values);
        self.run_sql(sql, &values, parameters, None, cx);
    }

    pub(super) fn render_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (query, dirty, saving, is_new) = {
            let d = self.draft.as_ref().expect("a draft is open");
            (d.query.clone(), d.dirty(), d.saving, d.is_new)
        };
        let model = self.model();
        let (generated, compile_error) = self.compiled();
        let visual = model.is_some();
        let has_model = model.as_ref().is_some_and(|m| !m.is_empty());
        let action = query.action.clone();
        let badge = if action.is_some() {
            "Action query"
        } else if visual {
            "Visual builder"
        } else {
            "SQL"
        };
        let referenced = match &model {
            Some(m) => super::model::builder_parameter_names(m),
            None => sql::placeholder_names(&query.sql),
        };
        let name_input = self.input_el(Field::Name, "query-name", "Query name", &query.name, "Query name", window, cx);
        let run_button = if action.is_some() {
            self.render_action_buttons(cx).into_any_element()
        } else {
            Button::new("query-run")
                .primary()
                .icon(IconName::Play)
                .label("Run")
                .disabled(self.run.running || !compile_error.is_empty())
                .on_click(cx.listener(|this, _, _, cx| this.execute(cx)))
                .into_any_element()
        };
        let header = h_flex()
            .gap_2()
            .items_center()
            .child(div().flex_1().min_w_0().child(name_input))
            .child(widgets::styled_text("query-mode-badge", badge, |d| {
                d.ty(Type::Caption).font_weight(FontWeight::SEMIBOLD).px_2()
            }))
            .when(dirty, |el| {
                el.child(widgets::styled_text("query-unsaved", "Unsaved changes", |d| d.ty(Type::Caption)))
            })
            .child(run_button)
            .child(
                Button::new("query-save")
                    .outline()
                    .icon(IconName::Save)
                    .label("Save query")
                    .disabled(saving)
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
            .when(!is_new, |el| {
                let (dup_id, del_id) = (query.id.clone(), query.id.clone());
                el.child(
                    Button::new("query-duplicate")
                        .ghost()
                        .icon(IconName::Copy)
                        .label("Duplicate")
                        .on_click(cx.listener(move |this, _, window, cx| this.duplicate(&dup_id, window, cx))),
                )
                .child(
                    Button::new("query-delete")
                        .danger()
                        .outline()
                        .icon(IconName::Trash)
                        .label("Delete")
                        .on_click(cx.listener(move |this, _, window, cx| this.delete(&del_id, window, cx))),
                )
            });

        let this = cx.entity();
        let tabs = action.is_none().then(|| {
            let key = if self.tab == Tab::Builder { "builder" } else { "sql" };
            widgets::tab_strip(
                "query-tabs",
                &[("builder", "Builder"), ("sql", "SQL")],
                key,
                move |key, _, cx| {
                    let tab = if key == "builder" { Tab::Builder } else { Tab::Sql };
                    this.update(cx, |t, cx| t.set_tab(tab, cx));
                },
                cx,
            )
        });

        let panel = if self.tab == Tab::Builder && action.is_none() {
            match &model {
                Some(model) => self.render_builder(model, window, cx).into_any_element(),
                None => v_flex()
                    .gap_2()
                    .items_start()
                    .child(widgets::text("sql-query-note", "This query is written in SQL"))
                    .child(
                        Button::new("start-builder")
                            .outline()
                            .label("Start visual builder")
                            .on_click(cx.listener(|this, _, window, cx| this.start_builder(window, cx))),
                    )
                    .into_any_element(),
            }
        } else if has_model {
            let shown = if generated.is_empty() { compile_error.clone() } else { generated.clone() };
            if *self.generated_editor.read(cx).value() != *shown {
                self.generated_editor
                    .update(cx, |e, cx| e.set_value(shown.clone(), window, cx));
            }
            let lines = shown.lines().count().clamp(3, 14) as f32;
            let editable = !generated.is_empty() && compile_error.is_empty();
            v_flex()
                .gap_2()
                .items_start()
                .child(widgets::observed(
                    div()
                        .id("generated-sql")
                        .aria_label(shown.clone())
                        .w_full()
                        .h(px(lines * 20. + 16.))
                        .child(
                            Editor::new(&self.generated_editor)
                                .h_full()
                                .readonly(true)
                                .aria_label("Generated SQL"),
                        ),
                ))
                .when(editable, |el| {
                    el.child(
                        Button::new("switch-to-sql")
                            .outline()
                            .label("Edit as SQL (one-way)")
                            .tooltip("Turns this query into hand-written SQL. It cannot go back to the builder.")
                            .on_click(cx.listener(|this, _, window, cx| this.switch_to_sql(window, cx))),
                    )
                })
                .into_any_element()
        } else {
            div()
                .h(px(200.))
                .w_full()
                .child(Editor::new(&self.sql_editor).h_full().aria_label("SQL editor"))
                .into_any_element()
        };

        let note = (!compile_error.is_empty() && has_model)
            .then(|| widgets::text("query-note", compile_error.clone()));
        let results = self.render_results(visual, &query, &generated, !is_new && !dirty, cx);
        v_flex()
            .gap_3()
            .child(header)
            .child(self.render_type_fields(&query, cx))
            .children(tabs)
            .child(panel)
            .children(note)
            .child(self.render_parameters_panel(&query, &referenced, window, cx))
            .children(self.render_parameter_values(&query, window, cx))
            .child(results)
    }

    fn render_results(
        &mut self,
        visual: bool,
        query: &crate::archive::SavedQuery,
        generated: &str,
        exact_saved: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if query.action.is_some() {
            let result = self.action.result.clone();
            let apply = self
                .action
                .preview_rows
                .filter(|_| self.action_blocked().is_none());
            return widgets::observed(
                v_flex()
                    .id("action-results")
                    .aria_label("Action query result")
                    .gap_2()
                    .items_start()
                    .children(result.map(|(text, error)| {
                        if error {
                            self.error_line("action-error", &text, cx)
                        } else {
                            widgets::text("action-result", text).into_any_element()
                        }
                    }))
                    .children(apply.map(|n| {
                        Button::new("apply-action")
                            .primary()
                            .label(format!("Apply to {}", widgets::plural(n as usize, "row")))
                            .on_click(cx.listener(|this, _, window, cx| this.go_action(false, window, cx)))
                    })),
            )
            .into_any_element();
        }
        let export = (self.run.has_result() && !self.run.running)
            .then(|| self.render_export(query, generated, exact_saved, cx));
        widgets::observed(
            v_flex()
                .id("query-results")
                .aria_label(if visual { "Preview" } else { "Results" })
                .gap_2()
                .children(export)
                .children(self.render_run_status(cx))
                .children(self.run.has_result().then(|| self.results.clone())),
        )
        .into_any_element()
    }
}
