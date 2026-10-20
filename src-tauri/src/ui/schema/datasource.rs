//! Settings > Datasource (port of `src/schema/DatasourceTab.tsx`): embedded SQLite or a
//! developer-supplied PostgreSQL (PRD 9, 21.4).
//!
//! Test ids: selects `Record store`, `TLS (sslmode)`, `Credential mode`; inputs `Host`,
//! `Port`, `Database`, `User`, `Password`, `Schema`; `shared-credentials-note`,
//! `plaintext-warning`, checkbox `accept-plaintext`; buttons `test-connection`,
//! `forget-password`, `save-datasource`, `capabilities-toggle` (label "<Store> store
//! capabilities"); results `connection-report`, `datasource-status`, `datasource-error`;
//! capability tables per `capability.rs`.

use crate::ui::controls::{check, error_line, input_text, new_input, note, select, set_input, text_input};
use super::{
    capability::{capability_summary, mapping_tables},
    logical::store_label,
};
use crate::manager::AppError;
use crate::recordstore::{
    commands::{
        clear_datasource_password, connect_datasource, set_datasource_password, store_capabilities,
        test_datasource_connection, ConnectionReport,
    },
    DatasourceConfig, StoreCapabilities,
};
use crate::ui::{prelude::*, shell::Shell, store::StoreEvent, task, widgets};
use gpui_kit::component::input::{InputEvent, InputState};

const SSL_MODES: &[(&str, &str)] = &[
    ("verify-full", "verify-full (TLS, verify certificate and host)"),
    ("verify-ca", "verify-ca (TLS, verify certificate)"),
    ("require", "require (TLS, no verification)"),
    ("prefer", "prefer (TLS if offered, else plaintext)"),
    ("disable", "disable (no TLS)"),
];

pub struct DatasourceTab {
    shell: Entity<Shell>,
    draft: DatasourceConfig,
    // What the inputs were last filled from, to notice an outside change.
    synced: DatasourceConfig,
    host: Entity<InputState>,
    port: Entity<InputState>,
    database: Entity<InputState>,
    user: Entity<InputState>,
    password: Entity<InputState>,
    schema: Entity<InputState>,
    report: Option<ConnectionReport>,
    status: String,
    error: String,
    busy: bool,
    capabilities: Option<StoreCapabilities>,
    expanded: bool,
    _subscriptions: Vec<Subscription>,
}

