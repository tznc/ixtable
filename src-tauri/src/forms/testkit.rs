//! Builders for table schemas in tests (the TS tests wrote them as object literals).

use crate::data::{Column, ForeignKey, LogicalType, TableSchema};

/// A column whose logical type is inferred from its declared type.
pub fn col(name: &str, declared: &str) -> Column {
    Column {
        name: name.into(),
        declared_type: declared.into(),
        logical_type: LogicalType::from_sqlite_declared(declared),
        nullable: true,
        default_value: None,
        primary_key_position: 0,
        generated: false,
        unique: false,
        auto_increment: false,
    }
}

pub fn pk(mut column: Column, position: u32) -> Column {
    column.primary_key_position = position;
    column
}

pub fn not_null(mut column: Column) -> Column {
    column.nullable = false;
    column
}

pub fn auto(mut column: Column) -> Column {
    column.auto_increment = true;
    column
}

pub fn fk(from: &[&str], target: &str, target_columns: &[&str]) -> ForeignKey {
    ForeignKey {
        id: 0,
        name: None,
        from_columns: from.iter().map(|s| s.to_string()).collect(),
        target_table: target.into(),
        target_columns: target_columns.iter().map(|s| s.to_string()).collect(),
        on_update: String::new(),
        on_delete: String::new(),
    }
}

pub fn table(name: &str, columns: Vec<Column>, foreign_keys: Vec<ForeignKey>) -> TableSchema {
    let mut keys: Vec<&Column> = columns.iter().filter(|c| c.primary_key_position > 0).collect();
    keys.sort_by_key(|c| c.primary_key_position);
    let primary_key = keys.iter().map(|c| c.name.clone()).collect();
    TableSchema {
        name: name.into(),
        object_type: "table".into(),
        columns,
        foreign_keys,
        without_rowid: false,
        primary_key,
        uniques: vec![],
        checks: vec![],
        indexes: vec![],
    }
}

/// A real backend session for data-layer tests: a new document under a unique window label,
/// closed on drop.
pub struct Db {
    pub label: String,
}

impl Db {
    pub fn open() -> Self {
        let label = format!("forms-test-{}", uuid::Uuid::new_v4());
        crate::new_document(label.clone()).expect("new document");
        Self { label }
    }

    /// Creates a table. `columns`: (name, declared type, primary key position; 0 for none).
    pub fn table(&self, name: &str, columns: &[(&str, &str, u32)]) {
        self.table_with(name, columns, serde_json::json!([]));
    }

    pub fn table_with(
        &self,
        name: &str,
        columns: &[(&str, &str, u32)],
        foreign_keys: serde_json::Value,
    ) {
        let spec = serde_json::json!({
            "name": name,
            "columns": columns.iter().map(|(n, t, pk)| serde_json::json!({
                "name": n, "declaredType": t, "nullable": true, "primaryKeyPosition": pk,
                "unique": false, "defaultExpression": null, "generatedExpression": null,
            })).collect::<Vec<_>>(),
            "foreignKeys": foreign_keys, "checks": [], "withoutRowid": false,
        });
        crate::create_database_table(
            self.label.clone(),
            serde_json::from_value(spec).expect("table spec"),
        )
        .expect("create table");
    }

    /// Inserts a row through the record layer (hooks run, caches invalidate).
    pub fn insert(&self, table: &str, values: serde_json::Value) {
        let values = values
            .as_object()
            .expect("an object")
            .iter()
            .map(|(column, value)| crate::data::NamedValue {
                column: column.clone(),
                value: crate::forms::values::to_data_value(value),
            })
            .collect();
        crate::records::insert(&self.label, table, values).expect("insert row");
    }

    /// Replaces parts of the document config (test shortcut; the app edits through ConfigStore).
    pub fn configure(&self, change: impl FnOnce(&mut crate::archive::DocumentConfig)) {
        let mut config = crate::read_document_config(self.label.clone()).expect("read config");
        change(&mut config);
        crate::update_document_config(self.label.clone(), config).expect("update config");
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        crate::forms::data::forget(&self.label);
        let _ = crate::close_document(self.label.clone(), true);
    }
}
