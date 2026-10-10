//! Streaming file writers for CSV, XLSX and JSON exports. Each sink accepts
//! rows one at a time and never holds the whole result in memory.
use super::{ExportColumn, ExportFormat};
use crate::data::{DataValue, LogicalType};
use chrono::{DateTime, Datelike, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use rust_xlsxwriter::{ExcelDateTime, Format, Workbook, Worksheet};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// Most data rows an XLSX sheet holds: Excel's 1,048,576 rows minus the header.
pub const XLSX_MAX_ROWS: u64 = 1_048_575;

/// Longest string Excel keeps in one cell.
const XLSX_MAX_CELL_CHARS: usize = 32_767;

/// Largest integer an Excel number (an f64) represents exactly.
const MAX_EXACT_INTEGER: i64 = 1 << 53;

/// Receives exported rows in order and writes them to one file.
pub trait RowSink {
    /// Writes one data row; `values` has one entry per exported column.
    fn row(&mut self, values: &[DataValue]) -> Result<(), String>;
    /// Flushes and closes the file. Must be called; dropping without finish
    /// leaves a partial file (the caller deletes it).
    fn finish(self: Box<Self>) -> Result<(), String>;
}

/// Creates `path` and returns a sink that writes `format`, header included.
pub fn open_sink(
    format: ExportFormat,
    path: &Path,
    columns: &[ExportColumn],
) -> Result<Box<dyn RowSink>, String> {
    match format {
        ExportFormat::Csv => Ok(Box::new(CsvSink::open(path, columns)?)),
        ExportFormat::Json => Ok(Box::new(JsonSink::open(path, columns)?)),
        ExportFormat::Xlsx => Ok(Box::new(XlsxSink::open(path, columns, XLSX_MAX_ROWS)?)),
    }
}

fn create(path: &Path) -> Result<BufWriter<File>, String> {
    File::create(path)
        .map(|f| BufWriter::with_capacity(256 * 1024, f))
        .map_err(|e| format!("Could not create {}: {e}", path.display()))
}

fn io_err(e: std::io::Error) -> String {
    format!("Could not write export file: {e}")
}

// CSV

struct CsvSink {
    out: BufWriter<File>,
}

impl CsvSink {
    fn open(path: &Path, columns: &[ExportColumn]) -> Result<Self, String> {
        let mut out = create(path)?;
        // The BOM makes Excel read the file as UTF-8.
        out.write_all(&[0xEF, 0xBB, 0xBF]).map_err(io_err)?;
        let mut sink = Self { out };
        let header: Vec<String> = columns.iter().map(|c| c.name.clone()).collect();
        sink.write_fields(header.iter().map(String::as_str))?;
        Ok(sink)
    }

    fn write_fields<'a>(&mut self, fields: impl Iterator<Item = &'a str>) -> Result<(), String> {
        for (i, field) in fields.enumerate() {
            if i > 0 {
                self.out.write_all(b",").map_err(io_err)?;
            }
            write_csv_field(&mut self.out, field).map_err(io_err)?;
        }
        self.out.write_all(b"\r\n").map_err(io_err)
    }
}

fn write_csv_field(out: &mut impl Write, field: &str) -> std::io::Result<()> {
    let needs_quotes =
        field.contains([',', '"', '\r', '\n']) || field.starts_with(' ') || field.ends_with(' ');
    if !needs_quotes {
        return out.write_all(field.as_bytes());
    }
    out.write_all(b"\"")?;
    let mut rest = field;
    while let Some(i) = rest.find('"') {
        out.write_all(&rest.as_bytes()[..=i])?;
        out.write_all(b"\"")?;
        rest = &rest[i + 1..];
    }
    out.write_all(rest.as_bytes())?;
    out.write_all(b"\"")
}

fn real_text(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.is_infinite() {
        if v > 0.0 { "inf" } else { "-inf" }.into()
    } else {
        v.to_string()
    }
}

