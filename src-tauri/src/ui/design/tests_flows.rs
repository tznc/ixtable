//! Designer flows that end in other modes: filters and styles authored in the studio and
//! applied in Runtime (port of the first test of `expressions.test.tsx`), Problems links
//! (`expression-problems.test.tsx`), orphaned query bindings (`data-binding-guards.test.tsx`)
//! and the generated-forms offer and navigation editor (`guided-crud.test.tsx`).

use super::test_support::{choose, form, open_designer, studio, wait_until};
use super::tests::tall_document;
use crate::data::DataValue;
use crate::design::{Control, Form, NavKind, SourceKind, Tone};
use crate::ui::runtime::tests::{nav, wait};
use crate::ui::runtime::tests_misc::table_with_keys;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::json;

fn form_named(h: &Harness, name: &str) -> Form {
    h.config().design.forms.into_iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no form {name}"))
}

fn control_of(h: &Harness, form: &str, label: &str) -> Control {
    form_named(h, form)
        .controls
        .into_iter()
        .find(|c| c.label == label)
        .unwrap_or_else(|| panic!("no control {label} on {form}"))
}

fn seed(h: &mut Harness) {
    table_with_keys(
        h,
        "customers",
        json!([
            {"name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1},
            {"name": "name", "declaredType": "TEXT"},
            {"name": "active", "declaredType": "INTEGER"},
        ]),
        json!([]),
    );
    table_with_keys(
        h,
        "orders",
        json!([
            {"name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1},
            {"name": "customer_id", "declaredType": "INTEGER"},
            {"name": "qty", "declaredType": "INTEGER"},
        ]),
        json!([{"columns": ["customer_id"], "targetTable": "customers", "targetColumns": ["id"], "onUpdate": null, "onDelete": null}]),
    );
    table_with_keys(
        h,
        "order_items",
        json!([
            {"name": "id", "declaredType": "INTEGER", "primaryKeyPosition": 1},
            {"name": "order_id", "declaredType": "INTEGER"},
            {"name": "product", "declaredType": "TEXT"},
            {"name": "quantity", "declaredType": "INTEGER"},
        ]),
        json!([{"columns": ["order_id"], "targetTable": "orders", "targetColumns": ["id"], "onUpdate": null, "onDelete": null}]),
    );
    for (id, name, active) in [(1, "Acme", 1), (2, "Globex", 0), (3, "Initech", 1)] {
        h.insert_row(
            "customers",
            &[
                ("id", DataValue::Integer(id)),
                ("name", DataValue::Text(name.into())),
                ("active", DataValue::Integer(active)),
            ],
        );
    }
    for (id, qty) in [(1, 2), (2, 9), (3, 0)] {
        h.insert_row(
            "orders",
            &[("id", DataValue::Integer(id)), ("customer_id", DataValue::Integer(1)), ("qty", DataValue::Integer(qty))],
        );
    }
    for (id, product, quantity) in [(1, "Widget", 2), (2, "Gadget", 0)] {
        h.insert_row(
            "order_items",
            &[
                ("id", DataValue::Integer(id)),
                ("order_id", DataValue::Integer(1)),
                ("product", DataValue::Text(product.into())),
                ("quantity", DataValue::Integer(quantity)),
            ],
        );
    }
}

/// Generates the forms of `table` from the form list and waits for them.
fn generate_form(h: &mut Harness, table: &str, list_name: &str) {
    choose(h, "generate-table", table);
    h.click("generate-form");
    wait_until(h, "the generated forms", |h| h.config().design.forms.iter().any(|f| f.name == list_name));
}

#[gpui_kit::test]
fn applies_designer_authored_filters_to_a_list_form_a_related_list_and_a_lookup_and_styles_cells(
    cx: &mut TestAppContext,
) {
    let mut h = tall_document(cx);
    seed(&mut h);
    open_designer(&mut h);
    generate_form(&mut h, "orders", "Orders list");

    // A conditional style on Qty: a wrong name is flagged, a right one is kept.
    h.click(control_of(&h, "Orders", "Qty").id);
    h.click("add-style");
    h.fill("style-1-when", "bogus > 5");
    assert!(h.label("style-1-when-problem").contains("bogus"), "{}", h.label("style-1-when-problem"));
    h.fill("style-1-when", "value > 5");
    assert!(!h.exists("style-1-when-problem"));
    choose(&mut h, "style-1-tone", "Negative");
    let style = control_of(&h, "Orders", "Qty").styles.remove(0);
    assert_eq!((style.when.as_str(), style.tone), ("value > 5", Tone::Negative));

    h.click(control_of(&h, "Orders", "Customer").id);
    h.fill("lookup-filter", "record.active = 1");
    assert!(!h.exists("lookup-filter-problem"));

    h.click(control_of(&h, "Orders", "Order items").id);
    h.fill("related-filter", "record.quantity > 0");

    let list = form_named(&h, "Orders list");
    h.click(format!("form:{}", list.id));
    h.fill("list-filter", "record.nope > 0");
    assert!(h.exists("list-filter-problem"));
    h.fill("list-filter", "record.qty > 0");
    assert!(!h.exists("list-filter-problem"));
    assert_eq!(form_named(&h, "Orders list").filter.as_deref(), Some("record.qty > 0"));

    // Runtime applies all of it.
    h.switch_mode(ModeId::Run);
    let orders = nav(&mut h, "Orders");
    h.click(orders);
    wait(&mut h);
    assert_eq!(h.label("pager-status"), "1–2 of 2");
    assert!(h.exists("row:1"));
    assert!(!h.exists("row:2"), "the filter hides the order with no quantity");
    assert_eq!(h.label("cell:1:qty"), "9");
    assert_eq!(h.label("cell:1:qty:tone"), "negative");
    assert!(!h.exists("cell:0:qty:tone"), "only values over 5 are styled");

    h.click("row:0");
    wait(&mut h);
    assert_eq!(h.label("related:order-items:cell:0:product"), "Widget");
    assert!(!h.exists("related:order-items:row:1"), "the related filter hides Gadget");

    h.click("edit");
    wait(&mut h);
    h.click("field:customer");
    wait(&mut h);
    assert!(h.exists("field:customer:option:initech"));
    assert!(!h.exists("field:customer:option:globex"), "the choice filter hides inactive customers");
}

#[gpui_kit::test]
fn lists_expression_errors_in_problems_opens_the_owning_control_and_blocks_bundle_export(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    h.create_table("orders", &[("id", "INTEGER", 1), ("total", "INTEGER", 0)]);
    open_designer(&mut h);
    generate_form(&mut h, "orders", "Orders list");
    let total = control_of(&h, "Orders", "Total");
    h.click(total.id.clone());
    h.type_into("prop-visible-when", "record.total >");
    assert!(h.exists("prop-visible-when-problem"));

    h.switch_mode(ModeId::App);
    h.click("tab-problems");
    wait_until(&mut h, "the problems", |h| h.label("problems-summary") != "Checking…");
    let problem = h.label(("problem", 0usize));
    assert!(problem.contains("Control \"Total\" › Visible when"), "{problem}");
    assert!(problem.contains("Unexpected end of expression"), "{problem}");

    h.click("tab-release");
    let path = h.temp_path("blocked.ixtr");
    h.dialogs.queue_save(Some(path.clone()));
    h.click("export-bundle");
    let error = h.error_at("release-error");
    assert!(error.contains("EXPRESSION_ERRORS"), "{error}");
    assert!(error.contains("Settings › Problems"), "{error}");
    assert_eq!(h.dialogs.count("save"), 0);
    assert!(!path.exists());

    h.click("tab-problems");
    wait_until(&mut h, "the problems", |h| h.exists(("problem-open", 0usize)));
    assert!(h.label(("problem-open", 0usize)).starts_with("Open form Orders"));
    h.click(("problem-open", 0usize));
    assert_eq!(h.mode(), ModeId::Design);
    assert_eq!(h.label("properties-title"), "Control");
    let studio = studio(&mut h);
    let shown = h.cx.read(|cx| studio.read(cx).input_text("prop-visible-when", cx));
    assert_eq!(shown.as_deref(), Some("record.total >"));
    assert!(h.exists("prop-visible-when-problem"));

    h.fill("prop-visible-when", "record.total > 0");
    assert!(!h.exists("prop-visible-when-problem"));
    h.switch_mode(ModeId::App);
    h.click("tab-problems");
    wait_until(&mut h, "no errors", |h| h.label("problems-summary").starts_with("0 errors,"));
    assert!(!h.exists(("problem", 0usize)));
}

#[gpui_kit::test]
fn lists_orphaned_query_parameter_bindings_with_a_remove_action(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    let (query, form_id) = (uuid::Uuid::new_v4().to_string(), uuid::Uuid::new_v4().to_string());
    {
        let (query, form_id) = (query.clone(), form_id.clone());
        super::test_support::edit_store(&mut h, move |config| {
            config.saved_queries = serde_json::from_value(json!([{
                "id": query, "name": "Parts in bin",
                "sql": "SELECT id, name FROM parts WHERE bin = $bin ORDER BY id",
                "parameters": [{"name": "bin", "logicalType": "text", "required": true}],
            }]))
            .expect("saved query");
            config.design.forms.push(
                serde_json::from_value(json!({
                    "id": form_id, "name": "Bin parts",
                    "source": {"kind": "query", "queryId": query, "params": {"bin": "'A'", "old": "1"}},
                    "modes": ["list"], "listColumns": ["id", "name"], "controls": [],
                }))
                .expect("form"),
            );
        });
    }
    open_designer(&mut h);
    h.click(format!("form:{form_id}"));
    assert_eq!(h.label("orphan-old"), "$old is no longer a parameter of this query.");
    h.click("remove-binding:old");
    let source = h.config().design.forms.iter().find(|f| f.id == form_id).and_then(|f| f.source.clone()).expect("source");
    assert_eq!(source.kind, SourceKind::Query);
    assert_eq!(source.query_id.as_deref(), Some(query.as_str()));
    assert_eq!(source.params.into_iter().collect::<Vec<_>>(), [("bin".to_string(), "'A'".to_string())]);
    assert!(!h.exists("orphan-old"));
}

/// Creates a table (an id and a `name` column) in the Data mode form.
fn create_table_in_ui(h: &mut Harness, name: &str) {
    h.go("new-table");
    h.enter("Table name", name);
    h.go("submit-create-table");
    assert!(h.exists(format!("table:{name}")), "the new table is listed");
}

#[gpui_kit::test]
fn offers_forms_for_a_new_table_and_generates_the_app_from_the_form_list(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    create_table_in_ui(&mut h, "customers");
    assert_eq!(h.label("create-forms-offer"), "Forms for customers");
    h.click("offer-dismiss");
    assert!(!h.exists("create-forms-offer"));

    create_table_in_ui(&mut h, "orders");
    assert_eq!(h.label("create-forms-offer"), "Forms for orders");
    h.click("create-forms");
    wait_until(&mut h, "the forms", |h| h.exists("offer-done"));
    assert_eq!(h.label("offer-done"), "Forms for orders are ready in Design and Run mode.");
    let names = |h: &Harness| h.config().design.forms.into_iter().map(|f| f.name).collect::<Vec<_>>();
    assert_eq!(names(&h), ["Main form", "Orders list", "Orders"]);

    h.switch_mode(ModeId::Design);
    h.click("generate-app");
    wait_until(&mut h, "the notice", |h| h.notice().is_some());
    assert_eq!(h.notice().as_deref(), Some("Added 3 forms and pages."));
    h.click("generate-app");
    wait_until(&mut h, "the second notice", |h| {
        h.notice().as_deref() == Some("Every table already has forms.")
    });
    assert_eq!(names(&h), ["Main form", "Orders list", "Orders", "Customers list", "Customers"]);
    let labels: Vec<_> = h.config().design.navigation.into_iter().map(|n| n.label).collect();
    assert_eq!(labels, ["Main form", "Orders", "Customers"]);
}

#[gpui_kit::test]
fn retargets_a_navigation_item_to_a_table_page_in_the_navigation_editor(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    seed(&mut h);
    open_designer(&mut h);
    h.click("tab-navigation");
    assert_eq!(h.label("navigation-editor"), "Navigation editor");
    let main = h.config().design.navigation[0].clone();
    assert_eq!(main.label, "Main form");
    choose(&mut h, &format!("nav-kind:{}", main.id), "table");
    choose(&mut h, &format!("nav-target:{}", main.id), "orders");
    let item = h.config().design.navigation[0].clone();
    assert_eq!((item.kind, item.target_id.as_deref()), (NavKind::Table, Some("orders")));

    // The group, order and start page controls edit the same tree.
    h.click("add-nav-group");
    let navigation = h.config().design.navigation;
    assert_eq!(navigation.len(), 2);
    assert_eq!(navigation[1].kind, NavKind::Group);
    choose(&mut h, &format!("nav-group:{}", main.id), "In New group");
    let navigation = h.config().design.navigation;
    assert_eq!(navigation.len(), 1);
    assert_eq!(navigation[0].children[0].id, main.id);
    h.click(format!("nav-remove:{}", navigation[0].id));
    assert!(h.config().design.navigation.is_empty());
    assert_eq!(form(&h).name, "Main form");
}
