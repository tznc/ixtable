//! Syntax tree and parser for Access expressions and query statements: SELECT,
//! TRANSFORM, and the action statements INSERT, UPDATE and DELETE.
use super::lexer::{lex, Tok};

#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub text: String,
    /// Written after `!` rather than `.` (form and control references).
    pub bang: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    Xor,
    And,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Concat,
    Add,
    Sub,
    Mod,
    IntDiv,
    Mul,
    Div,
    Pow,
    Eqv,
    Imp,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    /// Raw text between `#` signs.
    Date(String),
    Name(Vec<Part>),
    /// `*` or `T.*`.
    Star(Vec<String>),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
    Like {
        expr: Box<Expr>,
        pattern: Box<Expr>,
        not: bool,
    },
    Between {
        expr: Box<Expr>,
        lo: Box<Expr>,
        hi: Box<Expr>,
        not: bool,
    },
    In {
        expr: Box<Expr>,
        list: Vec<Expr>,
        not: bool,
    },
    InSelect {
        expr: Box<Expr>,
        select: Box<Select>,
        not: bool,
    },
    IsNull {
        expr: Box<Expr>,
        not: bool,
    },
    Call {
        name: String,
        args: Vec<Expr>,
        distinct: bool,
    },
    Sub(Box<Select>),
    Exists(Box<Select>),
    Paren(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub enum From {
    Table {
        name: String,
        alias: Option<String>,
    },
    Sub {
        select: Box<Select>,
        alias: Option<String>,
    },
    Join {
        kind: JoinKind,
        left: Box<From>,
        right: Box<From>,
        on: Expr,
    },
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Select {
    pub distinct: bool,
    pub distinct_row: bool,
    /// `TOP n [PERCENT]`.
    pub top: Option<(String, bool)>,
    pub columns: Vec<(Expr, Option<String>)>,
    pub into: Option<String>,
    pub from: Vec<From>,
    pub where_clause: Option<Expr>,
    pub group_by: Vec<Expr>,
    pub having: Option<Expr>,
    pub order_by: Vec<(Expr, bool)>,
    /// Following `UNION [ALL]` parts (`true` for ALL).
    pub unions: Vec<(bool, Select)>,
}

/// A crosstab: `TRANSFORM agg SELECT ... PIVOT expr [IN (values)]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Crosstab {
    pub transform: (Expr, Option<String>),
    pub select: Select,
    pub pivot: Expr,
    pub pivot_in: Vec<Expr>,
}

/// Where an append query's rows come from.
#[derive(Debug, Clone, PartialEq)]
pub enum InsertSource {
    Select(Select),
    Values(Vec<Expr>),
}

/// `INSERT INTO table [(columns)] SELECT ... | VALUES (...)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Insert {
    pub table: String,
    pub columns: Vec<String>,
    pub source: InsertSource,
}

/// `UPDATE from SET target = value, ... [WHERE ...]`; `from` may join tables.
#[derive(Debug, Clone, PartialEq)]
pub struct Update {
    pub from: From,
    pub sets: Vec<(Vec<Part>, Expr)>,
    pub where_clause: Option<Expr>,
}

/// `DELETE [target.*] FROM ... [WHERE ...]`; `target` names the table rows go from.
#[derive(Debug, Clone, PartialEq)]
pub struct Delete {
    pub target: Option<String>,
    pub from: Vec<From>,
    pub where_clause: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    /// A SELECT; `into` set makes it a make-table query.
    Select(Select),
    Crosstab(Crosstab),
    Insert(Insert),
    Update(Update),
    Delete(Delete),
}

pub struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

const CLAUSE_WORDS: [&str; 14] = [
    "FROM", "WHERE", "GROUP", "HAVING", "ORDER", "UNION", "INTO", "PIVOT", "ON", "INNER", "LEFT",
    "RIGHT", "WITH", "IN",
];

