//! Database login for a manually shared PostgreSQL bundle (PRD 9.3). The bundle carries
//! no password, so Runtime asks for one when none is stored or the server refuses it,
//! verifies it by connecting, and keeps it encrypted on this computer for this
//! installation. It can be changed or forgotten later. Unless the connection verifies
//! the server (verify-full), the recipient must accept the risk before the password is
//! sent; the acceptance is stored with it. Port of src/release/DatabaseLogin.tsx.
//!
//! Test ids: button `database-login-open`; dialog title `database-login`,
//! `database-login-intro`, `database-login-security`, `database-login-warning`,
//! `database-login-rejected`, `database-login-connected`, inputs `database-user` and
//! `database-password`, checkbox `database-accept-unverified`, `database-login-error`,
//! `database-connect`, `database-forget`, `database-cancel`.

use crate::{
    manager::AppError,
    ui::{dialog::dialog, forms, prelude::*, shell::Shell, widgets::Severity},
};
use gpui_kit::component::{checkbox::Checkbox, input::InputState};
use serde_json::Value;

pub struct DatabaseLogin {
    shell: Entity<Shell>,
    status: Option<Value>,
    open: bool,
    user: Entity<InputState>,
    password: Entity<InputState>,
    accept_unverified: bool,
    busy: bool,
    error: String,
    loaded_for: Option<Option<String>>,
    focus: FocusHandle,
    // The next render fills the inputs from `status` and focuses the right one.
    sync_inputs: bool,
    // The next render empties the password field (after a refused login).
    clear_password: bool,
}

