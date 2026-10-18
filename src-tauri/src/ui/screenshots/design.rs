//! Design mode captures (`forms-01`, `form-views-07`, `nesting-01`, `grid-01..03`,
//! `app-qa-06/07`). Port of the Design parts of scripts/screenshot/specs/{forms,form-views,
//! nesting,grid,app-shell}.spec.tsx.
//!
//! The window is tall so the whole properties panel is on screen (the harness only clicks
//! what is visible).

use super::runtime::{column, id_column, int, keyed, seed_sales, seed_task_table, text};
use crate::design::{ControlKind, FormMode};
use crate::ui::design::test_support::{choose, id, control, form, open_designer, options_of, wait_until};
use crate::ui::runtime::tests::{seed, uid};
use crate::ui::runtime::tests_misc::table_with_keys;
use crate::ui::screenshot::context;
use crate::ui::testing::{Harness, UiContext};
use serde_json::json;

const WINDOW: (f32, f32) = (1500.0, 1500.0);

fn generate_form<C: UiContext>(h: &mut Harness<'_, C>, table: &str) {
    choose(h, "generate-table", table);
    h.click("generate-form");
    // The detail form is named after the table ("Orders"); a related list may come first.
    let detail = |h: &Harness<'_, C>| h.config().design.forms.into_iter().find(|f| f.name.to_lowercase() == table);
    wait_until(h, "the generated form", |h| detail(h).is_some());
    // Open it, as its page's "Edit form" does.
    let detail = detail(h).expect("the detail form");
    crate::ui::design::test_support::open_form(h, &detail.id);
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn design_forms() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    seed_sales(&mut h);
    open_designer(&mut h);
    generate_form(&mut h, "customers");
    generate_form(&mut h, "orders");

    // Resize Amount with the keyboard, then add a validation rule.
    let orders = h.config().design.forms.into_iter().find(|f| f.name == "Orders").expect("Orders form");
    let amount = orders.controls.iter().find(|c| c.label == "Amount").expect("Amount").clone();
    h.click(id(&amount.id));
    let before = amount.placement.column_span;
    h.press("alt-left");
    h.press("alt-left");
    let after = |h: &Harness<'_, _>| {
        let form = h.config().design.forms.into_iter().find(|f| f.name == "Orders").expect("Orders");
        form.controls.iter().find(|c| c.label == "Amount").expect("Amount").placement.column_span
    };
    assert_eq!(after(&h), before - 2);
    h.fill("prop-validation-rule", "value > 0");
    h.fill("prop-error-message", "Amount must be positive.");
    let stored = h.config().design.forms.into_iter().find(|f| f.name == "Orders").expect("Orders");
    let rule = stored.controls.iter().find(|c| c.label == "Amount").expect("Amount").validation.clone();
    assert_eq!(rule.expression.as_deref(), Some("value > 0"));
    assert_eq!(rule.message.as_deref(), Some("Amount must be positive."));
    h.capture(
        "forms-01-designer",
        &[
            "The generated Orders form shows its controls as cards on the 12-column grid.",
            "The selected Amount card is highlighted, narrower than its neighbours, and shows resize handles.",
            "The Properties panel shows the Amount validation rule and error message.",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn design_form_properties() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    seed_task_table(&mut h);
    seed(&mut h, |config| {
        let id = config.design.forms[0].id.clone();
        let form = json!({
            "id": id, "name": "Tasks", "source": {"kind": "table", "table": "tasks"},
            "listColumns": ["title"], "rules": [], "pageSize": 25, "controls": [],
            "navigationBar": true, "modes": ["list", "detail", "create", "edit", "continuous"],
        });
        config.design.forms[0] = serde_json::from_value(form).expect("form");
    });
    open_designer(&mut h);
    assert!(h.exists("mode-continuous") && h.exists("mode-split") && h.exists("navigation-bar"));
    let stored = form(&h);
    assert!(stored.navigation_bar);
    assert!(stored.modes.contains(&FormMode::Continuous));
    assert!(!stored.modes.contains(&FormMode::Split));
    h.capture(
        "form-views-07-properties",
        &[
            "The Properties panel's Modes section lists List, Detail, Create, Edit, Continuous and Split mode checkboxes.",
            "Continuous mode and 'Record navigation bar' are checked; Split mode is not.",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn design_nesting_child_form_picker() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    seed_sales(&mut h);
    table_with_keys(
        &mut h,
        "allocations",
        json!([id_column(), column("order_item_id", "INTEGER"), column("bin", "TEXT"), column("quantity", "INTEGER")]),
        keyed("order_item_id", "order_items"),
    );
    h.database_changed();
    for (id, item, bin) in [(1, 1, "Aisle 3 / Bin A1"), (2, 1, "Aisle 3 / Bin A2"), (3, 2, "Aisle 7 / Bin C4")] {
        h.insert_row(
            "allocations",
            &[("id", int(id)), ("order_item_id", int(item)), ("bin", text(bin)), ("quantity", int(1))],
        );
    }

    let ids: Vec<(&str, &str, String)> = vec![
        ("customers", "Customers", uid()),
        ("orders", "Orders", uid()),
        ("order_items", "Order items", uid()),
        ("allocations", "Allocations", uid()),
    ];
    let build = |index: usize, columns: &[&str], child: Option<(usize, &str)>| {
        let (table, name, id) = &ids[index];
        let mut controls: Vec<_> = columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                json!({
                    "id": uid(), "kind": "text", "label": c[..1].to_uppercase() + &c[1..], "binding": {"column": c},
                    "placement": {"column": 1 + i * 6, "row": 1, "columnSpan": 6, "rowSpan": 1},
                })
            })
            .collect();
        if let Some((child, foreign_key)) = child {
            controls.push(json!({
                "id": uid(), "kind": "relatedList", "label": ids[child].1,
                "related": {"table": ids[child].0, "foreignKey": foreign_key, "parentColumn": "id",
                            "columns": [], "formId": ids[child].2},
                "placement": {"column": 1, "row": 2, "columnSpan": 12, "rowSpan": 1},
            }));
        }
        json!({
            "id": id, "name": name, "source": {"kind": "table", "table": table},
            "modes": ["list", "detail", "create", "edit"], "listColumns": columns, "controls": controls,
        })
    };
    let forms = vec![
        build(0, &["name", "city"], Some((1, "customer_id"))),
        build(1, &["status", "amount"], Some((2, "order_id"))),
        build(2, &["product", "quantity"], Some((3, "order_item_id"))),
        build(3, &["bin", "quantity"], None),
    ];
    let customers = ids[0].2.clone();
    let items_form = ids[2].2.clone();
    seed(&mut h, move |config| {
        config.design.forms.extend(forms.into_iter().map(|f| serde_json::from_value(f).expect("form")));
        config.design.navigation[0].label = "Customers".into();
        config.design.navigation[0].kind = crate::design::NavKind::Form;
        config.design.navigation[0].target_id = Some(customers);
    });

    open_designer(&mut h);
    crate::ui::design::test_support::open_form(&mut h, &items_form);
    assert_eq!(h.label("form-title"), "Order items");
    let items = h.config().design.forms.into_iter().find(|f| f.id == items_form).expect("Order items form");
    let related = items.controls.iter().find(|c| c.kind == ControlKind::RelatedList).expect("the Allocations list");
    h.click(id(&related.id));
    assert_eq!(h.label("related-form"), "Allocations");
    assert_eq!(options_of(&mut h, "related-form"), ["Generated from the table", "Main form", "Allocations"]);
    h.capture(
        "nesting-01-child-form-picker",
        &[
            "The Order items form is open in the designer with its Allocations related list selected.",
            "The Child form picker offers Main form and Allocations only: Customers and Orders would loop back, so they are left out.",
        ],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn design_grid_tracks_and_regions() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, (1500.0, 3600.0));
    h.click("new-document");
    // Four columns keep the region editor inside the window.
    seed(&mut h, |config| config.design.forms[0].layout.columns.truncate(4));
    open_designer(&mut h);

    h.click("grid-settings-toggle");
    choose(&mut h, "grid-column-1-kind", "Fixed size (px)");
    h.click("grid-add-region");
    h.fill("grid-region-1-name", "header");
    h.press("enter");
    let layout = form(&h).layout;
    assert_eq!(layout.columns[0].kind, crate::design::TrackKind::Fixed);
    assert_eq!(layout.named_regions.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["header"]);
    h.capture(
        "grid-01-tracks-and-regions",
        &[
            "Form properties list the column widths with Column 1 set to 'Fixed size (px)', its name, kind, and labelled Size, Minimum, and Maximum inputs readable.",
            "A named area 'header' appears in the area editor with its column, row, and span inputs.",
        ],
    );

    h.click("add-label");
    choose(&mut h, "prop-region", "header");
    let label = control(&h, ControlKind::Label).expect("the text control");
    assert_eq!(label.placement.region.as_deref(), Some("header"));
    assert_eq!(h.label("prop-region"), "header");
    h.capture(
        "grid-02-control-in-region",
        &[
            "The Text control's Region select reads 'header'.",
            "Its column and span inputs are disabled, with the hint 'Placed in a region; choose None to set column and row.'",
        ],
    );

    h.click("back-to-form");
    h.fill("grid-region-1-name", "");
    h.press("enter");
    assert_eq!(h.label("grid-region-1-name-error"), "Area name is required.");
    h.capture(
        "grid-03-region-name-required",
        &["The Area 1 name input is empty and shows 'Area name is required.' inline beneath it."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn design_form_builder_and_preview() {
    let mut cx = context();
    let mut h = Harness::start_sized(&mut cx, WINDOW);
    h.click("new-document");
    h.create_table(
        "products",
        &[("id", "INTEGER", 1), ("sku", "TEXT", 0), ("product_name", "TEXT", 0), ("price", "INTEGER", 0)],
    );
    h.insert_row(
        "products",
        &[("id", int(1)), ("sku", text("PROD-101")), ("product_name", text("Desk lamp")), ("price", int(45))],
    );
    open_designer(&mut h);
    generate_form(&mut h, "products");
    wait_until(&mut h, "the generated controls", |h| {
        h.config().design.forms.iter().any(|f| f.controls.iter().any(|c| c.binding.as_ref().is_some_and(|b| b.column == "sku")))
    });
    assert_eq!(h.mode(), crate::ui::shell::ModeId::Design);
    assert!(h.exists("design-studio") && h.exists("add-text") && h.exists("canvas"));
    h.capture(
        "app-qa-06-form-builder",
        &[
            "Design mode shows the production form builder.",
            "The component library, form canvas, and properties panel are visible.",
        ],
    );

    h.click("tab-preview");
    let listed = h.config().design.forms.iter().find(|f| f.name.to_lowercase().starts_with("products")).map(|f| f.list_columns.clone());
    assert!(listed.as_ref().is_some_and(|c| c.iter().any(|c| c == "sku")), "{listed:?}");
    wait_until(&mut h, "the preview to load", |h| !h.exists("preview-loading"));
    let preview = h.label("design-preview");
    assert!(!h.exists("preview-empty"), "{preview}");
    let studio = crate::ui::design::test_support::studio(&mut h);
    wait_until(&mut h, &format!("the product record of {preview}"), |h| {
        let Some(preview) = h.cx.read(|cx| studio.read(cx).preview()) else {
            return false;
        };
        let Some(renderer) = h.cx.read(|cx| preview.read(cx).renderer()) else {
            return false;
        };
        h.cx.read(|cx| {
            renderer
                .read(cx)
                .record()
                .is_some_and(|record| record.read(cx).record().get("sku").and_then(|v| v.as_str()) == Some("PROD-101"))
        })
    });
    h.capture(
        "app-qa-07-app-preview",
        &[
            "The saved Products design is shown in its runtime view.",
            "Bound labels and live product records are visible.",
        ],
    );
}
