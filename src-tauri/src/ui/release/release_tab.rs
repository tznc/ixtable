//! Settings > Release: version, notes, optional password, and runtime-only bundle
//! export. Port of src/release/ReleaseTab.tsx.
//!
//! Test ids: inputs `release-version`, `release-min-runtime`, `release-notes-input`,
//! `release-password`, `release-confirm`; checkbox `release-protect`; hints
//! `release-version-hint`, `release-min-hint`, `release-password-hint`;
//! `export-bundle`, `release-exporting`, `release-error`; result
//! `exported-bundle` (heading "Exported NAME VERSION"), key-value values `bundle-file`,
//! `bundle-size`, `bundle-sha256`, `bundle-fingerprint`, `bundle-encrypted` ("Yes" /
//! "No"), each with a `<id>-copy` button for the file, hash and key.

use super::{
    bundle_error::bundle_error_banner,
    bundle_flow::IXTR,
    gate::{expression_blocker, expression_error_count},
};
use crate::{
    archive::ReleaseInfo,
    bundle_export::BundleInfo,
    manager::AppError,
    ui::{
        dialogs,
        display::{key_value, kv},
        forms,
        prelude::*,
        shell::Shell,
    },
};
use gpui_kit::component::input::{InputState, TextareaState};
use serde_json::json;

pub fn is_semver(value: &str) -> bool {
    semver::Version::parse(value.trim()).is_ok()
}

/// Binary size with one decimal above 1 KB.
pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1_048_576 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    }
}

pub struct ReleaseTab {
    shell: Entity<Shell>,
    version: Entity<InputState>,
    min_runtime: Entity<InputState>,
    notes: Entity<TextareaState>,
    protect: bool,
    password: Entity<InputState>,
    confirm: Entity<InputState>,
    busy: bool,
    error: Option<AppError>,
    info: Option<BundleInfo>,
    clear_secrets: bool,
    _subscriptions: Vec<Subscription>,
}

impl ReleaseTab {
    pub fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let release = shell.read(cx).config(cx).release.clone();
        let version = if release.version.is_empty() { "1.0.0".to_string() } else { release.version.clone() };
        let version = forms::line_state(window, cx, "1.0.0", &version);
        let min_runtime = forms::line_state(
            window,
            cx,
            "Any",
            release.min_runtime_version.as_deref().unwrap_or(""),
        );
        let notes = forms::area_state(window, cx, &release.notes);
        let password = forms::secret_state(window, cx, "Password");
        let confirm = forms::secret_state(window, cx, "Confirm password");
        // Typing re-validates the form.
        let subscriptions = [
            cx.observe(&version, |_, _, cx| cx.notify()),
            cx.observe(&min_runtime, |_, _, cx| cx.notify()),
            cx.observe(&password, |_, _, cx| cx.notify()),
            cx.observe(&confirm, |_, _, cx| cx.notify()),
        ]
        .into();
        Self {
            shell,
            version,
            min_runtime,
            notes,
            protect: false,
            password,
            confirm,
            busy: false,
            error: None,
            info: None,
            clear_secrets: false,
            _subscriptions: subscriptions,
        }
    }

    fn errors(&self, cx: &App) -> (String, String, String) {
        let version = forms::text_of(&self.version, cx);
        let min = forms::text_of(&self.min_runtime, cx);
        let (password, confirm) = (forms::text_of(&self.password, cx), forms::text_of(&self.confirm, cx));
        let version_error = if is_semver(&version) {
            ""
        } else {
            "Enter a semantic version such as 1.2.0."
        };
        let min_error = if min.trim().is_empty() || is_semver(&min) {
            ""
        } else {
            "Enter a semantic version or leave blank."
        };
        let password_error = if !self.protect {
            ""
        } else if password.chars().count() < 8 {
            "Use at least 8 characters."
        } else if password != confirm {
            "The passwords do not match."
        } else {
            ""
        };
        (version_error.into(), min_error.into(), password_error.into())
    }

    fn fail(&mut self, e: AppError, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = Some(e);
        cx.notify();
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let (v, m, p) = self.errors(cx);
        if !(v.is_empty() && m.is_empty() && p.is_empty()) {
            return;
        }
        self.busy = true;
        self.error = None;
        self.info = None;
        let version = forms::text_of(&self.version, cx).trim().to_string();
        let notes = forms::area_text(&self.notes, cx);
        let min = forms::text_of(&self.min_runtime, cx).trim().to_string();
        let min = (!min.is_empty()).then_some(min);
        let release = ReleaseInfo {
            version: version.clone(),
            notes: notes.clone(),
            min_runtime_version: min.clone(),
        };
        let store = self.shell.read(cx).store.clone();
        let edited = store.update(cx, |s, cx| s.update("Edit release", move |c| c.release = release, cx));
        if let Err(e) = edited {
            return self.fail(e, cx);
        }
        // Expressions run only in the expression engine, so the gate is here.
        let config = store.read(cx).config().clone();
        let count = expression_error_count(&config);
        if count > 0 {
            return self.fail(
                AppError::new("EXPRESSION_ERRORS", expression_blocker(count)),
                cx,
            );
        }
        let password = self.protect.then(|| forms::text_of(&self.password, cx));
        let pick = dialogs::save_file(
            cx,
            "Export runtime bundle",
            &format!("{}-{version}.ixtr", config.name),
            &[IXTR],
        );
        cx.notify();
        task::after(cx, pick, move |this, path, cx| {
            let Some(path) = path else {
                this.busy = false;
                this.shell.update(cx, |s, cx| s.set_notice("Export canceled.", cx));
                return cx.notify();
            };
            let label = this.shell.read(cx).label().to_string();
            let path = path.to_string_lossy().into_owned();
            task::spawn_call(
                cx,
                move || {
                    let options = serde_json::from_value(json!({
                        "version": version,
                        "releaseNotes": notes,
                        "minRuntimeVersion": min,
                        "password": password,
                    }))
                    .map_err(|e| AppError::new("INVALID_VERSION", e))?;
                    let info =
                        crate::bundle_export::export_runtime_bundle(label.clone(), path, options)?;
                    Ok::<_, AppError>((info, crate::document_state(label)?))
                },
                |this, result, cx| {
                    this.busy = false;
                    match result {
                        Ok((info, state)) => {
                            this.info = Some(info);
                            this.shell.update(cx, |s, cx| s.apply_session(&state, cx));
                            this.clear_secrets = true;
                        }
                        Err(e) => this.error = Some(e),
                    }
                    cx.notify();
                },
            );
        });
    }

}

