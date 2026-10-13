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

/// Local date and time of an RFC 3339 timestamp; other text is returned as it is.
pub fn format_date_time(iso: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M:%S")
                .to_string()
        })
        .unwrap_or_else(|_| iso.to_string())
}

pub fn plural(count: usize, word: &str) -> String {
    format!("{count} {word}{}", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::{format_bytes, format_percent, plural};

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
}
