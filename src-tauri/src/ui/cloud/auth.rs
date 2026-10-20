//! Cloud sign-in: the session model, email/password and browser sign-in, invitations
//! and the account line. Port of src/cloud/auth.tsx, client.ts's session store and
//! useCloudSession.ts.
//!
//! Test ids: sign-in card `cloud-signin` (title "Sign in to ixtable Cloud"), inputs
//! `cloud-email` and `cloud-password`, buttons `cloud-signin-submit`, `cloud-signup`,
//! `cloud-forgot`, `cloud-google`, `cloud-microsoft`, `cloud-handoff-cancel`; texts
//! `cloud-purpose`, `cloud-busy`, `cloud-handoff`, `cloud-handoff-url`, `cloud-notice`
//! and the error `cloud-signin-error`. Invitations: `invite-open`, input `invite-link`,
//! `invite-accept`, `invite-cancel`, `invite-notice`, `invite-error`. Account line:
//! `cloud-account` ("Signed in as EMAIL") and `cloud-signout`.

use super::{
    contract,
    errors::{CloudError, cloud_error_notice},
    services::{CloudServices, Services},
    types::{Session, SessionTokens},
};
use crate::ui::{forms, prelude::*, task};
use gpui_kit::component::{Disableable as _, input::InputState};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

const POLL: Duration = Duration::from_secs(2);
const POLL_LIMIT: usize = 450;

// ---- the session model

/// The signed-in cloud session (None when signed out), shared by every cloud view.
pub struct CloudAuth {
    pub session: Option<Session>,
    /// The stored session has been looked at.
    pub ready: bool,
    loading: bool,
}

pub struct AuthChanged;
impl EventEmitter<AuthChanged> for CloudAuth {}

struct AuthGlobal(Entity<CloudAuth>);
impl Global for AuthGlobal {}

impl CloudAuth {
    pub fn global(cx: &mut App) -> Entity<CloudAuth> {
        if cx.try_global::<AuthGlobal>().is_none() {
            let model = cx.new(|_| CloudAuth { session: None, ready: false, loading: false });
            cx.set_global(AuthGlobal(model));
        }
        cx.global::<AuthGlobal>().0.clone()
    }

    /// Loads the stored session (refreshing it when needed) the first time it is needed.
    pub fn ensure_loaded(&mut self, cx: &mut Context<Self>) {
        if self.ready || self.loading {
            return;
        }
        self.loading = true;
        let services = Services::get(cx);
        task::spawn_call(cx, move || services.load_session(), |this, session, cx| {
            this.loading = false;
            this.publish(session, cx);
        });
    }

    pub fn publish(&mut self, session: Option<Session>, cx: &mut Context<Self>) {
        if self.ready && self.session == session {
            return;
        }
        self.session = session;
        self.ready = true;
        cx.emit(AuthChanged);
        cx.notify();
    }
}

/// Reads the session model, starting its first load.
pub fn session_snapshot(auth: &Entity<CloudAuth>, cx: &mut App) -> (Option<Session>, bool) {
    auth.update(cx, |a, cx| a.ensure_loaded(cx));
    let a = auth.read(cx);
    (a.session.clone(), a.ready)
}

// ---- email/password and browser sign-in

pub struct SignInPanel {
    purpose: String,
    auth: Entity<CloudAuth>,
    services: Arc<dyn CloudServices>,
    label: String,
    email: Entity<InputState>,
    password: Entity<InputState>,
    busy: String,
    error: Option<CloudError>,
    notice: String,
    handoff: Option<(String, bool)>,
    cancelled: Arc<AtomicBool>,
}

