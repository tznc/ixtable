//! Settings > Cloud (Studio): sign in, link the document to a cloud application, sync
//! roles, deliver credentials, publish checkpoints, and browse, restore or back up
//! versions (PRD 5 steps 8-9, 14; 22-23). Port of src/cloud/CloudTab.tsx.
//!
//! Test ids: `cloud-checking`; the sign-in card (`cloud-signin`, see `auth`), the account
//! line (`cloud-account`, `cloud-signout`), `cloud-app` (heading: the application's name),
//! `cloud-app-id` ("Application id ID", with the not-the-owner note), `cloud-error`; the
//! panels add their own (`studio_panels`, `publish`, `versions`).

use super::{
    auth::{AccountLine, AuthChanged, CloudAuth, SignInPanel, session_snapshot},
    errors::{CloudError, cloud_error_notice},
    open::open_in_window,
    publish::{PublishEvent, PublishPanel},
    services::{CloudServices, Services},
    studio_panels::{CredentialsPanel, LinkPanel, RolesPanel},
    types::{CloudApp, Session},
    versions::{VersionsEvent, VersionsPanel},
};
use crate::{
    ui::{forms, prelude::*, shell::Shell, store::StoreEvent},
};
use std::sync::Arc;

/// The panels of a linked document, built for one cloud application.
struct Linked {
    roles: Entity<RolesPanel>,
    credentials: Option<Entity<CredentialsPanel>>,
    publish: Entity<PublishPanel>,
    versions: Entity<VersionsPanel>,
}

pub struct CloudTab {
    shell: Entity<Shell>,
    auth: Entity<CloudAuth>,
    services: Arc<dyn CloudServices>,
    app: Option<CloudApp>,
    error: Option<CloudError>,
    // The (user, application) the loaded `app` belongs to.
    loaded_for: Option<(String, String)>,
    sign_in: Option<Entity<SignInPanel>>,
    account: Option<(String, Entity<AccountLine>)>,
    link: Option<Entity<LinkPanel>>,
    linked: Option<Linked>,
    _subscriptions: Vec<Subscription>,
}

impl CloudTab {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let auth = CloudAuth::global(cx);
        let store = shell.read(cx).store.clone();
        let subscriptions = vec![
            cx.subscribe(&auth, |this, _, _: &AuthChanged, cx| {
                this.sync_app(false, cx);
                cx.notify();
            }),
            cx.observe(&auth, |_, _, cx| cx.notify()),
            cx.subscribe(&store, |this, _, event: &StoreEvent, cx| {
                if matches!(event, StoreEvent::Changed) {
                    this.sync_app(false, cx);
                }
                cx.notify();
            }),
        ];
        let mut this = Self {
            shell,
            auth,
            services: Services::get(cx),
            app: None,
            error: None,
            loaded_for: None,
            sign_in: None,
            account: None,
            link: None,
            linked: None,
            _subscriptions: subscriptions,
        };
        this.sync_app(false, cx);
        this
    }

    fn session(&self, cx: &App) -> Option<Session> {
        self.auth.read(cx).session.clone()
    }

    /// Reads the linked application again when the user or the link changed (or `force`).
    fn sync_app(&mut self, force: bool, cx: &mut Context<Self>) {
        let user = self.session(cx).map(|s| s.user_id);
        let app_id = self.shell.read(cx).config(cx).cloud.as_ref().map(|l| l.app_id.clone());
        let key = user.zip(app_id);
        if key == self.loaded_for && !force {
            return;
        }
        self.loaded_for = key.clone();
        let Some((_, app_id)) = key else {
            self.app = None;
            return;
        };
        let services = self.services.clone();
        let expected = self.loaded_for.clone();
        task::spawn_call(cx, move || services.get_app(&app_id), move |this, result, cx| {
            if this.loaded_for != expected {
                return;
            }
            match result {
                Ok(app) => {
                    this.app = app;
                    this.error = None;
                }
                Err(e) => this.error = Some(e),
            }
            cx.notify();
        });
    }
}

