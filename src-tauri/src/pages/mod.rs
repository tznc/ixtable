//! Page definitions (PRD §16).
//!
//! A dashboard is a grid layout (the same `design::GridLayout` / `design::Placement`
//! types forms use, validated by the same grid rules) holding components that point
//! at saved queries, forms, reports, and actions. Rust stores and validates
//! definitions only; queries run through `queries.rs`, and charts, KPI expressions,
//! and filters are evaluated by the TypeScript dashboard view.
use crate::archive::{check_named_ids, DocumentConfig, Issue};
use crate::design::{
    validate_grid_layout, validate_grid_span, ConditionalStyle, FormMode, GridLayout, Placement,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub mod upgrade;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub layout: GridLayout,
    #[serde(default)]
    pub filters: Vec<PageFilter>,
    #[serde(default)]
    pub components: Vec<PageComponent>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum FilterControl {
    Select,
    #[default]
    Text,
    Date,
    Number,
    /// Binds two parameters: `<param>From` and `<param>To`.
    DateRange,
}

/// A dashboard-wide input whose value is passed to every component query as `param`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageFilter {
    pub id: String,
    pub name: String,
    pub param: String,
    #[serde(default = "text_type")]
    pub logical_type: String,
    #[serde(default, rename = "default", skip_serializing_if = "Option::is_none")]
    pub default_value: Option<serde_json::Value>,
    #[serde(default)]
    pub control: FilterControl,
    /// Fixed choices for `select` filters.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    /// Saved query whose first column supplies `select` choices.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_query_id: Option<String>,
}
fn text_type() -> String {
    "text".into()
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ComponentKind {
    #[default]
    Kpi,
    Table,
    Chart,
    Filter,
    Form,
    Report,
    Button,
    Text,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum ChartType {
    #[default]
    Bar,
    Line,
    Area,
    Pie,
    Donut,
    Scatter,
    Summary,
}

/// KPI comparison: a second value from the same result (field or expression).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KpiComparison {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    #[serde(default)]
    pub label: String,
}

/// One component on the dashboard grid. Fields not used by `kind` stay empty.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageComponent {
    pub id: String,
    pub kind: ComponentKind,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub placement: Placement,
    /// kpi, table, chart: the saved query supplying rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_id: Option<String>,
    /// kpi: column read from the first row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_field: Option<String>,
    /// kpi: expression over `rows` and `params` (takes precedence over `valueField`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression: Option<String>,
    /// kpi, chart: format pattern (src/expr `format`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comparison: Option<KpiComparison>,
    /// table: shown columns (empty shows all).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub columns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chart_type: Option<ChartType>,
    /// chart: category (or x value) column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,
    /// chart: value columns, one series each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub y: Vec<String>,
    /// chart: column whose values split the first y column into series.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_by: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stacked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub form_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<FormMode>,
    /// form (detail, edit): record id expression over `params` and `app`; blank opens the source's first row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_id: Option<String>,
    /// button: caption.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// text: plain text; blank lines separate paragraphs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// table: row filter expression over `record`, `params` and `app`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// table: conditional styles, each naming its column.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub styles: Vec<ConditionalStyle>,
    /// Shown when this expression over `params` and `app` holds (blank: always).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<String>,
    /// Enabled when this expression holds (blank: always).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_when: Option<String>,
}

/// The query parameter names a filter supplies.
pub fn filter_params(filter: &PageFilter) -> Vec<String> {
    match filter.control {
        FilterControl::DateRange => vec![
            format!("{}From", filter.param),
            format!("{}To", filter.param),
        ],
        _ => vec![filter.param.clone()],
    }
}

fn missing(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|text| text.trim().is_empty())
}

struct Checker<'a> {
    config: &'a DocumentConfig,
    dashboard: &'a Page,
    out: Vec<Issue>,
}

