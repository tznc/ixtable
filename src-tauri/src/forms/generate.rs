//! Generated CRUD forms (PRD §14; port of `src/design/generate.ts`): a list form and a
//! detail/create/edit form for one table, built only from the public form primitives a
//! designer uses by hand.

use super::fields::with_field_defaults;
use super::schema::{new_form, new_id, next_placement};
use crate::data::{Column, ForeignKey, LogicalType, TableSchema};
use crate::design::{
    Control, ControlKind, Form, FormMode, FormSource, KeyPair, NavKind, NavigationItem,
    RelatedList, Relationship, SourceKind, Validation,
};
use crate::recordstore::EntitySettings;
use crate::ui::grid::engine::default_grid_layout;
use std::collections::HashMap;

/// "order_items" to "Order items", "customer_id" to "Customer id".
pub fn humanize(name: &str) -> String {
    let replaced: String = {
        let mut out = String::new();
        let mut in_run = false;
        for c in name.chars() {
            if c == '_' || c == '-' {
                if !in_run {
                    out.push(' ');
                }
                in_run = true;
            } else {
                in_run = false;
                out.push(c);
            }
        }
        out
    };
    let text = replaced.trim();
    let mut chars = text.chars();
    match chars.next() {
        None => name.to_string(),
        Some(first) => first.to_uppercase().chain(chars).collect(),
    }
}

fn logical_kind(logical: &LogicalType) -> Option<ControlKind> {
    match logical {
        LogicalType::Text | LogicalType::Uuid => Some(ControlKind::Text),
        LogicalType::Json => Some(ControlKind::Multiline),
        LogicalType::Integer => Some(ControlKind::Number),
        LogicalType::Real | LogicalType::Decimal { .. } => Some(ControlKind::Decimal),
        LogicalType::Boolean => Some(ControlKind::Boolean),
        LogicalType::Date => Some(ControlKind::Date),
        LogicalType::Time => Some(ControlKind::Time),
        LogicalType::Timestamp => Some(ControlKind::Datetime),
        LogicalType::Blob => None,
    }
}

/// Control kind for a column: its logical type (as `inspect_table` reports it, on any
/// backend) when known, else the declared SQL type by SQLite affinity rules plus date names.
pub fn kind_for_declared(declared: &str, logical: Option<&LogicalType>) -> Option<ControlKind> {
    if let Some(logical) = logical {
        return logical_kind(logical);
    }
    let upper = declared.to_ascii_uppercase();
    if upper.contains("BLOB") {
        return None;
    }
    if upper.contains("BOOL") {
        return Some(ControlKind::Boolean);
    }
    if upper.contains("TIMESTAMP") || upper.contains("DATETIME") {
        return Some(ControlKind::Datetime);
    }
    if upper.contains("DATE") {
        return Some(ControlKind::Date);
    }
    if upper.contains("TIME") {
        return Some(ControlKind::Time);
    }
    if upper.contains("INT") {
        return Some(ControlKind::Number);
    }
    if ["REAL", "FLOA", "DOUB", "NUM", "DEC", "MONEY"]
        .iter()
        .any(|t| upper.contains(t))
    {
        return Some(ControlKind::Decimal);
    }
    Some(ControlKind::Text)
}

pub fn kind_for_column(column: &Column) -> Option<ControlKind> {
    kind_for_declared(&column.declared_type, Some(&column.logical_type))
}

/// Display column for a lookup: the first text column that is not a key, else the key itself.
pub fn display_column_for(target: Option<&TableSchema>, fallback: &str) -> String {
    target
        .and_then(|t| {
            t.columns.iter().find(|c| {
                c.primary_key_position == 0 && kind_for_column(c) == Some(ControlKind::Text)
            })
        })
        .map_or_else(|| fallback.to_string(), |c| c.name.clone())
}

/// True for a single primary key the database fills in on insert, as the catalog reports
/// it (`auto_increment`).
pub fn is_auto_key(table: &TableSchema, column: &Column) -> bool {
    column.auto_increment
        && column.primary_key_position > 0
        && table
            .columns
            .iter()
            .filter(|c| c.primary_key_position > 0)
            .count()
            == 1
}

