//! Port of `tests/integration/golden/report-snapshots.test.tsx`: each template's reports lay
//! out and print to the committed golden layout JSON and PDF in `tests/fixtures/report-goldens`.
//! Run with `UPDATE_GOLDENS=1` to rewrite them.
//!
//! The layout is compared as JSON values (numbers compare by value) after the image asset ids
//! are replaced by `{{asset:n}}`; the PDF is compared byte for byte.

use crate::report_engine::{
    AssetInfo, Item, LayoutOptions, layout_report,
    data::{LoadControl, load_report_data},
    pdf::{PdfAsset, PdfOptions, report_pdf_bytes},
};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

const NOW: &str = "2026-01-15T09:30:00.000Z";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("tests").join("fixtures").join("report-goldens")
}

fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

/// `JSON.stringify(value, null, 1)`.
fn pretty_one_space(value: &Value) -> String {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut ser = serde_json::Serializer::with_formatter(&mut out, formatter);
    value.serialize(&mut ser).expect("serialize");
    String::from_utf8(out).expect("utf8")
}

fn golden_bytes(name: &str, actual: &[u8]) {
    let path = fixtures().join(name);
    if std::env::var("UPDATE_GOLDENS").as_deref() == Ok("1") {
        std::fs::create_dir_all(fixtures()).expect("fixtures dir");
        std::fs::write(&path, actual).expect("write golden");
        return;
    }
    let expected = std::fs::read(&path).unwrap_or_else(|_| panic!("Missing golden {name}; run with UPDATE_GOLDENS=1 to create it"));
    assert!(actual == expected.as_slice(), "{name} differs; run with UPDATE_GOLDENS=1 if intended");
}

fn golden_layout(name: &str, actual: &Value) {
    let path = fixtures().join(name);
    if std::env::var("UPDATE_GOLDENS").as_deref() == Ok("1") {
        std::fs::create_dir_all(fixtures()).expect("fixtures dir");
        std::fs::write(&path, format!("{}\n", pretty_one_space(actual))).expect("write golden");
        return;
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("Missing golden {name}; run with UPDATE_GOLDENS=1 to create it"));
    let expected: Value = serde_json::from_str(&text).expect("golden json");
    assert_eq!(
        super::journey::normalize(actual.clone()),
        super::journey::normalize(expected),
        "{name} differs; run with UPDATE_GOLDENS=1 if intended"
    );
}

fn snapshot_reports(template: &str, template_name: &str, expected_reports: &[&str]) {
    let label = format!("golden-snap-{template}");
    crate::templates::create_from_template(label.clone(), template.into()).expect("create from template");
    let config = crate::read_document_config(label.clone()).expect("config");
    assert_eq!(config.reports.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), expected_reports);
    let now = chrono::DateTime::parse_from_rfc3339(NOW).expect("now").with_timezone(&chrono::Utc);
    for report in &config.reports {
        let params: serde_json::Map<String, Value> = report.params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let data = load_report_data(&label, report, &config, &params, &LoadControl::new(&label)).expect("report data");
        assert!(!data.rows.is_empty(), "{} has seed rows", report.name);
        let options = LayoutOptions {
            params: Some(params),
            now: Some(now),
            tables: data.tables.clone(),
            assets: data.assets.iter().map(|(id, a)| (id.clone(), AssetInfo { media_type: a.media_type.clone() })).collect(),
            subreports: data.subreports.clone(),
        };
        let doc = layout_report(report, &data.rows, &options);
        assert!(doc.diagnostics.is_empty(), "{} lays out without problems: {:?}", report.name, doc.diagnostics);
        let assets = data
            .assets
            .iter()
            .map(|(id, a)| (id.clone(), PdfAsset { media_type: a.media_type.clone(), data: a.data.clone() }))
            .collect();
        let pdf_options = PdfOptions { title: Some(report.name.clone()), creation_date: NOW.into(), ..PdfOptions::default() };
        let (pdf, warnings) = report_pdf_bytes(&doc, &assets, pdf_options).expect("pdf");
        assert!(warnings.is_empty(), "{} images all print: {warnings:?}", report.name);
        let name = format!("{}-{}", slug(template_name), slug(&report.name));
        let mut json = serde_json::to_string(&doc).expect("layout json");
        let mut ids: Vec<String> = vec![];
        for page in &doc.pages {
            for item in &page.items {
                if let Item::Image(image) = item {
                    if !ids.contains(&image.asset_id) {
                        ids.push(image.asset_id.clone());
                    }
                }
            }
        }
        for (n, id) in ids.iter().enumerate() {
            json = json.replace(id.as_str(), &format!("{{{{asset:{}}}}}", n + 1));
        }
        golden_layout(&format!("{name}.layout.json"), &serde_json::from_str(&json).expect("layout"));
        golden_bytes(&format!("{name}.pdf"), &pdf);
    }
    crate::close_document(label, true).expect("close");
}

#[test]
fn golden_crm_activity_report_matches_its_golden_layout_and_pdf() {
    snapshot_reports("crm", "CRM", &["Activity report"]);
}

#[test]
fn golden_inventory_valuation_report_matches_its_golden_layout_and_pdf() {
    snapshot_reports("inventory", "Inventory", &["Inventory valuation"]);
}

#[test]
fn golden_work_order_sheet_matches_its_golden_layout_and_pdf() {
    snapshot_reports("work-orders", "Work orders", &["Work order sheet"]);
}
