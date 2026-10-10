//! Access SQL → DuckDB SQL (saved queries, report datasets) and Access
//! expressions → SQLite SQL (CHECK constraints, defaults, calculated columns).
//!
//! Names are resolved against the sources in scope. A bracketed name that is
//! no column becomes a query parameter, as Access prompts for it; so do form
//! references (`Forms!Orders!OrderID`). Saved queries used as sources are
//! inlined as CTEs by the caller (see [`Translation::queries`]).
use super::ast::*;
use super::functions;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    DuckDb,
    Sqlite,
}

/// Column kinds the translator needs to pick literals and functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Number,
    Bool,
    Date,
    Other,
}

/// Tables and queries visible to a query: name → (column, kind).
pub trait Schema {
    fn columns(&self, source: &str) -> Option<Vec<(String, Kind)>>;
    fn is_query(&self, source: &str) -> bool;
    /// Attachment and multi-value columns, which live in child tables.
    fn complex(&self, _source: &str) -> Vec<String> {
        vec![]
    }
}

/// A parameter the translated SQL refers to as `$name`.
#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    /// The Access text (`Beginning Date`, `Forms!Orders!OrderID`).
    pub original: String,
    /// ixtable logical type name.
    pub logical_type: String,
}

/// What a translation produced besides the SQL text.
#[derive(Debug, Default, Clone)]
pub struct Translation {
    pub params: Vec<Param>,
    /// Saved queries used as sources, which the caller must define as CTEs.
    pub queries: Vec<String>,
    /// Output column names of a SELECT.
    pub columns: Vec<String>,
    /// The table an INSERT, UPDATE or DELETE writes, or a make-table query creates.
    pub target: Option<String>,
    pub notes: Vec<String>,
}

pub(super) struct Scope {
    /// (name or alias, columns)
    pub(super) sources: Vec<(String, Vec<(String, Kind)>)>,
    /// Complex columns per source name.
    pub(super) complex: Vec<(String, Vec<String>)>,
    /// Select-list aliases with their expressions (Access lets any clause use them).
    pub(super) aliases: Vec<(String, Expr)>,
}

pub struct SqlWriter<'a> {
    pub dialect: Dialect,
    pub(super) schema: &'a dyn Schema,
    pub(super) scopes: Vec<Scope>,
    pub out: Translation,
    declared: BTreeMap<String, String>,
    /// For field rules and defaults: the table whose columns are bare names.
    pub table_columns: Vec<(String, Kind)>,
    /// Aliases being inlined (guards against self reference).
    inlining: Vec<String>,
    /// True while translating a column DEFAULT (SQLite keywords for now).
    pub in_default: bool,
    /// Placeholder names standing for outer-query SQL in a domain function's
    /// criteria (`DLookup(..., "ID=" & [ID])`): (name, SQL).
    pub(super) outer: Vec<(String, String)>,
    /// Qualify bare column names with their source (outer references).
    pub(super) qualify: bool,
}

pub fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn string_literal(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Parameter names must be plain identifiers.
pub fn param_name(original: &str) -> String {
    let mut s: String = original
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    while s.contains("__") {
        s = s.replace("__", "_");
    }
    let s = s.trim_matches('_').to_string();
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) {
        format!("p_{s}")
    } else {
        s
    }
}

/// Access PARAMETERS type → ixtable logical type.
pub fn logical_param_type(access: &str) -> String {
    let t = access.to_ascii_lowercase();
    let t = t.split('(').next().unwrap_or("").trim();
    match t {
        "bit" | "yesno" | "logical" | "boolean" => "boolean",
        "byte" | "short" | "long" | "integer" | "int" | "smallint" | "counter"
        | "autoincrement" | "bigint" => "integer",
        "currency" | "money" | "ieeesingle" | "single" | "ieeedouble" | "double" | "float"
        | "real" | "decimal" | "numeric" | "number" => "number",
        "datetime" | "date" | "time" => "date",
        _ => "text",
    }
    .to_string()
}

