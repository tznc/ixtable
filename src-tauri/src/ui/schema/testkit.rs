//! Builders for backend schema types in unit tests.

use crate::data::{CheckDef, Column, TableSchema};

pub fn column(name: &str, logical: &str) -> Column {
    Column {
        name: name.into(),
        declared_type: logical.to_uppercase(),
        logical_type: logical.parse().expect("logical type"),
        nullable: true,
        default_value: None,
        primary_key_position: 0,
        generated: false,
        unique: false,
        auto_increment: false,
    }
}

pub fn schema(name: &str, columns: Vec<Column>, checks: &[&str]) -> TableSchema {
    TableSchema {
        name: name.into(),
        object_type: "table".into(),
        columns,
        foreign_keys: vec![],
        without_rowid: false,
        primary_key: vec![],
        uniques: vec![],
        checks: checks
            .iter()
            .map(|expression| CheckDef {
                name: None,
                expression: (*expression).into(),
            })
            .collect(),
        indexes: vec![],
    }
}
