use super::writers::XlsxSink;
use super::*;
use crate::data::{DataValue, LogicalType};
use calamine::{open_workbook, Data, Reader, Xlsx};
use std::path::{Path, PathBuf};

fn temp_path(ext: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ixtable-export-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(format!("out.{ext}"))
}

fn col(name: &str, logical: Option<LogicalType>) -> ExportColumn {
    ExportColumn {
        name: name.into(),
        logical,
    }
}

fn text(s: &str) -> DataValue {
    DataValue::Text(s.into())
}

fn write(
    format: ExportFormat,
    ext: &str,
    cols: &[ExportColumn],
    rows: &[Vec<DataValue>],
) -> PathBuf {
    let path = temp_path(ext);
    let mut sink = open_sink(format, &path, cols).unwrap();
    for row in rows {
        sink.row(row).unwrap();
    }
    sink.finish().unwrap();
    path
}

#[test]
fn csv_bytes_quote_and_escape() {
    let cols = [col("id", None), col("note, \"x\"", None), col("v", None)];
    let rows = vec![
        vec![DataValue::Integer(1), text("a,b"), DataValue::Null],
        vec![
            DataValue::Integer(-2),
            text("say \"hi\"\nthere"),
            DataValue::Boolean(true),
        ],
        vec![
            DataValue::Real(1.5),
            text(" pad"),
            DataValue::Boolean(false),
        ],
        vec![
            DataValue::Real(f64::NAN),
            text("é ü"),
            DataValue::Real(f64::INFINITY),
        ],
        vec![
            DataValue::Real(f64::NEG_INFINITY),
            text("cr\rx"),
            DataValue::Decimal("12.50".into()),
        ],
        vec![
            DataValue::Date("2024-01-02".into()),
            DataValue::Timestamp("2024-01-02T03:04:05".into()),
            DataValue::Blob("AQID".into()),
        ],
    ];
    let path = write(ExportFormat::Csv, "csv", &cols, &rows);
    let bytes = std::fs::read(&path).unwrap();
    let expected = "\u{feff}id,\"note, \"\"x\"\"\",v\r\n\
1,\"a,b\",\r\n\
-2,\"say \"\"hi\"\"\nthere\",true\r\n\
1.5,\" pad\",false\r\n\
NaN,é ü,inf\r\n\
-inf,\"cr\rx\",12.50\r\n\
2024-01-02,2024-01-02T03:04:05,AQID\r\n";
    assert_eq!(String::from_utf8(bytes).unwrap(), expected);
}

#[test]
fn csv_empty_export_has_bom_and_header() {
    let path = write(
        ExportFormat::Csv,
        "csv",
        &[col("a", None), col("b", None)],
        &[],
    );
    assert_eq!(std::fs::read(path).unwrap(), b"\xEF\xBB\xBFa,b\r\n");
}

#[test]
fn json_mixed_types_and_duplicate_keys() {
    let cols = [
        col("a", None),
        col("a", None),
        col("a", None),
        col("b", None),
        col("c", None),
        col("d", None),
        col("e", None),
        col("f", None),
    ];
    let rows = vec![
        vec![
            DataValue::Null,
            DataValue::Integer(7),
            DataValue::Real(2.5),
            DataValue::Decimal("1.10".into()),
            DataValue::Boolean(true),
            text("q\"\n"),
            DataValue::Date("2024-01-02".into()),
            DataValue::Blob("AQID".into()),
        ],
        vec![
            DataValue::Null,
            DataValue::Null,
            DataValue::Null,
            DataValue::Null,
            DataValue::Null,
            DataValue::Null,
            DataValue::Null,
            DataValue::Null,
        ],
    ];
    let path = write(ExportFormat::Json, "json", &cols, &rows);
    let out = std::fs::read_to_string(path).unwrap();
    let expected = "[\n\
{\"a\":null,\"a_2\":7,\"a_3\":2.5,\"b\":\"1.10\",\"c\":true,\"d\":\"q\\\"\\n\",\"e\":\"2024-01-02\",\"f\":\"AQID\"},\n\
{\"a\":null,\"a_2\":null,\"a_3\":null,\"b\":null,\"c\":null,\"d\":null,\"e\":null,\"f\":null}\n\
]\n";
    assert_eq!(out, expected);
    serde_json::from_str::<serde_json::Value>(&out).unwrap();
}

#[test]
fn json_embeds_json_columns_and_nulls_non_finite_reals() {
    let cols = [
        col("doc", Some(LogicalType::Json)),
        col("x", None),
        col("txt", None),
    ];
    let rows = vec![
        vec![
            text("{\"k\": [1, 2]}"),
            DataValue::Real(f64::NAN),
            text("{\"k\":1}"),
        ],
        vec![text("not json"), DataValue::Real(f64::INFINITY), text("")],
    ];
    let path = write(ExportFormat::Json, "json", &cols, &rows);
    let out = std::fs::read_to_string(path).unwrap();
    let expected = "[\n\
{\"doc\":{\"k\": [1, 2]},\"x\":null,\"txt\":\"{\\\"k\\\":1}\"},\n\
{\"doc\":\"not json\",\"x\":null,\"txt\":\"\"}\n\
]\n";
    assert_eq!(out, expected);
    serde_json::from_str::<serde_json::Value>(&out).unwrap();
}

