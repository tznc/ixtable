use crate::access::jet::JetFile;
use crate::access::model::{AccessFile, Value};
use std::fmt::Write;

/// The normalized dump `scripts/access/Norm.java` prints with Jackcess, so the
/// two readers can be diffed over a corpus.
pub fn dump(path: &std::path::Path) -> Result<String, String> {
    let mut f = JetFile::open(std::fs::File::open(path).map_err(|e| e.to_string())?)?;
    let mut out = String::new();
    let mut tables: Vec<_> = f
        .db()
        .tables
        .iter()
        .map(|t| {
            (
                t.name.clone(),
                t.columns.iter().map(|c| c.name.clone()).collect::<Vec<_>>(),
            )
        })
        .collect();
    tables.sort_by_key(|(n, _)| n.to_lowercase());
    for (name, cols) in tables {
        let _ = writeln!(
            out,
            "TABLE {name} | {}",
            cols.iter().map(|c| format!("{c},")).collect::<String>()
        );
        let mut rows = vec![];
        f.rows(&name, &mut |values| {
            rows.push(
                values
                    .iter()
                    .map(|v| format!("{} | ", norm(v)))
                    .collect::<String>(),
            );
            Ok(())
        })?;
        rows.sort();
        for r in rows {
            let _ = writeln!(out, "{r}");
        }
    }
    for q in &f.db().queries {
        let _ = writeln!(out, "QUERY {} {:?}\n{}", q.name, q.kind, q.sql);
    }
    for r in &f.db().relationships {
        let _ = writeln!(
            out,
            "REL {} {}{:?} -> {}{:?} flags={}",
            r.name, r.table, r.columns, r.ref_table, r.ref_columns, r.flags
        );
    }
    Ok(out)
}

fn sum(b: &[u8]) -> u32 {
    b.iter().fold(0u32, |s, x| s.wrapping_add(*x as u32))
}

fn trim_decimal(s: &str) -> String {
    if !s.contains('.') {
        return if s == "-0" { "0".into() } else { s.to_string() };
    }
    let t = s.trim_end_matches('0').trim_end_matches('.');
    if t == "-0" || t.is_empty() {
        "0".into()
    } else {
        t.to_string()
    }
}

pub fn norm(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Double(d) => trim_decimal(&format!("{d:.4}")),
        Value::Decimal(s) => trim_decimal(s),
        Value::Text(t) | Value::Guid(t) => t
            .replace('\\', "\\\\")
            .replace('\r', "\\r")
            .replace('\n', "\\n"),
        Value::DateTime(d) => d.split('.').next().unwrap_or_default().to_string(),
        Value::Binary(b) => format!("bin:{}:{}", b.len(), sum(b)),
        Value::Attachments(a) => format!(
            "[{}]",
            a.iter()
                .map(|x| format!("{}:{}:{};", x.file_name, x.data.len(), sum(&x.data)))
                .collect::<String>()
        ),
        Value::Multi(m) => format!(
            "[{}]",
            m.iter()
                .map(|x| format!("{};", norm(x)))
                .collect::<String>()
        ),
    }
}

#[test]
fn dump_corpus_for_comparison() {
    let (Some(input), Some(output)) = (
        std::env::var_os("IXTABLE_ACCESS_COMPARE_IN"),
        std::env::var_os("IXTABLE_ACCESS_COMPARE_OUT"),
    ) else {
        return;
    };
    let output = std::path::PathBuf::from(output);
    std::fs::create_dir_all(&output).unwrap();
    for line in std::fs::read_to_string(input)
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
    {
        let path = std::path::Path::new(line);
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let text = dump(path).unwrap_or_else(|e| format!("ERROR {e}\n"));
        std::fs::write(output.join(format!("{name}.rust.txt")), text).unwrap();
    }
}

/// A copy of a fixture with header bytes XORed: on masked bytes, XOR changes the plain value.
fn with_header_bits(name: &str, at: usize, bits: &[u8]) -> std::path::PathBuf {
    let path = super::fixtures::binary(name);
    let mut bytes = std::fs::read(&path).unwrap();
    for (i, b) in bits.iter().enumerate() {
        bytes[at + i] ^= b;
    }
    std::fs::write(&path, bytes).unwrap();
    path
}

fn open_error(path: &std::path::Path) -> String {
    JetFile::open(std::fs::File::open(path).unwrap())
        .err()
        .unwrap()
}

