//! Structured query definitions and their Access SQL.
//!
//! Access stores a query designed in the query grid as rows of `MSysQueries`
//! (binary files) or as `Operation`/`InputTables`/`OutputColumns`/... blocks
//! (template SaveAsText). Both become a [`QueryDef`], which renders the Access SQL
//! that the SQL view would show. Union, pass-through and data-definition queries
//! are stored as SQL text and skip this step.
use crate::access::model::QueryKind;
use crate::access::text_format::Node;

/// `Option` / `MSysQueries` flag bits of a select query.
pub mod select_flags {
    pub const SELECT_STAR: u32 = 0x01;
    pub const DISTINCT: u32 = 0x02;
    pub const OWNER_ACCESS: u32 = 0x04;
    pub const DISTINCT_ROW: u32 = 0x08;
    pub const TOP: u32 = 0x10;
    pub const PERCENT: u32 = 0x20;
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct OutputColumn {
    pub expression: String,
    pub alias: Option<String>,
    /// Target column of an append or update query.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Join {
    pub left: String,
    pub right: String,
    pub expression: String,
    /// 1 inner, 2 left outer, 3 right outer.
    pub kind: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryDef {
    pub kind: QueryKind,
    pub flags: u32,
    /// `TOP` value (`RowCount`).
    pub top: Option<String>,
    /// Destination table of append and make-table queries.
    pub target: Option<String>,
    /// Source tables or queries with optional aliases.
    pub inputs: Vec<(String, Option<String>)>,
    pub columns: Vec<OutputColumn>,
    pub joins: Vec<Join>,
    pub where_clause: Option<String>,
    pub groups: Vec<String>,
    pub having: Option<String>,
    /// Expressions with `true` for descending.
    pub order: Vec<(String, bool)>,
    /// `PARAMETERS` declarations: name and Access type name.
    pub parameters: Vec<(String, String)>,
    /// Crosstab: the row headings are `groups`, this is the column heading.
    pub pivot: Option<String>,
    /// Crosstab: the value aggregate.
    pub transform: Option<String>,
}

impl QueryDef {
    pub fn new(kind: QueryKind) -> Self {
        Self {
            kind,
            flags: 0,
            top: None,
            target: None,
            inputs: vec![],
            columns: vec![],
            joins: vec![],
            where_clause: None,
            groups: vec![],
            having: None,
            order: vec![],
            parameters: vec![],
            pivot: None,
            transform: None,
        }
    }
}

/// The query kind of a SaveAsText `Operation` / `MSysQueries` type value.
pub fn kind_from_operation(op: i64) -> QueryKind {
    match op {
        2 => QueryKind::MakeTable,
        3 => QueryKind::Append,
        4 => QueryKind::Update,
        5 => QueryKind::Delete,
        6 => QueryKind::Crosstab,
        7 => QueryKind::DataDefinition,
        8 => QueryKind::PassThrough,
        9 => QueryKind::Union,
        _ => QueryKind::Select,
    }
}

/// Reads a template query file into SQL text and kind.
pub fn from_text(root: &Node) -> Result<(QueryKind, String, Vec<(String, String)>), String> {
    if let Some(sql) = root.get("SQL") {
        // Saved as SQL text (union, pass-through, data definition, or SQL-view edits).
        let kind = sql_kind(sql);
        return Ok((kind, sql.to_string(), declared_parameters(sql)));
    }
    let op = root
        .int("Operation")
        .ok_or("query has neither SQL nor Operation")?;
    let mut def = QueryDef::new(kind_from_operation(op));
    def.flags = root.int("Option").unwrap_or(0) as u32;
    def.top = root.get("RowCount").map(str::to_string);
    def.target = root.get("Name").map(str::to_string);
    def.where_clause = root.get("Where").map(str::to_string);
    def.having = root.get("Having").map(str::to_string);
    if let Some(b) = root.block("InputTables") {
        for (k, v) in b.pairs() {
            match k {
                "Name" => def.inputs.push((v.to_string(), None)),
                "Alias" => {
                    if let Some(last) = def.inputs.last_mut() {
                        last.1 = Some(v.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(b) = root.block("OutputColumns") {
        let mut pending = OutputColumn::default();
        for (k, v) in b.pairs() {
            match k {
                "Alias" => pending.alias = Some(v.to_string()),
                "Name" => pending.name = Some(v.to_string()),
                "Expression" => {
                    pending.expression = v.to_string();
                    def.columns.push(std::mem::take(&mut pending));
                }
                _ => {}
            }
        }
    }
    if let Some(b) = root.block("Joins") {
        let (mut left, mut right, mut expr) = (String::new(), String::new(), String::new());
        for (k, v) in b.pairs() {
            match k {
                "LeftTable" => left = v.to_string(),
                "RightTable" => right = v.to_string(),
                "Expression" => expr = v.to_string(),
                "Flag" => def.joins.push(Join {
                    left: std::mem::take(&mut left),
                    right: std::mem::take(&mut right),
                    expression: std::mem::take(&mut expr),
                    kind: v.trim().parse().unwrap_or(1),
                }),
                _ => {}
            }
        }
    }
    if let Some(b) = root.block("OrderBy") {
        let mut expr = String::new();
        for (k, v) in b.pairs() {
            match k {
                "Expression" => expr = v.to_string(),
                "Flag" => def.order.push((std::mem::take(&mut expr), v.trim() == "1")),
                _ => {}
            }
        }
    }
    if let Some(b) = root.block("Groups") {
        for (k, v) in b.pairs() {
            if k == "Expression" {
                def.groups.push(v.to_string());
            }
        }
    }
    if let Some(b) = root.block("Parameters") {
        let mut name = String::new();
        for (k, v) in b.pairs() {
            match k {
                "Name" => name = v.to_string(),
                "Flag" => def
                    .parameters
                    .push((std::mem::take(&mut name), parameter_type(v))),
                _ => {}
            }
        }
    }
    let params = def.parameters.clone();
    Ok((def.kind, to_access_sql(&def), params))
}

/// DAO data type codes used for query parameters.
pub fn parameter_type(code: &str) -> String {
    match code.trim().parse::<i64>().unwrap_or(10) {
        1 => "Bit",
        2 => "Byte",
        3 => "Short",
        4 => "Long",
        5 => "Currency",
        6 => "IEEESingle",
        7 => "IEEEDouble",
        8 => "DateTime",
        11 => "LongBinary",
        12 => "LongText",
        15 => "Guid",
        16 => "BigInt",
        20 => "Decimal",
        _ => "Text",
    }
    .to_string()
}

/// Guesses the kind of a query stored as SQL text from its first keyword.
pub fn sql_kind(sql: &str) -> QueryKind {
    let upper = sql.trim_start().to_ascii_uppercase();
    let body = strip_parameters(&upper);
    let first = body.split_whitespace().next().unwrap_or("");
    match first {
        "TRANSFORM" => QueryKind::Crosstab,
        "DELETE" => QueryKind::Delete,
        "UPDATE" => QueryKind::Update,
        "INSERT" => QueryKind::Append,
        "CREATE" | "ALTER" | "DROP" => QueryKind::DataDefinition,
        _ if contains_word(body, "UNION") => QueryKind::Union,
        _ if contains_word(body, "INTO") && first == "SELECT" => QueryKind::MakeTable,
        _ => QueryKind::Select,
    }
}

fn strip_parameters(upper: &str) -> &str {
    if upper.starts_with("PARAMETERS") {
        if let Some(i) = upper.find(';') {
            return upper[i + 1..].trim_start();
        }
    }
    upper
}

fn contains_word(s: &str, word: &str) -> bool {
    s.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|w| w == word)
}

/// `PARAMETERS [Start Date] DateTime, [End] Text(255);` declarations of SQL text.
pub fn declared_parameters(sql: &str) -> Vec<(String, String)> {
    let trimmed = sql.trim_start();
    if !trimmed.to_ascii_uppercase().starts_with("PARAMETERS") {
        return vec![];
    }
    let Some(end) = trimmed.find(';') else {
        return vec![];
    };
    let list = &trimmed["PARAMETERS".len()..end];
    split_top_level(list, ',')
        .into_iter()
        .filter_map(|p| {
            let p = p.trim();
            if p.is_empty() {
                return None;
            }
            let (name, ty) = if let Some(rest) = p.strip_prefix('[') {
                let close = rest.find(']')?;
                (
                    rest[..close].to_string(),
                    rest[close + 1..].trim().to_string(),
                )
            } else {
                let mut it = p.splitn(2, char::is_whitespace);
                (
                    it.next()?.to_string(),
                    it.next().unwrap_or("Text").trim().to_string(),
                )
            };
            Some((name, ty))
        })
        .collect()
}

/// Splits on `sep` outside brackets, quotes and parentheses.
pub fn split_top_level(s: &str, sep: char) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let (mut depth, mut bracket, mut quote) = (0i32, false, None::<char>);
    for c in s.chars() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[') => bracket = true,
            (None, ']') => bracket = false,
            (None, '(') if !bracket => depth += 1,
            (None, ')') if !bracket => depth -= 1,
            (None, _) if c == sep && depth == 0 && !bracket => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out
}

/// Quotes a table or query name for Access SQL when it is not a plain identifier.
pub fn quote_name(name: &str) -> String {
    if !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
    {
        name.to_string()
    } else {
        format!("[{name}]")
    }
}

enum Source {
    Table {
        key: String,
        sql: String,
    },
    Join {
        left: Box<Source>,
        right: Box<Source>,
        kind: i64,
        on: Vec<String>,
    },
}

impl Source {
    fn contains(&self, name: &str) -> bool {
        match self {
            Source::Table { key, .. } => key.eq_ignore_ascii_case(name),
            Source::Join { left, right, .. } => left.contains(name) || right.contains(name),
        }
    }

    /// Adds an ON condition to the innermost join of `left` and `right`.
    fn add_condition(&mut self, left: &str, right: &str, kind: i64, expr: &str) -> bool {
        let Source::Join {
            left: l,
            right: r,
            kind: k,
            on,
        } = self
        else {
            return false;
        };
        for child in [l, r] {
            if child.contains(left) && child.contains(right) {
                return child.add_condition(left, right, kind, expr);
            }
        }
        if *k == kind {
            on.push(expr.to_string());
            return true;
        }
        false
    }

    fn render(&self, top: bool) -> String {
        match self {
            Source::Table { sql, .. } => sql.clone(),
            Source::Join {
                left,
                right,
                kind,
                on,
            } => {
                let op = match kind {
                    2 => "LEFT JOIN",
                    3 => "RIGHT JOIN",
                    _ => "INNER JOIN",
                };
                let cond = if on.len() > 1 {
                    format!("(({}))", on.join(") AND ("))
                } else {
                    on.join("")
                };
                let s = format!(
                    "{} {op} {} ON {cond}",
                    left.render(false),
                    right.render(false)
                );
                if top {
                    s
                } else {
                    format!("({s})")
                }
            }
        }
    }
}

/// The FROM clause: input tables combined by the joins (Jackcess' algorithm).
fn from_clause(def: &QueryDef) -> Vec<String> {
    let mut sources: Vec<Source> = def
        .inputs
        .iter()
        .map(|(name, alias)| {
            let mut sql = quote_name(name);
            if let Some(a) = alias {
                sql.push_str(&format!(" AS {}", quote_name(a)));
            }
            Source::Table {
                key: alias.clone().unwrap_or_else(|| name.clone()),
                sql,
            }
        })
        .collect();
    for j in &def.joins {
        let li = sources.iter().position(|s| s.contains(&j.left));
        let ri = sources.iter().position(|s| s.contains(&j.right));
        if let (Some(l), Some(r)) = (li, ri) {
            if l == r {
                // Both tables are already joined: another condition of that join.
                sources[l].add_condition(&j.left, &j.right, j.kind, &j.expression);
                continue;
            }
        }
        let index = li.into_iter().chain(ri).min().unwrap_or(sources.len());
        // Remove the higher index first so the lower one stays valid.
        let mut slots: Vec<(usize, bool)> = li
            .map(|l| (l, true))
            .into_iter()
            .chain(ri.map(|r| (r, false)))
            .collect();
        slots.sort_by(|a, b| b.0.cmp(&a.0));
        let (mut left, mut right) = (None, None);
        for (i, is_left) in slots {
            let s = sources.remove(i);
            if is_left {
                left = Some(s);
            } else {
                right = Some(s);
            }
        }
        let plain = |name: &str| Source::Table {
            key: name.to_string(),
            sql: quote_name(name),
        };
        let left = left.unwrap_or_else(|| plain(&j.left));
        let right = right.unwrap_or_else(|| plain(&j.right));
        let index = index.min(sources.len());
        sources.insert(
            index,
            Source::Join {
                left: Box::new(left),
                right: Box::new(right),
                kind: j.kind,
                on: vec![j.expression.clone()],
            },
        );
    }
    sources.iter().map(|s| s.render(true)).collect()
}

/// Renders the Access SQL of a structured query.
pub fn to_access_sql(def: &QueryDef) -> String {
    use select_flags::*;
    let mut sql = String::new();
    if !def.parameters.is_empty() {
        let list: Vec<String> = def
            .parameters
            .iter()
            .map(|(n, t)| format!("[{n}] {t}"))
            .collect();
        sql.push_str(&format!("PARAMETERS {};\n", list.join(", ")));
    }
    let from = from_clause(def);
    let mut tail = String::new();
    if !from.is_empty() && def.kind != QueryKind::Update {
        tail.push_str(&format!("\nFROM {}", from.join(", ")));
    }
    if let Some(w) = &def.where_clause {
        tail.push_str(&format!("\nWHERE {w}"));
    }
    if def.kind == QueryKind::Crosstab {
        let groups: Vec<&str> = def.groups.iter().map(String::as_str).collect();
        let mut s = format!(
            "TRANSFORM {}\nSELECT {}{tail}",
            def.transform.clone().unwrap_or_default(),
            select_list(def).join(", ")
        );
        if !groups.is_empty() {
            s.push_str(&format!("\nGROUP BY {}", groups.join(", ")));
        }
        if !def.order.is_empty() {
            let order: Vec<String> = def
                .order
                .iter()
                .map(|(e, d)| if *d { format!("{e} DESC") } else { e.clone() })
                .collect();
            s.push_str(&format!("\nORDER BY {}", order.join(", ")));
        }
        s.push_str(&format!(
            "\nPIVOT {}",
            def.pivot.clone().unwrap_or_default()
        ));
        return sql + &s;
    }
    if !def.groups.is_empty() {
        tail.push_str(&format!("\nGROUP BY {}", def.groups.join(", ")));
    }
    if let Some(h) = &def.having {
        tail.push_str(&format!("\nHAVING {h}"));
    }
    if !def.order.is_empty() {
        let order: Vec<String> = def
            .order
            .iter()
            .map(|(e, d)| if *d { format!("{e} DESC") } else { e.clone() })
            .collect();
        tail.push_str(&format!("\nORDER BY {}", order.join(", ")));
    }
    let mut modifiers = String::new();
    if def.flags & DISTINCT != 0 {
        modifiers.push_str("DISTINCT ");
    } else if def.flags & DISTINCT_ROW != 0 {
        modifiers.push_str("DISTINCTROW ");
    }
    if def.flags & TOP != 0 {
        if let Some(top) = &def.top {
            modifiers.push_str(&format!("TOP {top} "));
            if def.flags & PERCENT != 0 {
                modifiers.push_str("PERCENT ");
            }
        }
    }
    let body = match def.kind {
        QueryKind::Delete => format!("DELETE {}{tail}", delete_list(def)),
        QueryKind::Update => {
            let sets: Vec<String> = def
                .columns
                .iter()
                .map(|c| format!("{} = {}", c.name.clone().unwrap_or_default(), c.expression))
                .collect();
            format!("UPDATE {}\nSET {}{tail}", from.join(", "), sets.join(", "))
        }
        QueryKind::Append => {
            let target = def.target.clone().unwrap_or_default();
            let names: Vec<String> = def
                .columns
                .iter()
                .filter_map(|c| c.name.clone())
                .map(|n| quote_name(&n))
                .collect();
            let values: Vec<String> = def
                .columns
                .iter()
                .map(|c| render_column(c, false))
                .collect();
            format!(
                "INSERT INTO {} ({})\nSELECT {modifiers}{}{tail}",
                quote_name(&target),
                names.join(", "),
                values.join(", ")
            )
        }
        QueryKind::MakeTable => format!(
            "SELECT {modifiers}{} INTO {}{tail}",
            select_list(def).join(", "),
            quote_name(&def.target.clone().unwrap_or_default())
        ),
        _ => format!("SELECT {modifiers}{}{tail}", select_list(def).join(", ")),
    };
    sql + &body
}

fn render_column(c: &OutputColumn, with_alias: bool) -> String {
    match (&c.alias, with_alias) {
        (Some(a), true) => format!("{} AS {}", c.expression, quote_name(a)),
        _ => c.expression.clone(),
    }
}

fn select_list(def: &QueryDef) -> Vec<String> {
    let mut cols: Vec<String> = def.columns.iter().map(|c| render_column(c, true)).collect();
    if def.flags & select_flags::SELECT_STAR != 0 || cols.is_empty() {
        cols.push("*".into());
    }
    cols
}

fn delete_list(def: &QueryDef) -> String {
    let cols: Vec<String> = def.columns.iter().map(|c| c.expression.clone()).collect();
    if cols.is_empty() {
        "*".into()
    } else {
        cols.join(", ")
    }
}
