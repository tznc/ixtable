//! Human-readable sizes and times for the persistence panels.
//! Port of src/persistence/format.ts.

/// Decimal byte size (the 500 MB cloud limit is decimal).
pub fn format_bytes(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let units = ["KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit: i32 = -1;
    while value >= 1000.0 && unit < units.len() as i32 - 1 {
        value /= 1000.0;
        unit += 1;
    }
    let text = if value >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    format!("{text} {}", units[unit as usize])
}

pub fn format_percent(fraction: f64) -> String {
    let tenths = (fraction * 1000.0).round() / 10.0;
    format!("{tenths}%")
}

/// Local date and time of an RFC 3339 timestamp ("18 Oct 2026, 14:05"); other text is
/// returned as it is.
pub fn format_date_time(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|t| t.with_timezone(&chrono::Local).format("%-d %b %Y, %H:%M").to_string())
        .unwrap_or_else(|_| iso.to_string())
}

/// How long before `now` an RFC 3339 time was: "just now", "5 min ago", "2 hours ago",
/// "3 days ago"; older times and unreadable text fall back to the date.
pub fn relative_time(iso: &str, now: chrono::DateTime<chrono::Utc>) -> String {
    let Ok(t) = chrono::DateTime::parse_from_rfc3339(iso) else {
        return iso.to_string();
    };
    let secs = (now - t.with_timezone(&chrono::Utc)).num_seconds().max(0);
    let unit = |n: i64, word: &str| format!("{n} {word}{} ago", if n == 1 { "" } else { "s" });
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => unit(secs / 3600, "hour"),
        86_400..=2_591_999 => unit(secs / 86_400, "day"),
        _ => format_date_time(iso),
    }
}

fn is_uuid(word: &str) -> bool {
    word.len() == 36
        && word.char_indices().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        })
}

/// Makes backend text readable: ids after "checkpoint" are dropped, other ids become
/// "(id)", and RFC 3339 timestamps become dates.
pub fn scrub_internal(text: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for word in text.split(' ') {
        let core = word.trim_matches(|c: char| ",.;:()\"".contains(c));
        let at = if core.is_empty() { 0 } else { word.find(core).unwrap_or(0) };
        let (lead, trail) = (&word[..at], &word[at + core.len()..]);
        if is_uuid(core) {
            if out.last().is_some_and(|w| w.ends_with("checkpoint")) {
                if let Some(last) = out.last_mut() {
                    last.push_str(trail);
                }
            } else {
                out.push(format!("{lead}(id){trail}"));
            }
        } else if chrono::DateTime::parse_from_rfc3339(core).is_ok() {
            out.push(format!("{lead}{}{trail}", format_date_time(core)));
        } else {
            out.push(word.to_string());
        }
    }
    out.join(" ")
}

pub use crate::ui::widgets::plural;

#[cfg(test)]
mod tests {
    use super::{format_bytes, format_percent, plural, relative_time, scrub_internal};
    use chrono::{TimeZone, Utc};

    #[test]
    fn formats_decimal_sizes() {
        assert_eq!(format_bytes(7), "7 B");
        assert_eq!(format_bytes(2048), "2.0 KB");
        assert_eq!(format_bytes(200_000), "200 KB");
        assert_eq!(format_bytes(500_000_000), "500 MB");
        assert_eq!(format_bytes(1_500_000_000), "1.5 GB");
    }

    #[test]
    fn formats_percent_and_plurals() {
        assert_eq!(format_percent(0.25), "25%");
        assert_eq!(format_percent(0.1234), "12.3%");
        assert_eq!(plural(1, "asset"), "1 asset");
        assert_eq!(plural(2, "asset"), "2 assets");
    }

    #[test]
    fn relative_times_read_like_speech() {
        let now = Utc.with_ymd_and_hms(2026, 10, 18, 12, 0, 0).unwrap();
        assert_eq!(relative_time("2026-10-18T11:59:50Z", now), "just now");
        assert_eq!(relative_time("2026-10-18T11:55:00Z", now), "5 min ago");
        assert_eq!(relative_time("2026-10-18T11:00:00Z", now), "1 hour ago");
        assert_eq!(relative_time("2026-10-16T12:00:00Z", now), "2 days ago");
        assert_eq!(relative_time("not a time", now), "not a time");
    }

    #[test]
    fn scrub_hides_ids_and_formats_timestamps() {
        let text = "restore the checkpoint 123e4567-e89b-12d3-a456-426614174000. At 2026-10-18T14:05:00.123456789Z it ran";
        let out = scrub_internal(text);
        assert!(!out.contains("123e4567") && !out.contains("T14:05"), "{out}");
        assert!(out.starts_with("restore the checkpoint. At "), "{out}");
        assert!(out.contains("2026"), "{out}");
    }
}