impl SignInPanel {
    pub fn new(purpose: &str, label: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let auth = CloudAuth::global(cx);
        Self {
            purpose: purpose.to_string(),
            auth,
            services: Services::get(cx),
            label,
            email: forms::line_state(window, cx, "Email", ""),
            password: forms::secret_state(window, cx, "Password"),
            busy: String::new(),
            error: None,
            notice: String::new(),
            handoff: None,
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    fn start(&mut self, busy: &str, cx: &mut Context<Self>) {
        self.busy = busy.to_string();
        self.error = None;
        self.notice.clear();
        cx.notify();
    }

    fn fail(&mut self, error: CloudError, cx: &mut Context<Self>) {
        self.busy.clear();
        self.error = Some(error);
        cx.notify();
    }

    fn submit(&mut self, sign_up: bool, cx: &mut Context<Self>) {
        let (email, password) = (forms::text_of(&self.email, cx), forms::text_of(&self.password, cx));
        if email.trim().is_empty() || password.chars().count() < 8 {
            self.error = Some(CloudError::new(
                "VALIDATION",
                "Enter your email and a password of at least 8 characters.",
            ));
            cx.notify();
            return;
        }
        self.start(if sign_up { "Creating account…" } else { "Signing in…" }, cx);
        let services = self.services.clone();
        task::spawn_call(
            cx,
            move || {
                if sign_up {
                    services.sign_up(&email, &password)
                } else {
                    services.sign_in(&email, &password)
                }
            },
            |this, result, cx| match result {
                Ok(session) => {
                    this.busy.clear();
                    if session.is_none() {
                        this.notice = "Check your email to confirm the account, then sign in.".into();
                    }
                    this.auth.update(cx, |a, cx| a.publish(session, cx));
                    cx.notify();
                }
                Err(e) => this.fail(e, cx),
            },
        );
    }

    fn forgot(&mut self, cx: &mut Context<Self>) {
        let email = forms::text_of(&self.email, cx).trim().to_string();
        self.start("Sending the reset email…", cx);
        if email.is_empty() {
            return self.fail(
                CloudError::new("VALIDATION", "Enter your email address, then choose Forgot password."),
                cx,
            );
        }
        let services = self.services.clone();
        let asked = email.clone();
        task::spawn_call(cx, move || services.request_password_reset(&asked), move |this, result, cx| {
            match result {
                Ok(()) => {
                    this.busy.clear();
                    this.notice = format!(
                        "If {email} has an account, a password reset email is on its way. Open its link to choose a new password, then sign in here."
                    );
                    cx.notify();
                }
                Err(e) => this.fail(e, cx),
            }
        });
    }

    fn browser(&mut self, provider: &'static str, cx: &mut Context<Self>) {
        self.start("Waiting for the browser sign-in…", cx);
        self.cancelled.store(false, Ordering::SeqCst);
        let (services, label, cancelled) =
            (self.services.clone(), self.label.clone(), self.cancelled.clone());
        cx.spawn(async move |this, cx| {
            let begin = {
                let (services, label) = (services.clone(), label.clone());
                cx.background_executor()
                    .spawn(async move { services.desktop_auth_start(&label, provider) })
                    .await
            };
            let start = match begin {
                Ok(start) => start,
                Err(e) => {
                    let _ = this.update(cx, |this, cx| this.fail(e, cx));
                    return;
                }
            };
            let (url, opened) = (start.url.clone(), start.opened);
            let _ = this.update(cx, |this, cx| {
                this.handoff = Some((url, opened));
                cx.notify();
            });
            for _ in 0..POLL_LIMIT {
                if cancelled.load(Ordering::SeqCst) {
                    break;
                }
                let polled = {
                    let (services, label, state) = (services.clone(), label.clone(), start.state.clone());
                    cx.background_executor()
                        .spawn(async move { services.desktop_auth_poll(&label, &state) })
                        .await
                };
                match polled {
                    Err(e) => {
                        let _ = this.update(cx, |this, cx| {
                            this.handoff = None;
                            this.fail(e, cx)
                        });
                        return;
                    }
                    Ok(poll) => {
                        if let (true, Some(session)) = (poll.status == "approved", poll.session) {
                            let tokens = SessionTokens {
                                access_token: session["access_token"].as_str().unwrap_or("").into(),
                                refresh_token: session["refresh_token"].as_str().unwrap_or("").into(),
                            };
                            let services = services.clone();
                            let adopted = cx
                                .background_executor()
                                .spawn(async move { services.adopt_session(tokens) })
                                .await;
                            let _ = this.update(cx, |this, cx| {
                                this.handoff = None;
                                match adopted {
                                    Ok(session) => {
                                        this.busy.clear();
                                        this.auth.update(cx, |a, cx| a.publish(session, cx));
                                        cx.notify();
                                    }
                                    Err(e) => this.fail(e, cx),
                                }
                            });
                            return;
                        }
                    }
                }
                cx.background_executor().timer(POLL).await;
            }
            let _ = this.update(cx, |this, cx| {
                this.handoff = None;
                if cancelled.load(Ordering::SeqCst) {
                    this.busy.clear();
                    cx.notify();
                } else {
                    this.fail(
                        CloudError::new("AUTH_EXPIRED", "The browser sign-in was not completed in time."),
                        cx,
                    );
                }
            });
        })
        .detach();
    }
}

impl Drop for SignInPanel {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }
}

impl Render for SignInPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = !self.busy.is_empty();
        let mut card = forms::card("cloud-signin", "Sign in to ixtable Cloud", cx)
            .child(widgets::text("cloud-purpose", self.purpose.clone()))
            .child(forms::input_field("cloud-email", "Email", &self.email, cx))
            .child(forms::input_field("cloud-password", "Password", &self.password, cx))
            .child(
                forms::actions()
                    .child(
                        forms::primary("cloud-signin-submit", "Sign in")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.submit(false, cx))),
                    )
                    .child(
                        forms::secondary("cloud-signup", "Create account")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.submit(true, cx))),
                    )
                    .child(
                        forms::link("cloud-forgot", "Forgot password?")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.forgot(cx))),
                    ),
            )
            .child(
                forms::actions()
                    .child(
                        forms::secondary("cloud-google", "Sign in with Google")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.browser("google", cx))),
                    )
                    .child(
                        forms::secondary("cloud-microsoft", "Sign in with Microsoft")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.browser("azure", cx))),
                    ),
            );
        if let Some((url, opened)) = &self.handoff {
            card = card.child(widgets::text(
                "cloud-handoff",
                if *opened {
                    "Finish signing in in your browser. This window continues automatically."
                } else {
                    "Open this address in your browser to finish signing in:"
                },
            ));
            if !opened {
                card = card.child(widgets::text("cloud-handoff-url", url.clone()));
            }
            card = card.child(forms::link("cloud-handoff-cancel", "Cancel").on_click(cx.listener(
                |this, _, _, cx| {
                    this.cancelled.store(true, Ordering::SeqCst);
                    this.handoff = None;
                    this.busy.clear();
                    cx.notify();
                },
            )));
        }
        if busy {
            card = card.child(widgets::text("cloud-busy", self.busy.clone()));
        }
        if !self.notice.is_empty() {
            card = card.child(widgets::text("cloud-notice", self.notice.clone()));
        }
        if let Some(e) = &self.error {
            card = card.child(cloud_error_notice("cloud-signin-error", e, cx));
        }
        card
    }
}

