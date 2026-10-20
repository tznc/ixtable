//! Studio's ribbon (`ui::ribbon`) above every page: Home (undo, records, Run App and its
//! "Run as" menu), Create (tables, queries, forms, pages, reports, actions, triggers),
//! External data (import, file sources, Access migration) and Database tools
//! (relationships, problems, roles, migrations, release). A command switches to the page
//! that owns the object and starts the same flow as the page's own button.
//!
//! Test ids: `tab-home`, `tab-create`, `tab-external`, `tab-tools`; commands are
//! `ribbon-<command>` (`ribbon-new-table`, `ribbon-problems`, ...), plus `undo`, `redo`
//! and the Run App split button `run-app`.

use super::{AutomationTab, ModeId, SelectionKind, Shell, settings::AppSettings};
use crate::ui::{automation::AutomationMode, data::workbench::DataMode, pages::DesignMode, prelude::*, ribbon};

pub const TABS: [(&str, &str); 4] =
    [("home", "Home"), ("create", "Create"), ("external", "External data"), ("tools", "Database tools")];

/// What a ribbon command does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Refresh,
    Find,
    NewTable,
    NewQuery,
    NewForm,
    NewPage,
    NewReport,
    NewAction,
    NewTrigger,
    ImportRecords,
    NewRelationship,
    /// Opens a settings tab by key (`problems`, `roles`, ...).
    Settings(&'static str),
}

