//! Publish checkpoint (PRD 22.2): preflight (size, validation, concurrency, TLS,
//! credentials), then save, upload and `publish-checkpoint`. Autosave never publishes;
//! only the button does. Port of src/cloud/PublishPanel.tsx.
//!
//! Test ids. Card `cloud-publish` ("Publish checkpoint"); `publish-checking`; the
//! preflight: `preflight-size`, `preflight-largest:<i>`, `preflight-blockers` (heading),
//! `preflight-blocker:<i>`, `preflight-warning:<i>`, `security-store`,
//! `security-credentials`, `security-tls`, `security-policies`, `security-migrations`.
//! Form: inputs `publish-version`, `publish-min-runtime`, `publish-notes`, errors
//! `publish-version-error` / `publish-min-error`, checkboxes `publish-ack-shared` and
//! `publish-ack-insecure`, buttons `publish-submit` and `publish-recheck`, transfer bar
//! `publish-progress`. Conflict dialog `publish-conflict` ("The published history moved
//! on") with `conflict-overwrite`, `conflict-fork`, `conflict-cancel`. Outcome:
//! `publish-notice` / `publish-error`.

use super::{
    action::{Action, action_status},
    contract,
    errors::CloudError,
    services::{CloudServices, Services},
    studio_panels::set_link,
    transfer::{Transfer, format_bytes, transfer_bar},
    types::{CloudApp, PublishedVersion},
};
use crate::{
    archive::ReleaseInfo,
    cloud::{CloudLink, publish::Preflight},
    manager::SessionState,
    ui::{dialog::dialog, forms, forms::eid, prelude::*, release::gate, shell::Shell},
};
use gpui_kit::component::input::{InputState, TextareaState};
use serde_json::{Value, json};
use std::sync::Arc;

pub enum PublishEvent {
    /// A version was published (or the document forked): lists should reload.
    Published,
}

/// A publish refused because the published history moved on.
struct Conflict {
    body: Value,
    head_version_id: Option<String>,
}

/// What the background half of a publish learned.
struct Staged {
    /// The session state after saving, to hand to the shell.
    state: Option<SessionState>,
    outcome: Result<Outcome, CloudError>,
}

enum Outcome {
    Published(PublishedVersion),
    Conflict(Box<Conflict>, CloudError),
}

/// Saves the document (publishing and backing up both need the file on disk). The Save
/// As dialog of a never-saved document is not offered here: the user saves first.
pub fn save_for_upload(label: &str, what: &str) -> Result<SessionState, CloudError> {
    let mut state = crate::document_state(label.to_string())?;
    if state.path.is_none() {
        return Err(CloudError::new("SAVE_REQUIRED", format!("Save the document before {what}.")));
    }
    if state.dirty {
        state = crate::save_document(label.to_string())?;
    }
    if state.path.is_none() || state.dirty {
        return Err(CloudError::new("SAVE_REQUIRED", format!("Save the document before {what}.")));
    }
    Ok(state)
}

fn semver_error(text: &str, blank_ok: bool) -> bool {
    let text = text.trim();
    !(blank_ok && text.is_empty()) && semver::Version::parse(text).is_err()
}

pub struct PublishPanel {
    shell: Entity<Shell>,
    services: Arc<dyn CloudServices>,
    app: Option<CloudApp>,
    preflight: Option<Preflight>,
    checking: bool,
    version: Entity<InputState>,
    min_runtime: Entity<InputState>,
    notes: Entity<TextareaState>,
    ack_shared: bool,
    ack_insecure: bool,
    conflict: Option<Conflict>,
    action: Action,
    transfer: Transfer,
    focus: FocusHandle,
    _watch: Subscription,
}

impl EventEmitter<PublishEvent> for PublishPanel {}

impl PublishPanel {
    pub fn new(
        shell: Entity<Shell>,
        app: Option<CloudApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let release = shell.read(cx).config(cx).release.clone();
        let version = if release.version.is_empty() { "1.0.0".to_string() } else { release.version };
        let watch = cx.observe(&shell.read(cx).store.clone(), |_, _, cx| cx.notify());
        let mut this = Self {
            shell,
            services: Services::get(cx),
            app,
            preflight: None,
            checking: false,
            version: forms::line_state(window, cx, "Version", &version),
            min_runtime: forms::line_state(
                window,
                cx,
                "optional",
                release.min_runtime_version.as_deref().unwrap_or(""),
            ),
            notes: forms::area_state(window, cx, &release.notes),
            ack_shared: false,
            ack_insecure: false,
            conflict: None,
            action: Action::default(),
            transfer: Transfer::default(),
            focus: cx.focus_handle(),
            _watch: watch,
        };
        this.check(cx);
        this
    }

