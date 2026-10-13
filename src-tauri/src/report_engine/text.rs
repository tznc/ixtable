//! Text metrics: Helvetica advance widths, the bundled fallback fonts for characters
//! outside WinAnsi, normalization and greedy word wrap. Preview and PDF measure with
//! the same tables, so lines wrap identically everywhere.

use super::metrics::{HELVETICA_BOLD_WIDTHS, HELVETICA_WIDTHS};
use std::cell::RefCell;
use std::collections::HashMap;

/// Unicode code points outside Latin-1 that WinAnsiEncoding maps into 128..159.
const WIN_ANSI_HIGH: [(u32, u32); 27] = [
    (0x20ac, 128),
    (0x201a, 130),
    (0x0192, 131),
    (0x201e, 132),
    (0x2026, 133),
    (0x2020, 134),
    (0x2021, 135),
    (0x02c6, 136),
    (0x2030, 137),
    (0x0160, 138),
    (0x2039, 139),
    (0x0152, 140),
    (0x017d, 142),
    (0x2018, 145),
    (0x2019, 146),
    (0x201c, 147),
    (0x201d, 148),
    (0x2022, 149),
    (0x2013, 150),
    (0x2014, 151),
    (0x02dc, 152),
    (0x2122, 153),
    (0x0161, 154),
    (0x203a, 155),
    (0x0153, 156),
    (0x017e, 158),
    (0x0178, 159),
];

const QUESTION: u32 = 63;

/// Line advance as a multiple of the font size.
pub const LINE_HEIGHT: f64 = 1.2;
/// Baseline offset from the top of a line, as a multiple of the font size.
pub const BASELINE: f64 = 0.8555;

/// WinAnsi byte for one character, or `?` (63) when the character has none.
pub fn win_ansi_code(ch: char) -> u32 {
    let cp = ch as u32;
    if (32..=126).contains(&cp) || (160..=255).contains(&cp) {
        return cp;
    }
    WIN_ANSI_HIGH
        .iter()
        .find(|(c, _)| *c == cp)
        .map_or(QUESTION, |(_, code)| *code)
}

thread_local! {
    static FALLBACK: RefCell<HashMap<char, Option<(usize, i32)>>> = RefCell::new(HashMap::new());
}

/// Bundled fallback font (index into the fallback fonts) and advance width in 1/1000 em
/// for a character outside WinAnsi, or `None` when no bundled font draws it.
pub fn fallback_glyph(ch: char) -> Option<(usize, i32)> {
    if win_ansi_code(ch) != QUESTION || ch as u32 == QUESTION {
        return None;
    }
    FALLBACK.with(|cache| {
        *cache.borrow_mut().entry(ch).or_insert_with(|| {
            crate::report_pdf::glyph_for(ch as u32)
                .map(|(font, gid)| (font, crate::report_pdf::glyph_width(font, gid)))
        })
    })
}

/// Font a character prints in: -1 for Helvetica, else the fallback font index.
pub fn font_of(ch: char) -> i32 {
    fallback_glyph(ch).map_or(-1, |(font, _)| font as i32)
}

/// Text as the report engine prints it: tabs become spaces, line breaks become `\n`,
/// other control characters are dropped and characters neither the standard PDF fonts
/// nor the bundled fallback fonts can show become `?`.
pub fn normalize_text(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n").replace('\t', " ");
    let mut out = String::new();
    for ch in text.chars() {
        if ch == '\n' {
            out.push(ch);
        } else if ch < ' ' || ch == '\u{7f}' {
            continue;
        } else if win_ansi_code(ch) == QUESTION && fallback_glyph(ch).is_none() {
            out.push('?');
        } else {
            out.push(ch);
        }
    }
    out
}

/// Width of `text` (already normalized) in points.
pub fn measure_text(text: &str, font_size: f64, bold: bool) -> f64 {
    let table = if bold { &HELVETICA_BOLD_WIDTHS } else { &HELVETICA_WIDTHS };
    let mut units = 0i64;
    for ch in text.chars() {
        units += match fallback_glyph(ch) {
            Some((_, width)) => i64::from(width),
            None => win_ansi_code(ch)
                .checked_sub(32)
                .and_then(|i| table.get(i as usize))
                .map_or(0, |w| i64::from(*w)),
        };
    }
    units as f64 * font_size / 1000.0
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub text: String,
    /// -1 for Helvetica, else the fallback font index.
    pub font: i32,
    /// Offset from the start of the line in points.
    pub dx: f64,
    pub width: f64,
}

/// Splits a laid-out line into runs of one font each, positioned with the fixed metrics.
pub fn text_runs(line: &str, font_size: f64, bold: bool) -> Vec<TextRun> {
    let mut runs: Vec<TextRun> = vec![];
    let mut dx = 0.0;
    for ch in line.chars() {
        let font = font_of(ch);
        let width = measure_text(&ch.to_string(), font_size, bold);
        match runs.last_mut() {
            Some(last) if last.font == font => {
                last.text.push(ch);
                last.width += width;
            }
            _ => runs.push(TextRun {
                text: ch.to_string(),
                font,
                dx,
                width,
            }),
        }
        dx += width;
    }
    runs
}

/// Greedy word wrap with the fixed Helvetica metrics. Explicit `\n` starts a new line; a
/// word wider than `max_width` is broken between characters.
pub fn wrap_text(text: &str, max_width: f64, font_size: f64, bold: bool) -> Vec<String> {
    let fits = |s: &str| measure_text(s, font_size, bold) <= max_width + 1e-9;
    let mut lines: Vec<String> = vec![];
    for paragraph in normalize_text(text).split('\n') {
        let mut current = String::new();
        for word in paragraph.split(' ') {
            let candidate = if current.is_empty() {
                word.to_string()
            } else {
                format!("{current} {word}")
            };
            if fits(&candidate) || (current.is_empty() && word.is_empty()) {
                current = candidate;
                continue;
            }
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            current = word.to_string();
            while current.chars().count() > 1 && !fits(&current) {
                let chars: Vec<char> = current.chars().collect();
                let mut n = 1;
                while n < chars.len() && fits(&chars[..=n].iter().collect::<String>()) {
                    n += 1;
                }
                lines.push(chars[..n].iter().collect());
                current = chars[n..].iter().collect();
            }
        }
        lines.push(current);
    }
    lines
}