impl Render for ReleaseTab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Forget the typed passwords once a bundle was written.
        if std::mem::take(&mut self.clear_secrets) {
            forms::set_text(&self.password, "", window, cx);
            forms::set_text(&self.confirm, "", window, cx);
        }
        let (version_error, min_error, password_error) = self.errors(cx);
        let invalid = !(version_error.is_empty() && min_error.is_empty() && password_error.is_empty());
        let hint = |id: &'static str, error: &str, fallback: &'static str, cx: &App| {
            let theme = cx.theme();
            let (text, color) = if error.is_empty() {
                (fallback.to_string(), theme.muted_foreground)
            } else {
                (error.to_string(), theme.danger)
            };
            widgets::styled_text(id, text, move |d| d.ty(Type::Caption).text_color(color))
        };
        let mut panel = v_flex()
            .gap_3()
            .child(widgets::section_heading("release-heading", "Release"))
            .child(forms::muted(
                "Export a signed, runtime-only bundle (.ixtr). Recipients open it with ixtable \
                 Runtime without Studio access. Its records seed each installation on first \
                 open; later updates keep the recipient's records and apply your migrations.",
                cx,
            ))
            .child(forms::input_field("release-version", "Version", &self.version, cx))
            .child(hint(
                "release-version-hint",
                &version_error,
                "Semantic version; each update needs a higher version.",
                cx,
            ))
            .child(forms::input_field(
                "release-min-runtime",
                "Minimum Runtime version",
                &self.min_runtime,
                cx,
            ))
            .child(hint(
                "release-min-hint",
                &min_error,
                "Older Runtimes refuse the bundle.",
                cx,
            ))
            .child(forms::area_field(
                "release-notes-input",
                "Release notes",
                &self.notes,
                px(96.),
                cx,
            ));
        let this = cx.entity();
        panel = panel
            .child(div().font_weight(FontWeight::MEDIUM).child("Password protection"))
            .child(
                crate::ui::controls::check("release-protect", "Protect the bundle with a password", self.protect, move |checked, _, cx| {
                    this.update(cx, |t, cx| {
                        t.protect = checked;
                        cx.notify();
                    })
                }),
            )
            .child(forms::muted(
                "A password protects the bundle at rest and against casual unauthorized opening. \
                 It does not stop an authorized recipient from extracting displayed data or \
                 runtime-accessible credentials.",
                cx,
            ));
        if self.protect {
            panel = panel
                .child(forms::input_field("release-password", "Password", &self.password, cx))
                .child(forms::input_field(
                    "release-confirm",
                    "Confirm password",
                    &self.confirm,
                    cx,
                ))
                .child(hint(
                    "release-password-hint",
                    &password_error,
                    "Share the password separately from the file.",
                    cx,
                ));
        }
        panel = panel.child(
            forms::actions()
                .child(
                    forms::primary("export-bundle", "Export runtime bundle…")
                        .disabled(self.busy || invalid)
                        .on_click(cx.listener(|this, _, _, cx| this.export(cx))),
                )
                .when(self.busy, |el| el.child(widgets::text("release-exporting", "Exporting…"))),
        );
        if let Some(e) = &self.error {
            panel = panel.child(bundle_error_banner("release-error", e, cx));
        }
        if let Some(info) = &self.info {
            panel = panel.child(
                forms::card("exported-bundle", &format!("Exported {} {}", info.name, info.version), cx)
                    .child(key_value(
                        "bundle-details",
                        vec![
                            kv("bundle-file", "File", info.path.clone()).code(),
                            kv("bundle-size", "Size", format_size(info.size)),
                            kv("bundle-sha256", "SHA-256", info.sha256.clone()).code(),
                            kv("bundle-fingerprint", "Signing key", info.signer_fingerprint.clone()).code(),
                            kv("bundle-encrypted", "Password protected", if info.encrypted { "Yes" } else { "No" }),
                        ],
                        cx,
                    ))
                    .child(forms::muted(
                        "The signing key is stored on this computer only. It is a local developer \
                         key, not an ixtable Cloud identity; recipients pin it on first open and \
                         refuse updates signed by another key.",
                        cx,
                    )),
            );
        }
        panel
    }
}

#[cfg(test)]
mod tests {
    use super::{format_size, is_semver};

    #[test]
    fn accepts_semantic_versions_only() {
        for ok in ["1.0.0", "0.1.0-beta.1", "2.3.4+build.5", " 1.2.3 "] {
            assert!(is_semver(ok), "{ok}");
        }
        for bad in ["", "1", "1.0", "v1.0.0", "01.0.0", "1.0.0.0"] {
            assert!(!is_semver(bad), "{bad}");
        }
    }

    #[test]
    fn formats_binary_sizes() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(3 * 1_048_576), "3.0 MB");
    }
}