    pub fn set_app(&mut self, app: Option<CloudApp>, cx: &mut Context<Self>) {
        if self.app != app {
            self.app = app;
            cx.notify();
        }
    }

    fn link(&self, cx: &App) -> Option<CloudLink> {
        self.shell.read(cx).config(cx).cloud.clone()
    }

    /// Reads the preflight again (size, blockers, security).
    pub fn check(&mut self, cx: &mut Context<Self>) {
        let Some(link) = self.link(cx) else { return };
        let label = self.shell.read(cx).label().to_string();
        let services = self.services.clone();
        self.checking = true;
        task::spawn_call(
            cx,
            move || {
                let users = services.runtime_capacity(&link.app_id).unwrap_or(0);
                services.publish_preflight(&label, users)
            },
            |this, result, cx| {
                this.checking = false;
                this.preflight = result.ok();
                cx.notify();
            },
        );
        cx.notify();
    }

    /// Expression errors block publishing: Rust cannot check expressions.
    fn expression_errors(&self, cx: &App) -> usize {
        gate::expression_error_count(self.shell.read(cx).config(cx))
    }

    fn blockers(&self, cx: &App) -> Vec<String> {
        let mut all = Vec::new();
        let errors = self.expression_errors(cx);
        if errors > 0 {
            all.push(gate::expression_blocker(errors));
        }
        if let Some(p) = &self.preflight {
            all.extend(p.blockers.iter().cloned());
        }
        all
    }

    fn version_error(&self, cx: &App) -> bool {
        semver_error(&forms::text_of(&self.version, cx), false)
    }

    fn min_error(&self, cx: &App) -> bool {
        semver_error(&forms::text_of(&self.min_runtime, cx), true)
    }

    fn needs_shared(&self) -> bool {
        self.preflight.as_ref().is_some_and(|p| p.security.shared_credential_warning)
    }

    fn needs_insecure(&self) -> bool {
        self.preflight.as_ref().is_some_and(|p| p.security.insecure_override_confirmed)
    }

    fn blocked(&self, cx: &App) -> bool {
        self.preflight.is_none()
            || !self.blockers(cx).is_empty()
            || self.version_error(cx)
            || self.min_error(cx)
            || (self.needs_shared() && !self.ack_shared)
            || (self.needs_insecure() && !self.ack_insecure)
    }

    fn security_body(&self) -> Value {
        let Some(p) = &self.preflight else { return Value::Null };
        let mut body = serde_json::to_value(&p.security).unwrap_or(Value::Null);
        if let Some(map) = body.as_object_mut() {
            map.insert("insecureTransportConfirmed".into(), json!(self.needs_insecure() && self.ack_insecure));
            map.insert(
                "insecureTransportConfirmedAt".into(),
                json!(p.security.insecure_override_confirmed_at),
            );
            map.insert("sharedCredentialAcknowledged".into(), json!(self.needs_shared() && self.ack_shared));
            map.insert("concurrencyPoliciesResolved".into(), json!(p.security.entity_policies_resolved));
            map.insert("unresolvedEntities".into(), json!(p.unresolved_entities));
        }
        body
    }

