//! A small backtracking matcher for the JavaScript regular expression syntax
//! that `regexmatch` accepts (Unicode mode, no back-references, no flags),
//! plus the safety screen that rejects patterns which can run for minutes.
//! The matcher also has a step budget, so no accepted pattern can hang.
use super::values::is_js_space;
use std::cell::Cell;

pub const MAX_REGEX_LENGTH: usize = 200;
pub const MAX_REGEX_INPUT: usize = 10_000;
const MAX_STEPS: u64 = 20_000_000;
// Each repeated group iteration nests several stack frames, so this keeps
// the matcher well inside a 2 MB thread stack.
const MAX_REPEAT_DEPTH: u32 = 400;
const MAX_PARSE_DEPTH: u32 = 100;

#[derive(Debug)]
enum ClassItem {
    Range(char, char),
    Digit(bool),
    Word(bool),
    Space(bool),
    /// A Unicode property such as `\p{L}`; the flag negates it.
    Property(fn(char) -> bool, bool),
}

#[derive(Debug)]
struct Class {
    negated: bool,
    items: Vec<ClassItem>,
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

impl ClassItem {
    fn matches(&self, c: char) -> bool {
        match *self {
            ClassItem::Range(lo, hi) => lo <= c && c <= hi,
            ClassItem::Digit(neg) => c.is_ascii_digit() != neg,
            ClassItem::Word(neg) => is_word(c) != neg,
            ClassItem::Space(neg) => is_js_space(c) != neg,
            ClassItem::Property(test, neg) => test(c) != neg,
        }
    }
}

impl Class {
    fn matches(&self, c: char) -> bool {
        self.items.iter().any(|i| i.matches(c)) != self.negated
    }
}

#[derive(Debug)]
enum Node {
    Empty,
    Char(char),
    Any,
    Class(Class),
    Start,
    End,
    WordBoundary(bool),
    Seq(Vec<Node>),
    Alt(Vec<Node>),
    Repeat {
        node: Box<Node>,
        min: u32,
        max: Option<u32>,
        greedy: bool,
    },
    Look {
        node: Box<Node>,
        ahead: bool,
        negate: bool,
    },
}

#[derive(Debug)]
pub struct Regex {
    root: Node,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    depth: u32,
}

type Parsed<T> = Result<T, String>;

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let hit = self.peek() == Some(c);
        if hit {
            self.pos += 1;
        }
        hit
    }

    fn parse_alt(&mut self) -> Parsed<Node> {
        self.depth += 1;
        if self.depth > MAX_PARSE_DEPTH {
            return Err("pattern is nested too deeply".into());
        }
        let mut alts = vec![self.parse_seq()?];
        while self.eat('|') {
            alts.push(self.parse_seq()?);
        }
        self.depth -= 1;
        Ok(if alts.len() == 1 {
            alts.remove(0)
        } else {
            Node::Alt(alts)
        })
    }

    fn parse_seq(&mut self) -> Parsed<Node> {
        let mut nodes = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            nodes.push(self.parse_term()?);
        }
        Ok(match nodes.len() {
            0 => Node::Empty,
            1 => nodes.remove(0),
            _ => Node::Seq(nodes),
        })
    }

    fn parse_term(&mut self) -> Parsed<Node> {
        let atom = self.parse_atom()?;
        let Some((min, max)) = self.parse_quantifier()? else {
            return Ok(atom);
        };
        if matches!(
            atom,
            Node::Start | Node::End | Node::WordBoundary(_) | Node::Look { .. }
        ) {
            return Err("nothing to repeat".into());
        }
        let greedy = !self.eat('?');
        Ok(Node::Repeat {
            node: Box::new(atom),
            min,
            max,
            greedy,
        })
    }

    fn parse_quantifier(&mut self) -> Parsed<Option<(u32, Option<u32>)>> {
        let range = match self.peek() {
            Some('*') => (0, None),
            Some('+') => (1, None),
            Some('?') => (0, Some(1)),
            Some('{') => return self.parse_braces().map(Some),
            _ => return Ok(None),
        };
        self.pos += 1;
        Ok(Some(range))
    }

    fn number(&mut self) -> Option<u32> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit()) {
            self.pos += 1;
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        if text.is_empty() {
            None
        } else {
            Some(text.parse().unwrap_or(u32::MAX))
        }
    }

    fn parse_braces(&mut self) -> Parsed<(u32, Option<u32>)> {
        self.pos += 1;
        let min = self.number().ok_or("incomplete quantifier")?;
        let max = if self.eat(',') {
            if self.peek() == Some('}') {
                None
            } else {
                Some(self.number().ok_or("incomplete quantifier")?)
            }
        } else {
            Some(min)
        };
        if !self.eat('}') {
            return Err("incomplete quantifier".into());
        }
        if max.is_some_and(|m| m < min) {
            return Err("numbers out of order in {} quantifier".into());
        }
        Ok((min, max))
    }

    fn parse_atom(&mut self) -> Parsed<Node> {
        let Some(c) = self.peek() else {
            return Ok(Node::Empty);
        };
        self.pos += 1;
        match c {
            '(' => self.parse_group(),
            '[' => self.parse_class(),
            '.' => Ok(Node::Any),
            '^' => Ok(Node::Start),
            '$' => Ok(Node::End),
            '\\' => self.parse_escape(),
            '*' | '+' | '?' | '{' => Err("nothing to repeat".into()),
            '}' | ']' => Err("lone quantifier brackets".into()),
            ')' => Err("unmatched ')'".into()),
            other => Ok(Node::Char(other)),
        }
    }

    fn parse_group(&mut self) -> Parsed<Node> {
        let mut look = None;
        if self.eat('?') {
            look = match (self.peek(), self.peek_at(1)) {
                (Some(':'), _) => {
                    self.pos += 1;
                    None
                }
                (Some('='), _) => {
                    self.pos += 1;
                    Some((true, false))
                }
                (Some('!'), _) => {
                    self.pos += 1;
                    Some((true, true))
                }
                (Some('<'), Some('=')) => {
                    self.pos += 2;
                    Some((false, false))
                }
                (Some('<'), Some('!')) => {
                    self.pos += 2;
                    Some((false, true))
                }
                (Some('<'), _) => {
                    let rest = &self.chars[self.pos..];
                    let close = rest.iter().position(|c| *c == '>');
                    let Some(close) = close.filter(|n| *n > 1) else {
                        return Err("invalid capture group name".into());
                    };
                    self.pos += close + 1;
                    None
                }
                _ => return Err("invalid group".into()),
            };
        }
        let inner = self.parse_alt()?;
        if !self.eat(')') {
            return Err("unterminated group".into());
        }
        Ok(match look {
            Some((ahead, negate)) => Node::Look {
                node: Box::new(inner),
                ahead,
                negate,
            },
            None => inner,
        })
    }

    fn hex(&mut self, digits: usize) -> Option<u32> {
        let slice = self.chars.get(self.pos..self.pos + digits)?;
        let text: String = slice.iter().collect();
        let value = u32::from_str_radix(&text, 16)
            .ok()
            .filter(|_| text.chars().all(|c| c.is_ascii_hexdigit()))?;
        self.pos += digits;
        Some(value)
    }

    fn unicode_escape(&mut self) -> Parsed<char> {
        if self.eat('{') {
            let start = self.pos;
            while self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                self.pos += 1;
            }
            let text: String = self.chars[start..self.pos].iter().collect();
            if !self.eat('}') || text.is_empty() {
                return Err("invalid Unicode escape".into());
            }
            return u32::from_str_radix(&text, 16)
                .ok()
                .and_then(char::from_u32)
                .ok_or_else(|| "invalid Unicode escape".to_string());
        }
        let high = self.hex(4).ok_or("invalid Unicode escape")?;
        if (0xD800..0xDC00).contains(&high)
            && self.peek() == Some('\\')
            && self.peek_at(1) == Some('u')
        {
            let save = self.pos;
            self.pos += 2;
            if let Some(low) = self.hex(4).filter(|l| (0xDC00..0xE000).contains(l)) {
                let code = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                return char::from_u32(code).ok_or_else(|| "invalid Unicode escape".to_string());
            }
            self.pos = save;
        }
        char::from_u32(high).ok_or_else(|| "invalid Unicode escape".to_string())
    }

    /// Escapes valid both inside and outside a character class.
    fn common_escape(&mut self, c: char) -> Parsed<Option<char>> {
        Ok(Some(match c {
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            'f' => '\u{c}',
            'v' => '\u{b}',
            '0' if !self.peek().is_some_and(|n| n.is_ascii_digit()) => '\0',
            'x' => char::from_u32(self.hex(2).ok_or("invalid escape")?).ok_or("invalid escape")?,
            'u' => self.unicode_escape()?,
            'c' => {
                let letter = self.peek().filter(char::is_ascii_alphabetic);
                let letter = letter.ok_or("invalid unicode escape")?;
                self.pos += 1;
                char::from_u32(u32::from(letter) % 32).ok_or("invalid escape")?
            }
            '^' | '$' | '\\' | '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|'
            | '/' => c,
            _ => return Ok(None),
        }))
    }

    /// Reads `{Name}` after `\p` or `\P`. Covers the general categories
    /// that expressions use; others are rejected.
    fn property(&mut self, negated: bool) -> Parsed<ClassItem> {
        if !self.eat('{') {
            return Err("invalid property name".into());
        }
        let Some(close) = self.chars[self.pos..].iter().position(|c| *c == '}') else {
            return Err("invalid property name".into());
        };
        let name: String = self.chars[self.pos..self.pos + close].iter().collect();
        self.pos += close + 1;
        let test: fn(char) -> bool = match name.as_str() {
            "L" | "Letter" => char::is_alphabetic,
            "Lu" | "Uppercase_Letter" => char::is_uppercase,
            "Ll" | "Lowercase_Letter" => char::is_lowercase,
            "N" | "Number" | "Nd" | "Decimal_Number" => char::is_numeric,
            "Alphabetic" => char::is_alphabetic,
            "White_Space" => char::is_whitespace,
            "ASCII" => |c| c.is_ascii(),
            _ => return Err(format!("unsupported property name '{name}'")),
        };
        Ok(ClassItem::Property(test, negated))
    }

    fn class_escape_item(c: char) -> Option<ClassItem> {
        Some(match c {
            'd' => ClassItem::Digit(false),
            'D' => ClassItem::Digit(true),
            'w' => ClassItem::Word(false),
            'W' => ClassItem::Word(true),
            's' => ClassItem::Space(false),
            'S' => ClassItem::Space(true),
            _ => return None,
        })
    }

    fn parse_escape(&mut self) -> Parsed<Node> {
        let Some(c) = self.peek() else {
            return Err("\\ at end of pattern".into());
        };
        self.pos += 1;
        if let Some(item) = Self::class_escape_item(c) {
            return Ok(Node::Class(Class {
                negated: false,
                items: vec![item],
            }));
        }
        if c == 'p' || c == 'P' {
            let item = self.property(c == 'P')?;
            return Ok(Node::Class(Class {
                negated: false,
                items: vec![item],
            }));
        }
        match c {
            'b' => return Ok(Node::WordBoundary(false)),
            'B' => return Ok(Node::WordBoundary(true)),
            '1'..='9' | 'k' => return Err("back-references are not allowed".into()),
            _ => {}
        }
        match self.common_escape(c)? {
            Some(ch) => Ok(Node::Char(ch)),
            None => Err("invalid escape".into()),
        }
    }

    fn class_atom(&mut self) -> Parsed<Result<char, ClassItem>> {
        let Some(c) = self.peek() else {
            return Err("unterminated character class".into());
        };
        self.pos += 1;
        if c != '\\' {
            return Ok(Ok(c));
        }
        let Some(e) = self.peek() else {
            return Err("\\ at end of pattern".into());
        };
        self.pos += 1;
        if let Some(item) = Self::class_escape_item(e) {
            return Ok(Err(item));
        }
        if e == 'p' || e == 'P' {
            return Ok(Err(self.property(e == 'P')?));
        }
        match e {
            'b' => Ok(Ok('\u{8}')),
            '-' => Ok(Ok('-')),
            _ => match self.common_escape(e)? {
                Some(ch) => Ok(Ok(ch)),
                None => Err("invalid escape".into()),
            },
        }
    }

    fn parse_class(&mut self) -> Parsed<Node> {
        let negated = self.eat('^');
        let mut items = Vec::new();
        loop {
            if self.peek().is_none() {
                return Err("unterminated character class".into());
            }
            if self.eat(']') {
                break;
            }
            let first = self.class_atom()?;
            if self.peek() == Some('-') && self.peek_at(1).is_some_and(|c| c != ']') {
                self.pos += 1;
                let second = self.class_atom()?;
                let (Ok(lo), Ok(hi)) = (&first, &second) else {
                    return Err("invalid character class".into());
                };
                if lo > hi {
                    return Err("range out of order in character class".into());
                }
                items.push(ClassItem::Range(*lo, *hi));
                continue;
            }
            items.push(match first {
                Ok(c) => ClassItem::Range(c, c),
                Err(item) => item,
            });
        }
        Ok(Node::Class(Class { negated, items }))
    }
}

