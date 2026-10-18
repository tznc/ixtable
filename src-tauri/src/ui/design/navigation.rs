//! The navigation tree (nested groups) and the start page (port of `NavigationEditor.tsx`).
//!
//! Test ids: the editor is `navigation-editor` ("Navigation editor"), its list
//! `navigation-items` ("Navigation items"); per item `nav-label:<id>`, `nav-kind:<id>`,
//! `nav-target:<id>` ("<label> opens"), `nav-mode:<id>`, `nav-group:<id>`, `nav-up:<id>`,
//! `nav-down:<id>`, `nav-remove:<id>`, and `nav-param:<id>:<name>` for each parameter of the
//! page an item opens; then `add-nav-item`, `add-nav-group` and `start-page`. The page links
//! overview is `page-links` with one `page-link:<n>` row per link (`no-page-links`).

use super::{ctl::icon_button, studio::DesignStudio};
use crate::design::{FormMode, NavKind, NavigationItem};
use crate::forms::{
    operations::{map_navigation, regroup_navigation, shift_navigation, take_navigation},
    schema::{FORM_MODES, flatten_navigation, form_mode_from_key, form_mode_key, new_id},
};
use crate::page_engine::links::LinkSource;
use crate::ui::{display, prelude::*};

const KINDS: [(&str, NavKind); 5] = [
    ("page", NavKind::Page),
    ("form", NavKind::Form),
    ("report", NavKind::Report),
    ("table", NavKind::Table),
    ("group", NavKind::Group),
];

fn kind_key(kind: NavKind) -> &'static str {
    KINDS.iter().find(|(_, k)| *k == kind).map_or("page", |(key, _)| key)
}

/// What each kind of item can open, and the groups an item can move into.
struct Targets {
    by_kind: Vec<(NavKind, Vec<(String, String)>)>,
    groups: Vec<(String, String)>,
}

impl Targets {
    fn of(&self, kind: NavKind) -> Vec<(String, String)> {
        self.by_kind.iter().find(|(k, _)| *k == kind).map(|(_, t)| t.clone()).unwrap_or_default()
    }
}

impl DesignStudio {
    fn edit_navigation(&mut self, label: &str, change: impl FnOnce(&[NavigationItem]) -> Vec<NavigationItem> + 'static, cx: &mut Context<Self>) {
        self.edit_design(label, move |d| d.navigation = change(&d.navigation), cx);
    }

    fn patch_navigation(&mut self, id: &str, patch: impl Fn(&mut NavigationItem) + 'static, cx: &mut Context<Self>) {
        let id = id.to_string();
        self.edit_navigation("Edit navigation", move |items| map_navigation(items, &id, &|mut item| {
            patch(&mut item);
            item
        }), cx);
    }

    fn add_navigation(&mut self, kind: NavKind, cx: &mut Context<Self>) {
        let first = self.config(cx).pages.first().map(|p| (p.id.clone(), p.name.clone()));
        let is_group = kind == NavKind::Group;
        let item = NavigationItem {
            id: new_id(),
            label: if is_group { "New group".to_string() } else { first.as_ref().map_or("New page".to_string(), |f| f.1.clone()) },
            kind,
            target_id: if is_group { None } else { first.map(|f| f.0) },
            mode: None,
            params: Default::default(),
            children: vec![],
        };
        self.edit_design(
            "Add navigation item",
            move |d| {
                if d.start_page.is_none() && !is_group {
                    d.start_page = Some(item.id.clone());
                }
                d.navigation.push(item);
            },
            cx,
        );
    }

