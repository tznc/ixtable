//! A table's datasheet view settings, kept in the document like the relationship
//! layout (port of `src/data/sheet/layout.ts`).

use crate::archive::DocumentConfig;
use crate::data::{totals::TotalFunction, Column};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DatasheetLayout {
    /// Column names hidden from the datasheet.
    pub hidden: Vec<String>,
    /// Number of leftmost visible columns frozen while scrolling sideways.
    pub frozen: usize,
    /// The totals row, when shown: the aggregate per column (absent = none).
    pub totals: Option<BTreeMap<String, TotalFunction>>,
}

fn layouts_of(navigation: &Value) -> Map<String, Value> {
    navigation
        .get("datasheetLayouts")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

pub fn read_layout(config: &DocumentConfig, table: &str) -> DatasheetLayout {
    let layouts = layouts_of(&config.navigation_state);
    let Some(saved) = layouts.get(table) else {
        return DatasheetLayout::default();
    };
    DatasheetLayout {
        hidden: saved
            .get("hidden")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        frozen: saved
            .get("frozen")
            .and_then(Value::as_u64)
            .map_or(0, |n| n as usize),
        totals: saved.get("totals").and_then(Value::as_object).map(|o| {
            o.iter()
                .filter_map(|(k, v)| {
                    serde_json::from_value::<TotalFunction>(v.clone())
                        .ok()
                        .map(|f| (k.clone(), f))
                })
                .collect()
        }),
    }
}

/// Replaces `table`'s layout in `config` (an empty layout removes the entry).
pub fn write_layout(config: &mut DocumentConfig, table: &str, layout: &DatasheetLayout) {
    let mut layouts = layouts_of(&config.navigation_state);
    let mut navigation = match config.navigation_state.take() {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    let empty = layout.hidden.is_empty() && layout.frozen == 0 && layout.totals.is_none();
    if empty {
        layouts.remove(table);
    } else {
        let totals = layout.totals.as_ref().map(|t| {
            Value::Object(
                t.iter()
                    .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or(Value::Null)))
                    .collect(),
            )
        });
        layouts.insert(
            table.to_string(),
            json!({
                "hidden": layout.hidden,
                "frozen": layout.frozen,
                "totals": totals.unwrap_or(Value::Null),
            }),
        );
    }
    navigation.insert("datasheetLayouts".into(), Value::Object(layouts));
    config.navigation_state = Value::Object(navigation);
}

/// Indexes (into `columns`) of the columns the datasheet shows, in table order.
pub fn visible_columns(columns: &[Column], layout: &DatasheetLayout) -> Vec<usize> {
    columns
        .iter()
        .enumerate()
        .filter(|(_, c)| !layout.hidden.contains(&c.name))
        .map(|(i, _)| i)
        .collect()
}

/// Hides `column`; a frozen column that is hidden leaves the frozen count in range.
pub fn hide_column(layout: &DatasheetLayout, columns: &[Column], column: &str) -> DatasheetLayout {
    let shown: Vec<&str> = visible_columns(columns, layout)
        .into_iter()
        .map(|i| columns[i].name.as_str())
        .collect();
    let Some(at) = shown.iter().position(|n| *n == column) else {
        return layout.clone();
    };
    if shown.len() <= 1 {
        return layout.clone();
    }
    let mut next = layout.clone();
    next.hidden.push(column.to_string());
    if at < layout.frozen {
        next.frozen -= 1;
    }
    next
}

/// Freezes the visible columns up to and including `column`.
pub fn freeze_through(layout: &DatasheetLayout, columns: &[Column], column: &str) -> DatasheetLayout {
    let shown: Vec<&str> = visible_columns(columns, layout)
        .into_iter()
        .map(|i| columns[i].name.as_str())
        .collect();
    match shown.iter().position(|n| *n == column) {
        Some(at) => DatasheetLayout {
            frozen: at + 1,
            ..layout.clone()
        },
        None => layout.clone(),
    }
}