impl Checker<'_> {
    fn error(&mut self, kind: &str, id: &str, message: String) {
        self.out.push(Issue::error(kind, id, message));
    }
    fn warning(&mut self, kind: &str, id: &str, message: String) {
        self.out.push(Issue::warning(kind, id, message));
    }
    fn query_exists(&self, id: &str) -> bool {
        self.config.saved_queries.iter().any(|q| q.id == id)
    }

    fn layout(&mut self) {
        let dashboard = self.dashboard;
        let regions = match validate_grid_layout(&dashboard.layout) {
            Ok(regions) => regions,
            Err(message) => {
                self.error(
                    "page",
                    &dashboard.id,
                    format!("{}: {message}", dashboard.name),
                );
                return;
            }
        };
        let columns = dashboard.layout.columns.len();
        for component in &dashboard.components {
            let p = &component.placement;
            let result = match &p.region {
                Some(region) if regions.contains(region) => Ok(()),
                Some(_) => Err("placement references an unknown region".to_string()),
                None => validate_grid_span(p.column, p.row, p.column_span, p.row_span, columns),
            };
            if let Err(message) = result {
                self.error("dashboardComponent", &component.id, message);
            }
        }
    }

    fn filters(&mut self) {
        let dashboard = self.dashboard;
        let mut ids = HashSet::new();
        let mut params = HashSet::new();
        for filter in &dashboard.filters {
            if filter.id.trim().is_empty() || !ids.insert(filter.id.as_str()) {
                self.error(
                    "dashboardFilter",
                    &filter.id,
                    "filter ids must be non-empty and unique".into(),
                );
            }
            if filter.param.trim().is_empty() {
                self.error(
                    "dashboardFilter",
                    &filter.id,
                    format!("filter \"{}\" has no parameter name", filter.name),
                );
            } else if !params.insert(filter.param.as_str()) {
                self.error(
                    "dashboardFilter",
                    &filter.id,
                    format!("more than one filter sets parameter {}", filter.param),
                );
            }
            if let Some(query) = &filter.options_query_id {
                if !self.query_exists(query) {
                    self.error(
                        "dashboardFilter",
                        &filter.id,
                        format!(
                            "filter \"{}\" lists choices from a saved query that does not exist",
                            filter.name
                        ),
                    );
                }
            }
        }
        // Each filter parameter should be declared by a query the dashboard runs.
        let declared: HashSet<&str> = component_queries(self.config, dashboard)
            .flat_map(|q| q.parameters.iter().map(|p| p.name.as_str()))
            .collect();
        for filter in &dashboard.filters {
            if filter.param.trim().is_empty() {
                continue;
            }
            let unused: Vec<String> = filter_params(filter)
                .into_iter()
                .filter(|name| !declared.contains(name.as_str()))
                .collect();
            if !unused.is_empty() {
                let list = unused
                    .iter()
                    .map(|n| format!("${n}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                self.warning(
                    "dashboardFilter",
                    &filter.id,
                    format!(
                        "filter \"{}\" sets {list}, which no query on this dashboard declares",
                        filter.name
                    ),
                );
            }
        }
        // Required query parameters need a filter (or a default) to supply them.
        let supplied: HashSet<String> = dashboard.filters.iter().flat_map(filter_params).collect();
        for component in &dashboard.components {
            let Some(query) = component
                .query_id
                .as_ref()
                .and_then(|id| self.config.saved_queries.iter().find(|q| &q.id == id))
            else {
                continue;
            };
            for parameter in &query.parameters {
                let has_default = parameter
                    .default_value
                    .as_ref()
                    .is_some_and(|v| !v.is_null());
                if parameter.required && !has_default && !supplied.contains(&parameter.name) {
                    self.warning(
                        "dashboardComponent",
                        &component.id,
                        format!(
                            "query \"{}\" requires ${} but no dashboard filter sets it",
                            query.name, parameter.name
                        ),
                    );
                }
            }
        }
    }

    fn components(&mut self) {
        let dashboard = self.dashboard;
        let config = self.config;
        let mut ids = HashSet::new();
        for c in &dashboard.components {
            let id = c.id.as_str();
            let kind = "dashboardComponent";
            let title = if c.title.is_empty() {
                id
            } else {
                c.title.as_str()
            };
            if id.trim().is_empty() || !ids.insert(id) {
                self.error(
                    kind,
                    id,
                    "component ids must be non-empty and unique within a dashboard".into(),
                );
            }
            if matches!(
                c.kind,
                ComponentKind::Kpi | ComponentKind::Table | ComponentKind::Chart
            ) {
                match &c.query_id {
                    Some(q) if self.query_exists(q) => {}
                    Some(_) => self.error(
                        kind,
                        id,
                        format!("\"{title}\" uses a saved query that does not exist"),
                    ),
                    None => self.warning(kind, id, format!("\"{title}\" has no saved query")),
                }
            }
            for (name, value) in [
                ("filter", &c.filter),
                ("visibility", &c.visible_when),
                ("enabled", &c.enabled_when),
            ] {
                if value.as_deref().is_some_and(|t| t.trim().is_empty()) {
                    self.error(
                        kind,
                        id,
                        format!("\"{title}\" has an empty {name} expression"),
                    );
                }
            }
            if c.styles.iter().any(|s| s.when.trim().is_empty()) {
                self.error(
                    kind,
                    id,
                    format!("\"{title}\" has a conditional style with an empty condition"),
                );
            }
            if c.kind == ComponentKind::Table
                && c.styles
                    .iter()
                    .any(|s| s.column.as_deref().is_none_or(|t| t.trim().is_empty()))
            {
                self.error(
                    kind,
                    id,
                    format!("\"{title}\" has a conditional style with no column"),
                );
            }
            match c.kind {
                ComponentKind::Kpi => {
                    if missing(&c.value_field) && missing(&c.expression) {
                        self.warning(
                            kind,
                            id,
                            format!("KPI \"{title}\" needs a value field or expression"),
                        );
                    }
                }
                ComponentKind::Chart => {
                    let summary = c.chart_type == Some(ChartType::Summary);
                    if missing(&c.x) && !summary {
                        self.warning(kind, id, format!("chart \"{title}\" has no x field"));
                    }
                    if c.y.iter().all(|y| y.trim().is_empty()) {
                        self.warning(kind, id, format!("chart \"{title}\" has no value fields"));
                    }
                }
                ComponentKind::Table => {
                    if c.page_size == Some(0) {
                        self.error(
                            kind,
                            id,
                            format!("table \"{title}\" needs a page size of at least 1"),
                        );
                    }
                }
                ComponentKind::Filter => match &c.filter_id {
                    Some(f) if dashboard.filters.iter().any(|x| &x.id == f) => {}
                    _ => self.error(
                        kind,
                        id,
                        format!("\"{title}\" shows a filter that does not exist"),
                    ),
                },
                ComponentKind::Form => match &c.form_id {
                    Some(f) if config.design.forms.iter().any(|x| &x.id == f) => {
                        let form = config.design.forms.iter().find(|x| &x.id == f);
                        if let (Some(form), Some(mode)) = (form, c.mode) {
                            if !form_supports(form, mode) {
                                self.warning(
                                    kind,
                                    id,
                                    format!("\"{title}\" opens its form in a mode the form does not support"),
                                );
                            }
                        }
                    }
                    Some(_) => self.error(
                        kind,
                        id,
                        format!("\"{title}\" embeds a form that does not exist"),
                    ),
                    None => self.warning(kind, id, format!("\"{title}\" has no form")),
                },
                ComponentKind::Report => match &c.report_id {
                    Some(r) if config.reports.iter().any(|x| &x.id == r) => {}
                    Some(_) => self.error(
                        kind,
                        id,
                        format!("\"{title}\" embeds a report that does not exist"),
                    ),
                    None => self.warning(kind, id, format!("\"{title}\" has no report")),
                },
                ComponentKind::Button => match &c.action_id {
                    Some(a) if config.actions.iter().any(|x| &x.id == a) => {
                        let action = config.actions.iter().find(|x| &x.id == a);
                        let sets_form = action.is_some_and(|action| {
                            crate::automation::all_steps(&action.steps).iter().any(|s| {
                                s.kind == "setState"
                                    && s.fields.get("scope").and_then(|v| v.as_str())
                                        == Some("form")
                            })
                        });
                        // A warning: the step may sit in a branch that never runs here.
                        if sets_form {
                            self.warning(
                                kind,
                                id,
                                format!("button \"{title}\" runs an action that sets form state, but a dashboard button has no form"),
                            );
                        }
                    }
                    Some(_) => self.error(
                        kind,
                        id,
                        format!("button \"{title}\" runs an action that does not exist"),
                    ),
                    None => self.warning(kind, id, format!("button \"{title}\" has no action")),
                },
                ComponentKind::Text => {}
            }
        }
    }
}

