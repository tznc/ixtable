//! Config validation (PRD §8): `Issue`, per-collection id checks, and the
//! `validate_config` aggregate over design and every feature module's `validate`.
use crate::archive::{DocumentConfig, CONFIG_VERSION};
use crate::manager::AppError;
use crate::{automation, pages, migrations, recordstore, reports, roles};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// One validation finding. Dependency errors are reported, not blocking.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: Severity,
    pub object_kind: String,
    pub object_id: String,
    pub message: String,
}
impl Issue {
    pub fn error(kind: &str, id: &str, message: impl Into<String>) -> Self {
        Self::with(Severity::Error, kind, id, message)
    }
    pub fn warning(kind: &str, id: &str, message: impl Into<String>) -> Self {
        Self::with(Severity::Warning, kind, id, message)
    }
    fn with(severity: Severity, kind: &str, id: &str, message: impl Into<String>) -> Self {
        Self {
            severity,
            object_kind: kind.into(),
            object_id: id.into(),
            message: message.into(),
        }
    }
}

/// Flags missing ids, duplicate ids, and blank names in one object collection.
pub fn check_named_ids<'a>(
    kind: &str,
    items: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<Issue> {
    let mut seen = std::collections::HashSet::new();
    let mut issues = vec![];
    for (id, name) in items {
        if id.trim().is_empty() {
            issues.push(Issue::error(
                kind,
                id,
                format!("{kind} \"{name}\" has no id"),
            ));
        } else if !seen.insert(id) {
            issues.push(Issue::error(kind, id, format!("duplicate {kind} id {id}")));
        }
        if name.trim().is_empty() {
            issues.push(Issue::warning(kind, id, format!("{kind} {id} has no name")));
        }
    }
    issues
}

/// Aggregates design validation and every feature module's `validate`.
pub fn validate_config(config: &DocumentConfig) -> Vec<Issue> {
    let mut issues = vec![];
    if config.version > CONFIG_VERSION {
        issues.push(Issue::error(
            "document",
            "config",
            format!("unsupported config version {}", config.version),
        ));
    }
    if let Err(message) = config.design.validate() {
        issues.push(Issue::error("design", "design", message));
    }
    issues.extend(check_named_ids(
        "query",
        config
            .saved_queries
            .iter()
            .map(|q| (q.id.as_str(), q.name.as_str())),
    ));
    issues.extend(crate::queries::validate(config));
    issues.extend(reports::validate(config));
    issues.extend(pages::validate(config));
    issues.extend(crate::page_engine::links::link_issues(config));
    issues.extend(automation::validate(config));
    issues.extend(migrations::validate(config));
    issues.extend(recordstore::validate(config));
    issues.extend(crate::import::validate(config));
    issues.extend(roles::validate(config));
    issues.extend(crate::design::validate(config));
    issues
}

pub fn validate_document(window_label: String) -> Result<Vec<Issue>, AppError> {
    let config = crate::manager()?.config(&window_label)?;
    let mut issues = validate_config(&config);
    issues.extend(crate::design::table_issues(&window_label, &config).unwrap_or_default());
    issues.extend(crate::recordstore::table_issues(&window_label, &config));
    Ok(issues)
}
