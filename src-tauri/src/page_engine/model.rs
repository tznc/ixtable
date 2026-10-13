//! Page definition helpers: creation, duplication, placement, grid changes and the
//! editor-side checks (port of `src/dashboards/model.ts`).

use crate::archive::{DocumentConfig, Issue, QueryParameter, SavedQuery, Severity};
use crate::pages::{ChartType, ComponentKind, Page, PageComponent, PageFilter, FilterControl};
use crate::design::{Form, FormMode, GridLayout, SourceKind};
use crate::forms::schema::FORM_MODES;
use crate::ui::grid::engine::{
    GridDelta, GridItemRef, SpanConstraints, clamp_regions, default_grid_layout, move_placement, next_placement,
    remap_region, resize_placement, validate_layout,
};

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Component kinds in palette order.
pub const COMPONENT_KINDS: [ComponentKind; 8] = [
    ComponentKind::Kpi,
    ComponentKind::Chart,
    ComponentKind::Table,
    ComponentKind::Filter,
    ComponentKind::Form,
    ComponentKind::Report,
    ComponentKind::Button,
    ComponentKind::Text,
];

pub const CHART_TYPES: [ChartType; 7] = [
    ChartType::Bar,
    ChartType::Line,
    ChartType::Area,
    ChartType::Pie,
    ChartType::Donut,
    ChartType::Scatter,
    ChartType::Summary,
];

pub const FILTER_CONTROLS: [FilterControl; 5] = [
    FilterControl::Select,
    FilterControl::Text,
    FilterControl::Date,
    FilterControl::Number,
    FilterControl::DateRange,
];

/// The serialized name of a kind (`kpi`, `button`, ...).
pub fn kind_key(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Kpi => "kpi",
        ComponentKind::Table => "table",
        ComponentKind::Chart => "chart",
        ComponentKind::Filter => "filter",
        ComponentKind::Form => "form",
        ComponentKind::Report => "report",
        ComponentKind::Button => "button",
        ComponentKind::Text => "text",
    }
}

pub fn kind_label(kind: ComponentKind) -> &'static str {
    match kind {
        ComponentKind::Kpi => "KPI",
        ComponentKind::Chart => "Chart",
        ComponentKind::Table => "Table",
        ComponentKind::Filter => "Filter",
        ComponentKind::Form => "Form",
        ComponentKind::Report => "Report",
        ComponentKind::Button => "Action button",
        ComponentKind::Text => "Text",
    }
}

pub fn chart_key(chart: ChartType) -> &'static str {
    match chart {
        ChartType::Bar => "bar",
        ChartType::Line => "line",
        ChartType::Area => "area",
        ChartType::Pie => "pie",
        ChartType::Donut => "donut",
        ChartType::Scatter => "scatter",
        ChartType::Summary => "summary",
    }
}

pub fn chart_label(chart: ChartType) -> &'static str {
    match chart {
        ChartType::Bar => "Bar",
        ChartType::Line => "Line",
        ChartType::Area => "Area",
        ChartType::Pie => "Pie",
        ChartType::Donut => "Donut",
        ChartType::Scatter => "Scatter",
        ChartType::Summary => "Summary",
    }
}

pub fn control_key(control: FilterControl) -> &'static str {
    match control {
        FilterControl::Select => "select",
        FilterControl::Text => "text",
        FilterControl::Date => "date",
        FilterControl::Number => "number",
        FilterControl::DateRange => "dateRange",
    }
}

pub fn control_label(control: FilterControl) -> &'static str {
    match control {
        FilterControl::Select => "Drop-down list",
        FilterControl::Text => "Text box",
        FilterControl::Date => "Date",
        FilterControl::Number => "Number",
        FilterControl::DateRange => "Date range",
    }
}

/// Default size on a 12-column grid, and resize limits, per component kind.
struct Size {
    columns: i64,
    rows: i64,
    min: SpanConstraints,
}

