//! Helpers shared by the automation unit tests: a real session with tables, config edits
//! and row reads. The TypeScript tests ran against an in-memory fake backend; these run
//! against the real RecordStore, so constraint and conflict errors come from the database.

use super::context::{ActionContext, ActionHost, SessionEnv, TriggerEnv};
use super::host::RecordingHost;
use super::values::row_to_object;
use crate::archive::DocumentConfig;
use crate::automation::{ActionDef, Step, Trigger};
use crate::data::{DataValue, NamedValue};
use serde_json::{json, Map, Value};
use std::sync::Arc;

/// A fresh session that is closed when dropped.
pub struct Db {
    pub label: String,
}

impl Db {
    pub fn new() -> Self {
        let label = format!("automation-{}", uuid::Uuid::new_v4());
        crate::manager().unwrap().new_session(&label).expect("session");
        Self { label }
    }

    /// Creates a table: the first column is the integer primary key, the rest are text
    /// unless the name ends in `:int` or `:real`.
    pub fn table(&self, name: &str, columns: &[&str]) {
        self.table_checked(name, columns, &[]);
    }

    /// Like `table`, with CHECK constraint expressions.
    pub fn table_checked(&self, name: &str, columns: &[&str], checks: &[&str]) {
        let columns: Vec<Value> = columns
            .iter()
            .enumerate()
            .map(|(i, spec)| {
                let (column, ty) = match spec.split_once(':') {
                    Some((c, "int")) => (c, "INTEGER"),
                    Some((c, "real")) => (c, "REAL"),
                    Some((c, t)) => (c, t),
                    None if i == 0 => (*spec, "INTEGER"),
                    None => (*spec, "TEXT"),
                };
                json!({
                    "name": column, "declaredType": ty, "nullable": i != 0,
                    "primaryKeyPosition": if i == 0 { 1 } else { 0 },
                    "unique": false, "defaultExpression": null, "generatedExpression": null,
                })
            })
            .collect();
        let spec = json!({
            "name": name, "columns": columns, "foreignKeys": [], "checks": checks, "withoutRowid": false,
        });
        crate::create_database_table(self.label.clone(), serde_json::from_value(spec).unwrap())
            .expect("create table");
    }

    /// Lets updates and deletes of `table` skip the original values the default optimistic
    /// policy requires (tests that write raw rows without a form's snapshot).
    pub fn last_write_wins(&self, table: &str) {
        self.set_config(|c| {
            for entity in c.entities.iter_mut().filter(|e| e.table == table) {
                entity.concurrency = "lastWriteWins".into();
            }
        });
    }

    /// Inserts a row straight into the store (no record hooks).
    pub fn seed(&self, table: &str, values: Value) {
        let values = values
            .as_object()
            .unwrap()
            .iter()
            .map(|(column, v)| NamedValue {
                column: column.clone(),
                value: super::values::to_data_value(v),
            })
            .collect();
        crate::recordstore::commands::insert_row(self.label.clone(), table.into(), values, None)
            .expect("seed row");
    }

    pub fn config(&self) -> DocumentConfig {
        crate::manager().unwrap().config(&self.label).unwrap()
    }

    pub fn set_config(&self, edit: impl FnOnce(&mut DocumentConfig)) {
        let mut config = self.config();
        edit(&mut config);
        crate::manager()
            .unwrap()
            .update_config(&self.label, config)
            .expect("update config");
    }

    pub fn arc_config(&self) -> Arc<DocumentConfig> {
        Arc::new(self.config())
    }

    /// Every row of a table as objects, in identity order.
    pub fn rows(&self, table: &str) -> Vec<Map<String, Value>> {
        let page = crate::read_table_page(
            self.label.clone(),
            table.into(),
            0,
            1000,
            vec![],
            vec![],
            None,
        )
        .expect("read page");
        let names: Vec<String> = page.columns.iter().map(|c| c.name.clone()).collect();
        page.rows.iter().map(|r| row_to_object(&names, r)).collect()
    }

