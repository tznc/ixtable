//! Access (VBA) built-in functions in SQL, plus `Like` patterns and date literals.
use super::ast::*;
use super::sql::{Dialect, SqlWriter};

/// True when an expression uses an aggregate (outside subqueries).
pub fn contains_aggregate(e: &Expr) -> bool {
    match e {
        Expr::Call { name, args, .. } => is_aggregate(name) || args.iter().any(contains_aggregate),
        Expr::Neg(x) | Expr::Not(x) | Expr::Paren(x) => contains_aggregate(x),
        Expr::Bin(_, l, r) => contains_aggregate(l) || contains_aggregate(r),
        Expr::Like { expr, pattern, .. } => contains_aggregate(expr) || contains_aggregate(pattern),
        Expr::Between { expr, lo, hi, .. } => {
            contains_aggregate(expr) || contains_aggregate(lo) || contains_aggregate(hi)
        }
        Expr::In { expr, list, .. } => {
            contains_aggregate(expr) || list.iter().any(contains_aggregate)
        }
        Expr::IsNull { expr, .. } => contains_aggregate(expr),
        _ => false,
    }
}

pub fn is_aggregate(name: &str) -> bool {
    aggregate_name(name).is_some()
}

/// Access aggregate → DuckDB aggregate.
pub fn aggregate_name(name: &str) -> Option<&'static str> {
    Some(match name.to_ascii_lowercase().as_str() {
        "sum" => "sum",
        "avg" => "avg",
        "count" => "count",
        "min" => "min",
        "max" => "max",
        "first" => "first",
        "last" => "last",
        "stdev" => "stddev_samp",
        "stdevp" => "stddev_pop",
        "var" => "var_samp",
        "varp" => "var_pop",
        _ => return None,
    })
}

/// DateAdd/DateDiff interval codes.
pub fn interval(code: &str) -> Option<(&'static str, i64)> {
    Some(match code.to_ascii_lowercase().as_str() {
        "yyyy" => ("year", 1),
        "q" => ("month", 3),
        "m" => ("month", 1),
        "y" | "d" | "w" => ("day", 1),
        "ww" => ("day", 7),
        "h" => ("hour", 1),
        "n" => ("minute", 1),
        "s" => ("second", 1),
        _ => return None,
    })
}

fn arg_str(args: &[Expr], i: usize) -> Option<&str> {
    match args.get(i) {
        Some(Expr::Str(s)) => Some(s),
        _ => None,
    }
}

