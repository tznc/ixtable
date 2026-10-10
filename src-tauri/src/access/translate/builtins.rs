//! Access built-in functions in DuckDB SQL beyond the core set in
//! `functions.rs`: text case and names, number and date formatting,
//! `Partition`, and the less common math and string functions.
use super::ast::*;
use super::sql::{string_literal, SqlWriter};

fn arg_int(args: &[Expr], i: usize) -> Option<i64> {
    match args.get(i) {
        Some(Expr::Num(n)) => n.parse().ok(),
        Some(Expr::Neg(x)) => match &**x {
            Expr::Num(n) => n.parse::<i64>().ok().map(|v| -v),
            _ => None,
        },
        Some(Expr::Bool(b)) => Some(if *b { -1 } else { 0 }),
        _ => None,
    }
}

/// True when the argument is absent or a literal false/0.
fn is_false(args: &[Expr], i: usize) -> bool {
    args.get(i).is_none() || arg_int(args, i) == Some(0)
}

/// Upper-cases the first letter of each word.
fn proper(s: &str) -> String {
    format!("array_to_string(list_transform(string_split(lower({s}), ' '), w -> concat(upper(left(w, 1)), substring(w, 2))), ' ')")
}

/// Translates `lower` (a lower-case Access function name) when DuckDB can
/// express it. `a` holds the translated arguments. None: not a known built-in.
pub fn duck_call(
    w: &mut SqlWriter,
    lower: &str,
    a: &[String],
    args: &[Expr],
) -> Result<Option<String>, String> {
    let need = |k: usize| {
        if a.len() < k {
            Err(format!("{lower} needs {k} arguments"))
        } else {
            Ok(())
        }
    };
    let sql = match lower {
        "strconv" => {
            need(2)?;
            match arg_int(args, 1) {
                Some(1) => format!("upper({})", a[0]),
                Some(2) => format!("lower({})", a[0]),
                Some(3) => proper(&a[0]),
                _ => {
                    return Err(
                        "StrConv supports only vbUpperCase, vbLowerCase and vbProperCase".into(),
                    )
                }
            }
        }
        "monthname" => {
            need(1)?;
            let f = if is_false(args, 1) { "%B" } else { "%b" };
            format!(
                "strftime(make_date(2000, CAST({} AS INTEGER), 1), '{f}')",
                a[0]
            )
        }
        "weekdayname" => {
            need(1)?;
            if !is_false(args, 2) && arg_int(args, 2) != Some(1) {
                return Err("WeekdayName with a first day other than Sunday".into());
            }
            let f = if is_false(args, 1) { "%A" } else { "%a" };
            // 2000-01-01 was a Saturday, so day 1 (Sunday) is the 2nd.
            format!(
                "strftime(make_date(2000, 1, 1 + CAST({} AS INTEGER)), '{f}')",
                a[0]
            )
        }
        "formatcurrency" | "formatnumber" | "formatpercent" => {
            need(1)?;
            let digits = match args.get(1) {
                None => 2,
                Some(_) => arg_int(args, 1)
                    .filter(|d| (0..=10).contains(d))
                    .ok_or_else(|| format!("{lower} needs literal digits"))?,
            };
            let (prefix, scale, suffix) = match lower {
                "formatcurrency" => ("$", "", ""),
                "formatpercent" => ("", " * 100", "%"),
                _ => ("", "", ""),
            };
            // fmt rounds half to even; Access rounds half away from zero.
            let core = format!(
                "format('{{:,.{digits}f}}', round(CAST(({}){scale} AS DOUBLE), {digits}))",
                a[0]
            );
            wrap(prefix, core, suffix)
        }
        "formatdatetime" => {
            need(1)?;
            let f = match arg_int(args, 1).unwrap_or(0) {
                0 => "%-m/%-d/%Y %-I:%M:%S %p",
                1 => "%A, %B %-d, %Y",
                2 => "%-m/%-d/%Y",
                3 => "%-I:%M:%S %p",
                4 => "%H:%M",
                _ => return Err("FormatDateTime needs a literal named format".into()),
            };
            format!("strftime(CAST({} AS TIMESTAMP), '{f}')", a[0])
        }
        "partition" => {
            need(4)?;
            partition(a)
        }
        "hex" => format!("upper(to_hex(CAST({} AS BIGINT)))", a[0]),
        "oct" => format!("printf('%o', CAST({} AS BIGINT))", a[0]),
        "rnd" => "random()".into(),
        "timer" => {
            "(epoch(CAST(now() AS TIMESTAMP)) - epoch(CAST(CAST(now() AS TIMESTAMP) AS DATE)))"
                .into()
        }
        "timeserial" => {
            need(3)?;
            format!(
                "(TIME '00:00:00' + to_seconds(CAST({} AS BIGINT) * 3600 + CAST({} AS BIGINT) * 60 + CAST({} AS BIGINT)))",
                a[0], a[1], a[2]
            )
        }
        "instrrev" => {
            need(2)?;
            if a.len() > 2 {
                return Err("InStrRev with a start position".into());
            }
            // Position of the last match: 0 when absent.
            format!(
                "(CASE WHEN instr({0}, {1}) = 0 THEN 0 ELSE length({0}) - instr(reverse({0}), reverse({1})) - length({1}) + 2 END)",
                a[0], a[1]
            )
        }
        "strreverse" => format!("reverse({})", a[0]),
        "strcomp" => {
            need(2)?;
            // Text compare (the Access default in queries) ignores case.
            let binary = arg_int(args, 2) == Some(0);
            let (l, r) = if binary {
                (a[0].clone(), a[1].clone())
            } else {
                (format!("lower({})", a[0]), format!("lower({})", a[1]))
            };
            format!("(CASE WHEN {l} IS NULL OR {r} IS NULL THEN NULL WHEN {l} < {r} THEN -1 WHEN {l} > {r} THEN 1 ELSE 0 END)")
        }
        "atn" => format!("atan({})", a[0]),
        "sin" | "cos" | "tan" => format!("{lower}({})", a[0]),
        "iserror" => "FALSE".into(),
        "eval" => match args.first() {
            Some(Expr::Str(text)) => {
                let e = parse_expression(text).map_err(|e| format!("Eval: {e}"))?;
                w.expr(&e)?
            }
            _ => return Err("Eval with a computed expression".into()),
        },
        _ => return Ok(None),
    };
    Ok(Some(sql))
}

