//! The Data page's sidebar: tables, views and saved queries of the document, with search.
//! Whatever is chosen here opens in the Data page workspace. Port of
//! src/shell/ObjectBrowser.tsx.
//!
//! Test ids: `new-table`, `new-query`, `object-search` (input), `objects-loading`,
//! one button per object: `("table", name)`, `("view", name)`, `("query", id)`, and
//! `query-draft` for the new query that is not saved yet.

use super::{Selection, SelectionKind, Shell};
use crate::manager::AppError;
use crate::ui::widgets::{self, Type, TypeScale as _};
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

    fn choose(&self, selection: Selection, cx: &mut App) {
        self.shell.update(cx, |shell, cx| shell.select(Some(selection), cx));
    }
}

struct Item {
    kind: SelectionKind,
    id: String,
    name: String,
    /// A short tag shown as a badge after the name ("action", "read-only", "unsaved").
    tag: Option<&'static str>,
    /// Element id of the row (`table:<name>`, `query:<id>`, `query-draft`).
    key: String,
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
                tag: (o.object_type == "view").then_some("read-only"),
                key: format!("{}:{}", o.object_type, o.name),
            })
            .collect();
        let mut queries: Vec<Item> = shell
            .config(cx)
            .saved_queries
            .iter()
            .filter(|q| q.name.to_lowercase().contains(&needle))
            .map(|q| Item {
                kind: SelectionKind::Query,
                id: q.id.clone(),
                name: if q.name.is_empty() { "Untitled query".to_string() } else { q.name.clone() },
                tag: (!q.name.is_empty() && q.action.is_some()).then_some("action"),
                key: format!("query:{}", q.id),
            })
            .collect();
        // The new query being edited is listed (and reopened from here) until it is saved.
        if let Some((id, name)) = &shell.unsaved_query {
            queries.push(Item {
                kind: SelectionKind::NewQuery,
                id: id.clone(),
                name: name.clone(),
                tag: Some("unsaved"),
                key: "query-draft".into(),
            });
        }
        let loading = shell.metadata_loading;
        let error = shell.metadata_error.clone();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let entity = cx.entity();

        let group = |title: &'static str, icon: IconName, items: Vec<Item>, add: Option<(&'static str, &'static str, SelectionKind)>| {
            let count = items.iter().filter(|i| i.kind != SelectionKind::NewQuery).count();
            let empty = items.is_empty() && !needle.is_empty();
            let items_empty = items.is_empty();
            v_flex()
                .gap_0p5()
                .child(
                    h_flex()
                        .px_2()
                        .gap_2()
                        .items_center()
                        .child(Icon::new(icon).xsmall().text_color(muted))
                        .child(
                            widgets::styled_text(
                                SharedString::from(format!("group-{}", title.to_lowercase())),
                                title,
                                |d| d.flex_1().ty(Type::Caption).font_weight(FontWeight::SEMIBOLD),
                            ),
                        )
                        .child(div().ty(Type::Caption).text_color(muted).child(count.to_string()))
                        .when_some(add, |el, (id, tip, kind)| {
                            let entity = entity.clone();
                            el.child(
                                Button::new(id)
                                    .ghost()
                                    .xsmall()
                                    .icon(Icon::new(IconName::Plus))
                                    .tooltip(tip)
                                    .selected(active.as_ref().is_some_and(|s| s.kind == kind && s.id.is_empty()))
                                    .on_click(move |_, _, cx| {
                                        let selection = Selection {
                                            kind,
                                            id: String::new(),
                                        };
                                        entity.update(cx, |this, cx| this.choose(selection, cx));
                                    }),
                            )
                        }),
                )
                .children(items.into_iter().map(|item| {
                    let selected = active
                        .as_ref()
                        .is_some_and(|s| s.kind == item.kind && s.id == item.id);
                    let selection = Selection {
                        kind: item.kind,
                        id: item.id.clone(),
                    };
                    let entity = entity.clone();
                    let icon = match item.kind {
                        SelectionKind::View => IconName::Eye,
                        SelectionKind::Query | SelectionKind::NewQuery => IconName::CodeXml,
                        _ => IconName::Table2,
                    };
                    let full = match item.tag {
                        Some(tag) => format!("{} ({tag})", item.name),
                        None => item.name.clone(),
                    };
                    // The name truncates; the tag badge stays visible. The row icon shows on
                    // hover or selection only, and keeps its space so names don't shift.
                    let group_name = SharedString::from(format!("row-{}", item.key));
                    let icon_group = group_name.clone();
                    div().w_full().group(group_name).child(Button::new(SharedString::from(item.key))
                        .ghost()
                        .small()
                        .w_full()
                        .accessibility_label(full.clone())
                        .child(
                            h_flex()
                                .w_full()
                                .min_w_0()
                                .gap_1()
                                .items_center()
                                .child(
                                    div()
                                        .flex_none()
                                        .when(!selected, |d| d.opacity(0.).group_hover(icon_group, |s| s.opacity(1.)))
                                        .child(Icon::new(icon).small()),
                                )
                                .child(div().min_w_0().overflow_hidden().whitespace_nowrap().text_ellipsis().child(item.name.clone()))
                                .when_some(item.tag, |row, tag| {
                                    row.child(
                                        div()
                                            .flex_none()
                                            .px_1()
                                            .rounded_sm()
                                            .bg(muted.opacity(0.15))
                                            .ty(Type::Caption)
                                            .text_color(muted)
                                            .child(tag),
                                    )
                                }),
                        )
                        .tooltip(full)
                        .selected(selected)
                        .on_click(move |_, _, cx| {
                            let selection = selection.clone();
                            entity.update(cx, |this, cx| this.choose(selection, cx));
                        }))
                }))
                .when(empty, |el| {
                    el.child(div().px_2().ty(Type::Caption).text_color(muted).child("No matches"))
                })
                .when(items_empty && needle.is_empty(), |el| {
                    let hint = match add {
                        Some((_, _, SelectionKind::NewTable)) => "No tables yet.",
                        _ => "No queries yet.",
                    };
                    el.child(
                        v_flex()
                            .px_2()
                            .items_start()
                            .child(widgets::styled_text(
                                SharedString::from(format!("empty-{}", title.to_lowercase())),
                                hint,
                                move |d| d.ty(Type::Caption).text_color(muted),
                            ))
                            .when_some(add, |el, (id, tip, kind)| {
                                let entity = entity.clone();
                                el.child(
                                    Button::new(SharedString::from(format!("{id}-empty")))
                                        .link()
                                        .xsmall()
                                        .label(tip)
                                        .on_click(move |_, _, cx| {
                                            let selection = Selection { kind, id: String::new() };
                                            entity.update(cx, |this, cx| this.choose(selection, cx));
                                        }),
                                )
                            }),
                    )
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
                    .child(div().ty(Type::Caption).text_color(muted).child("OBJECTS"))
                    .when(loading, |el| el.child(widgets::styled_text("objects-loading", "Loading…", |d| d.ty(Type::Caption)))),
            )
            .child(Input::new(&self.search).id("object-search").aria_label("Search database objects"))
            .when_some(error, |el, e| {
                el.child(widgets::error_callout("objects-error", "The tables could not be loaded.", &AppError::new("", e), None, cx))
            })
            .child(group("Tables", IconName::Database, tables, Some(("new-table", "New table", SelectionKind::NewTable))))
            .child(group("Queries", IconName::CodeXml, queries, Some(("new-query", "New query", SelectionKind::NewQuery))))
    }
}
