//! Studio's cloud cards: create or link the cloud application, sync roles, deliver the
//! PostgreSQL credential. Port of src/cloud/StudioPanels.tsx.
//!
//! Test ids. Link card `cloud-link` ("Create a cloud application"): `link-loading`,
//! organization buttons `link-org:<id>`, inputs `link-org-name`, `link-name`,
//! `link-existing`, buttons `link-create` and `link-existing-submit`, outcome
//! `link-notice` / `link-error`. Roles card `cloud-roles`: `roles-count`, `roles-sync`,
//! `roles-notice` / `roles-error`. Credentials card `cloud-credentials`: `cred-warning`,
//! checkboxes `cred-scope-shared` / `cred-scope-user`, inputs `cred-user-id`,
//! `cred-username`, `cred-password`, button `cred-upload`, `cred-notice` / `cred-error`.

use super::{
    action::{Action, action_status},
    contract,
    errors::CloudError,
    services::{CloudServices, Services},
    types::{CloudApp, Organization},
};
use crate::{
    cloud::CloudLink,
    ui::{forms, forms::eid, prelude::*, shell::Shell},
};
use gpui_kit::component::input::InputState;
use serde_json::{Value, json};
use std::sync::Arc;

/// The document's roles as `roles-sync` takes them.
pub fn role_payload(config: &crate::archive::DocumentConfig) -> Value {
    Value::Array(
        config
            .roles
            .iter()
            .map(|r| {
                json!({
                    "id": r.id,
                    "name": r.name,
                    "permissions": serde_json::to_value(&r.permissions).unwrap_or(Value::Null),
                })
            })
            .collect(),
    )
}

/// The cloud application the definition is linked to.
pub fn linked_app(config: &crate::archive::DocumentConfig) -> Option<String> {
    config.cloud.as_ref().map(|l| l.app_id.clone())
}

/// Records the cloud link in the definition (undoable).
pub fn set_link(
    shell: &Entity<Shell>,
    label: &str,
    link: CloudLink,
    cx: &mut App,
) -> Result<(), CloudError> {
    let store = shell.read(cx).store.clone();
    store
        .update(cx, |s, cx| s.update(label, move |c| c.cloud = Some(link), cx))
        .map(|_| ())
        .map_err(Into::into)
}

// ---- link

/// Creates (or links) the cloud application for this document.
pub struct LinkPanel {
    shell: Entity<Shell>,
    services: Arc<dyn CloudServices>,
    orgs: Option<Vec<Organization>>,
    org_id: String,
    org_name: Entity<InputState>,
    name: Entity<InputState>,
    existing: Entity<InputState>,
    action: Action,
}

impl LinkPanel {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = Services::get(cx);
        let name = shell.read(cx).config(cx).name.clone();
        let this = Self {
            shell,
            services: services.clone(),
            orgs: None,
            org_id: String::new(),
            org_name: forms::line_state(window, cx, "Organization name", ""),
            name: forms::line_state(window, cx, "Application name", &name),
            existing: forms::line_state(window, cx, "Application id", ""),
            action: Action::default(),
        };
        task::spawn_call(cx, move || services.list_organizations(), |this, result, cx| {
            let all = result.unwrap_or_default();
            let managed: Vec<_> =
                all.into_iter().filter(|o| o.role == "owner" || o.role == "admin").collect();
            if this.org_id.is_empty() {
                this.org_id = managed.first().map(|o| o.id.clone()).unwrap_or_default();
            }
            this.orgs = Some(managed);
            cx.notify();
        });
        this
    }

    fn create(&mut self, cx: &mut Context<Self>) {
        let (org_id, org_name, name) = (
            self.org_id.clone(),
            forms::text_of(&self.org_name, cx).trim().to_string(),
            forms::text_of(&self.name, cx).trim().to_string(),
        );
        let (config, document_id) = {
            let shell = self.shell.read(cx);
            (shell.config(cx).clone(), shell.doc.document_id.clone())
        };
        let name = if name.is_empty() { config.name.clone() } else { name };
        let kind = if config.datasource.kind == "postgres" { "postgres" } else { "sqlite" };
        self.action.begin();
        let services = self.services.clone();
        task::spawn_call(
            cx,
            move || -> Result<(String, String, String), CloudError> {
                let mut target = org_id;
                if target.is_empty() {
                    if org_name.is_empty() {
                        return Err(CloudError::new("VALIDATION", "Name the new organization."));
                    }
                    target = services.create_organization(&org_name)?.id;
                }
                let reply = services.invoke(
                    "apps-create",
                    json!({
                        "orgId": target,
                        "name": name,
                        "documentId": document_id,
                        "datasourceKind": kind,
                    }),
                )?;
                let app = contract::apps_create(&reply)?;
                if !config.roles.is_empty() {
                    let synced = services
                        .invoke("roles-sync", json!({ "appId": app.id, "roles": role_payload(&config) }))?;
                    contract::roles_sync(&synced)?;
                }
                Ok((app.id, target, app.name))
            },
            |this, result, cx| {
                let outcome = result.and_then(|(app_id, org_id, _)| {
                    let link = CloudLink { app_id, org_id, head_version_id: None };
                    set_link(&this.shell, "Link cloud application", link, cx)?;
                    Ok("Cloud application created and linked to this document.".to_string())
                });
                this.action.finish(outcome);
                cx.notify();
            },
        );
        cx.notify();
    }

    fn link_existing(&mut self, cx: &mut Context<Self>) {
        let id = forms::text_of(&self.existing, cx).trim().to_string();
        self.action.begin();
        let services = self.services.clone();
        task::spawn_call(
            cx,
            move || -> Result<CloudApp, CloudError> {
                let session = services.require_session()?;
                let app = services.get_app(&id)?.ok_or_else(|| {
                    CloudError::new("NOT_FOUND", "No cloud application with that id is visible to you.")
                })?;
                if app.owner_id != session.user_id {
                    return Err(CloudError::new(
                        "FORBIDDEN",
                        "Only the application's owner can publish to it.",
                    ));
                }
                Ok(app)
            },
            |this, result, cx| {
                let outcome = result.and_then(|app| {
                    let link = CloudLink { app_id: app.id, org_id: app.org_id, head_version_id: None };
                    set_link(&this.shell, "Link cloud application", link, cx)?;
                    Ok(format!("Linked to {}.", app.name))
                });
                this.action.finish(outcome);
                cx.notify();
            },
        );
        cx.notify();
    }
}

