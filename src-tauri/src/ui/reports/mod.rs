//! Reports mode (PRD §15): report list, band designer, preview and PDF export. Port of
//! `src/reports/`; the layout and PDF engine is `crate::report_engine`.
//!
//! - `ReportsMode` is the mode's view: a report list, the toolbar (name, dataset, duplicate,
//!   delete, Design/Preview tabs) and the designer or the preview.
//! - `ReportDesigner` (`designer.rs`, `canvas.rs`, `props*.rs`, `settings.rs`) edits one report.
//! - `ReportPreview` (`preview.rs`) shows the laid-out pages; `ReportRun` (`run.rs`) opens a
//!   report from Runtime, asking for parameters first.
//!
//! Test ids: `new-report`, `report-item-N`, `no-reports`, `report-name`, `report-dataset`,
//! `duplicate-report`, `delete-report`, `tab-design`, `tab-preview`, `mode-title`.
//!
//! The dataset choice lists saved queries and tables/views directly; the Query agent's
//! shared `QueryPicker` can replace it once it lands.

mod canvas;
mod designer;
mod extra;
pub(crate) mod fields;
mod page_view;
mod preview;
mod props;
mod props_kinds;
mod run;
mod settings;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_designer;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests_run;

pub use designer::ReportDesigner;
pub(crate) use page_view::hex as page_color;
pub use page_view::{chart_element, page_image, page_view, PageImages};
pub use preview::ReportPreview;
pub use run::{ParameterPrompt, ReportRun};

use crate::report_engine::{
    data::{load_dataset, Session},
    model::{duplicate_report, new_report},
};
use crate::reports::Report;
use crate::ui::shell::{ModeId, RevealTarget, Shell};
use crate::ui::prelude::*;
use fields::{choice_field, input, Inputs};
use serde_json::Map;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Design,
    Preview,
}

const TABS: [(&str, &str); 2] = [("design", "Design"), ("preview", "Preview")];

/// "Report 1", "Report 2", ... the first not taken.
fn unique_name(base: &str, taken: &[String]) -> String {
    (1..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !taken.contains(name))
        .unwrap_or_default()
}

pub struct ReportsMode {
    shell: Entity<Shell>,
    pub selected: Option<String>,
    pub tab: Tab,
    focus: Option<String>,
    focus_seq: u64,
    designer: Option<(String, u64, Entity<ReportDesigner>)>,
    preview: Option<(String, Entity<ReportPreview>)>,
    columns: Vec<String>,
    columns_key: Option<String>,
    inputs: Inputs<ReportsMode, String>,
    _subscription: Subscription,
}

impl ReportsMode {
    pub fn new(shell: Entity<Shell>, _: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscription = cx.observe(&shell, |_, _, cx| cx.notify());
        Self {
            shell,
            selected: None,
            tab: Tab::Design,
            focus: None,
            focus_seq: 0,
            designer: None,
            preview: None,
            columns: vec![],
            columns_key: None,
            inputs: Inputs::new(Self::on_edit),
            _subscription: subscription,
        }
    }

    pub fn designer(&self) -> Option<Entity<ReportDesigner>> {
        self.designer.as_ref().map(|(_, _, d)| d.clone())
    }

    pub fn preview(&self) -> Option<Entity<ReportPreview>> {
        self.preview.as_ref().map(|(_, p)| p.clone())
    }

    /// Opens the report another mode asked to reveal.
    fn take_reveal(&mut self, cx: &mut Context<Self>) {
        let request = self.shell.update(cx, |s, _| s.take_reveal(ModeId::Reports));
        if let Some(RevealTarget::Reports { object_id, element_id }) = request {
            self.selected = Some(object_id);
            self.tab = Tab::Design;
            self.focus = element_id;
            self.focus_seq += 1;
            cx.notify();
        }
    }

    /// The report shown: the selected one, else the first.
    fn current(&self, cx: &App) -> Option<Report> {
        let config = self.shell.read(cx).config(cx);
        config
            .reports
            .iter()
            .find(|r| Some(&r.id) == self.selected.as_ref())
            .or(config.reports.first())
            .cloned()
    }

    /// Applies `mutate` to the current report as one undo step named `label`.
    fn change(&mut self, label: &str, mutate: impl FnOnce(&mut Report) + 'static, cx: &mut Context<Self>) {
        let Some(id) = self.current(cx).map(|r| r.id) else {
            return;
        };
        self.edit_config(
            label,
            move |config| {
                if let Some(report) = config.reports.iter_mut().find(|r| r.id == id) {
                    mutate(report);
                }
            },
            cx,
        );
    }

    fn edit_config(
        &mut self,
        label: &str,
        mutate: impl FnOnce(&mut crate::archive::DocumentConfig) + 'static,
        cx: &mut Context<Self>,
    ) {
        let store = self.shell.read(cx).store.clone();
        let result = store.update(cx, |s, cx| s.update(label, mutate, cx));
        if let Err(e) = result {
            self.shell.update(cx, |s, cx| s.set_error(Some(e), cx));
        }
        cx.notify();
    }

