//! Port of tests/unit/field-inputs.test.tsx: one field hosted on its own and fed values the
//! way a form feeds them. The rich-text box edits markup instead of a content-editable box, so
//! those cases are adapted. Imports are explicit.

use super::{FieldProps, FieldView, field_id};
use crate::data::DataValue;
use crate::design::{Binding, Control, ControlKind, KeyPair, Relationship, SelectOption};
use crate::ui::testing::Harness;
use gpui_kit::{AppContext as _, Entity, Render, TestAppContext};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

type Changes = Rc<RefCell<Vec<Value>>>;

struct Host {
    field: Entity<FieldView>,
}

impl Render for Host {
    fn render(&mut self, _: &mut gpui_kit::Window, _: &mut gpui_kit::Context<Self>) -> impl gpui_kit::IntoElement {
        self.field.clone()
    }
}

fn control(kind: ControlKind) -> Control {
    Control { id: "f".into(), kind, label: "Field".into(), ..Default::default() }
}

fn props(control: &Control, value: Value, read_only: bool, changes: &Changes) -> FieldProps {
    let log = changes.clone();
    FieldProps {
        label: control.label.clone().into(),
        hide_label: false,
        control: control.clone(),
        value,
        read_only,
        error: None,
        tone: None,
        filter_scope: None,
        key_values: Default::default(),
        table: Some("docs".into()),
        prefix: "".into(),
        window_label: "field-test".into(),
        on_change: Rc::new(move |value, _| log.borrow_mut().push(value)),
        on_keys: Rc::new(|_, _| {}),
        on_blur: Rc::new(|_| {}),
        on_enter: Rc::new(|_| {}),
    }
}

fn host<'a>(cx: &'a mut TestAppContext, first: FieldProps) -> (Harness<'a>, Entity<FieldView>) {
    host_in(Harness::start(cx), first)
}

fn host_in<'a>(mut h: Harness<'a>, first: FieldProps) -> (Harness<'a>, Entity<FieldView>) {
    let (window, host) = h.cx.update(|cx| {
        gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, |window, cx| {
            let field = cx.new(|cx| {
                let mut field = FieldView::new();
                field.set_props(first, window, cx);
                field
            });
            cx.new(|_| Host { field })
        })
        .expect("open the field window")
    });
    h.window = window;
    h.settle();
    let field = h.cx.read(|cx| host.read(cx).field.clone());
    (h, field)
}

// Feeds a new value, as the form does after a change.
fn feed(h: &mut Harness, field: &Entity<FieldView>, next: FieldProps) {
    h.update(|window, cx| field.update(cx, |f, cx| f.set_props(next, window, cx)));
    h.settle();
}

fn id(control: &Control) -> gpui_kit::SharedString {
    field_id("", &control.label, &control.id)
}

#[gpui_kit::test]
fn formats_masked_entry_as_it_is_typed_and_stores_the_typed_characters(cx: &mut TestAppContext) {
    let changes: Changes = Default::default();
    let mut mask = control(ControlKind::Text);
    mask.input_mask = Some("(000) 000-0000".into());
    let (mut h, field) = host(cx, props(&mask, Value::Null, false, &changes));
    h.click(id(&mask));
    for ch in "5551234567".chars() {
        h.input(&ch.to_string());
        let stored = changes.borrow().last().cloned().unwrap_or(Value::Null);
        feed(&mut h, &field, props(&mask, stored, false, &changes));
    }
    let shown = h.cx.read(|cx| field.read(cx).text.as_ref().expect("a text input").read(cx).value().to_string());
    assert_eq!(shown, "(555) 123-4567");
    assert_eq!(changes.borrow().last(), Some(&json!("5551234567")));
    // The input's placeholder is the mask drawn with blanks.
    let template = crate::forms::mask::mask_template(&crate::forms::mask::parse_mask("(000) 000-0000"));
    assert_eq!(template, "(___) ___-____");
}

// Focuses the text area directly: its wrapper is not the focus target.
fn focus_area(h: &mut Harness, field: &Entity<FieldView>) -> Entity<gpui_kit::component::input::TextareaState> {
    let area = h.cx.read(|cx| field.read(cx).area.clone().expect("a text area"));
    h.update(|window, cx| {
        use gpui_kit::Focusable as _;
        let handle = area.read(cx).focus_handle(cx);
        handle.focus(window, cx);
    });
    h.settle();
    area
}

