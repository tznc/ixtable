//! Record sources and lookups of forms, reports and fields.
//!
//! A form bound to a query that reads one table (the usual `X Extended` query)
//! is bound to that table so it stays editable; the query's computed columns
//! become computed controls. Lookups whose display column is computed by a
//! query read a SQLite view of that query.
use super::forms::Context;
use super::queries;
use crate::access::model::{ColType, Column, Table};
use crate::access::translate::ast::{parse_statement, Expr, From, Select, Statement};
use crate::access::translate::sql::{natural_name, quote, Dialect, SqlWriter};
use serde_json::json;

/// What a form or report reads.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    None,
    /// An ixtable table (by its document name).
    Table(String),
    /// A saved query (read-only).
    Query {
        id: String,
        columns: Vec<String>,
    },
}

/// How a name on a form maps to its record.
#[derive(Debug, Clone, PartialEq)]
pub enum Field {
    /// A column of the bound table.
    Column(String),
    /// A value the record source computes (ixtable expression).
    Computed(String),
}

/// A form's record source resolved to an editable table where possible.
#[derive(Debug, Clone)]
pub struct Bound {
    pub source: Source,
    /// The Access table the source edits, when it is one table.
    pub table: Option<String>,
    /// Output name → field, for query sources bound to their base table.
    pub fields: Vec<(String, Field)>,
    pub notes: Vec<String>,
}

impl Bound {
    pub fn field(&self, name: &str) -> Option<Field> {
        if let Some((_, f)) = self
            .fields
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            return Some(f.clone());
        }
        None
    }
}

fn select_of(sql: &str) -> Option<Select> {
    match parse_statement(sql).ok()? {
        Statement::Select(s) if s.unions.is_empty() => Some(s),
        _ => None,
    }
}

fn single_table(s: &Select) -> Option<String> {
    match s.from.as_slice() {
        [From::Table { name, .. }] => Some(name.clone()),
        _ => None,
    }
}