/// The table's primary key columns, in key order.
pub fn primary_key_of(table: Option<&TableSchema>) -> Vec<String> {
    let mut keys: Vec<&Column> = table
        .map(|t| t.columns.iter().filter(|c| c.primary_key_position > 0).collect())
        .unwrap_or_default();
    keys.sort_by_key(|c| c.primary_key_position);
    keys.into_iter().map(|c| c.name.clone()).collect()
}

/// Target column for each source column of a key; an unnamed target is the parent's primary key.
pub fn key_pairs(fk: &ForeignKey, parent: Option<&TableSchema>) -> Vec<KeyPair> {
    let primary = primary_key_of(parent);
    fk.from_columns
        .iter()
        .enumerate()
        .map(|(i, column)| KeyPair {
            column: column.clone(),
            target: fk
                .target_columns
                .get(i)
                .or_else(|| primary.get(i))
                .cloned()
                .unwrap_or_else(|| "id".into()),
        })
        .collect()
}

#[derive(Default)]
pub struct GenerateOptions<'a> {
    /// Schemas of tables referenced by foreign keys, used to pick lookup display columns.
    pub targets: HashMap<String, &'a TableSchema>,
    /// Tables whose foreign keys point at this table; each becomes a related-record list.
    pub children: Vec<&'a TableSchema>,
    /// Child table to the form used to add and edit its rows in the related list.
    pub child_forms: HashMap<String, String>,
    /// Entity settings; field formats and input masks of the table's columns shape controls.
    pub entities: &'a [EntitySettings],
}

#[derive(Clone, Debug)]
pub struct GeneratedCrud {
    pub list: Form,
    pub detail: Form,
    pub navigation: NavigationItem,
}

/// The foreign key whose selector edits `column`: a multi-column key it leads (that selector
/// writes every column of the key), else a single-column one.
fn key_for<'a>(table: &'a TableSchema, column: &str) -> Option<&'a ForeignKey> {
    table
        .foreign_keys
        .iter()
        .find(|fk| fk.from_columns.len() > 1 && fk.from_columns[0] == column)
        .or_else(|| {
            table
                .foreign_keys
                .iter()
                .find(|fk| fk.from_columns.len() == 1 && fk.from_columns[0] == column)
        })
}

/// Columns written by a multi-column selector led by another column (no control of their own).
fn covered_columns(table: &TableSchema) -> Vec<String> {
    let mut covered = vec![];
    for fk in &table.foreign_keys {
        let leads = fk.from_columns.len() > 1
            && key_for(table, &fk.from_columns[0]).is_some_and(|k| std::ptr::eq(k, fk));
        if !leads {
            continue;
        }
        for column in &fk.from_columns[1..] {
            let own = key_for(table, column);
            if own.is_none_or(|k| k.from_columns.len() == 1) {
                covered.push(column.clone());
            }
        }
    }
    covered
}

fn strip_id_suffix(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    let cut = if lower.ends_with("_id") {
        name.len() - 3
    } else if lower.ends_with("id") {
        name.len() - 2
    } else {
        name.len()
    };
    name[..cut].to_string()
}