    fn publish(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(cx) {
            return;
        }
        let Some(link) = self.link(cx) else { return };
        let release = ReleaseInfo {
            version: forms::text_of(&self.version, cx).trim().to_string(),
            notes: forms::area_text(&self.notes, cx),
            min_runtime_version: Some(forms::text_of(&self.min_runtime, cx).trim().to_string())
                .filter(|v| !v.is_empty()),
        };
        self.action.begin();
        let store = self.shell.read(cx).store.clone();
        let edit = release.clone();
        let edited = store.update(cx, |s, cx| s.update("Edit release", move |c| c.release = edit, cx));
        if let Err(e) = edited {
            self.action.finish(Err(e.into()));
            cx.notify();
            return;
        }
        // Rust cannot check expressions, so a fresh check gates the upload.
        let errors = self.expression_errors(cx);
        if errors > 0 {
            self.action.finish(Err(CloudError::new("EXPRESSION_ERRORS", gate::expression_blocker(errors))));
            cx.notify();
            return;
        }
        let label = self.shell.read(cx).label().to_string();
        let services = self.services.clone();
        let transfer_id = self.transfer.begin(services.clone(), |v: &mut Self| &mut v.transfer, cx);
        let migrations = self.preflight.as_ref().map(|p| json!(p.migrations)).unwrap_or(json!([]));
        let security = self.security_body();
        let app_id = link.app_id.clone();
        let expected = link.head_version_id.clone();
        let fallback_head = self.app.as_ref().and_then(|a| a.head_version_id.clone());
        task::spawn_call_in(
            window,
            cx,
            move || -> Staged {
                let state = match save_for_upload(&label, "publishing") {
                    Ok(state) => state,
                    Err(e) => return Staged { state: None, outcome: Err(e) },
                };
                let staged = |outcome| Staged { state: Some(state.clone()), outcome };
                let upload = match services.require_session().and_then(|session| {
                    services.upload_archive(&label, &session.access_token, &app_id, "version", &transfer_id)
                }) {
                    Ok(upload) => upload,
                    Err(e) => return staged(Err(e)),
                };
                let body = json!({
                    "appId": app_id,
                    "uploadId": upload.upload_id,
                    "version": release.version,
                    "releaseNotes": release.notes,
                    "minRuntimeVersion": release.min_runtime_version.clone().unwrap_or_else(|| "0.0.0".into()),
                    "migrations": migrations,
                    "security": security,
                    "expectedHeadVersionId": expected,
                });
                let published = services
                    .invoke("publish-checkpoint", body.clone())
                    .and_then(|reply| contract::publish_checkpoint(&reply));
                staged(match published {
                    Ok(version) => Ok(Outcome::Published(version)),
                    Err(e) if e.code == "VERSION_CONFLICT" => {
                        let head = e.detail("headVersionId").map(str::to_string).or(fallback_head);
                        Ok(Outcome::Conflict(Box::new(Conflict { body, head_version_id: head }), e))
                    }
                    Err(e) => Err(e),
                })
            },
            |this, staged, window, cx| {
                this.transfer.end();
                if let Some(state) = staged.state {
                    this.shell.update(cx, |s, cx| s.apply_session(&state, cx));
                }
                match staged.outcome {
                    Ok(Outcome::Published(version)) => this.finish(version, window, cx),
                    Ok(Outcome::Conflict(conflict, error)) => {
                        this.conflict = Some(*conflict);
                        window.focus(&this.focus, cx);
                        this.action.finish(Err(error));
                        cx.notify();
                    }
                    Err(e) => {
                        this.action.finish(Err(e));
                        cx.notify();
                    }
                }
            },
        );
        cx.notify();
    }

    /// Records the published version as the document's base, saves it (so the next
    /// publish detects divergence) and reloads the lists.
    fn finish(&mut self, version: PublishedVersion, window: &mut Window, cx: &mut Context<Self>) {
        let id = version.id.clone();
        let store = self.shell.read(cx).store.clone();
        let recorded = store.update(cx, |s, cx| {
            s.update(
                "Record published version",
                move |c| {
                    if let Some(link) = c.cloud.as_mut() {
                        link.head_version_id = Some(id);
                    }
                },
                cx,
            )
        });
        if let Err(e) = recorded {
            self.action.finish(Err(e.into()));
            cx.notify();
            return;
        }
        let notice = format!(
            "Published version {}. Runtime users receive it on their next sync.",
            version.version
        );
        self.save_and_report(notice, window, cx);
    }

    fn save_and_report(&mut self, notice: String, window: &mut Window, cx: &mut Context<Self>) {
        let label = self.shell.read(cx).label().to_string();
        task::spawn_call_in(
            window,
            cx,
            move || crate::save_document(label),
            move |this, saved, _, cx| {
                match saved {
                    Ok(state) => {
                        this.shell.update(cx, |s, cx| s.apply_session(&state, cx));
                        this.action.finish(Ok(notice));
                    }
                    Err(e) => this.action.finish(Err(e.into())),
                }
                cx.emit(PublishEvent::Published);
                this.check(cx);
                cx.notify();
            },
        );
    }

