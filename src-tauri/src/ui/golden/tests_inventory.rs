//! Port of `tests/integration/golden/inventory.test.tsx`: the Inventory template through its
//! queries, valuation report, dashboard, transactional transfers, concurrency policies and an
//! archive round trip. Report and dashboard steps go through `ReportRun` and the backend (see
//! `tests_crm.rs`).

use super::journey::*;
use crate::data::DataValue;
use gpui_kit::TestAppContext;
use serde_json::json;
use sha2::{Digest, Sha256};

const APP: &str = "inventory";
const PLACEHOLDER_SHA256: &str = "f795a9febe71e68963d920336c389277b1c994aa314bdbf90c6e57376ca13e41";

#[gpui_kit::test]
fn golden_inventory_aggregation_queries_valuation_report_and_low_stock_dashboard(cx: &mut TestAppContext) {
    with_journey(APP, "reads", |journey| {
        let mut h = journey.step("Create Inventory from the start screen", move |_| start_from_template(cx, "inventory"));
        let label = h.label.clone();
        journey.step("Validate definitions", |j| j.check("no error issues", validation_errors(&label), Vec::<String>::new()));
        journey.step("Stock on hand by product", |j| {
            j.check(
                "totals",
                saved_query(&label, "Stock by product", &[]),
                json!([["BOLT-M8", "Hex bolt M8", 1050], ["DRILL-18V", "Cordless drill 18V", 22], ["GLOVE-L", "Work gloves (L)", 233],
                       ["LADDER-6", "Step ladder 6ft", 2], ["NUT-M8", "Hex nut M8", 1200], ["TAPE-25", "Measuring tape 25ft", 47]]),
            );
        });
        journey.step("Stock on hand by product and location (parameter)", |j| {
            j.check(
                "ST1 only",
                saved_query(&label, "Stock on hand", &[("location", json!("ST1"))]),
                json!([["BOLT-M8", "Hex bolt M8", "ST1", 50], ["DRILL-18V", "Cordless drill 18V", "ST1", 2],
                       ["GLOVE-L", "Work gloves (L)", "ST1", 30], ["TAPE-25", "Measuring tape 25ft", "ST1", 1]]),
            );
            j.check("by location", saved_query(&label, "Stock by location", &[]), json!([["ST1", 83, 337.5], ["ST2", 9, 85.5], ["WH1", 2462, 3660]]));
        });
        journey.step("Valuation and low stock", |j| {
            let valuation = saved_query(&label, "Inventory valuation", &[]);
            j.check("valuation total", valuation.iter().map(|r| r[5].as_f64().unwrap_or(0.0)).sum::<f64>(), 4083.0);
            j.check(
                "low stock",
                saved_query(&label, "Low stock", &[]).iter().map(|r| format!("{}@{}:{}<{}", r[0].as_str().unwrap_or(""), r[2].as_str().unwrap_or(""), r[3], r[4])).collect::<Vec<_>>(),
                json!(["BOLT-M8@ST1:50<100", "DRILL-18V@ST1:2<3", "GLOVE-L@ST2:3<5", "TAPE-25@ST1:1<2"]),
            );
        });
        journey.step("Valuation report: page 1 of N, subtotals, total", |j| {
            let (studio, run) = host_report(&mut h, "Inventory valuation", None);
            let pages = report_pages(&mut h, &run);
            let has = |page: &Vec<String>, text: &str| page.iter().any(|t| t == text);
            assert!(has(&pages[0], "Inventory valuation"), "{:?}", pages[0]);
            assert!(has(&pages[0], "Contoso Parts"));
            assert!(has(&pages[0], "Subtotal 382.50"));
            j.check("grand total on the last page", has(pages.last().unwrap(), "Total inventory value: 4,083.00"), true);
            leave_report(&mut h, studio);
        });
        journey.step("Low-stock dashboard with location filter", |j| {
            let mut dash = dashboard(&h, "Stock overview");
            assert_eq!(dash.kpi("Low-stock items"), "4");
            assert_eq!(dash.kpi("Inventory value"), "$4,083.00");
            assert!(dash.options("Location").iter().any(|o| o == "ST2"));
            dash.filter("Location", "ST2");
            assert_eq!(dash.kpi("Inventory value"), "$85.50");
            j.check("ST2 low stock", dash.kpi("Low-stock items"), "1");
        });
    });
}

fn movements(label: &str) -> serde_json::Value {
    scalar(label, "SELECT count(*) FROM stock_movements")
}

