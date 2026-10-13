//! Settings tab "Roles" (port of `RolesTab.tsx`): local role definitions and their
//! permission matrix (PRD section 20).
//!
//! Test ids: `rbac-note` (the limitation text), `role-select:<role id>` (selected = open),
//! `add-role` ("New role"), `delete-role`, `role-name` (input), `role-editor` (labelled
//! "Role <name>"), `perm:<kind>:<object id>:<flag>` (checkboxes labelled "read form Orders"),
//! `nav-perm:<item id>` ("Show X in navigation"), `action-perm:<action id>` ("Run X"),
//! `trigger-gap:<trigger id>`, `no-actions`, `no-roles` and `roles-error`.

use crate::design::NavigationItem;
use crate::forms::rbac::{set_object_permission, toggle_listed};
use crate::forms::schema::{flatten_navigation, new_id};
use crate::forms::trigger_warnings::user_trigger_gaps;
use crate::manager::AppError;
use crate::roles::{Permissions, Role};
use crate::ui::{prelude::*, shell::Shell, store::StoreEvent};
use gpui_kit::component::{
    checkbox::Checkbox,
    input::{Input, InputEvent, InputState},
};

/// PRD section 20.2: ixtable RBAC is not a database security boundary.
pub const RBAC_LIMITATION: &str = "Roles control what the ixtable Runtime shows and allows: navigation, forms, reports, dashboards, queries, and actions. \
They are not a database security boundary. With a direct PostgreSQL connection, a user who extracts valid database \
credentials can bypass these rules. Strong isolation needs separate least-privileged database credentials and database \
permissions that you set up yourself.";

const FLAGS: [&str; 4] = ["read", "create", "update", "delete"];

struct Row {
    kind: &'static str,
    id: String,
    name: String,
}

pub struct RolesTab {
    shell: Entity<Shell>,
    selected: Option<String>,
    name: Entity<InputState>,
    named_for: Option<String>,
    error: Option<AppError>,
    _subscriptions: Vec<Subscription>,
}

impl RolesTab {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = shell.read(cx).store.clone();
        let first = shell.read(cx).config(cx).roles.first().map(|r| r.id.clone());
        let name = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe(&store, |_, _, _: &StoreEvent, cx| cx.notify()),
            cx.subscribe(&name, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().to_string();
                    this.edit("Rename role", move |role| role.name = text.clone(), cx);
                }
            }),
        ];
        Self { shell, selected: first, name, named_for: None, error: None, _subscriptions: subscriptions }
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    fn roles(&self, cx: &App) -> Vec<Role> {
        self.shell.read(cx).config(cx).roles.clone()
    }

    fn update(&mut self, label: &str, mutate: impl FnOnce(&mut crate::archive::DocumentConfig), cx: &mut Context<Self>) {
        let store = self.shell.read(cx).store.clone();
        let result = store.update(cx, |s, cx| s.update(label, mutate, cx));
        self.error = result.err();
        cx.notify();
    }

    fn edit(&mut self, label: &str, change: impl FnOnce(&mut Role), cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else { return };
        self.update(
            label,
            move |config| {
                if let Some(role) = config.roles.iter_mut().find(|r| r.id == id) {
                    change(role);
                }
            },
            cx,
        );
    }

    fn edit_permissions(&mut self, change: impl FnOnce(&Permissions) -> Permissions, cx: &mut Context<Self>) {
        self.edit("Edit role", move |role| role.permissions = change(&role.permissions), cx);
    }

    /// Adds a role named "Role N" and selects it.
    pub fn add_role(&mut self, cx: &mut Context<Self>) {
        let created = Role {
            id: new_id(),
            name: format!("Role {}", self.roles(cx).len() + 1),
            permissions: Permissions::default(),
        };
        self.selected = Some(created.id.clone());
        self.update("Add role", move |config| config.roles.push(created), cx);
    }

    fn remove_role(&mut self, id: String, cx: &mut Context<Self>) {
        self.selected = None;
        self.update("Delete role", move |config| config.roles.retain(|r| r.id != id), cx);
    }

    fn rows(&self, cx: &App) -> Vec<Row> {
        let shell = self.shell.read(cx);
        let config = shell.config(cx);
        let mut rows: Vec<Row> = vec![];
        rows.extend(config.design.forms.iter().map(|f| Row { kind: "form", id: f.id.clone(), name: f.name.clone() }));
        rows.extend(config.reports.iter().map(|r| Row { kind: "report", id: r.id.clone(), name: r.name.clone() }));
        rows.extend(config.dashboards.iter().map(|d| Row { kind: "dashboard", id: d.id.clone(), name: d.name.clone() }));
        rows.extend(
            shell
                .objects
                .iter()
                .filter(|o| o.object_type == "table")
                .map(|o| Row { kind: "table", id: o.name.clone(), name: o.name.clone() }),
        );
        // Action queries are governed by their target table's permissions.
        rows.extend(
            config
                .saved_queries
                .iter()
                .filter(|q| q.action.is_none())
                .map(|q| Row { kind: "query", id: q.id.clone(), name: q.name.clone() }),
        );
        rows
    }
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    chars.next().map_or(String::new(), |c| c.to_uppercase().collect::<String>() + chars.as_str())
}

