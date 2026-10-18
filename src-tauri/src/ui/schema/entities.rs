//! Settings > Entities (port of `src/schema/EntitiesTab.tsx`): the record-conflict
//! policy per table (PRD 19).
//!
//! Test ids: `entities-empty`, per table a row `entity:<table>`, selects
//! `Concurrency policy for <table>` and `Action for <table>`, `entity-status:<table>`
//! ("No policy" | "Action missing" | "Policy set", as a badge), and for settings whose table
//! is gone `orphan:<table>` and `Remove settings for <table>`.

use crate::ui::controls::select;
use crate::recordstore::EntitySettings;
use crate::ui::{
    display::{Tone, status_badge},
    prelude::*,
    shell::Shell,
    widgets,
};

const POLICIES: &[(&str, &str)] = &[
    ("optimistic", "Optimistic: reject stale edits"),
    ("lastWriteWins", "Last write wins"),
    ("customAction", "Custom transactional action"),
];

pub struct EntitiesTab {
    shell: Entity<Shell>,
    error: String,
    _subscription: Subscription,
}

impl EntitiesTab {
    pub fn new(shell: Entity<Shell>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        // The config may have changed behind the store (migrations); pick it up.
        let store = shell.read(cx).store.clone();
        let _ = store.update(cx, |s, cx| s.reload(None, cx));
        Self {
            shell,
            error: String::new(),
            _subscription: subscription,
        }
    }

    /// Names of the user tables.
    pub fn tables(&self, cx: &App) -> Vec<String> {
        self.shell
            .read(cx)
            .objects
            .iter()
            .filter(|o| o.object_type == "table" && !o.name.starts_with("_ixtable_"))
            .map(|o| o.name.clone())
            .collect()
    }

    fn save(&mut self, table: &str, patch: impl FnOnce(&mut EntitySettings) + 'static, cx: &mut Context<Self>) {
        let name = table.to_string();
        let store = self.shell.read(cx).store.clone();
        let result = store.update(
            cx,
            |s, cx| {
                s.update(
                    &format!("Concurrency policy for {name}"),
                    move |c| {
                        let index = c.entities.iter().position(|e| e.table == name);
                        let mut entity = index.map_or_else(
                            || EntitySettings {
                                id: uuid::Uuid::new_v4().to_string(),
                                table: name.clone(),
                                concurrency: "optimistic".into(),
                                action_id: None,
                                fields: vec![],
                            },
                            |i| c.entities[i].clone(),
                        );
                        patch(&mut entity);
                        match index {
                            Some(i) => c.entities[i] = entity,
                            None => c.entities.push(entity),
                        }
                    },
                    cx,
                )
            },
        );
        self.error = result.err().map(|e| e.message).unwrap_or_default();
        cx.notify();
    }

    fn remove_orphan(&mut self, id: &str, table: &str, cx: &mut Context<Self>) {
        let id = id.to_string();
        let store = self.shell.read(cx).store.clone();
        let result = store.update(
            cx,
            |s, cx| s.update(&format!("Remove entity {table}"), move |c| c.entities.retain(|e| e.id != id), cx),
        );
        self.error = result.err().map(|e| e.message).unwrap_or_default();
        cx.notify();
    }
}

impl Render for EntitiesTab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tables = self.tables(cx);
        let config = self.shell.read(cx).config(cx).clone();
        let entity = cx.entity();
        let border = cx.theme().border;
        let mut panel = v_flex()
            .gap_3()
            .child(widgets::section_heading("tab-title", "Record conflicts"))
            .child(div().text_sm().text_color(cx.theme().muted_foreground).child(
                "Choose how each table resolves conflicting edits. Optimistic entities compare the values a user started from and reject the save if someone else changed the record first.",
            ));
        if tables.is_empty() {
            panel = panel.child(widgets::text("entities-empty", "No tables yet."));
        }
        for table in &tables {
            let settings = config.entities.iter().find(|e| &e.table == table);
            let policy = settings.map(|e| e.concurrency.clone()).unwrap_or_default();
            let action_id = settings.and_then(|e| e.action_id.clone()).unwrap_or_default();
            let missing_action = policy == "customAction" && !config.actions.iter().any(|a| a.id == action_id);
            let (status, tone) = if policy.is_empty() {
                ("No policy", Tone::Warning)
            } else if missing_action {
                ("Action missing", Tone::Danger)
            } else {
                ("Policy set", Tone::Success)
            };
            let mut options: Vec<(String, String)> =
                POLICIES.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect();
            if policy.is_empty() {
                options.insert(0, (String::new(), "Not set".into()));
            }
            let (policy_entity, action_entity) = (entity.clone(), entity.clone());
            let (policy_table, action_table) = (table.clone(), table.clone());
            let actions: Vec<(String, String)> = std::iter::once((String::new(), "Choose an action".to_string()))
                .chain(config.actions.iter().map(|a| (a.id.clone(), a.name.clone())))
                .collect();
            panel = panel.child(
                h_flex()
                    .id(SharedString::from(format!("entity:{table}")))
                    .aria_label(table.clone())
                    .gap_3()
                    .items_center()
                    .py_1()
                    .border_b_1()
                    .border_color(border)
                    .child(div().w(px(180.)).child(table.clone()))
                    .child(select(
                        SharedString::from(format!("Concurrency policy for {table}")),
                        &policy,
                        options,
                        "Not set",
                        move |value, _, cx| {
                            let value = value.to_string();
                            policy_entity.update(cx, |this, cx| {
                                this.save(&policy_table, move |e| e.concurrency = value, cx)
                            })
                        },
                    ))
                    .when(policy == "customAction", |el| {
                        el.child(select(
                            SharedString::from(format!("Action for {table}")),
                            &action_id,
                            actions,
                            "Choose an action",
                            move |value, _, cx| {
                                let value = (!value.is_empty()).then(|| value.to_string());
                                action_entity.update(cx, |this, cx| {
                                    this.save(&action_table, move |e| e.action_id = value, cx)
                                })
                            },
                        ))
                    })
                    .child(status_badge(SharedString::from(format!("entity-status:{table}")), status, tone, cx))
                    .test_support(),
            );
        }
        for orphan in config.entities.iter().filter(|e| !tables.contains(&e.table)) {
            let (id, table) = (orphan.id.clone(), orphan.table.clone());
            panel = panel.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(widgets::text(
                        SharedString::from(format!("orphan:{table}")),
                        format!("Settings for missing table {table}"),
                    ))
                    .child(
                        Button::new(SharedString::from(format!("Remove settings for {table}")))
                            .outline()
                            .small()
                            .label(format!("Remove settings for {table}"))
                            .on_click(cx.listener(move |this, _, _, cx| this.remove_orphan(&id, &table, cx))),
                    ),
            );
        }
        if !self.error.is_empty() {
            panel = panel.child(crate::ui::controls::error_line("entities-error", &self.error, cx));
        }
        panel
    }
}