fn on_hand(label: &str, sku: &str, code: &str) -> serde_json::Value {
    scalar(
        label,
        &format!("SELECT sum(m.quantity) FROM stock_movements m JOIN products p ON p.id = m.product_id JOIN locations l ON l.id = m.location_id WHERE p.sku = '{sku}' AND l.code = '{code}'"),
    )
}

fn new_transfer(h: &mut Harness, product: &str, from: &str, to: &str, quantity: u32) {
    show_list(h, "Transfers");
    h.click("new-record");
    wait(h);
    choose_related(h, "product", product);
    choose_related(h, "from-location", from);
    choose_related(h, "to-location", to);
    h.fill("field:quantity", &quantity.to_string());
    h.click("create");
    wait(h);
}

fn post(h: &mut Harness) {
    h.click("button:post-transfer");
    wait(h);
}

use crate::ui::testing::Harness;

#[gpui_kit::test]
fn golden_inventory_constraints_transactional_transfers_and_concurrency_policies(cx: &mut TestAppContext) {
    with_journey(APP, "transactions", |journey| {
        let mut h = journey.step("Create Inventory from the start screen", move |_| start_from_template(cx, "inventory"));
        let label = h.label.clone();

        journey.step("Create a product; SKU pattern and unique SKU", |j| {
            show_list(&mut h, "Products");
            h.click("new-record");
            wait(&mut h);
            assert!(h.exists("image:product-photo"));
            h.fill("field:sku", "ab");
            h.fill("field:name", "Claw hammer");
            h.fill("field:unit-cost", "18.5");
            h.click("create");
            wait(&mut h);
            assert_eq!(h.label("field:sku:error"), "Use at least three capital letters, digits, or dashes.");
            h.fill("field:sku", "BOLT-M8");
            h.click("create");
            wait(&mut h);
            expect_alert(&mut h, "Unique constraint failed");
            h.fill("field:sku", "HAMMER-16");
            choose_related(&mut h, "supplier", "Fabrikam Tools");
            h.click("create");
            wait(&mut h);
            assert!(h.exists("edit"), "the product opens as saved: {}", messages(&mut h));
            j.check(
                "stored product",
                sql(&label, "SELECT sku, name, supplier_id, unit_cost, unit, active FROM products WHERE sku = 'HAMMER-16'"),
                json!([["HAMMER-16", "Claw hammer", 3, 18.5, "each", 1]]),
            );
        });

        journey.step("Receive stock through the product's movements list", |j| {
            h.click("related:movements:add");
            wait(&mut h);
            let form = "related:movements:form";
            choose_related_in(&mut h, form, "location", "Downtown store");
            fill_in(&mut h, form, "field:quantity", "12");
            fill_in(&mut h, form, "field:reference", "PO-1007");
            h.click_in(form, "create");
            wait(&mut h);
            assert_eq!(h.label("related:movements:cell:0:reference"), "PO-1007");
            j.check("hammer on hand at ST1", on_hand(&label, "HAMMER-16", "ST1"), 12);
        });

        journey.step("Composite primary key rejects a second threshold", |j| {
            let result = insert_direct(
                &label,
                "reorder_thresholds",
                vec![
                    named("product_id", DataValue::Integer(1)),
                    named("location_id", DataValue::Integer(1)),
                    named("min_quantity", DataValue::Integer(5)),
                ],
            );
            j.check("error code", code_of(result), "CONSTRAINT_VIOLATION");
        });

        journey.step("Post the seeded transfer: both movements in one transaction", |j| {
            let before = movements(&label).as_i64().unwrap_or(0);
            show_list(&mut h, "Transfers");
            open_row(&mut h, "1");
            post(&mut h);
            eventually(&mut h, "the transfer to post", |_| {
                sql(&label, "SELECT status, posted_on IS NOT NULL FROM transfers WHERE id = 1") == vec![vec![json!("posted"), json!(true)]]
            });
            j.check("two movements", movements(&label).as_i64().unwrap_or(0) - before, 2);
            j.check("stock moved", [on_hand(&label, "BOLT-M8", "WH1"), on_hand(&label, "BOLT-M8", "ST1")], [800, 250]);
            j.check(
                "BOLT-M8 is no longer low at ST1",
                saved_query(&label, "Low stock", &[]).iter().map(|r| format!("{}@{}", r[0].as_str().unwrap_or(""), r[2].as_str().unwrap_or(""))).collect::<Vec<_>>(),
                json!(["DRILL-18V@ST1", "GLOVE-L@ST2", "TAPE-25@ST1"]),
            );
        });

        journey.step("A failing transfer leaves no partial rows", |j| {
            let before = movements(&label);
            new_transfer(&mut h, "Hex nut M8", "Main warehouse", "Main warehouse", 10);
            post(&mut h);
            expect_alert(&mut h, "no record changes were saved");
            j.check("no movement written", movements(&label), before);
            j.check("transfer still a draft", sql(&label, "SELECT status FROM transfers WHERE id = 2"), json!([["draft"]]));
        });

        journey.step("Insufficient stock is refused before any write", |j| {
            let before = movements(&label);
            new_transfer(&mut h, "Cordless drill 18V", "Downtown store", "Airport store", 5);
            post(&mut h);
            expect_alert(&mut h, "Not enough stock at the source location: 2 on hand.");
            j.check("no movement written", movements(&label), before);
        });

        journey.step("Optimistic policy on products rejects a stale edit", |j| {
            show_list(&mut h, "Products");
            open_row(&mut h, "BOLT-M8");
            h.click("edit");
            wait(&mut h);
            assert!(h.exists("field:name"), "the edit form is open");
            let changed = update_direct(
                &label,
                "products",
                vec![named("unit_cost", DataValue::Real(0.3))],
                vec![DataValue::Integer(1)],
                vec![named("name", DataValue::Text("Hex bolt M8".into()))],
            )
            .expect("the other change");
            assert_eq!(changed.outcome.changed, 1);
            h.fill("field:name", "Hex bolt M8 zinc");
            h.click("save");
            wait(&mut h);
            expect_alert(&mut h, "Someone else changed this record");
            j.check("the other change is kept", sql(&label, "SELECT name, unit_cost FROM products WHERE id = 1"), json!([["Hex bolt M8", 0.3]]));
        });

        journey.step("Last-write-wins policy on movements accepts a stale edit", |j| {
            let stale = |value: &str| vec![named("note", DataValue::Text(value.into()))];
            let done = update_direct(&label, "stock_movements", stale("Counted"), vec![DataValue::Integer(1)], stale("stale value")).expect("update");
            j.check("updated", done.outcome.changed, 1);
            let conflict = update_direct(
                &label,
                "products",
                vec![named("unit", DataValue::Text("box".into()))],
                vec![DataValue::Integer(1)],
                vec![named("unit", DataValue::Text("stale value".into()))],
            );
            j.check("products still optimistic", code_of(conflict), "CONFLICT");
        });
    });
}