impl DatasourceTab {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store = shell.read(cx).store.clone();
        let current = shell.read(cx).config(cx).datasource.clone();
        let host = new_input(window, cx, "", &current.host);
        let port = new_input(window, cx, "", &current.port.to_string());
        let database = new_input(window, cx, "", &current.database);
        let user = new_input(window, cx, "", &current.user);
        let schema = new_input(window, cx, "", &current.schema);
        let password = cx.new(|cx| InputState::new(window, cx).masked(true));
        let mut subscriptions = vec![cx.subscribe_in(&store, window, |this, _, e: &StoreEvent, window, cx| {
            if matches!(e, StoreEvent::Changed | StoreEvent::State(_)) {
                this.adopt(window, cx);
            }
        })];
        for input in [&host, &port, &database, &user, &password, &schema] {
            subscriptions.push(cx.subscribe(input, |this, _, e: &InputEvent, cx| {
                if matches!(e, InputEvent::Change) {
                    this.read_inputs(cx);
                }
            }));
        }
        let mut this = Self {
            shell,
            draft: current.clone(),
            synced: current,
            host,
            port,
            database,
            user,
            password,
            schema,
            report: None,
            status: String::new(),
            error: String::new(),
            busy: false,
            capabilities: None,
            expanded: false,
            _subscriptions: subscriptions,
        };
        this.load_capabilities(cx);
        this
    }

    fn read_inputs(&mut self, cx: &mut Context<Self>) {
        self.draft.host = input_text(&self.host, cx);
        self.draft.port = input_text(&self.port, cx).trim().parse().unwrap_or(0);
        self.draft.database = input_text(&self.database, cx);
        self.draft.user = input_text(&self.user, cx);
        self.draft.schema = input_text(&self.schema, cx);
        cx.notify();
    }

    // Adopts the saved datasource when it changed outside this tab.
    fn adopt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.shell.read(cx).config(cx).datasource.clone();
        if current == self.synced {
            return;
        }
        let kind_changed = current.kind != self.synced.kind;
        self.synced = current.clone();
        self.draft = current.clone();
        set_input(&self.host, &current.host, window, cx);
        set_input(&self.port, &current.port.to_string(), window, cx);
        set_input(&self.database, &current.database, window, cx);
        set_input(&self.user, &current.user, window, cx);
        set_input(&self.schema, &current.schema, window, cx);
        if kind_changed {
            self.load_capabilities(cx);
        }
        cx.notify();
    }

    fn load_capabilities(&mut self, cx: &mut Context<Self>) {
        let label = self.shell.read(cx).label().to_string();
        task::spawn_call(
            cx,
            move || store_capabilities(label),
            |this, result, cx| {
                this.capabilities = result.ok();
                cx.notify();
            },
        );
    }

    pub fn is_postgres(&self) -> bool {
        self.draft.is_postgres()
    }

    fn plaintext(&self) -> bool {
        self.draft.is_postgres() && self.draft.allows_plaintext()
    }

    /// Whether a non-TLS connection still needs the explicit override.
    pub fn blocked(&self) -> bool {
        self.plaintext() && !self.draft.insecure_transport_confirmed
    }

    fn set_kind(&mut self, kind: &str, cx: &mut Context<Self>) {
        self.draft.kind = kind.to_string();
        cx.notify();
    }

    fn test(&mut self, cx: &mut Context<Self>) {
        self.busy = true;
        self.error.clear();
        self.report = None;
        cx.notify();
        let label = self.shell.read(cx).label().to_string();
        let draft = self.draft.clone();
        let password = input_text(&self.password, cx);
        task::spawn_call(
            cx,
            move || test_datasource_connection(label, draft, Some(password)),
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(report) => this.report = Some(report),
                    Err(e) => this.error = e.message,
                }
                cx.notify();
            },
        );
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.busy = true;
        self.error.clear();
        self.status.clear();
        cx.notify();
        let label = self.shell.read(cx).label().to_string();
        // The id is settled once, so the stored password and the saved datasource name the same one.
        if self.draft.id.is_empty() {
            self.draft.id = uuid::Uuid::new_v4().to_string();
        }
        if self.draft.port == 0 {
            self.draft.port = 5432;
        }
        let draft = self.draft.clone();
        let password = input_text(&self.password, cx);
        task::spawn_call_in(
            window,
            cx,
            move || {
                if password.is_empty() {
                    return Ok(draft.password_ref.clone());
                }
                set_datasource_password(label, draft.id.clone(), password, Some(draft.clone())).map(Some)
            },
            |this, result: Result<Option<String>, AppError>, window, cx| match result {
                Ok(password_ref) => this.persist(password_ref, window, cx),
                Err(e) => this.fail(e, cx),
            },
        );
    }

    fn fail(&mut self, e: AppError, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = e.message;
        cx.notify();
    }

    // Writes the datasource into the document, then attaches the reader to it.
    fn persist(&mut self, password_ref: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        let plaintext = self.plaintext();
        let mut next = self.draft.clone();
        next.password_ref = password_ref;
        if plaintext {
            if next.insecure_transport_confirmed_at.is_none() {
                next.insecure_transport_confirmed_at = Some(chrono::Utc::now().to_rfc3339());
            }
        } else {
            next.insecure_transport_confirmed = false;
            next.insecure_transport_confirmed_at = None;
        }
        let store = self.shell.read(cx).store.clone();
        let saved = next.clone();
        let result = store.update(cx, |s, cx| s.update("Change datasource", move |c| c.datasource = saved, cx));
        if let Err(e) = result {
            return self.fail(e, cx);
        }
        self.synced = next.clone();
        self.draft = next;
        set_input(&self.password, "", window, cx);
        let label = self.shell.read(cx).label().to_string();
        task::spawn_call(
            cx,
            move || connect_datasource(label),
            |this, result, cx| {
                this.busy = false;
                match result {
                    Ok(attached) => {
                        this.status = if attached.attached {
                            format!(
                                "Saved. Reads now use the {} store.",
                                if attached.kind == "postgres" { "PostgreSQL" } else { "embedded SQLite" }
                            )
                        } else {
                            format!(
                                "Saved, but the store is not reachable: {}",
                                attached.error.map_or("unknown error".to_string(), |e| e)
                            )
                        };
                        this.shell.update(cx, |s, cx| s.reload_metadata(cx));
                        this.load_capabilities(cx);
                    }
                    Err(e) => this.error = e.message,
                }
                cx.notify();
            },
        );
    }

    fn forget_password(&mut self, cx: &mut Context<Self>) {
        let Some(reference) = self.shell.read(cx).config(cx).datasource.password_ref.clone() else {
            return;
        };
        let label = self.shell.read(cx).label().to_string();
        match clear_datasource_password(label, reference) {
            Ok(()) => {
                let store = self.shell.read(cx).store.clone();
                let result = store.update(
                    cx,
                    |s, cx| s.update("Forget datasource password", |c| c.datasource.password_ref = None, cx),
                );
                match result {
                    Ok(_) => {
                        self.draft.password_ref = None;
                        self.status = "The stored password was removed from this computer.".into();
                    }
                    Err(e) => self.error = e.message,
                }
            }
            Err(e) => self.error = e.message,
        }
        cx.notify();
    }

    fn labelled(label: &str, control: impl IntoElement) -> Div {
        h_flex()
            .gap_3()
            .items_center()
            .child(div().w(px(160.)).child(label.to_string()))
            .child(div().w(px(360.)).child(control))
    }
}

