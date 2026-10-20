//! Impact preview shown before destructive schema changes and table rebuilds (port of
//! `src/schema/ImpactDialog.tsx`): affected rows, dependents, the mode of each
//! operation and the exact SQL that will run.
//!
//! Test ids: `impact-dialog` (label = title), `impact-op:<i>`, `impact-op-mode:<i>`,
//! `impact-op-destructive:<i>`, `impact-rebuild-note`, `impact-fact:rows|referencing|
//! indexes|dependents`, `impact-warning:<i>` (shown first), `impact-show-sql` (toggle),
//! `impact-sql` (only while shown), `impact-error`,
//! `impact-ack` (checkbox), `impact-cancel`, `impact-confirm`.

use crate::ui::controls::{check, error_line, note};
use crate::ui::display::{Tone, status_badge};
use crate::ui::dialog::dialog;
use super::{
    logical::mode_label,
};
use crate::recordstore::{ChangeMode, ChangePlan, TableImpact};
use crate::ui::prelude::*;

pub enum ImpactEvent {
    Confirm,
    Cancel,
}

pub struct ImpactDialog {
    title: String,
    plan: Option<ChangePlan>,
    impact: Option<TableImpact>,
    confirm_label: String,
    acknowledgement: Option<String>,
    pub busy: bool,
    pub error: String,
    pub acknowledged: bool,
    show_sql: bool,
}
impl EventEmitter<ImpactEvent> for ImpactDialog {}

pub fn change_mode_key(mode: &ChangeMode) -> &'static str {
    match mode {
        ChangeMode::InPlace => "inPlace",
        ChangeMode::Rebuild => "rebuild",
        ChangeMode::Unsupported => "unsupported",
    }
}

impl ImpactDialog {
    pub fn new(
        title: String,
        plan: Option<ChangePlan>,
        impact: Option<TableImpact>,
        confirm_label: &str,
        acknowledgement: Option<String>,
    ) -> Self {
        Self {
            title,
            plan,
            impact,
            confirm_label: confirm_label.to_string(),
            acknowledgement,
            busy: false,
            error: String::new(),
            acknowledged: false,
            show_sql: false,
        }
    }

    fn shown(&self) -> Option<&TableImpact> {
        self.impact
            .as_ref()
            .or_else(|| self.plan.as_ref().and_then(|p| p.impact.as_ref()))
    }

    fn destructive(&self) -> bool {
        self.plan.as_ref().is_none_or(|p| p.destructive || p.rebuild)
    }

    pub fn confirm_required(&self) -> bool {
        self.destructive() || self.acknowledgement.is_some()
    }

    pub fn set_state(&mut self, busy: bool, error: String, cx: &mut Context<Self>) {
        self.busy = busy;
        self.error = error;
        cx.notify();
    }

    fn statements(&self) -> Vec<String> {
        match (&self.plan, self.shown()) {
            (Some(plan), _) => plan.statements.clone(),
            (None, Some(shown)) => shown.statements.clone(),
            _ => vec![],
        }
    }
}

fn fact(id: &'static str, label: &str, value: String, cx: &App) -> Div {
    h_flex()
        .gap_3()
        .child(
            div()
                .w(px(180.))
                .ty(Type::Body)
                .text_color(cx.theme().muted_foreground)
                .child(label.to_string()),
        )
        .child(div().flex_1().min_w_0().ty(Type::Body).child(widgets::text(id, value)))
}