fn column_control(
    table: &TableSchema,
    column: &Column,
    form: &Form,
    options: &GenerateOptions,
) -> Option<Control> {
    let foreign_key = key_for(table, &column.name);
    let kind = if foreign_key.is_some() {
        Some(ControlKind::Relationship)
    } else {
        kind_for_column(column)
    }?;
    let auto = is_auto_key(table, column);
    let composite = foreign_key.is_some_and(|fk| fk.from_columns.len() > 1);
    let label = match foreign_key {
        Some(fk) if composite => {
            let t = &fk.target_table;
            if t.to_ascii_lowercase().ends_with('s') {
                t[..t.len() - 1].to_string()
            } else {
                t.clone()
            }
        }
        Some(_) => {
            let stripped = strip_id_suffix(&column.name);
            if stripped.is_empty() {
                column.name.clone()
            } else {
                stripped
            }
        }
        None => column.name.clone(),
    };
    let mut control = Control {
        id: new_id(),
        kind,
        label: humanize(&label),
        binding: Some(crate::design::Binding {
            table: None,
            column: column.name.clone(),
        }),
        validation: Validation {
            required: !column.nullable && !auto && column.default_value.is_none(),
            ..Default::default()
        },
        placement: next_placement(
            form,
            None,
            Some(if kind == ControlKind::Multiline { 12 } else { 6 }),
            None,
        ),
        ..Default::default()
    };
    if auto {
        control.read_only = true;
    }
    let Some(fk) = foreign_key else {
        let probe = Form {
            source: Some(FormSource {
                kind: SourceKind::Table,
                table: Some(table.name.clone()),
                ..Default::default()
            }),
            controls: vec![control.clone()],
            ..form.clone()
        };
        if let Some(mut formatted) = with_field_defaults(&probe, options.entities)
            .map(|f| f.controls[0].clone())
        {
            if matches!(
                formatted.kind,
                ControlKind::RichText | ControlKind::Attachment
            ) {
                formatted.placement = next_placement(form, None, Some(12), None);
            }
            return Some(formatted);
        }
        return Some(control);
    };
    let target = options.targets.get(&fk.target_table).copied();
    let keys = key_pairs(fk, target);
    let value_column = keys[0].target.clone();
    control.relationship = Some(Relationship {
        table: fk.target_table.clone(),
        display_column: display_column_for(target, &value_column),
        value_column,
        keys: if composite { keys } else { vec![] },
        filter: None,
    });
    Some(control)
}

fn related_list_control(
    table: &TableSchema,
    child: &TableSchema,
    detail: &Form,
    options: &GenerateOptions,
) -> Option<Control> {
    let link = child
        .foreign_keys
        .iter()
        .find(|fk| fk.target_table == table.name)?;
    let keys = key_pairs(link, Some(table));
    let related = RelatedList {
        table: child.name.clone(),
        foreign_key: keys[0].column.clone(),
        parent_column: keys[0].target.clone(),
        columns: child
            .columns
            .iter()
            .filter(|c| !link.from_columns.contains(&c.name) && kind_for_column(c).is_some())
            .map(|c| c.name.clone())
            .collect(),
        form_id: options.child_forms.get(&child.name).cloned(),
        keys: if keys.len() > 1 { keys } else { vec![] },
        filter: None,
    };
    Some(Control {
        id: new_id(),
        kind: ControlKind::RelatedList,
        label: humanize(&child.name),
        placement: next_placement(detail, None, None, None),
        related: Some(related),
        ..Default::default()
    })
}

/// Generated CRUD (PRD §14): a list form and a detail/create/edit form for one table.
pub fn generate_crud_forms(table: &TableSchema, options: &GenerateOptions) -> GeneratedCrud {
    let title = humanize(&table.name);
    let source = FormSource {
        kind: SourceKind::Table,
        table: Some(table.name.clone()),
        ..Default::default()
    };
    let mut detail = Form {
        modes: vec![FormMode::Detail, FormMode::Create, FormMode::Edit],
        ..new_form(&title, Some(source.clone()))
    };
    let covered = covered_columns(table);
    for column in &table.columns {
        if covered.contains(&column.name) {
            continue;
        }
        if let Some(control) = column_control(table, column, &detail, options) {
            detail.controls.push(control);
        }
    }
    for child in &options.children {
        if let Some(control) = related_list_control(table, child, &detail, options) {
            detail.controls.push(control);
        }
    }
    let list = Form {
        modes: vec![FormMode::List],
        layout: default_grid_layout(),
        list_columns: table
            .columns
            .iter()
            .filter(|c| kind_for_column(c).is_some())
            .map(|c| c.name.clone())
            .collect(),
        detail_form_id: Some(detail.id.clone()),
        ..new_form(&format!("{title} list"), Some(source))
    };
    let navigation = NavigationItem {
        id: new_id(),
        label: title,
        kind: NavKind::Form,
        target_id: Some(list.id.clone()),
        ..Default::default()
    };
    GeneratedCrud {
        list,
        detail,
        navigation,
    }
}

#[cfg(test)]
#[path = "generate_tests.rs"]
mod tests;
