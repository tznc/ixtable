//! Field settings: serde shape, validation, and following column renames and drops.
use super::*;
use crate::archive::DocumentConfig;
use crate::data::AlterTable;

fn field(column: &str, format: Option<&str>) -> FieldSettings {
    FieldSettings {
        id: format!("f-{column}"),
        column: column.into(),
        format: format.map(str::to_string),
        ..Default::default()
    }
}

#[test]
fn entity_fields_round_trip_and_stay_out_of_older_json() {
    let plain: EntitySettings =
        serde_json::from_str(r#"{"id":"e","table":"t","concurrency":"optimistic"}"#).unwrap();
    assert!(plain.fields.is_empty());
    assert!(!serde_json::to_string(&plain).unwrap().contains("fields"));
    let json = r#"{"id":"e","table":"t","concurrency":"optimistic","actionId":null,"fields":[{"id":"f","column":"phone","inputMask":"(000) 000-0000"},{"id":"g","column":"tags","format":"multiSelect","options":["a","b"]}]}"#;
    let parsed: EntitySettings = serde_json::from_str(json).unwrap();
    assert_eq!(
        parsed.fields[0].input_mask.as_deref(),
        Some("(000) 000-0000")
    );
    assert_eq!(parsed.fields[1].options, vec!["a", "b"]);
    let back: EntitySettings =
        serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
    assert_eq!(back, parsed);
}

#[test]
fn field_settings_follow_column_renames_and_drops() {
    let mut fields = vec![
        field("notes", Some("richText")),
        field("files", Some("attachment")),
    ];
    follow_columns(
        &mut fields,
        &[
            AlterTable::RenameColumn {
                column: "notes".into(),
                new_name: "details".into(),
            },
            AlterTable::DropColumn {
                column: "files".into(),
            },
        ],
    );
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].column, "details");
}

#[test]
fn validation_flags_duplicate_unknown_and_empty_multi_select_fields() {
    let mut config = DocumentConfig::default();
    config.entities.push(EntitySettings {
        id: "e".into(),
        table: "t".into(),
        fields: vec![
            field("a", Some("richText")),
            field("a", None),
            field("b", Some("hologram")),
            field("c", Some("multiSelect")),
        ],
        ..Default::default()
    });
    let messages: Vec<String> = validate(&config).into_iter().map(|i| i.message).collect();
    assert!(messages
        .iter()
        .any(|m| m == "t.a has more than one field setting"));
    assert!(messages
        .iter()
        .any(|m| m == "t.b uses an unknown field format hologram"));
    assert!(messages
        .iter()
        .any(|m| m == "t.c is a multi-select field with no choices"));
}
