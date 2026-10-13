//! Port of tests/unit/forms-state.test.ts.

use super::{
    FormScope, compute, condition, default_record, disabled_columns, enabled_controls,
    expression_problem, has_errors, validate_control, validate_form, visible_controls,
};
use crate::design::{
    Binding, Control, ControlKind, ControlParent, Form, FormRule, FormSource, SourceKind,
    Validation,
};
use crate::forms::schema::{new_control, new_form};
use serde_json::{Map, Value, json};

fn scope(record: Value) -> FormScope {
    FormScope::new(
        record.as_object().cloned().unwrap_or_default(),
        Map::new(),
        json!({"role": null}).as_object().cloned().unwrap(),
    )
}

fn orders() -> Form {
    new_form(
        "Orders",
        Some(FormSource {
            kind: SourceKind::Table,
            table: Some("orders".into()),
            ..Default::default()
        }),
    )
}

fn bound(mut control: Control, column: &str) -> Control {
    control.binding = Some(Binding {
        table: None,
        column: column.into(),
    });
    control
}

struct Fixture {
    f: Form,
    name: Control,
    qty: Control,
    section: Control,
    vat: Control,
}

fn fixture() -> Fixture {
    let mut f = orders();
    let mut name = bound(new_control(ControlKind::Text, &f, None), "name");
    name.label = "Name".into();
    name.validation = Validation {
        required: true,
        ..Default::default()
    };
    f.controls.push(name.clone());
    let mut qty = bound(new_control(ControlKind::Number, &f, None), "qty");
    qty.label = "Quantity".into();
    qty.validation = Validation {
        required: false,
        min: Some(1.0),
        max: Some(10.0),
        expression: Some("value % 2 = 0".into()),
        message: Some("Use an even quantity.".into()),
        ..Default::default()
    };
    f.controls.push(qty.clone());
    let mut section = new_control(ControlKind::Section, &f, None);
    section.visible_when = Some("record.kind = 'company'".into());
    f.controls.push(section.clone());
    let parent = ControlParent {
        id: section.id.clone(),
        tab: None,
    };
    let mut vat = bound(new_control(ControlKind::Text, &f, Some(&parent)), "vat");
    vat.label = "VAT".into();
    vat.validation = Validation {
        required: true,
        ..Default::default()
    };
    f.controls.push(vat.clone());
    f.rules.push(FormRule {
        id: "r".into(),
        expression: "record.qty is null or record.qty < 8".into(),
        message: "Too many.".into(),
    });
    Fixture {
        f,
        name,
        qty,
        section,
        vat,
    }
}

fn keys(errors: &[(String, String)]) -> Vec<&str> {
    errors.iter().map(|(id, _)| id.as_str()).collect()
}

#[test]
fn validates_required_range_and_expression_rules_with_custom_messages() {
    let Fixture { qty, name, .. } = fixture();
    let s = scope(json!({}));
    let check = |c: &Control, v: Value| validate_control(c, &v, &s);
    assert_eq!(check(&name, json!("  ")).as_deref(), Some("Name is required."));
    assert_eq!(check(&name, json!("Acme")), None);
    assert_eq!(check(&qty, json!(0)).as_deref(), Some("Use an even quantity."));
    assert_eq!(check(&qty, json!(3)).as_deref(), Some("Use an even quantity."));
    assert_eq!(check(&qty, json!(4)), None);
    let mut max5 = qty.clone();
    max5.validation = Validation {
        max: Some(5.0),
        ..Default::default()
    };
    assert_eq!(check(&max5, json!(6)).as_deref(), Some("Quantity must be at most 5."));
    let mut pattern = name.clone();
    pattern.validation = Validation {
        pattern: Some("[A-Z]+".into()),
        ..Default::default()
    };
    assert_eq!(
        check(&pattern, json!("abc")).as_deref(),
        Some("Name is not in the expected format.")
    );
}

#[test]
fn skips_hidden_containers_and_checks_form_rules() {
    let Fixture {
        f,
        name,
        vat,
        section,
        ..
    } = fixture();
    let person = scope(json!({"kind": "person", "qty": 8}));
    assert!(!visible_controls(&f, &person).contains(&section.id));
    assert!(!visible_controls(&f, &person).contains(&vat.id));
    let errors = validate_form(&f, &person);
    assert_eq!(keys(&errors.fields), [name.id.as_str()]);
    assert_eq!(errors.form, ["Too many."]);
    let company = validate_form(&f, &scope(json!({"kind": "company", "name": "Acme", "qty": 2})));
    assert_eq!(keys(&company.fields), [vat.id.as_str()]);
    assert!(!has_errors(&validate_form(
        &f,
        &scope(json!({"kind": "company", "name": "A", "qty": 2, "vat": "X"}))
    )));
}