#[test]
fn json_duplicate_suffix_avoids_existing_names() {
    let cols = [col("a", None), col("a", None), col("a_2", None)];
    let path = write(
        ExportFormat::Json,
        "json",
        &cols,
        &[vec![DataValue::Integer(1); 3]],
    );
    let out = std::fs::read_to_string(path).unwrap();
    assert_eq!(out, "[\n{\"a\":1,\"a_3\":1,\"a_2\":1}\n]\n");
}

#[test]
fn json_empty_export_is_empty_array() {
    let path = write(ExportFormat::Json, "json", &[col("a", None)], &[]);
    let out = std::fs::read_to_string(path).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&out).unwrap(),
        serde_json::json!([])
    );
}

fn read_xlsx(path: &Path) -> calamine::Range<Data> {
    let mut wb: Xlsx<_> = open_workbook(path).unwrap();
    assert_eq!(wb.sheet_names(), vec!["Export".to_string()]);
    wb.worksheet_range("Export").unwrap()
}

#[test]
fn xlsx_round_trip_through_calamine() {
    let cols = [
        col("name", None),
        col("n", None),
        col("r", None),
        col("dec", None),
        col("flag", None),
        col("d", None),
        col("ts", None),
        col("t", None),
    ];
    let rows = vec![
        vec![
            text("alpha"),
            DataValue::Integer(42),
            DataValue::Real(1.25),
            DataValue::Decimal("10.50".into()),
            DataValue::Boolean(true),
            DataValue::Date("2024-01-02".into()),
            DataValue::Timestamp("2024-01-02T03:04:05".into()),
            DataValue::Time("13:14:15".into()),
        ],
        vec![
            text("beta"),
            DataValue::Integer(i64::MAX),
            DataValue::Real(f64::NAN),
            DataValue::Decimal("12345678901234567890.123".into()),
            DataValue::Boolean(false),
            DataValue::Date("not a date".into()),
            DataValue::Timestamp("2024-01-02 03:04:05.5+02:00".into()),
            DataValue::Time("??".into()),
        ],
        vec![DataValue::Null; 8],
    ];
    let path = write(ExportFormat::Xlsx, "xlsx", &cols, &rows);
    let range = read_xlsx(&path);
    let cell = |r: u32, c: u32| range.get_value((r, c)).cloned().unwrap_or(Data::Empty);
    let header: Vec<String> = (0..8).map(|c| cell(0, c).to_string()).collect();
    assert_eq!(header, ["name", "n", "r", "dec", "flag", "d", "ts", "t"]);
    assert_eq!(cell(1, 0), Data::String("alpha".into()));
    assert_eq!(cell(1, 1), Data::Float(42.0));
    assert_eq!(cell(1, 2), Data::Float(1.25));
    assert_eq!(cell(1, 3), Data::Float(10.5));
    assert_eq!(cell(1, 4), Data::Bool(true));
    let serial = |r: u32, c: u32| match cell(r, c) {
        Data::DateTime(d) => d.as_f64(),
        other => panic!("not a datetime: {other:?}"),
    };
    let secs = |h: f64, m: f64, s: f64| (h * 3600.0 + m * 60.0 + s) / 86_400.0;
    assert!((serial(1, 5) - 45_293.0).abs() < 1e-9);
    assert!((serial(1, 6) - (45_293.0 + secs(3.0, 4.0, 5.0))).abs() < 1e-6);
    assert!((serial(1, 7) - secs(13.0, 14.0, 15.0)).abs() < 1e-6);
    assert_eq!(cell(2, 1), Data::String(i64::MAX.to_string()));
    assert_eq!(cell(2, 2), Data::String("NaN".into()));
    assert_eq!(cell(2, 3), Data::String("12345678901234567890.123".into()));
    assert_eq!(cell(2, 4), Data::Bool(false));
    assert_eq!(cell(2, 5), Data::String("not a date".into()));
    // The +02:00 offset is normalized to UTC (03:04:05.5 -> 01:04:05.5).
    assert!((serial(2, 6) - (45_293.0 + secs(1.0, 4.0, 5.5))).abs() < 1e-6);
    assert_eq!(cell(2, 7), Data::String("??".into()));
}

#[test]
fn xlsx_truncates_long_strings_on_char_boundary() {
    let long: String = "é".repeat(40_000);
    let path = write(
        ExportFormat::Xlsx,
        "xlsx",
        &[col("s", None)],
        &[vec![DataValue::Text(long)]],
    );
    let range = read_xlsx(&path);
    match range.get_value((1, 0)).unwrap() {
        Data::String(s) => assert_eq!(s.chars().count(), 32_767),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn xlsx_row_limit_errors() {
    let path = temp_path("xlsx");
    let mut sink = XlsxSink::open(&path, &[col("a", None)], 2).unwrap();
    sink.row(&[DataValue::Integer(1)]).unwrap();
    sink.row(&[DataValue::Integer(2)]).unwrap();
    let err = sink.row(&[DataValue::Integer(3)]).unwrap_err();
    assert_eq!(err, "XLSX supports at most 1,048,575 data rows");
    assert_eq!(XLSX_MAX_ROWS, 1_048_575);
}

#[test]
fn xlsx_empty_export_has_header_only() {
    let path = write(
        ExportFormat::Xlsx,
        "xlsx",
        &[col("a", None), col("b", None)],
        &[],
    );
    let range = read_xlsx(&path);
    assert_eq!(range.height(), 1);
    assert_eq!(range.get_value((0, 1)), Some(&Data::String("b".into())));
}
