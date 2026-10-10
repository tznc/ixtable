//! Access tables → SQLite tables (`docs/decisions/access-import.md`, "Tables").
//!
//! Each Access table keeps its name and column names. Attachment and
//! multi-value columns become child tables, the way Access itself stores them
//! (hidden `f_<guid>_<column>` tables). Constraints are planned here and
//! dropped later by `staging` when the data does not satisfy them.
use crate::access::model::*;
use crate::access::translate::ast::{parse_expression, parse_field_rule};
use crate::access::translate::sql::{quote, Dialect, Kind, Schema, SqlWriter};
use std::collections::BTreeMap;

/// How a staged value becomes the final value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conv {
    AsIs,
    /// `YYYY-MM-DD` from a date-time.
    DateOnly,
    /// `HH:MM:SS` from a date-time.
    TimeOnly,
}

#[derive(Debug, Clone)]
pub struct ColumnPlan {
    pub name: String,
    pub declared: String,
    pub kind: Kind,
    pub not_null: bool,
    pub default_sql: Option<String>,
    /// Field validation rule as a SQLite CHECK, with its message.
    pub check: Option<(String, String)>,
    pub conv: Conv,
    /// Position of the source column in the Access row (main tables).
    pub source: Option<usize>,
    /// A calculated column's SQLite expression, kept current by triggers.
    pub calc_sql: Option<String>,
    pub date_candidate: Conv,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ForeignKeyPlan {
    pub relationship: String,
    pub columns: Vec<String>,
    pub target_table: String,
    pub target_columns: Vec<String>,
    pub on_update: &'static str,
    pub on_delete: &'static str,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IndexPlan {
    pub name: String,
    pub columns: Vec<String>,
    pub unique: bool,
}

/// What a planned table holds.
#[derive(Debug, Clone, PartialEq)]
pub enum TableRole {
    Main,
    /// Files of an attachment column: (parent table, column).
    Attachments(String, String),
    /// Values of a multi-value column: (parent table, column).
    Values(String, String),
}

#[derive(Debug, Clone)]
pub struct TablePlan {
    pub access: String,
    pub name: String,
    pub role: TableRole,
    pub columns: Vec<ColumnPlan>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKeyPlan>,
    /// Table validation rule: (SQLite CHECK, message).
    pub checks: Vec<(String, String)>,
    pub indexes: Vec<IndexPlan>,
    /// Child tables refer to the parent's single key column, or its rowid.
    pub parent_key: Option<String>,
}

impl TablePlan {
    pub fn column(&self, name: &str) -> Option<&ColumnPlan> {
        self.columns
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn column_mut(&mut self, name: &str) -> Option<&mut ColumnPlan> {
        self.columns
            .iter_mut()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// The single-column primary key, if any.
    pub fn single_key(&self) -> Option<&str> {
        match self.primary_key.as_slice() {
            [k] => Some(k),
            _ => None,
        }
    }
}

/// Table names ixtable reserves.
pub fn table_name(access: &str, taken: &[String]) -> String {
    let lower = access.to_ascii_lowercase();
    let mut name = if lower.starts_with("_ixtable_") || lower.starts_with("sqlite_") {
        format!("Access {access}")
    } else {
        access.to_string()
    };
    let base = name.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&name)) {
        name = format!("{base} {n}");
        n += 1;
    }
    name
}

pub fn kind_of(ty: ColType) -> Kind {
    match ty {
        ColType::Boolean => Kind::Bool,
        ColType::Byte
        | ColType::Integer
        | ColType::Long
        | ColType::BigInt
        | ColType::Currency
        | ColType::Single
        | ColType::Double
        | ColType::Numeric { .. } => Kind::Number,
        ColType::DateTime | ColType::ExtDateTime => Kind::Date,
        ColType::Text | ColType::Memo | ColType::Guid => Kind::Text,
        _ => Kind::Other,
    }
}

/// SQLite declared type (which ixtable maps to a logical type).
pub fn declared_type(col: &Column) -> String {
    match col.ty {
        ColType::Boolean => "BOOLEAN".into(),
        ColType::Byte | ColType::Integer | ColType::Long | ColType::BigInt | ColType::Complex => {
            "INTEGER".into()
        }
        ColType::Currency => "DECIMAL(15,4)".into(),
        ColType::Numeric { precision, scale } => {
            let p = precision.clamp(1, 15);
            format!("DECIMAL({p},{})", scale.min(p))
        }
        ColType::Single | ColType::Double => "REAL".into(),
        ColType::DateTime | ColType::ExtDateTime => "TIMESTAMP".into(),
        ColType::Guid => "UUID".into(),
        ColType::Binary | ColType::Ole => "BLOB".into(),
        ColType::Text | ColType::Memo => "TEXT".into(),
    }
}

/// A date-only or time-only `Format` makes the column a candidate for DATE or TIME.
fn date_candidate(col: &Column) -> Conv {
    if !matches!(col.ty, ColType::DateTime) {
        return Conv::AsIs;
    }
    let f = col.prop("Format").unwrap_or("").to_ascii_lowercase();
    match f.as_str() {
        "short date" | "medium date" | "long date" => Conv::DateOnly,
        "short time" | "medium time" | "long time" => Conv::TimeOnly,
        "" | "general date" => Conv::AsIs,
        custom if !custom.contains(['h', 'n', 's']) && custom.contains(['d', 'y']) => {
            Conv::DateOnly
        }
        _ => Conv::AsIs,
    }
}

/// Columns of every table and query as the translator sees them.
pub struct DbSchema<'a> {
    pub db: &'a AccessDb,
    /// Output columns of saved queries, filled as they translate.
    pub query_columns: BTreeMap<String, Vec<(String, Kind)>>,
}

