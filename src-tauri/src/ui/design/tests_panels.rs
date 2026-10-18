//! Form list, form properties, containers and style rules (the editors that the React tests
//! reached through the larger flows), and the track labels of `tests/unit/designer-inputs`.

use super::test_support::{choose, control, edit_store, form, id, open_designer, open_form, studio};
use super::tests::tall_document;
use crate::design::{ControlKind, FormMode, GridTrack, SourceKind, Tone, TrackKind};
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::json;

fn text_of(h: &mut Harness, id: &str) -> Option<String> {
    let studio = studio(h);
    h.cx.read(|cx| studio.read(cx).input_text(id, cx))
}

#[gpui_kit::test]
fn labels_each_tracks_kind_size_and_limits(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    edit_store(&mut h, |config| {
        config.design.forms[0].layout.columns[0] =
            GridTrack { kind: TrackKind::Fixed, value: Some(120.0), min: None, max: Some(300.0) };
    });
    open_designer(&mut h);
    assert_eq!(h.label("grid-column-1-name"), "Column 1");
    assert_eq!(h.label("grid-column-1-size"), "Size px");
    assert_eq!(h.label("grid-column-1-min"), "Min px");
    assert_eq!(h.label("grid-column-1-max"), "Max px");
    assert_eq!(text_of(&mut h, "grid-column-1-size").as_deref(), Some("120"));
    assert_eq!(text_of(&mut h, "grid-column-1-min").as_deref(), Some(""));
    assert_eq!(text_of(&mut h, "grid-column-1-max").as_deref(), Some("300"));
    // A share track labels its size as a share.
    assert_eq!(h.label("grid-column-2-size"), "Share");

    // A minimum above the maximum is shown instead of saved.
    h.fill("grid-column-1-min", "400");
    h.press("enter");
    assert_eq!(h.label("grid-column-1-min-error"), "Minimum cannot exceed maximum.");
    assert_eq!(form(&h).layout.columns[0].min, None);
    h.fill("grid-column-1-min", "100");
    h.press("enter");
    assert_eq!(form(&h).layout.columns[0].min, Some(100.0));
    assert!(!h.exists("grid-column-1-min-error"));
}

