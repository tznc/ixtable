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

use crate::access::model::{AccessDb, AccessFile, SourceFormat};
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
    /// Set by the user to stop the import; checked between rows and between steps.
    #[serde(skip)]
    pub cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

/// The error text of a cancelled import ([`crate::access::import_access_file`] maps it to
/// the code `IMPORT_CANCELLED`).
pub const CANCELLED: &str = "the import was cancelled";

impl Options {
    pub fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
    }
}

/// "about 2 minutes left" from the rows copied so far, or None while too early to tell.
pub fn time_left(done: u64, total: u64, elapsed: std::time::Duration) -> Option<String> {
    if done == 0 || total <= done || elapsed.as_secs_f64() < 2.0 {
        return None;
    }
    let secs = elapsed.as_secs_f64() / done as f64 * (total - done) as f64;
    let plural = |n: u64, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    Some(if secs < 10.0 {
        "a few seconds left".into()
    } else if secs < 60.0 {
        format!("about {} left", plural(((secs / 10.0).round() * 10.0) as u64, "second"))
    } else if secs < 3600.0 {
        format!("about {} left", plural((secs / 60.0).round() as u64, "minute"))
    } else {
        let minutes = (secs / 60.0).round() as u64;
        format!("about {} {} left", plural(minutes / 60, "hour"), plural(minutes % 60, "minute"))
    })
}

fn yes() -> bool {
    true
}

