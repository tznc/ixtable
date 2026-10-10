//! Design validation: blocking structural checks and non-blocking dependency issues.
use super::{
    Control, ControlKind, DesignSchema, Form, FormMode, GridLayout, GridTrack, NavKind,
    NavigationItem, SourceKind, TrackKind, DESIGN_SCHEMA_VERSION,
};
use crate::archive::{DocumentConfig, Issue};
use std::collections::{HashMap, HashSet};

fn validate_tracks(tracks: &[GridTrack], label: &str) -> Result<(), String> {
    if tracks.is_empty() && label == "columns" {
        return Err("grid layout requires at least one column track".into());
    }
    for track in tracks {
        match track.kind {
            TrackKind::Content => {}
            TrackKind::Fixed | TrackKind::Fr => {
                if track.value.is_none_or(|value| value <= 0.0) {
                    return Err(format!("{label} tracks need a positive size"));
                }
            }
        }
        if track.min.zip(track.max).is_some_and(|(min, max)| min > max) {
            return Err(format!("{label} track minimum cannot exceed maximum"));
        }
    }
    Ok(())
}

pub(crate) fn validate_span(
    column: u16,
    row: u16,
    column_span: u16,
    row_span: u16,
    columns: usize,
) -> Result<(), String> {
    if column == 0 || row == 0 || column_span == 0 || row_span == 0 {
        return Err("grid placement uses 1-based positions and positive spans".into());
    }
    let end = column as usize + column_span as usize - 1;
    if end > columns {
        return Err("grid placement exceeds declared columns".into());
    }
    Ok(())
}

pub(crate) fn validate_layout(layout: &GridLayout) -> Result<HashSet<&String>, String> {
    validate_tracks(&layout.columns, "columns")?;
    validate_tracks(&layout.rows, "rows")?;
    for breakpoint in &layout.breakpoints {
        validate_tracks(&breakpoint.columns, "breakpoint columns")?;
    }
    let mut region_names = HashSet::new();
    for region in &layout.named_regions {
        if region.name.is_empty() || !region_names.insert(&region.name) {
            return Err("named grid regions must be unique".into());
        }
        validate_span(
            region.column,
            region.row,
            region.column_span,
            region.row_span,
            layout.columns.len(),
        )?;
    }
    Ok(region_names)
}

/// The grid a control is placed in: its container's inner layout, else the form's.
fn container_layout<'a>(form: &'a Form, control: &Control) -> &'a GridLayout {
    control
        .parent
        .as_ref()
        .and_then(|parent| form.controls.iter().find(|c| c.id == parent.id))
        .and_then(|container| container.layout.as_ref())
        .unwrap_or(&form.layout)
}

pub fn structural(design: &DesignSchema) -> Result<(), String> {
    if design.version != DESIGN_SCHEMA_VERSION {
        return Err(format!(
            "unsupported design schema version {}",
            design.version
        ));
    }
    let mut forms = HashSet::new();
    for form in &design.forms {
        if form.id.is_empty() || !forms.insert(&form.id) {
            return Err("design form ids must be non-empty and unique".into());
        }
        let form_regions = validate_layout(&form.layout)?;
        let mut controls = HashSet::new();
        for control in &form.controls {
            if control.id.is_empty() || !controls.insert(&control.id) {
                return Err("control ids must be non-empty and unique within a form".into());
            }
            if let Some(layout) = &control.layout {
                validate_layout(layout)?;
            }
            let layout = container_layout(form, control);
            let placement = &control.placement;
            match &placement.region {
                Some(region) if std::ptr::eq(layout, &form.layout) => {
                    if !form_regions.contains(region) {
                        return Err("control placement references an unknown region".into());
                    }
                }
                Some(region) => {
                    if !layout.named_regions.iter().any(|r| &r.name == region) {
                        return Err("control placement references an unknown region".into());
                    }
                }
                None => validate_span(
                    placement.column,
                    placement.row,
                    placement.column_span,
                    placement.row_span,
                    layout.columns.len(),
                )?,
            }
            if control
                .validation
                .min
                .zip(control.validation.max)
                .is_some_and(|(min, max)| min > max)
            {
                return Err("validation minimum cannot exceed maximum".into());
            }
        }
    }
    Ok(())
}

fn blank(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| text.trim().is_empty())
}

fn missing(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(|text| text.trim().is_empty())
}

