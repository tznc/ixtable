//! Default node positions of the relationship diagram (port of `src/data/layout.ts`).

use crate::data::TableSchema;
use std::collections::HashMap;

const LAYOUT_COLUMNS: usize = 3;
const COLUMN_WIDTH: f32 = 300.0;
const NODE_HEADER: f32 = 40.0;
const FIELD_HEIGHT: f32 = 24.0;
const ROW_GAP: f32 = 50.0;
const MIN_ROW_HEIGHT: f32 = 140.0;
const LEFT: f32 = 30.0;
const TOP: f32 = 25.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

/// What the layout needs of a table: its name, size and the tables it references.
#[derive(Clone, Debug, Default)]
pub struct LayoutNode {
    pub name: Option<String>,
    pub columns: usize,
    pub targets: Vec<String>,
}

impl LayoutNode {
    pub fn of(schema: &TableSchema) -> Self {
        Self {
            name: Some(schema.name.clone()),
            columns: schema.columns.len(),
            targets: schema
                .foreign_keys
                .iter()
                .map(|k| k.target_table.clone())
                .collect(),
        }
    }
}

pub fn node_height(columns: usize) -> f32 {
    MIN_ROW_HEIGHT.max(NODE_HEADER + columns as f32 * FIELD_HEIGHT)
}

/// Default node positions. Related tables are laid out by foreign-key dependency:
/// referenced tables in the left column and each referencing table one column right
/// of its deepest parent, so every edge runs left to right between neighbouring
/// columns instead of behind other nodes. Within a column, tables sit next to their
/// parents (barycenter order). Tables without relationships follow in rows of three
/// below the related ones.
pub fn default_layout(nodes: &[LayoutNode]) -> Vec<Position> {
    let names: HashMap<String, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.name.clone().unwrap_or_else(|| format!("#{i}")), i))
        .collect();
    // Parent indexes of each table (self-references and unknown targets ignored).
    let parents: Vec<Vec<usize>> = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let mut list: Vec<usize> = Vec::new();
            for target in &node.targets {
                if let Some(&parent) = names.get(target) {
                    if parent != index && !list.contains(&parent) {
                        list.push(parent);
                    }
                }
            }
            list
        })
        .collect();
    let mut related: Vec<usize> = Vec::new();
    let add = |related: &mut Vec<usize>, i: usize| {
        if !related.contains(&i) {
            related.push(i);
        }
    };
    for (index, list) in parents.iter().enumerate() {
        if !list.is_empty() {
            add(&mut related, index);
        }
        for &parent in list {
            add(&mut related, parent);
        }
    }

    let level = levels(&parents, &related);
    let mut columns: Vec<Vec<usize>> = Vec::new();
    let mut sorted = related.clone();
    sorted.sort_unstable();
    for index in sorted {
        let depth = level[index].unwrap_or(0).max(0) as usize;
        if columns.len() <= depth {
            columns.resize(depth + 1, Vec::new());
        }
        columns[depth].push(index);
    }
    let mut positions: Vec<Option<Position>> = vec![None; nodes.len()];
    let mut bottom = TOP;
    for (depth, column) in columns.iter().enumerate() {
        // Order by the average row of the parents already placed (stable for ties).
        let rank = |index: usize, positions: &[Option<Position>]| -> f32 {
            let placed: Vec<f32> = parents[index]
                .iter()
                .filter_map(|&p| positions[p].map(|pos| pos.y))
                .collect();
            if placed.is_empty() {
                f32::INFINITY
            } else {
                placed.iter().sum::<f32>() / placed.len() as f32
            }
        };
        let mut ordered = column.clone();
        if depth > 0 {
            let ranks: HashMap<usize, f32> = column.iter().map(|&i| (i, rank(i, &positions))).collect();
            ordered.sort_by(|a, b| {
                ranks[a]
                    .partial_cmp(&ranks[b])
                    .filter(|o| o.is_ne())
                    .unwrap_or_else(|| a.cmp(b))
            });
        }
        let mut top = TOP;
        for index in ordered {
            positions[index] = Some(Position {
                x: LEFT + depth as f32 * COLUMN_WIDTH,
                y: top,
            });
            top += node_height(nodes[index].columns) + ROW_GAP;
        }
        bottom = bottom.max(top);
    }

    let isolated: Vec<usize> = (0..nodes.len()).filter(|i| !related.contains(i)).collect();
    let mut top = if related.is_empty() { TOP } else { bottom };
    for row in isolated.chunks(LAYOUT_COLUMNS) {
        for (column, &index) in row.iter().enumerate() {
            positions[index] = Some(Position {
                x: LEFT + column as f32 * COLUMN_WIDTH,
                y: top,
            });
        }
        let tallest = row
            .iter()
            .map(|&i| node_height(nodes[i].columns))
            .fold(0.0, f32::max);
        top += tallest + ROW_GAP;
    }
    positions
        .into_iter()
        .map(|p| p.unwrap_or(Position { x: LEFT, y: TOP }))
        .collect()
}

/// Column of each related table: 0 for tables with no parents, else one past the
/// deepest parent.
fn levels(parents: &[Vec<usize>], related: &[usize]) -> Vec<Option<i32>> {
    fn visit(
        index: usize,
        parents: &[Vec<usize>],
        level: &mut Vec<Option<i32>>,
        visiting: &mut Vec<usize>,
    ) -> i32 {
        if let Some(known) = level[index] {
            return known;
        }
        // A cycle (A to B to A) is cut where it is found.
        if visiting.contains(&index) {
            return -1;
        }
        visiting.push(index);
        let mut deepest = -1;
        for &parent in &parents[index] {
            deepest = deepest.max(visit(parent, parents, level, visiting));
        }
        visiting.retain(|&i| i != index);
        level[index] = Some(deepest + 1);
        deepest + 1
    }
    let mut level = vec![None; parents.len()];
    let mut visiting = Vec::new();
    for &index in related {
        visit(index, parents, &mut level, &mut visiting);
    }
    level
}
