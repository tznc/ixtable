//! The rich-text field's Markdown toolbar and images: picked and pasted images go to the form's
//! attachment field, and the preview loads attachment images from the record. Imports are explicit.

use super::richtext::RichImage;
use super::{FieldProps, FieldView, field_id};
use crate::design::{Control, ControlKind};
use crate::ui::testing::Harness;
use base64::{Engine, engine::general_purpose::STANDARD};
use gpui_kit::{AppContext as _, ClipboardItem, Entity, Image, ImageFormat, Render, TestAppContext};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

/// A 1×1 PNG.
const PIXEL: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

#[derive(Default)]
struct Log {
    changes: RefCell<Vec<Value>>,
    keys: RefCell<Vec<(String, Value)>>,
}

struct Host {
    field: Entity<FieldView>,
}

impl Render for Host {
    fn render(&mut self, _: &mut gpui_kit::Window, _: &mut gpui_kit::Context<Self>) -> impl gpui_kit::IntoElement {
        self.field.clone()
    }
}

fn rich() -> Control {
    Control { id: "notes".into(), kind: ControlKind::RichText, label: "Notes".into(), ..Default::default() }
}

fn props(value: Value, log: &Rc<Log>, window_label: &str, record: Value, attachments: bool) -> FieldProps {
    let (changes, keys) = (log.clone(), log.clone());
    let control = rich();
    FieldProps {
        label: control.label.clone().into(),
        control,
        value,
        read_only: false,
        error: None,
        tone: None,
        filter_scope: None,
        key_values: record.as_object().cloned().unwrap_or_default(),
        table: Some("docs".into()),
        attachment_column: attachments.then(|| "files".into()),
        prefix: "".into(),
        window_label: window_label.to_string().into(),
        on_change: Rc::new(move |value, _| changes.changes.borrow_mut().push(value)),
        on_keys: Rc::new(move |values, _| keys.keys.borrow_mut().extend(values)),
        on_blur: Rc::new(|_| {}),
        on_enter: Rc::new(|_| {}),
    }
}

fn host<'a>(mut h: Harness<'a>, first: FieldProps) -> (Harness<'a>, Entity<FieldView>) {
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

fn wait(h: &mut Harness) {
    for _ in 0..40 {
        h.advance(Duration::from_millis(20));
        std::thread::sleep(Duration::from_millis(5));
        h.settle();
    }
}

fn id() -> gpui_kit::SharedString {
    field_id("", "Notes", "notes")
}

/// A document with a `docs` table whose `files` column is an attachment field.
fn document(cx: &mut TestAppContext) -> Harness<'_> {
    let mut h = Harness::new_document(cx);
    h.create_table("docs", &[("id", "INTEGER", 1), ("notes", "TEXT", 0), ("files", "TEXT", 0)]);
    let mut config = h.config();
    config.entities.push(
        serde_json::from_value(json!({
            "id": "e1", "table": "docs", "concurrency": "optimistic",
            "fields": [
                { "id": "f1", "column": "files", "format": "attachment" },
                { "id": "f2", "column": "notes", "format": "richText" },
            ],
        }))
        .unwrap(),
    );
    crate::update_document_config(h.label.clone(), config).expect("update config");
    h
}

#[gpui_kit::test]
fn toolbar_writes_markdown_blocks(cx: &mut TestAppContext) {
    let log = Rc::new(Log::default());
    let h = Harness::start(cx);
    let (mut h, field) = host(h, props(Value::Null, &log, "field-test", json!({}), false));
    let area = focus_area(&mut h, &field);
    h.input("Title");
    h.click(format!("{}:format:heading", id()));
    assert_eq!(log.changes.borrow().last(), Some(&json!("<h2>Title</h2>")));
    h.update(|window, cx| area.update(cx, |s, cx| s.replace_all("todo".to_string(), window, cx)));
    h.settle();
    h.click(format!("{}:format:task-list", id()));
    assert_eq!(log.changes.borrow().last(), Some(&json!("<ul><li><input type=\"checkbox\" disabled>todo</li></ul>")));
    h.update(|window, cx| area.update(cx, |s, cx| s.replace_all(String::new(), window, cx)));
    h.settle();
    h.click(format!("{}:format:table", id()));
    let html = log.changes.borrow().last().cloned().expect("a table");
    assert!(html.as_str().is_some_and(|t| t.starts_with("<table><thead><tr><th>Column 1</th>")), "{html}");
    // Typing replaces the selected first heading.
    h.input("Item");
    let shown = h.cx.read(|cx| area.read(cx).value().to_string());
    assert!(shown.starts_with("| Item | Column 2 |"), "{shown}");
    // Without an attachment field there is no Image button.
    assert!(!h.exists(format!("{}:format:image", id())));
    assert!(h.exists(format!("{}:format:code-block", id())));
}

