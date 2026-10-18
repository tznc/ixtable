//! Report definition helpers the designer uses: new reports, groups and components, band
//! lookup in print order, duplication, subreport nesting rules and definition problems.
//! Port of `src/reports/model.ts` over the stored `crate::reports` types.

use crate::reports::{Band, Component, Report, ReportGroup, MAX_SUBREPORT_DEPTH};
use serde_json::{json, Map, Value};

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn empty_band(height: f64) -> Band {
    Band {
        height,
        ..Band::default()
    }
}

pub fn new_report(name: &str) -> Report {
    let mut report = Report {
        id: new_id(),
        name: name.into(),
        ..Report::default()
    };
    report.bands.report_header = empty_band(40.0);
    report.bands.page_header = empty_band(0.0);
    report.bands.detail = empty_band(18.0);
    report.bands.page_footer = empty_band(20.0);
    report.bands.report_footer = empty_band(30.0);
    report
}

pub fn new_group(group_by: &str) -> ReportGroup {
    ReportGroup {
        id: new_id(),
        group_by: group_by.into(),
        header: empty_band(22.0),
        footer: empty_band(20.0),
        ..ReportGroup::default()
    }
}

/// Component kinds in palette order with their display names.
pub const KINDS: [(&str, &str); 9] = [
    ("staticText", "Text"),
    ("field", "Field"),
    ("calculated", "Calculated"),
    ("image", "Image"),
    ("line", "Line"),
    ("rectangle", "Rectangle"),
    ("table", "Table"),
    ("subreport", "Subreport"),
    ("chart", "Chart"),
];

/// Display name of a component kind.
pub fn kind_label(kind: &str) -> &'static str {
    KINDS.iter().find(|(k, _)| *k == kind).map_or("Component", |(_, l)| l)
}

fn kind_size(kind: &str) -> (f64, f64) {
    match kind {
        "image" => (80.0, 60.0),
        "line" => (200.0, 4.0),
        "rectangle" => (120.0, 40.0),
        "table" => (300.0, 40.0),
        "subreport" => (300.0, 60.0),
        "chart" => (280.0, 160.0),
        _ => (120.0, 16.0),
    }
}

/// A new component of `kind`, placed at (x, y) and clamped into a band of `band_width`.
pub fn new_component(kind: &str, band_width: f64, at: (f64, f64)) -> Component {
    let (w0, h0) = kind_size(kind);
    let w = w0.min(band_width);
    let extra = match kind {
        "staticText" => json!({ "text": "Text" }),
        "field" => json!({ "expression": "" }),
        "calculated" => json!({ "expression": "count(rows)" }),
        "image" => json!({ "assetId": "" }),
        "line" => json!({ "orientation": "horizontal", "style": { "borderWidth": 1 } }),
        "rectangle" => json!({ "style": { "borderWidth": 1 } }),
        "table" => json!({ "queryId": "", "columns": [] }),
        "subreport" => json!({ "reportId": "", "links": [] }),
        "chart" => json!({ "chartType": "bar", "queryId": "", "xField": "", "yFields": [] }),
        _ => json!({}),
    };
    Component {
        id: new_id(),
        kind: kind.into(),
        x: at.0.min(band_width - w).max(0.0),
        y: at.1,
        w,
        h: h0,
        extra: match extra {
            Value::Object(map) => map,
            _ => Map::new(),
        },
    }
}

/// Identifies one band of a report: fixed bands by name, group bands by group id.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BandKey {
    ReportHeader,
    PageHeader,
    Detail,
    PageFooter,
    ReportFooter,
    GroupHeader(String),
    GroupFooter(String),
}

impl BandKey {
    /// A stable text form (`detail`, `groupHeader:<id>`).
    pub fn id(&self) -> String {
        match self {
            BandKey::ReportHeader => "reportHeader".into(),
            BandKey::PageHeader => "pageHeader".into(),
            BandKey::Detail => "detail".into(),
            BandKey::PageFooter => "pageFooter".into(),
            BandKey::ReportFooter => "reportFooter".into(),
            BandKey::GroupHeader(id) => format!("groupHeader:{id}"),
            BandKey::GroupFooter(id) => format!("groupFooter:{id}"),
        }
    }
}

pub struct BandEntry<'a> {
    pub key: BandKey,
    pub label: String,
    pub band: &'a Band,
}

/// Friendly group name: the grouped field ("Status group"), never the raw expression.
pub fn group_label(group: &ReportGroup, index: usize) -> String {
    let field = group
        .group_by
        .trim()
        .trim_start_matches("record.")
        .trim_matches(|c| c == '[' || c == ']');
    let plain = !field.is_empty() && field.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ' ');
    if !plain {
        return format!("Group {}", index + 1);
    }
    let mut words = field.replace('_', " ");
    if let Some(first) = words.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    format!("{words} group")
}