#[gpui_kit::test]
fn emits_html_from_the_rich_text_box(cx: &mut TestAppContext) {
    let changes: Changes = Default::default();
    let rich = control(ControlKind::RichText);
    let (mut h, field) = host(cx, props(&rich, Value::Null, false, &changes));
    focus_area(&mut h, &field);
    h.input("Hello *you*");
    assert_eq!(changes.borrow().last(), Some(&json!("<p>Hello <i>you</i></p>")));
}

#[gpui_kit::test]
fn formats_the_selection_from_the_rich_text_toolbar(cx: &mut TestAppContext) {
    let changes: Changes = Default::default();
    let rich = control(ControlKind::RichText);
    let base = id(&rich);
    let (mut h, field) = host(cx, props(&rich, Value::Null, false, &changes));
    let area = focus_area(&mut h, &field);
    h.input("Hello");
    feed(&mut h, &field, props(&rich, json!("<p>Hello</p>"), false, &changes));
    h.update(|_, cx| area.update(cx, |s, cx| s.set_selected_range(0..5, cx)));
    h.click(format!("{base}:format:bold"));
    assert_eq!(changes.borrow().last(), Some(&json!("<p><b>Hello</b></p>")));
    feed(&mut h, &field, props(&rich, json!("<p><b>Hello</b></p>"), false, &changes));
    // Typing goes on in the box after a toolbar click.
    h.press("end");
    h.input("!");
    let shown = h.cx.read(|cx| area.read(cx).value().to_string());
    assert_eq!(shown, "**Hello**!");
    h.press("backspace");
    h.click(format!("{base}:format:numbered-list"));
    assert_eq!(changes.borrow().last(), Some(&json!("<ol><li><b>Hello</b></li></ol>")));
    feed(&mut h, &field, props(&rich, json!("<ol><li><b>Hello</b></li></ol>"), false, &changes));
    // The preview renders the stored HTML.
    assert_eq!(h.label(format!("{base}:preview")), "Hello");
    let shown = h.cx.read(|cx| area.read(cx).value().to_string());
    assert_eq!(shown, "1. **Hello**");
}

#[gpui_kit::test]
fn shows_stored_rich_text_as_markup_while_editing(cx: &mut TestAppContext) {
    let changes: Changes = Default::default();
    let rich = control(ControlKind::RichText);
    let value = json!("<p>Plan</p><ul><li><u>one</u></li><li>two</li></ul>");
    let (mut h, field) = host(cx, props(&rich, value, false, &changes));
    let shown = h.cx.read(|cx| field.read(cx).area.as_ref().expect("a text area").read(cx).value().to_string());
    assert_eq!(shown, "Plan\n- ++one++\n- two");
    assert!(changes.borrow().is_empty(), "showing a value must not change it");
}

#[gpui_kit::test]
fn shows_read_only_rich_text_formatted_without_scripts(cx: &mut TestAppContext) {
    let changes: Changes = Default::default();
    let rich = control(ControlKind::RichText);
    let value = json!("<b>Bold</b><script>alert(1)</script>");
    let (mut h, _field) = host(cx, props(&rich, value, true, &changes));
    assert_eq!(h.label(id(&rich)), "Bold");
    assert!(!h.exists(format!("{}:format:bold", id(&rich))), "read-only rich text has no toolbar");
}

#[gpui_kit::test]
fn toggles_choices_into_a_json_array_and_keeps_unknown_stored_choices(cx: &mut TestAppContext) {
    let changes: Changes = Default::default();
    let mut multi = control(ControlKind::MultiSelect);
    multi.options = ["red", "blue"].iter().map(|v| SelectOption { value: v.to_string(), label: String::new() }).collect();
    let base = id(&multi);
    let option = |name: &str| format!("{base}:option:{name}");
    let (mut h, field) = host(cx, props(&multi, json!("[\"green\"]"), false, &changes));
    // The stored choice the list no longer has is shown and removable.
    assert!(h.exists(option("green")));
    h.click(option("blue"));
    assert_eq!(changes.borrow().last(), Some(&json!("[\"blue\",\"green\"]")));
    feed(&mut h, &field, props(&multi, json!("[\"blue\",\"green\"]"), false, &changes));
    h.click(option("green"));
    feed(&mut h, &field, props(&multi, json!("[\"blue\"]"), false, &changes));
    h.click(option("blue"));
    assert_eq!(changes.borrow().last(), Some(&Value::Null));
}

fn two_files() -> Value {
    json!(json!([
        {"id": "a", "name": "a.txt", "mime": "text/plain", "size": 10, "sha256": "x"},
        {"id": "b", "name": "b.txt", "mime": "text/plain", "size": 2000, "sha256": "y"},
    ])
    .to_string())
}