impl Render for CloudTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (session, ready) = session_snapshot(&self.auth, cx);
        let mut panel = v_flex().gap_3().p_4();
        if !ready {
            return panel.child(widgets::text("cloud-checking", "Checking cloud sign-in…")).into_any_element();
        }
        let Some(session) = session else {
            let label = self.shell.read(cx).label().to_string();
            let sign_in = self
                .sign_in
                .get_or_insert_with(|| {
                    cx.new(|cx| {
                        SignInPanel::new("Sign in to publish this application to its runtime users.", label, window, cx)
                    })
                })
                .clone();
            return panel.child(sign_in).into_any_element();
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
        panel = panel.child(account);
        if let Some(e) = &self.error {
            panel = panel.child(cloud_error_notice("cloud-error", e, cx));
        }
        let config = self.shell.read(cx).config(cx).clone();
        let Some(link) = config.cloud.clone() else {
            self.linked = None;
            let shell = self.shell.clone();
            let view = self
                .link
                .get_or_insert_with(|| cx.new(|cx| LinkPanel::new(shell, window, cx)))
                .clone();
            return panel.child(view).into_any_element();
        };
        self.link = None;
        // The panels outlive a change of application (a fork), keeping their outcomes.
        if self.linked.is_none() {
            let (shell, app) = (self.shell.clone(), self.app.clone());
            let roles = cx.new(|cx| RolesPanel::new(shell.clone(), cx));
            let credentials = None;
            let publish = cx.new(|cx| PublishPanel::new(shell.clone(), app.clone(), window, cx));
            let versions = cx.new(|cx| VersionsPanel::new(shell.clone(), app, cx));
            self._subscriptions.push(cx.subscribe(&publish, |this, _, _: &PublishEvent, cx| {
                this.sync_app(true, cx);
                if let Some(linked) = &this.linked {
                    linked.versions.update(cx, |v, cx| v.reload(cx));
                }
            }));
            let open_shell = self.shell.clone();
            self._subscriptions.push(cx.subscribe(&versions, move |_, _, event: &VersionsEvent, cx| {
                let VersionsEvent::Restored(state) = event;
                open_in_window(&open_shell, state.clone(), cx);
            }));
            self.linked = Some(Linked { roles, credentials, publish, versions });
        }
        // Credentials exist only for a PostgreSQL datasource.
        let postgres = config.datasource.kind == "postgres";
        if let Some(linked) = self.linked.as_mut() {
            if !postgres {
                linked.credentials = None;
            } else if linked.credentials.is_none() {
                let shell = self.shell.clone();
                linked.credentials = Some(cx.new(|cx| CredentialsPanel::new(shell, window, cx)));
            }
        }
        // The panels that depend on the application row follow it.
        let app = self.app.clone();
        if let Some(linked) = &self.linked {
            linked.publish.update(cx, |p, cx| p.set_app(app.clone(), cx));
            if linked.versions.read(cx).app() != app.as_ref() {
                linked.versions.update(cx, |v, cx| v.set_app(app.clone(), cx));
            }
        }
        let Some(linked) = &self.linked else { return panel.into_any_element() };
        let name = self.app.as_ref().map_or("Cloud application", |a| a.name.as_str());
        let not_owner = self.app.as_ref().is_some_and(|a| a.owner_id != session.user_id);
        panel = panel
            .child(
                forms::card("cloud-app", name, cx).child(widgets::text(
                    "cloud-app-id",
                    format!(
                        "Application id {}{}",
                        link.app_id,
                        if not_owner { " · you are not this application's owner" } else { "" }
                    ),
                )),
            )
            .child(linked.roles.clone());
        if let Some(credentials) = &linked.credentials {
            panel = panel.child(credentials.clone());
        }
        panel.child(linked.publish.clone()).child(linked.versions.clone()).into_any_element()
    }
}
