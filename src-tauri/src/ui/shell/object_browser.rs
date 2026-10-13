//! Tables, views and saved queries of the document, with search. Selecting a query
//! opens Query mode. Port of src/shell/ObjectBrowser.tsx.
//!
//! Test ids: `new-table`, `new-query`, `object-search` (input), `objects-loading`,
//! and one button per object: `("table", name)`, `("view", name)`, `("query", id)`.

use super::{ModeId, Selection, SelectionKind, Shell};
use crate::ui::widgets;
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::*;
use gpui_kit::prelude::FluentBuilder as _;

pub struct ObjectBrowser {
    shell: Entity<Shell>,
    search: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl ObjectBrowser {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search objects"));
        let subscriptions = vec![
            cx.observe(&shell, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |_, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        Self {
            shell,
            search,
            _subscriptions: subscriptions,
        }
    }

    fn choose(&self, selection: Selection, window: &mut Window, cx: &mut App) {
        let opens_query = matches!(selection.kind, SelectionKind::Query | SelectionKind::NewQuery);
        self.shell.update(cx, |shell, cx| {
            shell.select(Some(selection), cx);
            if opens_query {
                shell.change_mode(ModeId::Query, window, cx);
            }
        });
    }
}

struct Item {
    kind: SelectionKind,
    id: String,
    name: String,
}

impl Render for ObjectBrowser {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let needle = self.search.read(cx).value().trim().to_lowercase();
        let shell = self.shell.read(cx);
        let active = shell.selection.clone();
        let tables: Vec<Item> = shell
            .objects
            .iter()
            .filter(|o| {
                (o.object_type == "table" || o.object_type == "view")
                    && !o.name.starts_with("_ixtable_")
                    && o.name.to_lowercase().contains(&needle)
            })
            .map(|o| Item {
                kind: if o.object_type == "view" {
                    SelectionKind::View
                } else {
                    SelectionKind::Table
                },
                id: o.name.clone(),
                name: o.name.clone(),
            })
            .collect();
        let queries: Vec<Item> = shell
            .config(cx)
            .saved_queries
            .iter()
            .filter(|q| q.name.to_lowercase().contains(&needle))
            .map(|q| Item {
                kind: SelectionKind::Query,
                id: q.id.clone(),
                name: q.name.clone(),
            })
            .collect();
        let loading = shell.metadata_loading;
        let error = shell.metadata_error.clone();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let danger = theme.danger;
        let entity = cx.entity();

        let group = |title: &'static str, items: Vec<Item>, add: Option<(&'static str, &'static str, SelectionKind)>| {
            let count = items.len();
            let empty = items.is_empty() && !needle.is_empty();
            v_flex()
                .gap_0p5()
                .child(
                    h_flex()
                        .px_2()
                        .gap_2()
                        .items_center()
                        .child(Icon::new(IconName::Database).xsmall().text_color(muted))
                        .child(
                            widgets::styled_text(
                                SharedString::from(format!("group-{}", title.to_lowercase())),
                                title,
                                |d| d.flex_1().text_xs().font_weight(FontWeight::SEMIBOLD),
                            ),
                        )
                        .child(div().text_xs().text_color(muted).child(count.to_string()))
                        .when_some(add, |el, (id, tip, kind)| {
                            let entity = entity.clone();
                            el.child(
                                Button::new(id)
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(IconName::Plus))
                                    .tooltip(tip)
                                    .selected(active.as_ref().is_some_and(|s| s.kind == kind))
                                    .on_click(move |_, window, cx| {
                                        let selection = Selection {
                                            kind,
                                            id: String::new(),
                                        };
                                        entity.update(cx, |this, cx| {
                                            if kind == SelectionKind::NewTable {
                                                this.shell.update(cx, |s, cx| s.select(Some(selection), cx));
                                            } else {
                                                this.choose(selection, window, cx);
                                            }
                                        });
                                    }),
                            )
                        }),
                )
                .children(items.into_iter().map(|item| {
                    let kind_key = match item.kind {
                        SelectionKind::View => "view",
                        SelectionKind::Query => "query",
                        _ => "table",
                    };
                    let selected = active
                        .as_ref()
                        .is_some_and(|s| s.kind == item.kind && s.id == item.id);
                    let label = if item.kind == SelectionKind::View {
                        format!("{} (read-only)", item.name)
                    } else {
                        item.name.clone()
                    };
                    let selection = Selection {
                        kind: item.kind,
                        id: item.id.clone(),
                    };
                    let entity = entity.clone();
                    Button::new(SharedString::from(format!("{kind_key}:{}", item.id)))
                        .ghost()
                        .small()
                        .justify_start()
                        .w_full()
                        .icon(Icon::new(if item.kind == SelectionKind::View {
                            IconName::Eye
                        } else {
                            IconName::Table2
                        }))
                        .label(label)
                        .selected(selected)
                        .on_click(move |_, window, cx| {
                            let selection = selection.clone();
                            entity.update(cx, |this, cx| this.choose(selection, window, cx));
                        })
                }))
                .when(empty, |el| {
                    el.child(div().px_2().text_xs().text_color(muted).child("No matches"))
                })
        };

        v_flex()
            .id("database-objects")
            .gap_3()
            .p_2()
            .child(
                h_flex()
                    .justify_between()
                    .px_1()
                    .child(div().text_xs().text_color(muted).child("OBJECTS"))
                    .when(loading, |el| el.child(widgets::styled_text("objects-loading", "Loading…", |d| d.text_xs()))),
            )
            .child(Input::new(&self.search).id("object-search").aria_label("Search database objects"))
            .when_some(error, |el, e| el.child(widgets::styled_text("objects-error", e, |d| d.text_xs().text_color(danger))))
            .child(group("Tables", tables, Some(("new-table", "New table", SelectionKind::NewTable))))
            .child(group("Queries", queries, Some(("new-query", "New query", SelectionKind::NewQuery))))
    }
}