    pub(super) fn render_navigation(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let (design, config) = {
            let c = self.config(cx);
            (c.design.clone(), c.clone())
        };
        let all = flatten_navigation(&design.navigation);
        let targets = Targets {
            by_kind: vec![
                (NavKind::Form, design.forms.iter().map(|f| (f.id.clone(), f.name.clone())).collect()),
                (NavKind::Report, config.reports.iter().map(|r| (r.id.clone(), r.name.clone())).collect()),
                (NavKind::Page, config.pages.iter().map(|d| (d.id.clone(), d.name.clone())).collect()),
                (NavKind::Table, self.tables(cx).into_iter().map(|t| (t.clone(), t)).collect()),
                (NavKind::Group, vec![]),
            ],
            groups: all.iter().filter(|i| i.kind == NavKind::Group).map(|i| (i.id.clone(), i.label.clone())).collect(),
        };
        let mut start_options = vec![(String::new(), "First item".to_string())];
        start_options.extend(all.iter().filter(|i| i.kind != NavKind::Group).map(|i| (i.id.clone(), i.label.clone())));
        let start = self.pick(
            "start-page",
            "Start page",
            design.start_page.as_deref().unwrap_or(""),
            start_options,
            "First item",
            |this, id, cx| {
                let id = id.to_string();
                this.edit_design("Set start page", move |d| d.start_page = (!id.is_empty()).then_some(id), cx)
            },
            cx,
        );
        let items = self.render_nav_items(&design.navigation, 0, &targets, &design, window, cx);
        v_flex()
            .id("navigation-editor")
            .aria_label("Navigation editor")
            .gap_2()
            .p_3()
            .child(widgets::section_heading("navigation-title", "Navigation"))
            .child(div().text_xs().text_color(muted).child("Groups nest navigation items. Roles can hide items in Settings › Roles."))
            .child(items)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("add-nav-item")
                            .outline()
                            .small()
                            .icon(IconName::Plus)
                            .label("Add navigation item")
                            .on_click(cx.listener(|this, _, _, cx| this.add_navigation(NavKind::Page, cx))),
                    )
                    .child(
                        Button::new("add-nav-group")
                            .outline()
                            .small()
                            .icon(IconName::FolderPlus)
                            .label("Add group")
                            .on_click(cx.listener(|this, _, _, cx| this.add_navigation(NavKind::Group, cx))),
                    ),
            )
            .child(start)
            .child(page_links(&config, cx))
            .test_support()
    }

    fn render_nav_items(
        &mut self,
        items: &[NavigationItem],
        depth: usize,
        targets: &Targets,
        design: &crate::design::DesignSchema,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let mut list = v_flex().id(SharedString::from(format!("navigation-items-{depth}"))).gap_1().pl(px(depth as f32 * 16.0));
        if depth == 0 {
            list = v_flex().id("navigation-items").aria_label("Navigation items").gap_1();
        }
        for item in items {
            let id = item.id.clone();
            let label = self.text_field(
                &format!("nav-label:{id}"),
                "Navigation label",
                &item.label,
                "",
                {
                    let id = id.clone();
                    move |this, text, cx| this.patch_navigation(&id, move |i| i.label = text.clone(), cx)
                },
                window,
                cx,
            );
            let kind = self.pick_bare(
                &format!("nav-kind:{id}"),
                kind_key(item.kind),
                KINDS.iter().map(|(k, _)| (k.to_string(), k.to_string())).collect(),
                {
                    let (id, by_kind) = (id.clone(), targets.by_kind.clone());
                    move |this, value, cx| {
                        let kind = KINDS.iter().find(|(k, _)| *k == value).map_or(NavKind::Form, |(_, k)| *k);
                        let first = by_kind.iter().find(|(k, _)| *k == kind).and_then(|(_, t)| t.first()).map(|t| t.0.clone());
                        this.patch_navigation(&id, move |i| {
                            i.kind = kind;
                            i.target_id = if kind == NavKind::Group { None } else { first.clone() };
                        }, cx)
                    }
                },
                cx,
            );
            let target = (item.kind != NavKind::Group).then(|| {
                let mut choices = vec![(String::new(), "—".to_string())];
                choices.extend(targets.of(item.kind));
                let id = id.clone();
                self.pick_bare(
                    &format!("nav-target:{id}"),
                    item.target_id.as_deref().unwrap_or(""),
                    choices,
                    move |this, value, cx| {
                        let value = value.to_string();
                        this.patch_navigation(&id, move |i| i.target_id = (!value.is_empty()).then(|| value.clone()), cx)
                    },
                    cx,
                )
            });
            let target_page = (item.kind == NavKind::Page)
                .then(|| design_page(&self.config(cx).pages, item.target_id.as_deref()))
                .flatten();
            let solo_form = target_page.as_ref().is_some_and(|p| {
                matches!(p.components.as_slice(), [only] if only.kind == crate::pages::ComponentKind::Form)
            });
            let params: Vec<_> = target_page
                .map(|page| {
                    page.parameters
                        .iter()
                        .map(|param| {
                            let (iid, name) = (id.clone(), param.name.clone());
                            let label = if param.required { format!("{} (required)", param.name) } else { param.name.clone() };
                            self.text_field(
                                &format!("nav-param:{id}:{}", param.name),
                                &label,
                                item.params.get(&param.name).map(String::as_str).unwrap_or(""),
                                "Expression, e.g. 'East'",
                                move |this, text, cx| {
                                    let name = name.clone();
                                    this.patch_navigation(&iid, move |i| {
                                        if text.trim().is_empty() {
                                            i.params.remove(&name);
                                        } else {
                                            i.params.insert(name.clone(), text.clone());
                                        }
                                    }, cx)
                                },
                                window,
                                cx,
                            )
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mode = (item.kind == NavKind::Form || solo_form).then(|| {
                let mut choices = vec![(String::new(), "Default mode".to_string())];
                choices.extend(
                    FORM_MODES
                        .into_iter()
                        .filter(|m| !matches!(m, FormMode::Detail | FormMode::Edit))
                        .map(|m| (form_mode_key(m).to_string(), form_mode_key(m).to_string())),
                );
                let id = id.clone();
                self.pick_bare(
                    &format!("nav-mode:{id}"),
                    item.mode.map_or("", form_mode_key),
                    choices,
                    move |this, value, cx| {
                        let mode = form_mode_from_key(value);
                        this.patch_navigation(&id, move |i| i.mode = mode, cx)
                    },
                    cx,
                )
            });
            let parent = targets
                .groups
                .iter()
                .find(|(gid, _)| {
                    design_group_children(design, gid).iter().any(|child| *child == item.id)
                })
                .map(|(gid, _)| gid.clone())
                .unwrap_or_default();
            let mut group_options = vec![(String::new(), "Top level".to_string())];
            group_options.extend(targets.groups.iter().filter(|(gid, _)| *gid != item.id).map(|(gid, l)| (gid.clone(), format!("In {l}"))));
            let group = {
                let id = id.clone();
                self.pick_bare(
                    &format!("nav-group:{id}"),
                    &parent,
                    group_options,
                    move |this, value, cx| {
                        let (id, value) = (id.clone(), value.to_string());
                        this.edit_navigation(
                            "Move navigation item",
                            move |items| regroup_navigation(items, &id, (!value.is_empty()).then_some(value.as_str())),
                            cx,
                        )
                    },
                    cx,
                )
            };
            let shift = |key: &str, icon: IconName, delta: i64, cx: &mut Context<Self>| {
                let id = id.clone();
                icon_button(format!("{key}:{id}"), icon, if delta < 0 { "Move up" } else { "Move down" }).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let id = id.clone();
                        this.edit_navigation("Edit navigation", move |items| shift_navigation(items, &id, delta), cx)
                    }),
                )
            };
            let up = shift("nav-up", IconName::ArrowUp, -1, cx);
            let down = shift("nav-down", IconName::ArrowDown, 1, cx);
            let remove = {
                let id = id.clone();
                icon_button(format!("nav-remove:{id}"), IconName::Trash, &format!("Remove {}", item.label)).on_click(cx.listener(
                    move |this, _, _, cx| {
                        let id = id.clone();
                        this.edit_navigation("Remove navigation item", move |items| take_navigation(items, &id).0, cx)
                    },
                ))
            };
            list = list.child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().w(px(180.)).child(label))
                            .child(kind)
                            .children(target)
                            .children(mode)
                            .child(group)
                            .child(up)
                            .child(down)
                            .child(remove),
                    )
                    .when(!params.is_empty(), |el| el.child(h_flex().gap_2().pl(px(16.)).flex_wrap().children(params)))
                    .when(item.kind == NavKind::Group, |el| {
                        el.child(self.render_nav_items(&item.children, depth + 1, targets, design, window, cx))
                    }),
            );
        }
        list
    }
}

