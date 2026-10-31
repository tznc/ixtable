//! Damaged and unusual files found by chaos testing the import.
use super::convert::{import, manager};
use super::fixtures::template_with;
use crate::access::convert::report::Status;

pub const OBJECTS: &str = "template/database/objects/";

fn edit_part(parts: &mut std::collections::BTreeMap<String, Vec<u8>>, part: &str, f: impl FnOnce(String) -> String) {
    let key = format!("{OBJECTS}{part}");
    let text = String::from_utf8(parts[&key].clone()).unwrap();
    parts.insert(key, f(text).into_bytes());
}

fn notes_of(report: &crate::access::convert::report::ImportReport, kind: &str, name: &str) -> Vec<String> {
    report
        .items
        .iter()
        .filter(|i| i.kind == kind && i.name == name)
        .flat_map(|i| i.notes.clone())
        .collect()
}

#[test]
fn values_that_do_not_fit_their_column_are_emptied_and_reported() {
    let path = template_with(|parts| {
        edit_part(parts, "sampleData/tableOrders.xml", |s| {
            s.replacen("2024-01-05T00:00:00", "31/02/99999T25:61", 1)
                .replacen("<Amount>75</Amount>", "<Amount>not a number</Amount>", 1)
        })
    });
    let (report, errs) = import(&path, "chaos-values");
    assert_eq!(errs, Vec::<String>::new());
    let m = manager();
    // Every row is still there and the table reads.
    let rows = m
        .read_query("chaos-values", "SELECT \"Order Date\", \"Amount\" FROM \"Orders\" ORDER BY \"ID\"")
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0][0], crate::data::DataValue::Null);
    assert_eq!(rows[1][1], crate::data::DataValue::Null);
    let notes = notes_of(&report, "table", "Orders").join("\n");
    assert!(notes.contains("Order Date: 1 value does not fit"), "{notes}");
    assert!(notes.contains("31/02/99999T25:61"), "{notes}");
    assert!(notes.contains("Amount: 1 value does not fit"), "{notes}");
    assert!(report.items.iter().any(|i| i.kind == "table" && i.name == "Orders" && i.status == Status::Partial));
    m.close("chaos-values", true).unwrap();
}

fn with_order_lookup(sql: &'static str) -> std::path::PathBuf {
    template_with(move |parts| {
        parts.insert(
            format!("{OBJECTS}queryGhost lookup.txt"),
            b"Operation =1\nOption =0\nBegin InputTables\n    Name =\"Ghost\"\nEnd\nBegin OutputColumns\n    Expression =\"Ghost.ID\"\n    Expression =\"Ghost.Name\"\nEnd\n".to_vec(),
        );
        edit_part(parts, "tableOrders.xsd", |s| {
            s.replace(
                "SELECT Customers.ID, Customers.Company FROM Customers ORDER BY Customers.Company;",
                sql,
            )
        })
    })
}

#[test]
fn a_lookup_view_with_a_spaced_name_keeps_the_tables_listed() {
    // A computed display column makes the lookup a view ("Orders lookup").
    let path = with_order_lookup("SELECT Customers.ID, [Company] &amp; &quot; (&quot; &amp; [Category] &amp; &quot;)&quot; AS Label FROM Customers;");
    let (_, errs) = import(&path, "chaos-view");
    assert_eq!(errs, Vec::<String>::new());
    let objects = manager().database_objects("chaos-view").expect("objects list");
    assert!(objects.iter().any(|o| o.name == "Orders"), "{objects:?}");
    manager().close("chaos-view", true).unwrap();
}

#[test]
fn a_lookup_over_a_missing_table_is_dropped_and_reported() {
    let path = with_order_lookup("Ghost lookup");
    let (report, errs) = import(&path, "chaos-ghost");
    assert_eq!(errs, Vec::<String>::new());
    let objects = manager().database_objects("chaos-ghost").expect("objects list");
    assert!(objects.iter().any(|o| o.name == "Orders"), "{objects:?}");
    assert!(!objects.iter().any(|o| o.object_type == "view"), "{objects:?}");
    let warnings = report.warnings.join("\n");
    assert!(warnings.contains("The lookup of Customer reads Ghost lookup"), "{warnings}");
    manager().close("chaos-ghost", true).unwrap();
}

#[test]
fn a_damaged_catalog_is_reported_before_the_import() {
    let path = super::fixtures::binary("damaged/flip050-orders.mdb");
    let inventory = crate::access::inventory(&path).unwrap();
    let warnings = inventory.warnings.join("\n");
    assert!(warnings.contains("The database catalog is damaged"), "{warnings}");
}

#[test]
fn a_table_that_cannot_be_read_is_a_not_converted_item() {
    let path = super::fixtures::binary("damaged/flip001-complex-data.accdb");
    let mut file = crate::access::open(&path).unwrap();
    let conversion = crate::access::convert::convert(
        file.as_mut(),
        "flip001-complex-data.accdb",
        &Default::default(),
        &mut |_, _| {},
    )
    .unwrap();
    let item = conversion
        .report
        .items
        .iter()
        .find(|i| i.kind == "table" && i.name == "Table1")
        .expect("an item for the unreadable table");
    assert_eq!(item.status, Status::Skipped);
    assert!(item.notes.join(" ").contains("could not be read"), "{:?}", item.notes);
}

