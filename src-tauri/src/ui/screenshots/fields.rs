//! Field formats in the table designer (`fields-01`), and rich text and attachments on a
//! Run mode form (`fields-03` to `fields-05`). The Runtime entry capture (`fields-02`)
//! belongs to the Runtime spec.

use crate::ui::data::testkit::{data_mode, make_table};
use crate::ui::runtime::tests::{seed, task_form, wait};
use crate::ui::screenshot::context;
use super::runtime::field;
use crate::ui::shell::ModeId;
use crate::ui::testing::{Harness, UiContext as _};
use serde_json::json;

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn fields() {
    let mut cx = context();
    // Tall enough to show the Field settings section below the fold.
    let mut h = Harness::start_sized(&mut cx, (1280.0, 1900.0));
    h.click("new-document");
    let text = |name: &str| json!({ "name": name, "declaredType": "TEXT" });
    make_table(
        &h,
        "contacts",
        json!([
            { "name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1 },
            text("name"), text("phone"), text("notes"), text("interests"), text("documents"),
        ]),
        json!([]),
    );
    h.database_changed();
    h.click("table:contacts");
    h.click("design-table");
    h.append("phone input mask", "(000) 000-0000");
    h.pick("notes format", "Rich text");
    h.pick("interests format", "Multiple choices");
    let mode = data_mode(&mut h);
    let designer = h.cx.read(|cx| mode.read(cx).designer().cloned()).expect("designer");
    let editor = h.cx.read(|cx| designer.read(cx).field_settings().clone());
    h.update(|window, cx| {
        editor.update(cx, |e, cx| e.set_choices("interests", "Pricing\nSupport\nTraining", window, cx))
    });
    h.pick("documents format", "Attachments");
    h.settle();
    let fields = h.config().entities.iter().find(|e| e.table == "contacts").map_or(0, |e| e.fields.len());
    assert_eq!(fields, 4);
    h.capture(
        "fields-01-table-designer",
        &[
            "The Field settings section lists the text columns with a Format picker each.",
            "phone shows the input mask (000) 000-0000 and its preview (___) ___-____.",
            "notes is Rich text, interests is Multiple choices with three choices, documents is Attachments.",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn fields_runtime() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, (1280.0, 1400.0));
    h.click("new-document");
    h.create_table("tasks", &[("id", "INTEGER", 1), ("title", "TEXT", 0), ("notes", "TEXT", 0), ("files", "TEXT", 0)]);
    h.database_changed();
    seed(&mut h, |config| {
        config.entities.push(
            serde_json::from_value(json!({
                "id": "tasks-entity", "table": "tasks", "concurrency": "optimistic",
                "fields": [
                    { "id": "notes-field", "column": "notes", "format": "richText" },
                    { "id": "files-field", "column": "files", "format": "attachment" },
                ],
            }))
            .expect("entity settings"),
        );
    });
    task_form(
        &mut h,
        json!({
            "controls": [
                field("Title", "title", "text", 1, 1, 12),
                field("Notes", "notes", "richText", 2, 1, 12),
                field("Files", "files", "attachment", 3, 1, 12),
            ],
        }),
    );
    h.switch_mode(ModeId::Run);
    wait(&mut h);
    h.click("new-record");
    wait(&mut h);
    h.fill("field:title", "Plan the launch");

    // Markdown typed and from the toolbar, then an image inserted into the Files field.
    h.click("field:notes");
    h.input("Launch plan");
    h.click("field:notes:format:heading");
    h.press("end");
    h.input("\n\n**Before Friday**, <u>confirm</u> the venue.\n\n- [x] Book the van\n- [ ] Print badges\n\n");
    h.input("| Item | Qty |\n| --- | ---: |\n| Badges | 120 |\n| Lanyards | 120 |\n\nFloor plan: ");
    let plan = h.temp_path("floor-plan.png");
    let picture = image::RgbImage::from_fn(240, 90, |x, y| {
        if (x / 30 + y / 30) % 2 == 0 { image::Rgb([59, 130, 246]) } else { image::Rgb([226, 232, 240]) }
    });
    picture.save(&plan).expect("write a sample image");
    h.dialogs.queue_open(Some(plan));
    h.click("field:notes:format:image");
    wait(&mut h);
    assert!(h.label("field:notes:preview").starts_with("Launch plan\nBefore Friday, confirm the venue."));
    assert!(h.exists("field:files:file:floor-plan-png"), "the image is stored in the Files field");
    h.capture(
        "fields-03-rich-text-toolbar",
        &[
            "Above the Notes box a toolbar of 15 icon buttons: bold to strike, code, heading, quote, three list kinds, link, image, table, code block, divider.",
            "The preview under the box shows a 'Launch plan' heading, a bold and underlined sentence, a checked and an unchecked task, a two-column table and a blue checkered image.",
            "Files lists floor-plan.png, the image the toolbar inserted.",
        ],
    );

    for name in ["agenda.pdf", "venue-map.png"] {
        let path = h.temp_path(name);
        std::fs::write(&path, vec![b'x'; 2048]).expect("write a sample file");
        h.dialogs.queue_open(Some(path));
        h.click("field:files:add");
        wait(&mut h);
    }
    assert_eq!(h.label("field:files:file:agenda-pdf"), "agenda.pdf (2 KB)");
    h.click("field:files:remove:agenda-pdf");
    wait(&mut h);
    assert!(!h.exists("field:files:file:agenda-pdf"));
    assert!(h.exists("field:files:remove:venue-map-png"));
    h.capture(
        "fields-04-attachment-removed",
        &[
            "Files lists floor-plan.png and venue-map.png (2 KB), each with a download and a trash button.",
            "An 'Add file' button sits under the list; agenda.pdf is gone.",
        ],
    );

    h.click("create");
    wait(&mut h);
    assert!(h.label("field:notes").starts_with("Launch plan\nBefore Friday, confirm the venue."));
    assert!(h.exists("field:files:save:venue-map-png"));
    assert!(!h.exists("field:files:remove:venue-map-png"));
    h.capture(
        "fields-05-read-only",
        &[
            "The saved record shows Notes formatted: heading, tasks, table and the checkered image, with no toolbar.",
            "Files shows venue-map.png with only a download button, and no Add file button.",
        ],
    );
}
