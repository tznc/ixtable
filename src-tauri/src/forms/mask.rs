//! Access-style input masks (port of `src/fields/mask.ts`). A mask has up to three
//! `;`-separated sections: the pattern, `0` to store literal characters with the
//! value (blank or `1` stores only what was typed), and the placeholder character
//! shown for empty slots (`_`).
//!
//! Pattern characters: `0` digit, `9` optional digit, `#` optional digit, space, + or -,
//! `L` letter, `?` optional letter, `A` letter or digit, `a` optional letter or digit,
//! `&` any character, `C` optional any character, `>` upper-cases and `<` lower-cases
//! what follows, `!` is ignored, `\x` and `"text"` are literals, and anything else is
//! a literal too.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Casing {
    Upper,
    Lower,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accepts {
    Digit,
    DigitSpaceSign,
    Letter,
    LetterOrDigit,
    Any,
}

impl Accepts {
    fn test(self, c: char) -> bool {
        match self {
            Accepts::Digit => c.is_ascii_digit(),
            Accepts::DigitSpaceSign => c.is_ascii_digit() || matches!(c, ' ' | '+' | '-'),
            Accepts::Letter => c.is_alphabetic(),
            Accepts::LetterOrDigit => c.is_alphabetic() || c.is_ascii_digit(),
            Accepts::Any => true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Slot {
    Literal(char),
    Input {
        accepts: Accepts,
        required: bool,
        casing: Option<Casing>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedMask {
    pub slots: Vec<Slot>,
    pub store_literals: bool,
    pub placeholder: char,
}

fn slot_for(c: char) -> Option<(Accepts, bool)> {
    Some(match c {
        '0' => (Accepts::Digit, true),
        '9' => (Accepts::Digit, false),
        '#' => (Accepts::DigitSpaceSign, false),
        'L' => (Accepts::Letter, true),
        '?' => (Accepts::Letter, false),
        'A' => (Accepts::LetterOrDigit, true),
        'a' => (Accepts::LetterOrDigit, false),
        '&' => (Accepts::Any, true),
        'C' => (Accepts::Any, false),
        _ => return None,
    })
}

/// Splits on `;` outside quotes and escapes.
fn sections(mask: &str) -> Vec<String> {
    let chars: Vec<char> = mask.chars().collect();
    let mut out = vec![String::new()];
    let mut quoted = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let last = out.last_mut().expect("section");
            last.push(c);
            i += 1;
            last.push(chars[i]);
            i += 1;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        if c == ';' && !quoted {
            out.push(String::new());
        } else {
            out.last_mut().expect("section").push(c);
        }
        i += 1;
    }
    out
}

pub fn parse_mask(mask: &str) -> ParsedMask {
    let parts = sections(mask);
    let pattern = parts.first().cloned().unwrap_or_default();
    let store = parts.get(1).cloned().unwrap_or_default();
    let placeholder = parts.get(2).cloned().unwrap_or_default();
    let mut slots = Vec::new();
    let mut casing = None;
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            i += 1;
            slots.push(Slot::Literal(chars[i]));
        } else if c == '"' {
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                slots.push(Slot::Literal(chars[i]));
                i += 1;
            }
        } else if c == '>' {
            casing = Some(Casing::Upper);
        } else if c == '<' {
            casing = Some(Casing::Lower);
        } else if c == '!' {
            // Right-to-left fill is not supported; the flag is ignored.
        } else if let Some((accepts, required)) = slot_for(c) {
            slots.push(Slot::Input {
                accepts,
                required,
                casing,
            });
        } else {
            slots.push(Slot::Literal(c));
        }
        i += 1;
    }
    ParsedMask {
        slots,
        store_literals: store.trim() == "0",
        placeholder: placeholder.chars().next().unwrap_or('_'),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaskResult {
    /// What the input shows: typed characters with the literals between them.
    pub display: String,
    /// What is stored: `display`, or only the typed characters when literals are not stored.
    pub stored: String,
    /// Every required slot is filled.
    pub complete: bool,
    /// Characters of the input that fit no slot.
    pub rejected: usize,
}

fn cased(c: char, casing: Option<Casing>) -> String {
    match casing {
        Some(Casing::Upper) => c.to_uppercase().collect(),
        Some(Casing::Lower) => c.to_lowercase().collect(),
        None => c.to_string(),
    }
}

/// Fits typed text into the mask. Literals in the text are matched where the mask has
/// them and skipped elsewhere, so both stored forms (with or without literals) and
/// pasted text format the same way. Literals after the last typed character are left
/// off, so deleting backwards never gets stuck on one.
pub fn apply_mask(mask: &ParsedMask, text: &str) -> MaskResult {
    let input: Vec<char> = text.chars().collect();
    let literals: Vec<char> = mask
        .slots
        .iter()
        .filter_map(|s| match s {
            Slot::Literal(c) => Some(*c),
            _ => None,
        })
        .collect();
    let (mut display, mut data, mut pending) = (String::new(), String::new(), String::new());
    let (mut rejected, mut complete, mut at) = (0usize, true, 0usize);
    for slot in &mask.slots {
        match slot {
            Slot::Literal(c) => {
                if input.get(at) == Some(c) {
                    at += 1;
                }
                pending.push(*c);
            }
            Slot::Input {
                accepts,
                required,
                casing,
            } => {
                // Skip characters this slot cannot take: stray literals silently, anything else counts.
                while at < input.len() && !accepts.test(input[at]) {
                    if !required {
                        break;
                    }
                    if !literals.contains(&input[at]) {
                        rejected += 1;
                    }
                    at += 1;
                }
                if at < input.len() && accepts.test(input[at]) {
                    let c = cased(input[at], *casing);
                    at += 1;
                    display.push_str(&pending);
                    display.push_str(&c);
                    data.push_str(&c);
                    pending.clear();
                } else if *required {
                    complete = false;
                }
            }
        }
    }
    for c in &input[at.min(input.len())..] {
        if !literals.contains(c) {
            rejected += 1;
        }
    }
    MaskResult {
        stored: if mask.store_literals {
            display.clone()
        } else {
            data
        },
        display,
        complete,
        rejected,
    }
}

/// The empty mask as shown in a placeholder, e.g. `(___) ___-____`.
pub fn mask_template(mask: &ParsedMask) -> String {
    mask.slots
        .iter()
        .map(|slot| match slot {
            Slot::Literal(c) => *c,
            Slot::Input { .. } => mask.placeholder,
        })
        .collect()
}

/// A message when `value` does not fill the mask, else None. Blank values pass.
pub fn mask_problem(mask_text: &str, value: Option<&str>, label: &str) -> Option<String> {
    let value = value.filter(|v| !v.is_empty())?;
    let mask = parse_mask(mask_text);
    let result = apply_mask(&mask, value);
    if result.rejected > 0 || !result.complete {
        return Some(format!("{label} must match {}.", mask_template(&mask)));
    }
    None
}

/// Stored text for typed text, or a readable message when it does not fit.
pub fn masked_text(mask_text: &str, text: &str, label: &str) -> Result<String, String> {
    if let Some(problem) = mask_problem(mask_text, Some(text), label) {
        return Err(problem);
    }
    if text.is_empty() {
        return Ok(String::new());
    }
    Ok(apply_mask(&parse_mask(mask_text), text).stored)
}

/// A mask that has no slot to type into cannot be used.
pub fn mask_issue(mask_text: &str) -> Option<&'static str> {
    let has_input = parse_mask(mask_text)
        .slots
        .iter()
        .any(|s| matches!(s, Slot::Input { .. }));
    (!has_input).then_some("The input mask has no characters to type into.")
}
