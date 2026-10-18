//! Reports mode flows (port of `tests/integration/report.test.tsx`). Imports are explicit:
//! `gpui_kit::*` would shadow `#[test]`.
//!
//! The designer is driven through the window like a user would. A component is found by
//! its stored id (`component-<id>`), read back from the config.

use super::test_support::{
    add_component, add_query, choose, id, last_component, new_report, page_texts, pick_orders_dataset, report,
    seed_orders, validation_issues_for_reports, wait_for_page, wait_until,
};
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;

/// A new document in a window tall enough to show the whole properties panel (the
/// harness only clicks what is visible).
pub fn tall_document(cx: &mut TestAppContext) -> Harness<'_> {
    let mut h = Harness::start_sized(cx, (1500.0, 2600.0));
    h.click("new-document");
    assert!(h.has_shell(), "New document did not open the shell");
    h
}

#[gpui_kit::test]
fn builds_a_grouped_report_previews_page_1_of_n_with_totals_prints_and_exports_identical_pdfs(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    seed_orders(&mut h);
    add_query(&mut h, "q-orders", "Orders", "SELECT * FROM orders");
    new_report(&mut h);

    pick_orders_dataset(&mut h);
    choose(&mut h, "group-1-field", "region");
    let group = report(&h).bands.groups[0].clone();
    assert_eq!(group.group_by, "record.region");

    add_component(&mut h, "reportHeader", "staticText");
    h.fill("prop-text", "Sales by region");

    add_component(&mut h, &format!("groupHeader:{}", group.id), "field");
    choose(&mut h, "prop-bound-field", "region");

    add_component(&mut h, "detail", "field");
    choose(&mut h, "prop-bound-field", "customer");
    add_component(&mut h, "detail", "field");
    choose(&mut h, "prop-bound-field", "amount");

    add_component(&mut h, &format!("groupFooter:{}", group.id), "calculated");
    h.fill("prop-expression", "'Subtotal ' & sum(rows.amount)");

    add_component(&mut h, "reportFooter", "calculated");
    h.fill("prop-expression", "'Grand total ' & format(sum(rows.amount), '#,##0.00')");
    let grand = last_component(&h, |r| &r.bands.report_footer);
    h.click(id(&format!("component-{}", grand.id)));
    h.press("shift-right");
    h.press("down");
    let moved = last_component(&h, |r| &r.bands.report_footer);
    assert_eq!((moved.x, moved.y), (10.0, 1.0));

    add_component(&mut h, "pageFooter", "calculated");
    h.fill("prop-expression", "'Page ' & page & ' of ' & pages");

    h.click("tab-preview");
    wait_for_page(&mut h, "Page 1 of 2");
    let first = page_texts(&mut h);
    for want in ["Sales by region", "East", "Subtotal 590"] {
        assert!(first.iter().any(|t| t == want), "{want} missing from {first:?}");
    }
    h.click("next-page");
    wait_for_page(&mut h, "Page 2 of 2");
    let last = page_texts(&mut h);
    assert!(last.iter().any(|t| t == "Grand total 1,830.00"), "{last:?}");
    assert!(last.iter().any(|t| t == "Page 2 of 2"), "{last:?}");

    // Print writes the same PDF as Export PDF to a file chosen in the save dialog.
    let (first_pdf, second_pdf) = (h.temp_path("report-a.pdf"), h.temp_path("report-b.pdf"));
    h.dialogs.queue_save(Some(first_pdf.clone()));
    h.click("print");
    wait_until(&mut h, "the print export", |h| h.notice().is_some());
    h.dialogs.queue_save(Some(second_pdf.clone()));
    h.click("export-pdf");
    let want = "Exported report-b.pdf".to_string();
    wait_until(&mut h, "the second export", |h| h.notice().as_deref() == Some(want.as_str()));
    h.click("notice-action");
    h.settle();
    assert_eq!(h.dialogs.count("reveal"), 1);
    let a = std::fs::read(&first_pdf).expect("first pdf");
    let b = std::fs::read(&second_pdf).expect("second pdf");
    assert_eq!(&a[..8], b"%PDF-1.4");
    let text: String = a.iter().map(|c| *c as char).collect();
    assert!(text.contains("(Grand total 1,830.00) Tj"));
    assert_eq!(a, b);

    assert_eq!(h.config().reports.len(), 1);
    assert_eq!(validation_issues_for_reports(&h), Vec::<String>::new());
}

