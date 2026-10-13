//! Golden application templates (PRD §3.3, §26): editable starter apps that are
//! pure definition data, created only through public features.
//!
//! Each template is `golden/<app>/app.yaml` (the DocumentConfig YAML that the
//! Settings › YAML tab edits), `seed.sql` (INSERT statements only), and optional
//! application assets. Creating a document from a template:
//! 1. starts a new untitled session;
//! 2. imports the template's assets with the public asset import and replaces
//!    `{{asset:<file name>}}` placeholders in the YAML with the new asset ids;
//! 3. loads the YAML through `document_config_from_yaml` (as the YAML tab does)
//!    and appends `seed.sql` as the final migration, "Seed sample data";
//! 4. stores the config and applies the migrations through `migrations::apply_sqlite`.
//!
//! There is no template-specific runtime code: once created, the document is an
//! ordinary document.
use crate::archive::{self, DocumentConfig};
use crate::manager::{AppError, DocumentManager, SessionState};
use crate::migrations::Migration;
use serde::Serialize;
use std::fs;

/// Order of the seed migration: after every schema migration of every template version.
pub const SEED_ORDER: u32 = 1000;

/// An application asset carried by a template.
pub struct TemplateAsset {
    pub file_name: &'static str,
    pub media_type: &'static str,
    pub bytes: &'static [u8],
}

/// One embedded template version.
pub struct Template {
    pub id: &'static str,
    /// Listed on the start screen. Unlisted versions are migration fixtures.
    pub listed: bool,
    pub yaml: &'static str,
    pub seed: &'static str,
    /// Fixed UUIDv7 of the "Seed sample data" migration. Versions of one app share it,
    /// so an upgraded document does not run the seed again.
    pub seed_migration_id: &'static str,
    pub assets: &'static [TemplateAsset],
}

pub const TEMPLATES: &[Template] = &[
    Template {
        id: "crm",
        listed: true,
        yaml: include_str!("../../../golden/crm/app.yaml"),
        seed: include_str!("../../../golden/crm/seed.sql"),
        seed_migration_id: "01a100fd-dee3-745f-a8b0-457e9c978be5",
        assets: &[],
    },
    Template {
        id: "inventory",
        listed: true,
        yaml: include_str!("../../../golden/inventory/app.yaml"),
        seed: include_str!("../../../golden/inventory/seed.sql"),
        seed_migration_id: "01a10134-cd6e-7051-aa6f-36c560dbeb95",
        assets: &[TemplateAsset {
            file_name: "product-placeholder.png",
            media_type: "image/png",
            bytes: include_bytes!("../../../golden/inventory/assets/product-placeholder.png"),
        }],
    },
    Template {
        id: "work-orders",
        listed: true,
        yaml: include_str!("../../../golden/work-orders/app.yaml"),
        seed: include_str!("../../../golden/work-orders/seed.sql"),
        seed_migration_id: "01a1016b-bbfe-76e3-84a4-2b192a072b1c",
        assets: &[],
    },
    // Version 2 of the work-order app: the application-migration fixture (PRD §26.4).
    Template {
        id: "work-orders@2",
        listed: false,
        yaml: include_str!("../../../golden/work-orders/v2/app.yaml"),
        seed: include_str!("../../../golden/work-orders/seed.sql"),
        seed_migration_id: "01a1016b-bbfe-76e3-84a4-2b192a072b1c",
        assets: &[],
    },
];

/// Start-screen summary of a template.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: String,
}

fn template(id: &str) -> Result<&'static Template, AppError> {
    TEMPLATES
        .iter()
        .find(|t| t.id == id)
        .ok_or_else(|| AppError::new("NOT_FOUND", format!("Template {id} does not exist")))
}

fn invalid(id: &str, e: impl ToString) -> AppError {
    AppError::new(
        "INVALID_TEMPLATE",
        format!("Template {id}: {}", e.to_string()),
    )
}

