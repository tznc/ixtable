//! Access saved queries → ixtable saved queries (DuckDB SQL).
//!
//! A query that reads other saved queries gets them as CTEs, in dependency
//! order, so each saved query stands alone. Append, update and delete queries
//! become action queries (`docs/decisions/action-queries.md`); a make-table
//! query becomes a replace query whose target table the import creates when
//! the file has none. Data-definition and pass-through queries have no
//! equivalent; their Access SQL is kept in the report and document settings.
use super::report::{ImportReport, Notes, Status};
use super::schema::DbSchema;
use crate::access::model::{AccessDb, QueryKind};
use crate::access::translate::ast::parse_statement;
use crate::access::translate::sql::{quote, Dialect, Kind, Param, SqlWriter};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// A converted query, by lower-case Access name.
#[derive(Debug, Clone)]
pub struct ConvertedQuery {
    pub id: String,
    pub name: String,
    /// The query body without CTEs.
    pub body: String,
    /// Saved queries it reads, transitively, dependencies first.
    pub deps: Vec<String>,
    pub params: Vec<Param>,
    pub columns: Vec<String>,
    /// The base table when the query reads exactly one table.
    pub base_table: Option<String>,
    /// For an action query: (kind, target table) as `ActionSpec` names them.
    pub action: Option<(&'static str, String)>,
    /// A make-table query whose target the file does not have: the import creates it.
    pub creates_table: bool,
}

impl ConvertedQuery {
    /// The standalone SQL with every dependency as a CTE.
    pub fn sql(&self, all: &BTreeMap<String, ConvertedQuery>) -> String {
        if self.deps.is_empty() {
            return self.body.clone();
        }
        let ctes: Vec<String> = self
            .deps
            .iter()
            .filter_map(|d| all.get(d))
            .map(|q| format!("{} AS ({})", quote(&q.name), q.body))
            .collect();
        format!("WITH {} {}", ctes.join(", "), self.body)
    }

    /// The `action` value of the saved query.
    pub fn action_json(&self) -> Option<Value> {
        self.action
            .as_ref()
            .map(|(kind, table)| json!({ "kind": kind, "table": table }))
    }

    pub fn all_params(&self, all: &BTreeMap<String, ConvertedQuery>) -> Vec<Param> {
        let mut out = self.params.clone();
        for d in &self.deps {
            for p in all.get(d).map(|q| q.params.clone()).unwrap_or_default() {
                if !out.iter().any(|x| x.name == p.name) {
                    out.push(p);
                }
            }
        }
        out
    }
}

pub struct QueryConversion {
    pub queries: BTreeMap<String, ConvertedQuery>,
    /// Access queries kept as SQL only: (name, kind, Access SQL).
    pub unconverted: Vec<(String, QueryKind, String)>,
}

/// One translated statement.
pub struct Translated {
    pub body: String,
    pub deps: Vec<String>,
    pub params: Vec<Param>,
    pub columns: Vec<String>,
    /// The table an action statement writes or a make-table query creates.
    pub target: Option<String>,
}

/// The `ActionSpec` kind of an Access query kind, for those that become action queries.
pub fn action_kind(kind: QueryKind) -> Option<&'static str> {
    match kind {
        QueryKind::Append => Some("insert"),
        QueryKind::Update => Some("update"),
        QueryKind::Delete => Some("delete"),
        QueryKind::MakeTable => Some("replace"),
        _ => None,
    }
}

fn converts(kind: QueryKind) -> bool {
    matches!(
        kind,
        QueryKind::Select | QueryKind::Union | QueryKind::Crosstab
    ) || action_kind(kind).is_some()
}

/// Translates one Access SQL text; dependencies must already be converted.
pub fn translate(
    db: &AccessDb,
    schema: &DbSchema,
    sql: &str,
    declared: &[(String, String)],
    done: &BTreeMap<String, ConvertedQuery>,
) -> Result<Translated, String> {
    let st = parse_statement(sql)?;
    let mut w = SqlWriter::new(Dialect::DuckDb, schema);
    w.declare(declared);
    let body = w.statement(&st)?;
    let mut deps: Vec<String> = vec![];
    for q in &w.out.queries {
        let key = q.to_lowercase();
        let Some(dep) = done.get(&key) else {
            return Err(format!("it reads the query {q}, which was not converted"));
        };
        for d in &dep.deps {
            if !deps.contains(d) {
                deps.push(d.clone());
            }
        }
        if !deps.contains(&key) {
            deps.push(key);
        }
    }
    let _ = db;
    Ok(Translated {
        body,
        deps,
        params: w.out.params,
        columns: w.out.columns,
        target: w.out.target,
    })
}

/// The single table a query reads, following other queries.
fn base_table(db: &AccessDb, sql: &str) -> Option<String> {
    use crate::access::translate::ast::{From, Statement};
    let Ok(Statement::Select(s)) = parse_statement(sql) else {
        return None;
    };
    if !s.unions.is_empty() {
        return None;
    }
    let mut tables = vec![];
    fn walk(f: &From, out: &mut Vec<String>) {
        match f {
            From::Table { name, .. } => out.push(name.clone()),
            From::Join { left, right, .. } => {
                walk(left, out);
                walk(right, out);
            }
            From::Sub { .. } => out.push(String::new()),
        }
    }
    for f in &s.from {
        walk(f, &mut tables);
    }
    match tables.as_slice() {
        [one] if db.table(one).is_some() => db.table(one).map(|t| t.name.clone()),
        [one] => db.query(one).and_then(|q| base_table(db, &q.sql)),
        _ => None,
    }
}