/// Whether an embedded form can open in `mode`: the form offers it, and query
/// sources (read-only) only list and show records.
fn form_supports(form: &crate::design::Form, mode: FormMode) -> bool {
    let read_only = form
        .source
        .as_ref()
        .is_some_and(|s| s.kind == crate::design::SourceKind::Query);
    form.modes.contains(&mode) && !(read_only && matches!(mode, FormMode::Create | FormMode::Edit))
}

fn component_queries<'a>(
    config: &'a DocumentConfig,
    dashboard: &'a Page,
) -> impl Iterator<Item = &'a crate::archive::SavedQuery> {
    let ids: HashSet<&str> = dashboard
        .components
        .iter()
        .filter_map(|c| c.query_id.as_deref())
        .collect();
    config
        .saved_queries
        .iter()
        .filter(move |q| ids.contains(q.id.as_str()))
}

/// Dependency and grid validation for every dashboard (non-blocking issues).
pub fn validate(config: &DocumentConfig) -> Vec<Issue> {
    let mut out = check_named_ids(
        "page",
        config
            .pages
            .iter()
            .map(|d| (d.id.as_str(), d.name.as_str())),
    );
    for dashboard in &config.pages {
        let mut checker = Checker {
            config,
            dashboard,
            out: vec![],
        };
        checker.layout();
        checker.filters();
        checker.components();
        out.extend(checker.out);
    }
    out
}

#[cfg(test)]
mod tests;
