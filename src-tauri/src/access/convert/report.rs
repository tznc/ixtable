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
        });
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