impl<'a> Context<'a> {
    pub fn access_table(&self, name: &str) -> Option<&'a Table> {
        self.db.table(name)
    }

    /// The ixtable table name of an Access table.
    pub fn table_name(&self, access: &str) -> Option<String> {
        self.plans
            .iter()
            .find(|p| p.access.eq_ignore_ascii_case(access))
            .map(|p| p.name.clone())
    }

    /// Resolves a form or report `RecordSource`.
    pub fn bind(&mut self, record_source: &str, owner: &str) -> Bound {
        let rs = record_source.trim();
        let mut bound = Bound {
            source: Source::None,
            table: None,
            fields: vec![],
            notes: vec![],
        };
        if rs.is_empty() {
            return bound;
        }
        if let Some(t) = self.access_table(rs) {
            bound.table = Some(t.name.clone());
            bound.source =
                Source::Table(self.table_name(&t.name).unwrap_or_else(|| t.name.clone()));
            bound.fields = t
                .columns
                .iter()
                .map(|c| (c.name.clone(), Field::Column(c.name.clone())))
                .collect();
            return bound;
        }
        let sql = match self.db.query(rs) {
            Some(q) => q.sql.clone(),
            None if rs.to_ascii_uppercase().starts_with("SELECT")
                || rs.to_ascii_uppercase().starts_with("PARAMETERS") =>
            {
                rs.to_string()
            }
            None => {
                bound
                    .notes
                    .push(format!("record source {rs} was not found"));
                return bound;
            }
        };
        // A query over one table binds to the table.
        if let Some((table, fields)) = self.one_table_fields(&sql, 0) {
            bound.table = Some(table.clone());
            bound.source = Source::Table(self.table_name(&table).unwrap_or(table));
            bound.fields = fields;
            if select_of(&sql).is_some_and(|s| s.where_clause.is_some()) {
                bound.notes.push(format!(
                    "the record source {rs} filters rows; the form shows every row of {}",
                    bound.table.clone().unwrap_or_default()
                ));
            }
            return bound;
        }
        match self
            .db
            .query(rs)
            .and_then(|q| self.queries.queries.get(&q.name.to_lowercase()))
            .filter(|q| q.action.is_none())
        {
            Some(q) => {
                bound.source = Source::Query {
                    id: q.id.clone(),
                    columns: q.columns.clone(),
                };
                bound.notes.push(format!(
                    "{rs} joins several tables, so the form is read-only"
                ));
            }
            None => match self.record_source_query(&sql, owner) {
                Ok((id, columns)) => {
                    bound.source = Source::Query { id, columns };
                    bound.notes.push(
                        "the SQL record source is a saved query, so the form is read-only".into(),
                    );
                }
                Err(e) => bound
                    .notes
                    .push(format!("record source not converted: {e}")),
            },
        }
        if let Source::Query { columns, .. } = &bound.source {
            bound.fields = columns
                .iter()
                .map(|c| (c.clone(), Field::Column(c.clone())))
                .collect();
        }
        bound
    }

    /// Saves a SQL record source as a saved query named after its form or report.
    pub fn record_source_query(
        &mut self,
        sql: &str,
        owner: &str,
    ) -> Result<(String, Vec<String>), String> {
        let t = queries::translate(self.db, self.schema, sql, &[], &self.queries.queries)?;
        if t.target.is_some() {
            return Err("a record source must read rows".into());
        }
        let columns = t.columns;
        let q = queries::ConvertedQuery {
            id: uuid::Uuid::now_v7().to_string(),
            name: format!("{owner} (record source)"),
            body: t.body,
            deps: t.deps,
            params: t.params,
            columns: columns.clone(),
            base_table: None,
            action: None,
            creates_table: false,
        };
        let params: Vec<_> = q
            .all_params(&self.queries.queries)
            .iter()
            .map(|p| json!({ "name": p.name, "logicalType": p.logical_type }))
            .collect();
        self.extra_queries.push(json!({ "id": q.id, "name": q.name, "sql": q.sql(&self.queries.queries), "parameters": params }));
        Ok((q.id, columns))
    }

    /// The saved action query for the SQL of a RunSQL macro action.
    pub fn sql_action_query(&self, sql: &str, name: &str) -> Result<serde_json::Value, String> {
        use crate::access::translate::ast::{parse_statement, Statement};
        let kind = match parse_statement(sql)? {
            Statement::Insert(_) => "insert",
            Statement::Update(_) => "update",
            Statement::Delete(_) => "delete",
            _ => return Err("RunSQL runs only INSERT, UPDATE and DELETE here".into()),
        };
        let t = queries::translate(self.db, self.schema, sql, &[], &self.queries.queries)?;
        let target = t.target.ok_or("the statement names no target table")?;
        let q = queries::ConvertedQuery {
            id: uuid::Uuid::now_v7().to_string(),
            name: name.to_string(),
            body: t.body,
            deps: t.deps,
            params: t.params,
            columns: vec![],
            base_table: None,
            action: Some((kind, target)),
            creates_table: false,
        };
        let params: Vec<_> = q
            .all_params(&self.queries.queries)
            .iter()
            .map(|p| json!({ "name": p.name, "logicalType": p.logical_type }))
            .collect();
        Ok(
            json!({ "id": q.id, "name": q.name, "sql": q.sql(&self.queries.queries), "parameters": params, "action": q.action_json() }),
        )
    }

    /// The base table of a one-table query chain and each output column as a column or expression.
    fn one_table_fields(&self, sql: &str, depth: usize) -> Option<(String, Vec<(String, Field)>)> {
        if depth > 8 {
            return None;
        }
        let s = select_of(sql)?;
        let from = single_table(&s)?;
        let (table, inner): (String, Vec<(String, Field)>) = match self.access_table(&from) {
            Some(t) => (
                t.name.clone(),
                t.columns
                    .iter()
                    .map(|c| (c.name.clone(), Field::Column(c.name.clone())))
                    .collect(),
            ),
            None => self.one_table_fields(&self.db.query(&from)?.sql, depth + 1)?,
        };
        let lookup = |name: &str| {
            inner
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, f)| f.clone())
        };
        let mut out = vec![];
        for (i, (e, alias)) in s.columns.iter().enumerate() {
            match e {
                Expr::Star(_) => out.extend(inner.iter().cloned()),
                Expr::Name(parts)
                    if alias.is_none()
                        || alias.as_deref() == parts.last().map(|p| p.text.as_str()) =>
                {
                    let n = &parts.last()?.text;
                    out.push((n.clone(), lookup(n)?));
                }
                other => {
                    let name = alias
                        .clone()
                        .or_else(|| natural_name(other))
                        .unwrap_or_else(|| format!("Expr{}", 1000 + i));
                    let inner_ref = &inner;
                    let resolve = |n: &str| match inner_ref
                        .iter()
                        .find(|(x, _)| x.eq_ignore_ascii_case(n))
                        .map(|(_, f)| f.clone())
                    {
                        Some(Field::Column(c)) => {
                            Some(crate::access::translate::expr::field("record", &c))
                        }
                        Some(Field::Computed(e)) => Some(format!("({e})")),
                        None => None,
                    };
                    let mut w = crate::access::translate::expr::ExprWriter::new(
                        crate::access::translate::expr::Target::Form,
                        &resolve,
                    );
                    if let Ok(expr) = w.write(other) {
                        out.push((name, Field::Computed(expr)));
                    }
                }
            }
        }
        Some((table, out))
    }

    /// Creates (once) a SQLite view of an Access query for lookups.
    pub fn ensure_view(&mut self, query: &str, depth: usize) -> Result<String, String> {
        let q = self
            .db
            .query(query)
            .ok_or_else(|| format!("{query} is not a query"))?;
        let (name, sql) = (q.name.clone(), q.sql.clone());
        self.view_of(&name, &sql, depth)
    }

    /// A view of a lookup's own SQL.
    pub fn ensure_sql_view(&mut self, name: &str, sql: &str) -> Result<String, String> {
        let mut name = name.to_string();
        let mut n = 2;
        while self.db.table(&name).is_some() || self.db.query(&name).is_some() {
            name = format!("{name} {n}");
            n += 1;
        }
        self.view_of(&name, sql, 0)
    }

    fn view_of(&mut self, name: &str, sql: &str, depth: usize) -> Result<String, String> {
        let name = name.to_string();
        if self.view_names.contains(&name.to_lowercase()) {
            return Ok(name);
        }
        if depth > 8 {
            return Err("queries nest too deeply".into());
        }
        let st = parse_statement(sql)?;
        let mut w = SqlWriter::new(Dialect::Sqlite, self.schema);
        let sql = w.statement(&st)?;
        if !w.out.params.is_empty() {
            return Err(format!("{name} has parameters"));
        }
        for dep in w.out.queries.clone() {
            self.ensure_view(&dep, depth + 1)?;
        }
        self.lookup_views
            .push(format!("CREATE VIEW {} AS {sql};", quote(&name)));
        self.view_names.insert(name.to_lowercase());
        Ok(name)
    }

    /// A lookup from `RowSourceType`/`RowSource` properties.
    #[allow(clippy::too_many_arguments)]
    pub fn lookup(
        &mut self,
        owner: &str,
        row_source_type: &str,
        row_source: &str,
        bound_column: i64,
        column_widths: &str,
        column_count: i64,
    ) -> Option<Lookup> {
        let rs = row_source.trim();
        if rs.is_empty() {
            return None;
        }
        if row_source_type.eq_ignore_ascii_case("Value List") {
            return Some(Lookup::Options(value_list(
                rs,
                column_count.max(1) as usize,
                bound_column.max(1) as usize,
            )));
        }
        if !row_source_type.is_empty() && !row_source_type.eq_ignore_ascii_case("Table/Query") {
            return None;
        }
        let sql = if self.access_table(rs).is_some() || self.db.query(rs).is_some() {
            format!("SELECT * FROM [{rs}]")
        } else {
            rs.to_string()
        };
        let s = select_of(&sql)?;
        let from = single_table(&s)?;
        // Output columns of the lookup's SELECT.
        let cols: Vec<String> = if s.columns.iter().any(|(e, _)| matches!(e, Expr::Star(_))) {
            self.source_columns(&from)
        } else {
            s.columns
                .iter()
                .enumerate()
                .map(|(i, (e, a))| {
                    a.clone()
                        .or_else(|| natural_name(e))
                        .unwrap_or_else(|| format!("Expr{}", 1000 + i))
                })
                .collect()
        };
        let widths = parse_widths(column_widths);
        let bound = (bound_column.max(1) - 1) as usize;
        let value = cols.get(bound)?.clone();
        let display = cols
            .iter()
            .enumerate()
            .find(|(i, _)| {
                *i != bound
                    && widths.get(*i).is_none_or(|w| *w > 0)
                    && (*i as i64) < column_count.max(1)
            })
            .or_else(|| cols.iter().enumerate().find(|(i, _)| *i != bound))
            .map(|(_, c)| c.clone())
            .unwrap_or_else(|| value.clone());
        let computed: Vec<String> = s
            .columns
            .iter()
            .filter_map(|(e, a)| match e {
                Expr::Name(_) | Expr::Star(_) => None,
                _ => a.clone(),
            })
            .collect();
        let plain = |c: &str| !computed.iter().any(|x| x.eq_ignore_ascii_case(c));
        // Simple columns of one table: look up the table itself.
        if let Some(t) = self.access_table(&from) {
            if plain(&value)
                && plain(&display)
                && t.column(&value).is_some()
                && t.column(&display).is_some()
            {
                return Some(Lookup::Table {
                    table: self.table_name(&t.name)?,
                    value,
                    display,
                });
            }
        }
        // Otherwise read a view: of the saved query, or of the row source SQL itself.
        let view = match self.db.query(rs) {
            Some(q) => self.ensure_view(&q.name.clone(), 0),
            None => self.ensure_sql_view(&format!("{owner} lookup"), &sql),
        };
        match view {
            Ok(view) => Some(Lookup::Table {
                table: view,
                value,
                display,
            }),
            Err(_) => self.table_fallback(&from, &value),
        }
    }

    fn table_fallback(&self, from: &str, value: &str) -> Option<Lookup> {
        let t = self.access_table(from)?;
        let display = best_display(t, value)?;
        Some(Lookup::Table {
            table: self.table_name(&t.name)?,
            value: value.to_string(),
            display,
        })
    }

    fn source_columns(&self, source: &str) -> Vec<String> {
        use crate::access::translate::sql::Schema;
        self.schema
            .columns(source)
            .unwrap_or_default()
            .into_iter()
            .map(|(c, _)| c)
            .collect()
    }

    /// A field's own lookup (table design `Lookup` tab).
    pub fn column_lookup(&mut self, c: &Column) -> Option<Lookup> {
        let display_control = c
            .prop("DisplayControl")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        if !matches!(display_control, 110 | 111) {
            return None;
        }
        let rst = c.prop("RowSourceType").unwrap_or("Table/Query").to_string();
        let rs = c.prop("RowSource")?.to_string();
        let bound = c
            .prop("BoundColumn")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let count = c
            .prop("ColumnCount")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let widths = c.prop("ColumnWidths").unwrap_or("").to_string();
        let owner = c.name.clone();
        self.lookup(&owner, &rst, &rs, bound, &widths, count)
    }
}

