//! Release notes and the migrations an update will run, and the confirm step before a
//! manual update is applied (PRD 22.3 step 7).
//! Port of src/release/UpdateConfirm.tsx.
//!
//! Test ids: dialog title `update-confirm` ("Update NAME to V?"), `update-installed`, `release-notes-text`, `migrations-note`,
//! `no-migrations`, `migration:<i>` (name), `migration-sql:<i>` (behind the toggle `migration-sql-disclosure:<i>-toggle`), `apply-update`,
//! `cancel-update`.

use crate::{
    ui::cloud::disclosure::disclosure,
    installation_checks::PendingMigration,
    ui::{
        dialog::{Dialog, dialog},
        forms,
        forms::eid,
        prelude::*,
    },
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
        .child(div().font_weight(FontWeight::SEMIBOLD).child("Release notes"))
        .child(widgets::text(
            "release-notes-text",
            if notes.is_empty() { "No release notes." } else { notes },
        ))
        .child(div().font_weight(FontWeight::SEMIBOLD).child("Migrations"));
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
                let (sql, border, muted) = (m.sql.clone(), cx.theme().border, cx.theme().muted);
                let sql_id = format!("migration-sql:{i}");
                block = block
                    .child(widgets::text(eid(format!("migration:{i}")), name.clone()))
                    .child(disclosure(
                        &format!("migration-sql-disclosure:{i}"),
                        "SQL",
                        move || {
                            div()
                                .w_full()
                                .p_2()
                                .rounded_md()
                                .border_1()
                                .border_color(border)
                                .bg(muted)
                                .child(widgets::styled_text(eid(sql_id), sql, |d| d.font_family("monospace").text_sm()))
                                .into_any_element()
                        },
                        cx,
                    ));
            }
        }
    }
    block
}

pub struct UpdateConfirmProps<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub installed_version: Option<&'a str>,
    pub release_notes: &'a str,
    pub migrations: Option<&'a [PendingMigration]>,
    pub migrations_unavailable: Option<&'a str>,
    pub busy: bool,
}

/// The confirm dialog; `apply` and `cancel` are wired by the owner.
pub fn update_confirm(
    props: UpdateConfirmProps<'_>,
    apply: impl Fn(&mut Window, &mut App) + 'static,
    cancel: impl Fn(&mut Window, &mut App) + 'static,
    cx: &App,
) -> Dialog {
    let title = format!("Update {} to {}?", props.name, props.version);
    let mut panel = dialog("update-confirm", &title);
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
        .footer(
            forms::secondary("cancel-update", "Cancel")
                .on_click(move |_, window, cx| cancel(window, cx)),
        )
        .footer(
            forms::primary("apply-update", "Apply update")
                .disabled(props.busy)
                .on_click(move |_, window, cx| apply(window, cx)),
        )
}