pub fn convert(db: &AccessDb, schema: &mut DbSchema, report: &mut ImportReport) -> QueryConversion {
    let mut done: BTreeMap<String, ConvertedQuery> = BTreeMap::new();
    let mut failed: BTreeMap<String, String> = BTreeMap::new();
    // Convert in rounds until no more queries can be converted (dependency order).
    let mut pending: Vec<&crate::access::model::Query> =
        db.queries.iter().filter(|q| converts(q.kind)).collect();
    loop {
        let before = pending.len();
        let mut next = vec![];
        for q in pending {
            match translate(db, schema, &q.sql, &q.parameters, &done).and_then(|t| {
                let kind = action_kind(q.kind);
                match (kind, &t.target) {
                    (Some(_), None) => Err("the statement names no target table".into()),
                    (None, Some(_)) if q.kind != QueryKind::MakeTable => {
                        Err(format!("{:?} query with an INTO clause", q.kind))
                    }
                    _ => Ok((t, kind)),
                }
            }) {
                Ok((t, kind)) => {
                    if kind.is_none() {
                        schema.query_columns.insert(
                            q.name.to_lowercase(),
                            t.columns.iter().map(|c| (c.clone(), Kind::Other)).collect(),
                        );
                    }
                    let creates_table = q.kind == QueryKind::MakeTable
                        && t.target.as_deref().is_some_and(|n| db.table(n).is_none());
                    done.insert(
                        q.name.to_lowercase(),
                        ConvertedQuery {
                            id: uuid::Uuid::now_v7().to_string(),
                            name: q.name.clone(),
                            body: t.body,
                            deps: t.deps,
                            params: t.params,
                            columns: t.columns,
                            base_table: kind.is_none().then(|| base_table(db, &q.sql)).flatten(),
                            action: kind.zip(t.target),
                            creates_table,
                        },
                    );
                }
                Err(e) if e.contains("was not converted") => {
                    failed.insert(q.name.to_lowercase(), e);
                    next.push(q);
                }
                Err(e) => {
                    failed.insert(q.name.to_lowercase(), e);
                }
            }
        }
        if next.is_empty() || next.len() == before {
            break;
        }
        pending = next;
    }
    for q in &db.queries {
        let key = q.name.to_lowercase();
        if let Some(c) = done.get(&key) {
            let mut notes = Notes::default();
            if q.kind == QueryKind::Crosstab {
                notes.push(
                    "crosstab converted to a DuckDB PIVOT; column headings come from the data",
                );
            }
            if c.creates_table {
                notes.push(format!(
                    "make-table query: the import creates the table {} and the query replaces its rows",
                    c.action.as_ref().map(|a| a.1.as_str()).unwrap_or("")
                ));
            }
            for p in &c.params {
                if p.original.contains('!') {
                    notes.push(format!(
                        "the form reference {} became the parameter ${}",
                        p.original, p.name
                    ));
                }
            }
            report.add("query", &q.name, notes.status(), notes.0);
        } else if let Some(e) = failed.get(&key) {
            report.add(
                "query",
                &q.name,
                Status::Skipped,
                vec![
                    format!("not converted: {e}"),
                    format!("Access SQL: {}", q.sql.trim()),
                ],
            );
        } else {
            let why = match q.kind {
                QueryKind::DataDefinition => "data-definition queries become migrations",
                _ => "pass-through queries run on a server ixtable does not connect to",
            };
            report.add(
                "query",
                &q.name,
                Status::Skipped,
                vec![why.to_string(), format!("Access SQL: {}", q.sql.trim())],
            );
        }
    }
    let unconverted = db
        .queries
        .iter()
        .filter(|q| {
            !matches!(
                q.kind,
                QueryKind::Select | QueryKind::Union | QueryKind::Crosstab
            ) && !done.contains_key(&q.name.to_lowercase())
        })
        .map(|q| (q.name.clone(), q.kind, q.sql.clone()))
        .collect();
    QueryConversion {
        queries: done,
        unconverted,
    }
}

/// Saved query definitions for the document config.
pub fn saved_queries(conv: &QueryConversion) -> Vec<Value> {
    let mut list: Vec<&ConvertedQuery> = conv.queries.values().collect();
    list.sort_by_key(|q| q.name.to_lowercase());
    list.iter()
        .map(|q| {
            let params: Vec<Value> = q
                .all_params(&conv.queries)
                .iter()
                .map(|p| json!({ "name": p.name, "logicalType": p.logical_type }))
                .collect();
            let mut query = json!({ "id": q.id, "name": q.name, "sql": q.sql(&conv.queries), "parameters": params });
            if let Some(action) = q.action_json() {
                query["action"] = action;
            }
            query
        })
        .collect()
}
