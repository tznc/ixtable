//! Tokens of Access SQL and Access expressions (Jet SQL, ANSI-89 mode).

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    /// A bare word: identifier or keyword.
    Word(String),
    /// `[Name with spaces]`.
    Bracket(String),
    /// `"text"` or `'text'` with doubled quotes resolved.
    Str(String),
    Num(String),
    /// `#1/31/2026#` (raw inside the hashes).
    Date(String),
    /// Operator or punctuation: `= <> < <= > >= + - * / \ ^ & ( ) , . ! ;`
    Op(&'static str),
}

impl Tok {
    pub fn is_word(&self, w: &str) -> bool {
        matches!(self, Tok::Word(x) if x.eq_ignore_ascii_case(w))
    }

    pub fn is_op(&self, o: &str) -> bool {
        matches!(self, Tok::Op(x) if *x == o)
    }
}

const OPS: [&str; 19] = [
    "<>", "<=", ">=", "=", "<", ">", "+", "-", "*", "/", "\\", "^", "&", "(", ")", ",", ".", "!",
    ";",
];

pub fn lex(s: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut out = vec![];
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '[' {
            let start = i + 1;
            let mut j = start;
            while j < chars.len() && chars[j] != ']' {
                j += 1;
            }
            if j >= chars.len() {
                return Err("unclosed [ in expression".into());
            }
            out.push(Tok::Bracket(chars[start..j].iter().collect()));
            i = j + 1;
            continue;
        }
        if c == '"' || c == '\'' {
            let mut text = String::new();
            let mut j = i + 1;
            loop {
                if j >= chars.len() {
                    return Err("unclosed text in expression".into());
                }
                if chars[j] == c {
                    if chars.get(j + 1) == Some(&c) {
                        text.push(c);
                        j += 2;
                        continue;
                    }
                    break;
                }
                text.push(chars[j]);
                j += 1;
            }
            out.push(Tok::Str(text));
            i = j + 1;
            continue;
        }
        if c == '#' {
            // A date literal, unless it is a `Like` digit wildcard (only in strings).
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '#' {
                j += 1;
            }
            if j >= chars.len() {
                return Err("unclosed # date".into());
            }
            out.push(Tok::Date(chars[i + 1..j].iter().collect()));
            i = j + 1;
            continue;
        }
        if c.is_ascii_digit()
            || (c == '.'
                && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit())
                && !prev_is_name(&out))
        {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            if j < chars.len()
                && (chars[j] == 'e' || chars[j] == 'E')
                && chars
                    .get(j + 1)
                    .is_some_and(|d| d.is_ascii_digit() || *d == '-' || *d == '+')
            {
                j += 2;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
            }
            out.push(Tok::Num(chars[i..j].iter().collect()));
            i = j;
            continue;
        }
        if c.is_alphabetic() || c == '_' || c == '@' || c == '$' {
            let mut j = i;
            while j < chars.len()
                && (chars[j].is_alphanumeric()
                    || chars[j] == '_'
                    || chars[j] == '$'
                    || chars[j] == '@')
            {
                j += 1;
            }
            out.push(Tok::Word(chars[i..j].iter().collect()));
            i = j;
            continue;
        }
        let rest: String = chars[i..chars.len().min(i + 2)].iter().collect();
        match OPS.iter().find(|o| rest.starts_with(**o)) {
            Some(op) => {
                out.push(Tok::Op(op));
                i += op.chars().count();
            }
            None => return Err(format!("unexpected character {c:?}")),
        }
    }
    Ok(out)
}

fn prev_is_name(out: &[Tok]) -> bool {
    matches!(
        out.last(),
        Some(Tok::Word(_) | Tok::Bracket(_)) | Some(Tok::Op(")"))
    )
}