/// Bands in print order with display labels.
pub fn band_entries(report: &Report) -> Vec<BandEntry<'_>> {
    let b = &report.bands;
    let mut out = vec![
        BandEntry { key: BandKey::ReportHeader, label: "Report header".into(), band: &b.report_header },
        BandEntry { key: BandKey::PageHeader, label: "Page header".into(), band: &b.page_header },
    ];
    for (i, g) in b.groups.iter().enumerate() {
        out.push(BandEntry {
            key: BandKey::GroupHeader(g.id.clone()),
            label: format!("{} header", group_label(g, i)),
            band: &g.header,
        });
    }
    out.push(BandEntry { key: BandKey::Detail, label: "Detail".into(), band: &b.detail });
    for (i, g) in b.groups.iter().enumerate().rev() {
        out.push(BandEntry {
            key: BandKey::GroupFooter(g.id.clone()),
            label: format!("{} footer", group_label(g, i)),
            band: &g.footer,
        });
    }
    out.push(BandEntry { key: BandKey::PageFooter, label: "Page footer".into(), band: &b.page_footer });
    out.push(BandEntry { key: BandKey::ReportFooter, label: "Report footer".into(), band: &b.report_footer });
    out
}

pub fn get_band<'a>(report: &'a Report, key: &BandKey) -> Option<&'a Band> {
    band_entries(report).into_iter().find(|e| &e.key == key).map(|e| e.band)
}

/// Changes band `key` in place.
pub fn with_band(report: &mut Report, key: &BandKey, change: impl FnOnce(&mut Band)) {
    let bands = &mut report.bands;
    match key {
        BandKey::ReportHeader => change(&mut bands.report_header),
        BandKey::PageHeader => change(&mut bands.page_header),
        BandKey::Detail => change(&mut bands.detail),
        BandKey::PageFooter => change(&mut bands.page_footer),
        BandKey::ReportFooter => change(&mut bands.report_footer),
        BandKey::GroupHeader(id) => {
            if let Some(g) = bands.groups.iter_mut().find(|g| &g.id == id) {
                change(&mut g.header);
            }
        }
        BandKey::GroupFooter(id) => {
            if let Some(g) = bands.groups.iter_mut().find(|g| &g.id == id) {
                change(&mut g.footer);
            }
        }
    }
}

/// Finds a component and the key of the band holding it.
pub fn find_component<'a>(report: &'a Report, id: &str) -> Option<(BandKey, &'a Component)> {
    band_entries(report).into_iter().find_map(|e| {
        e.band
            .components
            .iter()
            .find(|c| c.id == id)
            .map(|c| (e.key.clone(), c))
    })
}

/// Deep copy with fresh ids for the report, its groups, components, table columns and
/// conditions.
pub fn duplicate_report(report: &Report, name: &str) -> Report {
    let copy_band = |band: &Band| -> Band {
        let mut band = band.clone();
        for c in &mut band.components {
            c.id = new_id();
            if c.kind == "table" {
                if let Some(Value::Array(columns)) = c.extra.get_mut("columns") {
                    for col in columns {
                        if let Value::Object(col) = col {
                            col.insert("id".into(), Value::String(new_id()));
                        }
                    }
                }
            } else if let Some(Value::Array(conditions)) = c.extra.get_mut("conditions") {
                for rule in conditions {
                    if let Value::Object(rule) = rule {
                        rule.insert("id".into(), Value::String(new_id()));
                    }
                }
            }
        }
        band
    };
    let mut copy = report.clone();
    copy.id = new_id();
    copy.name = name.into();
    let b = &mut copy.bands;
    b.report_header = copy_band(&b.report_header);
    b.page_header = copy_band(&b.page_header);
    for g in &mut b.groups {
        g.id = new_id();
        g.header = copy_band(&g.header);
        g.footer = copy_band(&g.footer);
    }
    b.detail = copy_band(&b.detail);
    b.page_footer = copy_band(&b.page_footer);
    b.report_footer = copy_band(&b.report_footer);
    copy
}

/// A component's position and size in band points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Keeps a component inside its band (`width` x `height`) and at least 1 pt in size.
pub fn clamp_geometry(g: Geometry, width: f64, height: f64) -> Geometry {
    let clamp = |v: f64, lo: f64, hi: f64| v.min(hi).max(lo);
    let w = clamp(super::js_round(g.w), 1.0, width);
    let h = clamp(super::js_round(g.h), 1.0, height.max(1.0));
    Geometry {
        w,
        h,
        x: clamp(super::js_round(g.x), 0.0, width - w),
        y: clamp(super::js_round(g.y), 0.0, (height - h).max(0.0)),
    }
}

/// The least height a band may have: the bottom edge of its lowest component.
pub fn band_min_height(band: &Band) -> f64 {
    band.components.iter().map(|c| c.y + c.h).fold(0.0, f64::max)
}