fn attachment_control() -> Control {
    let mut attachment = control(ControlKind::Attachment);
    attachment.binding = Some(Binding { table: None, column: "files".into() });
    attachment
}

#[gpui_kit::test]
fn lists_attachment_files_with_their_sizes(cx: &mut TestAppContext) {
    let changes: Changes = Default::default();
    let attachment = attachment_control();
    let base = id(&attachment);
    let (mut h, _field) = host(cx, props(&attachment, two_files(), false, &changes));
    assert_eq!(h.label(format!("{base}:file:a-txt")), "a.txt (10 B)");
    assert_eq!(h.label(format!("{base}:file:b-txt")), "b.txt (2 KB)");
}

#[gpui_kit::test]
fn removes_one_file_from_an_attachment_field(cx: &mut TestAppContext) {
    let changes: Changes = Default::default();
    let attachment = attachment_control();
    let base = id(&attachment);
    let (mut h, field) = host(cx, props(&attachment, two_files(), false, &changes));
    h.click(format!("{base}:remove:a-txt"));
    let left = changes.borrow().last().cloned().expect("a change");
    let files = crate::forms::fields::parse_attachments(&left);
    assert_eq!(files.iter().map(|f| f.id.as_str()).collect::<Vec<_>>(), ["b"]);
    feed(&mut h, &field, props(&attachment, left, false, &changes));
    assert!(!h.exists(format!("{base}:file:a-txt")));
    // Removing the last file clears the value.
    h.click(format!("{base}:remove:b-txt"));
    assert_eq!(changes.borrow().last(), Some(&Value::Null));
}

#[gpui_kit::test]
fn read_only_attachments_can_be_saved_but_not_changed(cx: &mut TestAppContext) {
    let changes: Changes = Default::default();
    let attachment = attachment_control();
    let base = id(&attachment);
    let (mut h, _field) = host(cx, props(&attachment, two_files(), true, &changes));
    assert!(h.exists(format!("{base}:save:a-txt")));
    assert!(!h.exists(format!("{base}:remove:a-txt")));
    assert!(!h.exists(format!("{base}:add")));
}

#[gpui_kit::test]
fn adds_a_picked_file_and_saves_a_copy(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("docs", &[("id", "INTEGER", 1), ("files", "TEXT", 0)]);
    let mut config = h.config();
    config.entities.push(
        serde_json::from_value(json!({
            "id": "e1", "table": "docs", "concurrency": "optimistic",
            "fields": [{ "id": "f1", "column": "files", "format": "attachment" }],
        }))
        .unwrap(),
    );
    crate::update_document_config(h.label.clone(), config).expect("update config");
    let source = h.temp_path("notes.txt");
    std::fs::write(&source, "hello").unwrap();
    h.dialogs.queue_open(Some(source));
    let changes: Changes = Default::default();
    let attachment = attachment_control();
    let base = id(&attachment);
    let mut first = props(&attachment, two_files(), false, &changes);
    first.window_label = h.label.clone().into();
    let (mut h, field) = host_in(h, first);
    h.click(format!("{base}:add"));
    wait(&mut h);
    let value = changes.borrow().last().cloned().expect("the new file list");
    let files = crate::forms::fields::parse_attachments(&value);
    assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["a.txt", "b.txt", "notes.txt"]);
    let mut next = props(&attachment, value, false, &changes);
    next.window_label = h.label.clone().into();
    feed(&mut h, &field, next);
    assert_eq!(h.label(format!("{base}:file:notes-txt")), "notes.txt (5 B)");
    let copy = h.temp_path("copy.txt");
    h.dialogs.queue_save(Some(copy.clone()));
    h.click(format!("{base}:save:notes-txt"));
    wait(&mut h);
    assert_eq!(std::fs::read_to_string(copy).unwrap(), "hello");
}

// ---- relationship choices (tests/unit/relationship-filter.test.tsx) ------------------------

fn wait(h: &mut Harness) {
    for _ in 0..40 {
        h.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(5));
        h.settle();
    }
}

fn scope(form: Value) -> serde_json::Map<String, Value> {
    let Value::Object(form) = form else { panic!("a form scope is an object") };
    let mut scope = serde_json::Map::new();
    scope.insert("parent".into(), json!({}));
    scope.insert("form".into(), Value::Object(form));
    scope.insert("app".into(), json!({}));
    scope.insert("params".into(), json!({}));
    scope
}

