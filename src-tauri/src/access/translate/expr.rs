//! Access expressions → ixtable expressions (`src/expr`), for form controls,
//! validation rules, defaults and report text boxes.
use super::ast::*;
use super::format::format_pattern;
use super::functions::{access_date, interval};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Forms, field rules and defaults: names are `record` fields.
    Form,
    /// Report text boxes: names are `record` fields, aggregates run over `rows`.
    Report,
}

/// Maps a bare name (field or control) to an ixtable expression.
pub type Resolver<'a> = &'a dyn Fn(&str) -> Option<String>;

pub struct ExprWriter<'a> {
    pub target: Target,
    pub resolve: Resolver<'a>,
    /// Inside the second argument of `sumof` and friends names are item fields.
    in_item: bool,
}

/// A field reference: `record.Name` or `record.[Name with spaces]`.
pub fn field(root: &str, name: &str) -> String {
    format!("{root}.{}", ident(name))
}

pub fn ident(name: &str) -> String {
    let plain = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    if plain && !is_reserved(name) {
        name.to_string()
    } else {
        format!("[{}]", name.replace(']', ""))
    }
}

fn is_reserved(n: &str) -> bool {
    [
        "and", "or", "not", "in", "is", "null", "true", "false", "between",
    ]
    .contains(&n.to_ascii_lowercase().as_str())
}