impl Render for RolesTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let roles = self.roles(cx);
        if self.selected.as_ref().is_some_and(|id| !roles.iter().any(|r| &r.id == id)) {
            self.selected = None;
        }
        let role = self.selected.as_ref().and_then(|id| roles.iter().find(|r| &r.id == id)).cloned();
        if self.named_for != role.as_ref().map(|r| r.id.clone()) {
            self.named_for = role.as_ref().map(|r| r.id.clone());
            let text = role.as_ref().map(|r| r.name.clone()).unwrap_or_default();
            self.name.update(cx, |s, cx| s.set_value(text, window, cx));
        }
        let config = self.shell.read(cx).config(cx).clone();
        let mut page = v_flex()
            .gap_3()
            .child(widgets::styled_text("rbac-note", RBAC_LIMITATION, {
                let color = theme.muted_foreground;
                move |d| d.text_sm().text_color(color)
            }))
            .children(self.error.clone().map(|e| {
                super::message("roles-error", format!("{}: {}", e.code, e.message), true, cx)
            }));
        let list = v_flex()
            .gap_1()
            .w(px(200.))
            .children(roles.iter().map(|item| {
                let id = item.id.clone();
                Button::new(SharedString::from(format!("role-select:{}", item.id)))
                    .ghost()
                    .small()
                    .start_label(None, if item.name.is_empty() { "Untitled role".to_string() } else { item.name.clone() })
                    .selected(role.as_ref().is_some_and(|r| r.id == item.id))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.selected = Some(id.clone());
                        cx.notify();
                    }))
            }))
            .child(Button::new("add-role").small().icon(IconName::Plus).label("New role").on_click(
                cx.listener(|this, _, _, cx| this.add_role(cx)),
            ))
            .child(div().text_xs().text_color(theme.muted_foreground).child("The developer always has full access."));
        let editor: AnyElement = match role {
            None => widgets::styled_text("no-roles", "Create a role to choose what its users can see and change.", {
                let color = theme.muted_foreground;
                move |d| d.text_sm().text_color(color)
            })
            .into_any_element(),
            Some(role) => {
                let permissions = role.permissions.clone();
                let role_id = role.id.clone();
                let flag = |row: &Row, name: &str| {
                    permissions.objects.iter().any(|o| {
                        o.kind == row.kind
                            && o.id == row.id
                            && match name {
                                "read" => o.read,
                                "create" => o.create,
                                "update" => o.update,
                                _ => o.delete,
                            }
                    })
                };
                let rows = self.rows(cx);
                let matrix = v_flex().gap_0p5().items_start().children(rows.iter().map(|row| {
                    let cells = FLAGS.iter().map(|name| {
                        let (kind, object_id, name) = (row.kind, row.id.clone(), *name);
                        Checkbox::new(SharedString::from(format!("perm:{kind}:{object_id}:{name}")))
                            .checked(flag(row, name))
                            .accessibility_label(format!("{name} {kind} {}", row.name))
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                let (object_id, checked) = (object_id.clone(), *checked);
                                this.edit_permissions(
                                    move |p| set_object_permission(p, kind, &object_id, name, checked),
                                    cx,
                                );
                            }))
                    });
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(div().w(px(260.)).text_sm().child(format!("{} {}", row.kind, row.name)))
                        .children(cells)
                }));
                let header = h_flex().gap_3().child(div().w(px(260.)).text_sm().child("Object")).children(
                    FLAGS.iter().map(|name| div().w(px(24.)).text_xs().child(capitalized(name))),
                );
                let nav: Vec<&NavigationItem> = flatten_navigation(&config.design.navigation);
                let navigation = v_flex().gap_0p5().items_start().children(nav.iter().map(|item| {
                    let item_id = item.id.clone();
                    Checkbox::new(SharedString::from(format!("nav-perm:{}", item.id)))
                        .checked(permissions.navigation.contains(&item.id))
                        .accessibility_label(format!("Show {} in navigation", item.label))
                        .label(format!("Show {} in navigation", item.label))
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let (item_id, checked) = (item_id.clone(), *checked);
                            this.edit_permissions(
                                move |p| Permissions {
                                    navigation: toggle_listed(&p.navigation, &item_id, checked),
                                    ..p.clone()
                                },
                                cx,
                            );
                        }))
                }));
                let actions = v_flex().gap_0p5().items_start().children(config.actions.iter().map(|action| {
                    let action_id = action.id.clone();
                    Checkbox::new(SharedString::from(format!("action-perm:{}", action.id)))
                        .checked(permissions.actions.contains(&action.id))
                        .accessibility_label(format!("Run {}", action.name))
                        .label(format!("Run {}", action.name))
                        .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                            let (action_id, checked) = (action_id.clone(), *checked);
                            this.edit_permissions(
                                move |p| Permissions {
                                    actions: toggle_listed(&p.actions, &action_id, checked),
                                    ..p.clone()
                                },
                                cx,
                            );
                        }))
                }));
                let gaps = user_trigger_gaps(&config, &role_id);
                let delete_id = role_id.clone();
                v_flex()
                    .id("role-editor")
                    .aria_label(format!("Role {}", role.name))
                    .gap_2()
                    .flex_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().w(px(260.)).child(Input::new(&self.name).id("role-name").aria_label("Role name")))
                            .child(
                                Button::new("delete-role")
                                    .small()
                                    .icon(IconName::Delete)
                                    .label("Delete role")
                                    .on_click(cx.listener(move |this, _, _, cx| this.remove_role(delete_id.clone(), cx))),
                            ),
                    )
                    .children(gaps.iter().map(|gap| {
                        super::message(
                            SharedString::from(format!("trigger-gap:{}", gap.trigger_id)),
                            format!(
                                "Trigger \"{}\" runs as the signed-in user, but this role cannot {}. Saves that fire it will be refused for this role.",
                                gap.name,
                                gap.missing.join(", ")
                            ),
                            true,
                            cx,
                        )
                    }))
                    .child(widgets::section_heading("objects-heading", "Objects"))
                    .child(div().text_sm().text_color(theme.muted_foreground).child(
                        "Form permissions govern records changed through that form. Table permissions govern table pages and actions that write to the table. Reading a form, report, or dashboard also lets the role read the saved queries it shows.",
                    ))
                    .child(header)
                    .child(matrix)
                    .child(widgets::section_heading("navigation-heading", "Navigation"))
                    .child(navigation)
                    .child(widgets::section_heading("actions-heading", "Actions"))
                    .child(actions)
                    .when(config.actions.is_empty(), |d| d.child(widgets::text("no-actions", "No actions defined.")))
                    .test_support()
                    .into_any_element()
            }
        };
        page = page.child(h_flex().gap_4().items_start().child(list).child(editor));
        page
    }
}