fn relationship(filter: &str) -> Control {
    let mut c = control(ControlKind::Relationship);
    c.label = "Customer".into();
    c.relationship = Some(Relationship {
        table: "customers".into(),
        value_column: "id".into(),
        display_column: "name".into(),
        keys: vec![],
        filter: Some(filter.into()),
    });
    c
}

fn customers(h: &mut Harness) {
    h.create_table("customers", &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("region", "TEXT", 0)]);
    for (id, name, region) in [(1, "Acme", "East"), (2, "Globex", "West")] {
        h.insert_row(
            "customers",
            &[
                ("id", DataValue::Integer(id)),
                ("name", DataValue::Text(name.into())),
                ("region", DataValue::Text(region.into())),
            ],
        );
    }
    h.database_changed();
}

fn with_scope(control: &Control, form: Value, label: &str) -> FieldProps {
    let mut p = props(control, Value::Null, false, &Default::default());
    p.filter_scope = Some(scope(form));
    p.window_label = label.to_string().into();
    p
}

#[gpui_kit::test]
fn reloads_choices_only_when_a_value_the_filter_reads_changes(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    customers(&mut h);
    let rel = relationship("record.region = form.region");
    let label = h.label.clone();
    let (mut h, field) = host_in(h, with_scope(&rel, json!({"region": "East", "note": "a"}), &label));
    let base = id(&rel);
    h.click(base.clone());
    wait(&mut h);
    assert!(h.exists(format!("{base}:option:acme")));
    assert!(!h.exists(format!("{base}:option:globex")));
    let loads = h.cx.read(|cx| field.read(cx).seq);
    for note in ["ab", "abc"] {
        feed(&mut h, &field, with_scope(&rel, json!({"region": "East", "note": note}), &label));
    }
    wait(&mut h);
    assert_eq!(h.cx.read(|cx| field.read(cx).seq), loads, "an unrelated value must not reload the list");
    feed(&mut h, &field, with_scope(&rel, json!({"region": "West", "note": "abc"}), &label));
    wait(&mut h);
    assert!(h.exists(format!("{base}:option:globex")));
    assert!(!h.exists(format!("{base}:option:acme")));
    assert!(h.cx.read(|cx| field.read(cx).seq) > loads);
}

#[gpui_kit::test]
fn shows_a_filter_that_fails_to_parse_instead_of_an_empty_list(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    customers(&mut h);
    let rel = relationship("record.region =");
    let label = h.label.clone();
    let (mut h, _field) = host_in(h, with_scope(&rel, json!({}), &label));
    let base = id(&rel);
    h.click(base.clone());
    wait(&mut h);
    assert!(h.label(format!("{base}:choice-error")).starts_with("Choice filter: "));
    // Only the empty choice is left.
    assert!(h.exists(format!("{base}:option:none")));
    assert!(!h.exists(format!("{base}:option:acme")));
}

#[gpui_kit::test]
fn writes_every_key_column_of_a_filtered_multi_column_choice(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("thresholds", &[("product_id", "INTEGER", 1), ("location_id", "INTEGER", 2), ("label", "TEXT", 0)]);
    for (product, location, name) in [(1, 1, "Depot"), (1, 2, "Store")] {
        h.insert_row(
            "thresholds",
            &[
                ("product_id", DataValue::Integer(product)),
                ("location_id", DataValue::Integer(location)),
                ("label", DataValue::Text(name.into())),
            ],
        );
    }
    h.database_changed();
    let mut threshold = control(ControlKind::Relationship);
    threshold.label = "Threshold".into();
    threshold.relationship = Some(Relationship {
        table: "thresholds".into(),
        value_column: "product_id".into(),
        display_column: "label".into(),
        keys: vec![
            KeyPair { column: "product_id".into(), target: "product_id".into() },
            KeyPair { column: "loc".into(), target: "location_id".into() },
        ],
        filter: Some("record.label <> 'Depot'".into()),
    });
    let keys: Rc<RefCell<Vec<Vec<(String, Value)>>>> = Default::default();
    let label = h.label.clone();
    let mut first = with_scope(&threshold, json!({}), &label);
    let log = keys.clone();
    first.on_keys = Rc::new(move |values, _| log.borrow_mut().push(values));
    let (mut h, _field) = host_in(h, first);
    let base = id(&threshold);
    h.click(base.clone());
    wait(&mut h);
    assert!(!h.exists(format!("{base}:option:depot")));
    h.click(format!("{base}:option:store"));
    wait(&mut h);
    let last = keys.borrow().last().cloned();
    assert_eq!(last, Some(vec![("product_id".to_string(), json!(1)), ("loc".to_string(), json!(2))]));
}
