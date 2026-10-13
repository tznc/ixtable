//! Counts and ordering for the Access import report. Port of src/access/summary.ts and
//! the shared types of src/access/types.ts.

use crate::access::{Inventory, convert::report::ImportReport, model::SourceFormat};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessFormat {
    Template,
    Jet3,
    Jet4,
    Ace,
}

impl AccessFormat {
    pub fn label(self) -> &'static str {
        match self {
            Self::Template => "Access template (.accdt)",
            Self::Jet3 => "Access 97 database (.mdb)",
            Self::Jet4 => "Access 2000–2003 database (.mdb)",
            Self::Ace => "Access 2007 or later database (.accdb)",
        }
    }
}

impl From<SourceFormat> for AccessFormat {
    fn from(format: SourceFormat) -> Self {
        match format {
            SourceFormat::Template => Self::Template,
            SourceFormat::Jet3 => Self::Jet3,
            SourceFormat::Jet4 => Self::Jet4,
            SourceFormat::Ace => Self::Ace,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ItemStatus {
    Converted,
    Partial,
    Skipped,
}

impl ItemStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Converted => "Converted",
            Self::Partial => "Partly converted",
            Self::Skipped => "Not converted",
        }
    }

    /// Worst first.
    pub fn rank(self) -> u8 {
        match self {
            Self::Skipped => 0,
            Self::Partial => 1,
            Self::Converted => 2,
        }
    }
}

/// The ixtable object an Access object became (`kind`: form, report, action or query).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImportTarget {
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ImportItem {
    pub kind: String,
    pub name: String,
    pub status: ItemStatus,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub target: Option<ImportTarget>,
}

/// Kinds in the order the report lists them.
pub const KIND_ORDER: [&str; 7] =
    ["table", "relationship", "query", "form", "report", "macro", "module"];

pub fn kind_label(kind: &str) -> String {
    match kind {
        "table" => "Tables",
        "relationship" => "Relationships",
        "query" => "Queries",
        "form" => "Forms",
        "report" => "Reports",
        "macro" => "Macros",
        "module" => "Modules",
        other => other,
    }
    .to_string()
}

pub fn kind_rank(kind: &str) -> usize {
    KIND_ORDER.iter().position(|k| *k == kind).unwrap_or(KIND_ORDER.len())
}

pub fn total_rows(inventory: &Inventory) -> u64 {
    inventory.tables.iter().map(|t| t.rows.unwrap_or(0)).sum()
}

/// The items of a backend import report.
pub fn items_of(report: &ImportReport) -> Vec<ImportItem> {
    serde_json::to_value(&report.items)
        .ok()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindCount {
    pub kind: String,
    pub converted: usize,
    pub partial: usize,
    pub skipped: usize,
}

/// Counts per status for each kind, in report order.
pub fn status_counts(items: &[ImportItem]) -> Vec<KindCount> {
    let mut kinds: Vec<&str> = vec![];
    for item in items {
        if !kinds.contains(&item.kind.as_str()) {
            kinds.push(&item.kind);
        }
    }
    kinds.sort_by_key(|k| kind_rank(k));
    kinds
        .into_iter()
        .map(|kind| {
            let count = |status| {
                items.iter().filter(|i| i.kind == kind && i.status == status).count()
            };
            KindCount {
                kind: kind.to_string(),
                converted: count(ItemStatus::Converted),
                partial: count(ItemStatus::Partial),
                skipped: count(ItemStatus::Skipped),
            }
        })
        .collect()
}

/// Items that lost something, worst first, then by kind and name.
pub fn items_needing_attention(items: &[ImportItem]) -> Vec<ImportItem> {
    let mut out: Vec<ImportItem> =
        items.iter().filter(|i| i.status != ItemStatus::Converted).cloned().collect();
    out.sort_by(|a, b| {
        (a.status.rank(), kind_rank(&a.kind), a.name.to_lowercase())
            .cmp(&(b.status.rank(), kind_rank(&b.kind), b.name.to_lowercase()))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::TableSummary;

    fn item(kind: &str, name: &str, status: ItemStatus, notes: &[&str]) -> ImportItem {
        ImportItem {
            kind: kind.into(),
            name: name.into(),
            status,
            notes: notes.iter().map(|n| n.to_string()).collect(),
            target: None,
        }
    }

    fn report() -> Vec<ImportItem> {
        use ItemStatus::*;
        vec![
            item("module", "Helpers", Skipped, &["VBA"]),
            item("form", "Orders", Partial, &["button left out"]),
            item("table", "Orders", Converted, &[]),
            item("form", "Customers", Converted, &[]),
            item("query", "Totals", Skipped, &["action query"]),
            item("form", "Contacts", Partial, &["unbound box"]),
        ]
    }

    #[test]
    fn adds_up_rows_and_ignores_tables_whose_rows_are_unknown() {
        let table = |name: &str, columns, rows| TableSummary {
            name: name.into(),
            columns,
            rows,
        };
        let inventory = Inventory {
            format: SourceFormat::Ace,
            tables: vec![
                table("Customers", 4, Some(3)),
                table("Orders", 6, Some(10)),
                table("Linked", 2, None),
            ],
            relationships: 1,
            queries: vec![],
            forms: vec![],
            reports: vec![],
            macros: vec![],
            modules: vec![],
            compiled: vec![],
            warnings: vec![],
        };
        assert_eq!(total_rows(&inventory), 13);
    }

    #[test]
    fn counts_statuses_per_kind_in_report_order() {
        let count = |kind: &str, converted, partial, skipped| KindCount {
            kind: kind.into(),
            converted,
            partial,
            skipped,
        };
        assert_eq!(
            status_counts(&report()),
            vec![
                count("table", 1, 0, 0),
                count("query", 0, 0, 1),
                count("form", 1, 2, 0),
                count("module", 0, 0, 1),
            ]
        );
    }

    #[test]
    fn lists_skipped_items_before_partial_ones_then_by_kind_and_name() {
        let names: Vec<String> = items_needing_attention(&report())
            .iter()
            .map(|i| format!("{}:{}", i.kind, i.name))
            .collect();
        assert_eq!(names, ["query:Totals", "module:Helpers", "form:Contacts", "form:Orders"]);
    }

    #[test]
    fn labels_kinds_in_the_plural_and_passes_unknown_kinds_through() {
        assert_eq!(kind_label("macro"), "Macros");
        assert_eq!(kind_label("other"), "other");
    }
}
