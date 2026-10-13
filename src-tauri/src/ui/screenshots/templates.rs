//! Starting from the golden templates (`templates-*`). Port of
//! scripts/screenshot/specs/templates.spec.tsx.
//!
//! The start screen lists the templates (`templates-00`); each template then opens in the
//! Runtime (`templates-01`, `-03`, `-04`) and the CRM's relationship diagram follows in Data
//! mode (`templates-02`).

use crate::ui::screenshot::context;
use crate::ui::runtime::tests::{nav, wait};
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::HeadlessAppContext;

fn from_template<'a>(cx: &'a mut HeadlessAppContext, id: &str, size: (f32, f32)) -> Harness<'a, HeadlessAppContext> {
    let mut h = Harness::start_sized(cx, size);
    h.click(format!("template:{id}"));
    assert!(h.has_shell(), "the template did not open");
    h.switch_mode(ModeId::Run);
    wait(&mut h);
    h
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn templates_start_screen() {
    let mut cx = context();
    let mut h = Harness::start(&mut cx);
    for id in ["crm", "inventory", "work-orders"] {
        assert!(h.exists(format!("template:{id}")), "{id}");
    }
    h.capture(
        "templates-00-start-templates",
        &["The start screen lists the CRM, Inventory, and Work orders templates under 'Start from a template'."],
    );
    h.click("template:crm");
    assert!(h.has_shell());
    assert_eq!(h.config().name, "CRM");
    assert!(h.state().path.is_none());
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn templates_crm() {
    let mut cx = context();
    let mut h = from_template(&mut cx, "crm", (1440.0, 900.0));
    assert_eq!(h.config().name, "CRM");
    assert!(!h.exists("status:error"));
    h.capture(
        "templates-01-crm-runtime",
        &[
            "The CRM opens in the Runtime with its navigation (companies, contacts, deals, activities).",
            "The landing page shows live sample data rather than an empty state.",
        ],
    );
    h.switch_mode(ModeId::Data);
    wait(&mut h);
    let tables = ["companies", "contacts", "deals", "activities"];
    assert!(tables.iter().all(|t| h.exists(format!("node:{t}"))), "the CRM tables are on the canvas");
    h.capture(
        "templates-02-crm-relationships",
        &["Every CRM table appears with labelled relationships between keys."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn templates_inventory() {
    let mut cx = context();
    let mut h = from_template(&mut cx, "inventory", (1280.0, 900.0));
    let products = nav(&mut h, "Products");
    h.click(products);
    wait(&mut h);
    assert!(h.exists("row:0"));
    h.capture(
        "templates-03-inventory-runtime",
        &["The Inventory app opens with its navigation and a list page of sample records."],
    );
}

#[test]
#[ignore = "screenshot spec: run with `cargo screenshot`"]
fn templates_work_orders_dashboard() {
    let mut cx = context();
    let mut h = from_template(&mut cx, "work-orders", (1280.0, 1300.0));
    wait(&mut h);
    h.capture(
        "templates-04-work-orders-dashboard",
        &[
            "The Operations dashboard shows KPI tiles and a chart built from the template's queries.",
            "Navigation groups Maintenance, Reports, and Setup are visible.",
            "The Open work order list table fits its card, assignee and progress columns included.",
        ],
    );
}