fn csv_text(value: &DataValue) -> std::borrow::Cow<'_, str> {
    use std::borrow::Cow::{Borrowed, Owned};
    match value {
        DataValue::Null => Borrowed(""),
        DataValue::Integer(v) => Owned(v.to_string()),
        DataValue::Real(v) => Owned(real_text(*v)),
        DataValue::Boolean(v) => Borrowed(if *v { "true" } else { "false" }),
        DataValue::Text(s)
        | DataValue::Blob(s)
        | DataValue::Date(s)
        | DataValue::Timestamp(s)
        | DataValue::Decimal(s)
        | DataValue::Time(s) => Borrowed(s),
    }
}

impl RowSink for CsvSink {
    fn row(&mut self, values: &[DataValue]) -> Result<(), String> {
        let texts: Vec<_> = values.iter().map(csv_text).collect();
        self.write_fields(texts.iter().map(|t| &**t))
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        self.out.flush().map_err(io_err)
    }
}

// JSON

struct JsonSink {
    out: BufWriter<File>,
    keys: Vec<String>,
    json_columns: Vec<bool>,
    first: bool,
}

/// Column names made unique: later duplicates get `_2`, `_3`, ... suffixes.
fn unique_keys(columns: &[ExportColumn]) -> Vec<String> {
    let mut used: HashSet<String> = columns.iter().map(|c| c.name.clone()).collect();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut keys = Vec::with_capacity(columns.len());
    for column in columns {
        if seen.insert(column.name.as_str()) {
            keys.push(column.name.clone());
            continue;
        }
        let mut n = 2;
        let key = loop {
            let candidate = format!("{}_{n}", column.name);
            if used.insert(candidate.clone()) {
                break candidate;
            }
            n += 1;
        };
        keys.push(key);
    }
    keys
}

impl JsonSink {
    fn open(path: &Path, columns: &[ExportColumn]) -> Result<Self, String> {
        let mut out = create(path)?;
        out.write_all(b"[").map_err(io_err)?;
        Ok(Self {
            out,
            keys: unique_keys(columns),
            json_columns: columns
                .iter()
                .map(|c| c.logical == Some(LogicalType::Json))
                .collect(),
            first: true,
        })
    }

    fn write_value(&mut self, value: &DataValue, json_column: bool) -> Result<(), String> {
        let out = &mut self.out;
        match value {
            DataValue::Null => out.write_all(b"null").map_err(io_err),
            DataValue::Integer(v) => out.write_all(v.to_string().as_bytes()).map_err(io_err),
            DataValue::Real(v) => match serde_json::Number::from_f64(*v) {
                Some(n) => out.write_all(n.to_string().as_bytes()).map_err(io_err),
                None => out.write_all(b"null").map_err(io_err),
            },
            DataValue::Boolean(v) => out
                .write_all(if *v { b"true" as &[u8] } else { b"false" })
                .map_err(io_err),
            DataValue::Text(s)
                if json_column && serde_json::from_str::<serde::de::IgnoredAny>(s).is_ok() =>
            {
                out.write_all(s.trim().as_bytes()).map_err(io_err)
            }
            other => {
                serde_json::to_writer(out, csv_text(other).as_ref()).map_err(|e| io_err(e.into()))
            }
        }
    }
}

impl RowSink for JsonSink {
    fn row(&mut self, values: &[DataValue]) -> Result<(), String> {
        let sep: &[u8] = if self.first { b"\n" } else { b",\n" };
        self.first = false;
        self.out.write_all(sep).map_err(io_err)?;
        self.out.write_all(b"{").map_err(io_err)?;
        for (i, value) in values.iter().enumerate() {
            if i > 0 {
                self.out.write_all(b",").map_err(io_err)?;
            }
            let key = self
                .keys
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("column_{}", i + 1));
            serde_json::to_writer(&mut self.out, &key).map_err(|e| io_err(e.into()))?;
            self.out.write_all(b":").map_err(io_err)?;
            let json_column = self.json_columns.get(i).copied().unwrap_or(false);
            self.write_value(value, json_column)?;
        }
        self.out.write_all(b"}").map_err(io_err)
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        let tail: &[u8] = if self.first { b"]\n" } else { b"\n]\n" };
        self.out.write_all(tail).map_err(io_err)?;
        self.out.flush().map_err(io_err)
    }
}