pub fn quote_text(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

impl<'a> ExprWriter<'a> {
    pub fn new(target: Target, resolve: Resolver<'a>) -> Self {
        Self {
            target,
            resolve,
            in_item: false,
        }
    }

    pub fn write(&mut self, e: &Expr) -> Result<String, String> {
        Ok(match e {
            Expr::Null => "null".into(),
            Expr::Bool(b) => b.to_string(),
            Expr::Num(n) => n.clone(),
            Expr::Str(s) => quote_text(s),
            Expr::Date(d) => format!(
                "#{}#",
                access_date(d).ok_or_else(|| format!("unrecognized date #{d}#"))?
            ),
            Expr::Name(parts) => self.name(parts)?,
            Expr::Paren(x) => format!("({})", self.write(x)?),
            Expr::Neg(x) => format!("-{}", self.write(x)?),
            Expr::Not(x) => format!("not ({})", self.write(x)?),
            Expr::Bin(op, l, r) => self.binary(*op, l, r)?,
            Expr::Between { expr, lo, hi, not } => format!(
                "{} {}between {} and {}",
                self.write(expr)?,
                if *not { "not " } else { "" },
                self.write(lo)?,
                self.write(hi)?
            ),
            Expr::In { expr, list, not } => {
                let items: Result<Vec<String>, String> =
                    list.iter().map(|i| self.write(i)).collect();
                format!(
                    "{} {}in ({})",
                    self.write(expr)?,
                    if *not { "not " } else { "" },
                    items?.join(", ")
                )
            }
            Expr::IsNull { expr, not } => format!(
                "{} is {}null",
                self.write(expr)?,
                if *not { "not " } else { "" }
            ),
            Expr::Like { expr, pattern, not } => {
                let s = self.like(expr, pattern)?;
                if *not {
                    format!("not ({s})")
                } else {
                    s
                }
            }
            Expr::Call { name, args, .. } => self.call(name, args)?,
            Expr::Star(_) => return Err("* is not an expression".into()),
            Expr::Sub(_) | Expr::InSelect { .. } | Expr::Exists(_) => {
                return Err("subqueries are not supported in expressions".into())
            }
        })
    }

    fn name(&mut self, parts: &[Part]) -> Result<String, String> {
        let texts: Vec<&str> = parts.iter().map(|p| p.text.as_str()).collect();
        match texts.as_slice() {
            [one] => {
                let lower = one.to_ascii_lowercase();
                if self.target == Target::Report && !self.in_item {
                    match lower.as_str() {
                        "page" => return Ok("page".into()),
                        "pages" => return Ok("pages".into()),
                        _ => {}
                    }
                }
                if self.in_item {
                    return Ok(ident(one));
                }
                (self.resolve)(one)
                    .ok_or_else(|| format!("[{one}] is not a field of the record source"))
            }
            [tv, key] if tv.eq_ignore_ascii_case("tempvars") => Ok(field("app", key)),
            // A report shows all of its rows: no Access filter is ever on.
            [r, f] if r.eq_ignore_ascii_case("report") && f.eq_ignore_ascii_case("filter") => {
                Ok("''".into())
            }
            [r, f] if r.eq_ignore_ascii_case("report") && f.eq_ignore_ascii_case("filteron") => {
                Ok("false".into())
            }
            [me, rest]
                if me.eq_ignore_ascii_case("me")
                    || me.eq_ignore_ascii_case("form")
                    || me.eq_ignore_ascii_case("report") =>
            {
                self.name(&[Part {
                    text: rest.to_string(),
                    bang: false,
                }])
            }
            // Table.Field inside a single-source form or report.
            [_, f] if !parts[1].bang => self.name(&[Part {
                text: f.to_string(),
                bang: false,
            }]),
            _ => Err(format!(
                "{} refers to another form or object",
                texts.join("!")
            )),
        }
    }

    fn binary(&mut self, op: BinOp, l: &Expr, r: &Expr) -> Result<String, String> {
        // InStr(...) > 0 is a containment test.
        if let (Expr::Call { name, args, .. }, Expr::Num(n)) = (l, r) {
            if name.eq_ignore_ascii_case("InStr") && args.len() == 2 && n == "0" {
                let t = format!(
                    "contains({}, {})",
                    self.write(&args[0])?,
                    self.write(&args[1])?
                );
                match op {
                    BinOp::Gt | BinOp::Ne => return Ok(t),
                    BinOp::Eq => return Ok(format!("not {t}")),
                    _ => {}
                }
            }
        }
        let (ls, rs) = (self.write(l)?, self.write(r)?);
        let o = match op {
            BinOp::Or => "or",
            BinOp::And => "and",
            BinOp::Eq => "=",
            BinOp::Ne => "<>",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Concat => "&",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::IntDiv => return Ok(format!("floor({ls} / {rs})")),
            BinOp::Pow | BinOp::Xor | BinOp::Eqv | BinOp::Imp => {
                return Err(format!("operator {op:?} is not supported"))
            }
        };
        Ok(format!("{ls} {o} {rs}"))
    }

    fn like(&mut self, expr: &Expr, pattern: &Expr) -> Result<String, String> {
        let e = self.write(expr)?;
        let Expr::Str(p) = pattern else {
            return self.computed_like(&e, pattern);
        };
        let inner = p.trim_matches('*');
        let plain = !inner.contains(['*', '?', '#', '[']);
        if plain {
            let starts = p.starts_with('*');
            let ends = p.ends_with('*');
            return Ok(match (starts, ends) {
                (true, true) => format!("contains({e}, {})", quote_text(inner)),
                (false, true) => format!("startswith({e}, {})", quote_text(inner)),
                (true, false) => format!("endswith({e}, {})", quote_text(inner)),
                (false, false) => format!("lower({e}) = lower({})", quote_text(inner)),
            });
        }
        match super::functions::like_pattern(p) {
            super::functions::LikePattern::Regex(r) => Ok(format!(
                "regexmatch({e}, {})",
                quote_text(&format!("^{r}$"))
            )),
            super::functions::LikePattern::Like(l) => {
                let r: String = l.replace('%', ".*").replace('_', ".");
                Ok(format!(
                    "regexmatch({e}, {})",
                    quote_text(&format!("^{r}$"))
                ))
            }
        }
    }

    /// `Like "*" & [Find] & "*"`: a search box's pattern around a value.
    fn computed_like(&mut self, e: &str, pattern: &Expr) -> Result<String, String> {
        let star = |x: &Expr| matches!(x, Expr::Str(s) if s == "*");
        let (lead, middle, trail) = match pattern {
            Expr::Bin(BinOp::Concat, l, r) => match (&**l, &**r) {
                (Expr::Bin(BinOp::Concat, a, b), c) if star(a) && star(c) => (true, &**b, true),
                (a, b) if star(a) => (true, b, false),
                (a, b) if star(b) => (false, a, true),
                _ => return Err("Like with a computed pattern".into()),
            },
            _ => return Err("Like with a computed pattern".into()),
        };
        let v = self.write(middle)?;
        Ok(match (lead, trail) {
            (true, true) => format!("contains({e}, {v})"),
            (true, false) => format!("endswith({e}, {v})"),
            _ => format!("startswith({e}, {v})"),
        })
    }

    fn aggregate(&mut self, f: &str, args: &[Expr]) -> Result<String, String> {
        if self.target != Target::Report || self.in_item {
            return Err(format!("{f}() is only supported in reports"));
        }
        if f == "count" && matches!(args.first(), Some(Expr::Star(_))) {
            return Ok("count(rows)".into());
        }
        let arg = args.first().ok_or("aggregate needs an argument")?;
        if let Expr::Name(parts) = arg {
            if let Some(last) = parts.last() {
                return Ok(format!("{f}({})", field("rows", &last.text)));
            }
        }
        self.in_item = true;
        let inner = self.write(arg);
        self.in_item = false;
        let inner = inner?;
        Ok(match f {
            "count" => format!("countof(rows, {inner} is not null)"),
            other => format!("{other}of(rows, {inner})"),
        })
    }

    fn call(&mut self, name: &str, args: &[Expr]) -> Result<String, String> {
        let lower = name.to_ascii_lowercase();
        let lower = lower.trim_end_matches('$');
        match lower {
            "sum" | "avg" | "count" | "min" | "max" => return self.aggregate(lower, args),
            "dateadd" | "datediff" => {
                let Some(Expr::Str(code)) = args.first() else {
                    return Err(format!("{name} needs a literal interval"));
                };
                let (unit, mult) = match code.to_ascii_lowercase().as_str() {
                    "ww" => ("week", 1),
                    "w" | "y" => ("day", 1),
                    c => interval(c).ok_or("unknown interval")?,
                };
                let a: Result<Vec<String>, String> =
                    args[1..].iter().map(|x| self.write(x)).collect();
                let a = a?;
                if a.len() < 2 {
                    return Err(format!("{name} needs 3 arguments"));
                }
                let n = if mult == 1 {
                    a[0].clone()
                } else {
                    format!("({}) * {mult}", a[0])
                };
                return Ok(format!("{lower}('{unit}', {n}, {})", a[1]));
            }
            "datepart" => {
                let Some(Expr::Str(code)) = args.first() else {
                    return Err("DatePart needs a literal interval".into());
                };
                let d = self.write(args.get(1).ok_or("DatePart needs 2 arguments")?)?;
                return Ok(match code.to_ascii_lowercase().as_str() {
                    "yyyy" => format!("year({d})"),
                    "q" => format!("floor((month({d}) - 1) / 3) + 1"),
                    "m" => format!("month({d})"),
                    "d" => format!("day({d})"),
                    "w" => format!("weekday({d})"),
                    "h" => format!("hour({d})"),
                    "n" => format!("minute({d})"),
                    other => return Err(format!("DatePart \"{other}\" is not supported")),
                });
            }
            "strconv" => {
                let v = self.write(args.first().ok_or("StrConv needs 2 arguments")?)?;
                return match args.get(1) {
                    Some(Expr::Num(n)) if n == "1" => Ok(format!("upper({v})")),
                    Some(Expr::Num(n)) if n == "2" => Ok(format!("lower({v})")),
                    _ => Err("StrConv supports only vbUpperCase and vbLowerCase".into()),
                };
            }
            "monthname" => {
                let m = self.write(args.first().ok_or("MonthName needs a month")?)?;
                let abbreviated = matches!(args.get(1), Some(Expr::Bool(true)))
                    || matches!(args.get(1), Some(Expr::Num(n)) if n != "0");
                let p = if abbreviated { "MMM" } else { "MMMM" };
                return Ok(format!("format(date(2000, {m}, 1), '{p}')"));
            }
            "switch" => {
                if args.len() < 2 || args.len() % 2 != 0 {
                    return Err("Switch needs condition/value pairs".into());
                }
                let mut out = "null".to_string();
                for pair in args.chunks(2).rev() {
                    out = format!("if({}, {}, {out})", self.write(&pair[0])?, self.write(&pair[1])?);
                }
                return Ok(out);
            }
            "choose" => {
                let index = self.write(args.first().ok_or("Choose needs an index")?)?;
                let mut out = "null".to_string();
                for (i, v) in args.iter().enumerate().skip(1).rev() {
                    out = format!("if({index} = {i}, {}, {out})", self.write(v)?);
                }
                return Ok(out);
            }
            "format" => {
                let value = self.write(args.first().ok_or("Format needs a value")?)?;
                let Some(Expr::Str(f)) = args.get(1) else {
                    return Ok(format!("text({value})"));
                };
                return Ok(match format_pattern(f) {
                    Some(p) => format!("format({value}, {})", quote_text(&p)),
                    None => format!("text({value})"),
                });
            }
            _ => {}
        }
        let a: Result<Vec<String>, String> = args.iter().map(|x| self.write(x)).collect();
        let a = a?;
        let need = |k: usize| {
            if a.len() < k {
                Err(format!("{name} needs {k} arguments"))
            } else {
                Ok(())
            }
        };
        Ok(match lower {
            "iif" => {
                need(2)?;
                format!(
                    "if({}, {}, {})",
                    a[0],
                    a[1],
                    a.get(2).cloned().unwrap_or_else(|| "null".into())
                )
            }
            "nz" => {
                need(1)?;
                format!(
                    "coalesce({}, {})",
                    a[0],
                    a.get(1).cloned().unwrap_or_else(|| "''".into())
                )
            }
            "isnull" | "isempty" => format!("isnull({})", a[0]),
            "date" if a.is_empty() => "today()".into(),
            "now" if a.is_empty() => "now()".into(),
            "time" if a.is_empty() => "format(now(), 'HH:mm:ss')".into(),
            "year" | "month" | "day" | "hour" | "minute" | "weekday" => {
                format!("{lower}({})", a[0])
            }
            "dateserial" => {
                need(3)?;
                format!("date({}, {}, {})", a[0], a[1], a[2])
            }
            "len" => format!("len({})", a[0]),
            "left" | "right" => format!("{lower}({}, {})", a[0], a[1]),
            "mid" => format!("mid({})", a.join(", ")),
            "trim" | "ltrim" | "rtrim" => format!("trim({})", a[0]),
            "ucase" => format!("upper({})", a[0]),
            "lcase" => format!("lower({})", a[0]),
            "replace" => format!("replace({}, {}, {})", a[0], a[1], a[2]),
            "round" => format!("round({})", a.join(", ")),
            "int" => format!("floor({})", a[0]),
            "fix" => format!("if({0} < 0, ceil({0}), floor({0}))", a[0]),
            "abs" => format!("abs({})", a[0]),
            "cstr" => format!("text({})", a[0]),
            "clng" | "cint" => format!("round(number({}))", a[0]),
            "cdbl" | "csng" | "ccur" | "cdec" | "val" => format!("number({})", a[0]),
            "cdate" | "datevalue" => a[0].clone(),
            "coalesce" => format!("coalesce({})", a.join(", ")),
            "sgn" => format!("if({0} > 0, 1, if({0} < 0, -1, 0))", a[0]),
            "iserror" => "false".into(),
            _ => return Err(format!("{name}() is not supported")),
        })
    }
}

/// Translates Access expression text (with or without a leading `=`).
pub fn translate(text: &str, target: Target, resolve: Resolver) -> Result<String, String> {
    let body = text.trim().strip_prefix('=').unwrap_or(text.trim());
    let e = parse_expression(body)?;
    ExprWriter::new(target, resolve).write(&e)
}