/// Names visible to report expressions (for `check()` diagnostics).
pub const REPORT_SCOPE_NAMES: [&str; 11] = [
    "record",
    "rows",
    "params",
    "page",
    "pages",
    "groupPage",
    "groupPages",
    "report",
    "group",
    "rowNumber",
    "parent",
];

/// `record.<column>` with bracket quoting for names that are not plain identifiers.
pub fn field_expression(column: &str) -> String {
    let mut chars = column.chars();
    let plain = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain {
        format!("record.{column}")
    } else {
        format!("record.[{column}]")
    }
}

pub fn is_page_band(key: &BandKey) -> bool {
    matches!(key, BandKey::PageHeader | BandKey::PageFooter)
}

/// Ids of the reports a report's subreports print.
pub fn subreport_ids(report: &Report) -> Vec<String> {
    band_entries(report)
        .iter()
        .flat_map(|e| e.band.components.iter())
        .filter(|c| c.kind == "subreport")
        .filter_map(|c| c.extra.get("reportId").and_then(Value::as_str))
        .filter(|id| !id.is_empty())
        .map(String::from)
        .collect()
}

/// Levels of subreports below report `id` (0 without any); `None` when they loop.
pub fn subreport_depth(reports: &[Report], id: &str, path: &mut Vec<String>) -> Option<usize> {
    if path.iter().any(|p| p == id) {
        return None;
    }
    let ids = reports
        .iter()
        .find(|r| r.id == id)
        .map(subreport_ids)
        .unwrap_or_default();
    if ids.is_empty() {
        return Some(0);
    }
    path.push(id.to_string());
    let mut deepest = 0;
    for child in &ids {
        match subreport_depth(reports, child, path) {
            Some(d) => deepest = deepest.max(d),
            None => {
                path.pop();
                return None;
            }
        }
    }
    path.pop();
    Some(1 + deepest)
}

/// Levels of subreports above report `id`: the longest chain of reports that print it;
/// `None` for a loop.
fn subreport_height(reports: &[Report], id: &str, path: &mut Vec<String>) -> Option<usize> {
    if path.iter().any(|p| p == id) {
        return None;
    }
    path.push(id.to_string());
    let mut height = 0;
    for parent in reports.iter().filter(|r| subreport_ids(r).iter().any(|s| s == id)) {
        match subreport_height(reports, &parent.id, path) {
            Some(h) => height = height.max(1 + h),
            None => {
                path.pop();
                return None;
            }
        }
    }
    path.pop();
    Some(height)
}

/// Reports a subreport in `report` may print: never `report` itself or one that prints
/// it, and only while the whole chain stays within `MAX_SUBREPORT_DEPTH`.
pub fn embeddable_reports<'a>(reports: &'a [Report], report: &Report) -> Vec<&'a Report> {
    let Some(above) = subreport_height(reports, &report.id, &mut vec![]) else {
        return vec![];
    };
    reports
        .iter()
        .filter(|r| {
            r.id != report.id
                && subreport_depth(reports, &r.id, &mut vec![report.id.clone()])
                    .is_some_and(|below| above + 1 + below <= MAX_SUBREPORT_DEPTH)
        })
        .collect()
}

/// Definition problems the designer shows; mirrors `reports::validate` in Rust.
pub fn report_problems(report: &Report, reports: &[Report]) -> Vec<String> {
    let depth = subreport_depth(reports, &report.id, &mut vec![]);
    let mut out = vec![];
    match depth {
        None => out.push("its subreports print each other in a loop".to_string()),
        Some(d) if d > MAX_SUBREPORT_DEPTH => out.push(format!(
            "its subreports nest {d} levels deep (at most {MAX_SUBREPORT_DEPTH})"
        )),
        _ => {}
    }
    for e in band_entries(report) {
        let page = is_page_band(&e.key);
        let subs: Vec<&Component> = e.band.components.iter().filter(|c| c.kind == "subreport").collect();
        for c in &e.band.components {
            if page && (c.kind == "table" || c.kind == "subreport") {
                out.push(format!(
                    "{} component {}: {} are not supported in page headers or footers",
                    e.label,
                    c.id,
                    if c.kind == "table" { "tables" } else { "subreports" }
                ));
            }
        }
        if subs.len() > 1 {
            out.push(format!("{} has more than one subreport", e.label));
        }
        if !subs.is_empty() && e.band.components.iter().any(|c| c.kind == "table") {
            out.push(format!("{} has both a table and a subreport", e.label));
        }
        for c in subs {
            let target = c.extra.get("reportId").and_then(Value::as_str).unwrap_or("");
            if !target.is_empty() && !reports.is_empty() && !reports.iter().any(|r| r.id == target) {
                out.push(format!(
                    "{} component {}: the subreport's report does not exist",
                    e.label, c.id
                ));
            }
        }
    }
    out
}
