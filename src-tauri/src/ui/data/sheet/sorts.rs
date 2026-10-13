//! Column sorting from the header (port of `src/data/sheet/sorts.ts`).

use crate::data::Sort;

/// Clicking a header sorts by it; Shift-click adds it as the next sort key (or flips it).
pub fn next_sorts(sorts: &[Sort], column: &str, additive: bool) -> Vec<Sort> {
    let at = sorts.iter().position(|s| s.column == column);
    if !additive {
        let descending = at == Some(0) && !sorts[0].descending;
        return vec![Sort {
            column: column.into(),
            descending,
        }];
    }
    match at {
        Some(at) => sorts
            .iter()
            .enumerate()
            .map(|(i, s)| Sort {
                column: s.column.clone(),
                descending: if i == at { !s.descending } else { s.descending },
            })
            .collect(),
        None => {
            let mut out = sorts.to_vec();
            out.push(Sort {
                column: column.into(),
                descending: false,
            });
            out
        }
    }
}
