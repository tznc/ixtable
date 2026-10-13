//! Port of tests/unit/import-mapping.test.ts.

use super::mapping::{
    default_columns, default_mapping, default_primary_key, existing_table_target, identifier,
    name_from_path, new_table_target, ImportOptions,
};
use crate::data::{DataValue, LogicalType, TableSchema};
use crate::import::{
    parse::{ParsedFile, SourceColumn},
    write::{FieldMapping, ImportTarget, NewColumn},
};
use crate::ui::schema::testkit::{column, schema};

fn preview(names: &[&str]) -> ParsedFile {
    ParsedFile {
        format: crate::data::files::FileFormat::Csv,
        columns: names
            .iter()
            .map(|n| SourceColumn {
                name: n.to_string(),
                logical_type: LogicalType::Text,
            })
            .collect(),
        rows: Vec::<Vec<DataValue>>::new(),
        total_rows: 0,
        sheets: vec![],
    }
}

#[test]
fn turns_file_and_column_names_into_identifiers() {
    assert_eq!(identifier("Team Members!", "imported"), "team_members");
    assert_eq!(identifier("2024 sales", "imported"), "_2024_sales");
    assert_eq!(identifier("***", "imported"), "imported");
    assert_eq!(name_from_path("C:\\data\\Q1 Orders.csv"), "q1_orders");
    assert_eq!(name_from_path("/tmp/sales.v2.parquet"), "sales_v2");
}

#[test]
fn drafts_unique_new_table_columns_and_keys_an_id_column() {
    let mut drafts = default_columns(&preview(&["ID", "Name", "name", ""]));
    let names: Vec<&str> = drafts.iter().map(|d| d.name.as_str()).collect();
    assert_eq!(names, ["id", "name", "name_2", "column_4"]);
    assert_eq!(default_primary_key(&drafts), "id");
    drafts[0].include = false;
    assert_eq!(default_primary_key(&drafts), "");
    let target = new_table_target("t", &drafts, "").unwrap();
    assert_eq!(
        target,
        ImportTarget::NewTable {
            table: "t".into(),
            primary_key: None,
            columns: vec![
                NewColumn { source: "Name".into(), name: "name".into(), logical_type: LogicalType::Text },
                NewColumn { source: "name".into(), name: "name_2".into(), logical_type: LogicalType::Text },
                NewColumn { source: "".into(), name: "column_4".into(), logical_type: LogicalType::Text },
            ],
        }
    );
}

#[test]
fn maps_file_columns_to_fields_with_the_same_name_and_skips_the_rest() {
    let mut label = column("label", "text");
    label.generated = true;
    let table: TableSchema = schema(
        "members",
        vec![column("full_name", "text"), column("Age", "integer"), label],
        &[],
    );
    let mapping = default_mapping(&preview(&["Full Name", "age", "Label", "Other"]), Some(&table));
    let expected: Vec<(String, String)> = [
        ("Full Name", "full_name"),
        ("age", "Age"),
        ("Label", ""),
        ("Other", ""),
    ]
    .iter()
    .map(|(a, b)| (a.to_string(), b.to_string()))
    .collect();
    assert_eq!(mapping, expected);
    assert_eq!(
        existing_table_target("members", &mapping),
        ImportTarget::ExistingTable {
            table: "members".into(),
            mapping: vec![
                FieldMapping { source: "Full Name".into(), field: "full_name".into() },
                FieldMapping { source: "age".into(), field: "Age".into() },
            ],
        }
    );
}

#[test]
fn sends_empty_delimiter_and_sheet_as_detect_and_first() {
    let options = ImportOptions { header: false, delimiter: String::new(), sheet: String::new() }.clean();
    assert!(!options.header);
    assert_eq!(options.delimiter, None);
    assert_eq!(options.sheet, None);
}