// XLSX

pub(super) struct XlsxSink {
    workbook: Workbook,
    sheet: Worksheet,
    path: std::path::PathBuf,
    date_format: Format,
    timestamp_format: Format,
    time_format: Format,
    rows: u64,
    max_rows: u64,
}

impl XlsxSink {
    pub(super) fn open(
        path: &Path,
        columns: &[ExportColumn],
        max_rows: u64,
    ) -> Result<Self, String> {
        let mut workbook = Workbook::new();
        let mut sheet = workbook.new_worksheet_with_constant_memory();
        let xerr = |e: rust_xlsxwriter::XlsxError| format!("Could not write XLSX: {e}");
        sheet.set_name("Export").map_err(xerr)?;
        let bold = Format::new().set_bold();
        for (i, column) in columns.iter().enumerate() {
            let text = truncate_chars(&column.name);
            sheet
                .write_string_with_format(0, i as u16, text, &bold)
                .map_err(xerr)?;
        }
        sheet.set_freeze_panes(1, 0).map_err(xerr)?;
        // Fail early when the destination is not writable.
        File::create(path).map_err(|e| format!("Could not create {}: {e}", path.display()))?;
        Ok(Self {
            workbook,
            sheet,
            path: path.to_path_buf(),
            date_format: Format::new().set_num_format("yyyy-mm-dd"),
            timestamp_format: Format::new().set_num_format("yyyy-mm-dd hh:mm:ss"),
            time_format: Format::new().set_num_format("hh:mm:ss"),
            rows: 0,
            max_rows,
        })
    }

    /// Errors once one more data row would exceed the sheet's row limit.
    fn check_limit(&self) -> Result<(), String> {
        if self.rows >= self.max_rows {
            return Err("XLSX supports at most 1,048,575 data rows".into());
        }
        Ok(())
    }

    fn write_cell(&mut self, row: u32, col: u16, value: &DataValue) -> Result<(), String> {
        let xerr = |e: rust_xlsxwriter::XlsxError| format!("Could not write XLSX: {e}");
        let sheet = &mut self.sheet;
        match value {
            DataValue::Null => Ok(()),
            DataValue::Integer(v) if v.unsigned_abs() <= MAX_EXACT_INTEGER as u64 => sheet
                .write_number(row, col, *v as f64)
                .map(|_| ())
                .map_err(xerr),
            DataValue::Integer(v) => sheet
                .write_string(row, col, v.to_string())
                .map(|_| ())
                .map_err(xerr),
            DataValue::Real(v) if v.is_finite() => {
                sheet.write_number(row, col, *v).map(|_| ()).map_err(xerr)
            }
            DataValue::Real(v) => sheet
                .write_string(row, col, real_text(*v))
                .map(|_| ())
                .map_err(xerr),
            DataValue::Decimal(s) => match exact_number(s) {
                Some(n) => sheet.write_number(row, col, n).map(|_| ()).map_err(xerr),
                None => sheet
                    .write_string(row, col, truncate_chars(s))
                    .map(|_| ())
                    .map_err(xerr),
            },
            DataValue::Boolean(v) => sheet.write_boolean(row, col, *v).map(|_| ()).map_err(xerr),
            DataValue::Date(s) => match parse_date(s) {
                Some(d) => sheet
                    .write_datetime_with_format(row, col, &d, &self.date_format)
                    .map(|_| ())
                    .map_err(xerr),
                None => sheet
                    .write_string(row, col, truncate_chars(s))
                    .map(|_| ())
                    .map_err(xerr),
            },
            DataValue::Timestamp(s) => match parse_timestamp(s) {
                Some(d) => sheet
                    .write_datetime_with_format(row, col, &d, &self.timestamp_format)
                    .map(|_| ())
                    .map_err(xerr),
                None => sheet
                    .write_string(row, col, truncate_chars(s))
                    .map(|_| ())
                    .map_err(xerr),
            },
            DataValue::Time(s) => match parse_time(s) {
                Some(d) => sheet
                    .write_datetime_with_format(row, col, &d, &self.time_format)
                    .map(|_| ())
                    .map_err(xerr),
                None => sheet
                    .write_string(row, col, truncate_chars(s))
                    .map(|_| ())
                    .map_err(xerr),
            },
            DataValue::Text(s) | DataValue::Blob(s) => sheet
                .write_string(row, col, truncate_chars(s))
                .map(|_| ())
                .map_err(xerr),
        }
    }
}