fn check_control(config: &DocumentConfig, form: &Form, control: &Control, out: &mut Vec<Issue>) {
    let id = &control.id;
    let mut error = |message: String| out.push(Issue::error("control", id, message));
    let label = &control.label;
    for (name, value) in [
        ("visibility", &control.visible_when),
        ("enabled", &control.enabled_when),
        ("computed value", &control.computed),
        ("default value", &control.default_value),
        ("validation", &control.validation.expression),
        (
            "lookup filter",
            &control.relationship.as_ref().and_then(|r| r.filter.clone()),
        ),
        (
            "related list filter",
            &control.related.as_ref().and_then(|r| r.filter.clone()),
        ),
    ] {
        if blank(value) {
            error(format!("\"{label}\" has an empty {name} expression"));
        }
    }
    if control.styles.iter().any(|s| s.when.trim().is_empty()) {
        error(format!(
            "\"{label}\" has a conditional style with an empty condition"
        ));
    }
    if let Some(parent) = &control.parent {
        match form.controls.iter().find(|c| c.id == parent.id) {
            Some(container) if container.kind.is_container() && container.id != control.id => {
                let tab_ok = match (&container.kind, &parent.tab) {
                    (ControlKind::Tabs, Some(tab)) => container.tabs.iter().any(|t| &t.id == tab),
                    (ControlKind::Tabs, None) => false,
                    _ => true,
                };
                if !tab_ok {
                    error(format!(
                        "\"{label}\" is placed in a tab that does not exist"
                    ));
                }
            }
            _ => error(format!(
                "\"{label}\" is placed in a container that does not exist"
            )),
        }
    }
    let queries: HashSet<&str> = config.saved_queries.iter().map(|q| q.id.as_str()).collect();
    match control.kind {
        ControlKind::Computed if missing(&control.computed) => {
            error(format!("computed value \"{label}\" has no expression"))
        }
        ControlKind::Relationship => match &control.relationship {
            Some(r)
                if !r.table.is_empty()
                    && !r.value_column.is_empty()
                    && !r.display_column.is_empty() =>
            {
                if !keys_complete(&r.keys) {
                    error(format!(
                        "relationship selector \"{label}\" has a key column without a target"
                    ));
                }
            }
            _ => error(format!(
                "relationship selector \"{label}\" needs a table, value column, and display column"
            )),
        },
        ControlKind::Select => {
            if let Some(query) = &control.options_query_id {
                if !queries.contains(query.as_str()) {
                    error(format!(
                        "select \"{label}\" uses a saved query that does not exist"
                    ));
                }
            }
        }
        ControlKind::Button => match &control.action_id {
            Some(action) if config.actions.iter().any(|a| &a.id == action) => {}
            Some(_) => error(format!(
                "button \"{label}\" runs an action that does not exist"
            )),
            None => out.push(Issue::warning(
                "control",
                id,
                format!("button \"{label}\" has no action"),
            )),
        },
        ControlKind::Tabs if control.tabs.is_empty() => {
            error(format!("tab group \"{label}\" has no tabs"))
        }
        ControlKind::RelatedList => match &control.related {
            Some(r)
                if !r.table.is_empty()
                    && !r.foreign_key.is_empty()
                    && !r.parent_column.is_empty() =>
            {
                if !keys_complete(&r.keys) {
                    error(format!(
                        "related list \"{label}\" has a key column without a parent column"
                    ));
                }
                if let Some(child) = &r.form_id {
                    if !config.design.forms.iter().any(|f| &f.id == child) {
                        error(format!(
                            "related list \"{label}\" opens a form that does not exist"
                        ));
                    }
                }
            }
            _ => error(format!(
                "related list \"{label}\" needs a table, foreign key, and parent column"
            )),
        },
        _ => {}
    }
    let needs_binding = matches!(
        control.kind,
        ControlKind::Text
            | ControlKind::Multiline
            | ControlKind::Number
            | ControlKind::Decimal
            | ControlKind::Boolean
            | ControlKind::Date
            | ControlKind::Time
            | ControlKind::Datetime
            | ControlKind::Select
            | ControlKind::Relationship
    );
    if needs_binding
        && control.binding.as_ref().is_none_or(|b| b.column.is_empty())
        && control.computed.is_none()
    {
        out.push(Issue::warning(
            "control",
            id,
            format!("\"{label}\" is not bound to a column"),
        ));
    }
}