/// Translates a function call to SQL.
pub fn sql_call(
    w: &mut SqlWriter,
    name: &str,
    args: &[Expr],
    distinct: bool,
) -> Result<String, String> {
    let duck = w.dialect == Dialect::DuckDb;
    let lower = name.to_ascii_lowercase();
    let lower = lower.trim_end_matches('$');
    let mut a = vec![];
    // Interval and domain arguments are literals translated separately.
    let literal_first = matches!(lower, "dateadd" | "datediff" | "datepart");
    let domain = matches!(
        lower,
        "dlookup" | "dcount" | "dsum" | "davg" | "dmin" | "dmax" | "dfirst" | "dlast"
    );
    if !domain {
        for (i, x) in args.iter().enumerate() {
            if literal_first && i == 0 {
                a.push(String::new());
            } else {
                a.push(w.expr(x)?);
            }
        }
    }
    let n = args.len();
    let need = |k: usize| {
        if n < k {
            Err(format!("{name} needs {k} arguments"))
        } else {
            Ok(())
        }
    };
    if let Some(agg) = aggregate_name(lower) {
        if lower == "count" && matches!(args.first(), Some(Expr::Star(_))) {
            return Ok("count(*)".into());
        }
        if !duck
            && matches!(
                lower,
                "first" | "last" | "stdev" | "stdevp" | "var" | "varp"
            )
        {
            return Err(format!("{name} has no SQLite equivalent"));
        }
        need(1)?;
        let arg = if matches!(lower, "sum" | "avg") {
            w.numeric(&args[0])?
        } else {
            a[0].clone()
        };
        return Ok(format!(
            "{agg}({}{arg})",
            if distinct { "DISTINCT " } else { "" }
        ));
    }
    Ok(match lower {
        "iif" => {
            need(2)?;
            let otherwise = a.get(2).cloned().unwrap_or_else(|| "NULL".into());
            format!("CASE WHEN {} THEN {} ELSE {otherwise} END", a[0], a[1])
        }
        "nz" => {
            need(1)?;
            // Without a default, Nz gives "" or 0 depending on what the value is.
            let default = a.get(1).cloned().unwrap_or_else(|| match w.kind_of(&args[0]) {
                super::sql::Kind::Text | super::sql::Kind::Other => "''".into(),
                _ => "0".into(),
            });
            format!("coalesce({}, {default})", a[0])
        }
        "isnull" => {
            need(1)?;
            format!("({} IS NULL)", a[0])
        }
        "isempty" => {
            need(1)?;
            format!("({} IS NULL)", a[0])
        }
        "isnumeric" if duck => format!("(TRY_CAST({} AS DOUBLE) IS NOT NULL)", a[0]),
        "isdate" if duck => format!("(TRY_CAST({} AS TIMESTAMP) IS NOT NULL)", a[0]),
        // ixtable works in UTC; DuckDB without ICU has now() but not current_date.
        "date" if n == 0 => match (duck, w.in_default) {
            (true, _) => "CAST(CAST(now() AS TIMESTAMP) AS DATE)".into(),
            (false, true) => "CURRENT_DATE".into(),
            (false, false) => "date('now')".into(),
        },
        "now" if n == 0 => match (duck, w.in_default) {
            (true, _) => "CAST(now() AS TIMESTAMP)".into(),
            (false, true) => "CURRENT_TIMESTAMP".into(),
            (false, false) => "strftime('%Y-%m-%dT%H:%M:%S','now')".into(),
        },
        "time" if n == 0 => match (duck, w.in_default) {
            (true, _) => "CAST(CAST(now() AS TIMESTAMP) AS TIME)".into(),
            (false, true) => "CURRENT_TIME".into(),
            (false, false) => "time('now')".into(),
        },
        "year" | "month" | "day" | "hour" | "minute" | "second" if duck => format!("{lower}({})", a[0]),
        "year" | "month" | "day" | "hour" | "minute" | "second" => {
            let f = match lower {
                "year" => "%Y",
                "month" => "%m",
                "day" => "%d",
                "hour" => "%H",
                "minute" => "%M",
                _ => "%S",
            };
            format!("CAST(strftime('{f}', {}) AS INTEGER)", a[0])
        }
        "weekday" if duck => format!("(dayofweek({}) + 1)", a[0]),
        "dateserial" if duck => {
            need(3)?;
            format!("(make_date(CAST({} AS INTEGER), 1, 1) + to_months(CAST({} AS INTEGER) - 1) + to_days(CAST({} AS INTEGER) - 1))", a[0], a[1], a[2])
        }
        "dateadd" if duck => {
            need(3)?;
            let (unit, mult) = arg_str(args, 0).and_then(interval).ok_or("DateAdd needs a literal interval")?;
            let f = match unit {
                "year" => "to_years",
                "month" => "to_months",
                "day" => "to_days",
                "hour" => "to_hours",
                "minute" => "to_minutes",
                _ => "to_seconds",
            };
            let amount = if mult == 1 { format!("CAST({} AS INTEGER)", a[1]) } else { format!("CAST({} AS INTEGER) * {mult}", a[1]) };
            format!("({} + {f}({amount}))", a[2])
        }
        "datediff" if duck => {
            need(3)?;
            let code = arg_str(args, 0).ok_or("DateDiff needs a literal interval")?;
            let unit = match code.to_ascii_lowercase().as_str() {
                "ww" => "week",
                "q" => "quarter",
                c => interval(c).map(|(u, _)| u).ok_or("unknown DateDiff interval")?,
            };
            format!("date_diff('{unit}', {}, {})", a[1], a[2])
        }
        "datepart" if duck => {
            let code = arg_str(args, 0).ok_or("DatePart needs a literal interval")?;
            let part = match code.to_ascii_lowercase().as_str() {
                "yyyy" => "year",
                "q" => "quarter",
                "m" => "month",
                "y" => "dayofyear",
                "d" => "day",
                "w" => return Ok(format!("(dayofweek({}) + 1)", a[1])),
                "ww" => "week",
                "h" => "hour",
                "n" => "minute",
                "s" => "second",
                _ => return Err("unknown DatePart interval".into()),
            };
            format!("date_part('{part}', {})", a[1])
        }
        "datevalue" | "cdate" if duck => format!("CAST({} AS TIMESTAMP)", a[0]),
        "timevalue" if duck => format!("CAST({} AS TIME)", a[0]),
        "format" if duck => super::builtins::format_sql(&a[0], args.get(1))?,
        "left" => format!("{}({}, 1, {})", if duck { "substring" } else { "substr" }, a[0], a[1]),
        "right" if duck => format!("right({}, {})", a[0], a[1]),
        "right" => format!("substr({}, -({}))", a[0], a[1]),
        "mid" => match a.len() {
            2 => format!("substr({}, {})", a[0], a[1]),
            _ => format!("substr({}, {}, {})", a[0], a[1], a[2]),
        },
        "len" => format!("length({})", a[0]),
        "trim" => format!("trim({})", a[0]),
        "ltrim" => format!("ltrim({})", a[0]),
        "rtrim" => format!("rtrim({})", a[0]),
        "ucase" => format!("upper({})", a[0]),
        "lcase" => format!("lower({})", a[0]),
        "instr" => match a.len() {
            2 => format!("instr({}, {})", a[0], a[1]),
            _ if duck => format!("(CASE WHEN instr(substring({1}, {0}), {2}) = 0 THEN 0 ELSE instr(substring({1}, {0}), {2}) + {0} - 1 END)", a[0], a[1], a[2]),
            _ => return Err("InStr with a start position".into()),
        },
        "replace" => format!("replace({}, {}, {})", a[0], a[1], a[2]),
        "space" if duck => format!("repeat(' ', {})", a[0]),
        "string" if duck => format!("repeat({}, {})", a[1], a[0]),
        "chr" if duck => format!("chr({})", a[0]),
        "chr" => format!("char({})", a[0]),
        "asc" if duck => format!("ascii({})", a[0]),
        "val" => {
            if duck {
                format!("coalesce(TRY_CAST({} AS DOUBLE), 0)", a[0])
            } else {
                format!("CAST({} AS REAL)", a[0])
            }
        }
        "cstr" => format!("CAST({} AS {})", a[0], if duck { "VARCHAR" } else { "TEXT" }),
        "cint" | "clng" | "clnglng" => format!("CAST(round({}) AS {})", a[0], if duck { "BIGINT" } else { "INTEGER" }),
        "cdbl" | "csng" => format!("CAST({} AS {})", a[0], if duck { "DOUBLE" } else { "REAL" }),
        "ccur" | "cdec" => {
            if duck {
                format!("CAST({} AS DECIMAL(18,4))", a[0])
            } else {
                format!("round({}, 4)", a[0])
            }
        }
        "cbool" => format!("CAST({} AS {})", a[0], if duck { "BOOLEAN" } else { "INTEGER" }),
        // Int of a date-time is its date.
        "int" | "fix" | "datevalue" if w.kind_of(&args[0]) == super::sql::Kind::Date => {
            if duck {
                format!("CAST({} AS DATE)", a[0])
            } else {
                format!("date({})", a[0])
            }
        }
        "int" => format!("floor({})", a[0]),
        "fix" => format!("{}({})", if duck { "trunc" } else { "CAST" }, if duck { a[0].clone() } else { format!("{} AS INTEGER", a[0]) }),
        "abs" => format!("abs({})", w.numeric(&args[0])?),
        "sgn" => format!("sign({})", a[0]),
        "sqr" => format!("sqrt({})", a[0]),
        "exp" => format!("exp({})", a[0]),
        "log" => format!("ln({})", a[0]),
        "round" => match a.len() {
            1 => format!("round({})", a[0]),
            _ => format!("round({}, {})", a[0], a[1]),
        },
        "switch" => {
            if n < 2 || n % 2 != 0 {
                return Err("Switch needs condition/value pairs".into());
            }
            let pairs: Vec<String> = a.chunks(2).map(|p| format!("WHEN {} THEN {}", p[0], p[1])).collect();
            format!("CASE {} END", pairs.join(" "))
        }
        "choose" => {
            need(2)?;
            let arms: Vec<String> = a[1..].iter().enumerate().map(|(i, v)| format!("WHEN {} THEN {v}", i + 1)).collect();
            format!("CASE {} {} END", a[0], arms.join(" "))
        }
        "coalesce" => format!("coalesce({})", a.join(", ")),
        "plaintext" if duck => format!("replace(regexp_replace({}, '<[^>]*>', '', 'g'), '&nbsp;', ' ')", a[0]),
        _ if duck && super::domain::is_domain(lower) => super::domain::domain_sql(w, lower, args)?,
        _ if duck => match super::builtins::duck_call(w, lower, &a, args)? {
            Some(sql) => sql,
            // A VBA function: the column stays, without values.
            None if !is_builtin(lower) => {
                w.out.notes.push(format!("{name}() is a VBA function, so its column is empty"));
                "NULL".into()
            }
            None => return Err(format!("{name}() is not supported")),
        },
        _ => return Err(format!("{name}() is not supported")),
    })
}

