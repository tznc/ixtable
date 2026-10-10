//! The import report: what each Access object became, and what was lost.
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Converted with its behavior intact.
    Converted,
    /// Converted, but some parts have no ixtable equivalent (see notes).
    Partial,
    /// Not converted.
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// table, query, form, report, macro, module, relationship
    pub kind: String,
    pub name: String,
    pub status: Status,
    pub notes: Vec<String>,
    /// The ixtable object it became, for links from the migration report.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<Target>,
}

/// An object of the document: `kind` is form, report, action or query.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Target {
    pub kind: &'static str,
    pub id: String,
}

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub items: Vec<Item>,
    /// File-level messages (reader warnings).
    pub warnings: Vec<String>,
    pub tables: usize,
    pub rows: u64,
}

impl ImportReport {
    pub fn add(&mut self, kind: &str, name: &str, status: Status, notes: Vec<String>) {
        let mut notes = notes;
        notes.dedup();
        self.items.push(Item {
            kind: kind.into(),
            name: name.into(),
            status,
            notes,
            target: None,
        });
    }

    /// Points each item at the object it became, matched by name.
    pub fn link_targets(&mut self, config: &crate::archive::DocumentConfig) {
        let find = |list: Vec<(&str, &str)>, name: &str| {
            list.into_iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, id)| id.to_string())
        };
        for item in &mut self.items {
            let (kind, id) = match item.kind.as_str() {
                "form" => (
                    "form",
                    find(
                        config
                            .design
                            .forms
                            .iter()
                            .map(|f| (f.name.as_str(), f.id.as_str()))
                            .collect(),
                        &item.name,
                    ),
                ),
                "report" => (
                    "report",
                    find(
                        config
                            .reports
                            .iter()
                            .map(|r| (r.name.as_str(), r.id.as_str()))
                            .collect(),
                        &item.name,
                    ),
                ),
                "macro" => (
                    "action",
                    find(
                        config
                            .actions
                            .iter()
                            .map(|a| (a.name.as_str(), a.id.as_str()))
                            .collect(),
                        &item.name,
                    ),
                ),
                "query" => (
                    "query",
                    find(
                        config
                            .saved_queries
                            .iter()
                            .map(|q| (q.name.as_str(), q.id.as_str()))
                            .collect(),
                        &item.name,
                    ),
                ),
                _ => continue,
            };
            item.target = id.map(|id| Target { kind, id });
        }
    }

    /// Adds a note to an existing item (and downgrades a converted item to partial).
    pub fn note(&mut self, kind: &str, name: &str, note: impl Into<String>) {
        let note = note.into();
        match self
            .items
            .iter_mut()
            .find(|i| i.kind == kind && i.name.eq_ignore_ascii_case(name))
        {
            Some(item) => {
                if item.status == Status::Converted {
                    item.status = Status::Partial;
                }
                if !item.notes.contains(&note) {
                    item.notes.push(note);
                }
            }
            None => self.add(kind, name, Status::Partial, vec![note]),
        }
    }
}

/// Collects notes for one object while it converts.
#[derive(Debug, Default)]
pub struct Notes(pub Vec<String>);

impl Notes {
    pub fn push(&mut self, n: impl Into<String>) {
        let n = n.into();
        if !self.0.contains(&n) {
            self.0.push(n);
        }
    }

    pub fn status(&self) -> Status {
        if self.0.is_empty() {
            Status::Converted
        } else {
            Status::Partial
        }
    }
}