impl Render for LinkPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.action.busy;
        let existing_blank = forms::text_of(&self.existing, cx).trim().is_empty();
        let mut card = forms::card("cloud-link", "Create a cloud application", cx).child(forms::muted(
            "A cloud application has exactly one developer (you) and the runtime users you \
             invite. It is private: nobody can find or download it without an invitation.",
            cx,
        ));
        match &self.orgs {
            None => card = card.child(widgets::text("link-loading", "Loading organizations…")),
            Some(orgs) if !orgs.is_empty() => {
                let mut row = h_flex().gap_2().flex_wrap().child("Organization");
                for org in orgs {
                    let id = org.id.clone();
                    row = row.child(
                        Button::new(eid(format!("link-org:{}", org.id)))
                            .label(org.name.clone())
                            .selected_tint(org.id == self.org_id, cx)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.org_id = id.clone();
                                cx.notify();
                            })),
                    );
                }
                card = card.child(row);
            }
            Some(_) => {
                card = card.child(forms::input_field("link-org-name", "New organization name", &self.org_name, cx))
            }
        }
        card.child(forms::input_field("link-name", "Application name", &self.name, cx))
            .child(
                forms::actions().child(
                    forms::primary("link-create", "Create cloud application")
                        .disabled(busy || self.orgs.is_none())
                        .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
                ),
            )
            .child(forms::input_field("link-existing", "Existing application id", &self.existing, cx))
            .child(
                forms::actions().child(
                    forms::secondary("link-existing-submit", "Link existing application")
                        .disabled(busy || existing_blank)
                        .on_click(cx.listener(|this, _, _, cx| this.link_existing(cx))),
                ),
            )
            .child(action_status("link", &self.action, cx))
    }
}

// ---- roles

/// Uploads the document's runtime roles (`roles-sync`, upsert by role id).
pub struct RolesPanel {
    shell: Entity<Shell>,
    services: Arc<dyn CloudServices>,
    action: Action,
    _watch: Subscription,
}

impl RolesPanel {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let watch = cx.observe(&shell.read(cx).store.clone(), |_, _, cx| cx.notify());
        Self { shell, services: Services::get(cx), action: Action::default(), _watch: watch }
    }

    fn sync(&mut self, cx: &mut Context<Self>) {
        let config = self.shell.read(cx).config(cx).clone();
        let Some(app_id) = linked_app(&config) else { return };
        let services = self.services.clone();
        let count = config.roles.len();
        self.action.begin();
        task::spawn_call(
            cx,
            move || -> Result<String, CloudError> {
                let reply = services
                    .invoke("roles-sync", json!({ "appId": app_id, "roles": role_payload(&config) }))?;
                contract::roles_sync(&reply)?;
                Ok(format!("Synced {} to ixtable Cloud.", widgets::plural(count, "role")))
            },
            |this, result, cx| {
                this.action.finish(result);
                cx.notify();
            },
        );
        cx.notify();
    }
}