#[gpui_kit::test]
fn golden_inventory_the_product_image_asset_and_records_survive_an_archive_round_trip(cx: &mut TestAppContext) {
    with_journey(APP, "archive-round-trip", |journey| {
        let mut h = journey.step("Create Inventory from the start screen", move |_| start_from_template(cx, "inventory"));
        let label = h.label.clone();
        let assets = |label: &str| crate::list_attachments(label.into()).expect("attachments");
        let asset = assets(&label).remove(0);
        journey.check(
            "template asset imported",
            (&asset.display_name, &asset.media_type, &asset.checksum, asset.size),
            ("product-placeholder.png", "image/png", PLACEHOLDER_SHA256, 89),
        );
        let before = serde_json::to_value(h.config()).expect("config");
        journey.check(
            "product form shows the asset",
            h.config().design.forms.iter().flat_map(|f| &f.controls).any(|c| c.asset_id.as_deref() == Some(asset.id.as_str())),
            true,
        );
        let records = sql(&label, "SELECT * FROM stock_movements ORDER BY id");
        let archive = h.temp_path("inventory-golden.ixt");
        journey.step("Save, close, and reopen", |_| {
            h.dialogs.queue_save(Some(archive.clone()));
            h.click("save-project");
            assert_eq!(h.label("document-status"), "Saved archive");
            h.click("close-project");
            h.dialogs.queue_open(Some(archive.clone()));
            h.click("open-document");
            assert!(h.has_shell());
            wait(&mut h);
        });
        journey.step("Compare asset, definitions, and records", |j| {
            j.check("asset checksum", assets(&h.label).iter().map(|a| (a.id.clone(), a.checksum.clone())).collect::<Vec<_>>(), [(asset.id.clone(), PLACEHOLDER_SHA256.to_string())]);
            j.check("definitions identical", serde_json::to_value(h.config()).expect("config"), before);
            j.check("records identical", sql(&h.label, "SELECT * FROM stock_movements ORDER BY id"), records);
            let exported = h.temp_path("exported.png");
            crate::export_attachment(h.label.clone(), asset.id.clone(), exported.to_string_lossy().into()).expect("export");
            let bytes = std::fs::read(&exported).expect("exported file");
            j.check("exported bytes match", format!("{:x}", Sha256::digest(&bytes)), PLACEHOLDER_SHA256);
        });
    });
}