/// A copy of orders.mdb whose Orders data pages claim no rows.
fn orders_with_lost_rows() -> std::path::PathBuf {
    let path = super::fixtures::binary("orders.mdb");
    let orders = crate::access::jet::JetFile::open(std::fs::File::open(&path).unwrap())
        .unwrap()
        .table_page("Orders")
        .unwrap();
    let mut bytes = std::fs::read(&path).unwrap();
    for page in bytes.chunks_mut(4096) {
        let owner = u32::from_le_bytes([page[4], page[5], page[6], page[7]]);
        if page[0] == 0x01 && owner == orders {
            page[12] = 0;
            page[13] = 0;
        }
    }
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn rows_the_reader_cannot_find_are_reported() {
    let path = orders_with_lost_rows();
    let mut file = crate::access::open(&path).unwrap();
    let conversion =
        crate::access::convert::convert(file.as_mut(), "orders.mdb", &Default::default(), &mut |_, _| {}).unwrap();
    let notes = conversion
        .report
        .items
        .iter()
        .filter(|i| i.kind == "table" && i.name == "Orders")
        .flat_map(|i| i.notes.clone())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(notes.contains("8 of 8 rows could not be read"), "{notes}");
}

fn select_query(input: &str) -> Vec<u8> {
    format!("Operation =1\nOption =0\nBegin InputTables\n    Name =\"{input}\"\nEnd\nBegin OutputColumns\n    Expression =\"{input}.*\"\nEnd\n").into_bytes()
}

#[test]
fn queries_whose_names_differ_only_by_case_are_both_kept() {
    let path = template_with(|parts| {
        parts.insert(format!("{OBJECTS}queryorders.txt"), select_query("Orders"));
        parts.insert(format!("{OBJECTS}queryOrders.txt"), select_query("Customers"));
    });
    let (report, errs) = import(&path, "chaos-case");
    assert_eq!(errs, Vec::<String>::new());
    let config = manager().config("chaos-case").unwrap();
    let names: Vec<&str> = config.saved_queries.iter().map(|q| q.name.as_str()).collect();
    assert!(names.contains(&"Orders") || names.contains(&"orders"), "{names:?}");
    assert!(names.iter().any(|n| n.eq_ignore_ascii_case("orders 2")), "{names:?}");
    let notes = report.items.iter().flat_map(|i| i.notes.clone()).collect::<Vec<_>>().join("\n");
    assert!(notes.contains("renamed"), "{notes}");
    manager().close("chaos-case", true).unwrap();
}

#[test]
fn unreadable_sample_data_skips_that_table_s_rows_only() {
    let path = template_with(|parts| {
        let mut garbage = vec![0xFF, 0xFE];
        garbage.extend((0..999u32).map(|i| (i * 7919 % 251) as u8));
        parts.insert(format!("{OBJECTS}sampleData/tableOrders.xml"), garbage);
    });
    let (report, errs) = import(&path, "chaos-garbage");
    assert_eq!(errs, Vec::<String>::new());
    let m = manager();
    assert_eq!(m.read_query("chaos-garbage", "SELECT * FROM \"Orders\"").unwrap().rows.len(), 0);
    assert_eq!(m.read_query("chaos-garbage", "SELECT * FROM \"Customers\"").unwrap().rows.len(), 3);
    let notes = notes_of(&report, "table", "Orders").join("\n");
    assert!(notes.contains("sample data could not be read"), "{notes}");
    m.close("chaos-garbage", true).unwrap();
}

#[test]
fn not_a_number_doubles_reach_the_type_check_instead_of_vanishing() {
    use crate::access::convert::staging::sql_value;
    use rusqlite::types::Value as Sql;
    assert_eq!(sql_value(&crate::access::model::Value::Double(f64::NAN), false, false), Sql::Text("NaN".into()));
    assert_eq!(sql_value(&crate::access::model::Value::Double(f64::INFINITY), false, false), Sql::Text("inf".into()));
}

#[test]
fn currency_beyond_fifteen_digits_is_reported_as_rounded() {
    let path = template_with(|parts| {
        edit_part(parts, "sampleData/tableOrders.xml", |s| {
            s.replacen("<Amount>75</Amount>", "<Amount>922337203685477.5807</Amount>", 1)
        })
    });
    let (report, _) = import(&path, "chaos-currency");
    let notes = notes_of(&report, "table", "Orders").join("\n");
    assert!(notes.contains("Amount: 1 value has more than 15 significant digits"), "{notes}");
    manager().close("chaos-currency", true).unwrap();
}

#[test]
fn text_keys_match_regardless_of_case_as_in_access() {
    let path = super::fixtures::binary("text-keys.accdb");
    let (report, errs) = import(&path, "chaos-text-keys");
    assert_eq!(errs, Vec::<String>::new());
    let notes = report.items.iter().flat_map(|i| i.notes.clone()).collect::<Vec<_>>().join("\n");
    assert!(!notes.contains("refer to missing"), "{notes}");
    let schema = manager().table_schema("chaos-text-keys", "Items").unwrap();
    assert_eq!(schema.foreign_keys.len(), 1, "{schema:?}");
    manager().close("chaos-text-keys", true).unwrap();
}

#[test]
fn a_template_without_tables_makes_a_valid_document() {
    let path = template_with(|parts| {
        parts.retain(|k, _| !k.contains("/objects/table") && !k.contains("sampleData") && !k.ends_with("relationships.xml"));
        parts.retain(|k, _| !k.contains("/objects/") || k.contains("/objects/module"));
    });
    let (_, errs) = import(&path, "chaos-empty");
    assert_eq!(errs, Vec::<String>::new());
    manager().close("chaos-empty", true).unwrap();
}

#[test]
fn a_macro_that_runs_itself_is_cut_with_a_note() {
    let path = template_with(|parts| {
        parts.insert(
            format!("{OBJECTS}macroLoop.txt"),
            b"Version =196611\nBegin\n    Action =\"RunMacro\"\n    Argument =\"Loop\"\nEnd\n".to_vec(),
        );
    });
    let (report, errs) = import(&path, "chaos-loop");
    assert_eq!(errs, Vec::<String>::new());
    let notes = notes_of(&report, "macro", "Loop").join("\n");
    assert!(notes.contains("runs itself"), "{notes}");
    manager().close("chaos-loop", true).unwrap();
}

#[test]
fn config_problems_left_after_the_import_become_report_warnings() {
    let (_, _) = import(&super::fixtures::template(), "chaos-validate");
    let mut config = manager().config("chaos-validate").unwrap();
    let mut broken = config.migrations[0].clone();
    broken.id = "broken".into();
    broken.up = String::new();
    config.migrations.push(broken);
    let warnings = crate::access::convert::validation_warnings(&config);
    assert!(warnings.iter().any(|w| w.starts_with("The new document has a problem to fix:") && w.contains("no up SQL")), "{warnings:?}");
    manager().close("chaos-validate", true).unwrap();
}

#[test]
fn a_table_renamed_for_a_reserved_name_says_so() {
    let path = template_with(|parts| {
        let xsd = String::from_utf8(parts[&format!("{OBJECTS}tableOrders.xsd")].clone()).unwrap();
        parts.insert(format!("{OBJECTS}tablesqlite_stats.xsd"), xsd.replace("Orders", "sqlite_stats").into_bytes());
    });
    let (report, errs) = import(&path, "chaos-reserved");
    assert_eq!(errs, Vec::<String>::new());
    let notes = notes_of(&report, "table", "sqlite_stats").join("\n");
    assert!(notes.contains("named \"Access sqlite_stats\""), "{notes}");
    manager().close("chaos-reserved", true).unwrap();
}

fn cancel_flag() -> std::sync::Arc<std::sync::atomic::AtomicBool> {
    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false))
}