impl Render for ImpactDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let required = self.confirm_required();
        let destructive = self.destructive();
        let statements = self.statements();
        let sql = if statements.is_empty() {
            String::new()
        } else {
            format!("{};", statements.join(";\n"))
        };
        let entity = cx.entity();
        let removes_data = self.plan.as_ref().is_none_or(|p| p.destructive);
        let ack_text = if destructive && removes_data {
            "I reviewed the impact. Data this change removes can only be recovered from a checkpoint."
                .to_string()
        } else if destructive {
            "I reviewed the impact. The table is rebuilt; its rows are kept.".to_string()
        } else {
            self.acknowledgement.clone().unwrap_or_default()
        };
        let mut body = v_flex().gap_3();
        if let Some(plan) = &self.plan {
            body = body.children(plan.warnings.iter().enumerate().map(|(i, w)| {
                widgets::callout(
                    SharedString::from(format!("impact-warning:{i}")),
                    widgets::Severity::Warning,
                    w,
                    None,
                    cx,
                )
            }));
        }
        if let Some(plan) = &self.plan {
            body = body.child(v_flex().gap_1().children(plan.operations.iter().enumerate().map(|(i, op)| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().ty(Type::Body).child(widgets::text(
                        SharedString::from(format!("impact-op:{i}")),
                        op.summary.clone(),
                    )))
                    .child(status_badge(SharedString::from(format!("impact-op-mode:{i}")), mode_label(change_mode_key(&op.mode)), Tone::Neutral, cx))
                    .when(op.destructive, |el| {
                        el.child(status_badge(SharedString::from(format!("impact-op-destructive:{i}")), "Destructive", Tone::Danger, cx))
                    })
            })));
            if plan.rebuild {
                body = body.child(note(
                    widgets::text(
                        "impact-rebuild-note",
                        "This store cannot make every change in place, so the table is recreated: its rows are copied into a new table inside one transaction, indexes are recreated, and relationships are checked before committing.",
                    ),
                    cx,
                ));
            }
        }
        if let Some(shown) = self.shown() {
            let referencing = if shown.inbound_foreign_keys.is_empty() {
                "None".to_string()
            } else {
                shown
                    .inbound_foreign_keys
                    .iter()
                    .map(|f| {
                        format!(
                            "{} ({}): {} row{}, on delete {}",
                            f.table,
                            f.columns.join(", "),
                            f.rows,
                            if f.rows == 1 { "" } else { "s" },
                            f.on_delete
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let dependents = if shown.dependents.is_empty() {
                "None".to_string()
            } else {
                shown
                    .dependents
                    .iter()
                    .map(|d| format!("{} \u{201c}{}\u{201d}", d.kind, d.name))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            body = body.child(
                v_flex()
                    .gap_1()
                    .child(fact(
                        "impact-fact:rows",
                        &format!("Rows in {}", shown.table),
                        shown.rows.to_string(),
                        cx,
                    ))
                    .child(fact("impact-fact:referencing", "Tables referencing it", referencing, cx))
                    .child(fact(
                        "impact-fact:indexes",
                        "Indexes",
                        if shown.indexes.is_empty() {
                            "None".into()
                        } else {
                            shown.indexes.join(", ")
                        },
                        cx,
                    ))
                    .child(fact("impact-fact:dependents", "Dependent definitions", dependents, cx)),
            );
        }
        let show_sql = self.show_sql;
        body = body
            .when(!sql.is_empty(), |el| {
                el.child(
                    h_flex().child(
                        Button::new("impact-show-sql")
                            .ghost()
                            .small()
                            .icon(if show_sql { IconName::ChevronDown } else { IconName::ChevronRight })
                            .label(if show_sql { "Hide SQL" } else { "Show SQL" })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_sql = !this.show_sql;
                                cx.notify();
                            })),
                    ),
                )
            })
            .when(show_sql, |el| {
                el.child(
                    div()
                        .p_2()
                        .rounded_md()
                        .bg(cx.theme().muted)
                        .font_family("monospace")
                        .ty(Type::Caption)
                        .child(widgets::text("impact-sql", sql)),
                )
            })
            .when(!self.error.is_empty(), |el| {
                el.child(error_line("impact-error", &self.error, cx))
            })
            .when(required, |el| {
                let entity = entity.clone();
                el.child(check("impact-ack", &ack_text, self.acknowledged, move |value, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.acknowledged = value;
                        cx.notify();
                    })
                }))
            })
;
        let confirm = if required {
            Button::new("impact-confirm").danger()
        } else {
            Button::new("impact-confirm").primary()
        };
        dialog("impact-dialog", &self.title)
            .child(body)
            .footer(
                Button::new("impact-cancel")
                    .outline()
                    .label("Cancel")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ImpactEvent::Cancel))),
            )
            .footer(
                confirm
                    .label(self.confirm_label.clone())
                    .disabled(self.busy || (required && !self.acknowledged))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ImpactEvent::Confirm))),
            )
            .modal()
    }
}