    fn on_edit(this: &mut Self, key: &String, text: String, cx: &mut Context<Self>) {
        if key == "report-name" {
            this.change("Rename report", move |r| r.name = text, cx);
        }
    }

    pub fn create(&mut self, cx: &mut Context<Self>) {
        let taken: Vec<String> = self.shell.read(cx).config(cx).reports.iter().map(|r| r.name.clone()).collect();
        let report = new_report(&unique_name("Report", &taken));
        self.selected = Some(report.id.clone());
        self.tab = Tab::Design;
        self.edit_config("New report", move |c| c.reports.push(report), cx);
    }

    pub fn duplicate(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.current(cx) else {
            return;
        };
        let copy = duplicate_report(&report, &format!("{} copy", report.name));
        self.selected = Some(copy.id.clone());
        self.edit_config("Duplicate report", move |c| c.reports.push(copy), cx);
    }

    pub fn delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.current(cx).map(|r| r.id) else {
            return;
        };
        self.selected = None;
        self.edit_config("Delete report", move |c| c.reports.retain(|r| r.id != id), cx);
    }

    /// Dataset choice value: `query:<id>`, `table:<name>` or "".
    fn dataset_value(report: &Report) -> String {
        if let Some(q) = report.dataset_query_id.as_ref().filter(|q| !q.is_empty()) {
            format!("query:{q}")
        } else if let Some(t) = report.table.as_ref().filter(|t| !t.is_empty()) {
            format!("table:{t}")
        } else {
            String::new()
        }
    }

    fn set_dataset(&mut self, value: &str, cx: &mut Context<Self>) {
        let (kind, id) = value.split_once(':').unwrap_or(("", ""));
        let (query, table) = (
            (kind == "query").then(|| id.to_string()),
            (kind == "table").then(|| id.to_string()),
        );
        self.change(
            "Change report dataset",
            move |r| {
                r.dataset_query_id = query;
                r.table = table;
            },
            cx,
        );
    }

    fn dataset_options(&self, cx: &App) -> Vec<(String, String)> {
        let shell = self.shell.read(cx);
        let mut options = vec![(String::new(), "No dataset".to_string())];
        options.extend(shell.config(cx).saved_queries.iter().map(|q| (format!("query:{}", q.id), format!("Query: {}", q.name))));
        options.extend(
            shell
                .objects
                .iter()
                .filter(|o| o.object_type == "table" || o.object_type == "view")
                .map(|o| (format!("table:{}", o.name), format!("Table: {}", o.name))),
        );
        options
    }

    /// Loads the dataset's column names when the dataset changed (for the field pickers).
    fn load_columns(&mut self, report: &Report, cx: &mut Context<Self>) {
        let sql = self
            .shell
            .read(cx)
            .config(cx)
            .saved_queries
            .iter()
            .find(|q| Some(&q.id) == report.dataset_query_id.as_ref())
            .map(|q| q.sql.clone())
            .unwrap_or_default();
        let key = format!("{}|{:?}|{:?}|{}", report.id, report.dataset_query_id, report.table, sql);
        if self.columns_key.as_ref() == Some(&key) {
            return;
        }
        self.columns_key = Some(key.clone());
        let label = self.shell.read(cx).label().to_string();
        let config = self.shell.read(cx).config(cx).clone();
        let report = report.clone();
        task::spawn_call(
            cx,
            move || -> Vec<String> {
                if report.dataset_query_id.is_none() {
                    return match &report.table {
                        Some(table) => crate::inspect_table(label, table.clone(), None)
                            .map(|s| s.columns.into_iter().map(|c| c.name).collect())
                            .unwrap_or_default(),
                        None => vec![],
                    };
                }
                let params: Map<String, serde_json::Value> =
                    report.params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                load_dataset(&Session(&label), &report, &config, &params, "report-columns", 1)
                    .ok()
                    .flatten()
                    .map(|run| run.columns)
                    .unwrap_or_default()
            },
            move |this, columns, cx| {
                if this.columns_key.as_ref() != Some(&key) {
                    return;
                }
                this.columns = columns.clone();
                if let Some(designer) = this.designer() {
                    designer.update(cx, |d, cx| d.set_columns(columns, cx));
                }
                cx.notify();
            },
        );
    }

    /// The designer of `report`, created again for another report or a new reveal.
    fn designer_for(&mut self, report: &Report, cx: &mut Context<Self>) -> Entity<ReportDesigner> {
        if let Some((id, seq, designer)) = &self.designer {
            if *id == report.id && *seq == self.focus_seq {
                return designer.clone();
            }
        }
        let (shell, id, focus, columns) = (self.shell.clone(), report.id.clone(), self.focus.take(), self.columns.clone());
        let designer = cx.new(|cx| {
            let mut d = ReportDesigner::new(shell, &id, focus, cx);
            d.columns = columns;
            d
        });
        self.designer = Some((report.id.clone(), self.focus_seq, designer.clone()));
        designer
    }

    fn preview_for(&mut self, report: &Report, cx: &mut Context<Self>) -> Entity<ReportPreview> {
        if let Some((id, preview)) = &self.preview {
            if *id == report.id {
                return preview.clone();
            }
        }
        let (shell, id) = (self.shell.clone(), report.id.clone());
        let preview = cx.new(|cx| ReportPreview::new(shell, &id, None, true, cx));
        self.preview = Some((report.id.clone(), preview.clone()));
        preview
    }

    fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.selected = Some(id.to_string());
        cx.notify();
    }

    fn set_tab(&mut self, key: &str, cx: &mut Context<Self>) {
        self.tab = if key == "preview" { Tab::Preview } else { Tab::Design };
        cx.notify();
    }

    fn render_list(&self, reports: &[Report], current: Option<&str>, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (border, muted) = (theme.border, theme.muted_foreground);
        v_flex()
            .id("report-list")
            .w(px(220.))
            .h_full()
            .flex_none()
            .gap_1()
            .p_3()
            .border_r_1()
            .border_color(border)
            .child(div().text_xs().text_color(muted).child("REPORTS"))
            .child(
                Button::new("new-report")
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("New report")
                    .on_click(cx.listener(|this, _, _, cx| this.create(cx))),
            )
            .children(reports.iter().enumerate().map(|(i, r)| {
                let id = r.id.clone();
                Button::new(SharedString::from(format!("report-item-{}", i + 1)))
                    .ghost()
                    .small()
                    .justify_start()
                    .label(r.name.clone())
                    .selected(current == Some(r.id.as_str()))
                    .on_click(cx.listener(move |this, _, _, cx| this.select(&id, cx)))
            }))
            .when(reports.is_empty(), |el| el.child(widgets::text("no-reports", "No reports yet.")))
    }

    fn render_toolbar(&mut self, report: &Report, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let name = self.inputs.get("report-name".to_string(), &report.name, "", window, cx);
        let entity = cx.entity();
        let dataset = choice_field(
            "report-dataset",
            "Dataset",
            &Self::dataset_value(report),
            self.dataset_options(cx),
            "No dataset",
            move |value, _, cx| entity.update(cx, |this, cx| this.set_dataset(value, cx)),
            cx,
        );
        let tab = if self.tab == Tab::Preview { "preview" } else { "design" };
        let entity = cx.entity();
        h_flex()
            .gap_3()
            .items_end()
            .px_3()
            .py_2()
            .child(widgets::field("Report name", input(&name, "report-name", "Report name", false), cx).w(px(220.)))
            .child(dataset)
            .child(
                Button::new("duplicate-report")
                    .outline()
                    .small()
                    .icon(IconName::Copy)
                    .label("Duplicate report")
                    .on_click(cx.listener(|this, _, _, cx| this.duplicate(cx))),
            )
            .child(
                Button::new("delete-report")
                    .outline()
                    .small()
                    .icon(IconName::Delete)
                    .label("Delete report")
                    .on_click(cx.listener(|this, _, _, cx| this.delete(cx))),
            )
            .child(widgets::tab_strip(
                "report-view-tabs",
                &TABS,
                tab,
                move |key, _, cx| entity.update(cx, |this, cx| this.set_tab(key, cx)),
                cx,
            ))
    }
}

