//! "Export…" button with a CSV / Excel / JSON menu (port of `src/export/ExportMenu.tsx`
//! and `TableExport.tsx`). Choosing a format opens the save dialog (cancel does
//! nothing), then runs the export and reports the row count or the error.
//!
//! Test ids: `Export` (the button; a dropdown whose items are the formats, in order)
//! and `export-error`. The row count shows as a shell toast.

use super::names::{
    default_export_name, dialog_filters, exported_message, format_label, with_extension, EXPORT_FORMATS,
};
use crate::data::{Filter, Sort};
use crate::export::{ExportFormat, ExportSummary};
use crate::manager::AppError;
use crate::ui::{dialogs, prelude::*, shell::Shell, toast::Toast};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use std::sync::Arc;

/// Runs an export of the current view into the path; supplied by each entry point.
pub type ExportRunner = Arc<dyn Fn(ExportFormat, String) -> Result<ExportSummary, AppError> + Send + Sync>;

pub struct ExportMenu {
    name: String,
    runner: ExportRunner,
    disabled: bool,
    disabled_reason: Option<String>,
    busy: bool,
    shell: Entity<Shell>,
    error: String,
}

impl ExportMenu {
    /// `name` is the object name the default file name comes from.
    pub fn new(name: &str, runner: ExportRunner, shell: Entity<Shell>) -> Self {
        Self {
            name: name.to_string(),
            runner,
            disabled: false,
            disabled_reason: None,
            busy: false,
            shell,
            error: String::new(),
        }
    }

    pub fn set_disabled(&mut self, disabled: bool, reason: Option<String>, cx: &mut Context<Self>) {
        self.disabled = disabled;
        self.disabled_reason = reason;
        cx.notify();
    }

    /// Points the menu at another object and export.
    pub fn set_target(&mut self, name: &str, runner: ExportRunner, cx: &mut Context<Self>) {
        if self.name != name {
            self.error.clear();
        }
        self.name = name.to_string();
        self.runner = runner;
        cx.notify();
    }

    fn choose(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        self.error.clear();
        cx.notify();
        let default = default_export_name(&self.name, format);
        let pick = dialogs::save_file(cx, "Export data", &default, dialog_filters(format));
        task::after(cx, pick, move |this, path, cx| {
            let Some(path) = path else {
                return;
            };
            let path = with_extension(&path.to_string_lossy(), format);
            this.busy = true;
            cx.notify();
            let runner = this.runner.clone();
            task::spawn_call(
                cx,
                move || runner(format, path),
                |this, result, cx| {
                    this.busy = false;
                    match result {
                        Ok(summary) => {
                            let toast = Toast::success(exported_message(summary.rows));
                            this.shell.update(cx, |shell, cx| shell.toast(toast, cx));
                        }
                        Err(e) => this.error = e.message,
                    }
                    cx.notify();
                },
            );
        });
    }
}

/// An export of one table with the datasheet's sorts and filters.
pub fn table_runner(label: String, table: String, sorts: Vec<Sort>, filters: Vec<Filter>) -> ExportRunner {
    Arc::new(move |format, path| {
        tauri::async_runtime::block_on(crate::export::export_table(
            label.clone(),
            table.clone(),
            sorts.clone(),
            filters.clone(),
            format,
            path,
            None,
        ))
    })
}

/// Data mode: exports the whole table with the datasheet's sorts and filters.
pub fn table_export(
    shell: &Entity<Shell>,
    table: &str,
    sorts: Vec<Sort>,
    filters: Vec<Filter>,
    cx: &App,
) -> (String, ExportRunner) {
    let label = shell.read(cx).label().to_string();
    (table.to_string(), table_runner(label, table.to_string(), sorts, filters))
}

impl Render for ExportMenu {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let theme = cx.theme();
        let danger = theme.danger;
        let mut button = Button::new("Export")
            .outline()
            .small()
            .icon(IconName::ArrowDown)
            .label("Export…")
            .disabled(self.disabled || self.busy);
        if let (true, Some(reason)) = (self.disabled, self.disabled_reason.clone()) {
            button = button.tooltip(reason);
        }
        h_flex()
            .gap_2()
            .items_center()
            .child(button.dropdown_menu(move |menu, _, _| {
                let mut menu = menu;
                for format in EXPORT_FORMATS {
                    let entity = entity.clone();
                    menu = menu.item(
                        PopupMenuItem::new(format_label(format))
                            .on_click(move |_, _, cx| entity.update(cx, |this, cx| this.choose(format, cx))),
                    );
                }
                menu
            }))
            .when(!self.error.is_empty(), |el| {
                el.child(div().ty(Type::Body).text_color(danger).child(widgets::text("export-error", self.error.clone())))
            })
    }
}

#[cfg(test)]
#[path = "menu_tests.rs"]
mod tests;