#[gpui_kit::test]
fn renames_duplicates_and_deletes_reports_through_the_config_store(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    new_report(&mut h);
    h.fill("report-name", "Invoices");
    assert_eq!(h.config().reports[0].name, "Invoices");
    h.click("duplicate-report");
    assert_eq!(h.label("mode-title"), "Invoices copy");
    // The copy gets a page of its own, which goes with it.
    let page_names = |h: &Harness| h.config().pages.into_iter().map(|p| p.name).collect::<Vec<_>>();
    assert!(page_names(&h).contains(&"Invoices copy".to_string()));
    h.click("delete-report");
    assert!(!page_names(&h).contains(&"Invoices copy".to_string()));
    let names: Vec<String> = h.config().reports.into_iter().map(|r| r.name).collect();
    assert_eq!(names, ["Invoices"]);
}

#[gpui_kit::test]
fn sets_pagination_controls_in_the_designer_and_keeps_tables_out_of_page_bands(cx: &mut TestAppContext) {
    let mut h = tall_document(cx);
    seed_orders(&mut h);
    add_query(&mut h, "q-orders", "Orders", "SELECT * FROM orders");
    new_report(&mut h);
    pick_orders_dataset(&mut h);
    choose(&mut h, "group-1-field", "region");

    // A group that starts a new page, switched on and off again.
    let group_new_page = |h: &Harness| report(h).bands.groups[0].new_page;
    h.click("group-1-newPage");
    assert!(group_new_page(&h));
    h.click("group-1-newPage");
    assert!(!group_new_page(&h));
    // Restarting group page numbers forces a new page, and the checkbox can't be cleared.
    h.click("group-1-resetPageNumber");
    assert!(report(&h).bands.groups[0].reset_page_number);
    assert_eq!(h.label("group-1-newpage-hint"), "Always on while group page numbers restart.");
    h.click("group-1-newPage");
    assert!(!report(&h).bands.groups[0].new_page);

    h.click("band-reportHeader");
    assert_eq!(h.label("band-properties"), "Report header");
    h.click("band-break-after");
    assert!(report(&h).bands.report_header.page_break_after);

    // Page bands refuse tables and subreports.
    h.click("band-pageFooter");
    assert_eq!(
        h.label("table-hint"),
        "Tables and subreports are not supported in page headers or footers."
    );
    h.click("add-table");
    h.click("add-subreport");
    assert!(report(&h).bands.page_footer.components.is_empty());
    assert!(!h.exists("band-break-before"));
    h.click("add-calculated");
    h.fill("prop-expression", "'Part ' & groupPage & ' of ' & groupPages & ' / ' & page");

    h.click("tab-preview");
    wait_for_page(&mut h, "Page 1 of 4");
    assert!(page_texts(&mut h).iter().any(|t| t == "Part 1 of 1 / 1"));
    h.click("next-page");
    wait_for_page(&mut h, "Page 2 of 4");
    assert!(page_texts(&mut h).iter().any(|t| t == "Part 1 of 1 / 2"));

    let report = report(&h);
    assert!(report.bands.report_header.page_break_after);
    let kinds: Vec<&str> = report.bands.page_footer.components.iter().map(|c| c.kind.as_str()).collect();
    assert_eq!(kinds, ["calculated"]);
    assert!(report.bands.groups[0].reset_page_number);
    assert!(!report.bands.groups[0].new_page);
    assert_eq!(validation_issues_for_reports(&h), Vec::<String>::new());
}

#[gpui_kit::test]
fn lets_a_text_box_grow_with_its_text_in_the_designer_and_preview(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    new_report(&mut h);
    add_component(&mut h, "reportHeader", "staticText");
    h.fill("prop-text", "Quarterly notes for the regional sales review meeting");
    assert!(!last_component(&h, |r| &r.bands.report_header).extra.contains_key("canGrow"));

    h.click("tab-preview");
    wait_for_page(&mut h, "Page 1 of 1");
    let clipped = page_texts(&mut h);
    assert!(clipped.iter().any(|t| t == "Quarterly notes for the"), "{clipped:?}");
    assert!(!clipped.iter().any(|t| t == "meeting"), "{clipped:?}");

    h.click("tab-design");
    let text = last_component(&h, |r| &r.bands.report_header);
    h.click(id(&format!("component-{}", text.id)));
    h.click("prop-can-grow");
    assert_eq!(
        last_component(&h, |r| &r.bands.report_header).extra.get("canGrow"),
        Some(&serde_json::json!(true))
    );
    h.click("tab-preview");
    wait_until(&mut h, "the grown text", |h| page_texts(h).iter().any(|t| t == "meeting"));
}
