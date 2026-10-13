//! Action queries: the query type and target table, and the Preview and Run buttons.
//! Port of src/query/ActionQuery.tsx (docs/decisions/action-queries.md).
//!
//! Test ids: `query-type`, `target-table` (drop-downs), `preview-changes`, `run-action`.

use super::{
    QueryMode,
    action_text::{KIND_LABELS, describe_run, kind_from_id, kind_id, quote, template},
    choice::Opt,
    sql::{run_params, to_named_values},
};
use crate::{
    archive::{ActionSpec, SavedQuery},
    ui::{dialogs, prelude::*, task},
};

impl QueryMode {
    /// Query type and target table. Switching to an action query leaves the visual
    /// builder: action queries are DuckDB SQL.
    pub(super) fn render_type_fields(&mut self, query: &SavedQuery, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = query.action.as_ref().map_or("read", |a| kind_id(a.kind));
        let options: Vec<Opt> = KIND_LABELS.iter().map(|(v, l)| Opt::new(*v, *l)).collect();
        let kind_choice = self
            .choice(
                "query-type",
                kind,
                options,
                |this, value, window, cx| this.set_kind(&value, window, cx),
                cx,
            )
            .render(cx);
        let target = query.action.as_ref().map(|action| {
            let tables = self.tables(cx);
            let mut options = vec![Opt::new("", "Choose a table")];
            if !action.table.is_empty() && !tables.contains(&action.table) {
                options.push(Opt::new(
                    action.table.clone(),
                    format!("{} (missing)", action.table),
                ));
            }
            options.extend(tables.into_iter().map(Opt::same));
            self.choice(
                "target-table",
                action.table.clone(),
                options,
                |this, table, _, cx| this.set_target(table, cx),
                cx,
            )
            .render(cx)
        });
        h_flex()
            .gap_4()
            .items_center()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_sm().child("Query type"))
                    .child(kind_choice),
            )
            .children(target.map(|t| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().text_sm().child("Target table"))
                    .child(t)
            }))
    }

    pub fn set_kind(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = &self.draft else {
            return;
        };
        let query = d.query.clone();
        if value == "read" {
            return self.edit_query(|q| q.action = None, cx);
        }
        let Some(next) = kind_from_id(value) else {
            return;
        };
        let apply = move |this: &mut Self, cx: &mut Context<Self>| {
            let table = query.action.as_ref().map_or(String::new(), |a| a.table.clone());
            let sql = if !query.sql.trim().is_empty() || table.is_empty() {
                query.sql.clone()
            } else {
                template(next, &quote(&table))
            };
            this.edit_query(
                |q| {
                    q.builder = None;
                    q.sql = sql;
                    q.action = Some(ActionSpec { kind: next, table });
                },
                cx,
            );
        };
        let leaving_builder = self.model().is_some_and(|m| !m.is_empty());
        if !leaving_builder {
            return apply(self, cx);
        }
        let answer = dialogs::confirm(
            cx,
            "Leave the visual builder",
            "Action queries are written in SQL. Leave the visual builder?",
        );
        task::after_in(window, cx, answer, move |this, ok, _, cx| {
            if ok {
                apply(this, cx);
            }
        });
    }

    fn set_target(&mut self, table: String, cx: &mut Context<Self>) {
        self.edit_query(
            |q| {
                let Some(action) = q.action.as_mut() else {
                    return;
                };
                if q.sql.trim().is_empty() {
                    q.sql = template(action.kind, &quote(&table));
                }
                action.table = table;
            },
            cx,
        );
    }

    /// Why Preview and Run are unavailable, if they are.
    pub fn action_blocked(&self) -> Option<&'static str> {
        let d = self.draft.as_ref()?;
        let action = d.query.action.as_ref()?;
        if d.dirty() {
            Some("Save the query to run it")
        } else if action.table.is_empty() {
            Some("Choose a target table")
        } else if self.action.busy {
            Some("Running")
        } else {
            None
        }
    }

    /// Preview (a dry run that rolls back) and Run for a saved action query. Both run the
    /// saved definition, so unsaved edits must be saved first.
    pub(super) fn render_action_buttons(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let blocked = self.action_blocked().is_some();
        let tip = self.action_blocked();
        h_flex()
            .gap_2()
            .child(
                Button::new("preview-changes")
                    .outline()
                    .icon(IconName::Eye)
                    .label("Preview changes")
                    .disabled(blocked)
                    .when_some(tip, |b, tip| b.tooltip(tip))
                    .on_click(cx.listener(|this, _, window, cx| this.go_action(true, window, cx))),
            )
            .child(
                Button::new("run-action")
                    .primary()
                    .icon(IconName::Play)
                    .label("Run")
                    .disabled(blocked)
                    .when_some(tip, |b, tip| b.tooltip(tip))
                    .on_click(cx.listener(|this, _, window, cx| this.go_action(false, window, cx))),
            )
    }

    /// Runs the saved action query; `dry_run` rolls the changes back.
    pub fn go_action(&mut self, dry_run: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.action_blocked().is_some() {
            return;
        }
        let Some(d) = &self.draft else {
            return;
        };
        let (id, name) = (d.query.id.clone(), d.query.name.clone());
        let Some(spec) = d.query.action.clone() else {
            return;
        };
        let params = to_named_values(&run_params(&self.values));
        let label = self.label.clone();
        let start = move |this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
            this.action.busy = true;
            this.action.result = None;
            task::spawn_call_in(
                window,
                cx,
                move || crate::records::run_action_query(&label, &id, params, dry_run, 0),
                move |this, result, _, cx| {
                    this.action.busy = false;
                    let announce = match result {
                        Ok(run) => {
                            let text =
                                describe_run(run.changed, run.removed, run.dry_run, &run.table, spec.kind);
                            this.action.result = Some((text, false));
                            !dry_run
                        }
                        Err(e) => {
                            let saved = if e.committed { "The changes were saved, but " } else { "" };
                            this.action.result = Some((format!("{saved}{}", e.error.message), true));
                            e.committed
                        }
                    };
                    if announce {
                        this.shell.update(cx, |s, cx| s.records_changed(cx));
                    }
                    cx.notify();
                },
            );
            cx.notify();
        };
        if dry_run {
            return start(self, window, cx);
        }
        let message = format!(
            "Run {name}? It changes rows in {} and cannot be undone.",
            self.draft
                .as_ref()
                .and_then(|d| d.query.action.as_ref())
                .map_or("", |a| a.table.as_str())
        );
        let answer = dialogs::confirm(cx, "Run action query", &message);
        task::after_in(window, cx, answer, move |this, ok, window, cx| {
            if ok {
                start(this, window, cx);
            }
        });
    }
}