fn wrap(prefix: &str, core: String, suffix: &str) -> String {
    match (prefix.is_empty(), suffix.is_empty()) {
        (true, true) => core,
        _ => format!(
            "concat({}, {core}, {})",
            string_literal(prefix),
            string_literal(suffix)
        ),
    }
}

/// `Partition(n, start, stop, interval)`: the range label `" 10: 19"`.
fn partition(a: &[String]) -> String {
    let n = &format!("CAST(floor({}) AS BIGINT)", a[0]);
    let (start, stop, step) = (&a[1], &a[2], &a[3]);
    // Access pads both numbers to the width of the larger bound.
    let width = format!("CAST(greatest(length(CAST({stop} + 1 AS VARCHAR)), length(CAST({start} - 1 AS VARCHAR))) AS INTEGER)");
    let pad = |x: String| format!("lpad(CAST({x} AS VARCHAR), {width}, ' ')");
    let lo = format!("({start} + (({n}) - {start}) // {step} * {step})");
    let hi = format!("least({lo} + {step} - 1, {stop})");
    format!(
        "(CASE WHEN {} IS NULL THEN NULL WHEN {n} < {start} THEN concat({}, ':', {}) WHEN {n} > {stop} THEN concat({}, ':', {}) ELSE concat({}, ':', {}) END)",
        a[0],
        pad("''".into()),
        pad(format!("{start} - 1")),
        pad(format!("{stop} + 1")),
        pad("''".into()),
        pad(lo.clone()),
        pad(hi)
    )
}