impl Shell {
    /// Runs a ribbon command. Commands that start a page's own flow switch to the page
    /// first, then reach its view once this update ends.
    pub fn run_command(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::Refresh => self.records_changed(cx),
            Command::NewTable | Command::NewQuery => {
                let kind = if command == Command::NewTable { SelectionKind::NewTable } else { SelectionKind::NewQuery };
                self.change_mode(ModeId::Data, window, cx);
                self.select(Some(super::Selection { kind, id: String::new() }), cx);
            }
            Command::Find => self.with_view::<DataMode>(ModeId::Data, window, cx, |view, window, cx| {
                if let Some(sheet) = view.read(cx).sheet().cloned() {
                    sheet.update(cx, |s, cx| s.open_find(window, cx));
                }
            }),
            Command::ImportRecords => self.with_view::<DataMode>(ModeId::Data, window, cx, |view, window, cx| {
                view.update(cx, |v, cx| v.open_import(window, cx));
            }),
            Command::NewRelationship => self.with_view::<DataMode>(ModeId::Data, window, cx, |view, window, cx| {
                view.update(cx, |v, cx| v.open_relate(window, None, cx));
            }),
            Command::NewForm | Command::NewPage | Command::NewReport => {
                self.with_view::<DesignMode>(ModeId::Design, window, cx, move |view, _, cx| {
                    view.update(cx, |v, cx| match command {
                        Command::NewForm => v.new_form(cx),
                        Command::NewReport => v.new_report(cx),
                        _ => v.new_page(cx),
                    });
                })
            }
            Command::NewAction | Command::NewTrigger => {
                let tab = if command == Command::NewAction { AutomationTab::Actions } else { AutomationTab::Triggers };
                self.with_view::<AutomationMode>(ModeId::Automation, window, cx, move |view, _, cx| {
                    view.update(cx, |v, cx| v.create(tab, cx));
                })
            }
            Command::Settings(key) => self.with_view::<AppSettings>(ModeId::App, window, cx, move |view, _, cx| {
                view.update(cx, |v, cx| v.select(key, cx));
            }),
        }
    }

    /// Switches to `mode`, then runs `f` on its view after this update (the view reads the
    /// shell, which is borrowed until then).
    fn with_view<V: 'static>(
        &mut self,
        mode: ModeId,
        window: &mut Window,
        cx: &mut Context<Self>,
        f: impl FnOnce(Entity<V>, &mut Window, &mut App) + 'static,
    ) {
        self.change_mode(mode, window, cx);
        let Some(view) = self.mode_view().and_then(|v| v.downcast::<V>().ok()) else {
            return;
        };
        window.defer(cx, move |window, cx| f(view, window, cx));
    }

    pub(super) fn render_ribbon(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let shell = cx.entity();
        let groups = match self.ribbon_tab {
            "create" => self.create_groups(cx),
            "external" => self.external_groups(cx),
            "tools" => self.tools_groups(cx),
            _ => self.home_groups(cx),
        };
        ribbon::ribbon(
            &TABS,
            self.ribbon_tab,
            move |key, _, cx| {
                shell.update(cx, |s, cx| {
                    s.ribbon_tab = key;
                    cx.notify();
                })
            },
            None,
            groups,
            cx,
        )
    }

    /// A ribbon button that runs `command`.
    fn command(&self, id: &str, icon: IconName, label: &str, command: Command, cx: &mut Context<Self>) -> AnyElement {
        ribbon::command(&format!("ribbon-{id}"), icon, label)
            .disabled(self.pending.is_some())
            .on_click(cx.listener(move |this, _, window, cx| this.run_command(command, window, cx)))
            .into_any_element()
    }

    fn home_groups(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let store = self.store.read(cx);
        let undo_tip = store
            .undo_label()
            .map_or("Undo (Ctrl+Z)".to_string(), |l| format!("Undo {l} (Ctrl+Z)"));
        let redo_tip = store
            .redo_label()
            .map_or("Redo (Ctrl+Shift+Z)".to_string(), |l| format!("Redo {l} (Ctrl+Shift+Z)"));
        let (can_undo, can_redo) = (store.can_undo(), store.can_redo());
        let busy = self.pending.is_some();
        let can_find = self.doc.mode == ModeId::Data
            && self
                .mode_view()
                .and_then(|v| v.downcast::<DataMode>().ok())
                .is_some_and(|v| v.read(cx).sheet().is_some());
        let undo = vec![
            ribbon::command("undo", IconName::Undo2, "Undo")
                .tooltip(undo_tip)
                .disabled(!can_undo)
                .on_click(cx.listener(|this, _, _, cx| this.undo(cx)))
                .into_any_element(),
            ribbon::command("redo", IconName::Redo2, "Redo")
                .tooltip(redo_tip)
                .disabled(!can_redo)
                .on_click(cx.listener(|this, _, _, cx| this.redo(cx)))
                .into_any_element(),
        ];
        let records = vec![
            self.command("refresh", IconName::RefreshCw, "Refresh", Command::Refresh, cx),
            ribbon::command("ribbon-find", IconName::Search, "Find")
                .tooltip("Find in the open table")
                .disabled(!can_find)
                .on_click(cx.listener(|this, _, window, cx| this.run_command(Command::Find, window, cx)))
                .into_any_element(),
        ];
        let run = crate::ui::runtime::window::run_app_button(cx.entity(), &self.label, self.config(cx), busy)
            .into_any_element();
        vec![
            ribbon::group("undo", "Undo", undo, cx),
            ribbon::group("records", "Records", records, cx),
            ribbon::group("run", "Run", [run], cx),
        ]
    }

    fn create_groups(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let tables = vec![
            self.command("new-table", IconName::Table2, "Table", Command::NewTable, cx),
            self.command("new-query", IconName::CodeXml, "Query", Command::NewQuery, cx),
        ];
        let forms = vec![
            self.command("new-form", IconName::FileText, "Form", Command::NewForm, cx),
            self.command("new-page", IconName::LayoutDashboard, "Page", Command::NewPage, cx),
        ];
        let reports = vec![self.command("new-report", IconName::ChartColumn, "Report", Command::NewReport, cx)];
        let automation = vec![
            self.command("new-action", IconName::Zap, "Action", Command::NewAction, cx),
            self.command("new-trigger", IconName::Bell, "Trigger", Command::NewTrigger, cx),
        ];
        vec![
            ribbon::group("tables", "Tables", tables, cx),
            ribbon::group("forms", "Forms", forms, cx),
            ribbon::group("reports", "Reports", reports, cx),
            ribbon::group("automation", "Automation", automation, cx),
        ]
    }

    fn external_groups(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let import = vec![
            self.command("import-records", IconName::FolderOpen, "Import records", Command::ImportRecords, cx),
            self.command("file-sources", IconName::Folder, "File sources", Command::Settings("files"), cx),
        ];
        let mut groups = vec![ribbon::group("import", "Import", import, cx)];
        // The Access migration tab exists only after an Access import.
        if super::settings::visible("access", self, cx) {
            let access = vec![self.command(
                "access-migration",
                IconName::DatabaseZap,
                "Access migration",
                Command::Settings("access"),
                cx,
            )];
            groups.push(ribbon::group("access", "Access", access, cx));
        }
        groups
    }

    fn tools_groups(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let relationships =
            vec![self.command("new-relationship", IconName::Network, "Relationship", Command::NewRelationship, cx)];
        let analyze = vec![self.command("problems", IconName::TriangleAlert, "Problems", Command::Settings("problems"), cx)];
        let security = vec![self.command("roles", IconName::User, "Roles", Command::Settings("roles"), cx)];
        let advanced = vec![
            self.command("migrations", IconName::Database, "Migrations", Command::Settings("migrations"), cx),
            self.command("release", IconName::PackageOpen, "Release", Command::Settings("release"), cx),
        ];
        vec![
            ribbon::group("relationships", "Relationships", relationships, cx),
            ribbon::group("analyze", "Analyze", analyze, cx),
            ribbon::group("security", "Security", security, cx),
            ribbon::group("advanced", "Advanced", advanced, cx),
        ]
    }
}