struct Matcher<'a> {
    input: &'a [char],
    steps: Cell<u64>,
    depth: Cell<u32>,
    aborted: Cell<bool>,
}

type Cont<'k> = &'k mut dyn FnMut(usize) -> bool;

impl Matcher<'_> {
    fn tick(&self) -> bool {
        let steps = self.steps.get() + 1;
        self.steps.set(steps);
        if steps > MAX_STEPS {
            self.aborted.set(true);
        }
        !self.aborted.get()
    }

    fn single(&self, node: &Node, c: char) -> Option<bool> {
        match node {
            Node::Char(x) => Some(*x == c),
            Node::Any => Some(!is_line_terminator(c)),
            Node::Class(class) => Some(class.matches(c)),
            _ => None,
        }
    }

    fn run(&self, node: &Node, i: usize, k: Cont) -> bool {
        if !self.tick() {
            return false;
        }
        let len = self.input.len();
        match node {
            Node::Empty => k(i),
            Node::Start => i == 0 && k(i),
            Node::End => i == len && k(i),
            Node::WordBoundary(negated) => {
                let before = i > 0 && is_word(self.input[i - 1]);
                let after = i < len && is_word(self.input[i]);
                ((before != after) != *negated) && k(i)
            }
            Node::Char(_) | Node::Any | Node::Class(_) => {
                i < len && self.single(node, self.input[i]) == Some(true) && k(i + 1)
            }
            Node::Seq(nodes) => self.seq(nodes, i, k),
            Node::Alt(alts) => {
                for alt in alts {
                    if self.run(alt, i, k) {
                        return true;
                    }
                }
                false
            }
            Node::Repeat {
                node,
                min,
                max,
                greedy,
            } => {
                if self.single(node, ' ').is_some() {
                    self.repeat_single(node, *min, *max, *greedy, i, k)
                } else {
                    self.repeat(node, (*min, *max, *greedy), 0, i, k)
                }
            }
            Node::Look {
                node,
                ahead,
                negate,
            } => {
                let found = if *ahead {
                    self.run(node, i, &mut |_| true)
                } else {
                    (0..=i)
                        .rev()
                        .any(|start| self.run(node, start, &mut |end| end == i))
                };
                found != *negate && k(i)
            }
        }
    }

    fn seq(&self, nodes: &[Node], i: usize, k: Cont) -> bool {
        match nodes.split_first() {
            None => k(i),
            Some((first, rest)) => self.run(first, i, &mut |j| self.seq(rest, j, k)),
        }
    }

    fn repeat_single(
        &self,
        node: &Node,
        min: u32,
        max: Option<u32>,
        greedy: bool,
        i: usize,
        k: Cont,
    ) -> bool {
        let max = max.map_or(usize::MAX, |m| m as usize);
        let min = min as usize;
        let fits = |n: usize| {
            self.input
                .get(i + n)
                .is_some_and(|c| self.single(node, *c) == Some(true))
        };
        if greedy {
            let mut n = 0;
            while n < max && fits(n) {
                n += 1;
                if !self.tick() {
                    return false;
                }
            }
            (min..=n).rev().any(|count| self.tick() && k(i + count))
        } else {
            let mut n = 0;
            while n < min {
                if !fits(n) {
                    return false;
                }
                n += 1;
            }
            loop {
                if !self.tick() {
                    return false;
                }
                if k(i + n) {
                    return true;
                }
                if n < max && fits(n) {
                    n += 1;
                } else {
                    return false;
                }
            }
        }
    }

    fn repeat(
        &self,
        node: &Node,
        (min, max, greedy): (u32, Option<u32>, bool),
        count: u32,
        i: usize,
        k: Cont,
    ) -> bool {
        let depth = self.depth.get() + 1;
        if depth > MAX_REPEAT_DEPTH {
            self.aborted.set(true);
            return false;
        }
        self.depth.set(depth);
        let more = max.is_none_or(|m| count < m);
        let result = if count < min {
            self.more(node, (min, max, greedy), count, i, k)
        } else if greedy {
            (more && self.more(node, (min, max, greedy), count, i, k)) || k(i)
        } else {
            k(i) || (more && self.more(node, (min, max, greedy), count, i, k))
        };
        self.depth.set(depth - 1);
        result
    }

    fn more(
        &self,
        node: &Node,
        (min, max, greedy): (u32, Option<u32>, bool),
        count: u32,
        i: usize,
        k: Cont,
    ) -> bool {
        self.run(node, i, &mut |j| {
            // an empty iteration past the minimum can never help
            if j == i && count >= min {
                return false;
            }
            self.repeat(node, (min, max, greedy), count + 1, j, k)
        })
    }
}