impl<'a> SqlWriter<'a> {
    pub fn new(dialect: Dialect, schema: &'a dyn Schema) -> Self {
        Self {
            dialect,
            schema,
            scopes: vec![],
            out: Translation::default(),
            declared: BTreeMap::new(),
            table_columns: vec![],
            inlining: vec![],
            in_default: false,
            outer: vec![],
            qualify: false,
        }
    }

    /// Declared `PARAMETERS` (name, Access type).
    pub fn declare(&mut self, params: &[(String, String)]) {
        for (n, t) in params {
            self.declared
                .insert(n.to_lowercase(), logical_param_type(t));
        }
    }

    fn param(&mut self, original: &str) -> String {
        let name = param_name(original);
        if !self.out.params.iter().any(|p| p.name == name) {
            let logical_type = self
                .declared
                .get(&original.to_lowercase())
                .cloned()
                .unwrap_or_else(|| "text".into());
            self.out.params.push(Param {
                name: name.clone(),
                original: original.to_string(),
                logical_type,
            });
        }
        format!("${name}")
    }

    fn source_columns(&mut self, name: &str) -> Vec<(String, Kind)> {
        if self.schema.is_query(name)
            && !self
                .out
                .queries
                .iter()
                .any(|q| q.eq_ignore_ascii_case(name))
        {
            self.out.queries.push(name.to_string());
        }
        self.schema.columns(name).unwrap_or_default()
    }

    pub fn kind_of(&self, e: &Expr) -> Kind {
        match e {
            Expr::Str(_) => Kind::Text,
            Expr::Num(_) => Kind::Number,
            Expr::Bool(_) => Kind::Bool,
            Expr::Date(_) => Kind::Date,
            Expr::Neg(i) | Expr::Paren(i) => self.kind_of(i),
            Expr::Name(parts) => match self.resolve(parts) {
                Some((_, k)) => k,
                None => match parts.as_slice() {
                    [one] => self
                        .alias_expr(&one.text)
                        .map(|e| self.kind_of(&e))
                        .unwrap_or(Kind::Other),
                    _ => Kind::Other,
                },
            },
            Expr::Bin(BinOp::Concat, _, _) => Kind::Text,
            Expr::Bin(BinOp::Add | BinOp::Sub, l, r) => match (self.kind_of(l), self.kind_of(r)) {
                (Kind::Date, Kind::Date) => Kind::Number,
                (Kind::Date, _) | (_, Kind::Date) => Kind::Date,
                (Kind::Text, _) | (_, Kind::Text) => Kind::Other,
                _ => Kind::Number,
            },
            Expr::Bin(BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::IntDiv | BinOp::Pow, _, _) => {
                Kind::Number
            }
            Expr::Bin(..)
            | Expr::Not(_)
            | Expr::Like { .. }
            | Expr::Between { .. }
            | Expr::In { .. }
            | Expr::IsNull { .. } => Kind::Bool,
            Expr::Call { name, args, .. } => {
                let n = name.to_ascii_lowercase();
                match n.as_str() {
                    "date" | "now" | "dateadd" | "dateserial" | "cdate" | "datevalue" => Kind::Date,
                    "sum" | "avg" | "count" | "dcount" | "dsum" | "davg" | "year" | "month"
                    | "day" | "hour" | "minute" | "second" | "weekday" | "datediff"
                    | "datepart" | "len" | "instr" | "val" | "int" | "fix" | "abs" | "round"
                    | "cint" | "clng" | "cdbl" | "csng" | "ccur" | "cdec" | "sgn" | "sqr"
                    | "instrrev" | "strcomp" | "rnd" | "timer" | "atn" | "sin" | "cos"
                    | "tan" => {
                        Kind::Number
                    }
                    "min" | "max" | "first" | "last" | "nz" | "dmin" | "dmax" | "dfirst"
                    | "dlast" => args.first().map(|a| self.kind_of(a)).unwrap_or(Kind::Other),
                    "iif" => match (
                        args.get(1).map(|a| self.kind_of(a)),
                        args.get(2).map(|a| self.kind_of(a)),
                    ) {
                        (Some(k), _) if k != Kind::Other => k,
                        (_, Some(k)) => k,
                        _ => Kind::Other,
                    },
                    "format" | "left" | "right" | "mid" | "trim" | "ltrim" | "rtrim" | "ucase"
                    | "lcase" | "cstr" | "replace" | "plaintext" | "strconv" | "monthname"
                    | "weekdayname" | "formatcurrency" | "formatnumber" | "formatpercent"
                    | "formatdatetime" | "partition" | "hex" | "oct" | "strreverse" => Kind::Text,
                    _ => Kind::Other,
                }
            }
            _ => Kind::Other,
        }
    }