/// A lookup control's choices.
#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    Table {
        table: String,
        value: String,
        display: String,
    },
    Options(Vec<(String, String)>),
}

/// The first text column that is not the key, for showing a related row.
pub fn best_display(t: &Table, key: &str) -> Option<String> {
    t.columns
        .iter()
        .find(|c| {
            !c.name.eq_ignore_ascii_case(key)
                && matches!(c.ty, ColType::Text)
                && c.complex.is_none()
        })
        .or_else(|| {
            t.columns
                .iter()
                .find(|c| !c.name.eq_ignore_ascii_case(key) && c.complex.is_none())
        })
        .map(|c| c.name.clone())
}

/// `ColumnWidths` in twips ("0;1440", "0\";1\"", "0cm;2.54cm").
pub fn parse_widths(s: &str) -> Vec<i64> {
    s.split(';')
        .map(|w| {
            let w = w.trim();
            let num: String = w
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            let v: f64 = num.parse().unwrap_or(-1.0);
            if w.ends_with('"') || w.ends_with("in") {
                (v * 1440.0) as i64
            } else if w.ends_with("cm") {
                (v * 567.0) as i64
            } else {
                v as i64
            }
        })
        .collect()
}

/// `"A";"B"` or `1;"One";2;"Two"` → (value, label) pairs.
pub fn value_list(s: &str, columns: usize, bound: usize) -> Vec<(String, String)> {
    let items: Vec<String> = crate::access::query_def::split_top_level(s, ';')
        .into_iter()
        .map(|x| x.trim().trim_matches('"').trim_matches('\'').to_string())
        .collect();
    let cols = columns.max(1);
    items
        .chunks(cols)
        .filter(|row| !row.is_empty())
        .map(|row| {
            let value = row
                .get(bound - 1)
                .or(row.first())
                .cloned()
                .unwrap_or_default();
            let label = row
                .iter()
                .enumerate()
                .find(|(i, _)| *i != bound - 1)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| value.clone());
            (value, label)
        })
        .collect()
}