fn size(kind: ComponentKind) -> Size {
    let min = |columns: Option<i64>, rows: Option<i64>| SpanConstraints {
        min_column_span: columns,
        min_row_span: rows,
        ..Default::default()
    };
    let (columns, rows, min) = match kind {
        ComponentKind::Kpi => (3, 1, min(Some(2), None)),
        ComponentKind::Chart => (6, 2, min(Some(3), Some(2))),
        ComponentKind::Table => (6, 2, min(Some(3), None)),
        ComponentKind::Filter => (3, 1, min(Some(2), None)),
        ComponentKind::Form => (6, 3, min(Some(4), Some(2))),
        ComponentKind::Report => (12, 4, min(Some(6), Some(2))),
        ComponentKind::Button => (2, 1, min(None, None)),
        ComponentKind::Text => (4, 1, min(None, None)),
    };
    Size { columns, rows, min }
}

pub fn constraints_for(kind: ComponentKind) -> SpanConstraints {
    size(kind).min
}

pub fn new_dashboard(name: &str) -> Page {
    Page {
        id: new_id(),
        name: name.into(),
        layout: default_grid_layout(),
        filters: vec![],
        components: vec![],
    }
}

pub fn new_filter(name: &str, param: &str) -> PageFilter {
    PageFilter {
        id: new_id(),
        name: name.into(),
        param: param.into(),
        logical_type: "text".into(),
        control: FilterControl::Select,
        ..Default::default()
    }
}

/// A new component of `kind` at the first free slot of the dashboard grid; `extra` adjusts it.
pub fn new_component(kind: ComponentKind, dashboard: &Page, extra: impl FnOnce(&mut PageComponent)) -> PageComponent {
    let count = dashboard.layout.columns.len() as i64;
    let size = size(kind);
    let existing: Vec<_> = dashboard.components.iter().map(|c| c.placement.clone()).collect();
    let placement = next_placement(&dashboard.layout, &existing, Some(size.columns.min(count)), Some(size.rows));
    let n = dashboard.components.iter().filter(|c| c.kind == kind).count() + 1;
    let mut component = PageComponent {
        id: new_id(),
        kind,
        title: format!("{} {n}", kind_label(kind)),
        placement,
        ..Default::default()
    };
    match kind {
        ComponentKind::Chart => component.chart_type = Some(ChartType::Bar),
        ComponentKind::Table => component.page_size = Some(10),
        ComponentKind::Button => component.label = Some("Run".into()),
        ComponentKind::Form => component.mode = Some(FormMode::List),
        ComponentKind::Text => component.text = Some(String::new()),
        ComponentKind::Kpi | ComponentKind::Filter | ComponentKind::Report => {}
    }
    extra(&mut component);
    component
}

/// A copy with fresh ids; filter components follow their copied filters.
pub fn duplicate_dashboard(dashboard: &Page, name: &str) -> Page {
    let filter_ids: Vec<(String, String)> = dashboard.filters.iter().map(|f| (f.id.clone(), new_id())).collect();
    let mut copy = dashboard.clone();
    copy.id = new_id();
    copy.name = name.into();
    for (filter, (_, id)) in copy.filters.iter_mut().zip(&filter_ids) {
        filter.id = id.clone();
    }
    for component in &mut copy.components {
        component.id = new_id();
        if let Some(old) = component.filter_id.clone().filter(|f| !f.is_empty()) {
            if let Some((_, id)) = filter_ids.iter().find(|(from, _)| *from == old) {
                component.filter_id = Some(id.clone());
            }
        }
    }
    copy
}

/// Clamps every placement into the layout's base columns (after a column-count change).
/// `renames` are (old, new) region names, so placed components follow a renamed region.
pub fn with_layout(dashboard: &Page, next: GridLayout, renames: &[(String, String)]) -> Page {
    let mut layout = next;
    layout.named_regions = clamp_regions(&layout.named_regions, layout.columns.len());
    let mut out = dashboard.clone();
    for component in &mut out.components {
        let remapped = remap_region(&component.placement, &layout.named_regions, renames);
        let moved = move_placement(&remapped, GridDelta::default(), &layout);
        component.placement = resize_placement(&moved, GridDelta::default(), &layout, constraints_for(component.kind));
    }
    out.layout = layout;
    out
}