/// Access built-ins with no translation (as opposed to VBA functions of the database).
fn is_builtin(name: &str) -> bool {
    [
        "eval",
        "dlookup",
        "tempvars",
        "currentuser",
        "environ",
        "msgbox",
        "inputbox",
        "shell",
        "strconv",
        "weekdayname",
        "monthname",
        "formatcurrency",
        "formatnumber",
        "formatpercent",
        "formatdatetime",
        "partition",
        "iserror",
        "typename",
        "vartype",
        "hex",
        "oct",
        "rnd",
        "timer",
        "timeserial",
    ]
    .contains(&name)
}

pub enum LikePattern {
    Like(String),
    Regex(String),
}

/// Access wildcards: `*` any text, `?` one character, `#` one digit,
/// `[abc]` / `[!abc]` a character class.
pub fn like_pattern(p: &str) -> LikePattern {
    if !p.contains('#') && !p.contains('[') {
        let mut out = String::new();
        for c in p.chars() {
            match c {
                '*' => out.push('%'),
                '?' => out.push('_'),
                '%' | '_' | '\\' => {
                    out.push('\\');
                    out.push(c);
                }
                _ => out.push(c),
            }
        }
        return LikePattern::Like(out);
    }
    let mut out = String::new();
    let mut chars = p.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => out.push_str(".*"),
            '?' => out.push('.'),
            '#' => out.push_str("[0-9]"),
            '[' => {
                out.push('[');
                if chars.peek() == Some(&'!') {
                    chars.next();
                    out.push('^');
                }
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    if d == '\\' || d == '^' {
                        out.push('\\');
                    }
                    out.push(d);
                }
                out.push(']');
            }
            _ => {
                if "\\.+()|{}$^".contains(c) {
                    out.push('\\');
                }
                out.push(c);
            }
        }
    }
    LikePattern::Regex(out)
}

