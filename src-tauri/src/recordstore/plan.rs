//! Store-neutral schema planning: turns a `CreateTable` spec into a
//! `TableDef`, and applies `AlterTable` operations to a definition while
//! tracking where each final column's data comes from (for rebuilds).
use super::model::{ChangeMode, PlannedOperation};
use crate::data::{
    q, AlterTable, CheckDef, ColumnDef, CreateColumn, CreateForeignKey, CreateTable, ForeignKeyDef,
    IndexDef, LogicalType, TableDef, UniqueDef,
};
use std::collections::HashSet;

pub fn safe_expression(value: &str) -> Result<String, String> {
    let v = value.trim();
    if v.is_empty() || v.contains(';') || v.contains("--") || v.contains("/*") {
        Err(format!("Unsafe or empty SQL expression {value:?}"))
    } else {
        Ok(v.into())
    }
}
pub fn validate_name(kind: &str, name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err(format!("{kind} name is required"));
    }
    if name.contains('\0') {
        return Err(format!("{kind} name contains a NUL character"));
    }
    Ok(())
}
pub fn validate_table_name(name: &str) -> Result<(), String> {
    validate_name("Table", name)?;
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("_ixtable_") || lower.starts_with("sqlite_") {
        return Err(format!("Table name {name:?} is reserved for internal use"));
    }
    Ok(())
}
fn action(v: &Option<String>) -> Result<String, String> {
    let x = v
        .as_deref()
        .unwrap_or("NO ACTION")
        .trim()
        .to_ascii_uppercase();
    if matches!(
        x.as_str(),
        "NO ACTION" | "RESTRICT" | "SET NULL" | "SET DEFAULT" | "CASCADE"
    ) {
        Ok(x)
    } else {
        Err(format!("Invalid foreign-key action {x:?}"))
    }
}

/// Resolves a column spec. `declaredType` alone keeps working (legacy SQLite
/// affinities); `logicalType` wins when both are given.
pub fn column_def(c: &CreateColumn) -> Result<ColumnDef, String> {
    validate_name("Column", &c.name)?;
    let logical = match &c.logical_type {
        Some(t) => t.clone(),
        None => {
            let upper = c.declared_type.trim().to_ascii_uppercase().replace(' ', "");
            let known = LogicalType::from_sqlite_declared(&upper);
            let legacy = matches!(
                upper.as_str(),
                "INTEGER" | "REAL" | "TEXT" | "BLOB" | "NUMERIC" | ""
            );
            if !legacy && known.sqlite_declared().replace(' ', "") != upper {
                return Err(format!("Unsupported column type {:?}", c.declared_type));
            }
            known
        }
    };
    Ok(ColumnDef {
        name: c.name.clone(),
        declared_type: logical.sqlite_declared(),
        logical_type: logical,
        nullable: c.nullable,
        default_expression: c
            .default_expression
            .as_deref()
            .map(safe_expression)
            .transpose()?,
        generated_expression: c
            .generated_expression
            .as_deref()
            .map(safe_expression)
            .transpose()?,
        identity: false,
    })
}
pub fn foreign_key_def(f: &CreateForeignKey) -> Result<ForeignKeyDef, String> {
    if f.columns.is_empty() || f.columns.len() != f.target_columns.len() {
        return Err("A foreign key needs matching source and target columns".into());
    }
    validate_name("Target table", &f.target_table)?;
    Ok(ForeignKeyDef {
        name: f.name.clone().filter(|n| !n.trim().is_empty()),
        columns: f.columns.clone(),
        target_table: f.target_table.clone(),
        target_columns: f.target_columns.clone(),
        on_update: action(&f.on_update)?,
        on_delete: action(&f.on_delete)?,
    })
}

