//! SQLite database used as an application archive (see https://sqlite.org/sqlar.html).
//!
//! The on-disk `.ixt` file is a SQLite database with versioned tables for metadata,
//! compressed record-store bytes, JSON document config, and application-asset
//! attachments. Working sessions extract `data.db`, `document.json`, and
//! `config.yaml`. The `.ixt` file is the source of truth after a successful save.
//! File-format I/O (versions, streaming payloads, preservation) lives in `archive_io`.
use crate::design::DesignSchema;
pub use crate::validation::{check_named_ids, validate_config, Issue, Severity};
use crate::{automation, dashboards, migrations, recordstore, reports, roles};
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::fs;
use uuid::Uuid;

pub use crate::archive_io::{
    read_archive, read_header, write_archive, ArchiveHeader, FORMAT_VERSION, MIN_FORMAT_VERSION,
};
/// Current `DocumentConfig.version`. Version 2 configs load through serde defaults.
pub const CONFIG_VERSION: u32 = 4;

#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Invalid archive: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("Invalid archive: {0}")]
    Invalid(String),
    #[error("Unsupported archive format {0}. This ixtable build opens formats {min} to {max}.", min = MIN_FORMAT_VERSION, max = FORMAT_VERSION)]
    Unsupported(i64),
    #[error(
        "This document was created by a newer ixtable (format {0}). Update ixtable to open it."
    )]
    NewerFormat(i64),
    #[error(
        "This document's configuration (version {0}) was created by a newer ixtable. Update ixtable to open it."
    )]
    NewerConfig(u32),
    #[error("Corrupt payload: {0}")]
    Corrupt(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentConfig {
    pub version: u32,
    pub name: String,
    pub active_mode: String,
    #[serde(default)]
    pub navigation_state: serde_json::Value,
    #[serde(default)]
    pub settings: serde_json::Value,
    #[serde(default)]
    pub saved_queries: Vec<SavedQuery>,
    #[serde(default)]
    pub design: DesignSchema,
    #[serde(default)]
    pub reports: Vec<reports::Report>,
    #[serde(default)]
    pub dashboards: Vec<dashboards::Dashboard>,
    #[serde(default)]
    pub actions: Vec<automation::ActionDef>,
    #[serde(default)]
    pub triggers: Vec<automation::Trigger>,
    #[serde(default)]
    pub migrations: Vec<migrations::Migration>,
    #[serde(default)]
    pub datasource: recordstore::DatasourceConfig,
    #[serde(default)]
    pub entities: Vec<recordstore::EntitySettings>,
    #[serde(default)]
    pub roles: Vec<roles::Role>,
    #[serde(default)]
    pub release: ReleaseInfo,
    /// Bundled read-only CSV, JSON, and Parquet files (`import::sources`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub file_sources: Vec<crate::import::FileSource>,
    /// The ixtable Cloud application this document publishes to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud: Option<crate::cloud::CloudLink>,
    /// Top-level fields this build does not know (written by a newer ixtable with
    /// the same config version). Kept verbatim through load, save, and YAML.
    /// Only top-level fields: an unknown field inside a nested object (a form, a
    /// query) is dropped on save. A newer build that adds nested fields must bump
    /// the config version, which this build refuses to open (`NewerConfig`).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SavedQuery {
    pub id: String,
    pub name: String,
    pub sql: String,
    #[serde(default)]
    pub filter_state: Option<serde_json::Value>,
    #[serde(default)]
    pub parameters: Vec<QueryParameter>,
    #[serde(default)]
    pub builder: Option<serde_json::Value>,
    /// Set for an action query: `sql` changes rows of `action.table` instead of
    /// reading (`queries::action`, docs/decisions/action-queries.md). Config version 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ActionSpec>,
}

/// What an action query changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionSpec {
    pub kind: ActionKind,
    pub table: String,
}

/// `insert`, `update` and `delete` run `sql` as that statement. `replace` runs
/// `sql` as a SELECT whose rows replace every row of the table (Access make-table).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ActionKind {
    Insert,
    Update,
    Delete,
    Replace,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QueryParameter {
    pub name: String,
    pub logical_type: String,
    #[serde(default)]
    pub default_value: Option<serde_json::Value>,
    /// A required parameter with no default must be supplied at run time.
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseInfo {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub min_runtime_version: Option<String>,
}

impl DocumentConfig {
    /// Brings an older config up to `CONFIG_VERSION`; newer versions are rejected.
    pub fn upgrade(mut self) -> Result<Self, ArchiveError> {
        if self.version > CONFIG_VERSION {
            return Err(ArchiveError::NewerConfig(self.version));
        }
        self.version = CONFIG_VERSION;
        Ok(self)
    }
}

impl Default for DocumentConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            name: "Untitled".into(),
            active_mode: "data".into(),
            navigation_state: serde_json::json!({}),
            settings: serde_json::json!({}),
            saved_queries: vec![],
            design: DesignSchema::default(),
            reports: vec![],
            dashboards: vec![],
            actions: vec![],
            triggers: vec![],
            migrations: vec![],
            datasource: recordstore::DatasourceConfig::default(),
            entities: vec![],
            roles: vec![],
            release: ReleaseInfo::default(),
            file_sources: vec![],
            cloud: None,
            extra: serde_json::Map::new(),
        }
    }
}