    fn alias_expr(&self, name: &str) -> Option<Expr> {
        self.scopes
            .last()?
            .aliases
            .iter()
            .find(|(a, _)| a.eq_ignore_ascii_case(name))
            .map(|(_, e)| e.clone())
    }

    /// True when `parts` names an attachment or multi-value column.
    fn is_complex(&self, parts: &[Part]) -> bool {
        let Some(scope) = self.scopes.last() else {
            return false;
        };
        let has = |src: &str, col: &str| {
            scope.complex.iter().any(|(s, cols)| {
                (src.is_empty() || s.eq_ignore_ascii_case(src))
                    && cols.iter().any(|c| c.eq_ignore_ascii_case(col))
            })
        };
        match parts {
            [one] => has("", &one.text),
            [t, c] => has(&t.text, &c.text),
            [t, c, v] if v.text.eq_ignore_ascii_case("Value") => has(&t.text, &c.text),
            _ => false,
        }
    }

    /// Finds a column: (SQL reference, kind).
    fn resolve(&self, parts: &[Part]) -> Option<(String, Kind)> {
        let find = |cols: &[(String, Kind)], name: &str| {
            cols.iter()
                .find(|(c, _)| c.eq_ignore_ascii_case(name))
                .cloned()
        };
        match parts {
            [one] => {
                for scope in self.scopes.iter().rev() {
                    for (src, cols) in &scope.sources {
                        if let Some((c, k)) = find(cols, &one.text) {
                            if self.qualify {
                                return Some((format!("{}.{}", quote(src), quote(&c)), k));
                            }
                            return Some((quote(&c), k));
                        }
                    }
                }
                find(&self.table_columns, &one.text).map(|(c, k)| (quote(&c), k))
            }
            // `Table.Field` and `Table!Field` mean the same in Access SQL.
            [table, col] => {
                for scope in self.scopes.iter().rev() {
                    for (src, cols) in &scope.sources {
                        if src.eq_ignore_ascii_case(&table.text) {
                            let (c, k) =
                                find(cols, &col.text).unwrap_or((col.text.clone(), Kind::Other));
                            return Some((format!("{}.{}", quote(src), quote(&c)), k));
                        }
                    }
                }
                None
            }
            // `Query.Table.Field`: a query column Access named `Table.Field`.
            [query, table, col] => {
                let name = format!("{}.{}", table.text, col.text);
                for scope in self.scopes.iter().rev() {
                    for (src, cols) in &scope.sources {
                        if src.eq_ignore_ascii_case(&query.text) {
                            if let Some((c, k)) = find(cols, &name) {
                                return Some((format!("{}.{}", quote(src), quote(&c)), k));
                            }
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub fn expr(&mut self, e: &Expr) -> Result<String, String> {
        let duck = self.dialect == Dialect::DuckDb;
        Ok(match e {
            Expr::Null => "NULL".into(),
            Expr::Bool(b) => match (duck, b) {
                (true, true) => "TRUE".into(),
                (true, false) => "FALSE".into(),
                (false, b) => (*b as i32).to_string(),
            },
            Expr::Num(n) => n.clone(),
            Expr::Str(s) => string_literal(s),
            Expr::Date(d) => {
                let iso =
                    functions::access_date(d).ok_or_else(|| format!("unrecognized date #{d}#"))?;
                match (duck, iso.len() > 10) {
                    (true, false) => format!("DATE '{iso}'"),
                    (true, true) => format!("TIMESTAMP '{}'", iso.replace('T', " ")),
                    (false, _) => string_literal(&iso),
                }
            }
            Expr::Name(parts) => self.name(parts)?,
            Expr::Star(q) => match q.as_slice() {
                [] => "*".into(),
                [t] => format!("{}.*", quote(t)),
                _ => return Err("unsupported qualified *".into()),
            },
            Expr::Neg(x) => format!("-{}", self.expr(x)?),
            Expr::Not(x) => format!("NOT ({})", self.expr(x)?),
            Expr::Paren(x) => format!("({})", self.expr(x)?),
            Expr::Bin(op, l, r) => self.binary(*op, l, r)?,
            Expr::Like { expr, pattern, not } => self.like(expr, pattern, *not)?,
            Expr::Between { expr, lo, hi, not } => format!(
                "{} {}BETWEEN {} AND {}",
                self.expr(expr)?,
                if *not { "NOT " } else { "" },
                self.expr(lo)?,
                self.expr(hi)?
            ),
            Expr::In { expr, list, not } => {
                let items: Result<Vec<String>, String> =
                    list.iter().map(|i| self.expr(i)).collect();
                format!(
                    "{} {}IN ({})",
                    self.expr(expr)?,
                    if *not { "NOT " } else { "" },
                    items?.join(", ")
                )
            }
            Expr::InSelect { expr, select, not } => {
                format!(
                    "{} {}IN ({})",
                    self.expr(expr)?,
                    if *not { "NOT " } else { "" },
                    self.select(select)?
                )
            }
            Expr::IsNull { expr, not } => format!(
                "{} IS {}NULL",
                self.expr(expr)?,
                if *not { "NOT " } else { "" }
            ),
            Expr::Sub(s) => format!("({})", self.select(s)?),
            Expr::Exists(s) => format!("EXISTS ({})", self.select(s)?),
            Expr::Call {
                name,
                args,
                distinct,
            } => functions::sql_call(self, name, args, *distinct)?,
        })
    }

    fn name(&mut self, parts: &[Part]) -> Result<String, String> {
        if let Some((sql, _)) = self.resolve(parts) {
            return Ok(sql);
        }
        if let [one] = parts {
            if !self
                .inlining
                .iter()
                .any(|a| a.eq_ignore_ascii_case(&one.text))
            {
                if let Some(e) = self.alias_expr(&one.text) {
                    self.inlining.push(one.text.clone());
                    let sql = self.expr(&e);
                    self.inlining.pop();
                    return Ok(format!("({})", sql?));
                }
            }
        }
        if self.is_complex(parts) {
            return Err(format!(
                "{} is an attachment or multi-value field",
                parts
                    .iter()
                    .map(|p| p.text.clone())
                    .collect::<Vec<_>>()
                    .join(".")
            ));
        }
        if let [one] = parts {
            if let Some((_, sql)) = self.outer.iter().find(|(n, _)| *n == one.text) {
                return Ok(sql.clone());
            }
        }
        let original: Vec<String> = parts.iter().map(|p| p.text.clone()).collect();
        let first = parts[0].text.to_ascii_lowercase();
        if first == "me" && parts.len() == 2 {
            return self.name(&parts[1..]);
        }
        // Unknown names are parameters, as in Access (form references included).
        if self.dialect == Dialect::Sqlite {
            return Err(format!(
                "{} is not a column of the table",
                original.join(".")
            ));
        }
        Ok(self.param(&original.join("!")))
    }

    /// A value used as a number: Access counts True as -1.
    pub fn numeric(&mut self, e: &Expr) -> Result<String, String> {
        let sql = self.expr(e)?;
        Ok(
            if self.dialect == Dialect::DuckDb && self.kind_of(e) == Kind::Bool {
                format!("(-CAST({sql} AS INTEGER))")
            } else {
                sql
            },
        )
    }

    fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> Result<String, String> {
        let arithmetic = matches!(
            op,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Mod
                | BinOp::IntDiv
                | BinOp::Pow
        );
        let (ls, rs) = if arithmetic {
            (self.numeric(l)?, self.numeric(r)?)
        } else {
            (self.expr(l)?, self.expr(r)?)
        };
        if self.dialect == Dialect::DuckDb && matches!(op, BinOp::Add | BinOp::Sub) {
            // Access date arithmetic counts in days (fractions are times of day).
            match (self.kind_of(l), self.kind_of(r), op) {
                (Kind::Date, Kind::Date, BinOp::Sub) => {
                    return Ok(format!("((epoch(CAST({ls} AS TIMESTAMP)) - epoch(CAST({rs} AS TIMESTAMP))) / 86400)"));
                }
                (Kind::Date, Kind::Number | Kind::Other, _)
                    if !matches!(r, Expr::Name(_)) || self.kind_of(r) == Kind::Number =>
                {
                    let sign = if op == BinOp::Sub { "-" } else { "+" };
                    return Ok(format!("(CAST({ls} AS TIMESTAMP) {sign} to_seconds(CAST(round(({rs}) * 86400) AS BIGINT)))"));
                }
                (Kind::Number, Kind::Date, BinOp::Add) => {
                    return Ok(format!("(CAST({rs} AS TIMESTAMP) + to_seconds(CAST(round(({ls}) * 86400) AS BIGINT)))"));
                }
                _ => {}
            }
        }
        let text_compare = self.kind_of(l) == Kind::Text || self.kind_of(r) == Kind::Text;
        let cmp = |o: &str| {
            // Access compares text without case.
            if text_compare {
                format!("lower({ls}) {o} lower({rs})")
            } else {
                format!("{ls} {o} {rs}")
            }
        };
        Ok(match op {
            BinOp::Or => format!("{ls} OR {rs}"),
            BinOp::And => format!("{ls} AND {rs}"),
            BinOp::Xor => format!("(({ls}) <> ({rs}))"),
            BinOp::Eqv => format!("(({ls}) = ({rs}))"),
            BinOp::Imp => format!("(NOT ({ls}) OR ({rs}))"),
            BinOp::Eq => cmp("="),
            BinOp::Ne => cmp("<>"),
            BinOp::Lt => cmp("<"),
            BinOp::Le => cmp("<="),
            BinOp::Gt => cmp(">"),
            BinOp::Ge => cmp(">="),
            // `&` treats null as empty text.
            BinOp::Concat => match self.dialect {
                Dialect::DuckDb => format!("concat({ls}, {rs})"),
                Dialect::Sqlite => format!("(coalesce({ls}, '') || coalesce({rs}, ''))"),
            },
            BinOp::Add => format!("{ls} + {rs}"),
            BinOp::Sub => format!("{ls} - {rs}"),
            BinOp::Mul => format!("{ls} * {rs}"),
            BinOp::Div => format!("{ls} / {rs}"),
            BinOp::Mod => format!("{ls} % {rs}"),
            BinOp::IntDiv => match self.dialect {
                Dialect::DuckDb => format!("({ls}) // ({rs})"),
                Dialect::Sqlite => format!("CAST(({ls}) / ({rs}) AS INTEGER)"),
            },
            BinOp::Pow => format!("power({ls}, {rs})"),
        })
    }

    fn like(&mut self, expr: &Expr, pattern: &Expr, not: bool) -> Result<String, String> {
        let e = self.expr(expr)?;
        let neg = if not { "NOT " } else { "" };
        if let Expr::Str(p) = pattern {
            return Ok(match functions::like_pattern(p) {
                functions::LikePattern::Like(l) if self.dialect == Dialect::DuckDb => {
                    format!("{e} {neg}ILIKE {} ESCAPE '\\'", string_literal(&l))
                }
                functions::LikePattern::Like(l) => {
                    format!("{e} {neg}LIKE {} ESCAPE '\\'", string_literal(&l))
                }
                functions::LikePattern::Regex(r) if self.dialect == Dialect::DuckDb => {
                    format!("{neg}regexp_full_match({e}, {}, 'i')", string_literal(&r))
                }
                functions::LikePattern::Regex(_) => {
                    return Err("character classes in Like patterns".into())
                }
            });
        }
        // A computed pattern: translate wildcards at run time.
        let p = self.expr(pattern)?;
        Ok(format!(
            "{e} {neg}ILIKE replace(replace({p}, '*', '%'), '?', '_')"
        ))
    }

    pub(super) fn from(&mut self, f: &From) -> Result<String, String> {
        Ok(match f {
            From::Table { name, alias } => {
                let cols = self.source_columns(name);
                let key = alias.clone().unwrap_or_else(|| name.clone());
                let complex = self.schema.complex(name);
                if let Some(scope) = self.scopes.last_mut() {
                    scope.complex.push((key.clone(), complex));
                    scope.sources.push((key, cols));
                }
                match alias {
                    Some(a) => format!("{} AS {}", quote(name), quote(a)),
                    None => quote(name),
                }
            }
            From::Sub { select, alias } => {
                let sql = self.select(select)?;
                let cols = self
                    .out
                    .columns
                    .iter()
                    .map(|c| (c.clone(), Kind::Other))
                    .collect();
                let a = alias.clone().unwrap_or_else(|| "sub".into());
                if let Some(scope) = self.scopes.last_mut() {
                    scope.sources.push((a.clone(), cols));
                }
                format!("({sql}) AS {}", quote(&a))
            }
            From::Join {
                kind,
                left,
                right,
                on,
            } => {
                let l = self.from(left)?;
                let r = self.from(right)?;
                let k = match kind {
                    JoinKind::Inner => "INNER JOIN",
                    JoinKind::Left => "LEFT JOIN",
                    JoinKind::Right => "RIGHT JOIN",
                };
                format!("({l} {k} {r} ON {})", self.expr(on)?)
            }
        })
    }

    /// Translates a SELECT (with its UNION parts) in a new scope.
    pub fn select(&mut self, s: &Select) -> Result<String, String> {
        self.scopes.push(Scope {
            sources: vec![],
            complex: vec![],
            aliases: vec![],
        });
        let result = self.select_in_scope(s);
        self.scopes.pop();
        result
    }

    fn select_in_scope(&mut self, s: &Select) -> Result<String, String> {
        let mut from = vec![];
        for f in &s.from {
            from.push(self.from(f)?);
        }
        // Attachment and multi-value fields live in child tables.
        let columns: Vec<(Expr, Option<String>)> = s
            .columns
            .iter()
            .filter(|(e, _)| {
                let complex = matches!(e, Expr::Name(parts) if self.is_complex(parts));
                if complex {
                    self.out.notes.push("attachment and multi-value fields are left out (they are rows of their own tables)".into());
                }
                !complex
            })
            .cloned()
            .collect();
        let s = &Select {
            columns,
            ..s.clone()
        };
        if let Some(scope) = self.scopes.last_mut() {
            scope.aliases = s
                .columns
                .iter()
                .filter_map(|(e, a)| a.clone().map(|a| (a, e.clone())))
                .collect();
        }
        let names = output_names(s, &|src| self.scope_columns(src));
        let mut cols = vec![];
        for ((e, alias), name) in s.columns.iter().zip(&names) {
            let sql = self.expr(e)?;
            let natural = natural_name(e);
            cols.push(match (alias, &natural, e) {
                (_, _, Expr::Star(_)) => sql,
                (Some(a), _, _) => format!("{sql} AS {}", quote(a)),
                (None, Some(n), _) if n == name => sql,
                _ => format!("{sql} AS {}", quote(name)),
            });
        }
        let mut sql = format!(
            "SELECT {}{}",
            if s.distinct { "DISTINCT " } else { "" },
            cols.join(", ")
        );
        if !from.is_empty() {
            sql.push_str(&format!(" FROM {}", from.join(", ")));
        }
        if let Some(w) = &s.where_clause {
            sql.push_str(&format!(" WHERE {}", self.expr(w)?));
        }
        if !s.group_by.is_empty() {
            let g: Result<Vec<String>, String> = s.group_by.iter().map(|e| self.expr(e)).collect();
            sql.push_str(&format!(" GROUP BY {}", g?.join(", ")));
        }
        if let Some(h) = &s.having {
            sql.push_str(&format!(" HAVING {}", self.expr(h)?));
        }
        let expanded = self.expand_stars(&names);
        for (all, part) in &s.unions {
            let p = self.select(part)?;
            sql.push_str(&format!(" UNION {}{p}", if *all { "ALL " } else { "" }));
        }
        if !s.order_by.is_empty() {
            let mut o = vec![];
            for (e, desc) in &s.order_by {
                o.push(format!(
                    "{}{}",
                    self.order_term(e, s)?,
                    if *desc { " DESC" } else { "" }
                ));
            }
            sql.push_str(&format!(" ORDER BY {}", o.join(", ")));
        }
        if let Some((n, percent)) = &s.top {
            sql.push_str(&format!(" LIMIT {n}{}", if *percent { "%" } else { "" }));
        }
        self.out.columns = expanded;
        Ok(sql)
    }

    /// ORDER BY of a union may only use output names.
    fn order_term(&mut self, e: &Expr, s: &Select) -> Result<String, String> {
        if s.unions.is_empty() {
            return self.expr(e);
        }
        match e {
            Expr::Name(parts) => Ok(quote(
                &parts.last().map(|p| p.text.clone()).unwrap_or_default(),
            )),
            Expr::Num(n) => Ok(n.clone()),
            other => self.expr(other),
        }
    }

    fn scope_columns(&self, source: &str) -> Vec<String> {
        let scope = self.scopes.last();
        let all = scope.map(|s| s.sources.clone()).unwrap_or_default();
        if source.is_empty() {
            return all
                .iter()
                .flat_map(|(_, c)| c.iter().map(|(n, _)| n.clone()))
                .collect();
        }
        all.iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(source))
            .map(|(_, c)| c.iter().map(|(n, _)| n.clone()).collect())
            .unwrap_or_default()
    }

    fn expand_stars(&self, names: &[String]) -> Vec<String> {
        names
            .iter()
            .flat_map(|n| match n.strip_prefix('*') {
                Some(src) => self.scope_columns(src),
                None => vec![n.clone()],
            })
            .collect()
    }

    /// A whole query statement (SELECT or crosstab) as DuckDB SQL.
    pub fn statement(&mut self, st: &Statement) -> Result<String, String> {
        match st {
            Statement::Select(s) => {
                self.out.target = s.into.clone();
                self.select(s)
            }
            Statement::Crosstab(c) => self.crosstab(c),
            Statement::Insert(i) => self.insert(i),
            Statement::Update(u) => self.update(u),
            Statement::Delete(d) => self.delete(d),
        }
    }

    /// `TRANSFORM agg SELECT rows ... PIVOT col` → DuckDB `PIVOT` in a subquery.
    fn crosstab(&mut self, c: &Crosstab) -> Result<String, String> {
        if self.dialect != Dialect::DuckDb {
            return Err("crosstab queries need DuckDB".into());
        }
        let mut inner = c.select.clone();
        inner.order_by.clear();
        let pivot_alias = "__pivot".to_string();
        let value_alias = "__value".to_string();
        // The inner query yields the row headings, the pivot value and the value to aggregate.
        let (agg_name, agg_arg) = match &c.transform.0 {
            Expr::Call { name, args, .. } if args.len() == 1 => (name.clone(), args[0].clone()),
            _ => return Err("TRANSFORM needs a single aggregate".into()),
        };
        inner
            .columns
            .push((c.pivot.clone(), Some(pivot_alias.clone())));
        inner.columns.push((agg_arg, Some(value_alias.clone())));
        inner.group_by.clear();
        inner.having = None;
        let rows: Vec<String> = c
            .select
            .group_by
            .iter()
            .map(|g| natural_name(g).unwrap_or_default())
            .filter(|n| !n.is_empty())
            .collect();
        // Aggregated row columns of the SELECT become plain row columns of the pivot.
        inner.columns.retain(
            |(e, _)| !matches!(e, Expr::Call { name, .. } if functions::is_aggregate(name)),
        );
        let inner_sql = self.select(&inner)?;
        let agg = functions::aggregate_name(&agg_name)
            .ok_or_else(|| format!("{agg_name} cannot be pivoted"))?;
        let mut sql = format!(
            "SELECT * FROM (PIVOT ({inner_sql}) ON {} USING {agg}({})",
            quote(&pivot_alias),
            quote(&value_alias)
        );
        if !c.pivot_in.is_empty() {
            let values: Result<Vec<String>, String> =
                c.pivot_in.iter().map(|v| self.expr(v)).collect();
            sql = format!(
                "SELECT * FROM (PIVOT ({inner_sql}) ON {} IN ({}) USING {agg}({})",
                quote(&pivot_alias),
                values?.join(", "),
                quote(&value_alias)
            );
        }
        if !rows.is_empty() {
            sql.push_str(&format!(
                " GROUP BY {}",
                rows.iter().map(|r| quote(r)).collect::<Vec<_>>().join(", ")
            ));
        }
        sql.push(')');
        self.out
            .notes
            .push("crosstab converted to a DuckDB PIVOT".into());
        Ok(sql)
    }
}

/// The name Access gives a column without an alias.
pub fn natural_name(e: &Expr) -> Option<String> {
    match e {
        Expr::Name(parts) => parts.last().map(|p| p.text.clone()),
        Expr::Paren(i) => natural_name(i),
        _ => None,
    }
}

/// Output names: aliases, natural names, `Expr1000`... for expressions, and
/// `*source` markers for stars. Repeated natural names are qualified as Access does.
pub fn output_names(s: &Select, _cols: &dyn Fn(&str) -> Vec<String>) -> Vec<String> {
    let mut counter = 1000;
    let naturals: Vec<Option<String>> = s.columns.iter().map(|(e, _)| natural_name(e)).collect();
    s.columns
        .iter()
        .zip(&naturals)
        .map(|((e, alias), natural)| {
            if let Some(a) = alias {
                return a.clone();
            }
            if let Expr::Star(q) = e {
                return format!("*{}", q.first().cloned().unwrap_or_default());
            }
            match natural {
                Some(n) => {
                    let repeated = naturals
                        .iter()
                        .filter(|x| x.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(n)))
                        .count()
                        > 1;
                    match (repeated, e) {
                        (true, Expr::Name(parts)) if parts.len() > 1 => {
                            format!("{}.{}", parts[parts.len() - 2].text, n)
                        }
                        _ => n.clone(),
                    }
                }
                None => {
                    let name = format!("Expr{counter}");
                    counter += 1;
                    name
                }
            }
        })
        .collect()
}