pub fn def_from_spec(spec: &CreateTable) -> Result<TableDef, String> {
    validate_table_name(&spec.name)?;
    if spec.columns.is_empty() {
        return Err("A table name and at least one column are required".into());
    }
    let mut def = TableDef {
        name: spec.name.clone(),
        without_rowid: spec.without_rowid,
        ..Default::default()
    };
    for c in &spec.columns {
        def.columns.push(column_def(c)?);
        if c.unique {
            def.uniques.push(UniqueDef {
                name: None,
                columns: vec![c.name.clone()],
            })
        }
        if let Some(check) = c.check.as_deref().filter(|x| !x.trim().is_empty()) {
            def.checks.push(CheckDef {
                name: None,
                expression: safe_expression(check)?,
            })
        }
    }
    let mut pk: Vec<_> = spec
        .columns
        .iter()
        .filter(|c| c.primary_key_position > 0)
        .collect();
    pk.sort_by_key(|c| c.primary_key_position);
    def.primary_key = pk.iter().map(|c| c.name.clone()).collect();
    for f in &spec.foreign_keys {
        def.foreign_keys.push(foreign_key_def(f)?);
    }
    for check in &spec.checks {
        def.checks.push(CheckDef {
            name: None,
            expression: safe_expression(check)?,
        })
    }
    for u in &spec.uniques {
        def.uniques.push(UniqueDef {
            name: None,
            columns: u.clone(),
        })
    }
    for i in &spec.indexes {
        validate_name("Index", &i.name)?;
        def.indexes.push(IndexDef {
            name: i.name.clone(),
            table: spec.name.clone(),
            columns: i.columns.clone(),
            unique: i.unique,
            sql: None,
        })
    }
    validate_def(&def)?;
    Ok(def)
}

/// Structural checks shared by create and alter.
pub fn validate_def(def: &TableDef) -> Result<(), String> {
    let mut seen = HashSet::new();
    for c in &def.columns {
        if !seen.insert(c.name.to_ascii_lowercase()) {
            return Err("Column names must be unique".into());
        }
    }
    if def.columns.is_empty() {
        return Err("A table needs at least one column".into());
    }
    let known = |n: &String| seen.contains(&n.to_ascii_lowercase());
    let check_all = |what: &str, cols: &[String]| -> Result<(), String> {
        if cols.is_empty() {
            return Err(format!("{what} needs at least one column"));
        }
        match cols.iter().find(|c| !known(c)) {
            Some(c) => Err(format!("{what} references unknown column {c:?}")),
            None => Ok(()),
        }
    };
    if !def.primary_key.is_empty() {
        check_all("The primary key", &def.primary_key)?;
    }
    if def.without_rowid && def.primary_key.is_empty() {
        return Err("Tables without rowid require a primary key".into());
    }
    for u in &def.uniques {
        check_all("A unique constraint", &u.columns)?;
    }
    for f in &def.foreign_keys {
        check_all("A foreign key", &f.columns)?;
    }
    for i in &def.indexes {
        check_all(&format!("Index {:?}", i.name), &i.columns)?;
    }
    Ok(())
}

/// Replaces whole-word references to a column inside an SQL expression,
/// leaving string literals untouched.
pub fn rename_identifier(expr: &str, old: &str, new: &str) -> String {
    let mut out = String::with_capacity(expr.len());
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' || c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() {
                if chars[i] == c {
                    if chars.get(i + 1) == Some(&c) {
                        i += 2;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            let token: String = chars[start..=i.min(chars.len() - 1)].iter().collect();
            i += 1;
            if c == '"'
                && token.len() >= 2
                && token[1..token.len() - 1].replace("\"\"", "\"") == old
            {
                out.push_str(&q(new));
            } else {
                out.push_str(&token);
            }
        } else if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if word.eq_ignore_ascii_case(old) {
                out.push_str(&q(new));
            } else {
                out.push_str(&word);
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}
pub fn mentions(expr: &str, column: &str) -> bool {
    rename_identifier(expr, column, "\u{1}") != expr
}

/// Indexes of the checks that belong to `column`: they mention it and no other
/// column. The schema designer shows these as the column's check.
pub fn column_checks(def: &TableDef, column: &str) -> Vec<usize> {
    def.checks
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            mentions(&c.expression, column)
                && def.columns.iter().all(|o| {
                    o.name.eq_ignore_ascii_case(column) || !mentions(&c.expression, &o.name)
                })
        })
        .map(|(k, _)| k)
        .collect()
}

/// One expression for several column checks, as the designer displays it.
pub fn joined_check<'a>(exprs: impl Iterator<Item = &'a str>) -> Option<String> {
    let all: Vec<&str> = exprs.map(str::trim).collect();
    match all.len() {
        0 => None,
        1 => Some(all[0].to_string()),
        _ => Some(
            all.iter()
                .map(|e| format!("({e})"))
                .collect::<Vec<_>>()
                .join(" AND "),
        ),
    }
}

/// Result of applying operations to a definition.
pub struct Applied {
    pub def: TableDef,
    /// For each final column, the original column it is copied from.
    pub sources: Vec<Option<String>>,
    pub rename_to: Option<String>,
    pub operations: Vec<PlannedOperation>,
    pub warnings: Vec<String>,
}

