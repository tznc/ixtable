//! Saved-query checks for the Problems tab.
use super::*;

/// The visual builder model stored in `SavedQuery.builder` (mirrors
/// src/query/builder/model.ts). Only fields needed for consistency checks.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Builder {
    sources: Vec<BuilderSource>,
    joins: Vec<BuilderJoin>,
    fields: Vec<BuilderField>,
    filters: Option<BuilderGroup>,
    having: Option<BuilderGroup>,
    group_by: Vec<BuilderColumn>,
    order_by: Vec<BuilderOrder>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BuilderSource {
    table: String,
    alias: String,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BuilderJoin {
    kind: String,
    source: String,
    conditions: Vec<BuilderJoinCondition>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BuilderJoinCondition {
    left_source: String,
    left_column: String,
    right_column: String,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BuilderColumn {
    source: String,
    column: String,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BuilderField {
    id: String,
    source: String,
    column: String,
    aggregate: Option<String>,
    selected: bool,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BuilderOrder {
    field_id: String,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BuilderGroup {
    items: Vec<BuilderFilterItem>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct BuilderFilterItem {
    kind: String,
    source: String,
    column: String,
    aggregate: Option<String>,
    value: Option<serde_json::Value>,
    items: Vec<BuilderFilterItem>,
}

const AGGREGATES: [&str; 6] = ["count", "sum", "avg", "min", "max", "countDistinct"];

fn check_builder(query: &SavedQuery, builder: &serde_json::Value) -> Vec<String> {
    let model: Builder = match serde_json::from_value(builder.clone()) {
        Ok(m) => m,
        Err(e) => return vec![format!("builder model is malformed: {e}")],
    };
    let mut problems = vec![];
    let mut aliases = HashSet::new();
    if model.sources.is_empty() {
        problems.push("builder has no source table".into());
    }
    for s in &model.sources {
        if s.table.trim().is_empty() || s.alias.trim().is_empty() {
            problems.push("every builder source needs a table and an alias".into());
        } else if !aliases.insert(s.alias.as_str()) {
            problems.push(format!("builder source alias {:?} is used twice", s.alias));
        }
    }
    let known = |alias: &str| aliases.contains(alias);
    let base = model
        .sources
        .first()
        .map(|s| s.alias.as_str())
        .unwrap_or("");
    for j in &model.joins {
        if !matches!(j.kind.as_str(), "inner" | "left" | "right" | "full") {
            problems.push(format!(
                "join kind {:?} must be inner, left, right, or full",
                j.kind
            ));
        }
        if !known(&j.source) || j.source == base {
            problems.push(format!("join refers to unknown source {:?}", j.source));
        }
        if j.conditions.is_empty() {
            problems.push(format!("join to {:?} has no condition", j.source));
        }
        for c in &j.conditions {
            if !known(&c.left_source) || c.left_column.is_empty() || c.right_column.is_empty() {
                problems.push(format!(
                    "join to {:?} has an incomplete condition",
                    j.source
                ));
            }
        }
    }
    let joined: HashSet<&str> = model.joins.iter().map(|j| j.source.as_str()).collect();
    for s in model.sources.iter().skip(1) {
        if !joined.contains(s.alias.as_str()) {
            problems.push(format!("source {:?} is not joined", s.alias));
        }
    }
    let mut field_ids = HashSet::new();
    for f in &model.fields {
        field_ids.insert(f.id.as_str());
        if !known(&f.source) {
            problems.push(format!(
                "field {:?} refers to unknown source {:?}",
                f.column, f.source
            ));
        }
        if let Some(a) = &f.aggregate {
            if !AGGREGATES.contains(&a.as_str()) {
                problems.push(format!("unknown aggregate {a:?}"));
            }
        }
    }
    // compileBuilder groups non-aggregated fields itself; only explicit entries are checked.
    for g in &model.group_by {
        if !known(&g.source) {
            problems.push(format!("group by refers to unknown source {:?}", g.source));
        }
    }
    for o in &model.order_by {
        if !field_ids.contains(o.field_id.as_str()) {
            problems.push("sort refers to a missing field".into());
        }
    }
    let declared: HashSet<&str> = query.parameters.iter().map(|p| p.name.as_str()).collect();
    fn walk<'a>(items: &'a [BuilderFilterItem], out: &mut Vec<&'a BuilderFilterItem>) {
        for i in items {
            if i.kind == "group" {
                walk(&i.items, out)
            } else {
                out.push(i)
            }
        }
    }
    for (group, is_having) in [(&model.filters, false), (&model.having, true)] {
        let mut conditions = vec![];
        if let Some(g) = group {
            walk(&g.items, &mut conditions);
        }
        for c in conditions {
            if !known(&c.source) {
                problems.push(format!("filter refers to unknown source {:?}", c.source));
            }
            if !is_having && c.aggregate.is_some() {
                problems.push(format!(
                    "filter on {} uses an aggregate; move it to Having",
                    c.column
                ));
            }
            if let Some(name) = c
                .value
                .as_ref()
                .filter(|v| v.get("kind").and_then(|k| k.as_str()) == Some("param"))
                .and_then(|v| v.get("name"))
                .and_then(|n| n.as_str())
            {
                if !declared.contains(name) {
                    problems.push(format!("filter uses undeclared parameter ${name}"));
                }
            }
        }
    }
    problems
}

/// Saved-query checks for the Problems tab: unique names, parameters, read-only SQL,
/// and builder consistency.
pub fn validate(config: &DocumentConfig) -> Vec<Issue> {
    const KIND: &str = "query";
    let mut issues = vec![];
    let mut names: HashMap<String, &str> = HashMap::new();
    for q in &config.saved_queries {
        let id = q.id.as_str();
        let key = q.name.trim().to_lowercase();
        if !key.is_empty() {
            if let Some(other) = names.insert(key, id) {
                if other != id {
                    issues.push(Issue::error(
                        KIND,
                        id,
                        format!("Another query is also named \"{}\"", q.name),
                    ));
                }
            }
        }
        if q.sql.trim().is_empty() {
            issues.push(Issue::error(
                KIND,
                id,
                format!("Query \"{}\" has no SQL", q.name),
            ));
        } else {
            let checked = match &q.action {
                Some(spec) => action::check_sql(&q.sql, spec, &action::schema_of(config)),
                None => prepare_sql(&q.sql),
            };
            match checked {
                Err(e) => issues.push(Issue::error(
                    KIND,
                    id,
                    format!("Query \"{}\": {}", q.name, e.message),
                )),
                Ok(r) => {
                    for name in r
                        .names
                        .iter()
                        .filter(|n| !q.parameters.iter().any(|p| &p.name == *n))
                    {
                        issues.push(Issue::error(
                            KIND,
                            id,
                            format!(
                                "Query \"{}\" uses ${name}, but declares no parameter {name}",
                                q.name
                            ),
                        ));
                    }
                    for p in q.parameters.iter().filter(|p| !r.names.contains(&p.name)) {
                        issues.push(Issue::warning(
                            KIND,
                            id,
                            format!(
                                "Query \"{}\" declares parameter {} but never uses ${}",
                                q.name, p.name, p.name
                            ),
                        ));
                    }
                }
            }
        }
        let mut seen = HashSet::new();
        for p in &q.parameters {
            let valid_name =
                p.name.chars().next().is_some_and(ident_start) && p.name.chars().all(ident_char);
            if !valid_name {
                issues.push(Issue::error(
                    KIND,
                    id,
                    format!(
                        "Query \"{}\" has an invalid parameter name {:?}",
                        q.name, p.name
                    ),
                ));
            }
            if !seen.insert(p.name.as_str()) {
                issues.push(Issue::error(
                    KIND,
                    id,
                    format!("Query \"{}\" declares parameter {} twice", q.name, p.name),
                ));
            }
            if logical_type(&p.logical_type).is_none() {
                issues.push(Issue::error(
                    KIND,
                    id,
                    format!(
                        "Parameter {} has unsupported type {:?}",
                        p.name, p.logical_type
                    ),
                ));
            } else if let Some(default) = &p.default_value {
                if let Err(e) = bind_value(&p.name, Some(&p.logical_type), &json_value(default)) {
                    issues.push(Issue::error(KIND, id, format!("Default value: {e}")));
                }
            }
        }
        if q.action.is_some() {
            for user in readers_of(config, id) {
                issues.push(Issue::error(
                    KIND,
                    id,
                    format!(
                        "{user} reads \"{}\", but it is an action query and returns no rows",
                        q.name
                    ),
                ));
            }
        }
        if let Some(builder) = &q.builder {
            for problem in check_builder(q, builder) {
                issues.push(Issue::error(
                    KIND,
                    id,
                    format!("Query \"{}\": {problem}", q.name),
                ));
            }
        }
    }
    issues
}

/// Forms, reports and dashboards that read saved query `id`, by name.
fn readers_of(config: &DocumentConfig, id: &str) -> Vec<String> {
    let forms = config
        .design
        .forms
        .iter()
        .filter(|f| crate::authz::form_queries(f).any(|q| q == id))
        .map(|f| format!("Form \"{}\"", f.name));
    let reports = config
        .reports
        .iter()
        .filter(|r| {
            r.dataset_query_id.as_deref() == Some(id)
                || crate::authz::bands(r).iter().any(|b| {
                    b.components
                        .iter()
                        .any(|c| c.extra.get("queryId").and_then(|v| v.as_str()) == Some(id))
                })
        })
        .map(|r| format!("Report \"{}\"", r.name));
    let dashboards = config
        .dashboards
        .iter()
        .filter(|d| crate::authz::dashboard_queries(d).any(|q| q == id))
        .map(|d| format!("Dashboard \"{}\"", d.name));
    forms.chain(reports).chain(dashboards).collect()
}
