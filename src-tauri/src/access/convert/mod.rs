//! Access database → new ixtable document (`docs/decisions/access-import.md`).
//!
//! 1. plan the SQLite tables (`schema`) and stage every row in a scratch file
//!    (`staging`), dropping constraints the data breaks;
//! 2. convert queries, forms, reports and macros into definitions;
//! 3. start a new untitled document, store the definitions, apply the schema
//!    migration, and copy the staged rows in, as templates do.
pub mod actions;
pub mod autoforms;
pub mod controls;
pub mod forms;
pub mod layout;
pub mod macros;
pub mod navigation;
pub mod queries;
pub mod report;
pub mod reports;
pub mod schema;
pub mod sources;
pub mod staging;

use crate::access::model::{AccessFile, SourceFormat};
use crate::archive::DocumentConfig;
use crate::manager::{AppError, DocumentManager, SessionState};
use report::{ImportReport, Status};
use schema::{DbSchema, TablePlan, TableRole};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Options {
    /// Copy the rows (false: definitions and empty tables only).
    #[serde(default = "yes")]
    pub include_data: bool,
}

fn yes() -> bool {
    true
}

impl Default for Options {
    fn default() -> Self {
        Self { include_data: true }
    }
}

/// Everything the import produces before a document exists.
pub struct Conversion {
    pub config: Value,
    pub plans: Vec<TablePlan>,
    pub report: ImportReport,
    pub assets: Vec<forms::AssetRef>,
    pub staging: Option<staging::Staging>,
    /// Make-table queries whose target the import creates: (query id, table).
    pub make_tables: Vec<(String, String)>,
}

fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Notes keyed by Access object name → report items.
fn record_table_notes(
    report: &mut ImportReport,
    plans: &[TablePlan],
    notes: &BTreeMap<String, Vec<String>>,
    rows: &BTreeMap<String, u64>,
) {
    for p in plans {
        let mut n = notes.get(&p.access).cloned().unwrap_or_default();
        let (kind, name) = match &p.role {
            TableRole::Main => ("table", p.access.clone()),
            TableRole::Attachments(t, c) => {
                n.insert(
                    0,
                    format!("attachment field {c} of {t}: files are rows of {}", p.name),
                );
                ("table", p.access.clone())
            }
            TableRole::Values(t, c) => {
                n.insert(
                    0,
                    format!(
                        "multi-value field {c} of {t}: values are rows of {}",
                        p.name
                    ),
                );
                ("table", p.access.clone())
            }
        };
        let count = rows.get(&p.name).copied();
        if let Some(c) = count {
            report.rows += c;
        }
        let status = if n
            .iter()
            .any(|x| !x.starts_with("attachment field") && !x.starts_with("multi-value field"))
        {
            Status::Partial
        } else {
            Status::Converted
        };
        report.add(kind, &name, status, n);
    }
    for (k, n) in notes.iter().filter(|(k, _)| k.starts_with("relationship:")) {
        report.add(
            "relationship",
            k.trim_start_matches("relationship:"),
            Status::Partial,
            n.clone(),
        );
    }
}

/// The schema migration: tables, indexes, calculated-column triggers and lookup views.
fn schema_sql(plans: &[TablePlan], views: &[String]) -> String {
    let mut sql = vec![];
    for t in plans {
        sql.push(schema::create_table_sql(t));
    }
    for t in plans {
        for i in &t.indexes {
            sql.push(schema::index_sql(t, i));
        }
        sql.extend(schema::calc_triggers_sql(t));
    }
    sql.extend(views.iter().cloned());
    sql.join("\n")
}