fn check_navigation(
    config: &DocumentConfig,
    items: &[NavigationItem],
    ids: &mut HashSet<String>,
    out: &mut Vec<Issue>,
) {
    for item in items {
        let id = &item.id;
        if id.is_empty() || !ids.insert(id.clone()) {
            out.push(Issue::error(
                "navigation",
                id,
                "navigation ids must be non-empty and unique",
            ));
        }
        let target = item.target_id.as_deref().unwrap_or("");
        let exists = match item.kind {
            NavKind::Group => true,
            NavKind::Form => config.design.forms.iter().any(|f| f.id == target),
            NavKind::Report => config.reports.iter().any(|r| r.id == target),
            NavKind::Dashboard => config.dashboards.iter().any(|d| d.id == target),
            NavKind::Table => !target.is_empty(),
        };
        if !exists {
            out.push(Issue::error(
                "navigation",
                id,
                format!(
                    "navigation item \"{}\" opens something that does not exist",
                    item.label
                ),
            ));
        }
        check_navigation(config, &item.children, ids, out);
    }
}

/// Dependency validation over forms, controls, and navigation (non-blocking issues).
pub fn validate(config: &DocumentConfig) -> Vec<Issue> {
    let design = &config.design;
    let mut out = vec![];
    let embedded: HashSet<&str> = design
        .forms
        .iter()
        .flat_map(|f| f.controls.iter())
        .filter_map(|c| c.related.as_ref()?.form_id.as_deref())
        .collect();
    for form in &design.forms {
        let id = &form.id;
        if form.name.trim().is_empty() {
            out.push(Issue::warning("form", id, format!("form {id} has no name")));
        }
        match &form.source {
            Some(source) if source.kind == SourceKind::Query => {
                let query = source.query_id.as_deref().unwrap_or("");
                if !config.saved_queries.iter().any(|q| q.id == query) {
                    out.push(Issue::error(
                        "form",
                        id,
                        format!(
                            "form \"{}\" reads a saved query that does not exist",
                            form.name
                        ),
                    ));
                }
                if let Some(saved) = config.saved_queries.iter().find(|q| q.id == query) {
                    for (name, expr) in &source.params {
                        if !saved.parameters.iter().any(|p| &p.name == name) {
                            out.push(Issue::error(
                                "form",
                                id,
                                format!(
                                    "form \"{}\" binds ${name}, which its query does not declare",
                                    form.name
                                ),
                            ));
                        } else if expr.trim().is_empty() {
                            out.push(Issue::error(
                                "form",
                                id,
                                format!(
                                    "form \"{}\" binds ${name} to an empty expression",
                                    form.name
                                ),
                            ));
                        }
                    }
                }
                if form
                    .modes
                    .iter()
                    .any(|m| matches!(m, FormMode::Create | FormMode::Edit))
                {
                    out.push(Issue::warning("form", id, format!("form \"{}\" reads a query, so it is read-only; create and edit modes are ignored", form.name)));
                }
            }
            Some(source) if missing(&source.table) => {
                out.push(Issue::error(
                    "form",
                    id,
                    format!("form \"{}\" has no source table", form.name),
                ));
            }
            _ => {}
        }
        if let Some(detail) = &form.detail_form_id {
            if !design.forms.iter().any(|f| &f.id == detail) {
                out.push(Issue::error(
                    "form",
                    id,
                    format!(
                        "form \"{}\" opens a detail form that does not exist",
                        form.name
                    ),
                ));
            }
        }
        if form.source.is_none()
            && form
                .modes
                .iter()
                .any(|m| matches!(m, FormMode::Continuous | FormMode::Split))
        {
            out.push(Issue::warning(
                "form",
                id,
                format!(
                    "form \"{}\" has no source, so its continuous and split modes show no records",
                    form.name
                ),
            ));
        }
        for rule in &form.rules {
            if rule.expression.trim().is_empty() {
                out.push(Issue::error(
                    "form",
                    id,
                    format!("form \"{}\" has an empty validation rule", form.name),
                ));
            }
        }
        if blank(&form.filter) {
            out.push(Issue::error(
                "form",
                id,
                format!("form \"{}\" has an empty list filter", form.name),
            ));
        }
        let nested = form
            .controls
            .iter()
            .any(|c| c.kind == ControlKind::RelatedList);
        if nested && embedded.contains(id.as_str()) {
            out.push(Issue::error("form", id, format!("form \"{}\" is embedded in a related list and cannot hold its own related list (one level of master/detail only)", form.name)));
        }
        for control in &form.controls {
            check_control(config, form, control, &mut out);
        }
    }
    let mut ids = HashSet::new();
    check_navigation(config, &design.navigation, &mut ids, &mut out);
    if let Some(start) = &design.start_page {
        if !ids.contains(start) {
            out.push(Issue::error(
                "navigation",
                start,
                "the start page is not a navigation item",
            ));
        }
    }
    out
}

