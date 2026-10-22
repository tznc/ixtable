//! Grid designer flows (port of `tests/integration/form-grid-designer.test.tsx`). Controls are
//! found by their stored ids, read back from the config.
//!
//! The headless windows do not deliver an input's blur event when focus moves, so a draft
//! (applied on blur or Enter) is applied here with Enter; both run the same commit.
//!
//! A disabled input or button exposes no flag to the headless tests, so those checks are made
//! on the stored result (a resize that is not allowed leaves the placement unchanged).

use super::test_support::{choose, control, edit_store, form, id, open_designer, wait_until};
use super::tests::tall_document;
use crate::design::{Control, ControlKind, GridAlign, Placement, TrackKind};
use crate::ui::{shell::ModeId, testing::Harness};
use gpui_kit::TestAppContext;

/// Adds a control of the palette kind `kind` (`add-<kind>`) and returns it.
fn add(h: &mut Harness, kind: &str) -> Control {
    h.click(id(&format!("add-{kind}")));
    form(h).controls.last().cloned().expect("the new control")
}

/// Selects the control, then presses Alt+Left `times` times.
fn narrow(h: &mut Harness, control: &Control, times: usize) {
    h.click(id(&control.id));
    for _ in 0..times {
        h.press("alt-left");
    }
}

fn placement(h: &Harness, kind: ControlKind) -> Placement {
    control(h, kind).expect("the control").placement
}

#[gpui_kit::test]
fn limits_keyboard_resizing_per_control_kind(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);

    let related = add(&mut h, "related-list");
    assert_eq!(h.label(related.id.clone()), "New related records");
    narrow(&mut h, &related, 10);
    assert_eq!(placement(&h, ControlKind::RelatedList).column_span, 6);

    let toggle = add(&mut h, "boolean");
    assert_eq!(h.label(toggle.id.clone()), "New yes/no");
    narrow(&mut h, &toggle, 10);
    assert_eq!(placement(&h, ControlKind::Boolean).column_span, 1);
}

#[gpui_kit::test]
fn edits_tracks_alignment_and_regions_and_hides_enabled_when_on_static_controls(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);

    h.click("grid-settings-toggle");
    choose(&mut h, "grid-column-1-kind", "Fixed size (px)");
    choose(&mut h, "grid-justify", "center");
    let layout = form(&h).layout;
    assert_eq!(layout.columns[0].kind, TrackKind::Fixed);
    assert_eq!(layout.columns[0].value, Some(120.0));
    assert_eq!(layout.columns.len(), 12);
    assert_eq!(layout.justify_items, GridAlign::Center);
    h.click("grid-add-region");
    let names: Vec<_> = form(&h).layout.named_regions.into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["region1"]);

    add(&mut h, "label");
    assert!(!h.exists("prop-enabled-when"));
    assert!(h.exists("prop-visible-when"));
    choose(&mut h, "prop-region", "region1");
    assert_eq!(placement(&h, ControlKind::Label).region.as_deref(), Some("region1"));

    add(&mut h, "section");
    assert!(h.exists("prop-enabled-when"));

    h.click("form-breadcrumb-0");
    // The section the user opened stays open after the panel comes back.
    assert!(!h.exists("grid-settings-summary"));
    h.type_into("grid-region-1-column-span", "0");
    h.press("enter");
    assert_eq!(form(&h).layout.named_regions[0].column_span, 12);
}

#[gpui_kit::test]
fn renames_and_removes_a_region_a_control_uses_and_keeps_the_document_saveable(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("grid-settings-toggle");
    h.click("grid-add-region");
    add(&mut h, "label");
    choose(&mut h, "prop-region", "region1");
    assert_eq!(placement(&h, ControlKind::Label).region.as_deref(), Some("region1"));

    h.click("form-breadcrumb-0");
    h.fill("grid-region-1-name", "");
    h.press("enter");
    assert_eq!(h.label("grid-region-1-name-error"), "Area name is required.");
    h.type_into("grid-region-1-name", "header");
    h.press("enter");
    let saved = form(&h);
    let names: Vec<_> = saved.layout.named_regions.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["header"]);
    let label = saved.controls.iter().find(|c| c.kind == ControlKind::Label).expect("the label");
    assert_eq!(label.placement.region.as_deref(), Some("header"));

    choose(&mut h, "grid-justify", "end");
    assert_eq!(form(&h).layout.justify_items, GridAlign::End);

    h.click("grid-remove-region-1");
    let saved = form(&h);
    assert!(saved.layout.named_regions.is_empty());
    let label = saved.controls.iter().find(|c| c.kind == ControlKind::Label).expect("the label");
    assert_eq!(label.placement.region, None);
    choose(&mut h, "grid-justify", "start");
    assert_eq!(form(&h).layout.justify_items, GridAlign::Start);
    // The document still passes the backend's design validation.
    let issues = crate::validation::validate_document(h.label.clone()).expect("validate");
    assert!(
        !issues.iter().any(|i| i.severity == crate::validation::Severity::Error && i.object_kind == "form"),
        "{issues:?}"
    );
}