fn text(status: &Value, key: &str) -> String {
    status.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn flag(status: &Value, key: &str) -> bool {
    status.get(key).and_then(Value::as_bool).unwrap_or(false)
}

impl DatabaseLogin {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe(&shell, |_, _, cx| cx.notify()).detach();
        Self {
            shell,
            status: None,
            open: false,
            user: forms::line_state(window, cx, "Database user", ""),
            password: forms::secret_state(window, cx, "Database password"),
            accept_unverified: false,
            busy: false,
            error: String::new(),
            loaded_for: None,
            focus: cx.focus_handle(),
            sync_inputs: false,
            clear_password: false,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    fn label(&self, cx: &App) -> String {
        self.shell.read(cx).label().to_string()
    }

    fn show(&mut self, next: Value) {
        self.status = Some(next);
        self.accept_unverified = false;
        self.error.clear();
        self.sync_inputs = true;
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let label = self.label(cx);
        task::spawn_call(
            cx,
            move || serde_json::to_value(crate::recordstore::runtime_login::runtime_datasource_login_status(label)?).map_err(|e| AppError::new("IO_ERROR", e)),
            |this, result, cx| {
                match result {
                    Ok(next) => {
                        this.open = flag(&next, "needsLogin");
                        this.show(next);
                    }
                    Err(_) => this.status = None,
                }
                cx.notify();
            },
        );
    }

    fn act(
        &mut self,
        work: impl FnOnce(String) -> Result<Value, AppError> + Send + 'static,
        done: impl FnOnce(&mut Self, &Value, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.busy = true;
        self.error.clear();
        cx.notify();
        let label = self.label(cx);
        task::spawn_call(cx, move || work(label), move |this, result, cx| {
            this.busy = false;
            match result {
                Ok(next) => {
                    this.show(next.clone());
                    let shell = this.shell.clone();
                    shell.update(cx, |s, cx| s.database_changed(cx));
                    done(this, &next, cx);
                }
                Err(e) => {
                    this.clear_password = true;
                    this.error = if e.code == "AUTH_FAILED" {
                        format!("The database refused this login: {}", e.message)
                    } else {
                        format!("Could not connect: {}", e.message)
                    };
                }
            }
            cx.notify();
        });
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let user = forms::text_of(&self.user, cx).trim().to_string();
        let password = forms::text_of(&self.password, cx);
        let accept = self.accept_unverified;
        self.act(
            move |label| {
                let status = crate::recordstore::runtime_login::set_runtime_datasource_login(
                    label,
                    user,
                    password,
                    Some(accept),
                )?;
                serde_json::to_value(status).map_err(|e| AppError::new("IO_ERROR", e))
            },
            |this, next, cx| {
                this.open = false;
                let user = text(next, "user");
                this.shell.update(cx, |s, cx| {
                    s.set_notice(
                        format!("Connected to the database as {user}. The login is saved on this computer."),
                        cx,
                    )
                });
            },
            cx,
        );
    }

    fn forget(&mut self, cx: &mut Context<Self>) {
        self.act(
            |label| {
                let status = crate::recordstore::runtime_login::clear_runtime_datasource_login(label)?;
                serde_json::to_value(status).map_err(|e| AppError::new("IO_ERROR", e))
            },
            |this, next, cx| {
                this.open = flag(next, "needsLogin");
                this.shell.update(cx, |s, cx| {
                    s.set_notice("The saved database login was removed from this computer.", cx)
                });
            },
            cx,
        );
    }
}

impl Render for DatabaseLogin {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Reload when the bundle version changes (an update may change the datasource).
        let version = self.shell.read(cx).doc.bundle_version.clone();
        if self.loaded_for.as_ref() != Some(&version) {
            self.loaded_for = Some(version);
            self.load(cx);
        }
        let Some(status) = self.status.clone().filter(|s| flag(s, "applies")) else {
            return div().into_any_element();
        };
        if std::mem::take(&mut self.clear_password) {
            forms::set_text(&self.password, "", window, cx);
        }
        if std::mem::take(&mut self.sync_inputs) {
            let user = text(&status, "user");
            forms::set_text(&self.user, &user, window, cx);
            forms::set_text(&self.password, "", window, cx);
            let target = if user.is_empty() { &self.user } else { &self.password };
            if self.open {
                target.update(cx, |s, cx| s.focus(window, cx));
            }
        }
        let verified = flag(&status, "serverVerified");
        let mut flow = v_flex().gap_2().child(
            forms::secondary("database-login-open", "Database login…").on_click(
                cx.listener(|this, _, _, cx| {
                    this.open = true;
                    this.sync_inputs = true;
                    cx.notify();
                }),
            ),
        );
        if self.open {
            let busy = self.busy;
            let entity = cx.entity();
            let mut panel = dialog("database-login", "Database login")
                .child(widgets::text(
                    "database-login-intro",
                    format!(
                        "This application stores its records in PostgreSQL database “{}” on {}. \
                         Enter the login you were given. ixtable checks it by connecting, then \
                         keeps it encrypted on this computer for this installation only.",
                        text(&status, "database"),
                        text(&status, "host")
                    ),
                ))
                .child(widgets::text(
                    "database-login-security",
                    format!(
                        "Connection security: sslmode {}{}",
                        text(&status, "sslmode"),
                        if verified { " (server certificate and name verified)." } else { "." }
                    ),
                ));
            if !verified {
                panel = panel.child(widgets::callout(
                    "database-login-warning",
                    Severity::Warning,
                    "This connection does not verify the database server, so someone on your \
                     network could pose as it and read your password. Continue only on a network \
                     you trust, or ask the developer to use sslmode verify-full.",
                    None,
                    cx,
                ));
            }
            if text(&status, "reason") == "rejected" {
                panel = panel.child(widgets::text(
                    "database-login-rejected",
                    "The database refused the saved login. Enter it again.",
                ));
            }
            if flag(&status, "attached") {
                panel = panel.child(widgets::text(
                    "database-login-connected",
                    format!("Connected as {}.", text(&status, "user")),
                ));
            }
            panel = panel
                .child(forms::input_field("database-user", "Database user", &self.user, cx))
                .child(forms::input_field(
                    "database-password",
                    "Database password",
                    &self.password,
                    cx,
                ));
            if !verified {
                let this = cx.entity();
                panel = panel.child(
                    Checkbox::new("database-accept-unverified")
                        .label("I accept sending my password over a connection that does not verify the server")
                        .checked(self.accept_unverified)
                        .on_click(move |checked, _, cx| {
                            let checked = *checked;
                            this.update(cx, |t, cx| {
                                t.accept_unverified = checked;
                                cx.notify();
                            })
                        }),
                );
            }
            if !self.error.is_empty() {
                panel = panel.child(widgets::callout(
                    "database-login-error",
                    Severity::Danger,
                    &self.error,
                    None,
                    cx,
                ));
            }
            let can_connect = !busy && (verified || self.accept_unverified);
            let close = cx.listener(|this, _, _, cx| {
                this.open = false;
                cx.notify();
            });
            let panel = panel
                .footer(
                    forms::secondary("database-cancel", "Cancel")
                        .disabled(busy)
                        .on_click(close),
                )
                .when(text(&status, "source") == "installation", |d| {
                    d.footer(
                        forms::danger("database-forget", "Forget saved login")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.forget(cx))),
                    )
                })
                .footer(
                    forms::primary("database-connect", "Connect")
                        .disabled(!can_connect)
                        .on_click(cx.listener(|this, _, _, cx| this.submit(cx))),
                )
                .on_escape(&self.focus, busy, move |_, cx| {
                    entity.update(cx, |this, cx| {
                        this.open = false;
                        cx.notify();
                    })
                })
                .modal();
            flow = flow.child(panel);
        }
        flow.into_any_element()
    }
}