/// Converts a whole Access file into definitions and staged rows.
pub fn convert(
    file: &mut dyn AccessFile,
    file_name: &str,
    options: &Options,
    progress: &mut dyn FnMut(&str, u64),
) -> Result<Conversion, String> {
    let db = file.db().clone();
    let mut report = ImportReport {
        warnings: db.warnings.clone(),
        ..Default::default()
    };
    let mut notes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut plans = schema::plan_tables(&db, &mut notes);
    schema::plan_relationships(&db, &mut plans, &mut notes);
    let (staging, rows) = if options.include_data {
        let (s, rows) = staging::load(file, &plans, &std::env::temp_dir(), progress)?;
        staging::check(&s, &mut plans, &mut notes);
        (Some(s), rows)
    } else {
        (None, BTreeMap::new())
    };
    report.tables = plans.len();
    record_table_notes(&mut report, &plans, &notes, &rows);
    let mut dbschema = DbSchema {
        db: &db,
        query_columns: BTreeMap::new(),
    };
    let queries = queries::convert(&db, &mut dbschema, &mut report);
    let mut ctx = forms::Context::new(&db, &dbschema, &plans, &queries);
    // Reports first: macros and buttons may open them.
    let reports = reports::convert_all(&mut ctx, &mut report);
    let actions = macros::convert_macros(&mut ctx, &mut report);
    let (forms, form_actions) = forms::convert_all(&mut ctx, &mut report);
    let mut saved = queries::saved_queries(&queries);
    saved.extend(ctx.extra_queries.clone());
    let views = ctx.lookup_views.clone();
    let nav = navigation::build(&ctx, &forms, &reports, &plans);
    for m in &db.modules {
        report.add("module", &m.name, Status::Skipped, vec!["VBA code does not run in ixtable; the source is kept in the document asset \"Access VBA.txt\"".into()]);
    }
    let entities: Vec<Value> = plans
        .iter()
        .map(|p| json!({ "id": id(), "table": p.name, "concurrency": "optimistic" }))
        .collect();
    let migration = json!({
        "id": id(),
        "name": format!("001 Create tables from {file_name}"),
        "order": 1,
        "targetStore": "sqlite",
        "up": schema_sql(&plans, &views),
    });
    let mut all_actions = actions;
    all_actions.extend(form_actions);
    sanitize_actions(&mut all_actions);
    let action_queries: Vec<Value> = queries
        .unconverted
        .iter()
        .map(|(n, k, s)| json!({ "name": n, "kind": k, "sql": s }))
        .collect();
    let title = db
        .props
        .get("AppTitle")
        .cloned()
        .filter(|t| !t.trim().is_empty())
        .unwrap_or_else(|| {
            std::path::Path::new(file_name)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Access import".into())
        });
    let format = match db.format {
        SourceFormat::Template => "accdt",
        SourceFormat::Jet3 => "jet3",
        SourceFormat::Jet4 => "jet4",
        SourceFormat::Ace => "ace",
    };
    let config = json!({
        "version": crate::archive::CONFIG_VERSION,
        "name": title,
        "activeMode": if nav.items.is_empty() { "data" } else { "run" },
        "navigationState": {},
        "settings": {
            "description": format!("Imported from the Access file {file_name}."),
            "accessImport": { "source": file_name, "format": format, "actionQueries": action_queries },
        },
        "savedQueries": saved,
        "design": { "version": 3, "forms": forms, "navigation": nav.items, "startPage": nav.start_page },
        "reports": reports,
        "dashboards": [],
        "actions": all_actions,
        "triggers": [],
        "migrations": [migration],
        "datasource": { "kind": "sqlite", "id": id() },
        "entities": entities,
        "roles": [],
        "release": { "version": "1.0.0", "notes": format!("Imported from {file_name}.") },
    });
    let mut config = config;
    if vba_text(&*file).is_some() {
        // Referenced from the config so the asset is never an orphan.
        config["settings"]["accessImport"]["vbaAsset"] = json!(format!("{{{{asset:{VBA_KEY}}}}}"));
    }
    let assets = ctx.assets.clone();
    let make_tables = queries
        .queries
        .values()
        .filter(|q| q.creates_table)
        .filter_map(|q| {
            q.action
                .as_ref()
                .map(|(_, table)| (q.id.clone(), table.clone()))
        })
        .collect();
    Ok(Conversion {
        config,
        plans,
        report,
        assets,
        staging,
        make_tables,
    })
}

/// Drops `runAction` steps whose action was not converted.
fn sanitize_actions(actions: &mut [Value]) {
    let ids: Vec<String> = actions
        .iter()
        .filter_map(|a| a["id"].as_str().map(str::to_string))
        .collect();
    fn clean(steps: &mut Value, ids: &[String]) {
        if let Some(list) = steps.as_array_mut() {
            list.retain(|s| {
                s["kind"] != "runAction"
                    || s["actionId"]
                        .as_str()
                        .is_some_and(|i| ids.iter().any(|x| x == i))
            });
            for s in list.iter_mut() {
                clean(&mut s["then"], ids);
                clean(&mut s["else"], ids);
            }
        }
    }
    for a in actions.iter_mut() {
        clean(&mut a["steps"], &ids);
    }
}

/// Asset key of the VBA source text.
const VBA_KEY: &str = "access-vba";

/// Replaces `{{asset:<key>}}` placeholders with imported asset ids.
fn resolve_assets(config: &Value, ids: &BTreeMap<String, String>) -> Value {
    let mut text = config.to_string();
    for (key, id) in ids {
        text = text.replace(&format!("{{{{asset:{key}}}}}"), id);
    }
    serde_json::from_str(&text).unwrap_or_else(|_| config.clone())
}