#[gpui_kit::test]
fn applies_a_typed_span_on_blur_instead_of_snapping_each_keystroke(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    let related = add(&mut h, "related-list");
    let before = placement(&h, ControlKind::RelatedList).column_span;
    h.click("control-position-toggle");
    h.fill("prop-column-span", "10");
    assert_eq!(placement(&h, ControlKind::RelatedList).column_span, before, "applied before blur");
    h.press("enter");
    assert_eq!(placement(&h, ControlKind::RelatedList).column_span, 10);
    assert_eq!(related.kind, ControlKind::RelatedList);
}

#[gpui_kit::test]
fn picks_an_image_asset_by_its_stable_id_and_previews_it(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    h.switch_mode(ModeId::App);
    h.click("tab-assets");
    let dir = h.temp_path("image-assets");
    std::fs::create_dir_all(&dir).expect("asset dir");
    let path = dir.join("logo.png");
    std::fs::write(&path, [0x89, 0x50, 0x4e, 0x47, 1, 2, 3]).expect("write the image");
    h.dialogs.queue_open(Some(path));
    h.click("import-asset");
    assert_eq!(h.notice().unwrap_or_default(), "Imported logo.png.");
    let asset = crate::list_attachments(h.label.clone()).expect("assets").remove(0);

    open_designer(&mut h);
    let image = add(&mut h, "image");
    wait_until(&mut h, "the asset list", |h| h.exists("prop-asset"));
    choose(&mut h, "prop-asset", "logo.png");
    assert_eq!(control(&h, ControlKind::Image).expect("image").asset_id.as_deref(), Some(asset.id.as_str()));
    assert_eq!(h.label("image-preview"), "Image preview");
    wait_until(&mut h, "the preview to load", |h| h.try_label("image-preview-state").is_none());
    assert_eq!(image.label, "New image");
}

#[gpui_kit::test]
fn clears_a_stored_enabled_when_that_a_label_cannot_use(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    let note = Control {
        id: uuid::Uuid::new_v4().to_string(),
        kind: ControlKind::Label,
        label: "Note".into(),
        placement: Placement { column: 1, row: 1, column_span: 12, row_span: 1, region: None },
        enabled_when: Some("true".into()),
        ..Default::default()
    };
    let seeded = note.clone();
    edit_store(&mut h, move |config| config.design.forms[0].controls.push(seeded));

    // The stored value survives saving and opening the archive.
    let archive = h.temp_path("clear-enabled-when.ixt");
    h.dialogs.queue_save(Some(archive.clone()));
    h.click("save-project");
    assert_eq!(h.label("document-status"), "Saved");
    let reopened = format!("reopen-{}", uuid::Uuid::new_v4().simple());
    crate::open_document(reopened.clone(), archive.to_string_lossy().into()).expect("open the archive");
    let stored = crate::read_document_config(reopened.clone()).expect("config");
    crate::close_document(reopened, true).expect("close");
    assert_eq!(stored.design.forms[0].controls.last().and_then(|c| c.enabled_when.as_deref()), Some("true"));

    open_designer(&mut h);
    h.click(id(&note.id));
    h.click("clear-enabled-when");
    assert_eq!(control(&h, ControlKind::Label).expect("note").enabled_when, None);
    assert!(!h.exists("clear-enabled-when"));
}

#[test]
fn width_options_offer_only_even_shares_of_the_grid() {
    use super::control_props::width_options;
    let labels = |columns, current| width_options(columns, current).into_iter().map(|(v, l)| format!("{v}={l}")).collect::<Vec<_>>();
    assert_eq!(labels(12, 12), ["12=Full", "8=⅔", "6=½", "4=⅓", "3=¼"]);
    assert_eq!(labels(4, 3), ["4=Full", "3=3 of 4 columns", "2=½", "1=¼"]);
}

#[gpui_kit::test]
fn sets_the_column_span_from_the_width_choice_and_hides_exact_position(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    add(&mut h, "related-list");
    assert!(h.exists(id("prop-width")));
    assert!(!h.exists(id("prop-column-span")), "exact position starts collapsed");
    choose(&mut h, "prop-width", "½");
    let columns = form(&h).layout.columns.len() as u16;
    assert_eq!(placement(&h, ControlKind::RelatedList).column_span, columns / 2);
    h.click("control-position-toggle");
    assert!(h.exists(id("prop-column-span")));
}

#[gpui_kit::test]
fn keyboard_help_lives_behind_the_shortcuts_button(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    assert!(h.exists("form-shortcuts"));
    assert!(!h.exists("form-hint"));
    h.click("form-shortcuts");
    assert_eq!(h.label("form-hint"), "Enter selects a control, Alt+Arrow resizes, Alt+Shift+Arrow moves.");
    h.click("form-shortcuts");
    assert!(!h.exists("form-hint"));
}
