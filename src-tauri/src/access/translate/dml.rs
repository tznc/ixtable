//! Access action statements → DuckDB action-query SQL
//! (`docs/decisions/action-queries.md`).
//!
//! Access joins the tables of an UPDATE or DELETE in its FROM clause; DuckDB
//! names the target alone and lists the other tables in `FROM` (UPDATE) or
//! `USING` (DELETE), with the join conditions moved to WHERE. That is exact
//! for inner joins only, so outer joins are refused.
use super::ast::*;
use super::sql::{quote, Scope, SqlWriter};

/// The tables of a FROM tree with the conditions that joined them.
fn flatten(f: &From, tables: &mut Vec<From>, on: &mut Vec<Expr>) -> Result<(), String> {
    match f {
        From::Join {
            kind: JoinKind::Inner,
            left,
            right,
            on: cond,
        } => {
            flatten(left, tables, on)?;
            flatten(right, tables, on)?;
            on.push(cond.clone());
            Ok(())
        }
        From::Join { .. } => Err("outer joins in an UPDATE or DELETE are not supported".into()),
        other => {
            tables.push(other.clone());
            Ok(())
        }
    }
}

fn key(f: &From) -> Option<&str> {
    match f {
        From::Table { name, alias } => Some(alias.as_deref().unwrap_or(name)),
        From::Sub { alias, .. } => alias.as_deref(),
        From::Join { .. } => None,
    }
}

/// Picks the table rows are written to: the one `qualifier` names, else the first.
fn pick(
    tables: &mut Vec<From>,
    qualifier: Option<&str>,
) -> Result<(String, Option<String>), String> {
    let index = qualifier
        .and_then(|q| {
            tables.iter().position(|t| {
                key(t).is_some_and(|k| k.eq_ignore_ascii_case(q))
                    || matches!(t, From::Table { name, .. } if name.eq_ignore_ascii_case(q))
            })
        })
        .unwrap_or(0);
    if index >= tables.len() {
        return Err("the statement names no table".into());
    }
    match tables.remove(index) {
        From::Table { name, alias } => Ok((name, alias)),
        _ => Err("an action query must write to a table".into()),
    }
}

impl SqlWriter<'_> {
    fn open_scope(&mut self) {
        self.scopes.push(Scope {
            sources: vec![],
            complex: vec![],
            aliases: vec![],
        });
    }

    /// Renders the target and the other tables in the current scope.
    fn sources(
        &mut self,
        target: (String, Option<String>),
        others: &[From],
    ) -> Result<(String, Vec<String>), String> {
        if self.schema.is_query(&target.0) || self.schema.columns(&target.0).is_none() {
            return Err(format!("\"{}\" is not a table of this database", target.0));
        }
        self.out.target = Some(target.0.clone());
        let target = self.from(&From::Table {
            name: target.0,
            alias: target.1,
        })?;
        let mut rendered = vec![];
        for t in others {
            rendered.push(self.from(t)?);
        }
        Ok((target, rendered))
    }

    fn conditions(&mut self, on: &[Expr], where_clause: &Option<Expr>) -> Result<String, String> {
        let mut parts = vec![];
        for e in on.iter().chain(where_clause.iter()) {
            parts.push(format!("({})", self.expr(e)?));
        }
        Ok(if parts.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", parts.join(" AND "))
        })
    }

    /// `INSERT INTO t (cols) SELECT ...`; without a column list, by name.
    pub(super) fn insert(&mut self, i: &Insert) -> Result<String, String> {
        if self.schema.is_query(&i.table) || self.schema.columns(&i.table).is_none() {
            return Err(format!("\"{}\" is not a table of this database", i.table));
        }
        self.out.target = Some(i.table.clone());
        let columns = i
            .columns
            .iter()
            .map(|c| quote(c))
            .collect::<Vec<_>>()
            .join(", ");
        let into = if i.columns.is_empty() {
            format!("INSERT INTO {}", quote(&i.table))
        } else {
            format!("INSERT INTO {} ({columns})", quote(&i.table))
        };
        match &i.source {
            InsertSource::Select(s) => {
                let select = self.select(s)?;
                let by_name = if i.columns.is_empty() { " BY NAME" } else { "" };
                Ok(format!("{into}{by_name} {select}"))
            }
            InsertSource::Values(values) => {
                self.open_scope();
                let rendered: Result<Vec<String>, String> =
                    values.iter().map(|v| self.expr(v)).collect();
                self.scopes.pop();
                Ok(format!("{into} VALUES ({})", rendered?.join(", ")))
            }
        }
    }

    /// `UPDATE t SET col = value FROM others WHERE joins AND where`.
    pub(super) fn update(&mut self, u: &Update) -> Result<String, String> {
        let mut tables = vec![];
        let mut on = vec![];
        flatten(&u.from, &mut tables, &mut on)?;
        let qualifier = u
            .sets
            .iter()
            .find_map(|(t, _)| (t.len() > 1).then(|| t[t.len() - 2].text.clone()));
        let target = pick(&mut tables, qualifier.as_deref())?;
        self.open_scope();
        let result = (|| {
            let (target, others) = self.sources(target, &tables)?;
            let mut sets = vec![];
            for (column, value) in &u.sets {
                let name = column.last().map(|p| p.text.clone()).unwrap_or_default();
                sets.push(format!("{} = {}", quote(&name), self.expr(value)?));
            }
            let from = if others.is_empty() {
                String::new()
            } else {
                format!(" FROM {}", others.join(", "))
            };
            let filter = self.conditions(&on, &u.where_clause)?;
            Ok(format!(
                "UPDATE {target} SET {}{from}{filter}",
                sets.join(", ")
            ))
        })();
        self.scopes.pop();
        result
    }

    /// `DELETE FROM t USING others WHERE joins AND where`.
    pub(super) fn delete(&mut self, d: &Delete) -> Result<String, String> {
        let mut tables = vec![];
        let mut on = vec![];
        for f in &d.from {
            flatten(f, &mut tables, &mut on)?;
        }
        let target = pick(&mut tables, d.target.as_deref())?;
        self.open_scope();
        let result = (|| {
            let (target, others) = self.sources(target, &tables)?;
            let using = if others.is_empty() {
                String::new()
            } else {
                format!(" USING {}", others.join(", "))
            };
            let filter = self.conditions(&on, &d.where_clause)?;
            Ok(format!("DELETE FROM {target}{using}{filter}"))
        })();
        self.scopes.pop();
        result
    }
}
