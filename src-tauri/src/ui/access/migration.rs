//! The migration report kept in `settings.accessImport` of an imported document
//! (docs/decisions/access-import.md): reading it, filtering it, marking items reviewed,
//! and rendering it as Markdown or CSV. Port of src/access/migration.ts.

use super::summary::{AccessFormat, ImportItem, ItemStatus, kind_label, kind_rank};
use serde_json::{Map, Value, json};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq)]
pub struct AccessMigration {
    pub source: String,
    pub format: AccessFormat,
    pub report: Vec<ImportItem>,
    pub warnings: Vec<String>,
    pub imported_at: Option<String>,
    /// `kind:name` keys of items the developer marked as reviewed.
    pub reviewed: Vec<String>,
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default()
}

/// The stored format name; the importer writes "accdt" for templates.
fn format_of(value: Option<&Value>) -> AccessFormat {
    match value.and_then(Value::as_str) {
        Some("jet3") => AccessFormat::Jet3,
        Some("jet4") => AccessFormat::Jet4,
        Some("ace") => AccessFormat::Ace,
        _ => AccessFormat::Template,
    }
}

/// The migration report of a document, or None when it was not imported from Access.
pub fn read_migration(settings: &Value) -> Option<AccessMigration> {
    let access = settings.get("accessImport")?.as_object()?;
    let items = access.get("report")?.as_array()?;
    let report = items
        .iter()
        .filter_map(|i| serde_json::from_value::<ImportItem>(i.clone()).ok())
        .collect();
    Some(AccessMigration {
        source: access.get("source").and_then(Value::as_str).unwrap_or("").to_string(),
        format: format_of(access.get("format")),
        report,
        warnings: strings(access.get("warnings")),
        imported_at: access.get("importedAt").and_then(Value::as_str).map(str::to_string),
        reviewed: strings(access.get("reviewed")),
    })
}

pub fn item_key(item: &ImportItem) -> String {
    format!("{}:{}", item.kind, item.name)
}

