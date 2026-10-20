//! The "Largest files" table of the archive size reports (publish preflight and the backup
//! size dialog). Rows are observed as "NAME (SECTION) SIZE" under `<id>:<i>`; the rest of
//! the archive shows as one "Everything else" row (`<id>-other`) so the rows add up.

use super::transfer::format_bytes;
use crate::{
    archive_io::SizeEntry,
    ui::{display, forms::eid, prelude::*},
};

pub fn largest_files(id: &str, entries: &[SizeEntry], total_bytes: u64, cx: &App) -> Div {
    let mut rows: Vec<(ElementId, String, Vec<String>)> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let size = format_bytes(e.bytes);
            (eid(format!("{id}:{i}")), format!("{} ({}) {size}", e.name, e.section), vec![e.name.clone(), e.section.clone(), size])
        })
        .collect();
    let listed: u64 = entries.iter().map(|e| e.bytes).sum();
    if total_bytes > listed {
        let size = format_bytes(total_bytes - listed);
        rows.push((eid(format!("{id}-other")), format!("Everything else {size}"), vec!["Everything else".into(), String::new(), size]));
    }
    v_flex()
        .gap_1()
        .child(widgets::styled_text(eid(format!("{id}-heading")), "Largest files".to_string(), |d| {
            d.ty(Type::Label)
        }))
        .when(!rows.is_empty(), |col| col.child(display::text_table(eid(id.to_string()), &["File", "Section", "Size"], rows, cx)))
}