fn widening(from: &LogicalType, to: &LogicalType) -> bool {
    from == to
        || matches!(to, LogicalType::Text)
        || matches!(
            (from, to),
            (
                LogicalType::Integer,
                LogicalType::Real | LogicalType::Decimal { .. }
            ) | (LogicalType::Date, LogicalType::Timestamp)
        )
        || matches!((from, to), (LogicalType::Decimal { precision: Some(a), scale: s1 }, LogicalType::Decimal { precision: Some(b), scale: s2 }) if b >= a && s2 >= s1)
}

/// Applies `ops` in order. `classify` decides each operation's mode from the
/// definition as it stands before that operation.
pub fn apply_ops(
    original: &TableDef,
    ops: &[AlterTable],
    classify: impl Fn(&AlterTable, &TableDef) -> (ChangeMode, Option<String>),
) -> Result<Applied, String> {
    let mut def = original.clone();
    let mut sources: Vec<Option<String>> =
        def.columns.iter().map(|c| Some(c.name.clone())).collect();
    let mut rename_to = None;
    let mut operations = vec![];
    let mut warnings = vec![];
    let find = |def: &TableDef, name: &str| -> Result<usize, String> {
        def.columns
            .iter()
            .position(|c| c.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| format!("Column {name:?} does not exist"))
    };
    let rename_in_list = |list: &mut Vec<String>, old: &str, new: &str| {
        for c in list.iter_mut() {
            if c.eq_ignore_ascii_case(old) {
                *c = new.into();
            }
        }
    };
    for op in ops {
        let (mode, reason) = classify(op, &def);
        let mut destructive = false;
        let summary = match op {
            AlterTable::RenameTable { new_name } => {
                validate_table_name(new_name)?;
                rename_to = Some(new_name.clone());
                format!("Rename table to {new_name:?}")
            }
            AlterTable::RenameColumn { column, new_name } => {
                validate_name("Column", new_name)?;
                let i = find(&def, column)?;
                let old = def.columns[i].name.clone();
                def.columns[i].name = new_name.clone();
                rename_in_list(&mut def.primary_key, &old, new_name);
                for u in &mut def.uniques {
                    rename_in_list(&mut u.columns, &old, new_name);
                }
                for f in &mut def.foreign_keys {
                    rename_in_list(&mut f.columns, &old, new_name);
                }
                for x in &mut def.indexes {
                    rename_in_list(&mut x.columns, &old, new_name);
                }
                for c in &mut def.checks {
                    c.expression = rename_identifier(&c.expression, &old, new_name);
                }
                for c in &mut def.columns {
                    c.generated_expression = c
                        .generated_expression
                        .as_deref()
                        .map(|e| rename_identifier(e, &old, new_name));
                }
                format!("Rename column {old:?} to {new_name:?}")
            }
            AlterTable::AddColumn { column } => {
                let mut c = column.clone();
                c.primary_key_position = 0;
                let col = column_def(&c)?;
                if !col.nullable
                    && col.default_expression.is_none()
                    && col.generated_expression.is_none()
                {
                    warnings.push(format!(
                        "{:?} is required but has no default, so existing rows cannot be filled. Set a default value or make it optional.",
                        col.name
                    ));
                }
                def.columns.push(col);
                sources.push(None);
                if column.unique {
                    def.uniques.push(UniqueDef {
                        name: None,
                        columns: vec![column.name.clone()],
                    });
                }
                if let Some(check) = column.check.as_deref().filter(|x| !x.trim().is_empty()) {
                    def.checks.push(CheckDef {
                        name: None,
                        expression: safe_expression(check)?,
                    });
                }
                format!(
                    "Add column {:?} ({})",
                    column.name,
                    def.columns.last().unwrap().logical_type
                )
            }
            AlterTable::DropColumn { column } => {
                let i = find(&def, column)?;
                let name = def.columns[i].name.clone();
                if def
                    .primary_key
                    .iter()
                    .any(|k| k.eq_ignore_ascii_case(&name))
                {
                    return Err(format!(
                        "{name:?} is part of the primary key; change the key before dropping it"
                    ));
                }
                def.columns.remove(i);
                sources.remove(i);
                let has = |cols: &[String]| cols.iter().any(|c| c.eq_ignore_ascii_case(&name));
                def.uniques.retain(|u| !has(&u.columns));
                def.foreign_keys.retain(|f| !has(&f.columns));
                def.indexes.retain(|x| !has(&x.columns));
                let before = def.checks.len();
                def.checks.retain(|c| !mentions(&c.expression, &name));
                if def.checks.len() != before {
                    warnings.push(format!(
                        "Check constraints that mention {name:?} are removed."
                    ));
                }
                destructive = true;
                format!("Drop column {name:?} and its data")
            }
            AlterTable::AlterColumn { column, definition } => {
                let i = find(&def, column)?;
                let mut spec = definition.clone();
                spec.name = def.columns[i].name.clone();
                let next = column_def(&spec)?;
                let prev = def.columns[i].clone();
                destructive = !widening(&prev.logical_type, &next.logical_type);
                if prev.nullable && !next.nullable {
                    warnings.push(format!(
                        "Rows with an empty {:?} will make this change fail. Fill them in or set a default value first.",
                        prev.name
                    ));
                }
                let name = prev.name.clone();
                let unique_now = def.is_unique_column(&name);
                if definition.unique && !unique_now {
                    def.uniques.push(UniqueDef {
                        name: None,
                        columns: vec![name.clone()],
                    });
                } else if !definition.unique && unique_now {
                    def.uniques.retain(|u| {
                        !(u.columns.len() == 1 && u.columns[0].eq_ignore_ascii_case(&name))
                    });
                }
                // `None` keeps the column's checks; `Some("")` removes them; other
                // text replaces them unless it equals what is stored.
                let wanted = definition
                    .check
                    .as_deref()
                    .map(|x| Some(x.trim()).filter(|x| !x.is_empty()));
                let owned = column_checks(&def, &name);
                let current =
                    joined_check(owned.iter().map(|&k| def.checks[k].expression.as_str()));
                if let Some(wanted) = wanted.filter(|w| *w != current.as_deref()) {
                    let mut k = 0;
                    def.checks.retain(|_| {
                        k += 1;
                        !owned.contains(&(k - 1))
                    });
                    if let Some(check) = wanted {
                        def.checks.push(CheckDef {
                            name: None,
                            expression: safe_expression(check)?,
                        });
                    }
                }
                def.columns[i] = next;
                format!(
                    "Change column {name:?} to {}{}",
                    def.columns[i].logical_type,
                    if def.columns[i].nullable {
                        ""
                    } else {
                        ", required"
                    }
                )
            }
            AlterTable::SetPrimaryKey { columns } => {
                for c in columns {
                    find(&def, c)?;
                }
                def.primary_key = columns.clone();
                def.primary_key_name = None;
                def.autoincrement = false;
                if columns.is_empty() {
                    "Remove the primary key".into()
                } else {
                    format!("Set primary key ({})", columns.join(", "))
                }
            }
            AlterTable::AddForeignKey { foreign_key } => {
                let f = foreign_key_def(foreign_key)?;
                let s = format!(
                    "Add foreign key ({}) → {} ({}) on delete {}",
                    f.columns.join(", "),
                    f.target_table,
                    f.target_columns.join(", "),
                    f.on_delete
                );
                def.foreign_keys.push(f);
                s
            }
            AlterTable::DropForeignKey { columns } => {
                let before = def.foreign_keys.len();
                def.foreign_keys.retain(|f| &f.columns != columns);
                if before == def.foreign_keys.len() {
                    return Err(format!("No foreign key on ({})", columns.join(", ")));
                }
                format!("Drop foreign key ({})", columns.join(", "))
            }
            AlterTable::AddUnique { columns, name } => {
                def.uniques.push(UniqueDef {
                    name: name.clone().filter(|n| !n.trim().is_empty()),
                    columns: columns.clone(),
                });
                format!("Add unique constraint ({})", columns.join(", "))
            }
            AlterTable::DropUnique { columns } => {
                let before = def.uniques.len();
                def.uniques.retain(|u| &u.columns != columns);
                if before == def.uniques.len() {
                    return Err(format!("No unique constraint on ({})", columns.join(", ")));
                }
                format!("Drop unique constraint ({})", columns.join(", "))
            }
            AlterTable::AddCheck { expression, name } => {
                def.checks.push(CheckDef {
                    name: name.clone().filter(|n| !n.trim().is_empty()),
                    expression: safe_expression(expression)?,
                });
                format!("Add check ({expression})")
            }
            AlterTable::DropCheck { expression } => {
                let before = def.checks.len();
                def.checks.retain(|c| {
                    c.expression.trim() != expression.trim()
                        && c.name.as_deref() != Some(expression.as_str())
                });
                if before == def.checks.len() {
                    return Err(format!("No check constraint {expression:?}"));
                }
                format!("Drop check ({expression})")
            }
        };
        validate_def(&def)?;
        operations.push(PlannedOperation {
            summary,
            mode,
            destructive,
            reason,
        });
    }
    Ok(Applied {
        def,
        sources,
        rename_to,
        operations,
        warnings,
    })
}