impl Regex {
    fn parse(pattern: &str) -> Parsed<Self> {
        let mut parser = Parser {
            chars: pattern.chars().collect(),
            pos: 0,
            depth: 0,
        };
        let root = parser.parse_alt()?;
        match parser.peek() {
            None => Ok(Self { root }),
            Some(_) => Err("unmatched ')'".into()),
        }
    }

    /// Reports whether the pattern matches anywhere in `text`.
    pub fn is_match(&self, text: &str) -> Result<bool, String> {
        let input: Vec<char> = text.chars().collect();
        let matcher = Matcher {
            input: &input,
            steps: Cell::new(0),
            depth: Cell::new(0),
            aborted: Cell::new(false),
        };
        for start in 0..=input.len() {
            if matcher.run(&self.root, start, &mut |_| true) {
                return Ok(true);
            }
            if matcher.aborted.get() {
                return Err("pattern is too complex for this text".into());
            }
        }
        Ok(false)
    }
}

/// Compiles a user-supplied regular expression, rejecting patterns that can
/// backtrack catastrophically: back-references and a quantified group that
/// already contains a quantifier, such as `(a+)+`.
pub fn compile_safe_regex(pattern: &str) -> Result<Regex, String> {
    let chars: Vec<char> = pattern.chars().collect();
    let length: usize = chars.iter().map(|c| c.len_utf16()).sum();
    if length > MAX_REGEX_LENGTH {
        return Err(format!(
            "pattern is longer than {MAX_REGEX_LENGTH} characters"
        ));
    }
    let has_backref = chars
        .windows(2)
        .any(|w| w[0] == '\\' && matches!(w[1], '1'..='9'))
        || pattern.contains("\\k<");
    if has_backref {
        return Err("back-references are not allowed".into());
    }
    screen_nested_repetition(&chars)?;
    Regex::parse(pattern).map_err(|e| format!("invalid pattern: {e}"))
}