/// `Format(value, "pattern")` with a named or custom Access format.
pub fn format_sql(value: &str, pattern: Option<&Expr>) -> Result<String, String> {
    let Some(pattern) = pattern else {
        return Ok(format!("CAST({value} AS VARCHAR)"));
    };
    let Expr::Str(fmt) = pattern else {
        return Err("Format needs a literal format".into());
    };
    let named = match fmt.trim().to_ascii_lowercase().as_str() {
        "short date" => Some("%-m/%-d/%Y"),
        "medium date" => Some("%d-%b-%y"),
        "long date" => Some("%A, %B %-d, %Y"),
        "general date" => Some("%-m/%-d/%Y %-I:%M:%S %p"),
        "short time" => Some("%H:%M"),
        "medium time" => Some("%-I:%M %p"),
        "long time" => Some("%-I:%M:%S %p"),
        "ww" => return Ok(format!("CAST(week({value}) AS VARCHAR)")),
        "q" => return Ok(format!("CAST(quarter({value}) AS VARCHAR)")),
        "y" => return Ok(format!("CAST(dayofyear({value}) AS VARCHAR)")),
        "w" => return Ok(format!("CAST(dayofweek({value}) + 1 AS VARCHAR)")),
        "currency" | "euro" => return Ok(number_sql(value, &number_spec("$#,##0.00"))),
        "fixed" => return Ok(number_sql(value, &number_spec("0.00"))),
        "standard" => return Ok(number_sql(value, &number_spec("#,##0.00"))),
        "percent" => return Ok(number_sql(value, &number_spec("0.00%"))),
        "general number" | "general" | "" => return Ok(format!("CAST({value} AS VARCHAR)")),
        "yes/no" => return Ok(format!("CASE WHEN {value} THEN 'Yes' ELSE 'No' END")),
        "true/false" => return Ok(format!("CASE WHEN {value} THEN 'True' ELSE 'False' END")),
        "on/off" => return Ok(format!("CASE WHEN {value} THEN 'On' ELSE 'Off' END")),
        ">" => return Ok(format!("upper(CAST({value} AS VARCHAR))")),
        "<" => return Ok(format!("lower(CAST({value} AS VARCHAR))")),
        _ => None,
    };
    if let Some(f) = named {
        return Ok(format!("strftime(CAST({value} AS TIMESTAMP), '{f}')"));
    }
    let section = first_section(fmt);
    if section.contains(['0', '#']) && !is_date_pattern(&section) {
        return Ok(number_sql(value, &number_spec(&section)));
    }
    if is_date_pattern(&section) {
        let f = strftime_pattern(&section)?;
        return Ok(format!(
            "strftime(CAST({value} AS TIMESTAMP), {})",
            string_literal(&f)
        ));
    }
    Err(format!("the format \"{fmt}\" is not supported"))
}

fn first_section(s: &str) -> String {
    let mut out = String::new();
    let mut quoted = false;
    for c in s.chars() {
        if c == '"' {
            quoted = !quoted;
        }
        if c == ';' && !quoted {
            break;
        }
        out.push(c);
    }
    out
}

fn is_date_pattern(s: &str) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '"' => quoted = !quoted,
            '\\' => escaped = true,
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 'n' | 'N' | 's' | 'S' if !quoted => {
                return true
            }
            _ => {}
        }
    }
    false
}

/// Access date tokens → strftime. `m` after an hour means minutes.
pub fn strftime_pattern(p: &str) -> Result<String, String> {
    let chars: Vec<char> = p.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut after_hour = false;
    let run = |i: usize, c: char| {
        chars[i..]
            .iter()
            .take_while(|x| x.eq_ignore_ascii_case(&c))
            .count()
    };
    let lit = |out: &mut String, c: char| {
        if c == '%' {
            out.push_str("%%");
        } else {
            out.push(c);
        }
    };
    while i < chars.len() {
        let c = chars[i];
        let rest: String = chars[i..].iter().collect::<String>().to_ascii_lowercase();
        if rest.starts_with("am/pm") {
            out.push_str("%p");
            i += 5;
            continue;
        }
        if rest.starts_with("a/p") {
            out.push_str("%p");
            i += 3;
            continue;
        }
        if rest.starts_with("ampm") {
            out.push_str("%p");
            i += 4;
            continue;
        }
        match c.to_ascii_lowercase() {
            'y' => {
                let n = run(i, 'y');
                out.push_str(match n {
                    1 => "%j",
                    2 => "%y",
                    _ => "%Y",
                });
                i += n;
            }
            'm' => {
                let n = run(i, 'm');
                let minutes = after_hour && n <= 2 || next_is_seconds(&chars, i + n);
                out.push_str(match (minutes, n) {
                    (true, 1) => "%-M",
                    (true, _) => "%M",
                    (false, 1) => "%-m",
                    (false, 2) => "%m",
                    (false, 3) => "%b",
                    _ => "%B",
                });
                after_hour = false;
                i += n;
            }
            'd' => {
                let n = run(i, 'd');
                out.push_str(match n {
                    1 => "%-d",
                    2 => "%d",
                    3 => "%a",
                    4 => "%A",
                    5 => "%-m/%-d/%Y",
                    _ => "%A, %B %-d, %Y",
                });
                i += n;
            }
            'h' => {
                let n = run(i, 'h');
                let twelve = chars[i..].iter().collect::<String>().to_ascii_lowercase();
                let twelve =
                    twelve.contains("am/pm") || twelve.contains("a/p") || twelve.contains("ampm");
                out.push_str(match (twelve, n) {
                    (true, 1) => "%-I",
                    (true, _) => "%I",
                    (false, 1) => "%-H",
                    (false, _) => "%H",
                });
                after_hour = true;
                i += n;
            }
            'n' => {
                let n = run(i, 'n');
                out.push_str(if n == 1 { "%-M" } else { "%M" });
                i += n;
            }
            's' => {
                let n = run(i, 's');
                out.push_str(if n == 1 { "%-S" } else { "%S" });
                i += n;
            }
            'q' => return Err("the quarter in a date format".into()),
            'w' => return Err("the week in a date format".into()),
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    lit(&mut out, chars[i]);
                    i += 1;
                }
                i += 1;
            }
            '\\' => {
                if let Some(&n) = chars.get(i + 1) {
                    lit(&mut out, n);
                }
                i += 2;
            }
            _ => {
                lit(&mut out, c);
                i += 1;
            }
        }
    }
    Ok(out)
}