impl Render for DatasourceTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let postgres = self.draft.is_postgres();
        let plaintext = self.plaintext();
        let blocked = self.blocked();
        let busy = self.busy;
        let has_ref = self.shell.read(cx).config(cx).datasource.password_ref.is_some();
        let danger = cx.theme().danger;
        let border = cx.theme().border;
        let mut panel = v_flex()
            .gap_3()
            .child(widgets::section_heading("tab-title", "Data source"))
            .child(note(
                "Records are written to this store and read through DuckDB. PostgreSQL hosting, backups, and availability are yours to provide.",
                cx,
            ));
        let kind_entity = entity.clone();
        panel = panel.child(Self::labelled(
            "Record store",
            select(
                "Record store",
                &self.draft.kind,
                vec![
                    ("sqlite".into(), "Embedded SQLite (in the .ixt file)".into()),
                    ("postgres".into(), "PostgreSQL".into()),
                ],
                "Record store",
                move |value, _, cx| kind_entity.update(cx, |this, cx| this.set_kind(value, cx)),
            ),
        ));
        if postgres {
            let ssl = entity.clone();
            let mode = entity.clone();
            panel = panel
                .child(Self::labelled("Host", text_input(&self.host, "Host", "Host")))
                .child(Self::labelled("Port", text_input(&self.port, "Port", "Port")))
                .child(Self::labelled("Database", text_input(&self.database, "Database", "Database")))
                .child(Self::labelled("User", text_input(&self.user, "User", "User")))
                .child(Self::labelled(
                    "Password",
                    text_input(&self.password, "Password", "Password"),
                ))
                .when(has_ref, |el| el.child(note("A password is stored on this computer.", cx)))
                .child(Self::labelled("Schema", text_input(&self.schema, "Schema", "Schema")))
                .child(Self::labelled(
                    "TLS (sslmode)",
                    select(
                        "TLS (sslmode)",
                        &self.draft.sslmode,
                        SSL_MODES.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect(),
                        "TLS (sslmode)",
                        move |value, _, cx| {
                            ssl.update(cx, |this, cx| {
                                this.draft.sslmode = value.to_string();
                                this.draft.insecure_transport_confirmed = false;
                                cx.notify();
                            })
                        },
                    ),
                ))
                .child(Self::labelled(
                    "Credential mode",
                    select(
                        "Credential mode",
                        &self.draft.credential_mode,
                        vec![
                            ("shared".into(), "One shared application credential".into()),
                            ("perUser".into(), "A least-privileged credential per runtime user".into()),
                        ],
                        "Credential mode",
                        move |value, _, cx| {
                            mode.update(cx, |this, cx| {
                                this.draft.credential_mode = value.to_string();
                                cx.notify();
                            })
                        },
                    ),
                ))
                .child(note(
                    "The password is sealed in this computer's local secret store; the application file keeps only a reference to it.",
                    cx,
                ));
            if self.draft.credential_mode == "shared" {
                panel = panel.child(note(
                    widgets::text(
                        "shared-credentials-note",
                        "Shared credentials reduce revocation and database-level attribution: every runtime user connects as the same database role.",
                    ),
                    cx,
                ));
            }
        }
        if plaintext {
            let confirmed = self.draft.insecure_transport_confirmed;
            let accept = entity.clone();
            panel = panel.child(
                v_flex()
                    .gap_2()
                    .child(widgets::callout(
                        "plaintext-warning",
                        widgets::Severity::Danger,
                        "Severe security warning",
                        Some(&format!(
                            "With sslmode \u{201c}{}\u{201d}, credentials and records can travel over the network unencrypted. The override is recorded in the application and shown before publishing.",
                            self.draft.sslmode
                        )),
                        cx,
                    ))
                    .child(check(
                        "accept-plaintext",
                        "I accept connecting without TLS for this datasource",
                        confirmed,
                        move |value, _, cx| {
                            accept.update(cx, |this, cx| {
                                this.draft.insecure_transport_confirmed = value;
                                cx.notify();
                            })
                        },
                    )),
            );
        }
        panel = panel.child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("test-connection")
                        .outline()
                        .label("Test connection")
                        .disabled(busy || blocked)
                        .on_click(cx.listener(|this, _, _, cx| this.test(cx))),
                )
                .when(postgres && has_ref, |el| {
                    el.child(
                        Button::new("forget-password")
                            .outline()
                            .label("Forget stored password")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| this.forget_password(cx))),
                    )
                })
                .child(
                    Button::new("save-datasource")
                        .primary()
                        .label("Save data source")
                        .disabled(busy || blocked)
                        .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
                ),
        );
        if let Some(report) = &self.report {
            let text = format!(
                "{}{}: {}",
                if report.ok { "Connection succeeded" } else { "Connection failed" },
                report.server_version.as_ref().map_or(String::new(), |v| format!(" (server {v})")),
                report.message
            );
            panel = panel.child(div().when(!report.ok, |d| d.text_color(danger)).child(widgets::text("connection-report", text)));
        }
        if !self.status.is_empty() {
            panel = panel.child(widgets::text("datasource-status", self.status.clone()));
        }
        if !self.error.is_empty() {
            panel = panel.child(error_line("datasource-error", &self.error, cx));
        }
        if let Some(capabilities) = &self.capabilities {
            let title = format!("{} store capabilities", store_label(Some(capabilities)).unwrap_or_default());
            let expanded = self.expanded;
            panel = panel.child(
                v_flex()
                    .gap_2()
                    .pt_2()
                    .border_t_1()
                    .border_color(border)
                    .child(
                        Button::new("capabilities-toggle")
                            .ghost()
                            .start_label(
                                Some(if expanded { IconName::ChevronDown } else { IconName::ChevronRight }.into()),
                                title,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.expanded = !this.expanded;
                                cx.notify();
                            })),
                    )
                    .when(expanded, |el| {
                        el.child(capability_summary(capabilities, cx)).child(mapping_tables(capabilities, cx))
                    }),
            );
        }
        panel
    }
}
