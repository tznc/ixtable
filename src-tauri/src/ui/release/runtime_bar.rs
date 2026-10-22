//! Sidebar block for runtime-only windows: bundle name and version, manual update,
//! database login and data reset. Port of src/release/RuntimeBar.tsx.
//!
//! Test ids: `runtime-bundle` (kicker, the region), `runtime-name`, `runtime-version`
//! ("Version 1.0.0"), `check-update` (the update flow's button), `reset-installation`,
//! reset dialog title `reset-dialog`, `reset-message`, `reset-table:<name>`,
//! `reset-confirm`, `reset-keep`, `runtime-error`; the flow and login views add theirs.

use super::{
    bundle_error::bundle_error_banner,
    bundle_flow::{Act, BundleEvent, BundleFlow},
    database_login::DatabaseLogin,
};
use crate::{
    installation::{InstalledBundle, ResetPreview},
    manager::AppError,
    ui::{dialog::dialog, forms, forms::eid, prelude::*, shell::Shell},
};
use std::sync::Arc;

pub struct RuntimeBar {
    shell: Entity<Shell>,
    info: Option<InstalledBundle>,
    info_for: Option<Option<String>>,
    preview: Option<ResetPreview>,
    error: Option<AppError>,
    busy: bool,
    update: Entity<BundleFlow>,
    login: Entity<DatabaseLogin>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl RuntimeBar {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let label = shell.read(cx).label().to_string();
        let act: Act = {
            let label = label.clone();
            Arc::new(move |path, password, sha| {
                crate::installation_commands::update_runtime_installation(
                    label.clone(),
                    path.to_string(),
                    (!password.is_empty()).then(|| password.to_string()),
                    sha.to_string(),
                )
            })
        };
        let update = cx.new(|cx| {
            BundleFlow::new(label, Some(("check-update", "Check for update…")), act, true, window, cx)
        });
        let login = cx.new(|cx| DatabaseLogin::new(shell.clone(), window, cx));
        let subscriptions = vec![
            cx.observe(&shell, |_, _, cx| cx.notify()),
            // A refusal replaces any success toast that is still showing.
            cx.observe(&update, |this, update, cx| {
                if update.read(cx).has_error() {
                    this.shell.update(cx, |s, cx| {
                        s.notice = None;
                        cx.notify();
                    });
                }
            }),
            cx.subscribe(&update, |this, _, event: &BundleEvent, cx| {
                if let BundleEvent::Done(state, _) = event {
                    this.updated(state.clone(), cx);
                }
            }),
        ];
        Self {
            shell,
            info: None,
            info_for: None,
            preview: None,
            error: None,
            busy: false,
            update,
            login,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    fn updated(&mut self, state: crate::manager::SessionState, cx: &mut Context<Self>) {
        let version = state.bundle_version.clone().unwrap_or_default();
        self.refresh(&state, cx);
        self.shell.update(cx, |s, cx| {
            s.set_notice(format!("Updated to version {version}. Your records were kept."), cx)
        });
    }

    /// Adopts a new session state and reloads the definition and the data.
    fn refresh(&mut self, state: &crate::manager::SessionState, cx: &mut Context<Self>) {
        let shell = self.shell.clone();
        shell.update(cx, |s, cx| s.apply_session(state, cx));
        let store = shell.read(cx).store.clone();
        let _ = store.update(cx, |s, cx| s.reload(None, cx));
        shell.update(cx, |s, cx| s.database_changed(cx));
    }

    fn label(&self, cx: &App) -> String {
        self.shell.read(cx).label().to_string()
    }

    fn load_info(&mut self, cx: &mut Context<Self>) {
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || crate::installation_commands::runtime_installation_info(label),
            |this, result, cx| {
                this.info = result.ok();
                cx.notify();
            },
        );
    }

    fn show_reset(&mut self, cx: &mut Context<Self>) {
        self.error = None;
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || crate::installation_commands::preview_installation_reset(label),
            |this, result, cx| {
                match result {
                    Ok(preview) => this.preview = Some(preview),
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error = None;
        cx.notify();
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || crate::installation_commands::reset_runtime_installation_data(label, true),
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(state) => {
                        this.preview = None;
                        this.refresh(&state, cx);
                        this.shell.update(cx, |s, cx| {
                            s.set_notice(
                                "Installation data was reset from the bundle. A recovery copy was kept.",
                                cx,
                            )
                        });
                    }
                    Err(e) => this.error = Some(e),
                }
                cx.notify();
            },
        );
    }
}

impl Render for RuntimeBar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (name, version) = {
            let shell = self.shell.read(cx);
            (shell.doc.name.clone(), shell.doc.bundle_version.clone())
        };
        if self.info_for.as_ref() != Some(&version) {
            self.info_for = Some(version.clone());
            self.load_info(cx);
        }
        let busy = self.busy;
        let shown_version = version.or_else(|| self.info.as_ref().map(|i| i.version.clone()));
        let mut bar = v_flex()
            .gap_2()
            .p_3()
            .child(widgets::styled_text("runtime-bundle", "RUNTIME BUNDLE", |d| {
                d.ty(Type::Caption).text_color(cx.theme().muted_foreground)
            }))
            .child(widgets::styled_text(
                "runtime-name",
                self.info.as_ref().map_or(name, |i| i.name.clone()),
                |d| d.font_weight(FontWeight::SEMIBOLD),
            ))
            .child(widgets::text(
                "runtime-version",
                format!("Version {}", shown_version.unwrap_or_default()),
            ))
            .child(self.update.clone())
            .child(self.login.clone())
            .child(
                forms::danger("reset-installation", "Reset installation data…")
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.show_reset(cx))),
            );
        if let Some(preview) = self.preview.clone() {
            if !self.focus.contains_focused(window, cx) {
                window.focus(&self.focus, cx);
            }
            let entity = cx.entity();
            let mut panel = dialog("reset-dialog", "Reset installation data?")
                .child(widgets::text("reset-message", preview.message.clone()));
            for table in &preview.current {
                let after = preview
                    .bundled
                    .iter()
                    .find(|b| b.name == table.name)
                    .map_or(0, |b| b.rows);
                panel = panel.child(widgets::text(
                    eid(format!("reset-table:{}", table.name)),
                    format!("{}: {} now, {} after reset", table.name, table.rows, after),
                ));
            }
            let panel = panel
                .footer(forms::secondary("reset-keep", "Keep my records").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.preview = None;
                        cx.notify();
                    },
                )))
                .footer(
                    forms::danger("reset-confirm", "Replace all records")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.reset(cx))),
                )
                .on_escape(&self.focus, busy, move |_, cx| {
                    entity.update(cx, |this, cx| {
                        this.preview = None;
                        cx.notify();
                    })
                })
                .modal();
            bar = bar.child(panel);
        }
        if let Some(e) = &self.error {
            bar = bar.child(bundle_error_banner("runtime-error", e, cx));
        }
        bar
    }
}