fn import_assets(
    m: &DocumentManager,
    window: &str,
    assets: &[forms::AssetRef],
    vba: Option<String>,
) -> Result<BTreeMap<String, String>, AppError> {
    let dir = std::env::temp_dir().join(format!("ixtable-access-assets-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).map_err(|e| AppError::new("IO_ERROR", e))?;
    let mut ids = BTreeMap::new();
    let result = (|| {
        for a in assets {
            let path = dir.join(&a.file_name);
            std::fs::write(&path, &a.data).map_err(|e| AppError::new("IO_ERROR", e))?;
            let imported = m.import_asset(window, &path, Some(a.media_type))?;
            ids.insert(a.key.clone(), imported.asset.id);
        }
        if let Some(code) = vba {
            let path = dir.join("Access VBA.txt");
            std::fs::write(&path, code).map_err(|e| AppError::new("IO_ERROR", e))?;
            let imported = m.import_asset(window, &path, Some("text/plain"))?;
            ids.insert(VBA_KEY.to_string(), imported.asset.id);
        }
        Ok(())
    })();
    let _ = std::fs::remove_dir_all(&dir);
    result.map(|_| ids)
}

/// VBA modules and form/report code-behind as one text file.
fn vba_text(file: &dyn AccessFile) -> Option<String> {
    let db = file.db();
    let mut parts = vec![];
    for m in &db.modules {
        parts.push(format!("' ==== Module {} ====\n{}", m.name, m.source));
    }
    for (kind, objs) in [("Form", &db.forms), ("Report", &db.reports)] {
        for o in objs.iter() {
            if let Some(code) = &o.root.code {
                parts.push(format!("' ==== {kind} {} ====\n{code}", o.name));
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// Creates the new document from a conversion. Closes the session on failure.
pub fn create(
    m: &DocumentManager,
    window: &str,
    file: &dyn AccessFile,
    conversion: Conversion,
) -> Result<(SessionState, ImportReport), AppError> {
    let Conversion {
        config,
        plans,
        mut report,
        assets,
        staging,
        make_tables,
    } = conversion;
    m.new_session(window)?;
    let result = (|| {
        let ids = import_assets(m, window, &assets, vba_text(file))?;
        let config: DocumentConfig = serde_json::from_value(resolve_assets(&config, &ids))
            .map_err(|e| {
                AppError::new(
                    "IMPORT_FAILED",
                    format!("the converted definitions are invalid: {e}"),
                )
            })?;
        m.update_config(window, config.clone())?;
        let db = m.database_path(window)?;
        crate::migrations::apply_sqlite(&db, &config.migrations)
            .map_err(|e| AppError::new("MIGRATION_FAILED", e))?;
        if let Some(s) = &staging {
            staging::copy_into(s, &db, &plans).map_err(|e| AppError::new("IMPORT_FAILED", e))?;
        }
        m.mark_data_dirty(window)?;
        let mut config = config;
        actions::create_targets(m, window, &mut config, &make_tables, &mut report)?;
        check_queries(m, window, &config, &mut report);
        actions::check_statements(m, window, &config, &mut report)?;
        // The report stays with the document (Settings › YAML shows it).
        config.settings["accessImport"]["report"] =
            serde_json::to_value(&report.items).unwrap_or_default();
        m.update_config(window, config)
    })();
    match result {
        Ok(state) => {
            crate::logging::info(
                "access",
                &format!("imported {} objects", report.items.len()),
            );
            Ok((state, report))
        }
        Err(e) => {
            crate::logging::warn("access", &format!("import failed: {e}"));
            let _ = m.close(window, true);
            Err(e)
        }
    }
}

/// Runs DuckDB's binder over each saved query; failures go into the report.
fn check_queries(
    m: &DocumentManager,
    window: &str,
    config: &DocumentConfig,
    report: &mut ImportReport,
) {
    // Insert, update and delete statements are checked by `actions::check_statements`.
    let reads = config.saved_queries.iter().filter(|q| {
        q.action
            .as_ref()
            .is_none_or(|a| a.kind == crate::archive::ActionKind::Replace)
    });
    for q in reads {
        let sql = actions::with_null_params(q);
        if let Err(e) = m.read_query(window, &format!("DESCRIBE {sql}")) {
            report.note(
                "query",
                &q.name,
                format!("DuckDB rejects the converted SQL: {}", e.message),
            );
        }
    }
}
