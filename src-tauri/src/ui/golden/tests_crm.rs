//! Port of `tests/integration/golden/crm.test.tsx`: the CRM template through its queries,
//! report, dashboard, Runtime CRUD and an archive round trip.
//!
//! Where the React test used a screen that is not part of this port, the step reads through the
//! backend instead: the Activity report page is hosted as a `ReportRun` (the Runtime's report
//! page builder is not registered here) and the Pipeline dashboard's KPIs are computed from
//! its saved queries with the component expressions (`journey::dashboard`).

use super::journey::*;
use gpui_kit::TestAppContext;
use serde_json::json;

const APP: &str = "crm";

#[gpui_kit::test]
fn golden_crm_seeded_queries_activity_report_and_pipeline_dashboard_show_the_seeds_numbers(cx: &mut TestAppContext) {
    with_journey(APP, "reads", |journey| {
        let mut h = journey.step("Create CRM from the start screen", move |_| start_from_template(cx, "crm"));
        let label = h.label.clone();
        journey.step("Validate definitions", |j| j.check("no error issues", validation_errors(&label), Vec::<String>::new()));
        journey.step("Grouped query: deals by stage", |j| {
            j.check(
                "all stages",
                saved_query(&label, "Deals by stage", &[]),
                json!([["Lead", 2, 12000], ["Qualified", 2, 40000], ["Proposal", 2, 19000], ["Negotiation", 2, 48000], ["Won", 1, 5000], ["Lost", 1, 2000]]),
            );
            j.check(
                "stage parameter filters",
                saved_query(&label, "Deals by stage", &[("stage", json!("Proposal"))]),
                json!([["Proposal", 2, 19000]]),
            );
        });
        journey.step("Filtered query: open deals over 10,000", |j| {
            let rows = saved_query(&label, "Open deals", &[("min_amount", json!(10000))]);
            j.check(
                "titles by amount",
                rows.iter().map(|r| r[1].clone()).collect::<Vec<_>>(),
                json!(["Patient portal", "Grid analytics", "Fleet tracking", "Factory sensors"]),
            );
        });
        journey.step("Open pipeline totals", |j| {
            let rows = saved_query(&label, "Open pipeline", &[]);
            let sum = |i: usize| rows.iter().map(|r| r[i].as_f64().unwrap_or(0.0)).sum::<f64>();
            j.check("open pipeline and weighted value", [sum(2), sum(3)], [119000.0, 56700.0]);
        });
        journey.step("Activity report renders page 1 of N with totals", |j| {
            let (studio, run) = host_report(&mut h, "Activity report", None);
            let pages = report_pages(&mut h, &run);
            let first = &pages[0];
            assert!(first.iter().any(|t| t == "Activities by company"), "{first:?}");
            assert!(first.iter().any(|t| t == "Acme Corp"), "{first:?}");
            assert!(first.iter().any(|t| t == "3 activities, 1 open"), "{first:?}");
            j.check(
                "report total",
                pages.last().is_some_and(|p| p.iter().any(|t| t == "Total activities: 12")),
                true,
            );
            leave_report(&mut h, studio);
        });
        journey.step("Pipeline dashboard: KPIs and stage filter", |j| {
            let mut dash = dashboard(&h, "Pipeline");
            assert_eq!(dash.kpi("Open pipeline"), "$119,000");
            assert_eq!(dash.kpi("Open deals"), "8");
            assert!(dash.table("Open deal list").iter().any(|r| r["title"] == "Patient portal"));
            assert!(dash.options("Stage").iter().any(|o| o == "Proposal"));
            dash.filter("Stage", "Proposal");
            j.check("filtered KPIs", [dash.kpi("Open pipeline"), dash.kpi("Open deals")], ["$19,000", "2"]);
        });
    });
}

#[gpui_kit::test]
fn golden_crm_archive_save_close_and_reopen_keeps_definitions_and_records_identical(cx: &mut TestAppContext) {
    with_journey(APP, "archive-round-trip", |journey| {
        let mut h = journey.step("Create CRM from the start screen", move |_| start_from_template(cx, "crm"));
        let label = h.label.clone();
        let before = serde_json::to_value(h.config()).expect("config");
        let snapshot = |label: &str| {
            ["deal_stages", "companies", "contacts", "deals", "activities"]
                .iter()
                .map(|t| sql(label, &format!("SELECT * FROM {t} ORDER BY id")))
                .collect::<Vec<_>>()
        };
        let records = snapshot(&label);
        let archive = h.temp_path("crm-golden.ixt");
        journey.step("Save as .ixt", |_| {
            h.dialogs.queue_save(Some(archive.clone()));
            h.click("save-project");
            assert_eq!(h.label("document-status"), "Saved");
        });
        journey.step("Close and reopen", |_| {
            h.click("close-project");
            h.dialogs.queue_open(Some(archive.clone()));
            h.click("open-document");
            assert!(h.has_shell());
            wait(&mut h);
        });
        journey.step("Compare definitions and records", |j| {
            j.check("definitions identical", serde_json::to_value(h.config()).expect("config"), before);
            j.check("records identical", snapshot(&h.label), records);
            j.check("still valid", validation_errors(&h.label), Vec::<String>::new());
        });
    });
}