pub fn document_config_yaml(config: &DocumentConfig) -> Result<String, ArchiveError> {
    serde_yaml::to_string(config).map_err(|e| ArchiveError::Invalid(e.to_string()))
}

pub fn document_config_from_yaml(yaml: &str) -> Result<DocumentConfig, ArchiveError> {
    let config = serde_yaml::from_str::<DocumentConfig>(yaml)
        .map_err(|e| ArchiveError::Invalid(e.to_string()))?
        .upgrade()?;
    config.design.validate().map_err(ArchiveError::Invalid)?;
    Ok(config)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub display_name: String,
    pub media_type: String,
    pub checksum: String,
    pub size: u64,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip)]
    pub contents: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveMetadata {
    pub document_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub application_version: String,
}

#[derive(Debug)]
pub struct ArchiveDocument {
    pub metadata: ArchiveMetadata,
    pub data: Vec<u8>,
    pub config: DocumentConfig,
    pub attachments: Vec<Attachment>,
}

pub fn empty_data_db() -> Result<Vec<u8>, ArchiveError> {
    let path = std::env::temp_dir().join(format!("ixtable-empty-{}.db", Uuid::new_v4()));
    Connection::open(&path)?.execute_batch("PRAGMA user_version=1;")?;
    let bytes = fs::read(&path)?;
    let _ = fs::remove_file(path);
    Ok(bytes)
}

pub fn create_document(name: impl Into<String>) -> Result<ArchiveDocument, ArchiveError> {
    let now = Utc::now().to_rfc3339();
    Ok(ArchiveDocument {
        metadata: ArchiveMetadata {
            document_id: Uuid::new_v4().to_string(),
            created_at: now.clone(),
            updated_at: now,
            application_version: env!("CARGO_PKG_VERSION").into(),
        },
        data: empty_data_db()?,
        config: DocumentConfig {
            name: name.into(),
            ..Default::default()
        },
        attachments: vec![],
    })
}