// ---- invitations

pub enum InviteEvent {
    Accepted,
}

/// Accepts an emailed invitation from the desktop (`invitations-accept`).
pub struct AcceptInvitation {
    services: Arc<dyn CloudServices>,
    link: Entity<InputState>,
    open: bool,
    busy: bool,
    error: Option<CloudError>,
    notice: String,
}
impl EventEmitter<InviteEvent> for AcceptInvitation {}

impl AcceptInvitation {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            services: Services::get(cx),
            link: forms::line_state(window, cx, "https://…/invite?token=…", ""),
            open: false,
            busy: false,
            error: None,
            notice: String::new(),
        }
    }

    fn accept(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.error = None;
        self.notice.clear();
        let token = contract::invitation_token(&forms::text_of(&self.link, cx));
        if token.is_empty() {
            self.error = Some(CloudError::new(
                "VALIDATION",
                "Paste the invitation link from the email (it contains token=…).",
            ));
            cx.notify();
            return;
        }
        self.busy = true;
        cx.notify();
        let services = self.services.clone();
        task::spawn_call_in(
            window,
            cx,
            move || {
                let reply = services.invoke("invitations-accept", serde_json::json!({ "token": token }))?;
                contract::invitations_accept(&reply)
            },
            |this, result, window, cx| {
                this.busy = false;
                match result {
                    Ok(membership) => {
                        forms::set_text(&this.link, "", window, cx);
                        this.open = false;
                        this.notice = if membership.kind == "app" {
                            "Invitation accepted. The application is listed below."
                        } else {
                            "Invitation accepted. You joined the organization."
                        }
                        .into();
                        cx.emit(InviteEvent::Accepted);
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }
}

impl Render for AcceptInvitation {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut block = v_flex().gap_2();
        if self.open {
            let blank = forms::text_of(&self.link, cx).trim().is_empty();
            block = block
                .child(forms::input_field("invite-link", "Invitation link", &self.link, cx))
                .child(
                    forms::actions()
                        .child(
                            forms::primary("invite-accept", "Accept invitation")
                                .disabled(self.busy || blank)
                                .on_click(cx.listener(|this, _, window, cx| this.accept(window, cx))),
                        )
                        .child(
                            forms::secondary("invite-cancel", "Cancel")
                                .disabled(self.busy)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open = false;
                                    cx.notify();
                                })),
                        ),
                );
        } else {
            block = block.child(h_flex().child(
                forms::link("invite-open", "Accept an invitation…").on_click(cx.listener(|this, _, _, cx| {
                    this.open = true;
                    cx.notify();
                })),
            ));
        }
        if !self.notice.is_empty() {
            block = block.child(widgets::text("invite-notice", self.notice.clone()));
        }
        if let Some(e) = &self.error {
            block = block.child(cloud_error_notice("invite-error", e, cx));
        }
        block
    }
}

// ---- the account line

/// "Signed in as …" with a sign-out button.
pub struct AccountLine {
    email: String,
    auth: Entity<CloudAuth>,
    services: Arc<dyn CloudServices>,
    busy: bool,
}

impl AccountLine {
    pub fn new(email: String, cx: &mut Context<Self>) -> Self {
        Self { email, auth: CloudAuth::global(cx), services: Services::get(cx), busy: false }
    }
}

impl Render for AccountLine {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(widgets::text("cloud-account", format!("Signed in as {}", self.email)))
            .child(forms::link("cloud-signout", "Sign out").disabled(self.busy).on_click(cx.listener(
                |this, _, _, cx| {
                    this.busy = true;
                    cx.notify();
                    let services = this.services.clone();
                    task::spawn_call(cx, move || services.sign_out(), |this, (), cx| {
                        this.busy = false;
                        this.auth.update(cx, |a, cx| a.publish(None, cx));
                        cx.notify();
                    });
                },
            )))
    }
}
