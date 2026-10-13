//! Editable column definitions shared by the create form and the table designer
//! (port of `src/schema/columns.ts`).

use super::logical::logical_of;
use crate::data::{Column, CreateColumn, TableSchema};

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnDraft {
    pub name: String,
    pub logical_type: String,
    pub required: bool,
    pub default_expression: String,
    pub unique: bool,
    pub check: String,
}

pub fn empty_column(name: &str) -> ColumnDraft {
    ColumnDraft {
        name: name.to_string(),
        logical_type: "text".into(),
        required: false,
        default_expression: String::new(),
        unique: false,
        check: String::new(),
    }
}

pub fn draft_from_column(column: &Column, schema: &TableSchema) -> ColumnDraft {
    ColumnDraft {
        name: column.name.clone(),
        logical_type: logical_of(column),
        required: !column.nullable,
        default_expression: column.default_value.clone().unwrap_or_default(),
        unique: column.unique
            || schema
                .uniques
                .iter()
                .any(|u| u.columns.len() == 1 && u.columns[0] == column.name),
        check: column_check(schema, &column.name),
    }
}

/// Lower-cased identifiers an expression mentions, skipping string literals.
fn identifiers(expression: &str) -> Vec<String> {
    let chars: Vec<char> = expression.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\'' => {
                i += 1;
                while i < chars.len() {
                    if chars[i] == '\'' {
                        if chars.get(i + 1) == Some(&'\'') {
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                i += 1;
                let mut name = String::new();
                while i < chars.len() {
                    if chars[i] == '"' {
                        if chars.get(i + 1) == Some(&'"') {
                            name.push('"');
                            i += 2;
                            continue;
                        }
                        break;
                    }
                    name.push(chars[i]);
                    i += 1;
                }
                i += 1;
                out.push(name.to_lowercase());
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                    i += 1;
                }
                out.push(chars[start..i].iter().collect::<String>().to_lowercase());
            }
            _ => i += 1,
        }
    }
    out
}

/// The column's own check: table checks that mention this column and no other.
/// Mirrors `column_checks`/`joined_check` in `recordstore/plan.rs`, so the designer
/// shows exactly what `alter_column` would keep or replace.
pub fn column_check(schema: &TableSchema, column: &str) -> String {
    let me = column.to_lowercase();
    let others: Vec<String> = schema
        .columns
        .iter()
        .map(|c| c.name.to_lowercase())
        .filter(|c| *c != me)
        .collect();
    let owned: Vec<String> = schema
        .checks
        .iter()
        .map(|c| c.expression.trim().to_string())
        .filter(|e| {
            let ids = identifiers(e);
            ids.contains(&me) && !others.iter().any(|o| ids.contains(o))
        })
        .collect();
    if owned.len() == 1 {
        owned[0].clone()
    } else {
        owned
            .iter()
            .map(|e| format!("({e})"))
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

pub fn spec_from_draft(draft: &ColumnDraft, primary_key_position: u32) -> CreateColumn {
    let non_empty = |s: &str| {
        let s = s.trim();
        (!s.is_empty()).then(|| s.to_string())
    };
    CreateColumn {
        name: draft.name.trim().to_string(),
        declared_type: String::new(),
        logical_type: draft.logical_type.parse().ok(),
        nullable: !draft.required,
        primary_key_position,
        unique: draft.unique,
        default_expression: non_empty(&draft.default_expression),
        generated_expression: None,
        check: non_empty(&draft.check),
    }
}
