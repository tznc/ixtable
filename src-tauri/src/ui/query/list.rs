//! Saved queries with new, rename, duplicate and delete. Port of src/query/QueryList.tsx.
//!
//! Test ids: `query-new`, `query-open:<id>`, `query-rename:<id>`, `query-duplicate:<id>`,
//! `query-delete:<id>`, `rename-input` (while renaming), `rename-confirm`,
//! `rename-cancel`, `query-draft` (the unsaved new query), `query-list-empty`.

use super::{QueryMode, Renaming};
use crate::ui::prelude::*;
use crate::ui::widgets;
use gpui_kit::component::input::{Input, InputEvent, InputState};

impl QueryMode {
    /// Starts renaming query `id` in the list.
    pub fn start_rename(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.queries(cx).iter().find(|q| q.id == id).map(|q| q.name.clone()) else {
            return;
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let target = id.to_string();
        let subscription = cx.subscribe_in(&input, window, move |this, state, event: &InputEvent, window, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                let name = state.read(cx).value().to_string();
                this.finish_rename(&target, name, window, cx);
            }
        });
        input.update(cx, |s, cx| s.focus(window, cx));
        self.renaming = Some(Renaming {
            id: id.to_string(),
            input,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Confirms a rename: blank names are ignored.
    pub fn finish_rename(&mut self, id: &str, name: String, window: &mut Window, cx: &mut Context<Self>) {
        self.renaming = None;
        let name = name.trim().to_string();
        if !name.is_empty() {
            self.rename(id, name, window, cx);
        }
        cx.notify();
    }

    pub fn cancel_rename(&mut self, cx: &mut Context<Self>) {
        self.renaming = None;
        cx.notify();
    }

    pub(super) fn render_list(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let queries = self.queries(cx).to_vec();
        let active = self.draft.as_ref().filter(|d| !d.is_new).map(|d| d.query.id.clone());
        let draft_label = self.draft.as_ref().filter(|d| d.is_new).map(|d| {
            if d.query.name.is_empty() {
                "Untitled query".to_string()
            } else {
                d.query.name.clone()
            }
        });
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        let rows: Vec<AnyElement> = queries
            .iter()
            .map(|q| {
                let id = q.id.clone();
                if let Some(r) = self.renaming.as_ref().filter(|r| r.id == q.id) {
                    let (confirm_id, cancel_id) = (id.clone(), id.clone());
                    let input = r.input.clone();
                    let confirm_input = input.clone();
                    return h_flex()
                        .gap_1()
                        .child(div().flex_1().child(
                            Input::new(&input)
                                .id("rename-input")
                                .aria_label(format!("New name for {}", q.name)),
                        ))
                        .child(
                            Button::new("rename-confirm")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Check)
                                .tooltip("Confirm rename")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    let name = confirm_input.read(cx).value().to_string();
                                    this.finish_rename(&confirm_id, name, window, cx);
                                })),
                        )
                        .child(
                            Button::new("rename-cancel")
                                .ghost()
                                .xsmall()
                                .icon(IconName::Close)
                                .tooltip("Cancel rename")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let _ = &cancel_id;
                                    this.cancel_rename(cx)
                                })),
                        )
                        .into_any_element();
                }
                let selected = active.as_deref() == Some(q.id.as_str());
                let (open_id, rename_id, dup_id, del_id) = (id.clone(), id.clone(), id.clone(), id.clone());
                let name = if q.name.is_empty() { "Untitled query".to_string() } else { q.name.clone() };
                h_flex()
                    .gap_1()
                    .items_center()
                    .w_full()
                    .overflow_hidden()
                    .child(
                        // The name truncates so the row's icons stay inside the list.
                        div().flex_1().min_w_0().overflow_hidden().child(
                            Button::new(SharedString::from(format!("query-open:{id}")))
                                .ghost()
                                .small()
                                .w_full()
                                .start_label(Some(Icon::new(IconName::CodeXml)), if q.action.is_some() { format!("{name} (action)") } else { name })
                                .selected(selected)
                                .on_click(cx.listener(move |this, _, window, cx| this.open_id(&open_id, window, cx))),
                        ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("query-rename:{id}")))
                            .flex_none()
                            .ghost()
                            .xsmall()
                            .icon(IconName::Pencil)
                            .tooltip(format!("Rename {}", q.name))
                            .on_click(cx.listener(move |this, _, window, cx| this.start_rename(&rename_id, window, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("query-duplicate:{id}")))
                            .flex_none()
                            .ghost()
                            .xsmall()
                            .icon(IconName::Copy)
                            .tooltip(format!("Duplicate {}", q.name))
                            .on_click(cx.listener(move |this, _, window, cx| this.duplicate(&dup_id, window, cx))),
                    )
                    .child(
                        Button::new(SharedString::from(format!("query-delete:{id}")))
                            .flex_none()
                            .ghost()
                            .xsmall()
                            .icon(IconName::Delete)
                            .tooltip(format!("Delete {}", q.name))
                            .on_click(cx.listener(move |this, _, window, cx| this.delete(&del_id, window, cx))),
                    )
                    .into_any_element()
            })
            .collect();
        v_flex()
            .id("query-list")
            .w(px(260.))
            .h_full()
            .flex_none()
            .gap_1()
            .p_2()
            .border_r_1()
            .border_color(border)
            .child(
                h_flex()
                    .px_1()
                    .gap_2()
                    .items_center()
                    .child(widgets::styled_text("queries-heading", "Queries", |d| {
                        d.flex_1().text_sm().font_weight(FontWeight::SEMIBOLD)
                    }))
                    .child(div().text_xs().text_color(muted).child(queries.len().to_string()))
                    .child(
                        Button::new("query-new")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("New query")
                            .on_click(cx.listener(|this, _, window, cx| this.new_query(window, cx))),
                    ),
            )
            .children(rows)
            .children(draft_label.map(|label| {
                widgets::styled_text("query-draft", format!("{label} (unsaved)"), |d| {
                    d.px_2().py_1().text_sm().font_weight(FontWeight::MEDIUM)
                })
            }))
            .when(queries.is_empty() && self.draft.as_ref().is_none_or(|d| !d.is_new), |el| {
                el.child(widgets::styled_text("query-list-empty", "No saved queries yet", |d| {
                    d.px_2().text_xs().text_color(muted)
                }))
            })
    }
}