impl Parser {
    pub fn new(text: &str) -> Result<Self, String> {
        Ok(Self {
            toks: lex(text)?,
            pos: 0,
        })
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn peek_at(&self, n: usize) -> Option<&Tok> {
        self.toks.get(self.pos + n)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn eat_word(&mut self, w: &str) -> bool {
        if self.peek().is_some_and(|t| t.is_word(w)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn eat_op(&mut self, o: &str) -> bool {
        if self.peek().is_some_and(|t| t.is_op(o)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect_op(&mut self, o: &str) -> Result<(), String> {
        if self.eat_op(o) {
            Ok(())
        } else {
            Err(format!("expected {o} near {:?}", self.peek()))
        }
    }

    fn expect_word(&mut self, w: &str) -> Result<(), String> {
        if self.eat_word(w) {
            Ok(())
        } else {
            Err(format!("expected {w} near {:?}", self.peek()))
        }
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
            || (self.peek().is_some_and(|t| t.is_op(";")) && self.pos + 1 >= self.toks.len())
    }

    /// Parses a whole expression and requires the end of input.
    pub fn whole_expression(mut self) -> Result<Expr, String> {
        let e = self.expr()?;
        if !self.at_end() {
            return Err(format!("unexpected {:?} after the expression", self.peek()));
        }
        Ok(e)
    }

    pub fn expr(&mut self) -> Result<Expr, String> {
        self.imp()
    }

    fn binary_level(
        &mut self,
        ops: &[(&str, BinOp)],
        next: fn(&mut Self) -> Result<Expr, String>,
    ) -> Result<Expr, String> {
        let mut left = next(self)?;
        'outer: loop {
            for (word, op) in ops {
                let matched = match self.peek() {
                    Some(Tok::Word(w)) => w.eq_ignore_ascii_case(word),
                    Some(Tok::Op(o)) => o == word,
                    _ => false,
                };
                if matched {
                    self.pos += 1;
                    let right = next(self)?;
                    left = Expr::Bin(*op, Box::new(left), Box::new(right));
                    continue 'outer;
                }
            }
            return Ok(left);
        }
    }

    fn imp(&mut self) -> Result<Expr, String> {
        self.binary_level(&[("Imp", BinOp::Imp)], Self::eqv)
    }

    fn eqv(&mut self) -> Result<Expr, String> {
        self.binary_level(&[("Eqv", BinOp::Eqv)], Self::xor)
    }

    fn xor(&mut self) -> Result<Expr, String> {
        self.binary_level(&[("Xor", BinOp::Xor)], Self::or)
    }

    fn or(&mut self) -> Result<Expr, String> {
        self.binary_level(&[("Or", BinOp::Or)], Self::and)
    }

    fn and(&mut self) -> Result<Expr, String> {
        self.binary_level(&[("And", BinOp::And)], Self::not)
    }

    fn not(&mut self) -> Result<Expr, String> {
        if self.eat_word("Not") {
            return Ok(Expr::Not(Box::new(self.not()?)));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let left = self.concat()?;
        self.comparison_tail(left)
    }

    /// The comparison part after a left operand; field validation rules start here.
    pub fn comparison_tail(&mut self, left: Expr) -> Result<Expr, String> {
        let cmp = [
            ("=", BinOp::Eq),
            ("<>", BinOp::Ne),
            ("<=", BinOp::Le),
            (">=", BinOp::Ge),
            ("<", BinOp::Lt),
            (">", BinOp::Gt),
        ];
        for (o, op) in cmp {
            if self.eat_op(o) {
                let right = self.concat()?;
                return Ok(Expr::Bin(op, Box::new(left), Box::new(right)));
            }
        }
        let not = self.peek().is_some_and(|t| t.is_word("Not"))
            && self
                .peek_at(1)
                .is_some_and(|t| t.is_word("Like") || t.is_word("Between") || t.is_word("In"));
        if not {
            self.pos += 1;
        }
        if self.eat_word("Like") || self.eat_word("ALike") {
            let pattern = self.concat()?;
            return Ok(Expr::Like {
                expr: Box::new(left),
                pattern: Box::new(pattern),
                not,
            });
        }
        if self.eat_word("Between") {
            let lo = self.concat()?;
            self.expect_word("And")?;
            let hi = self.concat()?;
            return Ok(Expr::Between {
                expr: Box::new(left),
                lo: Box::new(lo),
                hi: Box::new(hi),
                not,
            });
        }
        if self.eat_word("In") {
            self.expect_op("(")?;
            if self.peek().is_some_and(|t| t.is_word("SELECT")) {
                let select = self.select()?;
                self.expect_op(")")?;
                return Ok(Expr::InSelect {
                    expr: Box::new(left),
                    select: Box::new(select),
                    not,
                });
            }
            let mut list = vec![self.expr()?];
            while self.eat_op(",") {
                list.push(self.expr()?);
            }
            self.expect_op(")")?;
            return Ok(Expr::In {
                expr: Box::new(left),
                list,
                not,
            });
        }
        if self.eat_word("Is") {
            let not = self.eat_word("Not");
            self.expect_word("Null")?;
            return Ok(Expr::IsNull {
                expr: Box::new(left),
                not,
            });
        }
        Ok(left)
    }

    fn concat(&mut self) -> Result<Expr, String> {
        self.binary_level(&[("&", BinOp::Concat)], Self::additive)
    }

    fn additive(&mut self) -> Result<Expr, String> {
        self.binary_level(&[("+", BinOp::Add), ("-", BinOp::Sub)], Self::modulo)
    }

    fn modulo(&mut self) -> Result<Expr, String> {
        self.binary_level(&[("Mod", BinOp::Mod)], Self::int_div)
    }

    fn int_div(&mut self) -> Result<Expr, String> {
        self.binary_level(&[("\\", BinOp::IntDiv)], Self::multiplicative)
    }

    fn multiplicative(&mut self) -> Result<Expr, String> {
        self.binary_level(&[("*", BinOp::Mul), ("/", BinOp::Div)], Self::unary)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat_op("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat_op("+") {
            return self.unary();
        }
        self.power()
    }

    fn power(&mut self) -> Result<Expr, String> {
        let base = self.primary()?;
        if self.eat_op("^") {
            let exp = self.unary()?;
            return Ok(Expr::Bin(BinOp::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let tok = self.next().ok_or("unexpected end of expression")?;
        match tok {
            Tok::Num(n) => Ok(Expr::Num(n)),
            Tok::Str(s) => Ok(Expr::Str(s)),
            Tok::Date(d) => Ok(Expr::Date(d)),
            Tok::Op("(") => {
                if self.peek().is_some_and(|t| t.is_word("SELECT")) {
                    let s = self.select()?;
                    self.expect_op(")")?;
                    return Ok(Expr::Sub(Box::new(s)));
                }
                let e = self.expr()?;
                self.expect_op(")")?;
                Ok(Expr::Paren(Box::new(e)))
            }
            Tok::Op("*") => Ok(Expr::Star(vec![])),
            Tok::Word(w)
                if self.peek().is_some_and(|t| t.is_op("("))
                    && (!is_keyword(&w)
                        || w.eq_ignore_ascii_case("Left")
                        || w.eq_ignore_ascii_case("Right")) =>
            {
                self.pos += 1;
                if w.eq_ignore_ascii_case("Exists") {
                    let s = self.select()?;
                    self.expect_op(")")?;
                    return Ok(Expr::Exists(Box::new(s)));
                }
                let mut args = vec![];
                let distinct = self.eat_word("DISTINCT");
                if !self.eat_op(")") {
                    loop {
                        args.push(self.expr()?);
                        if self.eat_op(")") {
                            break;
                        }
                        self.expect_op(",")?;
                    }
                }
                Ok(Expr::Call {
                    name: w,
                    args,
                    distinct,
                })
            }
            Tok::Word(w) if w.eq_ignore_ascii_case("Null") => Ok(Expr::Null),
            Tok::Word(w)
                if ["True", "Yes", "On"]
                    .iter()
                    .any(|k| w.eq_ignore_ascii_case(k)) =>
            {
                Ok(Expr::Bool(true))
            }
            Tok::Word(w)
                if ["False", "No", "Off"]
                    .iter()
                    .any(|k| w.eq_ignore_ascii_case(k)) =>
            {
                Ok(Expr::Bool(false))
            }
            Tok::Word(w) if is_keyword(&w) => Err(format!("unexpected keyword {w}")),
            Tok::Word(w) | Tok::Bracket(w) => self.name_rest(w),
            other => Err(format!("unexpected {other:?}")),
        }
    }

    /// Continues `a.b!c` after its first part; `T.*` becomes a star.
    fn name_rest(&mut self, first: String) -> Result<Expr, String> {
        let mut parts = vec![Part {
            text: first,
            bang: false,
        }];
        loop {
            let bang = match self.peek() {
                Some(Tok::Op(".")) => false,
                Some(Tok::Op("!")) => true,
                _ => break,
            };
            match self.peek_at(1).cloned() {
                Some(Tok::Word(w)) | Some(Tok::Bracket(w)) => {
                    self.pos += 2;
                    parts.push(Part { text: w, bang });
                }
                Some(Tok::Op("*")) => {
                    self.pos += 2;
                    return Ok(Expr::Star(parts.into_iter().map(|p| p.text).collect()));
                }
                _ => break,
            }
        }
        Ok(Expr::Name(parts))
    }

    /// `SELECT ...` up to the end of its clauses (and any UNION parts).
    pub fn select(&mut self) -> Result<Select, String> {
        let paren = self.eat_op("(");
        self.expect_word("SELECT")?;
        let mut s = Select::default();
        loop {
            if self.eat_word("DISTINCT") {
                s.distinct = true;
            } else if self.eat_word("DISTINCTROW") {
                s.distinct_row = true;
            } else if self.eat_word("ALL") {
            } else if self.eat_word("TOP") {
                let n = match self.next() {
                    Some(Tok::Num(n)) => n,
                    other => return Err(format!("TOP needs a number, found {other:?}")),
                };
                s.top = Some((n, self.eat_word("PERCENT")));
            } else {
                break;
            }
        }
        loop {
            let e = self.expr()?;
            let alias = if self.eat_word("AS") {
                Some(self.alias()?)
            } else {
                None
            };
            s.columns.push((e, alias));
            if !self.eat_op(",") {
                break;
            }
        }
        if self.eat_word("INTO") {
            s.into = Some(self.alias()?);
        }
        if self.eat_word("FROM") {
            loop {
                s.from.push(self.from_item()?);
                if !self.eat_op(",") {
                    break;
                }
            }
        }
        if self.eat_word("WHERE") {
            s.where_clause = Some(self.expr()?);
        }
        if self.eat_word("GROUP") {
            self.expect_word("BY")?;
            loop {
                s.group_by.push(self.expr()?);
                if !self.eat_op(",") {
                    break;
                }
            }
        }
        if self.eat_word("HAVING") {
            s.having = Some(self.expr()?);
        }
        if paren {
            self.expect_op(")")?;
        }
        while self.eat_word("UNION") {
            let all = self.eat_word("ALL");
            let mut part = self.select()?;
            // ORDER BY of the last part applies to the whole union.
            if !part.order_by.is_empty() && s.order_by.is_empty() {
                s.order_by = std::mem::take(&mut part.order_by);
            }
            let nested = std::mem::take(&mut part.unions);
            s.unions.push((all, part));
            s.unions.extend(nested);
        }
        if self.eat_word("ORDER") {
            self.expect_word("BY")?;
            loop {
                let e = self.expr()?;
                let desc = self.eat_word("DESC");
                if !desc {
                    self.eat_word("ASC");
                }
                s.order_by.push((e, desc));
                if !self.eat_op(",") {
                    break;
                }
            }
        }
        if self.eat_word("WITH") {
            // WITH OWNERACCESS OPTION
            self.eat_word("OWNERACCESS");
            self.eat_word("OPTION");
        }
        Ok(s)
    }

    fn alias(&mut self) -> Result<String, String> {
        match self.next() {
            Some(Tok::Word(w)) | Some(Tok::Bracket(w)) | Some(Tok::Str(w)) => Ok(w),
            other => Err(format!("expected a name, found {other:?}")),
        }
    }

    fn from_item(&mut self) -> Result<From, String> {
        let mut left = self.from_primary()?;
        loop {
            let kind = if self.eat_word("INNER") {
                JoinKind::Inner
            } else if self.eat_word("LEFT") {
                self.eat_word("OUTER");
                JoinKind::Left
            } else if self.eat_word("RIGHT") {
                self.eat_word("OUTER");
                JoinKind::Right
            } else {
                break;
            };
            self.expect_word("JOIN")?;
            let right = self.from_primary()?;
            self.expect_word("ON")?;
            let on = self.expr()?;
            left = From::Join {
                kind,
                left: Box::new(left),
                right: Box::new(right),
                on,
            };
        }
        Ok(left)
    }

    fn from_primary(&mut self) -> Result<From, String> {
        if self.peek().is_some_and(|t| t.is_op("(")) {
            if self.peek_at(1).is_some_and(|t| t.is_word("SELECT")) {
                self.pos += 1;
                let select = self.select()?;
                self.expect_op(")")?;
                let alias = self.table_alias()?;
                return Ok(From::Sub {
                    select: Box::new(select),
                    alias,
                });
            }
            self.pos += 1;
            let inner = self.from_item()?;
            self.expect_op(")")?;
            return Ok(inner);
        }
        let mut name = self.alias()?;
        // Qualified names (`[;DATABASE=...].Table`, `Owner.Table`) keep the last part.
        while self.peek().is_some_and(|t| t.is_op(".")) {
            self.pos += 1;
            name = self.alias()?;
        }
        if self.eat_word("IN") {
            // External database: `IN 'path'`.
            self.next();
        }
        let alias = self.table_alias()?;
        Ok(From::Table { name, alias })
    }

    fn table_alias(&mut self) -> Result<Option<String>, String> {
        if self.eat_word("AS") {
            return Ok(Some(self.alias()?));
        }
        match self.peek() {
            Some(Tok::Word(w)) if !is_keyword(w) => {
                let w = w.clone();
                self.pos += 1;
                Ok(Some(w))
            }
            Some(Tok::Bracket(w)) => {
                let w = w.clone();
                self.pos += 1;
                Ok(Some(w))
            }
            _ => Ok(None),
        }
    }

    /// A whole query: optional PARAMETERS, then SELECT or TRANSFORM.
    pub fn statement(mut self) -> Result<Statement, String> {
        if self.eat_word("PARAMETERS") {
            while self.pos < self.toks.len() && !self.eat_op(";") {
                self.pos += 1;
            }
        }
        if self.eat_word("TRANSFORM") {
            let e = self.expr()?;
            let alias = if self.eat_word("AS") {
                Some(self.alias()?)
            } else {
                None
            };
            let select = self.select()?;
            self.expect_word("PIVOT")?;
            let pivot = self.expr()?;
            let mut pivot_in = vec![];
            if self.eat_word("IN") {
                self.expect_op("(")?;
                loop {
                    pivot_in.push(self.expr()?);
                    if !self.eat_op(",") {
                        break;
                    }
                }
                self.expect_op(")")?;
            }
            return Ok(Statement::Crosstab(Crosstab {
                transform: (e, alias),
                select,
                pivot,
                pivot_in,
            }));
        }
        let statement = if self.eat_word("INSERT") {
            Statement::Insert(self.insert()?)
        } else if self.eat_word("UPDATE") {
            Statement::Update(self.update()?)
        } else if self.eat_word("DELETE") {
            Statement::Delete(self.delete()?)
        } else {
            Statement::Select(self.select()?)
        };
        if !self.at_end() {
            return Err(format!("unexpected {:?} after the query", self.peek()));
        }
        Ok(statement)
    }

    /// A column of an INSERT list or SET target: the last part of a name.
    fn target_parts(&mut self) -> Result<Vec<Part>, String> {
        match self.expr_primary_name()? {
            Expr::Name(parts) => Ok(parts),
            other => Err(format!("expected a field name, found {other:?}")),
        }
    }

    fn expr_primary_name(&mut self) -> Result<Expr, String> {
        let mut parts = vec![Part {
            text: self.alias()?,
            bang: false,
        }];
        while self.peek().is_some_and(|t| t.is_op(".") || t.is_op("!")) {
            let bang = self.peek().is_some_and(|t| t.is_op("!"));
            self.pos += 1;
            parts.push(Part {
                text: self.alias()?,
                bang,
            });
        }
        Ok(Expr::Name(parts))
    }

    fn insert(&mut self) -> Result<Insert, String> {
        self.expect_word("INTO")?;
        let mut table = self.alias()?;
        while self.eat_op(".") {
            table = self.alias()?;
        }
        if self.eat_word("IN") {
            return Err("appending to a table in another database is not supported".into());
        }
        let mut columns = vec![];
        if self.peek().is_some_and(|t| t.is_op("("))
            && !self.peek_at(1).is_some_and(|t| t.is_word("SELECT"))
        {
            self.pos += 1;
            loop {
                let parts = self.target_parts()?;
                columns.push(parts.last().map(|p| p.text.clone()).unwrap_or_default());
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(")")?;
        }
        let source = if self.eat_word("VALUES") {
            self.expect_op("(")?;
            let mut values = vec![];
            loop {
                values.push(self.expr()?);
                if !self.eat_op(",") {
                    break;
                }
            }
            self.expect_op(")")?;
            InsertSource::Values(values)
        } else {
            InsertSource::Select(self.select()?)
        };
        Ok(Insert {
            table,
            columns,
            source,
        })
    }

    fn update(&mut self) -> Result<Update, String> {
        self.eat_word("DISTINCTROW");
        let from = self.from_item()?;
        self.expect_word("SET")?;
        let mut sets = vec![];
        loop {
            let target = self.target_parts()?;
            self.expect_op("=")?;
            sets.push((target, self.expr()?));
            if !self.eat_op(",") {
                break;
            }
        }
        let where_clause = if self.eat_word("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Update {
            from,
            sets,
            where_clause,
        })
    }

    fn delete(&mut self) -> Result<Delete, String> {
        self.eat_word("DISTINCTROW");
        // `DELETE *`, `DELETE T.*` or a field list: only the table matters.
        let mut target = None;
        while !self.at_end() && !self.peek().is_some_and(|t| t.is_word("FROM")) {
            match self.next() {
                Some(Tok::Word(w)) | Some(Tok::Bracket(w)) if target.is_none() => target = Some(w),
                _ => {}
            }
        }
        self.expect_word("FROM")?;
        let mut from = vec![];
        loop {
            from.push(self.from_item()?);
            if !self.eat_op(",") {
                break;
            }
        }
        let where_clause = if self.eat_word("WHERE") {
            Some(self.expr()?)
        } else {
            None
        };
        Ok(Delete {
            target,
            from,
            where_clause,
        })
    }
}

pub fn is_keyword(w: &str) -> bool {
    let reserved = [
        "SELECT",
        "AND",
        "OR",
        "NOT",
        "LIKE",
        "BETWEEN",
        "IS",
        "AS",
        "BY",
        "JOIN",
        "DISTINCT",
        "DISTINCTROW",
        "TOP",
        "SET",
        "VALUES",
        "MOD",
        "XOR",
        "EQV",
        "IMP",
    ];
    CLAUSE_WORDS
        .iter()
        .chain(reserved.iter())
        .any(|k| w.eq_ignore_ascii_case(k))
}

pub fn parse_expression(text: &str) -> Result<Expr, String> {
    Parser::new(text)?.whole_expression()
}

pub fn parse_statement(text: &str) -> Result<Statement, String> {
    Parser::new(text)?.statement()
}

/// A field validation rule may start with its operator (`>=0`, `Like "*@*"`,
/// `Is Not Null`, `Between 1 And 5`); the field itself is the left operand.
pub fn parse_field_rule(rule: &str, field: &str) -> Result<Expr, String> {
    let mut p = Parser::new(rule)?;
    let starts_with_operator = match p.peek() {
        Some(Tok::Op(o)) => ["=", "<>", "<", "<=", ">", ">="].contains(o),
        Some(Tok::Word(w)) => {
            ["Like", "Between", "In", "Is", "ALike"]
                .iter()
                .any(|k| w.eq_ignore_ascii_case(k))
                || (w.eq_ignore_ascii_case("Not")
                    && p.peek_at(1).is_some_and(|t| {
                        t.is_word("Like") || t.is_word("Between") || t.is_word("In")
                    }))
        }
        _ => false,
    };
    if !starts_with_operator {
        return p.whole_expression();
    }
    let field = Expr::Name(vec![Part {
        text: field.to_string(),
        bang: false,
    }]);
    // Combine `>=0 And <=100` style rules: each operand of And/Or may start with an operator.
    let first = p.comparison_tail(field.clone())?;
    let mut e = first;
    loop {
        let op = if p.eat_word("And") {
            BinOp::And
        } else if p.eat_word("Or") {
            BinOp::Or
        } else {
            break;
        };
        let starts_op = matches!(p.peek(), Some(Tok::Op(o)) if ["=", "<>", "<", "<=", ">", ">="].contains(o))
            || p.peek().is_some_and(|t| {
                t.is_word("Like") || t.is_word("Is") || t.is_word("Between") || t.is_word("In")
            });
        let right = if starts_op {
            p.comparison_tail(field.clone())?
        } else {
            p.not()?
        };
        e = Expr::Bin(op, Box::new(e), Box::new(right));
    }
    if !p.at_end() {
        return Err(format!("unexpected {:?} in the validation rule", p.peek()));
    }
    Ok(e)
}
