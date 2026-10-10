//! A small XML reader for the template package parts (XSD schemas, sample data,
//! relationships, metadata). These files use no DTDs or external entities, so a
//! DOM with namespace prefixes kept in the names is all the importer needs.

/// An element with its attributes and children.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<XNode>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum XNode {
    Element(Element),
    Text(String),
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|c| match c {
            XNode::Element(e) => Some(e),
            XNode::Text(_) => None,
        })
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.elements().filter(move |e| e.name == name)
    }

    /// Concatenated text of this element and its descendants.
    pub fn text(&self) -> String {
        let mut out = String::new();
        collect_text(self, &mut out);
        out
    }

    /// Depth-first search for descendants with this name.
    pub fn descendants<'a>(&'a self, name: &'a str, out: &mut Vec<&'a Element>) {
        for e in self.elements() {
            if e.name == name {
                out.push(e);
            }
            e.descendants(name, out);
        }
    }
}

fn collect_text(e: &Element, out: &mut String) {
    for c in &e.children {
        match c {
            XNode::Text(t) => out.push_str(t),
            XNode::Element(e) => collect_text(e, out),
        }
    }
}

/// Decodes an XML part: UTF-16 with a BOM or UTF-8.
pub fn decode(bytes: &[u8]) -> String {
    crate::access::text_format::decode(bytes)
}

/// Parses a document and returns its root element.
pub fn parse(text: &str) -> Result<Element, String> {
    let mut p = Parser {
        s: text.as_bytes(),
        text,
        pos: 0,
    };
    p.skip_prolog()?;
    let root = p.element()?;
    Ok(root)
}

struct Parser<'a> {
    s: &'a [u8],
    text: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn err(&self, m: &str) -> String {
        format!("XML error at byte {}: {m}", self.pos)
    }

    fn starts(&self, p: &str) -> bool {
        self.s[self.pos..].starts_with(p.as_bytes())
    }

    fn skip_until(&mut self, end: &str) -> Result<(), String> {
        match self.text[self.pos..].find(end) {
            Some(i) => {
                self.pos += i + end.len();
                Ok(())
            }
            None => Err(self.err(&format!("missing {end}"))),
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.s.len() && self.s[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    /// Skips the XML declaration, comments, processing instructions and doctype.
    fn skip_prolog(&mut self) -> Result<(), String> {
        loop {
            self.skip_ws();
            if self.starts("\u{feff}") {
                self.pos += 3;
            } else if self.starts("<?") {
                self.skip_until("?>")?;
            } else if self.starts("<!--") {
                self.skip_until("-->")?;
            } else if self.starts("<!") {
                self.skip_until(">")?;
            } else {
                return Ok(());
            }
        }
    }

    fn name(&mut self) -> Result<String, String> {
        let start = self.pos;
        while self.pos < self.s.len() {
            let c = self.s[self.pos];
            if c.is_ascii_whitespace() || c == b'>' || c == b'/' || c == b'=' {
                break;
            }
            self.pos += 1;
        }
        if start == self.pos {
            return Err(self.err("expected a name"));
        }
        Ok(self.text[start..self.pos].to_string())
    }

    fn element(&mut self) -> Result<Element, String> {
        if !self.starts("<") {
            return Err(self.err("expected <"));
        }
        self.pos += 1;
        let mut el = Element {
            name: self.name()?,
            ..Default::default()
        };
        loop {
            self.skip_ws();
            if self.starts("/>") {
                self.pos += 2;
                return Ok(el);
            }
            if self.starts(">") {
                self.pos += 1;
                break;
            }
            let key = self.name()?;
            self.skip_ws();
            if !self.starts("=") {
                return Err(self.err("expected ="));
            }
            self.pos += 1;
            self.skip_ws();
            let quote = *self.s.get(self.pos).ok_or_else(|| self.err("eof"))?;
            if quote != b'"' && quote != b'\'' {
                return Err(self.err("expected quote"));
            }
            self.pos += 1;
            let start = self.pos;
            while self.pos < self.s.len() && self.s[self.pos] != quote {
                self.pos += 1;
            }
            let raw = &self.text[start..self.pos];
            self.pos += 1;
            el.attrs.push((key, unescape(raw)));
        }
        self.content(&mut el)?;
        Ok(el)
    }

    fn content(&mut self, el: &mut Element) -> Result<(), String> {
        loop {
            if self.pos >= self.s.len() {
                return Err(self.err(&format!("missing </{}>", el.name)));
            }
            if self.starts("</") {
                self.skip_until(">")?;
                return Ok(());
            }
            if self.starts("<!--") {
                self.skip_until("-->")?;
            } else if self.starts("<![CDATA[") {
                let start = self.pos + 9;
                self.skip_until("]]>")?;
                push_text(el, &self.text[start..self.pos - 3]);
            } else if self.starts("<?") {
                self.skip_until("?>")?;
            } else if self.starts("<") {
                let child = self.element()?;
                el.children.push(XNode::Element(child));
            } else {
                let start = self.pos;
                while self.pos < self.s.len() && self.s[self.pos] != b'<' {
                    self.pos += 1;
                }
                push_text(el, &unescape(&self.text[start..self.pos]));
            }
        }
    }
}

fn push_text(el: &mut Element, t: &str) {
    if let Some(XNode::Text(prev)) = el.children.last_mut() {
        prev.push_str(t);
    } else {
        el.children.push(XNode::Text(t.to_string()));
    }
}

/// Resolves the predefined entities and character references.
pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if entity.starts_with("#x") => u32::from_str_radix(&entity[2..], 16)
                .ok()
                .and_then(char::from_u32),
            _ if entity.starts_with('#') => entity[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Decodes XML names Access escaped as `_xHHHH_` (e.g. `Retired_x0020_Date`).
pub fn unescape_name(s: &str) -> String {
    if !s.contains("_x") {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < s.len() {
        if b[i] == b'_' && i + 7 <= s.len() && b[i + 1] == b'x' && b[i + 6] == b'_' {
            if let Some(c) = u32::from_str_radix(&s[i + 2..i + 6], 16)
                .ok()
                .and_then(char::from_u32)
            {
                out.push(c);
                i += 7;
                continue;
            }
        }
        let c = s[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(c);
        i += c.len_utf8();
    }
    out
}
