//! Domain aggregate functions (`DLookup`, `DCount`, `DSum`...) → scalar subqueries.
//!
//! Access builds the criteria as text at run time, usually from the current
//! row: `DLookup("Price", "Products", "ID=" & [ProductID])`. The text pieces
//! are joined with placeholders for the computed pieces, parsed as one WHERE
//! clause, and each placeholder becomes the outer SQL, so the subquery is
//! correlated with the row instead of rebuilt from text.
use super::ast::*;
use super::sql::SqlWriter;

pub fn is_domain(name: &str) -> bool {
    matches!(
        name,
        "dlookup" | "dcount" | "dsum" | "davg" | "dmin" | "dmax" | "dfirst" | "dlast"
    )
}

fn arg_str(args: &[Expr], i: usize) -> Option<&str> {
    match args.get(i) {
        Some(Expr::Str(s)) => Some(s),
        Some(Expr::Paren(inner)) => arg_str(std::slice::from_ref(inner), 0),
        _ => None,
    }
}

/// The pieces of a `&` (or `+` on text) chain, left to right.
fn concat_pieces<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    match e {
        Expr::Bin(BinOp::Concat | BinOp::Add, l, r)
            if is_texty(l) || is_texty(r) || matches!(e, Expr::Bin(BinOp::Concat, ..)) =>
        {
            concat_pieces(l, out);
            concat_pieces(r, out);
        }
        Expr::Paren(inner) if matches!(**inner, Expr::Bin(BinOp::Concat, ..)) => {
            concat_pieces(inner, out)
        }
        other => out.push(other),
    }
}

fn is_texty(e: &Expr) -> bool {
    matches!(e, Expr::Str(_) | Expr::Bin(BinOp::Concat, ..))
}

/// Criteria text with `[__outer_N]` placeholders, and the SQL of each one.
fn criteria_text(w: &mut SqlWriter, e: &Expr) -> Result<(String, Vec<(String, String)>), String> {
    let mut pieces = vec![];
    concat_pieces(e, &mut pieces);
    let mut text = String::new();
    let mut outer = vec![];
    let mut i = 0;
    while i < pieces.len() {
        match pieces[i] {
            Expr::Str(s) => text.push_str(s),
            computed => {
                let name = format!("__outer_{}", w.outer.len() + outer.len());
                w.qualify = true;
                let sql = w.expr(computed);
                w.qualify = false;
                let mut sql = sql?;
                // `"Name='" & [Name] & "'"`: the quotes wrap a value, not text to parse.
                let next = match pieces.get(i + 1) {
                    Some(Expr::Str(s)) => Some(s.as_str()),
                    _ => None,
                };
                let quote_char = ['\'', '"', '#']
                    .into_iter()
                    .find(|q| text.ends_with(*q) && next.is_some_and(|n| n.starts_with(*q)));
                if let Some(q) = quote_char {
                    text.pop();
                    if q != '#' {
                        sql = format!("CAST({sql} AS VARCHAR)");
                    }
                    text.push_str(&format!("[{name}]"));
                    outer.push((name, sql));
                    text.push_str(&next.unwrap_or_default()[1..]);
                    i += 2;
                    continue;
                }
                text.push_str(&format!("[{name}]"));
                outer.push((name, sql));
            }
        }
        i += 1;
    }
    Ok((text, outer))
}

/// `DLookup("expr", "domain", criteria)` → a scalar subquery.
pub fn domain_sql(w: &mut SqlWriter, f: &str, args: &[Expr]) -> Result<String, String> {
    let expr = arg_str(args, 0).ok_or_else(|| format!("{f} needs a literal expression"))?;
    let domain = arg_str(args, 1).ok_or_else(|| format!("{f} needs a literal domain"))?;
    let (criteria, outer) = match args.get(2) {
        None => (None, vec![]),
        Some(e) => match arg_str(args, 2) {
            Some(s) => (Some(s.to_string()), vec![]),
            None => {
                let (text, outer) = criteria_text(w, e)?;
                (Some(text), outer)
            }
        },
    };
    let agg = match f {
        "dlookup" => None,
        "dcount" => Some("Count"),
        "dsum" => Some("Sum"),
        "davg" => Some("Avg"),
        "dmin" => Some("Min"),
        "dmax" => Some("Max"),
        "dfirst" => Some("First"),
        _ => Some("Last"),
    };
    let domain = domain.trim().trim_start_matches('[').trim_end_matches(']');
    let column = match agg {
        Some(a) => format!("{a}({expr})"),
        None => expr.to_string(),
    };
    // The same table in the outer query: alias the domain so names stay apart.
    let shadowed = !outer.is_empty()
        && w.scopes.iter().any(|s| {
            s.sources
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(domain))
        });
    let mut sql = format!("SELECT {column} FROM [{domain}]");
    if shadowed {
        sql.push_str(" AS [__domain]");
    }
    if let Some(c) = criteria.filter(|c| !c.trim().is_empty()) {
        sql.push_str(&format!(" WHERE {c}"));
    }
    let select = match parse_statement(&sql) {
        Ok(Statement::Select(s)) => s,
        Ok(_) => return Err("a domain function needs a SELECT".into()),
        Err(e) => return Err(format!("{f} criteria could not be read ({e})")),
    };
    let added = outer.len();
    w.outer.extend(outer);
    let inner = w.select(&select);
    let keep = w.outer.len() - added;
    w.outer.truncate(keep);
    let mut inner = inner?;
    if agg.is_none() {
        inner.push_str(" LIMIT 1");
    }
    Ok(format!("({inner})"))
}