#[test]
fn evaluates_conditions_computed_values_and_defaults_safely() {
    let empty = scope(json!({}));
    assert!(condition(None, empty.scope(), true));
    assert!(condition(Some("record.a > 1"), scope(json!({"a": 2})).scope(), true));
    assert!(!condition(Some("record.a +"), empty.scope(), true));
    assert_eq!(
        compute(
            Some("record.qty * record.price"),
            scope(json!({"qty": 3, "price": 2.5})).scope()
        )
        .value,
        json!(7.5)
    );
    assert!(compute(Some("'a' * 2"), empty.scope()).error.is_some());
    let mut f = new_form("X", None);
    let mut status = bound(new_control(ControlKind::Text, &f, None), "status");
    status.default_value = Some("'open'".into());
    f.controls.push(status);
    let mut label = bound(new_control(ControlKind::Text, &f, None), "label");
    label.default_value = Some("upper(record.status)".into());
    f.controls.push(label);
    let record = default_record(&f, &Map::new(), &Map::new());
    assert_eq!(Value::Object(record), json!({"status": "open", "label": "OPEN"}));
}

#[test]
fn reports_expression_problems_for_the_designer() {
    let qty = ["qty".to_string()];
    assert_eq!(expression_problem(Some("record.qty > 1"), &qty, None), "");
    assert!(expression_problem(Some("record.nope > 1"), &qty, None).contains("Unknown field"));
    assert!(expression_problem(Some("foo(1)"), &[], None).contains("Unknown function"));
    assert_eq!(expression_problem(Some(""), &[], None), "");
}

#[test]
fn passes_a_containers_disabled_state_to_everything_inside_it() {
    let mut f = orders();
    let mut tabs = new_control(ControlKind::Tabs, &f, None);
    tabs.enabled_when = Some("record.open".into());
    f.controls.push(tabs.clone());
    let page = ControlParent {
        id: tabs.id.clone(),
        tab: Some(tabs.tabs[0].id.clone()),
    };
    let section = new_control(ControlKind::Section, &f, Some(&page));
    f.controls.push(section.clone());
    let in_section = ControlParent {
        id: section.id.clone(),
        tab: None,
    };
    let inner = bound(new_control(ControlKind::Text, &f, Some(&in_section)), "name");
    let button = new_control(ControlKind::Button, &f, Some(&in_section));
    let list = new_control(ControlKind::RelatedList, &f, Some(&page));
    let mut own = new_control(ControlKind::Number, &f, None);
    own.enabled_when = Some("record.qty > 1".into());
    let outside = new_control(ControlKind::Text, &f, None);
    f.controls.extend([
        inner.clone(),
        button.clone(),
        list.clone(),
        own.clone(),
        outside.clone(),
    ]);
    let nested: Vec<&String> = [&tabs, &section, &inner, &button, &list]
        .iter()
        .map(|c| &c.id)
        .collect();
    let closed = enabled_controls(&f, &scope(json!({"open": false, "qty": 5})));
    assert!(nested.iter().all(|id| !closed.contains(*id)));
    assert!(closed.contains(&own.id));
    assert!(closed.contains(&outside.id));
    let open = enabled_controls(&f, &scope(json!({"open": true, "qty": 0})));
    assert!(nested.iter().all(|id| open.contains(*id)));
    assert!(!open.contains(&own.id));
    assert!(visible_controls(&f, &scope(json!({"open": false}))).contains(&inner.id));
}

#[test]
fn does_not_validate_controls_the_user_cannot_change() {
    let mut f = orders();
    let mut section = new_control(ControlKind::Section, &f, None);
    section.enabled_when = Some("record.open".into());
    let parent = ControlParent {
        id: section.id.clone(),
        tab: None,
    };
    let mut name = bound(new_control(ControlKind::Text, &f, Some(&parent)), "name");
    name.validation.required = true;
    f.controls.extend([section, name.clone()]);
    assert!(!has_errors(&validate_form(&f, &scope(json!({"open": false})))));
    assert_eq!(
        keys(&validate_form(&f, &scope(json!({"open": true}))).fields),
        [name.id.as_str()]
    );
}

#[test]
fn finds_columns_whose_only_controls_are_disabled() {
    let mut f = orders();
    let a = bound(new_control(ControlKind::Boolean, &f, None), "a");
    let mut b = bound(new_control(ControlKind::Text, &f, None), "b");
    b.enabled_when = Some("not record.a".into());
    let mut section = new_control(ControlKind::Section, &f, None);
    section.enabled_when = Some("not record.a".into());
    let parent = ControlParent {
        id: section.id.clone(),
        tab: None,
    };
    let c = bound(new_control(ControlKind::Text, &f, Some(&parent)), "c");
    let d_inside = bound(new_control(ControlKind::Text, &f, Some(&parent)), "d");
    let d_outside = bound(new_control(ControlKind::Text, &f, None), "d");
    f.controls.extend([a, b, section, c, d_inside, d_outside]);
    let mut disabled: Vec<String> = disabled_columns(&f, &scope(json!({"a": true})))
        .into_iter()
        .collect();
    disabled.sort();
    assert_eq!(disabled, ["b", "c"]);
    assert!(disabled_columns(&f, &scope(json!({"a": false}))).is_empty());
}

#[test]
fn ignores_enabled_when_on_static_kinds() {
    let mut f = orders();
    let mut label = new_control(ControlKind::Label, &f, None);
    label.enabled_when = Some("false".into());
    f.controls.push(label.clone());
    assert!(enabled_controls(&f, &scope(json!({}))).contains(&label.id));
}