/// `#m/d/yyyy [h:mm[:ss] [AM|PM]]#` or `#yyyy-mm-dd#` → ISO text.
pub fn access_date(raw: &str) -> Option<String> {
    let raw = raw.trim();
    let (date_part, time_part) = match raw.find(' ') {
        Some(i) if raw[..i].contains(['/', '-']) => (&raw[..i], raw[i + 1..].trim()),
        _ if raw.contains(':') && !raw.contains(['/', '-']) => ("", raw),
        _ => (raw, ""),
    };
    let date = if date_part.is_empty() {
        None
    } else {
        let nums: Vec<i64> = date_part
            .split(['/', '-', '.'])
            .map(|x| x.trim().parse().ok())
            .collect::<Option<Vec<_>>>()?;
        let (y, m, d) = match nums.as_slice() {
            [y, m, d] if *y > 31 => (*y, *m, *d),
            [m, d, y] => (*y, *m, *d),
            [m, d] => (
                chrono::Datelike::year(&chrono::Local::now().date_naive()) as i64,
                *m,
                *d,
            ),
            _ => return None,
        };
        let y = match y {
            0..=29 => 2000 + y,
            30..=99 => 1900 + y,
            _ => y,
        };
        Some(chrono::NaiveDate::from_ymd_opt(
            y as i32, m as u32, d as u32,
        )?)
    };
    let time = if time_part.is_empty() {
        None
    } else {
        let upper = time_part.to_ascii_uppercase();
        let pm = upper.ends_with("PM");
        let am = upper.ends_with("AM");
        let t = upper.trim_end_matches("PM").trim_end_matches("AM").trim();
        let nums: Vec<u32> = t
            .split(':')
            .map(|x| x.trim().parse().ok())
            .collect::<Option<Vec<_>>>()?;
        let mut h = *nums.first()?;
        if pm && h < 12 {
            h += 12;
        }
        if am && h == 12 {
            h = 0;
        }
        Some(chrono::NaiveTime::from_hms_opt(
            h,
            *nums.get(1).unwrap_or(&0),
            *nums.get(2).unwrap_or(&0),
        )?)
    };
    Some(match (date, time) {
        (Some(d), None) => d.format("%Y-%m-%d").to_string(),
        (Some(d), Some(t)) => d.and_time(t).format("%Y-%m-%dT%H:%M:%S").to_string(),
        (None, Some(t)) => format!("1899-12-30T{}", t.format("%H:%M:%S")),
        (None, None) => return None,
    })
}