#[gpui_kit::test]
fn inserts_a_picked_image_as_an_attachment(cx: &mut TestAppContext) {
    let h = document(cx);
    let source = h.temp_path("diagram.png");
    std::fs::write(&source, STANDARD.decode(PIXEL).unwrap()).unwrap();
    h.dialogs.queue_open(Some(source));
    let log = Rc::new(Log::default());
    let label = h.label.clone();
    let (mut h, field) = host(h, props(Value::Null, &log, &label, json!({}), true));
    focus_area(&mut h, &field);
    h.input("See ");
    h.click(format!("{}:format:image", id()));
    wait(&mut h);
    let keys = log.keys.borrow().clone();
    assert_eq!(keys.len(), 1, "the attachment field gets the file");
    assert_eq!(keys[0].0, "files");
    let files = crate::forms::fields::parse_attachments(&keys[0].1);
    assert_eq!(files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["diagram.png"]);
    assert_eq!(log.changes.borrow().last(), Some(&json!("<p>See <img src=\"attachment:diagram.png\" alt=\"diagram\"></p>")));
    // The preview shows the uploaded image without reading it back.
    let shown = h.cx.read(|cx| field.read(cx).images.get("attachment:diagram.png").cloned());
    assert!(matches!(shown, Some(RichImage::Ready(data)) if data.starts_with("data:image/png;base64,")));
}

#[gpui_kit::test]
fn pastes_an_image_into_the_attachment_field(cx: &mut TestAppContext) {
    let h = document(cx);
    let log = Rc::new(Log::default());
    let label = h.label.clone();
    let existing = json!([{"id": "x", "name": "old.txt", "mime": "text/plain", "size": 1, "sha256": "s"}]).to_string();
    let record = json!({ "files": existing });
    let (mut h, field) = host(h, props(Value::Null, &log, &label, record, true));
    focus_area(&mut h, &field);
    let picture = Image::from_bytes(ImageFormat::Png, STANDARD.decode(PIXEL).unwrap());
    h.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_image(&picture)));
    h.press(if cfg!(target_os = "macos") { "cmd-v" } else { "ctrl-v" });
    wait(&mut h);
    let keys = log.keys.borrow().clone();
    let files = crate::forms::fields::parse_attachments(&keys.last().expect("the file list").1);
    assert_eq!(files.len(), 2, "the pasted image joins the record's files");
    assert!(files[1].name.starts_with("pasted-image-") && files[1].name.ends_with(".png"), "{}", files[1].name);
    let html = log.changes.borrow().last().cloned().expect("the image link");
    let expected = format!("<p><img src=\"attachment:{}\" alt=\"{}\"></p>", files[1].name, files[1].name.trim_end_matches(".png"));
    assert_eq!(html, json!(expected));
}

#[gpui_kit::test]
fn pasting_an_image_without_an_attachment_field_explains_why(cx: &mut TestAppContext) {
    let log = Rc::new(Log::default());
    let h = Harness::start(cx);
    let (mut h, field) = host(h, props(Value::Null, &log, "field-test", json!({}), false));
    focus_area(&mut h, &field);
    let picture = Image::from_bytes(ImageFormat::Png, STANDARD.decode(PIXEL).unwrap());
    h.update(|_, cx| cx.write_to_clipboard(ClipboardItem::new_image(&picture)));
    h.press(if cfg!(target_os = "macos") { "cmd-v" } else { "ctrl-v" });
    assert_eq!(h.label(format!("{}:image-error", id())), "Add an attachment field to this form to insert images");
    assert!(log.changes.borrow().is_empty());
}

#[gpui_kit::test]
fn preview_loads_images_from_the_record_attachments(cx: &mut TestAppContext) {
    let h = document(cx);
    let file = crate::record_attachments::upload_record_attachment(
        h.label.clone(),
        "docs".into(),
        "files".into(),
        "chart.png".into(),
        None,
        PIXEL.into(),
    )
    .expect("store the file");
    let files = json!([{"id": file.id, "name": "chart.png", "mime": "image/png", "size": file.size, "sha256": file.sha256}]);
    let record = json!({ "files": files.to_string() });
    let value = json!("<p>Sales <img src=\"attachment:chart.png\" alt=\"chart\"> <img src=\"attachment:gone.png\" alt=\"gone\"></p>");
    let log = Rc::new(Log::default());
    let label = h.label.clone();
    let (mut h, field) = host(h, props(value, &log, &label, record, true));
    wait(&mut h);
    let images = h.cx.read(|cx| field.read(cx).images.clone());
    assert!(matches!(images.get("attachment:chart.png"), Some(RichImage::Ready(data)) if data.starts_with("data:image/png")));
    // A missing file is not loaded; the preview shows its description.
    assert!(!images.contains_key("attachment:gone.png"));
    assert_eq!(h.label(format!("{}:preview", id())), "Sales chart gone");
    let shown = h.cx.read(|cx| field.read(cx).area.as_ref().expect("a text area").read(cx).value().to_string());
    assert_eq!(shown, "Sales ![chart](attachment:chart.png) ![gone](attachment:gone.png)");
    assert!(log.changes.borrow().is_empty(), "showing a value must not change it");
}