/// The template's YAML with asset placeholders replaced (`{{asset:<file>}}` → id).
fn resolve_yaml(template: &Template, asset_ids: &[(&str, String)]) -> String {
    let mut yaml = template.yaml.to_string();
    for (file, id) in asset_ids {
        yaml = yaml.replace(&format!("{{{{asset:{file}}}}}"), id);
    }
    yaml
}

/// Parses a template's YAML (as the YAML tab does) and appends the seed migration.
fn config_from_yaml(template: &Template, yaml: &str) -> Result<DocumentConfig, AppError> {
    let mut config =
        archive::document_config_from_yaml(yaml).map_err(|e| invalid(template.id, e))?;
    if !template.seed.trim().is_empty() {
        config.migrations.push(Migration {
            id: template.seed_migration_id.into(),
            name: "Seed sample data".into(),
            order: SEED_ORDER,
            target_store: "sqlite".into(),
            up: template.seed.to_string(),
            down: None,
            reversible: false,
            depends_on: config
                .migrations
                .iter()
                .filter(|m| m.order < SEED_ORDER)
                .map(|m| m.id.clone())
                .collect(),
        });
    }
    Ok(config)
}

/// The template's config without assets (placeholders left as-is).
pub fn template_config(id: &str) -> Result<DocumentConfig, AppError> {
    let t = template(id)?;
    config_from_yaml(t, t.yaml)
}

fn info(t: &Template) -> Result<TemplateInfo, AppError> {
    let config = template_config(t.id)?;
    let description = config
        .settings
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or_default()
        .to_string();
    Ok(TemplateInfo {
        id: t.id.into(),
        name: config.name,
        description,
        version: config.release.version,
    })
}

/// Templates shown under "Start from a template".
pub fn list_templates() -> Result<Vec<TemplateInfo>, AppError> {
    TEMPLATES.iter().filter(|t| t.listed).map(info).collect()
}

/// Creates a new untitled document from a template: definitions, assets, schema, and seed.
pub fn create_from_template(
    window_label: String,
    template_id: String,
) -> Result<SessionState, AppError> {
    create(crate::manager()?, &window_label, &template_id)
}

/// [`create_from_template`] against a given manager (tests use their own state dir).
pub fn create(m: &DocumentManager, window: &str, id: &str) -> Result<SessionState, AppError> {
    let t = template(id)?;
    m.new_session(window)?;
    let result = (|| {
        let asset_ids = import_assets(m, window, t)?;
        let config = config_from_yaml(t, &resolve_yaml(t, &asset_ids))?;
        m.update_config(window, config.clone())?;
        let db = m.database_path(window)?;
        crate::migrations::apply_sqlite(&db, &config.migrations)
            .map_err(|e| AppError::new("MIGRATION_FAILED", e))?;
        // Templates are SQLite documents: refresh the DuckDB reader after the writes.
        m.mark_data_dirty(window)
    })();
    match result {
        Ok(state) => {
            crate::logging::info("templates", &format!("created document from {id}"));
            Ok(state)
        }
        Err(e) => {
            crate::logging::warn("templates", &format!("{id}: {e}"));
            let _ = m.close(window, true);
            Err(e)
        }
    }
}

/// Imports a template's assets through the public asset import; returns (file, id) pairs.
fn import_assets(
    m: &DocumentManager,
    window: &str,
    t: &Template,
) -> Result<Vec<(&'static str, String)>, AppError> {
    if t.assets.is_empty() {
        return Ok(vec![]);
    }
    let dir = std::env::temp_dir().join(format!("ixtable-template-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&dir).map_err(|e| AppError::new("IO_ERROR", e))?;
    let result = t
        .assets
        .iter()
        .map(|a| {
            let path = dir.join(a.file_name);
            fs::write(&path, a.bytes).map_err(|e| AppError::new("IO_ERROR", e))?;
            let imported = m.import_asset(window, &path, Some(a.media_type))?;
            Ok((a.file_name, imported.asset.id))
        })
        .collect();
    let _ = fs::remove_dir_all(&dir);
    result
}

#[cfg(test)]
mod tests;