impl Default for Options {
    fn default() -> Self {
        Self { include_data: true, cancel: None }
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

/// runAction targets of an action's steps, at any depth.
fn run_targets(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if o.get("kind").and_then(Value::as_str) == Some("runAction") {
                if let Some(id) = o.get("actionId").and_then(Value::as_str) {
                    out.push(id.to_string());
                }
            }
            o.values().for_each(|x| run_targets(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| run_targets(x, out)),
        _ => {}
    }
}

fn drop_run_steps(v: &mut Value, target: &str) {
    match v {
        Value::Array(a) => {
            a.retain(|x| {
                !(x.get("kind").and_then(Value::as_str) == Some("runAction")
                    && x.get("actionId").and_then(Value::as_str) == Some(target))
            });
            a.iter_mut().for_each(|x| drop_run_steps(x, target));
        }
        Value::Object(o) => o.values_mut().for_each(|x| drop_run_steps(x, target)),
        _ => {}
    }
}

/// Access macros may call each other in a loop; ixtable refuses such actions. Cuts the
/// call that closes each loop: (macro name, called macro name).
fn break_run_cycles(actions: &mut [Value]) -> Vec<(String, String)> {
    let id_of = |a: &Value| a.get("id").and_then(Value::as_str).unwrap_or("").to_string();
    let name_of = |actions: &[Value], id: &str| {
        actions
            .iter()
            .find(|a| id_of(a) == id)
            .and_then(|a| a.get("name").and_then(Value::as_str))
            .unwrap_or(id)
            .to_string()
    };
    let mut cut = vec![];
    loop {
        let edges: BTreeMap<String, Vec<String>> = actions
            .iter()
            .map(|a| {
                let mut t = vec![];
                run_targets(a.get("steps").unwrap_or(&Value::Null), &mut t);
                (id_of(a), t)
            })
            .collect();
        let reaches = |from: &str, to: &str| {
            let mut stack = vec![from.to_string()];
            let mut seen = std::collections::BTreeSet::new();
            while let Some(n) = stack.pop() {
                if n == to {
                    return true;
                }
                if seen.insert(n.clone()) {
                    stack.extend(edges.get(&n).cloned().unwrap_or_default());
                }
            }
            false
        };
        let closing = edges
            .iter()
            .find_map(|(a, ts)| ts.iter().find(|b| reaches(b, a)).map(|b| (a.clone(), b.clone())));
        let Some((a, b)) = closing else { break };
        cut.push((name_of(actions, &a), name_of(actions, &b)));
        if let Some(action) = actions.iter_mut().find(|x| id_of(x) == a) {
            if let Some(steps) = action.get_mut("steps") {
                drop_run_steps(steps, &b);
            }
        }
    }
    cut
}

/// Access names are case-insensitive, but a template can hold parts whose names differ
/// only by case. The later ones get a number so neither is lost: (kind, old, new).
fn rename_case_clashes(db: &mut AccessDb) -> Vec<(String, String, String)> {
    fn dedupe(kind: &str, names: Vec<&mut String>, out: &mut Vec<(String, String, String)>) {
        let mut seen = std::collections::BTreeSet::new();
        let taken: std::collections::BTreeSet<String> = names.iter().map(|n| n.to_lowercase()).collect();
        for name in names {
            if seen.insert(name.to_lowercase()) {
                continue;
            }
            let mut n = 2;
            let new = loop {
                let candidate = format!("{name} {n}");
                if !taken.contains(&candidate.to_lowercase()) && !seen.contains(&candidate.to_lowercase()) {
                    break candidate;
                }
                n += 1;
            };
            seen.insert(new.to_lowercase());
            out.push((kind.into(), std::mem::replace(name, new.clone()), new));
        }
    }
    let mut out = vec![];
    dedupe("query", db.queries.iter_mut().map(|q| &mut q.name).collect(), &mut out);
    dedupe("form", db.forms.iter_mut().map(|f| &mut f.name).collect(), &mut out);
    dedupe("report", db.reports.iter_mut().map(|r| &mut r.name).collect(), &mut out);
    dedupe("macro", db.macros.iter_mut().map(|m| &mut m.name).collect(), &mut out);
    out
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
    let mut db = file.db().clone();
    let renames = rename_case_clashes(&mut db);
    let mut report = ImportReport {
        warnings: db.warnings.clone(),
        ..Default::default()
    };
    let mut notes: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut plans = schema::plan_tables(&db, &mut notes);
    schema::plan_relationships(&db, &mut plans, &mut notes);
    schema::mark_nocase_keys(&mut plans);
    let (staging, rows) = if options.include_data {
        let (s, rows) = staging::load(file, &plans, &std::env::temp_dir(), &|| options.cancelled(), progress)?;
        staging::check(&s, &mut plans, &mut notes);
        (Some(s), rows)
    } else {
        (None, BTreeMap::new())
    };
    report.tables = plans.len();
    for (table, note) in &db.table_notes {
        notes.entry(table.clone()).or_default().push(note.clone());
    }
    for t in &db.tables {
        let (Some(total), Some(plan)) = (t.row_count, plans.iter().find(|p| p.role == TableRole::Main && p.access == t.name)) else {
            continue;
        };
        let read = rows.get(&plan.name).copied().unwrap_or(total);
        if options.include_data && read < total {
            notes.entry(t.name.clone()).or_default().push(format!(
                "{} of {total} rows could not be read; the file may be damaged (run Compact and Repair in Access)",
                total - read
            ));
        }
    }
    record_table_notes(&mut report, &plans, &notes, &rows);
    for (name, why) in &db.unreadable_tables {
        report.add(
            "table",
            name,
            Status::Skipped,
            vec![format!("the table could not be read, so it was not imported ({why})")],
        );
    }
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
    for (owner, source, e) in &ctx.dropped_views {
        report.warnings.push(format!(
            "The lookup of {owner} reads {source}, which does not work after the import, so the lookup was dropped or simplified ({e})"
        ));
    }
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
    for (macro_name, target) in break_run_cycles(&mut all_actions) {
        report.note(
            "macro",
            &macro_name,
            format!("RunMacro {target} was left out: it runs itself again, which would never end"),
        );
    }
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
        "pages": [],
        "actions": all_actions,
        "triggers": [],
        // A file with no tables has nothing to migrate, and an empty migration is invalid.
        "migrations": if migration["up"].as_str().is_some_and(|u| u.trim().is_empty()) { vec![] } else { vec![migration] },
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
    for (kind, from, to) in &renames {
        report.note(
            kind,
            to,
            format!("renamed from {from}: another {kind} has the same name in a different case"),
        );
    }
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
        let mut config: DocumentConfig = serde_json::from_value(resolve_assets(&config, &ids))
            .map_err(|e| {
                AppError::new(
                    "IMPORT_FAILED",
                    format!("the converted definitions are invalid: {e}"),
                )
            })?;
        crate::pages::upgrade::sync(&mut config);
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
        report.link_targets(&config);
        report.warnings.extend(validation_warnings(&config));
        config.settings["accessImport"]["report"] =
            serde_json::to_value(&report.items).unwrap_or_default();
        config.settings["accessImport"]["warnings"] = serde_json::json!(report.warnings);
        config.settings["accessImport"]["importedAt"] =
            serde_json::json!(chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string());
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

/// Errors the document validator still finds after the import, as report warnings, so
/// the user meets them in the report rather than only in Problems.
pub fn validation_warnings(config: &DocumentConfig) -> Vec<String> {
    crate::archive::validate_config(config)
        .into_iter()
        .filter(|i| i.severity == crate::archive::Severity::Error)
        .map(|i| format!("The new document has a problem to fix: {}", i.message))
        .collect()
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
