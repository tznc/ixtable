//! Release notes and the migrations an update will run, and the confirm step before a
//! manual update or downgrade is applied (PRD 22.3 step 7).
//! Port of src/release/UpdateConfirm.tsx.
//!
//! Test ids: dialog title `update-confirm` ("Update NAME to V?" or "Install older
//! version V of NAME?"), `update-installed`, `release-notes-text`, `migrations-note`,
//! `no-migrations`, `migration:<i>` (name), `migration-sql:<i>`, `apply-update`,
//! `cancel-update`.

use crate::{
    installation_checks::PendingMigration,
    ui::{forms, forms::eid, prelude::*},
};

/// Release notes and migrations, as a block of children.
pub fn update_details(
    release_notes: &str,
    migrations: Option<&[PendingMigration]>,
    unavailable: Option<&str>,
    cx: &App,
) -> Div {
    let notes = release_notes.trim();
    let mut block = v_flex()
        .gap_2()
        .child(div().font_weight(FontWeight::MEDIUM).child("Release notes"))
        .child(widgets::text(
            "release-notes-text",
            if notes.is_empty() { "No release notes." } else { notes },
        ))
        .child(div().font_weight(FontWeight::MEDIUM).child("Migrations"));
    match migrations {
        None => {
            let why = unavailable.map(|u| format!(" ({u})")).unwrap_or_default();
            block = block.child(widgets::text(
                "migrations-note",
                format!(
                    "The migration preview is unavailable{why}. Pending migrations still run \
                     when you apply."
                ),
            ));
        }
        Some([]) => {
            block = block.child(widgets::text(
                "no-migrations",
                "No migrations will run on your records.",
            ));
        }
        Some(list) => {
            for (i, m) in list.iter().enumerate() {
                let name = if m.name.is_empty() { &m.id } else { &m.name };
                block = block
                    .child(widgets::text(eid(format!("migration:{i}")), name.clone()))
                    .child(widgets::styled_text(
                        eid(format!("migration-sql:{i}")),
                        m.sql.clone(),
                        |d| d.font_family("monospace").text_sm(),
                    ));
            }
        }
    }
    let _ = cx;
    block
}

pub struct UpdateConfirmProps<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub installed_version: Option<&'a str>,
    pub release_notes: &'a str,
    pub migrations: Option<&'a [PendingMigration]>,
    pub migrations_unavailable: Option<&'a str>,
    pub downgrade: bool,
    pub busy: bool,
}

/// The confirm dialog; `apply` and `cancel` are wired by the owner.
pub fn update_confirm(
    props: UpdateConfirmProps<'_>,
    apply: impl Fn(&mut Window, &mut App) + 'static,
    cancel: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> Stateful<Div> {
    let title = if props.downgrade {
        format!("Install older version {} of {}?", props.version, props.name)
    } else {
        format!("Update {} to {}?", props.name, props.version)
    };
    let apply_label = if props.downgrade {
        format!("Install older version {} anyway", props.version)
    } else {
        "Apply update".to_string()
    };
    let mut panel = forms::dialog("update-confirm", &title, cx);
    if let Some(installed) = props.installed_version {
        panel = panel.child(widgets::text(
            "update-installed",
            format!("Installed version: {installed}. Your records are kept."),
        ));
    }
    panel
        .child(update_details(
            props.release_notes,
            props.migrations,
            props.migrations_unavailable,
            cx,
        ))
        .child(
            forms::actions()
                .child(
                    forms::primary("apply-update", &apply_label)
                        .disabled(props.busy)
                        .on_click(move |_, window, cx| apply(window, cx)),
                )
                .child(
                    forms::secondary("cancel-update", "Cancel")
                        .on_click(move |_, window, cx| cancel(window, cx)),
                ),
        )
}