#[gpui_kit::test]
fn creates_renames_duplicates_and_deletes_forms(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("new-form");
    let forms = h.config().design.forms;
    assert_eq!(forms.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Main form", "Form 2"]);
    let created = forms[1].clone();
    assert_eq!(h.label("form-title"), "Form 2");

    // A new form gets a page of its own.
    assert!(h.config().pages.iter().any(|p| p.components.iter().any(|c| c.form_id.as_ref() == Some(&created.id))));

    h.fill("form-name", "Suppliers");
    assert_eq!(h.config().design.forms[1].name, "Suppliers");
    assert_eq!(h.label("form-title"), "Suppliers");

    h.click("duplicate-form");
    let forms = h.config().design.forms;
    assert_eq!(forms.len(), 3);
    assert_ne!(forms[2].id, created.id);
    assert!(forms[2].name.starts_with("Suppliers"), "{}", forms[2].name);
    assert_eq!(h.label("form-title"), forms[2].name);

    h.click("delete-form");
    open_form(&mut h, &created.id);
    h.click("delete-form");
    let config = h.config();
    assert_eq!(config.design.forms.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["Main form"]);
    // Deleting a form removes its page and the navigation items that opened it.
    assert!(config.design.navigation.iter().all(|n| n.target_id.as_deref() != Some(created.id.as_str())));
    assert_eq!(config.pages.len(), 1);
}

#[gpui_kit::test]
fn edits_form_modes_source_list_rules_and_navigation_bar(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    h.create_table("tasks", &[("id", "INTEGER", 1), ("title", "TEXT", 0)]);
    open_designer(&mut h);
    choose(&mut h, "data-table", "tasks");

    h.fill("form-name", "Task board");
    assert_eq!(form(&h).name, "Task board");
    assert_eq!(form(&h).source.as_ref().and_then(|s| s.table.as_deref()), Some("tasks"));

    h.click("mode-create");
    assert_eq!(form(&h).modes, [FormMode::List, FormMode::Detail, FormMode::Edit]);
    h.click("mode-create");
    assert_eq!(form(&h).modes, [FormMode::List, FormMode::Detail, FormMode::Create, FormMode::Edit]);
    let bar = form(&h).navigation_bar;
    h.click("navigation-bar");
    assert_eq!(form(&h).navigation_bar, !bar);

    h.fill("page-size", "50");
    h.press("enter");
    assert_eq!(form(&h).page_size, 50);
    h.click("list-column:title");
    assert_eq!(form(&h).list_columns, ["title"]);

    h.click("add-rule");
    h.fill("rule-1", "record.title <> ''");
    h.fill("rule-1-message", "Title is required");
    assert!(!h.exists("rule-1-problem"));
    let rule = form(&h).rules.remove(0);
    assert_eq!((rule.expression.as_str(), rule.message.as_str()), ("record.title <> ''", "Title is required"));
    h.fill("rule-1", "record.nope <>");
    assert!(h.exists("rule-1-problem"));
    h.click("remove-rule-1");
    assert!(form(&h).rules.is_empty());

    // A saved query source is read-only: it keeps the list and detail modes only.
    super::test_support::edit_store(&mut h, |config| {
        config.saved_queries = serde_json::from_value(json!([
            {"id": "q-open", "name": "Open tasks", "sql": "SELECT * FROM tasks"}
        ]))
        .expect("query");
    });
    choose(&mut h, "source-kind", "Saved query (read-only)");
    let source = form(&h).source.expect("a source");
    assert_eq!((source.kind, source.query_id.as_deref()), (SourceKind::Query, Some("q-open")));
    assert_eq!(form(&h).modes, [FormMode::List, FormMode::Detail]);
    assert_eq!(h.label("source-query"), "Open tasks");
    choose(&mut h, "source-kind", "Unbound");
    assert!(form(&h).source.is_none());
}

#[gpui_kit::test]
fn nests_controls_in_sections_and_tab_groups_with_their_own_grid(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-section");
    let section = control(&h, ControlKind::Section).expect("section");
    assert!(h.exists("container-columns"), "the container grid is edited with the section");
    assert!(h.exists(format!("canvas-{}", section.id)));

    // A control added while the section is selected lands inside it.
    h.click("add-text");
    let text = control(&h, ControlKind::Text).expect("text");
    assert_eq!(text.parent.as_ref().map(|p| p.id.as_str()), Some(section.id.as_str()));
    assert_eq!(h.label(text.id.clone()), "New text field");
    // Selecting the nested control does not select the section around it.
    h.click(id(&text.id));
    assert_eq!(h.label("properties-title"), "Control");
    assert_eq!(text_of(&mut h, "prop-label").as_deref(), Some("New text field"));

    // Moving it to the form takes it out again.
    choose(&mut h, "prop-container", "Form");
    assert!(control(&h, ControlKind::Text).expect("text").parent.is_none());
    choose(&mut h, "prop-container", &section.label);
    assert_eq!(control(&h, ControlKind::Text).expect("text").parent.map(|p| p.id), Some(section.id.clone()));

    // A tab group shows its pages; each page has its own grid.
    h.click("back-to-form");
    h.click("add-tabs");
    let tabs = control(&h, ControlKind::Tabs).expect("tabs");
    assert_eq!(tabs.tabs.len(), 2, "a tab group starts with two pages");
    h.click("add-tab");
    h.fill("tab-3-label", "Notes");
    let tabs = control(&h, ControlKind::Tabs).expect("tabs");
    assert_eq!(tabs.tabs[2].label, "Notes");
    h.click(format!("tab-page:{}:{}", tabs.id, tabs.tabs[1].id));
    h.click("add-label");
    let label = control(&h, ControlKind::Label).expect("label");
    assert_eq!(label.parent.as_ref().and_then(|p| p.tab.as_deref()), Some(tabs.tabs[1].id.as_str()));
    // Selecting the group by its card would land on the label inside it, so select it directly.
    let studio = studio(&mut h);
    let tabs_id = tabs.id.clone();
    h.cx.update(|cx| studio.update(cx, |s, cx| s.select(Some(tabs_id), cx)));
    h.settle();
    h.click("remove-tab-2");
    assert_eq!(control(&h, ControlKind::Tabs).expect("tabs").tabs.len(), 2);
    assert!(control(&h, ControlKind::Label).is_none(), "removing a page removes its controls");

    h.click("delete-control");
    assert!(control(&h, ControlKind::Tabs).is_none());
}

#[gpui_kit::test]
fn orders_and_removes_conditional_style_rules(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-number");
    h.click("add-style");
    h.fill("style-1-when", "value < 0");
    h.click("add-style");
    h.fill("style-2-when", "value > 100");
    choose(&mut h, "style-2-tone", "Warning");
    let styles = control(&h, ControlKind::Number).expect("number").styles;
    assert_eq!(styles.iter().map(|s| s.when.as_str()).collect::<Vec<_>>(), ["value < 0", "value > 100"]);
    assert_eq!(styles[1].tone, Tone::Warning);

    h.click("move-style-down-1");
    let styles = control(&h, ControlKind::Number).expect("number").styles;
    assert_eq!(styles.iter().map(|s| s.when.as_str()).collect::<Vec<_>>(), ["value > 100", "value < 0"]);
    h.click("remove-style-1");
    let styles = control(&h, ControlKind::Number).expect("number").styles;
    assert_eq!(styles.iter().map(|s| s.when.as_str()).collect::<Vec<_>>(), ["value < 0"]);
}

#[gpui_kit::test]
fn edits_select_options_lookups_validation_and_masks(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    open_designer(&mut h);
    h.click("add-select");
    h.fill("prop-options", "a=Alpha\nb\n\nc=Gamma");
    let options = control(&h, ControlKind::Select).expect("select").options;
    assert_eq!(
        options.iter().map(|o| (o.value.as_str(), o.label.as_str())).collect::<Vec<_>>(),
        [("a", "Alpha"), ("b", "b"), ("c", "Gamma")]
    );

    h.click("add-text");
    h.click("prop-required");
    assert!(control(&h, ControlKind::Text).expect("text").validation.required);
    h.fill("prop-mask", "(000) 000-0000");
    assert!(h.label("prop-mask-hint").starts_with("Entry looks like"));
    h.fill("prop-mask", "(--)");
    assert_eq!(h.label("prop-mask-error"), "The input mask has no characters to type into.");
    h.fill("prop-pattern", "^[a-z]+$");
    h.fill("prop-error-message", "Letters only");
    let text = control(&h, ControlKind::Text).expect("text");
    assert_eq!(text.validation.pattern.as_deref(), Some("^[a-z]+$"));
    assert_eq!(text.validation.message.as_deref(), Some("Letters only"));
    assert_eq!(text.input_mask.as_deref(), Some("(--)"));

    h.create_table("people", &[("id", "INTEGER", 1), ("name", "TEXT", 0)]);
    h.click("add-relationship");
    choose(&mut h, "lookup-table", "people");
    super::test_support::wait_until(&mut h, "the lookup columns", |h| {
        let studio = studio(h);
        h.cx.read(|cx| studio.read(cx).has_columns("people"))
    });
    choose(&mut h, "lookup-value", "id");
    choose(&mut h, "lookup-display", "name");
    let relationship = control(&h, ControlKind::Relationship).expect("lookup").relationship.expect("relationship");
    assert_eq!(
        (relationship.table.as_str(), relationship.value_column.as_str(), relationship.display_column.as_str()),
        ("people", "id", "name")
    );
}
