//! One row of labelled controls for a column (port of `src/schema/ColumnFields.tsx`).
//!
//! `label` prefixes every test id: `<label>:name`, `:type`, `:precision`, `:scale`,
//! `:required`, `:unique`, `:default`, `:check`. The type select's items are the
//! logical types the store supports (every type while capabilities load), keeping
//! the column's current one.

use crate::ui::controls::{check, input_text, new_input, on_change, select, set_input, text_input};
use super::{
    columns::ColumnDraft,
    logical::{format_logical, parse_logical, LOGICAL_TYPES},
};
use crate::ui::prelude::*;
use gpui_kit::component::input::InputState;

pub enum ColumnFieldsEvent {
    Changed,
}

pub struct ColumnFields {
    label: String,
    draft: ColumnDraft,
    name_editable: bool,
    types: Option<Vec<String>>,
    max_precision: u32,
    name: Entity<InputState>,
    precision: Entity<InputState>,
    scale: Entity<InputState>,
    default: Entity<InputState>,
    check: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}
impl EventEmitter<ColumnFieldsEvent> for ColumnFields {}

impl ColumnFields {
    pub fn new(
        label: &str,
        draft: ColumnDraft,
        name_editable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let parts = parse_logical(Some(&draft.logical_type));
        let name = new_input(window, cx, "Column name", &draft.name);
        let precision = new_input(window, cx, "Precision", &parts.precision.to_string());
        let scale = new_input(window, cx, "Scale", &parts.scale.to_string());
        let default = new_input(
            window,
            cx,
            "Default",
            &draft.default_expression,
        );
        let check = new_input(window, cx, "Check", &draft.check);
        let subscriptions = vec![
            on_change(cx, &name, |this, cx| {
                this.draft.name = input_text(&this.name, cx);
                this.changed(cx);
            }),
            on_change(cx, &default, |this, cx| {
                this.draft.default_expression = input_text(&this.default, cx);
                this.changed(cx);
            }),
            on_change(cx, &check, |this, cx| {
                this.draft.check = input_text(&this.check, cx);
                this.changed(cx);
            }),
            on_change(cx, &precision, |this, cx| {
                let precision: u32 = input_text(&this.precision, cx).trim().parse().unwrap_or(1);
                this.set_parts(|p| p.precision = precision.max(1), cx);
            }),
            on_change(cx, &scale, |this, cx| {
                let scale: u32 = input_text(&this.scale, cx).trim().parse().unwrap_or(0);
                this.set_parts(|p| p.scale = scale, cx);
            }),
        ];
        Self {
            label: label.to_string(),
            draft,
            name_editable,
            types: None,
            max_precision: 38,
            name,
            precision,
            scale,
            default,
            check,
            _subscriptions: subscriptions,
        }
    }

    pub fn draft(&self) -> &ColumnDraft {
        &self.draft
    }

    /// Limits the type choices to the store's logical types and its decimal precision.
    pub fn set_store(&mut self, types: Option<Vec<String>>, max_precision: u32, cx: &mut Context<Self>) {
        self.types = types;
        self.max_precision = max_precision;
        cx.notify();
    }

    /// Replaces the draft (and the inputs showing it).
    pub fn set_draft(&mut self, draft: ColumnDraft, window: &mut Window, cx: &mut Context<Self>) {
        set_input(&self.name, &draft.name, window, cx);
        set_input(&self.default, &draft.default_expression, window, cx);
        set_input(&self.check, &draft.check, window, cx);
        let parts = parse_logical(Some(&draft.logical_type));
        set_input(&self.precision, &parts.precision.to_string(), window, cx);
        set_input(&self.scale, &parts.scale.to_string(), window, cx);
        self.draft = draft;
        cx.notify();
    }

    fn changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(ColumnFieldsEvent::Changed);
        cx.notify();
    }

    fn set_parts(&mut self, f: impl FnOnce(&mut super::logical::LogicalParts), cx: &mut Context<Self>) {
        let mut parts = parse_logical(Some(&self.draft.logical_type));
        f(&mut parts);
        self.draft.logical_type = format_logical(&parts);
        self.changed(cx);
    }

    fn id(&self, part: &str) -> SharedString {
        format!("{}:{part}", self.label).into()
    }
}

impl Render for ColumnFields {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let parts = parse_logical(Some(&self.draft.logical_type));
        let types = self.types.clone();
        let options: Vec<(String, String)> = LOGICAL_TYPES
            .iter()
            .filter(|(value, _)| {
                types.as_ref().is_none_or(|t| t.iter().any(|x| x == value)) || *value == parts.base
            })
            .map(|(value, label)| (value.to_string(), label.to_string()))
            .collect();
        let entity = cx.entity();
        let max = self.max_precision;
        let muted = cx.theme().muted_foreground;
        h_flex()
            .w_full()
            .gap_2()
            .items_end()
            .when(self.name_editable, |el| {
                el.child(
                    div()
                        .w(px(150.))
                        .child(text_input(&self.name, self.id("name"), &format!("{} name", self.label))),
                )
            })
            .child(select(
                self.id("type"),
                &parts.base,
                options,
                "Type",
                {
                    let entity = entity.clone();
                    move |value, _, cx| {
                        let value = value.to_string();
                        entity.update(cx, |this, cx| {
                            let current = parse_logical(Some(&this.draft.logical_type));
                            this.set_parts(
                                |p| {
                                    p.base = value;
                                    p.precision = current.precision.min(max);
                                },
                                cx,
                            );
                        });
                    }
                },
            ))
            .when(parts.base == "decimal", |el| {
                el.child(
                    v_flex().w(px(70.)).gap_1().child(
                        widgets::styled_text(self.id("precision-label"), "Precision".to_string(), |d| {
                            d.text_sm().text_color(muted)
                        }),
                    ).child(text_input(&self.precision, self.id("precision"), &format!("{} precision", self.label))),
                )
                .child(
                    v_flex().w(px(70.)).gap_1().child(
                        widgets::styled_text(self.id("scale-label"), "Scale".to_string(), |d| {
                            d.text_sm().text_color(muted)
                        }),
                    ).child(text_input(&self.scale, self.id("scale"), &format!("{} scale", self.label))),
                )
            })
            .child(check(self.id("required"), "Required", self.draft.required, {
                let entity = entity.clone();
                move |value, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.draft.required = value;
                        this.changed(cx);
                    })
                }
            }))
            .child(check(self.id("unique"), "Unique", self.draft.unique, {
                let entity = entity.clone();
                move |value, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.draft.unique = value;
                        this.changed(cx);
                    })
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(90.))
                    .child(text_input(&self.default, self.id("default"), &format!("{} default", self.label))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(90.))
                    .child(text_input(&self.check, self.id("check"), &format!("{} check", self.label))),
            )
    }
}
