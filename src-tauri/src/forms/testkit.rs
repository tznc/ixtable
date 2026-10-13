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
