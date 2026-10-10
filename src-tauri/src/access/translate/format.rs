//! Access `Format` property values → ixtable format patterns (`src/expr` README).

/// Converts a named or custom Access format. None when it has no equivalent
/// (General Number, Yes/No, text formats like `>` or `@`).
pub fn format_pattern(access: &str) -> Option<String> {
    let named = match access.trim().to_ascii_lowercase().as_str() {
        "" | "general number" | "general" => return None,
        "currency" | "euro" => Some("$#,##0.00"),
        "fixed" => Some("0.00"),
        "standard" => Some("#,##0.00"),
        "percent" => Some("0.00%"),
        "scientific" => return None,
        "general date" => Some("M/d/yyyy h:mm:ss tt"),
        "long date" => Some("dddd, MMMM d, yyyy"),
        "medium date" => Some("dd-MMM-yy"),
        "short date" => Some("M/d/yyyy"),
        "long time" => Some("h:mm:ss tt"),
        "medium time" => Some("h:mm tt"),
        "short time" => Some("HH:mm"),
        "yes/no" | "true/false" | "on/off" => return None,
        _ => None,
    };
    if let Some(n) = named {
        return Some(n.to_string());
    }
    // Only the first section (positive values) of a multi-section format.
    let section = first_section(access);
    if section.chars().any(|c| matches!(c, '0' | '#')) {
        return Some(number_pattern(&section));
    }
    if section
        .chars()
        .any(|c| matches!(c.to_ascii_lowercase(), 'y' | 'm' | 'd' | 'h' | 'n' | 's'))
        && !section.contains(['@', '&', '<', '>'])
    {
        return Some(date_pattern(&section));
    }
    None
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

fn number_pattern(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                for d in chars.by_ref() {
                    if d == '"' {
                        break;
                    }
                    out.push(d);
                }
            }
            '\\' => {
                if let Some(d) = chars.next() {
                    out.push(d);
                }
            }
            '[' => {
                // Colors like [Red] have no equivalent.
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                }
            }
            '(' | ')' | '_' | '*' => {}
            _ => out.push(c),
        }
    }
    out.trim().to_string()
}

/// Access date tokens: m/mm month, n/nn minute, h/hh hour, AM/PM.
fn date_pattern(s: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let run = chars[i..]
            .iter()
            .take_while(|x| x.eq_ignore_ascii_case(&c))
            .count();
        let rest: String = chars[i..].iter().collect::<String>().to_ascii_uppercase();
        if rest.starts_with("AM/PM") {
            out.push_str("tt");
            i += 5;
            continue;
        }
        match c.to_ascii_lowercase() {
            'm' => out.push_str(&"M".repeat(run.min(4))),
            'n' => out.push_str(&"m".repeat(run.min(2))),
            'h' => out.push_str(
                &if s.to_ascii_uppercase().contains("AM/PM") {
                    "h"
                } else {
                    "H"
                }
                .repeat(run.min(2)),
            ),
            'd' => out.push_str(&"d".repeat(run.min(4))),
            'y' => out.push_str(if run >= 3 { "yyyy" } else { "yy" }),
            's' => out.push_str(&"s".repeat(run.min(2))),
            '"' => {
                let end = chars[i + 1..]
                    .iter()
                    .position(|x| *x == '"')
                    .map(|p| i + 1 + p)
                    .unwrap_or(chars.len());
                out.push_str(&format!(
                    "'{}'",
                    chars[i + 1..end].iter().collect::<String>()
                ));
                i = end + 1;
                continue;
            }
            '\\' => {
                if let Some(d) = chars.get(i + 1) {
                    out.push_str(&format!("'{d}'"));
                }
                i += 2;
                continue;
            }
            _ => {
                for _ in 0..run {
                    out.push(c);
                }
            }
        }
        i += run;
    }
    out
}