#[test]
fn a_cancelled_import_stops_and_leaves_no_document() {
    use std::sync::atomic::Ordering;
    let path = super::fixtures::binary("orders.accdb");
    let cancel = cancel_flag();
    let flag = cancel.clone();
    // The user cancels as soon as the first table reports progress.
    let progress: Box<dyn FnMut(&str, u64) + Send> = Box::new(move |_, _| flag.store(true, Ordering::SeqCst));
    let options = crate::access::convert::Options { cancel: Some(cancel), ..Default::default() };
    let err = tauri::async_runtime::block_on(crate::access::import_access_file(
        "chaos-cancel".into(),
        path.to_string_lossy().into_owned(),
        Some(options),
        Some(progress),
    ))
    .unwrap_err();
    assert_eq!(err.code, "IMPORT_CANCELLED");
    assert!(crate::manager().unwrap().state("chaos-cancel").is_err());
}

#[test]
fn the_time_left_is_estimated_from_the_rows_copied_so_far() {
    use crate::access::convert::time_left;
    use std::time::Duration;
    // Too early to say.
    assert_eq!(time_left(100, 1_000_000, Duration::from_millis(500)), None);
    assert_eq!(time_left(0, 1_000_000, Duration::from_secs(5)), None);
    // 100k rows in 10 s: 900k rows take 90 s more.
    assert_eq!(time_left(100_000, 1_000_000, Duration::from_secs(10)).as_deref(), Some("about 2 minutes left"));
    assert_eq!(time_left(500_000, 1_000_000, Duration::from_secs(20)).as_deref(), Some("about 20 seconds left"));
    assert_eq!(time_left(990_000, 1_000_000, Duration::from_secs(60)).as_deref(), Some("a few seconds left"));
    assert_eq!(time_left(10_000, 1_000_000, Duration::from_secs(60)).as_deref(), Some("about 1 hour 39 minutes left"));
}