/// Ids of the direct children of the group `group` anywhere in the tree.
fn design_group_children(design: &crate::design::DesignSchema, group: &str) -> Vec<String> {
    flatten_navigation(&design.navigation)
        .into_iter()
        .find(|i| i.id == group)
        .map(|g| g.children.iter().map(|c| c.id.clone()).collect())
        .unwrap_or_default()
}

fn design_page(pages: &[crate::pages::Page], id: Option<&str>) -> Option<crate::pages::Page> {
    pages.iter().find(|p| Some(p.id.as_str()) == id).cloned()
}

/// Every way one page leads to another: navigation items, page buttons and "Open page"
/// steps, with the parameters each passes.
fn page_links(config: &crate::archive::DocumentConfig, cx: &App) -> impl IntoElement {
    let muted = cx.theme().muted_foreground;
    let links = crate::page_engine::links::page_links(config);
    v_flex()
        .id("page-links")
        .aria_label("Page links")
        .gap_1()
        .pt_3()
        .child(widgets::section_heading("page-links-title", "Page links"))
        .child(div().text_xs().text_color(muted).child(
            "How users move between pages. Buttons set their target page in the page designer; actions use an Open page step.",
        ))
        .when(!links.is_empty(), |el| {
            let rows = links
                .iter()
                .enumerate()
                .map(|(i, link)| {
                    let from = match &link.source {
                        LinkSource::Navigation { label, .. } => format!("Navigation item {label}"),
                        LinkSource::Button { page_name, title, .. } => format!("Button {title} on {page_name}"),
                        LinkSource::Step { action_name, .. } => format!("Action {action_name}"),
                    };
                    let to = config.pages.iter().find(|p| p.id == link.target).map_or("A missing page".to_string(), |p| p.name.clone());
                    let passes = if link.params.is_empty() {
                        "—".to_string()
                    } else {
                        link.params.iter().map(|(k, v)| format!("{k} = {v}")).collect::<Vec<_>>().join(", ")
                    };
                    (
                        ElementId::from(SharedString::from(format!("page-link:{}", i + 1))),
                        link.describe_with(&config.pages),
                        vec![from, to, passes],
                    )
                })
                .collect();
            el.child(display::data_table("page-links-table", &["From", "To", "Passes"], rows, cx))
        })
        .when(links.is_empty(), |el| {
            el.child(widgets::empty_state("no-page-links", "No links between pages yet", "Add a navigation item, or set a button's target page.", cx))
        })
}