/// Editor-side checks: the shared grid rules plus missing references (mirrors `dashboards.rs`).
pub fn dashboard_issues(dashboard: &Page, config: &DocumentConfig) -> Vec<Issue> {
    let items: Vec<GridItemRef> = dashboard
        .components
        .iter()
        .map(|c| GridItemRef { id: c.id.clone(), placement: c.placement.clone() })
        .collect();
    let mut issues = validate_layout(&dashboard.layout, &items, "page", &dashboard.id, "dashboardComponent");
    let mut error = |id: &str, message: String| issues.push(Issue::error("dashboardComponent", id, message));
    for c in &dashboard.components {
        let title = if c.title.is_empty() { &c.id } else { &c.title };
        if !exists(config.saved_queries.iter().map(|q| &q.id), &c.query_id) {
            error(&c.id, format!("\"{title}\" uses a saved query that does not exist"));
        }
        if !exists(config.design.forms.iter().map(|f| &f.id), &c.form_id) {
            error(&c.id, format!("\"{title}\" embeds a form that does not exist"));
        }
        if !exists(config.reports.iter().map(|r| &r.id), &c.report_id) {
            error(&c.id, format!("\"{title}\" embeds a report that does not exist"));
        }
        if !exists(config.actions.iter().map(|a| &a.id), &c.action_id) {
            error(&c.id, format!("button \"{title}\" runs an action that does not exist"));
        }
        if c.kind == ComponentKind::Filter && !dashboard.filters.iter().any(|f| Some(&f.id) == c.filter_id.as_ref()) {
            error(&c.id, format!("\"{title}\" shows a filter that does not exist"));
        }
        if c.kind == ComponentKind::Table && c.styles.iter().any(|s| s.column.as_deref().is_none_or(|c| c.trim().is_empty())) {
            error(&c.id, format!("\"{title}\" has a conditional style with no column"));
        }
    }
    issues
}

/// True when `id` is unset or one of `ids`.
fn exists<'a>(mut ids: impl Iterator<Item = &'a String>, id: &Option<String>) -> bool {
    id.as_deref().is_none_or(|id| id.is_empty() || ids.any(|i| i == id))
}

/// Messages of the error-severity issues.
pub fn error_messages(issues: &[Issue]) -> Vec<String> {
    issues.iter().filter(|i| i.severity == Severity::Error).map(|i| i.message.clone()).collect()
}

/// Parameters declared by the saved queries this dashboard's components use, as (name, type).
pub fn dashboard_query_parameters(dashboard: &Page, queries: &[SavedQuery]) -> Vec<(String, String)> {
    let ids: Vec<&String> = dashboard.components.iter().filter_map(|c| c.query_id.as_ref()).collect();
    let mut seen: Vec<(String, String)> = vec![];
    for q in queries.iter().filter(|q| ids.contains(&&q.id)) {
        for QueryParameter { name, logical_type, .. } in &q.parameters {
            if !seen.iter().any(|(n, _)| n == name) {
                seen.push((name.clone(), logical_type.clone()));
            }
        }
    }
    seen
}

/// Modes a dashboard can open `form` in: the form's own modes, minus create and edit for
/// read-only query sources (all modes while no form is chosen).
pub fn embeddable_modes(form: Option<&Form>) -> Vec<FormMode> {
    let Some(form) = form else {
        return FORM_MODES.to_vec();
    };
    let read_only = form.source.as_ref().is_some_and(|s| s.kind == SourceKind::Query);
    form.modes.iter().copied().filter(|m| !(read_only && matches!(m, FormMode::Create | FormMode::Edit))).collect()
}