    /// One column of every row.
    pub fn column(&self, table: &str, column: &str) -> Vec<Value> {
        self.rows(table)
            .into_iter()
            .map(|mut r| r.remove(column).unwrap_or(Value::Null))
            .collect()
    }

    /// A context on this session's current config.
    pub fn ctx(&self, host: Arc<dyn ActionHost>) -> ActionContext {
        ActionContext::new(&self.label, self.arc_config(), host)
    }

    pub fn env(&self, host: Arc<dyn ActionHost>) -> Arc<dyn TriggerEnv> {
        Arc::new(SessionEnv::new(&self.label, host))
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        let _ = crate::close_document(self.label.clone(), true);
    }
}

pub fn host() -> Arc<RecordingHost> {
    Arc::new(RecordingHost::answering(true))
}

pub fn int(v: i64) -> DataValue {
    DataValue::Integer(v)
}

pub fn text(v: &str) -> DataValue {
    DataValue::Text(v.into())
}

pub fn named(column: &str, value: DataValue) -> NamedValue {
    NamedValue {
        column: column.into(),
        value,
    }
}

/// A step from JSON; `id` defaults to a fresh one.
pub fn step(mut value: Value) -> Step {
    let obj = value.as_object_mut().unwrap();
    obj.entry("id")
        .or_insert_with(|| Value::String(uuid::Uuid::new_v4().to_string()));
    serde_json::from_value(value).expect("step")
}

pub fn action(id: &str, steps: Vec<Step>) -> ActionDef {
    serde_json::from_value(json!({"id": id, "name": id, "steps": steps, "onError": "stop"}))
        .expect("action")
}

pub fn action_with(id: &str, on_error: &str, steps: Vec<Step>) -> ActionDef {
    let mut a = action(id, steps);
    a.on_error = serde_json::from_value(json!(on_error)).unwrap();
    a
}

pub fn trigger(value: Value) -> Trigger {
    let mut base = json!({
        "id": "t1", "name": "Trigger", "table": "orders", "event": "created",
        "actionId": "audit", "mode": "sync", "enabled": true,
        "maxAttempts": 3, "backoffMs": 10,
    });
    for (k, v) in value.as_object().unwrap() {
        base[k] = v.clone();
    }
    serde_json::from_value(base).expect("trigger")
}

pub fn obj(value: Value) -> Map<String, Value> {
    value.as_object().expect("object").clone()
}

/// The events a host recorded, without the refresh that follows every write.
pub fn events(host: &RecordingHost) -> Vec<Value> {
    host.events()
        .into_iter()
        .filter(|e| e != &json!(["refresh"]))
        .collect()
}

/// A session with `orders` (two open orders), an empty `audit` table that rejects the
/// message `bad`, and a form, report and saved query to navigate to or run.
pub fn orders_db() -> Db {
    let db = Db::new();
    db.table("orders", &["id", "status", "total:int"]);
    db.table_checked("audit", &["id", "message"], &["message <> 'bad'"]);
    db.seed("orders", json!({"id": 1, "status": "open", "total": 10}));
    db.seed("orders", json!({"id": 2, "status": "open", "total": 20}));
    db.set_config(|c| {
        c.saved_queries = vec![serde_json::from_value(json!({
            "id": "q1", "name": "Open orders", "sql": "select id, total from orders order by id",
        }))
        .unwrap()];
        c.design.forms = vec![serde_json::from_value(json!({"id": "f1", "name": "Orders"})).unwrap()];
        c.reports = vec![serde_json::from_value(json!({"id": "r1", "name": "Invoice"})).unwrap()];
    });
    db
}

/// Stores the actions in the session's definitions.
pub fn with_actions(db: &Db, actions: Vec<ActionDef>) {
    db.set_config(|c| c.actions = actions);
}

/// True when the message of a failed result contains `needle`, ignoring case.
pub fn error_has(error: &Option<String>, needle: &str) -> bool {
    error
        .as_deref()
        .is_some_and(|e| e.to_lowercase().contains(&needle.to_lowercase()))
}
