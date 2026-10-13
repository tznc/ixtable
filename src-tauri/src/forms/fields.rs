//! Field helpers forms need: input masks, per-column field settings, choices, attachments
//! and the text of formatted values. Port of the parts of `src/fields/*` the form logic
//! and the Runtime use (`mask.ts`, `values.ts`, `defaults.ts`, `richtext.ts` plain text).

use crate::design::{Control, ControlKind, Form, SelectOption};
use crate::recordstore::{EntitySettings, FieldSettings};
use serde_json::Value;

// ---- Input masks -------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum Slot {
    Literal(char),
    Input {
        class: char,
        required: bool,
        casing: Option<Casing>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Casing {
    Upper,
    Lower,
}

/// A parsed Access-style mask (`0` digit, `L` letter, `A` letter or digit and so on).
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedMask {
    slots: Vec<Slot>,
    pub store_literals: bool,
    pub placeholder: char,
}

#[derive(Clone, Debug, PartialEq)]
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

fn slot_class(c: char) -> Option<bool> {
    // Some(required) for a slot character.
    match c {
        '0' | 'L' | 'A' | '&' => Some(true),
        '9' | '#' | '?' | 'a' | 'C' => Some(false),
        _ => None,
    }
}

fn accepts(class: char, c: char) -> bool {
    let letter = c.is_alphabetic() && !c.is_numeric();
    match class {
        '0' | '9' => c.is_ascii_digit(),
        '#' => c.is_ascii_digit() || c == ' ' || c == '+' || c == '-',
        'L' | '?' => letter,
        'A' | 'a' => letter || c.is_ascii_digit(),
        _ => true,
    }
}

/// Splits on `;` outside quotes and escapes.
fn sections(mask: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut quoted = false;
    let chars: Vec<char> = mask.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && i + 1 < chars.len() {
            let last = out.len() - 1;
            out[last].push(c);
            out[last].push(chars[i + 1]);
            i += 2;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        if c == ';' && !quoted {
            out.push(String::new());
        } else {
            let last = out.len() - 1;
            out[last].push(c);
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
    let chars: Vec<char> = pattern.chars().collect();
    let mut slots = vec![];
    let mut casing = None;
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
            // Ignored.
        } else if let Some(required) = slot_class(c) {
            slots.push(Slot::Input {
                class: c,
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

fn cased(c: char, casing: Option<Casing>) -> String {
    match casing {
        Some(Casing::Upper) => c.to_uppercase().collect(),
        Some(Casing::Lower) => c.to_lowercase().collect(),
        None => c.to_string(),
    }
}

/// Fits typed text into the mask. Literals in the text are matched where the mask has them
/// and skipped elsewhere; literals after the last typed character are left off.
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
    let (mut rejected, mut complete, mut at) = (0, true, 0);
    for slot in &mask.slots {
        match slot {
            Slot::Literal(c) => {
                if input.get(at) == Some(c) {
                    at += 1;
                }
                pending.push(*c);
            }
            Slot::Input {
                class,
                required,
                casing,
            } => {
                while at < input.len() && !accepts(*class, input[at]) {
                    if !required {
                        break;
                    }
                    if !literals.contains(&input[at]) {
                        rejected += 1;
                    }
                    at += 1;
                }
                if at < input.len() && accepts(*class, input[at]) {
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
    rejected += input[at.min(input.len())..]
        .iter()
        .filter(|c| !literals.contains(c))
        .count();
    let stored = if mask.store_literals {
        display.clone()
    } else {
        data
    };
    MaskResult {
        display,
        stored,
        complete,
        rejected,
    }
}

/// The empty mask as shown in a placeholder, e.g. `(___) ___-____`.
pub fn mask_template(mask: &ParsedMask) -> String {
    mask.slots
        .iter()
        .map(|s| match s {
            Slot::Literal(c) => *c,
            Slot::Input { .. } => mask.placeholder,
        })
        .collect()
}

/// A message when `value` does not fill the mask, else None. Blank values pass.
pub fn mask_problem(mask_text: &str, value: &Value, label: &str) -> Option<String> {
    let text = value_text(value);
    if text.is_empty() {
        return None;
    }
    let mask = parse_mask(mask_text);
    let result = apply_mask(&mask, &text);
    if result.rejected > 0 || !result.complete {
        return Some(format!("{label} must match {}.", mask_template(&mask)));
    }
    None
}

/// Stored text for typed text, or a readable message when it does not fit.
pub fn masked_text(mask_text: &str, text: &str, label: &str) -> Result<String, String> {
    if let Some(problem) = mask_problem(mask_text, &Value::String(text.into()), label) {
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

// ---- Values ------------------------------------------------------------------------

/// JavaScript's `String(value)` for a JSON value ("" for null).
pub fn value_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_text(n),
        other => other.to_string(),
    }
}

fn number_text(n: &serde_json::Number) -> String {
    if let Some(i) = n.as_i64() {
        return i.to_string();
    }
    match n.as_f64() {
        Some(f) if f.fract() == 0.0 && f.abs() < 1e21 => format!("{}", f as i128),
        Some(f) => format!("{f}"),
        None => n.to_string(),
    }
}

/// The field settings of `table.column`, if any.
pub fn field_settings_for<'a>(
    entities: &'a [EntitySettings],
    table: Option<&str>,
    column: Option<&str>,
) -> Option<&'a FieldSettings> {
    let (table, column) = (table?, column?);
    entities
        .iter()
        .find(|e| e.table == table)?
        .fields
        .iter()
        .find(|f| f.column == column)
}

fn json_array(value: &Value) -> Vec<Value> {
    match value {
        Value::Array(items) => items.clone(),
        Value::String(s) if s.trim().starts_with('[') => {
            match serde_json::from_str::<Value>(s) {
                Ok(Value::Array(items)) => items,
                _ => vec![],
            }
        }
        _ => vec![],
    }
}

/// One file of an attachment field, as stored in the field's JSON array.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AttachmentRef {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub mime: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub sha256: String,
}

/// The files of an attachment value; anything but a JSON array of files is none.
pub fn parse_attachments(value: &Value) -> Vec<AttachmentRef> {
    json_array(value)
        .into_iter()
        .filter(|f| f.get("id").is_some_and(Value::is_string))
        .filter_map(|f| serde_json::from_value(f).ok())
        .collect()
}

/// The stored value for a list of files: a JSON array, or null when empty.
pub fn attachments_value(files: &[AttachmentRef]) -> Value {
    if files.is_empty() {
        Value::Null
    } else {
        Value::String(serde_json::to_string(files).unwrap_or_default())
    }
}

/// The choices of a multi-select value; a plain string is one choice.
pub fn parse_choices(value: &Value) -> Vec<String> {
    if value.is_null() || value == &Value::String(String::new()) {
        return vec![];
    }
    let items = json_array(value);
    let bracketed = matches!(value, Value::String(s) if s.trim().starts_with('['));
    if !items.is_empty() || bracketed {
        return items.iter().map(value_text).collect();
    }
    vec![value_text(value)]
}

/// The stored value for chosen items: a JSON array, or null when none.
pub fn choices_value(items: &[String]) -> Value {
    if items.is_empty() {
        Value::Null
    } else {
        Value::String(serde_json::to_string(items).unwrap_or_default())
    }
}

/// Readable size, e.g. `12 KB` (decimal units).
pub fn file_size(bytes: u64) -> String {
    if bytes < 1000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{} KB", (bytes as f64 / 1000.0).round() as u64)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    }
}

const DROPPED: [&str; 7] = [
    "script", "style", "template", "iframe", "object", "embed", "noscript",
];
const BLOCKS: [&str; 8] = ["p", "div", "li", "h1", "h2", "h3", "blockquote", "br"];

fn decode_entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        let Some(end) = tail.find(';').filter(|e| *e <= 8) else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some('\u{a0}'),
            _ => name
                .strip_prefix('#')
                .and_then(|n| match n.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => n.parse().ok(),
                })
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Plain text of a rich-text (sanitized HTML) value, for lists, datasheets and validation.
pub fn rich_text_plain(html: &Value) -> String {
    let text = value_text(html);
    if !text.contains(['<', '&']) {
        return text;
    }
    let mut parts = String::new();
    let mut skip: Option<String> = None;
    let mut rest = text.as_str();
    while !rest.is_empty() {
        let Some(open) = rest.find('<') else {
            if skip.is_none() {
                parts.push_str(&decode_entities(rest));
            }
            break;
        };
        if skip.is_none() {
            parts.push_str(&decode_entities(&rest[..open]));
        }
        let tail = &rest[open..];
        let Some(close) = tail.find('>') else {
            if skip.is_none() {
                parts.push_str(&decode_entities(tail));
            }
            break;
        };
        let tag = tail[1..close].trim();
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        match &skip {
            Some(dropped) => {
                if closing && *dropped == name {
                    skip = None;
                }
            }
            None => {
                if !closing && DROPPED.contains(&name.as_str()) {
                    skip = Some(name);
                } else if !closing && BLOCKS.contains(&name.as_str()) && !parts.is_empty() {
                    parts.push('\n');
                }
            }
        }
        rest = &tail[close + 1..];
    }
    let mut collapsed = String::new();
    for c in parts.chars() {
        if c == '\n' && collapsed.ends_with('\n') {
            continue;
        }
        collapsed.push(c);
    }
    collapsed.trim().to_string()
}

/// Text of a formatted field's value in a list cell, datasheet or report.
pub fn field_text(value: &Value, format: Option<&str>) -> String {
    match format {
        Some("richText") => rich_text_plain(value).replace('\n', " "),
        Some("attachment") => {
            let files = parse_attachments(value);
            match files.len() {
                0 => String::new(),
                1 => files[0].name.clone(),
                n => format!("{n} files"),
            }
        }
        Some("multiSelect") => parse_choices(value).join(", "),
        _ => value_text(value),
    }
}

/// The form with field settings filled into its controls: a plain text control bound to a
/// rich-text, attachment or multi-select field becomes that kind, a text control without
/// its own input mask uses its column's, and a multi-select without choices uses the
/// field's. Returns None when nothing changes.
pub fn with_field_defaults(form: &Form, entities: &[EntitySettings]) -> Option<Form> {
    let table = crate::forms::schema::form_table(form)?;
    let entity = entities
        .iter()
        .find(|e| e.table == table && !e.fields.is_empty())?;
    let mut changed = false;
    let controls: Vec<Control> = form
        .controls
        .iter()
        .map(|control| {
            let bound = control.binding.as_ref();
            let table_name = bound
                .and_then(|b| b.table.as_deref())
                .filter(|t| !t.is_empty())
                .unwrap_or(table);
            let field = if entity.table == table_name {
                bound.and_then(|b| entity.fields.iter().find(|f| f.column == b.column))
            } else {
                field_settings_for(entities, Some(table_name), bound.map(|b| b.column.as_str()))
            };
            let Some(field) = field else {
                return control.clone();
            };
            let format = field.format.as_deref().unwrap_or("");
            let options = |values: &[String]| -> Vec<SelectOption> {
                values
                    .iter()
                    .map(|v| SelectOption {
                        value: v.clone(),
                        label: v.clone(),
                    })
                    .collect()
            };
            let formatted = match format {
                "richText" => Some(ControlKind::RichText),
                "attachment" => Some(ControlKind::Attachment),
                "multiSelect" => Some(ControlKind::MultiSelect),
                _ => None,
            };
            if let (Some(kind), true) = (
                formatted,
                matches!(control.kind, ControlKind::Text | ControlKind::Multiline),
            ) {
                changed = true;
                let mut next = control.clone();
                next.kind = kind;
                if kind == ControlKind::MultiSelect {
                    next.options = options(&field.options);
                }
                return next;
            }
            if control.kind == ControlKind::Text
                && control.input_mask.as_deref().unwrap_or("").is_empty()
                && field.input_mask.as_deref().is_some_and(|m| !m.is_empty())
            {
                changed = true;
                let mut next = control.clone();
                next.input_mask = field.input_mask.clone();
                return next;
            }
            if control.kind == ControlKind::MultiSelect
                && control.options.is_empty()
                && !field.options.is_empty()
            {
                changed = true;
                let mut next = control.clone();
                next.options = options(&field.options);
                return next;
            }
            control.clone()
        })
        .collect();
    changed.then(|| Form {
        controls,
        ..form.clone()
    })
}

#[cfg(test)]
#[path = "fields_tests.rs"]
mod tests;
