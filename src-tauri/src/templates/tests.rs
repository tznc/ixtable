use super::*;
use crate::archive::{validate_config, Severity};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

/// One manager for every template test, in its own temporary state directory.
fn manager() -> &'static DocumentManager {
    static M: OnceLock<(PathBuf, DocumentManager)> = OnceLock::new();
    &M.get_or_init(|| {
        let base = std::env::temp_dir().join(format!("ixtable-templates-{}", uuid::Uuid::new_v4()));
        let m = DocumentManager::new(base.join("data"), base.join("cache")).unwrap();
        (base, m)
    })
    .1
}

fn errors(issues: &[crate::archive::Issue]) -> Vec<String> {
    issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| format!("{} {}: {}", i.object_kind, i.object_id, i.message))
        .collect()
}

fn count(m: &DocumentManager, window: &str, sql: &str) -> i64 {
    let result = m.read_query(window, sql).unwrap();
    match &result.rows[0][0] {
        crate::data::DataValue::Integer(n) => *n,
        other => panic!("{sql}: expected an integer, got {other:?}"),
    }
}

/// Creates the template in a fresh window and checks validation, migrations, and seed counts.
fn create_and_check(id: &str, window: &str, expected: &[(&str, i64)]) -> DocumentConfig {
    let m = manager();
    let state = create(m, window, id).unwrap_or_else(|e| panic!("{id}: {e}"));
    assert_eq!(state.path, None, "templates create untitled documents");
    assert_eq!(state.active_mode, "run");
    let config = m.config(window).unwrap();
    assert_eq!(errors(&validate_config(&config)), Vec::<String>::new());
    let mut tables = HashMap::new();
    for object in m.database_objects(window).unwrap() {
        let schema = m.table_schema(window, &object.name).unwrap();
        tables.insert(
            object.name,
            schema
                .columns
                .into_iter()
                .map(|c| c.name)
                .collect::<Vec<_>>(),
        );
    }
    assert_eq!(
        errors(&crate::design::validate_tables(&config, &tables)),
        Vec::<String>::new()
    );
    let names: Vec<String> = tables.keys().cloned().collect();
    assert_eq!(
        crate::recordstore::validate_tables(&config, &names),
        vec![],
        "every table has a concurrency policy"
    );
    for (table, rows) in expected {
        assert_eq!(
            count(m, window, &format!("SELECT count(*) FROM {table}")),
            *rows,
            "{id}: rows in {table}"
        );
    }
    let db = m.database_path(window).unwrap();
    let applied: Vec<String> = rusqlite::Connection::open(&db)
        .unwrap()
        .prepare("SELECT id FROM _ixtable_migrations ORDER BY applied_at")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut declared: Vec<&Migration> = config.migrations.iter().collect();
    declared.sort_by_key(|m| m.order);
    assert_eq!(
        applied,
        declared.iter().map(|m| m.id.clone()).collect::<Vec<_>>()
    );
    assert!(crate::migrations::pending(&db, &config.migrations)
        .unwrap()
        .is_empty());
    m.close(window, true).unwrap();
    config
}

#[test]
fn every_template_parses_through_the_yaml_path() {
    for t in TEMPLATES {
        let config = template_config(t.id).unwrap_or_else(|e| panic!("{}: {e}", t.id));
        let yaml = archive::document_config_yaml(&config).unwrap();
        assert_eq!(archive::document_config_from_yaml(&yaml).unwrap(), config);
        let seed = config.migrations.last().unwrap();
        assert_eq!(seed.order, SEED_ORDER);
        for statement in t.seed.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            let sql: String = statement
                .lines()
                .filter(|l| !l.trim_start().starts_with("--"))
                .collect::<Vec<_>>()
                .join("\n");
            assert!(
                sql.trim_start().to_uppercase().starts_with("INSERT INTO"),
                "{}: seed.sql may only insert rows: {sql}",
                t.id
            );
        }
    }
}

