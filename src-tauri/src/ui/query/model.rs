//! Visual query builder model, stored as JSON in `SavedQuery.builder` (PRD §12).
//! Port of src/query/builder/model.ts. The JSON shape is unchanged, so documents
//! written by the React UI load as-is.

use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Aggregate {
    Count,
    Sum,
    Avg,
    Min,
    Max,
    CountDistinct,
}

/// (aggregate, label) in menu order.
pub const AGGREGATES: [(Aggregate, &str); 6] = [
    (Aggregate::Count, "Count"),
    (Aggregate::CountDistinct, "Count distinct"),
    (Aggregate::Sum, "Sum"),
    (Aggregate::Avg, "Average"),
    (Aggregate::Min, "Min"),
    (Aggregate::Max, "Max"),
];

impl Aggregate {
    /// The id used in the JSON model.
    pub fn id(self) -> &'static str {
        match self {
            Aggregate::Count => "count",
            Aggregate::Sum => "sum",
            Aggregate::Avg => "avg",
            Aggregate::Min => "min",
            Aggregate::Max => "max",
            Aggregate::CountDistinct => "countDistinct",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        AGGREGATES.iter().map(|(a, _)| *a).find(|a| a.id() == id)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderSource {
    pub id: String,
    pub table: String,
    /// SQL alias, unique within the model. The first source is the FROM table.
    pub alias: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinCondition {
    /// Alias of a source listed before the joined one.
    #[serde(default)]
    pub left_source: String,
    #[serde(default)]
    pub left_column: String,
    /// Column of the joined source.
    #[serde(default)]
    pub right_column: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
}

/// (kind, id, label, sql) in menu order.
pub const JOIN_KINDS: [(JoinKind, &str, &str, &str); 4] = [
    (JoinKind::Inner, "inner", "Inner join", "INNER JOIN"),
    (JoinKind::Left, "left", "Left join", "LEFT JOIN"),
    (JoinKind::Right, "right", "Right join", "RIGHT JOIN"),
    (JoinKind::Full, "full", "Full outer join", "FULL OUTER JOIN"),
];

impl JoinKind {
    pub fn id(self) -> &'static str {
        JOIN_KINDS.iter().find(|k| k.0 == self).map_or("inner", |k| k.1)
    }
    pub fn sql(self) -> &'static str {
        JOIN_KINDS
            .iter()
            .find(|k| k.0 == self)
            .map_or("INNER JOIN", |k| k.3)
    }
    pub fn from_id(id: &str) -> Option<Self> {
        JOIN_KINDS.iter().find(|k| k.1 == id).map(|k| k.0)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderJoin {
    pub id: String,
    pub kind: JoinKind,
    /// Alias of the joined source.
    pub source: String,
    #[serde(default)]
    pub conditions: Vec<JoinCondition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderField {
    pub id: String,
    pub source: String,
    /// Column name, or `*` (only with `count`).
    pub column: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(default)]
    pub aggregate: Option<Aggregate>,
    /// False for a field used only for sorting: it is not output.
    #[serde(default)]
    pub selected: bool,
}

impl BuilderField {
    pub fn new(source: &str, column: &str) -> Self {
        Self {
            id: new_id(),
            source: source.into(),
            column: column.into(),
            alias: None,
            aggregate: None,
            selected: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterOperator {
    #[serde(rename = "=")]
    Eq,
    #[serde(rename = "<>")]
    Ne,
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = "<=")]
    Le,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = ">=")]
    Ge,
    #[serde(rename = "contains")]
    Contains,
    #[serde(rename = "startsWith")]
    StartsWith,
    #[serde(rename = "isNull")]
    IsNull,
    #[serde(rename = "isNotNull")]
    IsNotNull,
}

/// (operator, id, label) in menu order.
pub const FILTER_OPERATORS: [(FilterOperator, &str, &str); 10] = [
    (FilterOperator::Eq, "=", "equals"),
    (FilterOperator::Ne, "<>", "does not equal"),
    (FilterOperator::Lt, "<", "less than"),
    (FilterOperator::Le, "<=", "at most"),
    (FilterOperator::Gt, ">", "greater than"),
    (FilterOperator::Ge, ">=", "at least"),
    (FilterOperator::Contains, "contains", "contains"),
    (FilterOperator::StartsWith, "startsWith", "starts with"),
    (FilterOperator::IsNull, "isNull", "is empty"),
    (FilterOperator::IsNotNull, "isNotNull", "is not empty"),
];

impl FilterOperator {
    pub fn id(self) -> &'static str {
        FILTER_OPERATORS
            .iter()
            .find(|o| o.0 == self)
            .map_or("=", |o| o.1)
    }
    pub fn from_id(id: &str) -> Option<Self> {
        FILTER_OPERATORS.iter().find(|o| o.1 == id).map(|o| o.0)
    }
    pub fn unary(self) -> bool {
        matches!(self, FilterOperator::IsNull | FilterOperator::IsNotNull)
    }
}

/// A literal in a filter: `string | number | boolean | null` in the JSON model.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum Literal {
    Null,
    Bool(bool),
    Number(f64),
    Text(String),
}