    fn resolve(&mut self, action: &'static str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(conflict) = self.conflict.as_ref() else { return };
        let Some(link) = self.link(cx) else { return };
        let mut body = conflict.body.clone();
        if let Some(map) = body.as_object_mut() {
            map.insert("action".into(), json!(action));
            map.insert("fromVersionId".into(), json!(conflict.head_version_id));
        }
        self.action.begin();
        let services = self.services.clone();
        task::spawn_call_in(
            window,
            cx,
            move || {
                services
                    .invoke("versions-resolve", body)
                    .and_then(|reply| contract::versions_resolve(&reply))
            },
            move |this, result, window, cx| match result {
                Err(e) => {
                    this.action.finish(Err(e));
                    cx.notify();
                }
                Ok((version, app)) => {
                    this.conflict = None;
                    if let (true, Some(app)) = (action == "fork", app) {
                        let forked = CloudLink {
                            app_id: app.id,
                            org_id: link.org_id.clone(),
                            head_version_id: version.map(|v| v.id),
                        };
                        let name = if app.name.is_empty() { String::new() } else { format!(" ({})", app.name) };
                        match set_link(&this.shell, "Link forked cloud application", forked, cx) {
                            Ok(()) => this.save_and_report(
                                format!("Forked into a new cloud application{name}."),
                                window,
                                cx,
                            ),
                            Err(e) => {
                                this.action.finish(Err(e));
                                cx.notify();
                            }
                        }
                    } else if let Some(version) = version {
                        this.finish(version, window, cx);
                    } else {
                        this.action.finish(Ok("Resolved.".into()));
                        cx.notify();
                    }
                }
            },
        );
        cx.notify();
    }

    fn preflight_summary(&self, preflight: &Preflight, blockers: &[String], cx: &App) -> Div {
        let (size, security) = (&preflight.size, &preflight.security);
        let mut summary = v_flex().gap_1().child(widgets::text(
            "preflight-size",
            format!(
                "Archive size {} of {}{}",
                format_bytes(size.total_bytes),
                format_bytes(size.cloud_limit_bytes),
                if size.measured == "snapshot" { " (unsaved changes included)" } else { "" }
            ),
        ));
        summary = summary.child(super::largest::largest_files("preflight-largest", &size.largest[..size.largest.len().min(5)], size.total_bytes, cx));
        if !blockers.is_empty() {
            summary = summary.child(widgets::styled_text("preflight-blockers", "Publishing is blocked", {
                let danger = cx.theme().danger;
                move |d| d.font_weight(FontWeight::SEMIBOLD).text_color(danger)
            }));
            for (i, blocker) in blockers.iter().enumerate() {
                summary = summary.child(widgets::callout(
                    eid(format!("preflight-blocker:{i}")),
                    widgets::Severity::Danger,
                    blocker,
                    None,
                    cx,
                ));
            }
        }
        for (i, warning) in preflight.warnings.iter().enumerate() {
            summary = summary.child(widgets::callout(
                eid(format!("preflight-warning:{i}")),
                widgets::Severity::Warning,
                warning,
                None,
                cx,
            ));
        }
        let store = if security.store == "postgres" { "PostgreSQL" } else { "Embedded SQLite" };
        summary = summary.child(widgets::text("security-store", format!("Record store: {store}")));
        if security.store == "postgres" {
            let sslmode = security.sslmode.clone().unwrap_or_default();
            summary = summary
                .child(widgets::text(
                    "security-credentials",
                    format!(
                        "Credentials: {}",
                        if security.credential_mode.as_deref() == Some("perUser") { "Per user" } else { "Shared" }
                    ),
                ))
                .child(widgets::text(
                    "security-tls",
                    if security.tls {
                        format!("TLS: Required ({sslmode})")
                    } else {
                        format!(
                            "TLS: Not required ({sslmode}){}",
                            if security.insecure_override_confirmed { ", override confirmed" } else { "" }
                        )
                    },
                ));
        }
        summary
            .child(widgets::text(
                "security-policies",
                format!(
                    "Concurrency policies: {}",
                    if security.entity_policies_resolved {
                        "Set for every entity"
                    } else {
                        "Missing for some entities"
                    }
                ),
            ))
            .child(widgets::text(
                "security-migrations",
                format!("Migrations: {}", preflight.migrations.len()),
            ))
    }