/// Settings with `key` marked reviewed (or not); other settings are kept.
pub fn with_reviewed(settings: &Value, key: &str, reviewed: bool) -> Value {
    let mut base: Map<String, Value> = settings.as_object().cloned().unwrap_or_default();
    let mut access: Map<String, Value> =
        base.get("accessImport").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut current: BTreeSet<String> = strings(access.get("reviewed")).into_iter().collect();
    if reviewed {
        current.insert(key.to_string());
    } else {
        current.remove(key);
    }
    access.insert("reviewed".into(), json!(current.into_iter().collect::<Vec<_>>()));
    base.insert("accessImport".into(), Value::Object(access));
    Value::Object(base)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusFilter {
    Attention,
    All,
    Status(ItemStatus),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationFilter {
    pub status: StatusFilter,
    pub kind: String,
    pub search: String,
    pub hide_reviewed: bool,
}

impl MigrationFilter {
    pub fn new(status: StatusFilter) -> Self {
        Self { status, kind: String::new(), search: String::new(), hide_reviewed: false }
    }
}

/// Items matching the filter, worst first, then by kind and name.
pub fn filter_items(m: &AccessMigration, f: &MigrationFilter) -> Vec<ImportItem> {
    let search = f.search.trim().to_lowercase();
    let reviewed: BTreeSet<&String> = m.reviewed.iter().collect();
    let mut items: Vec<ImportItem> = m
        .report
        .iter()
        .filter(|i| match f.status {
            StatusFilter::All => true,
            StatusFilter::Attention => i.status != ItemStatus::Converted,
            StatusFilter::Status(s) => i.status == s,
        })
        .filter(|i| f.kind.is_empty() || i.kind == f.kind)
        .filter(|i| !f.hide_reviewed || !reviewed.contains(&item_key(i)))
        .filter(|i| {
            search.is_empty()
                || i.name.to_lowercase().contains(&search)
                || i.notes.iter().any(|n| n.to_lowercase().contains(&search))
        })
        .cloned()
        .collect();
    items.sort_by(|a, b| {
        (a.status.rank(), kind_rank(&a.kind), a.name.to_lowercase())
            .cmp(&(b.status.rank(), kind_rank(&b.kind), b.name.to_lowercase()))
    });
    items
}

/// Items still needing attention that nobody marked reviewed.
pub fn open_count(m: &AccessMigration) -> usize {
    m.report
        .iter()
        .filter(|i| i.status != ItemStatus::Converted && !m.reviewed.contains(&item_key(i)))
        .count()
}

fn md_cell(text: &str) -> String {
    text.replace('|', "\\|").replace("\r\n", " ").replace('\n', " ")
}

/// The report as Markdown: a summary table, then every item that lost something.
pub fn to_markdown(m: &AccessMigration, title: &str) -> String {
    let mut lines = vec![format!("# Access migration report: {title}"), String::new()];
    let source = if m.source.is_empty() { "unknown" } else { &m.source };
    lines.push(format!("Source: {source} ({})", m.format.label()));
    if let Some(at) = &m.imported_at {
        lines.push(format!("Imported: {at}"));
    }
    lines.push(String::new());
    lines.push("| Objects | Converted | Partly converted | Not converted |".into());
    lines.push("|---|---|---|---|".into());
    for c in super::summary::status_counts(&m.report) {
        lines.push(format!(
            "| {} | {} | {} | {} |",
            kind_label(&c.kind),
            c.converted,
            c.partial,
            c.skipped
        ));
    }
    let attention = filter_items(m, &MigrationFilter::new(StatusFilter::Attention));
    if !attention.is_empty() {
        lines.push(String::new());
        lines.push("## What did not convert fully".into());
        lines.push(String::new());
        for item in &attention {
            let mark = if m.reviewed.contains(&item_key(item)) { " (reviewed)" } else { "" };
            lines.push(format!(
                "### {} ({}): {}{mark}",
                md_cell(&item.name),
                item.kind,
                item.status.label()
            ));
            lines.push(String::new());
            for note in &item.notes {
                lines.push(format!("- {}", md_cell(note)));
            }
            lines.push(String::new());
        }
    }
    if !m.warnings.is_empty() {
        lines.push(String::new());
        lines.push("## Reading problems".into());
        lines.push(String::new());
        for w in &m.warnings {
            lines.push(format!("- {}", md_cell(w)));
        }
    }
    format!("{}\n", lines.join("\n").trim_end())
}

fn csv_cell(text: &str) -> String {
    if text.contains(['"', ',', '\r', '\n']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

/// One row per object and note: kind, name, status, reviewed, note.
pub fn to_csv(m: &AccessMigration) -> String {
    let mut rows: Vec<Vec<String>> = vec![
        ["Kind", "Name", "Status", "Reviewed", "Note"].iter().map(|s| s.to_string()).collect(),
    ];
    for item in filter_items(m, &MigrationFilter::new(StatusFilter::All)) {
        let base = vec![
            item.kind.clone(),
            item.name.clone(),
            item.status.label().to_string(),
            if m.reviewed.contains(&item_key(&item)) { "yes" } else { "no" }.to_string(),
        ];
        if item.notes.is_empty() {
            rows.push([base.clone(), vec![String::new()]].concat());
        }
        for note in &item.notes {
            rows.push([base.clone(), vec![note.clone()]].concat());
        }
    }
    let mut out: String = rows
        .iter()
        .map(|r| r.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(","))
        .collect::<Vec<_>>()
        .join("\r\n");
    out.push_str("\r\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Value {
        json!({
            "theme": "keep",
            "accessImport": {
                "source": "Orders.accdb",
                "format": "accdt",
                "importedAt": "2026-10-10T12:00:00Z",
                "warnings": ["page 9 could not be read"],
                "report": [
                    { "kind": "table", "name": "Orders", "status": "converted", "notes": [] },
                    { "kind": "form", "name": "Order Entry", "status": "partial",
                      "notes": ["the On Load VBA procedure does not run"],
                      "target": { "kind": "form", "id": "f1" } },
                    { "kind": "query", "name": "Pass, \"Through\"", "status": "skipped",
                      "notes": ["pass-through | server", "Access SQL: SELECT 1"] },
                    { "kind": "macro", "name": "AutoExec", "status": "partial",
                      "notes": ["Beep was left out"] },
                    { "kind": "bogus" },
                ],
            },
        })
    }

    fn names(items: Vec<ImportItem>) -> Vec<String> {
        items.into_iter().map(|i| i.name).collect()
    }

    fn all() -> MigrationFilter {
        MigrationFilter::new(StatusFilter::All)
    }

    #[test]
    fn reads_the_report_from_settings_and_ignores_anything_malformed() {
        let m = read_migration(&settings()).unwrap();
        assert_eq!(m.source, "Orders.accdb");
        assert_eq!(m.format, AccessFormat::Template);
        assert_eq!(m.report.len(), 4);
        assert!(m.reviewed.is_empty());
        assert!(read_migration(&json!({})).is_none());
        assert!(read_migration(&Value::Null).is_none());
        assert!(read_migration(&json!({ "accessImport": { "report": "x" } })).is_none());
    }

    #[test]
    fn filters_worst_first_and_searches_names_and_notes() {
        let m = read_migration(&settings()).unwrap();
        let with = |f: MigrationFilter| names(filter_items(&m, &f));
        assert_eq!(
            with(MigrationFilter::new(StatusFilter::Attention)),
            ["Pass, \"Through\"", "Order Entry", "AutoExec"]
        );
        assert_eq!(with(MigrationFilter { kind: "form".into(), ..all() }), ["Order Entry"]);
        assert_eq!(with(MigrationFilter { search: "beep".into(), ..all() }), ["AutoExec"]);
        assert_eq!(
            with(MigrationFilter::new(StatusFilter::Status(ItemStatus::Converted))),
            ["Orders"]
        );
    }

    #[test]
    fn marks_items_reviewed_without_touching_other_settings() {
        let key = item_key(&ImportItem {
            kind: "macro".into(),
            name: "AutoExec".into(),
            status: ItemStatus::Partial,
            notes: vec![],
            target: None,
        });
        let next = with_reviewed(&settings(), &key, true);
        assert_eq!(next["theme"], "keep");
        assert_eq!(next["accessImport"]["reviewed"], json!(["macro:AutoExec"]));
        let m = read_migration(&next).unwrap();
        assert_eq!(open_count(&m), 2);
        let hidden = MigrationFilter { hide_reviewed: true, ..MigrationFilter::new(StatusFilter::Attention) };
        assert_eq!(filter_items(&m, &hidden).len(), 2);
        let cleared = read_migration(&with_reviewed(&next, &key, false)).unwrap();
        assert!(cleared.reviewed.is_empty());
    }

    #[test]
    fn renders_markdown_and_csv() {
        let m = read_migration(&with_reviewed(&settings(), "form:Order Entry", true)).unwrap();
        let md = to_markdown(&m, "Order Desk");
        assert!(md.contains("# Access migration report: Order Desk"));
        assert!(md.contains("| Forms | 0 | 1 | 0 |"));
        assert!(md.contains("### Order Entry (form): Partly converted (reviewed)"));
        assert!(md.contains("- pass-through \\| server"));
        assert!(md.contains("## Reading problems"));
        let csv = to_csv(&m);
        let lines: Vec<&str> = csv.split("\r\n").collect();
        assert_eq!(lines[0], "Kind,Name,Status,Reviewed,Note");
        assert!(lines.contains(&"query,\"Pass, \"\"Through\"\"\",Not converted,no,pass-through | server"));
        assert!(lines.contains(&"form,Order Entry,Partly converted,yes,the On Load VBA procedure does not run"));
        assert!(lines.contains(&"table,Orders,Converted,no,"));
    }
}