#[test]
fn password_protected_and_encrypted_databases_are_refused() {
    for (name, encrypted) in [
        ("orders.mdb", "is encrypted; decrypt it in Access"),
        ("orders.accdb", "has a password; remove it in Access"),
    ] {
        let password = with_header_bits(name, 0x42, b"p\0w\0");
        assert!(open_error(&password).contains("has a password"), "{name}");
        let key = with_header_bits(name, 0x3E, &[0x12, 0x34, 0x56, 0x78]);
        assert!(open_error(&key).contains(encrypted), "{name}");
        let err = crate::access::inventory(&key).err().unwrap();
        assert!(err.contains(encrypted), "{name}: {err}");
    }
}

#[test]
fn compressed_text_numbers_and_dates_decode() {
    use crate::access::jet::row::{currency, numeric, ole_date};
    use crate::access::jet::text::decode_text;
    // FF FE, "Ab" compressed, switch, "€" as UTF-16, switch back, "c".
    assert_eq!(
        decode_text(
            &[0xFF, 0xFE, b'A', b'b', 0, 0xAC, 0x20, 0, b'c'],
            false,
            1252
        ),
        "Ab€c"
    );
    assert_eq!(decode_text(&[b'h', 0, b'i', 0], false, 1252), "hi");
    assert_eq!(decode_text(&[b'C', b'a', b'f', 0xE9], true, 1252), "Café");
    assert_eq!(currency(&1_205_000i64.to_le_bytes()), "120.5");
    assert_eq!(currency(&(-5i64).to_le_bytes()), "-0.0005");
    let mut n = [0u8; 17];
    n[13..17].copy_from_slice(&12345u32.to_le_bytes());
    assert_eq!(numeric(&n, 2), "123.45");
    n[0] = 0x80;
    assert_eq!(numeric(&n, 4), "-1.2345");
    assert_eq!(ole_date(0.0), "1899-12-30T00:00:00");
    assert_eq!(ole_date(45000.5), "2023-03-15T12:00:00");
    assert_eq!(ole_date(-1.25), "1899-12-29T06:00:00");
}

#[test]
fn attachment_payloads_unwrap() {
    use crate::access::blob::{split_attachment, unwrap_attachment};
    let mut content = vec![20, 0, 0, 0, 1, 0, 0, 0, 4, 0, 0, 0];
    content.extend("png\0".encode_utf16().flat_map(u16::to_le_bytes));
    content.extend(b"DATA");
    assert_eq!(
        split_attachment(&content),
        ("png".to_string(), b"DATA".to_vec())
    );
    let mut raw = vec![0, 0, 0, 0];
    raw.extend((content.len() as u32).to_le_bytes());
    raw.extend(&content);
    assert_eq!(unwrap_attachment(&raw).unwrap(), content);
    let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
    std::io::Write::write_all(&mut z, &content).unwrap();
    let mut deflated = vec![1, 0, 0, 0];
    deflated.extend((content.len() as u32).to_le_bytes());
    deflated.extend(z.finish().unwrap());
    assert_eq!(unwrap_attachment(&deflated).unwrap(), content);
}

