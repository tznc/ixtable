//! Choose a `.ixtr` file, verify it, ask for its password when protected, then run `act`
//! (open or update). An update or downgrade first shows its release notes and pending
//! migrations for confirmation; a downgrade is applied only from its explicit button.
//! Port of src/release/BundleFileFlow.tsx.
//!
//! Test ids: the button (`id` given to `new`, only when `show_button`), `bundle-progress`,
//! password dialog title `bundle-password` ("Password for NAME VERSION"), input
//! `bundle-password-input`, `bundle-unlock`, `bundle-password-cancel`, the update
//! confirm (see `update_confirm`), `bundle-error`, `bundle-downgrade`.

use super::{
    bundle_error::{bundle_error_banner, is_password_error},
    update_confirm::{UpdateConfirmProps, update_confirm},
};
use crate::{
    installation::BundleSummary,
    installation_checks::PendingMigration,
    manager::{AppError, SessionState},
    ui::{
        dialogs::{self, FileFilter},
        forms,
        prelude::*,
    },
};
use gpui_kit::component::input::InputState;
use std::sync::Arc;

pub const IXTR: FileFilter = FileFilter {
    name: "ixtable runtime bundles",
    extensions: &["ixtr"],
};

/// Opens or updates from a verified bundle: `(path, password, allow_downgrade, sha256)`.
pub type Act =
    Arc<dyn Fn(&str, &str, bool, &str) -> Result<SessionState, AppError> + Send + Sync>;

pub enum BundleEvent {
    Done(SessionState, Box<BundleSummary>),
    Canceled,
}

#[derive(Clone)]
struct Pending {
    path: String,
    summary: BundleSummary,
}

struct Confirm {
    pending: Pending,
    secret: String,
    migrations: Option<Vec<PendingMigration>>,
}

pub struct BundleFlow {
    label: String,
    button: Option<(&'static str, &'static str)>,
    act: Act,
    /// Dialogs float above the window (for a flow inside the narrow sidebar).
    overlaid: bool,
    busy: bool,
    error: Option<AppError>,
    prompt: Option<Pending>,
    downgrade: Option<Pending>,
    confirm: Option<Confirm>,
    password: Entity<InputState>,
    focus: FocusHandle,
    // Set when a dialog opens or closes; the next render (which has the window) moves
    // focus into the dialog and clears the password field.
    focus_prompt: bool,
    focus_confirm: bool,
    clear_password: bool,
}
impl EventEmitter<BundleEvent> for BundleFlow {}

impl BundleFlow {
    /// `button`: the (id, label) of the button this flow renders, or None when the host
    /// has its own button and calls `start` / `verify`.
    pub fn new(
        label: String,
        button: Option<(&'static str, &'static str)>,
        act: Act,
        overlaid: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            label,
            button,
            act,
            overlaid,
            busy: false,
            error: None,
            prompt: None,
            downgrade: None,
            confirm: None,
            password: forms::secret_state(window, cx, "Bundle password"),
            focus: cx.focus_handle(),
            focus_prompt: false,
            focus_confirm: false,
            clear_password: false,
        }
    }

    fn show_prompt(&mut self, pending: Pending) {
        self.prompt = Some(pending);
        self.focus_prompt = true;
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    fn reset(&mut self) {
        self.error = None;
        self.downgrade = None;
        self.confirm = None;
        self.prompt = None;
    }

    /// Asks for a bundle file, then verifies it.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.reset();
        cx.notify();
        let pick = dialogs::pick_file(cx, "Open ixtable runtime bundle", &[IXTR]);
        task::after(cx, pick, |this, path, cx| match path {
            Some(path) => this.verify(path.to_string_lossy().into_owned(), cx),
            None => cx.emit(BundleEvent::Canceled),
        });
    }

    /// Verifies a bundle the OS asked to open (no file dialog).
    pub fn verify(&mut self, path: String, cx: &mut Context<Self>) {
        self.reset();
        self.busy = true;
        cx.notify();
        let label = self.label.clone();
        let inspected = path.clone();
        task::spawn_call(
            cx,
            move || crate::installation_commands::inspect_runtime_bundle(label, inspected),
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(summary) if summary.encrypted => {
                        this.show_prompt(Pending { path, summary });
                        cx.notify();
                    }
                    Ok(summary) => this.proceed(Pending { path, summary }, String::new(), cx),
                    Err(e) => {
                        this.error = Some(e);
                        cx.notify();
                    }
                }
            },
        );
    }

    fn attempt(&mut self, pending: Pending, secret: String, allow_downgrade: bool, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        cx.notify();
        let act = self.act.clone();
        let (path, sha) = (pending.path.clone(), pending.summary.sha256.clone());
        let secret_for_act = secret.clone();
        task::spawn_call(
            cx,
            move || act(&path, &secret_for_act, allow_downgrade, &sha),
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(state) => {
                        this.prompt = None;
                        this.downgrade = None;
                        this.confirm = None;
                        this.clear_password = true;
                        cx.emit(BundleEvent::Done(state, Box::new(pending.summary)));
                    }
                    Err(e) => {
                        if e.code == "BUNDLE_DOWNGRADE" {
                            this.downgrade = Some(pending.clone());
                        }
                        if is_password_error(&e) {
                            this.show_prompt(pending);
                        }
                        this.error = Some(e);
                    }
                }
                cx.notify();
            },
        );
    }

    /// Updates and downgrades stop at the confirm step; everything else runs `act`.
    fn proceed(&mut self, pending: Pending, secret: String, cx: &mut Context<Self>) {
        let summary = &pending.summary;
        if summary.action != "update" && summary.action != "downgrade" {
            return self.attempt(pending, secret, false, cx);
        }
        self.busy = true;
        self.error = None;
        cx.notify();
        let known = summary.pending_migrations.clone();
        let unavailable = summary.migrations_unavailable.is_some();
        let (label, path, password) = (self.label.clone(), pending.path.clone(), secret.clone());
        task::spawn_call(
            cx,
            move || -> Result<Option<Vec<PendingMigration>>, AppError> {
                match known {
                    Some(list) => Ok(Some(list)),
                    None if unavailable => Ok(None),
                    None => crate::installation_commands::preview_runtime_update(
                        label,
                        path,
                        (!password.is_empty()).then_some(password),
                    )
                    .map(Some),
                }
            },
            move |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(migrations) => {
                        this.prompt = None;
                        this.confirm = Some(Confirm { pending, secret, migrations });
                        this.focus_confirm = true;
                    }
                    Err(e) => {
                        if is_password_error(&e) {
                            this.show_prompt(pending);
                        }
                        this.error = Some(e);
                    }
                }
                cx.notify();
            },
        );
    }

    fn cancel_prompt(&mut self, cx: &mut Context<Self>) {
        self.prompt = None;
        self.error = None;
        self.clear_password = true;
        cx.emit(BundleEvent::Canceled);
        cx.notify();
    }

    fn unlock(&mut self, cx: &mut Context<Self>) {
        let secret = forms::text_of(&self.password, cx);
        if let Some(pending) = self.prompt.clone() {
            if !secret.is_empty() {
                self.proceed(pending, secret, cx);
            }
        }
    }
}