fn next_is_seconds(chars: &[char], from: usize) -> bool {
    chars[from..]
        .iter()
        .find(|c| c.is_ascii_alphabetic())
        .is_some_and(|c| c.eq_ignore_ascii_case(&'s'))
}

#[derive(Debug, PartialEq)]
pub struct NumberSpec {
    pub prefix: String,
    pub suffix: String,
    pub grouping: bool,
    pub decimals: usize,
    /// Zeros before the decimal point: the minimum integer digits.
    pub zeros: usize,
    pub percent: bool,
}

/// Reads `$#,##0.00`, `0.0%`, `000`, `"Qty: "0`.
pub fn number_spec(p: &str) -> NumberSpec {
    let mut spec = NumberSpec {
        prefix: String::new(),
        suffix: String::new(),
        grouping: false,
        decimals: 0,
        zeros: 0,
        percent: false,
    };
    let chars: Vec<char> = p.chars().collect();
    let mut i = 0;
    let mut seen_digit = false;
    let mut after_point = false;
    while i < chars.len() {
        let c = chars[i];
        let text = if seen_digit {
            &mut spec.suffix
        } else {
            &mut spec.prefix
        };
        match c {
            '0' | '#' => {
                // Digits after a suffix started are part of the number again.
                seen_digit = true;
                if after_point {
                    if c == '0' {
                        spec.decimals += 1;
                    }
                } else if c == '0' {
                    spec.zeros += 1;
                }
            }
            ',' if seen_digit && !after_point => spec.grouping = true,
            '.' if seen_digit || chars.get(i + 1).is_some_and(|n| *n == '0' || *n == '#') => {
                seen_digit = true;
                after_point = true;
            }
            '%' => {
                spec.percent = true;
                text.push('%');
            }
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    text.push(chars[i]);
                    i += 1;
                }
            }
            '\\' => {
                if let Some(&n) = chars.get(i + 1) {
                    text.push(n);
                }
                i += 1;
            }
            _ => text.push(c),
        }
        i += 1;
    }
    spec
}

pub fn number_sql(value: &str, spec: &NumberSpec) -> String {
    let d = spec.decimals;
    // fmt rounds half to even; Access rounds half away from zero.
    let scaled = if spec.percent {
        format!("round(CAST(({value}) * 100 AS DOUBLE), {d})")
    } else {
        format!("round(CAST({value} AS DOUBLE), {d})")
    };
    let core = if spec.grouping {
        format!("format('{{:,.{d}f}}', {scaled})")
    } else if spec.zeros > 1 {
        let width = spec.zeros + if d > 0 { d + 1 } else { 0 };
        format!("format('{{:0{width}.{d}f}}', {scaled})")
    } else {
        format!("format('{{:.{d}f}}', {scaled})")
    };
    let core = if spec.zeros == 0 && !spec.grouping {
        // `#.00` drops the leading zero of a fraction.
        format!("regexp_replace({core}, '^(-?)0\\.', '\\1.')")
    } else {
        core
    };
    format!(
        "CASE WHEN {value} IS NULL THEN NULL ELSE {} END",
        wrap(&spec.prefix, core, &spec.suffix)
    )
}