fn check_orders_file(name: &str, format: crate::access::model::SourceFormat) {
    use crate::access::model::{ColType, QueryKind};
    let path = super::fixtures::binary(name);
    let mut f = JetFile::open(std::fs::File::open(&path).unwrap()).unwrap();
    let db = f.db().clone();
    assert_eq!(db.format, format);
    let names: Vec<&str> = db.tables.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["Customers", "Orders"]);
    let orders = db.table("Orders").unwrap();
    let ty = |n: &str| orders.column(n).unwrap().ty;
    assert_eq!(ty("Amount"), ColType::Currency);
    assert_eq!(ty("Order Date"), ColType::DateTime);
    assert_eq!(
        ty("Discount"),
        ColType::Numeric {
            precision: 5,
            scale: 2
        }
    );
    assert_eq!(ty("Paid"), ColType::Boolean);
    assert!(orders.column("ID").unwrap().auto_number);
    let customer = orders.column("Customer").unwrap();
    assert_eq!(
        customer.prop("RowSource"),
        Some("SELECT Customers.ID, Customers.Company FROM Customers ORDER BY Customers.Company;")
    );
    assert_eq!(customer.prop("ColumnWidths"), Some("0;1440"));
    assert_eq!(
        orders.column("Amount").unwrap().prop("ValidationRule"),
        Some(">=0")
    );
    assert_eq!(
        orders.column("Order Date").unwrap().prop("DefaultValue"),
        Some("=Date()")
    );
    assert_eq!(
        orders.prop("ValidationRule"),
        Some("[Shipped] Is Null Or [Shipped]>=[Order Date]")
    );
    assert_eq!(orders.row_count, Some(8));
    let customers = db.table("Customers").unwrap();
    assert!(customers.column("Company").unwrap().required());
    assert!(customers.column("Website").unwrap().hyperlink);
    assert!(customers
        .indexes
        .iter()
        .any(|i| i.primary && i.columns == [("ID".to_string(), true)]));
    assert!(customers
        .indexes
        .iter()
        .any(|i| i.name == "Company" && i.unique));
    let rel = &db.relationships[0];
    assert_eq!(
        (
            rel.name.as_str(),
            rel.table.as_str(),
            rel.ref_table.as_str()
        ),
        ("CustomersOrders", "Orders", "Customers")
    );
    assert_eq!(
        rel.flags & crate::access::model::rel_flags::CASCADE_DELETES,
        crate::access::model::rel_flags::CASCADE_DELETES
    );
    let q: Vec<(&str, QueryKind)> = db
        .queries
        .iter()
        .map(|q| (q.name.as_str(), q.kind))
        .collect();
    assert_eq!(
        q,
        [
            ("Customer Totals", QueryKind::Select),
            ("Big Orders", QueryKind::Select),
            ("Unpaid Orders", QueryKind::Select),
            ("Delete Old Orders", QueryKind::Delete)
        ]
    );
    assert_eq!(
        db.query("Big Orders").unwrap().parameters,
        [("Minimum amount".to_string(), "Currency".to_string())]
    );
    assert_eq!(
        db.query("Customer Totals").unwrap().sql,
        "SELECT Customers.Company, Sum(Orders.Amount) AS Total, Count(*) AS [Order Count]\nFROM Customers INNER JOIN Orders ON Customers.ID = Orders.Customer\nGROUP BY Customers.Company\nORDER BY Sum(Orders.Amount) DESC"
    );
    let text = dump(&path).unwrap();
    assert!(
        text.contains(
            "3 | Northwind Café | Online | Ünïcödé — ok | #https://northwind.example/# | false | "
        ),
        "{text}"
    );
    assert!(
        text.contains("3 | 2 | 2024-02-14T00:00:00 | 1999.99 | 40 | 0.15 | false | null | "),
        "{text}"
    );
    // The long memo spans LVAL pages.
    let mut memo = String::new();
    f.rows("Customers", &mut |r| {
        if let Value::Text(t) = &r[3] {
            if t.len() > memo.len() {
                memo = t.clone();
            }
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(
        memo.len(),
        (0..400)
            .map(|i| format!("Line {i}: a long memo that spans several pages. ").len())
            .sum::<usize>()
    );
}

#[test]
fn ace_fixture_reads() {
    check_orders_file("orders.accdb", crate::access::model::SourceFormat::Ace);
}

#[test]
fn jet4_fixture_reads() {
    check_orders_file("orders.mdb", crate::access::model::SourceFormat::Jet4);
}

#[test]
fn complex_columns_read_their_values() {
    let path = super::fixtures::binary("complex-data.accdb");
    let text = dump(&path).unwrap();
    assert!(
        text.contains("TABLE Table1 | id,memo-data,append-memo-data,multi-value-data,attach-data,"),
        "{text}"
    );
    assert!(text.contains("row2 | row2-memo | row2-memo | [value1;value4;] | [test_data.txt:38:3584;test_data2.txt:43:4051;] | "), "{text}");
    assert!(
        text.contains(
            "row3 | row3-memo | row3-memo-again | [value1;value2;value3;value4;] | [] | "
        ),
        "{text}"
    );
}

#[test]
fn non_access_files_are_refused() {
    let p = super::fixtures::temp("x.accdb");
    std::fs::write(&p, vec![0u8; 8192]).unwrap();
    let err = JetFile::open(std::fs::File::open(&p).unwrap())
        .err()
        .unwrap();
    assert!(err.contains("not an Access database"), "{err}");
}