    fn conflict_dialog(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.action.busy;
        let entity = cx.entity();
        dialog("publish-conflict", "The published history moved on")
            .child(
                "A checkpoint was published after the version this document is based on. \
                 Overwrite publishes this document as the newest version anyway. Fork creates \
                 a separate cloud application from this document. Nothing is merged automatically.",
            )
            .footer(
                forms::secondary("conflict-cancel", "Cancel").disabled(busy).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.conflict = None;
                        cx.notify();
                    },
                )),
            )
            .footer(
                forms::secondary("conflict-fork", "Fork")
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, window, cx| this.resolve("fork", window, cx))),
            )
            .footer(
                forms::danger("conflict-overwrite", "Overwrite")
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, window, cx| this.resolve("overwrite", window, cx))),
            )
            .on_escape(&self.focus, busy, move |_, cx| {
                entity.update(cx, |this, cx| {
                    this.conflict = None;
                    cx.notify();
                })
            })
            .modal()
    }
}

impl Render for PublishPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.link(cx).is_none() {
            return div().into_any_element();
        }
        let busy = self.action.busy;
        let blockers = self.blockers(cx);
        let blocked = self.blocked(cx);
        let mut card = forms::card("cloud-publish", "Publish checkpoint", cx).child(forms::muted(
            "Publishing uploads the saved archive as an immutable version. Autosave never publishes.",
            cx,
        ));
        card = match self.preflight.clone() {
            Some(preflight) => card.child(self.preflight_summary(&preflight, &blockers, cx)),
            None => card.child(widgets::text("publish-checking", "Checking the application…")),
        };
        card = card.child(forms::input_field("publish-version", "Version", &self.version, cx));
        if self.version_error(cx) {
            card = card.child(widgets::text("publish-version-error", "Enter a semantic version such as 1.2.0."));
        }
        card = card.child(forms::input_field("publish-min-runtime", "Minimum Runtime version", &self.min_runtime, cx));
        if self.min_error(cx) {
            card = card.child(widgets::text("publish-min-error", "Enter a semantic version or leave blank."));
        }
        card = card.child(forms::area_field("publish-notes", "Release notes", &self.notes, px(80.), cx));
        let entity = cx.entity();
        if self.needs_shared() {
            let entity = entity.clone();
            card = card.child(forms::check(
                "publish-ack-shared",
                "I understand every runtime user receives the same database credential",
                self.ack_shared,
                move |on, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.ack_shared = on;
                        cx.notify();
                    })
                },
            ));
        }
        if self.needs_insecure() {
            let entity = entity.clone();
            card = card.child(forms::check(
                "publish-ack-insecure",
                "I accept publishing with a PostgreSQL connection that may not use TLS",
                self.ack_insecure,
                move |on, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.ack_insecure = on;
                        cx.notify();
                    })
                },
            ));
        }
        card = card.child(
            forms::actions()
                .child(
                    forms::primary("publish-submit", "Publish checkpoint")
                        .disabled(busy || blocked)
                        .on_click(cx.listener(|this, _, window, cx| this.publish(window, cx))),
                )
                .child(forms::secondary("publish-recheck", "Check again").disabled(busy).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.action.error = None;
                        this.check(cx);
                    }),
                )),
        );
        if let Some(bar) = transfer_bar("publish-progress", "Upload progress", self.transfer.progress.as_ref(), cx) {
            card = card.child(bar);
        }
        if self.conflict.is_some() {
            card = card.child(self.conflict_dialog(cx));
        }
        card.child(action_status("publish", &self.action, cx)).into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::semver_error;

    #[test]
    fn versions_must_be_semantic() {
        assert!(!semver_error("1.2.0", false));
        assert!(!semver_error(" 2.0.0-beta.1+build.5 ", false));
        assert!(semver_error("1.2", false));
        assert!(semver_error("", false));
        assert!(semver_error("01.2.0", false));
        assert!(!semver_error("", true));
        assert!(semver_error("x", true));
    }
}