/// Keys in `raw` that the parsed config dropped: serde ignores unknown fields, so a
/// misspelled key (or a YAML flow mapping split by an unquoted comma) would vanish.
fn dropped(raw: &serde_json::Value, parsed: &serde_json::Value, path: &str, out: &mut Vec<String>) {
    use serde_json::Value::{Array, Object};
    match (raw, parsed) {
        (Object(a), Object(b)) => {
            for (key, value) in a {
                match b.get(key) {
                    Some(other) => dropped(value, other, &format!("{path}.{key}"), out),
                    None => out.push(format!("{path}.{key}")),
                }
            }
        }
        (Array(a), Array(b)) => {
            for (i, value) in a.iter().enumerate() {
                match b.get(i) {
                    Some(other) => dropped(value, other, &format!("{path}[{i}]"), out),
                    None => out.push(format!("{path}[{i}]")),
                }
            }
        }
        (a, b) if a.as_f64().is_some() && b.as_f64().is_some() => {
            if a.as_f64() != b.as_f64() {
                out.push(format!("{path}: {a} became {b}"));
            }
        }
        (a, b) if a != b => out.push(format!("{path}: {a} became {b}")),
        _ => {}
    }
}

#[test]
fn template_yaml_has_no_keys_the_config_ignores() {
    for t in TEMPLATES {
        let raw: serde_json::Value = serde_yaml::from_str(t.yaml).unwrap();
        let parsed = serde_json::to_value(template_config(t.id).unwrap()).unwrap();
        let mut out = vec![];
        dropped(&raw, &parsed, "", &mut out);
        assert_eq!(out, Vec::<String>::new(), "{}", t.id);
    }
}

#[test]
fn listed_templates_have_names_descriptions_and_versions() {
    let listed = list_templates().unwrap();
    assert!(listed.iter().any(|t| t.id == "crm"));
    for t in &listed {
        assert!(!t.name.is_empty() && !t.description.is_empty() && !t.version.is_empty());
    }
}

#[test]
fn unknown_template_is_not_found() {
    assert_eq!(template_config("nope").unwrap_err().code, "NOT_FOUND");
}

#[test]
fn crm_template_creates_a_seeded_valid_document() {
    create_and_check(
        "crm",
        "templates-crm",
        &[
            ("deal_stages", 6),
            ("companies", 5),
            ("contacts", 8),
            ("deals", 10),
            ("activities", 12),
        ],
    );
}

#[test]
fn template_definitions_have_no_validation_warnings() {
    for t in TEMPLATES {
        let config = template_config(t.id).unwrap();
        let issues: Vec<String> = validate_config(&config)
            .iter()
            .map(|i| format!("{} {}: {}", i.object_kind, i.object_id, i.message))
            .collect();
        assert_eq!(issues, Vec::<String>::new(), "{}", t.id);
    }
}

#[test]
fn inventory_template_carries_its_asset_and_resolves_placeholders() {
    let config = create_and_check(
        "inventory",
        "templates-inventory",
        &[
            ("suppliers", 3),
            ("locations", 3),
            ("products", 6),
            ("stock_movements", 18),
            ("reorder_thresholds", 7),
            ("transfers", 1),
        ],
    );
    let json = serde_json::to_string(&config).unwrap();
    assert!(
        !json.contains("{{asset:"),
        "asset placeholders are replaced"
    );
    let image = config
        .design
        .forms
        .iter()
        .flat_map(|f| &f.controls)
        .find(|c| c.asset_id.is_some());
    let id = image.and_then(|c| c.asset_id.clone()).unwrap();
    assert_eq!(id.len(), 36, "a real asset id");
    assert!(
        json.matches(&id).count() >= 2,
        "form and report share the asset"
    );
}

#[test]
fn inventory_has_composite_key_and_entity_policies() {
    let config = template_config("inventory").unwrap();
    let policy = |table: &str| {
        crate::recordstore::entity_policy(&config, table)
            .map(|e| e.concurrency.clone())
            .unwrap()
    };
    assert_eq!(policy("products"), "optimistic");
    assert_eq!(policy("stock_movements"), "lastWriteWins");
    assert!(config.migrations[0]
        .up
        .contains("PRIMARY KEY (product_id, location_id)"));
}

