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