impl Schema for DbSchema<'_> {
    fn columns(&self, source: &str) -> Option<Vec<(String, Kind)>> {
        if let Some(t) = self.db.table(source) {
            return Some(
                t.columns
                    .iter()
                    .filter(|c| c.complex.is_none())
                    .map(|c| (c.name.clone(), kind_of(c.ty)))
                    .collect(),
            );
        }
        self.query_columns.get(&source.to_lowercase()).cloned()
    }

    fn is_query(&self, source: &str) -> bool {
        self.db.table(source).is_none() && self.db.query(source).is_some()
    }

    fn complex(&self, source: &str) -> Vec<String> {
        self.db
            .table(source)
            .map(|t| {
                t.columns
                    .iter()
                    .filter(|c| c.complex.is_some())
                    .map(|c| c.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Columns of one table, for field rules, defaults and calculated columns.
struct TableOnly;

impl Schema for TableOnly {
    fn columns(&self, _: &str) -> Option<Vec<(String, Kind)>> {
        None
    }

    fn is_query(&self, _: &str) -> bool {
        false
    }
}

fn sqlite_writer(cols: &[(String, Kind)]) -> (TableOnly, Vec<(String, Kind)>) {
    (TableOnly, cols.to_vec())
}

/// A field or table validation rule as a SQLite CHECK expression.
pub fn rule_sql(
    rule: &str,
    field: Option<&str>,
    cols: &[(String, Kind)],
) -> Result<String, String> {
    let e = match field {
        Some(f) => parse_field_rule(rule, f)?,
        None => parse_expression(rule)?,
    };
    let (schema, cols) = sqlite_writer(cols);
    let mut w = SqlWriter::new(Dialect::Sqlite, &schema);
    w.table_columns = cols;
    let sql = w.expr(&e)?;
    // SQLite CHECK constraints must be deterministic.
    if sql.contains("'now'") {
        return Err(
            "it uses the current date, which a database constraint cannot; forms check it".into(),
        );
    }
    Ok(sql)
}

/// A `DefaultValue` as a SQLite DEFAULT clause body.
pub fn default_sql(value: &str, cols: &[(String, Kind)]) -> Result<String, String> {
    let body = value.trim().strip_prefix('=').unwrap_or(value.trim());
    let e = parse_expression(body)?;
    let (schema, cols) = sqlite_writer(cols);
    let mut w = SqlWriter::new(Dialect::Sqlite, &schema);
    w.table_columns = cols;
    w.in_default = true;
    let sql = w.expr(&e)?;
    let literal = sql.parse::<f64>().is_ok()
        || (sql.starts_with('\'') && sql.ends_with('\''))
        || sql == "NULL";
    Ok(if literal { sql } else { format!("({sql})") })
}

/// Plans the SQLite tables for every Access table.
pub fn plan_tables(db: &AccessDb, notes: &mut BTreeMap<String, Vec<String>>) -> Vec<TablePlan> {
    let mut plans: Vec<TablePlan> = vec![];
    let mut taken: Vec<String> = vec![];
    for t in &db.tables {
        let name = table_name(&t.name, &taken);
        taken.push(name.clone());
        let note = |notes: &mut BTreeMap<String, Vec<String>>, n: String| {
            notes.entry(t.name.clone()).or_default().push(n)
        };
        let cols: Vec<(String, Kind)> = t
            .columns
            .iter()
            .map(|c| (c.name.clone(), kind_of(c.ty)))
            .collect();
        let mut columns = vec![];
        for (i, c) in t.columns.iter().enumerate() {
            if c.complex.is_some() {
                continue;
            }
            let mut plan = ColumnPlan {
                name: c.name.clone(),
                declared: declared_type(c),
                kind: kind_of(c.ty),
                not_null: c.required(),
                default_sql: None,
                check: None,
                conv: Conv::AsIs,
                source: Some(i),
                calc_sql: None,
                date_candidate: date_candidate(c),
            };
            if let ColType::Numeric { precision, .. } = c.ty {
                if precision > 15 {
                    note(
                        notes,
                        format!(
                            "{}: decimal precision {precision} is stored with at most 15 digits",
                            c.name
                        ),
                    );
                }
            }
            if let Some(d) = c.prop("DefaultValue") {
                match default_sql(d, &cols) {
                    Ok(sql) => plan.default_sql = Some(sql),
                    Err(e) => note(
                        notes,
                        format!("{}: default value {d} was not converted ({e})", c.name),
                    ),
                }
            }
            if let Some(rule) = c.prop("ValidationRule") {
                let message = c.prop("ValidationText").unwrap_or("").to_string();
                match rule_sql(rule, Some(&c.name), &cols) {
                    Ok(sql) => plan.check = Some((sql, message)),
                    Err(e) => note(
                        notes,
                        format!("{}: validation rule {rule} was not converted ({e})", c.name),
                    ),
                }
            }
            if let Some(expr) = &c.expression {
                match default_sql(expr, &cols) {
                    Ok(sql) => plan.calc_sql = Some(sql),
                    Err(e) => note(
                        notes,
                        format!(
                            "{}: calculated column {expr} keeps its stored values ({e})",
                            c.name
                        ),
                    ),
                }
            }
            if c.hyperlink {
                note(
                    notes,
                    format!("{}: hyperlinks are stored as their address", c.name),
                );
            }
            if c.ty == ColType::Ole {
                note(
                    notes,
                    format!("{}: OLE objects are stored as binary data", c.name),
                );
            }
            columns.push(plan);
        }
        let primary_key: Vec<String> = t
            .primary_key()
            .map(|i| i.columns.iter().map(|(c, _)| c.clone()).collect())
            .filter(|k: &Vec<String>| {
                k.iter()
                    .all(|c| columns.iter().any(|p| p.name.eq_ignore_ascii_case(c)))
            })
            .unwrap_or_default();
        let mut checks = vec![];
        if let Some(rule) = t.prop("ValidationRule") {
            match rule_sql(rule, None, &cols) {
                Ok(sql) => checks.push((sql, t.prop("ValidationText").unwrap_or("").to_string())),
                Err(e) => note(
                    notes,
                    format!("table validation rule {rule} was not converted ({e})"),
                ),
            }
        }
        let mut indexes: Vec<IndexPlan> = vec![];
        for i in t.indexes.iter().filter(|i| !i.primary && !i.foreign) {
            let cols: Vec<String> = i.columns.iter().map(|(c, _)| c.clone()).collect();
            if cols.is_empty()
                || !cols
                    .iter()
                    .all(|c| columns.iter().any(|p| p.name.eq_ignore_ascii_case(c)))
            {
                continue;
            }
            if let Some(existing) = indexes.iter_mut().find(|x| x.columns == cols) {
                existing.unique |= i.unique;
                continue;
            }
            indexes.push(IndexPlan {
                name: format!("{name} {}", i.name),
                columns: cols,
                unique: i.unique,
            });
        }
        plans.push(TablePlan {
            access: t.name.clone(),
            name: name.clone(),
            role: TableRole::Main,
            columns,
            primary_key,
            foreign_keys: vec![],
            checks,
            indexes,
            parent_key: None,
        });
        // Complex columns become child tables.
        let main = plans.len() - 1;
        for c in t.columns.iter().filter(|c| c.complex.is_some()) {
            let child_name = table_name(&format!("{} {}", t.name, c.name), &taken);
            taken.push(child_name.clone());
            let child = child_table(t, c, &child_name, &plans[main]);
            plans.push(child);
        }
    }
    plans
}

fn plain_column(name: &str, declared: &str, kind: Kind) -> ColumnPlan {
    ColumnPlan {
        name: name.into(),
        declared: declared.into(),
        kind,
        not_null: false,
        default_sql: None,
        check: None,
        conv: Conv::AsIs,
        source: None,
        calc_sql: None,
        date_candidate: Conv::AsIs,
    }
}

fn child_table(t: &Table, c: &Column, name: &str, parent: &TablePlan) -> TablePlan {
    let parent_key = parent.single_key().map(str::to_string);
    let fk_name = format!(
        "{} {}",
        t.name,
        parent_key.clone().unwrap_or_else(|| "row".into())
    );
    let parent_decl = parent_key
        .as_deref()
        .and_then(|k| parent.column(k))
        .map(|p| p.declared.clone())
        .unwrap_or_else(|| "INTEGER".into());
    let mut columns = vec![
        plain_column("ID", "INTEGER", Kind::Number),
        plain_column(&fk_name, &parent_decl, Kind::Other),
    ];
    columns[1].not_null = true;
    let role = match &c.complex {
        Some(Complex::Attachment) => {
            columns.push(plain_column("File Name", "TEXT", Kind::Text));
            columns.push(plain_column("File Type", "TEXT", Kind::Text));
            columns.push(plain_column("File Data", "BLOB", Kind::Other));
            TableRole::Attachments(t.name.clone(), c.name.clone())
        }
        Some(Complex::MultiValue(ty)) => {
            let value = Column::new("Value", *ty);
            columns.push(plain_column("Value", &declared_type(&value), kind_of(*ty)));
            TableRole::Values(t.name.clone(), c.name.clone())
        }
        None => TableRole::Main,
    };
    let foreign_keys = match &parent_key {
        Some(k) => vec![ForeignKeyPlan {
            relationship: format!("{} {}", t.name, c.name),
            columns: vec![fk_name.clone()],
            target_table: parent.name.clone(),
            target_columns: vec![k.clone()],
            on_update: "CASCADE",
            on_delete: "CASCADE",
        }],
        None => vec![],
    };
    TablePlan {
        access: format!("{}.{}", t.name, c.name),
        name: name.to_string(),
        role,
        columns,
        primary_key: vec!["ID".into()],
        foreign_keys,
        checks: vec![],
        indexes: vec![IndexPlan {
            name: format!("{name} parent"),
            columns: vec![fk_name],
            unique: false,
        }],
        parent_key,
    }
}

/// Adds foreign keys for relationships whose target columns are a key.
pub fn plan_relationships(
    db: &AccessDb,
    plans: &mut [TablePlan],
    notes: &mut BTreeMap<String, Vec<String>>,
) {
    for r in &db.relationships {
        let mut note = |n: String| {
            notes
                .entry(format!("relationship:{}", r.name))
                .or_default()
                .push(n)
        };
        // `Column.Value` points into a multi-value column's values table.
        let (child_access, child_cols): (String, Vec<String>) = match r.columns.as_slice() {
            [c] if c.ends_with(".Value") => (
                format!("{}.{}", r.table, c.trim_end_matches(".Value")),
                vec!["Value".into()],
            ),
            cols => (r.table.clone(), cols.to_vec()),
        };
        let Some(target) = plans
            .iter()
            .find(|p| p.access.eq_ignore_ascii_case(&r.ref_table))
            .cloned()
        else {
            note(format!("{} is not a table", r.ref_table));
            continue;
        };
        let Some(child) = plans
            .iter_mut()
            .find(|p| p.access.eq_ignore_ascii_case(&child_access))
        else {
            note(format!("{} is not a table", r.table));
            continue;
        };
        if !r.enforced() {
            note(
                "referential integrity is not enforced in Access, so no foreign key was created"
                    .into(),
            );
            continue;
        }
        let keyed = target.primary_key.len() == r.ref_columns.len()
            && target
                .primary_key
                .iter()
                .all(|k| r.ref_columns.iter().any(|c| c.eq_ignore_ascii_case(k)))
            || target.indexes.iter().any(|i| {
                i.unique
                    && i.columns.len() == r.ref_columns.len()
                    && i.columns
                        .iter()
                        .all(|k| r.ref_columns.iter().any(|c| c.eq_ignore_ascii_case(k)))
            });
        if !keyed {
            note(format!(
                "{} is not a key of {}",
                r.ref_columns.join(", "),
                r.ref_table
            ));
            continue;
        }
        if !child_cols.iter().all(|c| child.column(c).is_some()) {
            note(format!(
                "{} has no column {}",
                r.table,
                child_cols.join(", ")
            ));
            continue;
        }
        let on_delete = if r.flags & rel_flags::CASCADE_DELETES != 0 {
            "CASCADE"
        } else if r.flags & rel_flags::CASCADE_NULL != 0 {
            "SET NULL"
        } else {
            "NO ACTION"
        };
        let on_update = if r.flags & rel_flags::CASCADE_UPDATES != 0 {
            "CASCADE"
        } else {
            "NO ACTION"
        };
        child.foreign_keys.push(ForeignKeyPlan {
            relationship: r.name.clone(),
            columns: child_cols,
            target_table: target.name.clone(),
            target_columns: r.ref_columns.clone(),
            on_update,
            on_delete,
        });
    }
}

fn column_sql(c: &ColumnPlan, pk_inline: bool) -> String {
    let mut s = format!("{} {}", quote(&c.name), final_declared(c));
    if pk_inline {
        s.push_str(" PRIMARY KEY");
    }
    if c.not_null && !pk_inline {
        s.push_str(" NOT NULL");
    }
    if let Some(d) = &c.default_sql {
        s.push_str(&format!(" DEFAULT {d}"));
    }
    if let Some((check, _)) = &c.check {
        s.push_str(&format!(" CHECK ({check})"));
    }
    s
}

pub fn final_declared(c: &ColumnPlan) -> String {
    match c.conv {
        Conv::DateOnly => "DATE".into(),
        Conv::TimeOnly => "TIME".into(),
        Conv::AsIs => c.declared.clone(),
    }
}

/// `CREATE TABLE` for the final schema.
pub fn create_table_sql(t: &TablePlan) -> String {
    // A single INTEGER key is the rowid, as Access AutoNumber keys expect.
    let inline = t
        .single_key()
        .filter(|k| t.column(k).is_some_and(|c| c.declared == "INTEGER"))
        .map(str::to_string);
    let mut parts: Vec<String> = t
        .columns
        .iter()
        .map(|c| {
            column_sql(
                c,
                inline
                    .as_deref()
                    .is_some_and(|k| k.eq_ignore_ascii_case(&c.name)),
            )
        })
        .collect();
    if inline.is_none() && !t.primary_key.is_empty() {
        parts.push(format!(
            "PRIMARY KEY ({})",
            t.primary_key
                .iter()
                .map(|k| quote(k))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for (check, _) in &t.checks {
        parts.push(format!("CHECK ({check})"));
    }
    for fk in &t.foreign_keys {
        parts.push(format!(
            "FOREIGN KEY ({}) REFERENCES {} ({}) ON UPDATE {} ON DELETE {}",
            fk.columns
                .iter()
                .map(|c| quote(c))
                .collect::<Vec<_>>()
                .join(", "),
            quote(&fk.target_table),
            fk.target_columns
                .iter()
                .map(|c| quote(c))
                .collect::<Vec<_>>()
                .join(", "),
            fk.on_update,
            fk.on_delete
        ));
    }
    format!(
        "CREATE TABLE {} (\n  {}\n);",
        quote(&t.name),
        parts.join(",\n  ")
    )
}

pub fn index_sql(t: &TablePlan, i: &IndexPlan) -> String {
    format!(
        "CREATE {}INDEX {} ON {} ({});",
        if i.unique { "UNIQUE " } else { "" },
        quote(&i.name),
        quote(&t.name),
        i.columns
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(", ")
    )
}

/// Triggers that keep calculated columns current: one per event, updating
/// them in dependency order, and only after updates of other columns (so the
/// trigger's own update does not fire it again).
pub fn calc_triggers_sql(t: &TablePlan) -> Vec<String> {
    let mut calcs: Vec<(&str, &str)> = t
        .columns
        .iter()
        .filter_map(|c| c.calc_sql.as_deref().map(|e| (c.name.as_str(), e)))
        .collect();
    if calcs.is_empty() {
        return vec![];
    }
    // Columns used by other calculated columns come first.
    let mut ordered: Vec<(&str, &str)> = vec![];
    while !calcs.is_empty() {
        let pos = calcs
            .iter()
            .position(|(_, e)| !calcs.iter().any(|(n, _)| e.contains(&quote(n))))
            .unwrap_or(0);
        ordered.push(calcs.remove(pos));
    }
    let updates: String = ordered
        .iter()
        .map(|(n, e)| {
            format!(
                " UPDATE {} SET {} = {e} WHERE rowid = NEW.rowid;",
                quote(&t.name),
                quote(n)
            )
        })
        .collect();
    let base: Vec<String> = t
        .columns
        .iter()
        .filter(|c| c.calc_sql.is_none())
        .map(|c| quote(&c.name))
        .collect();
    vec![
        format!(
            "CREATE TRIGGER {} AFTER INSERT ON {} BEGIN{updates} END;",
            quote(&format!("{} calculated insert", t.name)),
            quote(&t.name)
        ),
        format!(
            "CREATE TRIGGER {} AFTER UPDATE OF {} ON {} BEGIN{updates} END;",
            quote(&format!("{} calculated update", t.name)),
            base.join(", "),
            quote(&t.name)
        ),
    ]
}