impl Render for RolesPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.shell.read(cx).config(cx).roles.len();
        forms::card("cloud-roles", "Runtime roles", cx)
            .child(widgets::text(
                "roles-count",
                format!(
                    "{} defined in the Roles tab. Invited runtime users get one of \
                     them; ixtable enforces it in navigation, forms, reports, dashboards, and actions.",
                    widgets::plural(count, "role")
                ),
            ))
            .child(
                forms::actions().child(
                    forms::secondary("roles-sync", "Sync roles to cloud")
                        .disabled(self.action.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.sync(cx))),
                ),
            )
            .child(action_status("roles", &self.action, cx))
    }
}

// ---- credentials

/// Encrypted PostgreSQL credential delivery (PRD 21.3) with the shared-credential warning.
pub struct CredentialsPanel {
    shell: Entity<Shell>,
    services: Arc<dyn CloudServices>,
    shared: bool,
    user_id: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    action: Action,
}

impl CredentialsPanel {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shared = shell.read(cx).config(cx).datasource.credential_mode != "perUser";
        Self {
            shell,
            services: Services::get(cx),
            shared,
            user_id: forms::line_state(window, cx, "Runtime user id", ""),
            username: forms::line_state(window, cx, "Database user", ""),
            password: forms::secret_state(window, cx, "Password"),
            action: Action::default(),
        }
    }

    fn upload(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let window_label = self.shell.read(cx).label().to_string();
        let Some(app_id) = linked_app(self.shell.read(cx).config(cx)) else { return };
        let (user_id, username, password) = (
            forms::text_of(&self.user_id, cx).trim().to_string(),
            forms::text_of(&self.username, cx).trim().to_string(),
            forms::text_of(&self.password, cx),
        );
        let shared = self.shared;
        if !shared && (user_id.is_empty() || password.is_empty()) {
            self.action.error = Some(CloudError::new(
                "VALIDATION",
                "Enter the runtime user id and a password for that user.",
            ));
            cx.notify();
            return;
        }
        let services = self.services.clone();
        self.action.begin();
        task::spawn_call_in(
            window,
            cx,
            move || -> Result<(), CloudError> {
                let session = services.require_session()?;
                services.upload_credential(
                    &window_label,
                    &session.access_token,
                    &app_id,
                    if shared { "shared" } else { "user" },
                    (!shared).then_some(user_id),
                    (!password.is_empty()).then_some(password),
                    (!shared && !username.is_empty()).then_some(username),
                )
            },
            move |this, result, window, cx| {
                let outcome = result.map(|()| {
                    forms::set_text(&this.password, "", window, cx);
                    if shared {
                        "Shared credential encrypted and uploaded."
                    } else {
                        "Credential for that user encrypted and uploaded."
                    }
                    .to_string()
                });
                this.action.finish(outcome);
                cx.notify();
            },
        );
        cx.notify();
    }
}

impl Render for CredentialsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut card = forms::card("cloud-credentials", "Datasource credentials", cx)
            .child(widgets::text(
                "cred-warning",
                "Runtime users connect to PostgreSQL directly. An authorized user can extract the \
                 credential they receive, and ixtable roles do not limit what that credential can \
                 do in the database. Use separate, least-privileged credentials per user for strong \
                 isolation. Revoking a user stops future key grants but cannot erase a credential \
                 they already saw.",
            ))
            .child(forms::check(
                "cred-scope-shared",
                "One shared credential for every runtime user",
                self.shared,
                {
                    let entity = cx.entity();
                    move |_, _, cx| {
                        entity.update(cx, |this, cx| {
                            this.shared = true;
                            cx.notify();
                        })
                    }
                },
            ))
            .child(forms::check(
                "cred-scope-user",
                "A separate credential for one user",
                !self.shared,
                {
                    let entity = cx.entity();
                    move |_, _, cx| {
                        entity.update(cx, |this, cx| {
                            this.shared = false;
                            cx.notify();
                        })
                    }
                },
            ));
        if !self.shared {
            card = card
                .child(forms::input_field("cred-user-id", "Runtime user id", &self.user_id, cx))
                .child(forms::input_field("cred-username", "Database user", &self.username, cx));
        }
        card.child(forms::input_field("cred-password", "Password", &self.password, cx))
            .child(
                forms::actions().child(
                    forms::primary("cred-upload", "Encrypt and upload credential")
                        .disabled(self.action.busy)
                        .on_click(cx.listener(|this, _, window, cx| this.upload(window, cx))),
                ),
            )
            .child(action_status("cred", &self.action, cx))
    }
}