pub fn add_attachment(
    doc: &mut ArchiveDocument,
    display_name: String,
    media_type: String,
    contents: Vec<u8>,
) -> String {
    let id = Uuid::now_v7().to_string();
    let now = Utc::now().to_rfc3339();
    doc.attachments.push(Attachment {
        id: id.clone(),
        display_name,
        media_type,
        checksum: crate::archive_io::sha256_hex(&contents),
        size: contents.len() as u64,
        created_at: now.clone(),
        updated_at: now,
        contents,
    });
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::design::{Control, ControlKind, Placement, Validation};

    #[test]
    fn design_survives_archive_round_trip() {
        let path = std::env::temp_dir().join(format!("design-round-trip-{}.ixt", Uuid::new_v4()));
        let mut document = create_document("Designed").unwrap();
        document.config.design.forms[0].controls.push(Control {
            id: "name".into(),
            kind: ControlKind::Text,
            label: "Name".into(),
            binding: None,
            validation: Validation {
                required: true,
                ..Default::default()
            },
            placement: Placement {
                column: 1,
                row: 1,
                column_span: 12,
                row_span: 1,
                region: None,
            },
            ..Default::default()
        });
        write_archive(&path, &document).unwrap();
        let reopened = read_archive(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(reopened.config.design, document.config.design);
    }

    #[test]
    fn newer_config_versions_are_a_compatibility_error() {
        let newer = serde_json::from_value::<DocumentConfig>(serde_json::json!({
            "version": CONFIG_VERSION + 1, "name": "Future", "activeMode": "data"
        }))
        .unwrap();
        let err = newer.upgrade().unwrap_err();
        assert!(matches!(err, ArchiveError::NewerConfig(v) if v == CONFIG_VERSION + 1));
        assert!(err.to_string().contains("newer ixtable"), "{err}");
        let app = crate::manager::AppError::from(err);
        assert_eq!(app.code, "UNSUPPORTED_VERSION");
    }

    #[test]
    fn unknown_top_level_fields_survive_load_save_and_yaml() {
        let path = std::env::temp_dir().join(format!("extra-fields-{}.ixt", Uuid::new_v4()));
        let mut document = create_document("Extra").unwrap();
        let mut json = serde_json::to_value(&document.config).unwrap();
        json["futureFeature"] = serde_json::json!({"enabled": true, "items": [1, 2]});
        document.config = serde_json::from_value(json).unwrap();
        assert!(document.config.extra.contains_key("futureFeature"));
        write_archive(&path, &document).unwrap();
        let reopened = read_archive(&path).unwrap();
        fs::remove_file(path).unwrap();
        assert_eq!(reopened.config, document.config);
        let yaml = document_config_yaml(&reopened.config).unwrap();
        assert!(yaml.contains("futureFeature"), "{yaml}");
        let from_yaml = document_config_from_yaml(&yaml).unwrap();
        assert_eq!(
            from_yaml.extra["futureFeature"],
            serde_json::json!({"enabled": true, "items": [1, 2]})
        );
        assert_eq!(from_yaml, reopened.config);
    }

    #[test]
    fn legacy_config_without_design_gets_current_default() {
        let config: DocumentConfig = serde_json::from_value(serde_json::json!({
            "version": 2, "name": "Legacy", "activeMode": "data"
        }))
        .unwrap();
        assert_eq!(config.design.version, crate::design::DESIGN_SCHEMA_VERSION);
        assert!(config.design.validate().is_ok());
    }

    #[test]
    fn version_two_config_loads_as_version_three() {
        let legacy = serde_json::json!({
            "version": 2, "name": "Legacy", "activeMode": "data",
            "savedQueries": [{"id": "q1", "name": "All", "sql": "SELECT 1"}]
        });
        let config = serde_json::from_value::<DocumentConfig>(legacy)
            .unwrap()
            .upgrade()
            .unwrap();
        assert_eq!(config.version, CONFIG_VERSION);
        assert!(config.reports.is_empty() && config.roles.is_empty());
        assert_eq!(config.datasource.kind, "sqlite");
        assert!(config.saved_queries[0].parameters.is_empty());
        let yaml = document_config_from_yaml("name: Old\nactiveMode: data\nversion: 2\n").unwrap();
        assert_eq!(yaml.version, CONFIG_VERSION);
        assert!(document_config_from_yaml("name: New\nactiveMode: data\nversion: 99\n").is_err());
    }

    #[test]
    fn yaml_round_trips_feature_fields() {
        let mut config = DocumentConfig::default();
        config.saved_queries.push(SavedQuery {
            id: "q1".into(),
            name: "By customer".into(),
            sql: "SELECT 1".into(),
            parameters: vec![QueryParameter {
                name: "customer".into(),
                logical_type: "text".into(),
                default_value: Some(serde_json::json!("CUST-001")),
                ..Default::default()
            }],
            builder: Some(serde_json::json!({"source": "Customers"})),
            ..Default::default()
        });
        config.reports.push(crate::reports::Report {
            id: "r1".into(),
            name: "Sales".into(),
            ..Default::default()
        });
        config.dashboards.push(crate::dashboards::Dashboard {
            id: "d1".into(),
            name: "Overview".into(),
            ..Default::default()
        });
        config.actions.push(crate::automation::ActionDef {
            id: "a1".into(),
            name: "Approve".into(),
            ..Default::default()
        });
        config.triggers.push(crate::automation::Trigger {
            id: "t1".into(),
            name: "On create".into(),
            ..Default::default()
        });
        config.migrations.push(crate::migrations::Migration {
            id: "m1".into(),
            name: "Add index".into(),
            ..Default::default()
        });
        config.entities.push(crate::recordstore::EntitySettings {
            id: "e1".into(),
            table: "Orders".into(),
            ..Default::default()
        });
        config.roles.push(crate::roles::Role {
            id: "role1".into(),
            name: "Clerk".into(),
            ..Default::default()
        });
        config.release.version = "1.2.0".into();
        config.release.min_runtime_version = Some("1.0.0".into());
        let yaml = document_config_yaml(&config).unwrap();
        assert!(yaml.contains("minRuntimeVersion"));
        assert_eq!(document_config_from_yaml(&yaml).unwrap(), config);
    }

    #[test]
    fn validate_config_reports_duplicate_ids_and_design_errors() {
        let mut config = DocumentConfig::default();
        for name in ["One", ""] {
            config.reports.push(crate::reports::Report {
                id: "same".into(),
                name: name.into(),
                ..Default::default()
            });
        }
        config.design.version += 1;
        let issues = validate_config(&config);
        assert!(issues
            .iter()
            .any(|i| i.object_kind == "design" && i.severity == Severity::Error));
        assert!(issues
            .iter()
            .any(|i| i.object_kind == "report" && i.message.contains("duplicate")));
        assert!(issues
            .iter()
            .any(|i| i.object_kind == "report" && i.severity == Severity::Warning));
        let json = serde_json::to_value(&issues[0]).unwrap();
        assert_eq!(json["severity"], "error");
        assert!(json.get("objectKind").is_some());
    }

    #[test]
    fn yaml_round_trips_document_config() {
        let config = DocumentConfig::default();
        let yaml = document_config_yaml(&config).unwrap();
        let parsed = document_config_from_yaml(&yaml).unwrap();
        assert_eq!(parsed, config);
        let loaded =
            document_config_from_yaml("name: From YAML\nactiveMode: data\nversion: 2\n").unwrap();
        assert_eq!(loaded.name, "From YAML");
        assert!(loaded.design.validate().is_ok());
    }
}