/// Cuts `s` to Excel's cell limit on a character boundary.
fn truncate_chars(s: &str) -> &str {
    match s.char_indices().nth(XLSX_MAX_CELL_CHARS) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// A decimal as an f64 when its digits survive the conversion (at most 15
/// significant digits, no exponent).
fn exact_number(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() || t.contains(['e', 'E']) {
        return None;
    }
    let unsigned = t.strip_prefix(['-', '+']).unwrap_or(t);
    let (int_part, frac_part) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if !int_part
        .chars()
        .chain(frac_part.chars())
        .all(|c| c.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{int_part}{}", frac_part.trim_end_matches('0'));
    if digits.trim_start_matches('0').len() > 15 {
        return None;
    }
    t.parse::<f64>().ok().filter(|n| n.is_finite())
}

fn excel_date(date: NaiveDate) -> Option<ExcelDateTime> {
    ExcelDateTime::from_ymd(
        u16::try_from(date.year()).ok()?,
        date.month() as u8,
        date.day() as u8,
    )
    .ok()
}

fn excel_datetime(dt: NaiveDateTime) -> Option<ExcelDateTime> {
    excel_date(dt.date())?
        .and_hms_milli(
            dt.hour() as u16,
            dt.minute() as u8,
            dt.second() as u8,
            (dt.nanosecond() / 1_000_000).min(999) as u16,
        )
        .ok()
}

fn parse_date(s: &str) -> Option<ExcelDateTime> {
    excel_date(NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok()?)
}

fn parse_time(s: &str) -> Option<ExcelDateTime> {
    let t = NaiveTime::parse_from_str(s.trim(), "%H:%M:%S%.f").ok()?;
    ExcelDateTime::from_hms_milli(
        t.hour() as u16,
        t.minute() as u8,
        t.second() as u8,
        (t.nanosecond() / 1_000_000).min(999) as u16,
    )
    .ok()
}

fn parse_timestamp(s: &str) -> Option<ExcelDateTime> {
    let t = s.trim().replacen(' ', "T", 1);
    // An explicit offset is normalized to UTC; without one the value is kept.
    if let Ok(dt) = DateTime::parse_from_rfc3339(&t) {
        return excel_datetime(dt.naive_utc());
    }
    if let Ok(dt) = DateTime::parse_from_str(&t, "%Y-%m-%dT%H:%M:%S%.f%#z") {
        return excel_datetime(dt.naive_utc());
    }
    excel_datetime(NaiveDateTime::parse_from_str(&t, "%Y-%m-%dT%H:%M:%S%.f").ok()?)
}

impl RowSink for XlsxSink {
    fn row(&mut self, values: &[DataValue]) -> Result<(), String> {
        self.check_limit()?;
        self.rows += 1;
        let row = u32::try_from(self.rows).map_err(|_| "Too many rows".to_string())?;
        for (i, value) in values.iter().enumerate() {
            let col =
                u16::try_from(i).map_err(|_| "XLSX supports at most 16,384 columns".to_string())?;
            self.write_cell(row, col, value)?;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<(), String> {
        let sheet = std::mem::replace(&mut self.sheet, Worksheet::new());
        self.workbook.push_worksheet(sheet);
        self.workbook
            .save(&self.path)
            .map_err(|e| format!("Could not write XLSX: {e}"))
    }
}
