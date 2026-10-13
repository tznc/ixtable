//! Words for action queries. Port of src/query/actionText.ts and the SQL templates
//! of src/query/ActionQuery.tsx.

use crate::archive::ActionKind;

/// Query mode's names for the query types: (id, label), "read" first.
pub const KIND_LABELS: [(&str, &str); 5] = [
    ("read", "Read rows"),
    ("insert", "Insert rows"),
    ("update", "Update rows"),
    ("delete", "Delete rows"),
    ("replace", "Replace all rows"),
];

pub fn kind_id(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Insert => "insert",
        ActionKind::Update => "update",
        ActionKind::Delete => "delete",
        ActionKind::Replace => "replace",
    }
}

pub fn kind_from_id(id: &str) -> Option<ActionKind> {
    match id {
        "insert" => Some(ActionKind::Insert),
        "update" => Some(ActionKind::Update),
        "delete" => Some(ActionKind::Delete),
        "replace" => Some(ActionKind::Replace),
        _ => None,
    }
}

/// `1,234` with thousands separators.
pub fn group_digits(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn rows(n: u64) -> String {
    format!("{} {}", group_digits(n), if n == 1 { "row" } else { "rows" })
}

/// What a run did, in words.
pub fn describe_run(changed: u64, removed: u64, dry_run: bool, table: &str, kind: ActionKind) -> String {
    let verb = match kind {
        ActionKind::Insert | ActionKind::Replace => "insert",
        ActionKind::Update => "update",
        ActionKind::Delete => "delete",
    };
    let done = match kind {
        ActionKind::Insert | ActionKind::Replace => "Inserted",
        ActionKind::Update => "Updated",
        ActionKind::Delete => "Deleted",
    };
    let removed = if kind == ActionKind::Replace {
        format!(" after removing {}", rows(removed))
    } else {
        String::new()
    };
    if dry_run {
        format!(
            "Would {verb} {} in {table}{removed}. Nothing was changed.",
            rows(changed)
        )
    } else {
        format!("{done} {} in {table}{removed}.", rows(changed))
    }
}

/// The starting SQL for a kind of action query on `table` (already quoted).
pub fn template(kind: ActionKind, table: &str) -> String {
    match kind {
        ActionKind::Insert => format!("INSERT INTO {table} BY NAME\nSELECT ..."),
        ActionKind::Update => format!("UPDATE {table}\nSET ... \nWHERE ..."),
        ActionKind::Delete => format!("DELETE FROM {table}\nWHERE ..."),
        ActionKind::Replace => "SELECT ...".into(),
    }
}

/// A table name as written in SQL: bare when it is a plain lowercase identifier.
pub fn quote(name: &str) -> String {
    let mut chars = name.chars();
    let plain = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c == '_')
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
    if plain {
        name.to_string()
    } else {
        format!("\"{}\"", name.replace('"', "\"\""))
    }
}