#[gpui_kit::test]
fn golden_crm_runtime_crud_with_master_detail_validation_trigger_action_roles_constraints(cx: &mut TestAppContext) {
    // The action runner is process-wide: hold the real one so a parallel test's double cannot replace it.
    let _runner = crate::ui::runtime::tests::with_real_runner();
    with_journey(APP, "crud", |journey| {
        let mut h = journey.step("Create CRM from the start screen", move |_| start_from_template(cx, "crm"));
        let label = h.label.clone();

        journey.step("Create a company in the Companies form", |j| {
            show_list(&mut h, "Companies");
            assert!(h.try_label("row:0").is_some());
            let _ = row_of(&mut h, "Acme Corp");
            h.click("new-record");
            wait(&mut h);
            assert_eq!(h.label("record-title"), "New company");
            h.fill("field:name", "Wayne Enterprises");
            h.click("field:industry");
            h.click("field:industry:option:manufacturing");
            h.fill("field:website", "wayne.example");
            h.click("create");
            wait(&mut h);
            assert_eq!(h.label("field:website:error"), "Enter a web address starting with http:// or https://.");
            h.fill("field:website", "https://wayne.example");
            h.fill("field:city", "Gotham");
            h.click("create");
            wait(&mut h);
            assert!(h.exists("edit"), "the company opens as saved: {}", messages(&mut h));
            j.check(
                "stored company",
                sql(&label, "SELECT name, industry, city FROM companies WHERE id = 6"),
                json!([["Wayne Enterprises", "Manufacturing", "Gotham"]]),
            );
        });

        journey.step("Add a contact through the master/detail list", |j| {
            h.click("related:contacts:add");
            wait(&mut h);
            let form = "related:contacts:form";
            assert!(!exists_in(&mut h, form, "related:deals"));
            fill_in(&mut h, form, "field:name", "Lucius Fox");
            fill_in(&mut h, form, "field:email", "lucius-at-wayne");
            h.click_in(form, "create");
            wait(&mut h);
            assert_eq!(label_in(&mut h, form, "field:email:error").as_deref(), Some("Enter an email address like name@example.com."));
            fill_in(&mut h, form, "field:email", "lucius@wayne.example");
            h.click_in(form, "create");
            wait(&mut h);
            assert_eq!(h.label("related:contacts:cell:0:name"), "Lucius Fox");
            j.check(
                "contact linked to its company",
                sql(&label, "SELECT company_id, email FROM contacts WHERE name = 'Lucius Fox'"),
                json!([[6, "lucius@wayne.example"]]),
            );
        });

        journey.step("Add a deal: relationship selectors, validation, sync trigger", |j| {
            h.click("related:deals:add");
            wait(&mut h);
            let form = "related:deals:form";
            fill_in(&mut h, form, "field:title", "Bat signal");
            choose_related_in(&mut h, form, "stage", "Proposal");
            choose_related_in(&mut h, form, "contact", "Lucius Fox");
            fill_in(&mut h, form, "field:amount", "-5");
            h.click_in(form, "create");
            wait(&mut h);
            assert_eq!(label_in(&mut h, form, "field:amount:error").as_deref(), Some("Amount cannot be negative."));
            fill_in(&mut h, form, "field:amount", "50000");
            h.click_in(form, "create");
            wait(&mut h);
            assert_eq!(h.label("related:deals:cell:0:title"), "Bat signal");
            j.check(
                "stored deal",
                sql(&label, "SELECT d.title, s.name, d.amount, d.status FROM deals d JOIN deal_stages s ON s.id = d.stage_id WHERE d.company_id = 6"),
                json!([["Bat signal", "Proposal", 50000, "open"]]),
            );
            j.check(
                "the deal trigger logged an activity in the same workflow",
                sql(&label, "SELECT kind, subject, done FROM activities WHERE company_id = 6"),
                json!([["note", "Deal created: Bat signal", 1]]),
            );
        });

        journey.step("Duplicate company name surfaces the unique constraint", |j| {
            h.click("back-crumb-0");
            wait(&mut h);
            h.click("new-record");
            wait(&mut h);
            h.fill("field:name", "Acme Corp");
            h.click("create");
            wait(&mut h);
            expect_alert(&mut h, "Unique constraint failed");
            j.check("no duplicate stored", scalar(&label, "SELECT count(*) FROM companies"), 6);
        });

        journey.step("Check constraint rejects a negative amount written directly", |j| {
            let int = |v| crate::data::DataValue::Integer(v);
            let named = |c: &str, v| crate::data::NamedValue { column: c.into(), value: v };
            let result = crate::recordstore::commands::insert_row(
                label.clone(),
                "deals".into(),
                vec![
                    named("company_id", int(1)),
                    named("stage_id", int(1)),
                    named("title", crate::data::DataValue::Text("Bad".into())),
                    named("amount", crate::data::DataValue::Real(-1.0)),
                ],
                None,
            );
            j.check("error code", code_of(result), "CONSTRAINT_VIOLATION");
        });

        journey.step("Mark deal won (transactional action)", |j| {
            h.click("cancel");
            wait(&mut h);
            show_list(&mut h, "Deals");
            open_row(&mut h, "Bat signal");
            h.click("button:mark-deal-won");
            wait(&mut h);
            eventually(&mut h, "the deal to be won", |_| {
                sql(&label, "SELECT status FROM deals WHERE title = 'Bat signal'") == vec![vec![json!("won")]]
            });
            j.check(
                "stage, close date, and won activity",
                sql(&label, "SELECT s.name, d.close_date IS NOT NULL, (SELECT count(*) FROM activities a WHERE a.deal_id = d.id AND a.subject = 'Deal won: Bat signal') FROM deals d JOIN deal_stages s ON s.id = d.stage_id WHERE d.title = 'Bat signal'"),
                json!([["Won", true, 1]]),
            );
        });

        journey.step("Edit a contact and delete it from the related list", |j| {
            show_list(&mut h, "Companies");
            open_row(&mut h, "Wayne Enterprises");
            assert_eq!(h.label("related:contacts:cell:0:name"), "Lucius Fox");
            h.click("related:contacts:edit:0");
            wait(&mut h);
            let form = "related:contacts:form";
            fill_in(&mut h, form, "field:job-title", "CEO");
            h.click_in(form, "save");
            wait(&mut h);
            assert_eq!(h.label("related:contacts:cell:0:title"), "CEO");
            h.dialogs.queue_confirm(true);
            h.click("related:contacts:delete:0");
            wait(&mut h);
            assert_eq!(h.label("related:contacts:empty"), "No contacts yet.");
            j.check(
                "contact deleted, deal kept with no contact (ON DELETE SET NULL)",
                sql(&label, "SELECT (SELECT count(*) FROM contacts WHERE company_id = 6), contact_id FROM deals WHERE company_id = 6"),
                json!([[0, null]]),
            );
        });

        journey.step("Sales rep role: no delete on companies", |j| {
            crate::ui::runtime::tests::run_as(&mut h, "Sales rep");
            wait(&mut h);
            show_list(&mut h, "Companies");
            open_row(&mut h, "Acme Corp");
            assert!(h.exists("edit"));
            j.check("delete hidden", h.exists("delete"), false);
            let config = h.config();
            let has_nav = |label: &str| {
                fn find(items: &[crate::design::NavigationItem], label: &str) -> bool {
                    items.iter().any(|n| n.label == label || find(&n.children, label))
                }
                find(&config.design.navigation, label)
            };
            assert!(has_nav("Deal stages"));
            let stages = nav(&mut h, "Deal stages");
            assert!(!h.exists(stages), "Deal stages is hidden for the Sales rep");
            crate::ui::runtime::tests::run_as(&mut h, crate::ui::runtime::window::OWNER);
            wait(&mut h);
        });

        journey.step("Delete a company: foreign keys cascade", |j| {
            show_list(&mut h, "Companies");
            open_row(&mut h, "Wayne Enterprises");
            h.dialogs.queue_confirm(true);
            h.click("delete");
            wait(&mut h);
            eventually(&mut h, "the company to be gone", |_| scalar(&label, "SELECT count(*) FROM companies") == json!(5));
            j.check(
                "deals and activities of the company are gone",
                sql(&label, "SELECT (SELECT count(*) FROM deals), (SELECT count(*) FROM activities)"),
                json!([[10, 12]]),
            );
        });
    });
}