/// Checks table and column references against the live schema (table → columns).
pub fn validate_tables(
    config: &DocumentConfig,
    tables: &HashMap<String, Vec<String>>,
) -> Vec<Issue> {
    let mut out = vec![];
    let has = |table: &str, column: Option<&str>| {
        tables
            .get(table)
            .is_some_and(|cols| column.is_none_or(|c| cols.iter().any(|x| x == c)))
    };
    for form in &config.design.forms {
        let table = form
            .source
            .as_ref()
            .filter(|s| s.kind == SourceKind::Table)
            .and_then(|s| s.table.as_deref());
        if let Some(table) = table {
            if !has(table, None) {
                out.push(Issue::error(
                    "form",
                    &form.id,
                    format!(
                        "form \"{}\" uses table {table}, which does not exist",
                        form.name
                    ),
                ));
            }
        }
        for control in &form.controls {
            let mut error =
                |message: String| out.push(Issue::error("control", &control.id, message));
            if let (Some(table), Some(binding)) = (table, &control.binding) {
                if has(table, None)
                    && !binding.column.is_empty()
                    && !has(table, Some(&binding.column))
                {
                    error(format!(
                        "\"{}\" is bound to {table}.{}, which does not exist",
                        control.label, binding.column
                    ));
                }
            }
            if let Some(r) = &control.relationship {
                let targets = r.keys.iter().map(|k| &k.target);
                for column in [&r.value_column, &r.display_column]
                    .into_iter()
                    .chain(targets)
                {
                    if !has(&r.table, Some(column)) {
                        error(format!(
                            "\"{}\" looks up {}.{column}, which does not exist",
                            control.label, r.table
                        ));
                    }
                }
                if let Some(table) = table.filter(|t| has(t, None)) {
                    for key in r.keys.iter().filter(|k| !has(table, Some(&k.column))) {
                        error(format!(
                            "\"{}\" writes {table}.{}, which does not exist",
                            control.label, key.column
                        ));
                    }
                }
            }
            if let Some(r) = &control.related {
                let keys = r.keys.iter().map(|k| &k.column);
                for column in std::iter::once(&r.foreign_key).chain(keys) {
                    if !has(&r.table, Some(column)) {
                        error(format!(
                            "\"{}\" lists {}.{column}, which does not exist",
                            control.label, r.table
                        ));
                    }
                }
                if let Some(table) = table.filter(|t| has(t, None)) {
                    for key in r.keys.iter().filter(|k| !has(table, Some(&k.target))) {
                        error(format!(
                            "\"{}\" links to {table}.{}, which does not exist",
                            control.label, key.target
                        ));
                    }
                }
            }
        }
    }
    let mut stack: Vec<&NavigationItem> = config.design.navigation.iter().collect();
    while let Some(item) = stack.pop() {
        stack.extend(item.children.iter());
        if item.kind == NavKind::Table && !has(item.target_id.as_deref().unwrap_or(""), None) {
            out.push(Issue::error(
                "navigation",
                &item.id,
                format!(
                    "navigation item \"{}\" opens a table that does not exist",
                    item.label
                ),
            ));
        }
    }
    out
}

/// Table and column reference issues for the open document's design.
pub fn table_issues(
    window: &str,
    config: &DocumentConfig,
) -> Result<Vec<Issue>, crate::manager::AppError> {
    let manager = crate::manager()?;
    let mut tables = HashMap::new();
    for object in manager.database_objects(window)? {
        if let Ok(schema) = manager.table_schema(window, &object.name) {
            tables.insert(
                object.name,
                schema.columns.into_iter().map(|c| c.name).collect(),
            );
        }
    }
    Ok(validate_tables(config, &tables))
}

/// Config issues plus table/column references for the open document.
pub fn validate_document_design(window: &str) -> Result<Vec<Issue>, crate::manager::AppError> {
    let config = crate::manager()?.config(window)?;
    let mut issues = validate(&config);
    issues.extend(table_issues(window, &config)?);
    Ok(issues)
}

/// A multi-column key is complete when every pair names both columns.
fn keys_complete(keys: &[super::KeyPair]) -> bool {
    keys.iter()
        .all(|k| !k.column.is_empty() && !k.target.is_empty())
}
