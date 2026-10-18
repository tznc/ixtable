//! Start screen "Cloud apps": the applications the signed-in user may run (PRD 5 steps
//! 10-13). Port of src/cloud/CloudApps.tsx.
//!
//! Test ids: heading `cloud-apps` ("Cloud apps", in the card `cloud-apps-card`),
//! `cloud-apps-signin` ("Sign in to ixtable Cloud"), `cloud-apps-offline`,
//! `cloud-apps-loading`, `cloud-apps-empty`, per application `cloud-app:<appId>` (the
//! button, label "Open" or "Install"), `cloud-app-name:<appId>` (the application name) and
//! `cloud-app-detail:<appId>` ("Role: Sales · installed 1.0.0"), `cloud-apps-busy`,
//! transfer bar `cloud-apps-progress`, `cloud-apps-error`; the sign-in, account and
//! invitation views add theirs (`auth`).

use super::{
    auth::{AcceptInvitation, AccountLine, AuthChanged, CloudAuth, InviteEvent, SignInPanel, session_snapshot},
    errors::{CloudError, cloud_error_notice},
    open::{OpenResult, open_cloud_app},
    services::{CloudServices, Services},
    transfer::{Transfer, transfer_bar},
    types::{MemberApp, Session},
};
use crate::{
    cloud::install::LocalInstallation,
    ui::{forms, forms::eid, prelude::*},
};
use std::sync::Arc;

pub enum CloudAppsEvent {
    /// An application is installed or updated and ready: open its session.
    Opened(Box<OpenResult>),
}

#[derive(Clone)]
struct Row {
    app_id: String,
    name: String,
    role_name: String,
    installed: Option<LocalInstallation>,
}

fn merge(members: Vec<MemberApp>, local: &[LocalInstallation]) -> Vec<Row> {
    let mut rows: Vec<Row> = members
        .iter()
        .map(|m| Row {
            app_id: m.app_id.clone(),
            name: m.name.clone(),
            role_name: m.role_name.clone(),
            installed: local.iter().find(|l| l.app_id == m.app_id).cloned(),
        })
        .collect();
    // Offline (or a listing failure): installed apps still run.
    if members.is_empty() {
        for l in local {
            rows.push(Row {
                app_id: l.app_id.clone(),
                name: l.app_name.clone().unwrap_or_else(|| l.app_id.clone()),
                role_name: String::new(),
                installed: Some(l.clone()),
            });
        }
    }
    rows
}

pub struct CloudApps {
    label: String,
    services: Arc<dyn CloudServices>,
    auth: Entity<CloudAuth>,
    // The user the listing belongs to, and how often it was asked for.
    loaded_for: Option<(String, u32)>,
    revision: u32,
    apps: Option<Vec<Row>>,
    offline: bool,
    busy: String,
    error: Option<CloudError>,
    show_sign_in: bool,
    disabled: bool,
    sign_in: Option<Entity<SignInPanel>>,
    account: Option<(String, Entity<AccountLine>)>,
    accept: Option<Entity<AcceptInvitation>>,
    transfer: Transfer,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<CloudAppsEvent> for CloudApps {}

impl CloudApps {
    pub fn new(label: String, cx: &mut Context<Self>) -> Self {
        let auth = CloudAuth::global(cx);
        let subscriptions = vec![
            cx.subscribe(&auth, |this, _, _: &AuthChanged, cx| {
                this.show_sign_in = false;
                this.refresh(cx);
                cx.notify();
            }),
            cx.observe(&auth, |_, _, cx| cx.notify()),
        ];
        Self {
            label,
            services: Services::get(cx),
            auth,
            loaded_for: None,
            revision: 0,
            apps: None,
            offline: false,
            busy: String::new(),
            error: None,
            show_sign_in: false,
            disabled: false,
            sign_in: None,
            account: None,
            accept: None,
            transfer: Transfer::default(),
            _subscriptions: subscriptions,
        }
    }

    /// Disables opening while the start screen is busy.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    fn session(&self, cx: &App) -> Option<Session> {
        self.auth.read(cx).session.clone()
    }