#[test]
fn work_order_templates_create_seeded_valid_documents() {
    let expected = [
        ("statuses", 4),
        ("priorities", 4),
        ("assignees", 3),
        ("assets", 4),
        ("work_orders", 5),
        ("tasks", 11),
        ("work_order_log", 5),
    ];
    create_and_check("work-orders", "templates-wo1", &expected);
    let v2 = create_and_check("work-orders@2", "templates-wo2", &expected);
    assert!(v2
        .migrations
        .iter()
        .any(|m| m.up.contains("ADD COLUMN due_on")));
}

/// Id of work-orders v2's "002 Add work-order due dates" migration.
fn due_dates_migration(v2: &DocumentConfig) -> &str {
    v2.migrations
        .iter()
        .find(|m| m.name == "002 Add work-order due dates")
        .map(|m| m.id.as_str())
        .unwrap()
}

#[test]
fn work_orders_v2_extends_v1_without_changing_applied_migrations() {
    let v1 = template_config("work-orders").unwrap();
    let v2 = template_config("work-orders@2").unwrap();
    assert_eq!(v1.release.version, "1.0.0");
    assert_eq!(v2.release.version, "2.0.0");
    for m in &v1.migrations {
        let same = v2.migrations.iter().find(|x| x.id == m.id).unwrap();
        assert_eq!(same.checksum(), m.checksum(), "{} is immutable", m.id);
        assert_eq!(same.order, m.order);
    }
    let added: Vec<&str> = v2
        .migrations
        .iter()
        .filter(|m| !v1.migrations.iter().any(|x| x.id == m.id))
        .map(|m| m.id.as_str())
        .collect();
    assert_eq!(added, [due_dates_migration(&v2)]);
    // Every v1 object keeps its id in v2 so references and permissions survive the upgrade.
    let forms = |c: &DocumentConfig| {
        c.design
            .forms
            .iter()
            .map(|f| f.id.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(forms(&v1), forms(&v2));
    let ids = |c: &DocumentConfig| {
        let mut ids: Vec<String> = c.saved_queries.iter().map(|q| q.id.clone()).collect();
        ids.extend(c.actions.iter().map(|a| a.id.clone()));
        ids.extend(c.triggers.iter().map(|t| t.id.clone()));
        ids.extend(c.reports.iter().map(|r| r.id.clone()));
        ids.extend(c.dashboards.iter().map(|d| d.id.clone()));
        ids.extend(c.roles.iter().map(|r| r.id.clone()));
        ids
    };
    assert_eq!(ids(&v1), ids(&v2));
}

#[test]
fn upgrading_a_v1_database_applies_only_migration_002() {
    let v1 = template_config("work-orders").unwrap();
    let v2 = template_config("work-orders@2").unwrap();
    let db = std::env::temp_dir().join(format!("ixtable-wo-upgrade-{}.db", uuid::Uuid::new_v4()));
    let conn = rusqlite::Connection::open(&db).unwrap();
    crate::migrations::apply_sqlite(&db, &v1.migrations).unwrap();
    conn.execute(
        "INSERT INTO work_orders (number, asset_id, title) VALUES ('WO-2001', 2, 'Runtime record')",
        [],
    )
    .unwrap();
    let pending = crate::migrations::pending(&db, &v2.migrations).unwrap();
    assert_eq!(
        pending.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        [due_dates_migration(&v2)]
    );
    let logs = crate::migrations::apply_sqlite(&db, &v2.migrations).unwrap();
    assert_eq!(logs.len(), 1);
    let (title, due): (String, Option<String>) = conn
        .query_row(
            "SELECT title, due_on FROM work_orders WHERE number = 'WO-2001'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((title.as_str(), due), ("Runtime record", None));
    drop(conn);
    let _ = std::fs::remove_file(db);
}

/// Collects every `id` that names a definition object. Skipped: record matches and
/// write values (`match.id` is a column), and role permissions on tables (`id` is the
/// table name).
fn definition_ids(value: &serde_json::Value, path: &str, out: &mut Vec<(String, String)>) {
    use serde_json::Value::{Array, Object, String as Str};
    match value {
        Object(map) => {
            let table_permission = path.contains(".permissions.objects")
                && map.get("kind").and_then(|k| k.as_str()) == Some("table");
            for (key, child) in map {
                match (key.as_str(), child) {
                    ("match" | "values" | "params" | "settings", _) => {}
                    ("id", Str(id)) if !table_permission => out.push((path.into(), id.clone())),
                    _ => definition_ids(child, &format!("{path}.{key}"), out),
                }
            }
        }
        Array(items) => {
            for (i, item) in items.iter().enumerate() {
                definition_ids(item, &format!("{path}[{i}]"), out);
            }
        }
        _ => {}
    }
}

#[test]
fn every_template_id_is_a_uuid_v7() {
    for t in TEMPLATES {
        let config = serde_json::to_value(template_config(t.id).unwrap()).unwrap();
        let mut ids = vec![];
        definition_ids(&config, "", &mut ids);
        assert!(ids.len() > 50, "{}: found only {} ids", t.id, ids.len());
        for (path, id) in ids {
            let parsed = uuid::Uuid::parse_str(&id)
                .unwrap_or_else(|_| panic!("{} {path}: {id} is not a UUID", t.id));
            assert_eq!(parsed.get_version_num(), 7, "{} {path}: {id}", t.id);
            assert_eq!(
                parsed.get_variant(),
                uuid::Variant::RFC4122,
                "{} {path}: {id}",
                t.id
            );
            assert_eq!(
                parsed.hyphenated().to_string(),
                id,
                "{} {path}: lowercase hyphenated",
                t.id
            );
        }
    }
}

// Port of the "golden template files" and boolean-control cases of tests/unit/golden-templates.test.tsx.

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("golden")
}

#[test]
fn every_asset_placeholder_names_a_file_the_template_carries() {
    for app in ["crm", "inventory", "work-orders", "work-orders/v2"] {
        let yaml = std::fs::read_to_string(golden_dir().join(app).join("app.yaml")).expect("app.yaml");
        let assets = golden_dir().join(app.split('/').next().unwrap()).join("assets");
        let files: Vec<String> = std::fs::read_dir(&assets)
            .map(|dir| dir.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default();
        let definitions: String = yaml.lines().filter(|l| !l.trim().starts_with('#')).collect::<Vec<_>>().join("\n");
        let mut rest = definitions.as_str();
        while let Some(start) = rest.find("{{asset:") {
            let after = &rest[start + "{{asset:".len()..];
            let end = after.find("}}").expect("closing braces");
            assert!(files.iter().any(|f| f == &after[..end]), "{app}: {} is not in {files:?}", &after[..end]);
            rest = &after[end..];
        }
    }
}

#[test]
fn seed_sql_only_inserts_rows() {
    for app in ["crm", "inventory", "work-orders"] {
        let text = std::fs::read_to_string(golden_dir().join(app).join("seed.sql")).expect("seed.sql");
        let sql: String = text.lines().filter(|l| !l.trim().starts_with("--")).collect::<Vec<_>>().join("\n");
        let statements: Vec<&str> = sql.split(';').map(str::trim).filter(|s| !s.is_empty()).collect();
        assert!(statements.len() > 3, "{app}");
        for statement in statements {
            let ok = statement.strip_prefix("INSERT INTO ").is_some_and(|rest| {
                let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                !name.is_empty() && rest[name.len()..].starts_with(" (")
            });
            assert!(ok, "{app}: {statement}");
        }
    }
}

#[test]
fn boolean_controls_bound_to_integer_columns_write_integers_not_the_text_true() {
    use crate::data::DataValue;
    use crate::forms::values::to_column_value;
    use serde_json::json;
    assert_eq!(to_column_value(&json!(true), "INTEGER"), DataValue::Integer(1));
    assert_eq!(to_column_value(&json!(false), "INTEGER"), DataValue::Integer(0));
    assert_eq!(to_column_value(&json!(true), "BOOLEAN"), DataValue::Boolean(true));
}
