//! Relationship diagram edges and drawn relationships (port of `src/data/relationships.ts`).

use crate::data::TableSchema;

/// A relationship drawn on the canvas: `child.column` references `parent.column`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawnRelationship {
    pub child_table: String,
    pub child_column: String,
    pub parent_table: String,
    pub parent_column: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelationshipEdge {
    pub id: String,
    pub source: String,
    pub source_handle: String,
    pub target: String,
    pub target_handle: String,
    pub label: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Source,
    Target,
}

pub fn handle_id(index: usize, direction: Direction) -> String {
    let side = match direction {
        Direction::Source => "source",
        Direction::Target => "target",
    };
    format!("c{index}-{side}")
}

/// The column index a handle id names (`c2-source` is 2), if it names one.
fn column_index(handle: Option<&str>) -> Option<usize> {
    let rest = handle?.strip_prefix('c')?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || !rest[digits.len()..].starts_with('-') {
        return None;
    }
    digits.parse().ok()
}

/// True when `columns` contain a primary key, unique constraint, unique index, or
/// unique column.
pub fn is_unique_key(schema: &TableSchema, columns: &[String]) -> bool {
    let primary: Vec<String> = if schema.primary_key.is_empty() {
        schema
            .columns
            .iter()
            .filter(|c| c.primary_key_position > 0)
            .map(|c| c.name.clone())
            .collect()
    } else {
        schema.primary_key.clone()
    };
    let mut keys: Vec<Vec<String>> = vec![primary];
    keys.extend(schema.uniques.iter().map(|u| u.columns.clone()));
    keys.extend(
        schema
            .indexes
            .iter()
            .filter(|i| i.unique)
            .map(|i| i.columns.clone()),
    );
    keys.extend(
        schema
            .columns
            .iter()
            .filter(|c| c.unique)
            .map(|c| vec![c.name.clone()]),
    );
    keys.iter()
        .any(|key| !key.is_empty() && key.iter().all(|c| columns.contains(c)))
}

/// Diagram edges: one per column pair of each foreign key, so composite keys anchor
/// on every column. The first pair carries the label: `1 — 1` when the referencing
/// columns are unique, else `1 — ∞`, followed by the column list for composite keys.
pub fn relationship_edges(schemas: &[TableSchema]) -> Vec<RelationshipEdge> {
    let mut edges = Vec::new();
    for schema in schemas {
        for key in &schema.foreign_keys {
            let target = schemas.iter().find(|s| s.name == key.target_table);
            let cardinality = if is_unique_key(schema, &key.from_columns) {
                "1 — 1"
            } else {
                "1 — ∞"
            };
            let composite = if key.from_columns.len() > 1 {
                format!(" ({})", key.from_columns.join(", "))
            } else {
                String::new()
            };
            for (i, from) in key.from_columns.iter().enumerate() {
                let source_index = target
                    .and_then(|t| {
                        t.columns
                            .iter()
                            .position(|c| Some(&c.name) == key.target_columns.get(i))
                    })
                    .unwrap_or(0);
                let target_index = schema
                    .columns
                    .iter()
                    .position(|c| &c.name == from)
                    .unwrap_or(0);
                edges.push(RelationshipEdge {
                    id: format!("{}-{}-{i}", schema.name, key.id),
                    source: key.target_table.clone(),
                    source_handle: handle_id(source_index, Direction::Source),
                    target: schema.name.clone(),
                    target_handle: handle_id(target_index, Direction::Target),
                    label: (i == 0).then(|| format!("{cardinality}{composite}")),
                });
            }
        }
    }
    edges
}

/// A canvas connection from a parent's right handle to a child's left one.
pub struct Connection<'a> {
    pub source: Option<&'a str>,
    pub target: Option<&'a str>,
    pub source_handle: Option<&'a str>,
    pub target_handle: Option<&'a str>,
}

/// The relationship a canvas connection proposes.
pub fn drawn_from(connection: &Connection, schemas: &[TableSchema]) -> Option<DrawnRelationship> {
    let parent = schemas.iter().find(|s| Some(s.name.as_str()) == connection.source)?;
    let child = schemas.iter().find(|s| Some(s.name.as_str()) == connection.target)?;
    let parent_column = parent.columns.get(column_index(connection.source_handle)?)?;
    let child_column = child.columns.get(column_index(connection.target_handle)?)?;
    Some(DrawnRelationship {
        child_table: child.name.clone(),
        child_column: child_column.name.clone(),
        parent_table: parent.name.clone(),
        parent_column: parent_column.name.clone(),
    })
}