impl Render for BundleFlow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy;
        if std::mem::take(&mut self.clear_password) {
            forms::set_text(&self.password, "", window, cx);
        }
        if std::mem::take(&mut self.focus_prompt) {
            self.password.update(cx, |s, cx| s.focus(window, cx));
        }
        if std::mem::take(&mut self.focus_confirm) {
            window.focus(&self.focus, cx);
        }
        let mut flow = v_flex().gap_2();
        if let Some((id, label)) = self.button {
            flow = flow.child(
                forms::secondary(id, label)
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.start(cx))),
            );
        }
        if busy {
            flow = flow.child(widgets::text("bundle-progress", "Verifying bundle…"));
        }
        if let Some(prompt) = self.prompt.clone() {
            let title = format!("Password for {} {}", prompt.summary.name, prompt.summary.version);
            let entity = cx.entity();
            let has_password = !forms::text_of(&self.password, cx).is_empty();
            let panel = forms::escapable(
                forms::dialog("bundle-password", &title, cx)
                    .child(forms::input_field(
                        "bundle-password-input",
                        "Bundle password",
                        &self.password,
                        cx,
                    ))
                    .child(
                        forms::actions()
                            .child(
                                forms::primary("bundle-unlock", "Unlock")
                                    .disabled(busy || !has_password)
                                    .on_click(cx.listener(|this, _, _, cx| this.unlock(cx))),
                            )
                            .child(
                                forms::secondary("bundle-password-cancel", "Cancel")
                                    .disabled(busy)
                                    .on_click(cx.listener(|this, _, _, cx| this.cancel_prompt(cx))),
                            ),
                    ),
                &self.focus,
                busy,
                move |_, cx| entity.update(cx, |this, cx| this.cancel_prompt(cx)),
            );
            flow = flow.child(forms::placed(panel, self.overlaid));
        }
        if let Some(confirm) = &self.confirm {
            let summary = &confirm.pending.summary;
            let downgrade = summary.action == "downgrade";
            let entity = cx.entity();
            let cancel_entity = cx.entity();
            let (to_apply, secret) = (confirm.pending.clone(), confirm.secret.clone());
            let panel = update_confirm(
                UpdateConfirmProps {
                    name: &summary.name,
                    version: &summary.version,
                    installed_version: summary.installed_version.as_deref(),
                    release_notes: &summary.release_notes,
                    migrations: confirm.migrations.as_deref(),
                    migrations_unavailable: summary.migrations_unavailable.as_deref(),
                    downgrade,
                    busy,
                },
                move |_, cx| {
                    let (pending, secret) = (to_apply.clone(), secret.clone());
                    entity.update(cx, |this, cx| this.attempt(pending, secret, downgrade, cx));
                },
                move |_, cx| {
                    cancel_entity.update(cx, |this, cx| {
                        this.confirm = None;
                        this.clear_password = true;
                        cx.emit(BundleEvent::Canceled);
                        cx.notify();
                    })
                },
                cx,
            );
            flow = flow.child(forms::placed(panel, self.overlaid));
        }
        if let Some(e) = &self.error {
            flow = flow.child(bundle_error_banner("bundle-error", e, cx));
        }
        if let Some(older) = self.downgrade.clone() {
            let secret = forms::text_of(&self.password, cx);
            let label = format!("Install older version {} anyway", older.summary.version);
            flow = flow.child(
                forms::secondary("bundle-downgrade", &label)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.attempt(older.clone(), secret.clone(), true, cx)
                    })),
            );
        }
        flow
    }
}
