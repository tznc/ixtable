use crate::access::text_format::{decode, parse, Item, PropValue};

#[test]
fn properties_blocks_continuations_and_escapes() {
    let text = "Version =20\nBegin Form\n    Caption =\"Say \\\"hi\\\"\\015\\012\"\n    Where =\"(A=1 \"\n        \"Or B=2)\"\n    PopUp = NotDefault\n    Begin\n        Begin Label\n            Name =\"L1\"\n        End\n    End\nEnd\n";
    let root = parse(text).unwrap();
    assert_eq!(root.int("Version"), Some(20));
    let form = root.block("Form").unwrap();
    assert_eq!(form.get("Caption"), Some("Say \"hi\"\r\n"));
    assert_eq!(form.get("Where"), Some("(A=1 Or B=2)"));
    assert_eq!(form.flag("PopUp"), Some(true));
    let label = form.blocks().next().unwrap().block("Label").unwrap();
    assert_eq!(label.get("name"), Some("L1"));
}

#[test]
fn binary_data_nested_macros_and_typed_properties() {
    let text = "GUID = Begin\n    0x0102 ,\n    0xff\nEnd\nOnClickEmMacro = Begin\n    Version =196611\n    Begin\n        Action =\"Beep\"\n    End\nEnd\ndbText \"Name\" =\"Orders.ID\"\ndbBoolean \"ReturnsRecords\" =\"-1\"\n";
    let root = parse(text).unwrap();
    assert_eq!(root.binary("GUID"), Some(&[1u8, 2, 0xff][..]));
    let m = root.prop_block("OnClickEmMacro").unwrap();
    assert_eq!(m.blocks().next().unwrap().get("Action"), Some("Beep"));
    assert_eq!(root.get("Name"), Some("Orders.ID"));
    let typed: Vec<_> = root
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Prop {
                ty: Some(t),
                key,
                value: PropValue::Str(v),
            } => Some((t.as_str(), key.as_str(), v.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(
        typed,
        vec![
            ("dbText", "Name", "Orders.ID"),
            ("dbBoolean", "ReturnsRecords", "-1")
        ]
    );
}

#[test]
fn code_behind_is_kept_and_errors_name_the_line() {
    let root = parse("Begin Form\nEnd\nCodeBehindForm\nPrivate Sub X()\nEnd Sub").unwrap();
    assert_eq!(root.code.as_deref(), Some("Private Sub X()\nEnd Sub"));
    let err = parse("Begin Form\n  Caption =\"x\"\n").unwrap_err();
    assert!(err.message.contains("missing End"), "{err}");
    assert_eq!(parse("End").unwrap_err().line, 1);
}

#[test]
fn decodes_utf16_utf8_and_ansi() {
    let mut utf16 = vec![0xFF, 0xFE];
    utf16.extend("Café".encode_utf16().flat_map(u16::to_le_bytes));
    assert_eq!(decode(&utf16), "Café");
    assert_eq!(decode("\u{feff}Café".as_bytes()), "Café");
    assert_eq!(decode(&[b'C', b'a', b'f', 0xE9]), "Café");
}