fn screen_nested_repetition(p: &[char]) -> Result<(), String> {
    let at = |i: usize| p.get(i).copied().unwrap_or('\0');
    let quantifier = |c: char| matches!(c, '+' | '*' | '?' | '{');
    let mut stack: Vec<bool> = Vec::new();
    let mut in_class = false;
    let mut i = 0;
    while i < p.len() {
        let ch = p[i];
        if ch == '\\' {
            i += 2;
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            i += 1;
            continue;
        }
        if ch == '[' {
            in_class = true;
        } else if ch == '(' {
            stack.push(false);
            if at(i + 1) == '?' {
                i += 1;
                if at(i + 1) == '<' && !matches!(at(i + 2), '=' | '!') {
                    match p[i..].iter().position(|c| *c == '>') {
                        Some(offset) => i += offset,
                        None => break,
                    }
                } else if matches!(at(i + 1), ':' | '=' | '!' | '<') {
                    i += if at(i + 1) == '<' { 2 } else { 1 };
                }
            }
        } else if ch == ')' {
            let inner = stack.pop().unwrap_or(false);
            let next = at(i + 1);
            if inner && matches!(next, '+' | '*' | '{') {
                return Err("nested repetition like (a+)+ is not allowed".into());
            }
            if let Some(top) = stack.last_mut() {
                if inner || quantifier(next) {
                    *top = true;
                }
            }
        } else if quantifier(ch) {
            if let Some(top) = stack.last_mut() {
                *top = true;
            }
        }
        i += 1;
    }
    Ok(())
}
