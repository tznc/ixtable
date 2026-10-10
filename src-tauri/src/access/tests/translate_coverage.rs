//! Wider Access SQL coverage: built-in functions, `Format` patterns and
//! domain functions with criteria built from the row, each run in DuckDB.
use super::translate::Fixture;
use crate::access::translate::ast::parse_statement;
use crate::access::translate::builtins::{number_spec, strftime_pattern};
use crate::access::translate::sql::{Dialect, SqlWriter};

fn db() -> duckdb::Connection {
    let db = duckdb::Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE orders(\"ID\" INTEGER PRIMARY KEY, \"Customer\" INTEGER, \"Order Date\" TIMESTAMP, \"Amount\" DOUBLE, \"Paid\" BOOLEAN, \"Status\" VARCHAR);
         CREATE TABLE customers(\"ID\" INTEGER PRIMARY KEY, \"Company\" VARCHAR, \"Logo\" BLOB);
         INSERT INTO customers VALUES (1, 'Contoso', NULL), (2, 'O''Brien & Co', NULL);
         INSERT INTO orders VALUES (1, 1, TIMESTAMP '2024-03-05 14:07:09', 1234.5, false, 'open'),
           (2, 2, TIMESTAMP '2024-11-20 08:00:00', 0.25, true, 'closed'),
           (3, 2, TIMESTAMP '2024-12-01 09:30:00', 40, false, 'open');",
    )
    .unwrap();
    db
}

fn translate(access: &str) -> String {
    let st = parse_statement(access).unwrap_or_else(|e| panic!("{access}: {e}"));
    let mut w = SqlWriter::new(Dialect::DuckDb, &Fixture);
    w.statement(&st).unwrap_or_else(|e| panic!("{access}: {e}"))
}

fn untranslated(access: &str) -> String {
    let st = parse_statement(access).unwrap();
    SqlWriter::new(Dialect::DuckDb, &Fixture)
        .statement(&st)
        .unwrap_err()
}

/// The value of an expression for order 1, as text.
fn value(db: &duckdb::Connection, expr: &str) -> Option<String> {
    let sql = translate(&format!("SELECT {expr} AS V FROM Orders WHERE ID = 1"));
    db.query_row(
        &format!("SELECT CAST(\"V\" AS VARCHAR) FROM ({sql})"),
        [],
        |r| r.get(0),
    )
    .unwrap_or_else(|e| panic!("{expr} → {sql}: {e}"))
}

fn column(db: &duckdb::Connection, access: &str) -> Vec<Option<String>> {
    let sql = translate(access);
    let mut st = db
        .prepare(&format!("SELECT CAST(\"V\" AS VARCHAR) FROM ({sql}) ORDER BY \"ID\""))
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
    st.query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn text_number_and_date_builtins_run_in_duckdb() {
    let db = db();
    let cases = [
        ("StrConv(\"hello big world\", 3)", "Hello Big World"),
        ("StrConv(Status, 1)", "OPEN"),
        ("StrConv(\"ABC\", 2)", "abc"),
        ("MonthName(3)", "March"),
        ("MonthName(Month([Order Date]), True)", "Mar"),
        ("WeekdayName(1)", "Sunday"),
        ("WeekdayName(Weekday([Order Date]), True)", "Tue"),
        ("FormatCurrency(Amount)", "$1,234.50"),
        ("FormatNumber(Amount, 0)", "1,235"),
        ("FormatPercent(0.256, 1)", "25.6%"),
        ("FormatDateTime([Order Date], 2)", "3/5/2024"),
        ("FormatDateTime([Order Date], 4)", "14:07"),
        ("Partition(Amount, 0, 5000, 1000)", "1000:1999"),
        ("Partition(15, 0, 99, 10)", " 10: 19"),
        ("Partition(150, 0, 99, 10)", "100:   "),
        ("Hex(255)", "FF"),
        ("Oct(8)", "10"),
        ("InStrRev(\"a-b-c\", \"-\")", "4"),
        ("InStrRev(\"abc\", \"x\")", "0"),
        ("StrReverse(\"abc\")", "cba"),
        ("StrComp(\"abc\", \"ABC\")", "0"),
        ("StrComp(\"abc\", \"abd\")", "-1"),
        ("StrComp(\"b\", \"a\", 0)", "1"),
        ("TimeSerial(14, 30, 5)", "14:30:05"),
        ("Round(Atn(1) * 4, 4)", "3.1416"),
        ("Eval(\"1 + 2\")", "3"),
    ];
    for (expr, expected) in cases {
        assert_eq!(value(&db, expr).as_deref(), Some(expected), "{expr}");
    }
    let r: f64 = value(&db, "Rnd()").unwrap().parse().unwrap();
    assert!((0.0..1.0).contains(&r));
    assert!(untranslated("SELECT StrConv(Status, 64) FROM Orders").contains("StrConv"));
}

#[test]
fn format_patterns_run_in_duckdb() {
    let db = db();
    let cases = [
        ("Format([Order Date], \"yyyy-mm-dd\")", "2024-03-05"),
        ("Format([Order Date], \"mmm d, yyyy\")", "Mar 5, 2024"),
        ("Format([Order Date], \"dddd\")", "Tuesday"),
        ("Format([Order Date], \"hh:nn:ss\")", "14:07:09"),
        ("Format([Order Date], \"h:mm AM/PM\")", "2:07 PM"),
        ("Format([Order Date], \"mmmm yyyy\")", "March 2024"),
        ("Format([Order Date], \"\\Q\\t\\r \"\"x\"\" yy\")", "Qtr x 24"),
        ("Format([Order Date], \"Long Time\")", "2:07:09 PM"),
        ("Format(Amount, \"#,##0.00\")", "1,234.50"),
        ("Format(Amount, \"$#,##0\")", "$1,235"),
        ("Format(Amount, \"0.0\")", "1234.5"),
        ("Format(ID, \"0000\")", "0001"),
        ("Format(0.125, \"0.0%\")", "12.5%"),
        ("Format(0.5, \"#.00\")", ".50"),
        ("Format(ID, \"\"\"No. \"\"0\")", "No. 1"),
        ("Format(Paid, \"Yes/No\")", "No"),
        ("Format(Status, \">\")", "OPEN"),
        ("Format(Amount, \"Standard\")", "1,234.50"),
        ("Format(Amount)", "1234.5"),
    ];
    for (expr, expected) in cases {
        assert_eq!(value(&db, expr).as_deref(), Some(expected), "{expr}");
    }
    assert_eq!(strftime_pattern("m/d/yy h:nn").unwrap(), "%-m/%-d/%y %-H:%M");
    assert_eq!(strftime_pattern("hh:mm").unwrap(), "%H:%M");
    assert_eq!(strftime_pattern("mm:ss").unwrap(), "%M:%S");
    assert_eq!(number_spec("$#,##0.00").decimals, 2);
    assert!(untranslated("SELECT Format([Order Date], \"q\\/yyyy\") FROM Orders").contains("quarter"));
}

#[test]
fn domain_functions_with_criteria_from_the_row_are_correlated() {
    let db = db();
    assert_eq!(
        column(&db, "SELECT ID, DLookUp(\"Company\", \"Customers\", \"ID=\" & [Customer]) AS V FROM Orders"),
        [Some("Contoso".into()), Some("O'Brien & Co".into()), Some("O'Brien & Co".into())]
    );
    // Quotes around the value, and the domain is the query's own table.
    assert_eq!(
        column(&db, "SELECT ID, DCount(\"*\", \"Orders\", \"Status='\" & [Status] & \"'\") AS V FROM Orders"),
        [Some("2".into()), Some("1".into()), Some("2".into())]
    );
    assert_eq!(
        column(&db, "SELECT O.ID, DSum(\"Amount\", \"Orders\", \"Customer = \" & O.Customer & \" AND ID <> \" & O.ID) AS V FROM Orders AS O"),
        [None, Some("40.0".into()), Some("0.25".into())]
    );
    assert_eq!(
        column(&db, "SELECT ID, DMax(\"[Order Date]\", \"Orders\", \"[Order Date] < #\" & [Order Date] & \"#\") AS V FROM Orders"),
        [None, Some("2024-03-05 14:07:09".into()), Some("2024-11-20 08:00:00".into())]
    );
    // A text value with a quote in it stays one value.
    assert_eq!(
        column(&db, "SELECT ID, DCount(\"*\", \"Customers\", \"Company = '\" & DLookup(\"Company\", \"Customers\", \"ID=\" & [Customer]) & \"'\") AS V FROM Orders"),
        [Some("1".into()), Some("1".into()), Some("1".into())]
    );
    // DCount with a literal criteria (was translated as a VBA function before).
    assert_eq!(value(&db, "DCount(\"ID\", \"Orders\", \"Paid = False\")").as_deref(), Some("2"));
}
