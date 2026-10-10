//! Report definitions (PRD §15): a freeform, band-based report canvas.
//!
//! Rust only stores and validates report definitions and writes exported PDF
//! bytes; layout, pagination, expression evaluation and PDF generation live in
//! the TypeScript engine (`src/reports/engine`, `src/reports/pdf.ts`).
use crate::archive::{check_named_ids, DocumentConfig, Issue};
use crate::manager::AppError;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Component kinds the canvas supports. Anything else is rejected by `validate`.
pub const COMPONENT_KINDS: [&str; 8] = [
    "staticText",
    "field",
    "calculated",
    "image",
    "line",
    "rectangle",
    "table",
    "subreport",
];

/// Levels of subreports a report may nest below itself (PRD Phase 7).
pub const MAX_SUBREPORT_DEPTH: usize = 3;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub id: String,
    pub name: String,
    /// Saved query that supplies the report rows. Takes precedence over `table`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset_query_id: Option<String>,
    /// Table or view read in full when no saved query is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub table: Option<String>,
    /// Default parameter values, passed to the dataset query and exposed as `params`.
    #[serde(default)]
    pub params: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub page: PageSetup,
    #[serde(default)]
    pub bands: Bands,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageSetup {
    /// "A4" or "Letter".
    pub size: String,
    /// "portrait" or "landscape".
    pub orientation: String,
    pub margins: Margins,
}
impl Default for PageSetup {
    fn default() -> Self {
        Self {
            size: "A4".into(),
            orientation: "portrait".into(),
            margins: Margins::default(),
        }
    }
}
impl PageSetup {
    /// Page width and height in points.
    pub fn dimensions(&self) -> (f64, f64) {
        let (w, h) = if self.size == "Letter" {
            (612.0, 792.0)
        } else {
            (595.0, 842.0)
        };
        if self.orientation == "landscape" {
            (h, w)
        } else {
            (w, h)
        }
    }
    pub fn content_width(&self) -> f64 {
        self.dimensions().0 - self.margins.left - self.margins.right
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Margins {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}
impl Default for Margins {
    fn default() -> Self {
        Self {
            top: 36.0,
            right: 36.0,
            bottom: 36.0,
            left: 36.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bands {
    #[serde(default)]
    pub report_header: Band,
    #[serde(default)]
    pub page_header: Band,
    /// Outer group first. Each group has its own header and footer band.
    #[serde(default)]
    pub groups: Vec<ReportGroup>,
    #[serde(default)]
    pub detail: Band,
    #[serde(default)]
    pub page_footer: Band,
    #[serde(default)]
    pub report_footer: Band,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportGroup {
    pub id: String,
    /// Expression evaluated per row (`record.region`).
    pub group_by: String,
    #[serde(default)]
    pub descending: bool,
    #[serde(default)]
    pub header: Band,
    #[serde(default)]
    pub footer: Band,
    /// Start a new page for every group instance.
    #[serde(default, skip_serializing_if = "is_false")]
    pub new_page: bool,
    /// Repeat the group header at the top of continuation pages.
    #[serde(default, skip_serializing_if = "is_false")]
    pub repeat_header: bool,
    /// Restart `groupPage`/`groupPages` numbering for every group instance.
    #[serde(default, skip_serializing_if = "is_false")]
    pub reset_page_number: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Band {
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub keep_together: bool,
    /// Start a new page before this band.
    #[serde(default, skip_serializing_if = "is_false")]
    pub page_break_before: bool,
    /// Start a new page after this band.
    #[serde(default, skip_serializing_if = "is_false")]
    pub page_break_after: bool,
    #[serde(default)]
    pub components: Vec<Component>,
}

/// A positioned component. Kind-specific settings (text, expression, format,
/// style, columns, …) are kept verbatim in `extra`; the TS engine owns them.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Component {
    pub id: String,
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

const PAGE_HEADER: &str = "Page header";
const PAGE_FOOTER: &str = "Page footer";

fn band_issues(
    report: &Report,
    label: &str,
    band: &Band,
    width: f64,
    config: &DocumentConfig,
    ids: &mut HashSet<String>,
    issues: &mut Vec<Issue>,
) {
    let page_band = label == PAGE_HEADER || label == PAGE_FOOTER;
    let id = report.id.as_str();
    let err = |m: String| Issue::error("report", id, format!("{}: {m}", report.name));
    if !band.height.is_finite() || band.height < 0.0 {
        issues.push(err(format!("{label} height must be zero or more")));
    }
    let mut tables = 0;
    for c in &band.components {
        let what = format!("{label} component {}", c.id);
        if c.id.trim().is_empty() {
            issues.push(err(format!("{label} has a component without an id")));
        } else if !ids.insert(c.id.clone()) {
            issues.push(err(format!("duplicate component id {}", c.id)));
        }
        if !COMPONENT_KINDS.contains(&c.kind.as_str()) {
            issues.push(err(format!("{what} has unsupported kind \"{}\"", c.kind)));
        }
        let finite = [c.x, c.y, c.w, c.h].iter().all(|v| v.is_finite());
        if !finite || c.w <= 0.0 || c.h <= 0.0 {
            issues.push(err(format!("{what} must have a positive width and height")));
        } else if c.x < 0.0
            || c.y < 0.0
            || c.x + c.w > width + 0.01
            || c.y + c.h > band.height + 0.01
        {
            issues.push(err(format!("{what} lies outside the band")));
        }
        if c.kind == "table" && page_band {
            // A warning, not an error: older documents kept such tables and layout drops them.
            issues.push(Issue::warning(
                "report",
                id,
                format!(
                    "{}: {what}: tables are not supported in page headers or footers and are left out",
                    report.name
                ),
            ));
        }
        if c.kind == "table" {
            tables += 1;
            if let Some(q) = c.extra.get("queryId").and_then(|v| v.as_str()) {
                if !q.is_empty() && !config.saved_queries.iter().any(|s| s.id == q) {
                    issues.push(err(format!(
                        "{what} uses a saved query that does not exist"
                    )));
                }
            }
        }
    }
    if tables > 1 {
        issues.push(err(format!("{label} has more than one table")));
    }
    let subreports: Vec<&Component> = band
        .components
        .iter()
        .filter(|c| c.kind == "subreport")
        .collect();
    for c in &subreports {
        let what = format!("{label} component {}", c.id);
        if page_band {
            // Layout leaves it out, like a table in a page band.
            issues.push(Issue::warning(
                "report",
                id,
                format!(
                    "{}: {what}: subreports are not supported in page headers or footers and are left out",
                    report.name
                ),
            ));
        }
        let target = c
            .extra
            .get("reportId")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !target.is_empty() && !config.reports.iter().any(|r| r.id == target) {
            issues.push(err(format!("{what} prints a report that does not exist")));
        }
    }
    if subreports.len() > 1 {
        issues.push(err(format!("{label} has more than one subreport")));
    }
    if !subreports.is_empty() && tables > 0 {
        issues.push(err(format!("{label} has both a table and a subreport")));
    }
}

/// Report ids a report's subreports print.
fn subreport_ids(report: &Report) -> Vec<&str> {
    let b = &report.bands;
    let groups = b.groups.iter().flat_map(|g| [&g.header, &g.footer]);
    [
        &b.report_header,
        &b.page_header,
        &b.detail,
        &b.page_footer,
        &b.report_footer,
    ]
    .into_iter()
    .chain(groups)
    .flat_map(|band| band.components.iter())
    .filter(|c| c.kind == "subreport")
    .filter_map(|c| c.extra.get("reportId")?.as_str())
    .filter(|id| !id.is_empty())
    .collect()
}

/// Levels of subreports below `id` (0 without any); `None` when they loop.
fn subreport_depth<'a>(
    reports: &HashMap<&'a str, Vec<&'a str>>,
    id: &'a str,
    path: &mut Vec<&'a str>,
) -> Option<usize> {
    if path.contains(&id) {
        return None;
    }
    let children = reports.get(id).map(Vec::as_slice).unwrap_or_default();
    path.push(id);
    let mut depth = 0;
    for child in children {
        depth = depth.max(1 + subreport_depth(reports, child, path)?);
    }
    path.pop();
    Some(depth)
}

/// Checks every report: dataset exists, unique component ids, components
/// inside their band, positive sizes, supported page setup.
pub fn validate(config: &DocumentConfig) -> Vec<Issue> {
    let mut issues = check_named_ids(
        "report",
        config
            .reports
            .iter()
            .map(|r| (r.id.as_str(), r.name.as_str())),
    );
    let nesting: HashMap<&str, Vec<&str>> = config
        .reports
        .iter()
        .map(|r| (r.id.as_str(), subreport_ids(r)))
        .collect();
    for report in &config.reports {
        let id = report.id.as_str();
        let err = |m: &str| Issue::error("report", id, format!("{}: {m}", report.name));
        match subreport_depth(&nesting, id, &mut Vec::new()) {
            None => issues.push(err("its subreports print each other in a loop")),
            Some(depth) if depth > MAX_SUBREPORT_DEPTH => issues.push(err(&format!(
                "its subreports nest {depth} levels deep (at most {MAX_SUBREPORT_DEPTH})"
            ))),
            Some(_) => {}
        }
        // A report without a dataset is allowed: it renders its bands once, with no rows.
        if let Some(q) = report.dataset_query_id.as_ref().filter(|q| !q.is_empty()) {
            if !config.saved_queries.iter().any(|s| &s.id == q) {
                issues.push(err("dataset saved query does not exist"));
            }
        }
        let page = &report.page;
        if page.size != "A4" && page.size != "Letter" {
            issues.push(err("page size must be A4 or Letter"));
        }
        if page.orientation != "portrait" && page.orientation != "landscape" {
            issues.push(err("orientation must be portrait or landscape"));
        }
        let m = &page.margins;
        let (pw, ph) = page.dimensions();
        if [m.top, m.right, m.bottom, m.left]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0)
            || m.left + m.right >= pw
            || m.top + m.bottom >= ph
        {
            issues.push(err("margins leave no room for content"));
        }
        let width = page.content_width();
        let b = &report.bands;
        let mut ids = HashSet::new();
        let mut bands: Vec<(String, &Band)> = vec![
            ("Report header".into(), &b.report_header),
            (PAGE_HEADER.into(), &b.page_header),
        ];
        let mut group_ids = HashSet::new();
        for (i, g) in b.groups.iter().enumerate() {
            if g.id.trim().is_empty() || !group_ids.insert(g.id.as_str()) {
                issues.push(err("groups need unique ids"));
            }
            if g.group_by.trim().is_empty() {
                issues.push(err(&format!("group {} has no group-by expression", i + 1)));
            }
            bands.push((format!("Group {} header", i + 1), &g.header));
            bands.push((format!("Group {} footer", i + 1), &g.footer));
        }
        bands.push(("Detail".into(), &b.detail));
        bands.push((PAGE_FOOTER.into(), &b.page_footer));
        bands.push(("Report footer".into(), &b.report_footer));
        for (label, band) in bands {
            band_issues(report, &label, band, width, config, &mut ids, &mut issues);
        }
        if b.page_header.height + b.page_footer.height >= ph - m.top - m.bottom {
            issues.push(err("page header and footer leave no room for the body"));
        }
    }
    issues
}

/// Writes PDF bytes produced by the TS PDF writer to `path` (Export PDF…).
/// A runtime role must be allowed to read `report_id`.
#[tauri::command]
pub fn write_report_pdf(
    window_label: String,
    path: String,
    bytes_base64: String,
    report_id: Option<String>,
) -> Result<(), AppError> {
    crate::manager()?.state(&window_label)?;
    match &report_id {
        Some(id) => crate::authz::check(&window_label, "report", id, crate::authz::Op::Read)?,
        None => crate::authz::require_unrestricted(&window_label, "export this PDF")?,
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(bytes_base64.as_bytes())
        .map_err(|e| AppError::new("VALIDATION_ERROR", format!("invalid PDF data: {e}")))?;
    if !bytes.starts_with(b"%PDF-") {
        return Err(AppError::new(
            "VALIDATION_ERROR",
            "data is not a PDF document",
        ));
    }
    std::fs::write(&path, bytes).map_err(|e| AppError::new("IO_ERROR", e))
}

/// An application asset as the report engine needs it (images).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportAsset {
    pub id: String,
    pub media_type: String,
    pub data_base64: String,
}

/// Reads application assets (attachments) by id for report images.
#[tauri::command]
pub fn read_report_assets(
    window_label: String,
    ids: Vec<String>,
) -> Result<Vec<ReportAsset>, AppError> {
    let m = crate::manager()?;
    let list = m.asset_list(&window_label)?;
    collect_report_assets(list, &ids, |id| {
        std::fs::read(m.asset_path(&window_label, id)?).map_err(|e| AppError::new("IO_ERROR", e))
    })
}

/// Reads only the requested assets (each once), never the whole asset set.
fn collect_report_assets(
    list: Vec<crate::archive::Attachment>,
    ids: &[String],
    mut read: impl FnMut(&str) -> Result<Vec<u8>, AppError>,
) -> Result<Vec<ReportAsset>, AppError> {
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .filter(|a| ids.contains(&a.id) && seen.insert(a.id.clone()))
        .map(|a| {
            let bytes = read(&a.id)?;
            Ok(ReportAsset {
                data_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
                id: a.id,
                media_type: a.media_type,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::SavedQuery;
    use serde_json::json;

    #[test]
    fn report_assets_read_only_the_requested_ids() {
        let asset = |id: &str| crate::archive::Attachment {
            id: id.into(),
            display_name: id.into(),
            media_type: "image/png".into(),
            checksum: String::new(),
            size: 0,
            created_at: String::new(),
            updated_at: String::new(),
            contents: vec![],
        };
        let list = vec![asset("a"), asset("big"), asset("c")];
        let mut reads = vec![];
        let out = collect_report_assets(list, &["c".into(), "a".into(), "c".into()], |id| {
            reads.push(id.to_string());
            Ok(id.as_bytes().to_vec())
        })
        .unwrap();
        assert_eq!(reads, ["a", "c"]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].data_base64, "Yw==");
    }

    fn component(id: &str, kind: &str, x: f64, y: f64, w: f64, h: f64) -> Component {
        Component {
            id: id.into(),
            kind: kind.into(),
            x,
            y,
            w,
            h,
            ..Default::default()
        }
    }

    fn config_with(report: Report) -> DocumentConfig {
        let mut config = DocumentConfig::default();
        config.saved_queries.push(SavedQuery {
            id: "q1".into(),
            name: "Orders".into(),
            sql: "SELECT 1".into(),
            ..Default::default()
        });
        config.reports.push(report);
        config
    }

    fn valid_report() -> Report {
        let mut r = Report {
            id: "r1".into(),
            name: "Sales".into(),
            dataset_query_id: Some("q1".into()),
            ..Default::default()
        };
        r.bands.detail = Band {
            height: 20.0,
            keep_together: false,
            components: vec![component("c1", "field", 0.0, 0.0, 100.0, 20.0)],
            ..Default::default()
        };
        r
    }

    #[test]
    fn reports_valid_definition_has_no_issues() {
        assert!(validate(&config_with(valid_report())).is_empty());
    }

    #[test]
    fn reports_round_trip_keeps_component_settings() {
        let raw = json!({
            "id": "r1", "name": "Sales", "datasetQueryId": "q1",
            "page": {"size": "Letter", "orientation": "landscape",
                     "margins": {"top": 10, "right": 10, "bottom": 10, "left": 10}},
            "bands": {"detail": {"height": 20, "keepTogether": true, "components": [
                {"id": "c1", "kind": "field", "x": 0.0, "y": 0.0, "w": 50.0, "h": 12.0,
                 "expression": "record.amount", "format": "#,##0.00", "style": {"bold": true}}
            ]}}
        });
        let report: Report = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(report.page.dimensions(), (792.0, 612.0));
        let c = &report.bands.detail.components[0];
        assert_eq!(c.extra["expression"], json!("record.amount"));
        let back = serde_json::to_value(&report).unwrap();
        assert_eq!(
            back["bands"]["detail"]["components"][0],
            raw["bands"]["detail"]["components"][0]
        );
        let minimal: Report = serde_json::from_value(json!({"id": "r", "name": "n"})).unwrap();
        assert_eq!(minimal.page, PageSetup::default());
    }

    #[test]
    fn reports_validate_flags_missing_dataset_and_bad_components() {
        let mut r = valid_report();
        r.dataset_query_id = Some("missing".into());
        r.bands
            .detail
            .components
            .push(component("c1", "field", 0.0, 0.0, 10.0, 10.0));
        r.bands
            .detail
            .components
            .push(component("c2", "html", 0.0, 0.0, 10.0, 10.0));
        r.bands
            .detail
            .components
            .push(component("c3", "line", 500.0, 0.0, 100.0, 10.0));
        r.bands
            .detail
            .components
            .push(component("c4", "rectangle", 0.0, 0.0, 0.0, 10.0));
        let issues = validate(&config_with(r));
        let text: Vec<String> = issues.iter().map(|i| i.message.clone()).collect();
        let has = |s: &str| text.iter().any(|m| m.contains(s));
        assert!(has("dataset saved query does not exist"), "{text:?}");
        assert!(has("duplicate component id c1"), "{text:?}");
        assert!(has("unsupported kind \"html\""), "{text:?}");
        assert!(has("c3 lies outside the band"), "{text:?}");
        assert!(has("c4 must have a positive width and height"), "{text:?}");
        assert!(issues.iter().all(|i| i.object_id == "r1"));
    }

    #[test]
    fn reports_validate_page_groups_and_tables() {
        let mut r = valid_report();
        r.page.size = "A3".into();
        r.bands.groups.push(ReportGroup {
            id: "g1".into(),
            group_by: " ".into(),
            ..Default::default()
        });
        let mut t = component("t1", "table", 0.0, 0.0, 100.0, 20.0);
        t.extra.insert("queryId".into(), json!("nope"));
        r.bands.report_footer = Band {
            height: 40.0,
            keep_together: false,
            components: vec![t, component("t2", "table", 0.0, 20.0, 100.0, 20.0)],
            ..Default::default()
        };
        let issues = validate(&config_with(r));
        let text: Vec<String> = issues.iter().map(|i| i.message.clone()).collect();
        let has = |s: &str| text.iter().any(|m| m.contains(s));
        assert!(has("page size must be A4 or Letter"), "{text:?}");
        assert!(has("group 1 has no group-by expression"), "{text:?}");
        assert!(has("t1 uses a saved query that does not exist"), "{text:?}");
        assert!(has("Report footer has more than one table"), "{text:?}");
    }

    /// A report `id` whose detail band prints the report `child` as a subreport.
    fn printing(id: &str, child: Option<&str>) -> Report {
        let mut r = valid_report();
        r.id = id.into();
        r.name = id.into();
        if let Some(child) = child {
            let mut sub = component("s1", "subreport", 0.0, 20.0, 100.0, 40.0);
            sub.extra.insert("reportId".into(), json!(child));
            sub.extra.insert(
                "links".into(),
                json!([{"child": "order_id", "master": "id"}]),
            );
            r.bands.detail.height = 60.0;
            r.bands.detail.components.push(sub);
        }
        r
    }

    #[test]
    fn reports_subreports_nest_three_levels_without_loops() {
        // a → b → c → d: three levels below a.
        let mut config = config_with(printing("a", Some("b")));
        config.reports.push(printing("b", Some("c")));
        config.reports.push(printing("c", Some("d")));
        config.reports.push(printing("d", None));
        assert!(validate(&config).is_empty(), "{:?}", validate(&config));
        let back: Report =
            serde_json::from_value(serde_json::to_value(&config.reports[0]).unwrap()).unwrap();
        assert_eq!(back, config.reports[0]);
        // A fourth level is too deep for a only.
        config.reports[3] = printing("d", Some("e"));
        config.reports.push(printing("e", None));
        let deep: Vec<String> = validate(&config).into_iter().map(|i| i.message).collect();
        assert_eq!(deep, ["a: its subreports nest 4 levels deep (at most 3)"]);
        // A loop is reported on every report that reaches it.
        let mut config = config_with(printing("a", Some("b")));
        config.reports.push(printing("b", Some("c")));
        config.reports.push(printing("c", Some("b")));
        let looped = validate(&config);
        assert_eq!(looped.len(), 3, "{looped:?}");
        assert!(looped.iter().all(|i| i.message.contains("in a loop")));
    }

    #[test]
    fn reports_validate_subreport_placement_and_target() {
        let mut r = printing("a", Some("missing"));
        let mut second = component("s2", "subreport", 0.0, 0.0, 50.0, 10.0);
        second.extra.insert("reportId".into(), json!(""));
        r.bands.detail.components.push(second);
        r.bands
            .detail
            .components
            .push(component("t1", "table", 50.0, 0.0, 50.0, 10.0));
        r.bands.page_footer = Band {
            height: 20.0,
            components: vec![component("s3", "subreport", 0.0, 0.0, 50.0, 10.0)],
            ..Default::default()
        };
        let issues = validate(&config_with(r));
        let text: Vec<String> = issues.iter().map(|i| i.message.clone()).collect();
        let has = |s: &str| text.iter().any(|m| m.contains(s));
        assert!(has("s1 prints a report that does not exist"), "{text:?}");
        assert!(has("Detail has more than one subreport"), "{text:?}");
        assert!(has("Detail has both a table and a subreport"), "{text:?}");
        let warning = issues
            .iter()
            .find(|i| i.message.contains("s3: subreports are not supported"))
            .expect("page band warning");
        assert_eq!(warning.severity, crate::archive::Severity::Warning);
    }

    #[test]
    fn reports_validate_warns_about_tables_in_page_bands() {
        let mut r = valid_report();
        r.bands.page_header = Band {
            height: 20.0,
            components: vec![component("ph", "table", 0.0, 0.0, 100.0, 20.0)],
            ..Default::default()
        };
        r.bands.page_footer = Band {
            height: 20.0,
            components: vec![component("pf", "table", 0.0, 0.0, 100.0, 20.0)],
            ..Default::default()
        };
        let issues = validate(&config_with(r));
        assert!(
            issues
                .iter()
                .all(|i| i.severity == crate::validation::Severity::Warning),
            "{issues:?}"
        );
        let text: Vec<String> = issues.iter().map(|i| i.message.clone()).collect();
        assert!(
            text.iter()
                .any(|m| m.contains("Page header component ph: tables are not supported")),
            "{text:?}"
        );
        assert!(
            text.iter()
                .any(|m| m.contains("Page footer component pf: tables are not supported")),
            "{text:?}"
        );
    }

    #[test]
    fn reports_pagination_flags_round_trip_and_default_off() {
        let raw = json!({"id": "r", "name": "n", "bands": {
            "detail": {"height": 10, "keepTogether": false, "pageBreakBefore": true, "components": []},
            "groups": [{"id": "g", "groupBy": "record.a", "newPage": true,
                        "repeatHeader": true, "resetPageNumber": true}]
        }});
        let report: Report = serde_json::from_value(raw).unwrap();
        assert!(report.bands.detail.page_break_before);
        assert!(!report.bands.detail.page_break_after);
        let back = serde_json::to_value(&report).unwrap();
        assert_eq!(back["bands"]["detail"]["pageBreakBefore"], json!(true));
        assert!(back["bands"]["detail"].get("pageBreakAfter").is_none());
        let g = &back["bands"]["groups"][0];
        assert_eq!(g["newPage"], json!(true));
        assert_eq!(g["repeatHeader"], json!(true));
        assert_eq!(g["resetPageNumber"], json!(true));
        assert!(back["bands"]["reportHeader"]
            .get("pageBreakBefore")
            .is_none());
    }

    #[test]
    fn reports_table_dataset_or_no_dataset_is_valid() {
        let mut r = valid_report();
        r.dataset_query_id = None;
        r.table = Some("orders".into());
        assert!(validate(&config_with(r.clone())).is_empty());
        r.table = None;
        assert!(validate(&config_with(r)).is_empty());
    }
}