impl Serialize for Literal {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Literal::Null => s.serialize_none(),
            Literal::Bool(b) => s.serialize_bool(*b),
            // Whole numbers stay integers, as the React UI wrote them.
            Literal::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => s.serialize_i64(*n as i64),
            Literal::Number(n) => s.serialize_f64(*n),
            Literal::Text(t) => s.serialize_str(t),
        }
    }
}

impl Literal {
    /// How the input box shows the value (`String(value)`, empty for null).
    pub fn display(&self) -> String {
        match self {
            Literal::Null => String::new(),
            Literal::Bool(b) => b.to_string(),
            Literal::Number(n) => n.to_string(),
            Literal::Text(t) => t.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FilterValue {
    Value { value: Literal },
    Param { name: String },
}

impl FilterValue {
    pub fn blank() -> Self {
        FilterValue::Value {
            value: Literal::Text(String::new()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderCondition {
    pub id: String,
    pub source: String,
    pub column: String,
    /// Only in `having`: compare an aggregate of the column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aggregate: Option<Aggregate>,
    pub operator: FilterOperator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<FilterValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Combinator {
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Item {
    Condition(BuilderCondition),
    Group(BuilderGroup),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderGroup {
    pub id: String,
    pub combinator: Combinator,
    #[serde(default)]
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderSort {
    pub id: String,
    pub field_id: String,
    pub direction: Direction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GroupColumn {
    pub source: String,
    pub column: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderModel {
    pub version: u32,
    pub sources: Vec<BuilderSource>,
    pub joins: Vec<BuilderJoin>,
    pub fields: Vec<BuilderField>,
    pub filters: BuilderGroup,
    /// Explicit grouping. Non-aggregated selected fields are grouped automatically
    /// when any field aggregates.
    pub group_by: Vec<GroupColumn>,
    pub having: BuilderGroup,
    pub order_by: Vec<BuilderSort>,
    /// Serialized as `null` when absent, like the React model.
    pub limit: Option<f64>,
}

pub fn empty_group() -> BuilderGroup {
    BuilderGroup {
        id: new_id(),
        combinator: Combinator::And,
        items: vec![],
    }
}

pub fn empty_model() -> BuilderModel {
    BuilderModel {
        version: 1,
        sources: vec![],
        joins: vec![],
        fields: vec![],
        filters: empty_group(),
        group_by: vec![],
        having: empty_group(),
        order_by: vec![],
        limit: None,
    }
}

/// Fills missing collections so older or hand-edited models load.
pub fn normalize_model(raw: &Value) -> BuilderModel {
    fn list<T: for<'de> Deserialize<'de>>(raw: &Value, key: &str) -> Vec<T> {
        raw.get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default()
    }
    fn group(raw: &Value, key: &str) -> BuilderGroup {
        raw.get(key)
            .filter(|g| g.get("items").is_some_and(Value::is_array))
            .and_then(|g| serde_json::from_value(g.clone()).ok())
            .unwrap_or_else(empty_group)
    }
    BuilderModel {
        version: 1,
        sources: list(raw, "sources"),
        joins: list(raw, "joins"),
        fields: list(raw, "fields"),
        filters: group(raw, "filters"),
        group_by: list(raw, "groupBy"),
        having: group(raw, "having"),
        order_by: list(raw, "orderBy"),
        limit: raw.get("limit").and_then(Value::as_f64),
    }
}

impl BuilderModel {
    /// The JSON stored in `SavedQuery.builder` (the two top-level groups carry
    /// `"kind": "group"`, as the React model wrote them).
    pub fn to_value(&self) -> Value {
        let mut value = serde_json::to_value(self).unwrap_or(Value::Null);
        for key in ["filters", "having"] {
            if let Some(group) = value.get_mut(key).and_then(Value::as_object_mut) {
                group.insert("kind".into(), Value::String("group".into()));
            }
        }
        value
    }

    /// True when the model holds nothing worth confirming before it is discarded.
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

/// A short alias for a table that is not yet used in the model (`orders` -> `o`, `o2`).
pub fn alias_for(table: &str, used: &[String]) -> String {
    let base = table
        .chars()
        .find(|c| c.is_ascii_alphabetic())
        .map_or('t', |c| c.to_ascii_lowercase())
        .to_string();
    if !used.contains(&base) {
        return base;
    }
    let mut n = 2;
    while used.contains(&format!("{base}{n}")) {
        n += 1;
    }
    format!("{base}{n}")
}

/// Every condition in a group tree, depth first.
pub fn conditions_of(group: &BuilderGroup) -> Vec<&BuilderCondition> {
    group
        .items
        .iter()
        .flat_map(|item| match item {
            Item::Group(g) => conditions_of(g),
            Item::Condition(c) => vec![c],
        })
        .collect()
}

/// Parameter names referenced by filters and having conditions.
pub fn builder_parameter_names(model: &BuilderModel) -> Vec<String> {
    let mut names: Vec<String> = vec![];
    for c in conditions_of(&model.filters)
        .into_iter()
        .chain(conditions_of(&model.having))
    {
        if let Some(FilterValue::Param { name }) = &c.value {
            if !name.is_empty() && !names.contains(name) {
                names.push(name.clone());
            }
        }
    }
    names
}

/// Display name of a field: `alias.column`, or Row count for `count(*)`.
pub fn field_name(f: &BuilderField) -> String {
    if f.column == "*" {
        "Row count".into()
    } else {
        format!("{}.{}", f.source, f.column)
    }
}