    /// Loads the listing for the signed-in user (once per user and revision).
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session(cx) else {
            self.apps = None;
            self.loaded_for = None;
            return;
        };
        let key = (session.user_id, self.revision);
        if self.loaded_for.as_ref() == Some(&key) {
            return;
        }
        self.loaded_for = Some(key.clone());
        let (services, label) = (self.services.clone(), self.label.clone());
        task::spawn_call(
            cx,
            move || {
                let local = services.installed_apps(&label).unwrap_or_default();
                (services.list_member_apps(), local)
            },
            move |this, (members, local), cx| {
                if this.loaded_for.as_ref() != Some(&key) {
                    return;
                }
                match members {
                    Ok(members) => {
                        this.offline = false;
                        this.apps = Some(merge(members, &local));
                    }
                    Err(e) => {
                        this.offline = e.code == "CLOUD_OFFLINE";
                        if !this.offline {
                            this.error = Some(e);
                        }
                        this.apps = Some(merge(vec![], &local));
                    }
                }
                cx.notify();
            },
        );
    }

    fn open(&mut self, row: Row, cx: &mut Context<Self>) {
        self.busy = if row.installed.is_some() {
            format!("Opening {}…", row.name)
        } else {
            format!("Installing {}…", row.name)
        };
        self.error = None;
        let (services, label) = (self.services.clone(), self.label.clone());
        let transfer_id = self.transfer.begin(services.clone(), |v: &mut Self| &mut v.transfer, cx);
        task::spawn_call(
            cx,
            move || open_cloud_app(services.as_ref(), &label, &row.app_id, &transfer_id),
            |this, result, cx| {
                this.transfer.end();
                this.busy.clear();
                match result {
                    Ok(opened) => cx.emit(CloudAppsEvent::Opened(Box::new(opened))),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
}

impl Render for CloudApps {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (session, ready) = session_snapshot(&self.auth, cx);
        if !ready {
            return div().into_any_element();
        }
        self.refresh(cx);
        let mut section = forms::card("cloud-apps", "Cloud apps", cx);
        let Some(session) = session else {
            if !self.show_sign_in {
                section = section.child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .justify_start()
                        .child(forms::muted("Run the applications shared with you.", cx))
                        .child(forms::secondary("cloud-apps-signin", "Sign in to ixtable Cloud").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.show_sign_in = true;
                                cx.notify();
                            }),
                        )),
                );
            } else {
                let label = self.label.clone();
                let panel = self
                    .sign_in
                    .get_or_insert_with(|| {
                        cx.new(|cx| {
                            SignInPanel::new("Sign in to run the applications you were invited to.", label, window, cx)
                        })
                    })
                    .clone();
                section = section.child(panel);
            }
            return section.into_any_element();
        };
        let email = session.display();
        let account = match &self.account {
            Some((current, view)) if *current == email => view.clone(),
            _ => {
                let view = cx.new(|cx| AccountLine::new(email.clone(), cx));
                self.account = Some((email, view.clone()));
                view
            }
        };
        let accept = match &self.accept {
            Some(view) => view.clone(),
            None => {
                let view = cx.new(|cx| AcceptInvitation::new(window, cx));
                self._subscriptions.push(cx.subscribe(&view, |this, _, _: &InviteEvent, cx| {
                    this.revision += 1;
                    this.refresh(cx);
                }));
                self.accept = Some(view.clone());
                view
            }
        };
        section = section.child(account).child(accept);
        if self.offline {
            section = section.child(widgets::text(
                "cloud-apps-offline",
                "ixtable Cloud is not reachable. Installed applications run offline.",
            ));
        }
        match self.apps.clone() {
            None => section = section.child(widgets::text("cloud-apps-loading", "Loading cloud apps…")),
            Some(rows) if rows.is_empty() => {
                section = section.child(widgets::text("cloud-apps-empty", "No applications have been shared with you yet."))
            }
            Some(rows) => {
                for row in rows {
                    let verb = if row.installed.is_some() { "Open" } else { "Install" };
                    let detail = format!(
                        "{}{}",
                        if row.role_name.is_empty() { "Runtime user".to_string() } else { format!("Role: {}", row.role_name) },
                        match row.installed.as_ref().and_then(|i| i.version.as_ref()) {
                            Some(v) => format!(" · installed {v}"),
                            None => " · not installed".to_string(),
                        }
                    );
                    let app_id = row.app_id.clone();
                    let icon = if row.installed.is_some() { IconName::Play } else { IconName::Download };
                    let muted = cx.theme().muted_foreground;
                    section = section.child(
                        h_flex()
                            .gap_3()
                            .items_center()
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(widgets::styled_text(
                                        eid(format!("cloud-app-name:{app_id}")),
                                        row.name.clone(),
                                        |d| d.font_weight(FontWeight::SEMIBOLD),
                                    ))
                                    .child(widgets::styled_text(
                                        eid(format!("cloud-app-detail:{app_id}")),
                                        detail,
                                        move |d| d.text_sm().text_color(muted),
                                    )),
                            )
                            .child(
                                Button::new(eid(format!("cloud-app:{app_id}")))
                                    .outline()
                                    .icon(icon)
                                    .label(verb)
                                    .disabled(self.disabled || !self.busy.is_empty())
                                    .on_click(cx.listener(move |this, _, _, cx| this.open(row.clone(), cx))),
                            ),
                    );
                }
            }
        }
        if !self.busy.is_empty() {
            section = section.child(widgets::text("cloud-apps-busy", self.busy.clone()));
        }
        if let Some(bar) = transfer_bar("cloud-apps-progress", "Download progress", self.transfer.progress.as_ref(), cx) {
            section = section.child(bar);
        }
        if let Some(e) = &self.error {
            section = section.child(cloud_error_notice("cloud-apps-error", e, cx));
        }
        section.into_any_element()
    }
}