impl Render for ReportsMode {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // The shell is being updated while it builds this view, so reveals are taken here.
        self.take_reveal(cx);
        let reports = self.shell.read(cx).config(cx).reports.clone();
        let report = self.current(cx);
        let title = report.as_ref().map_or("Reports".to_string(), |r| r.name.clone());
        let editor = match &report {
            Some(report) => {
                self.load_columns(report, cx);
                let body = match self.tab {
                    Tab::Design => self.designer_for(report, cx).into_any_element(),
                    Tab::Preview => self.preview_for(report, cx).into_any_element(),
                };
                v_flex()
                    .size_full()
                    .child(self.render_toolbar(report, window, cx))
                    .child(div().flex_1().min_h_0().child(body))
                    .into_any_element()
            }
            None => widgets::empty_state("no-report", "Create a report to start designing.", "Use New report in the list.", cx)
                .into_any_element(),
        };
        v_flex()
            .size_full()
            .child(widgets::titlebar("Project / Reports", &title, cx))
            .child(
                h_flex()
                    .id("reports-mode")
                    .flex_1()
                    .min_h_0()
                    .child(self.render_list(&reports, report.as_ref().map(|r| r.id.as_str()), cx))
                    .child(div().flex_1().min_w_0().h_full().child(editor)),
            )
    }
}
