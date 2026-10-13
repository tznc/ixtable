//! Links between pages (`docs/decisions/pages.md`): navigation items, page buttons and
//! "Open page" action steps, the parameters each passes, and the checks that every link
//! reaches a page and supplies its required parameters.

use crate::archive::{DocumentConfig, Issue};
use crate::design::{NavKind, NavigationItem};
use crate::forms::schema::flatten_navigation;
use crate::pages::{ComponentKind, Page};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Where a link starts.
#[derive(Clone, Debug, PartialEq)]
pub enum LinkSource {
    Navigation { item: String, label: String },
    Button { page: String, page_name: String, component: String, title: String },
    Step { action: String, action_name: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageLink {
    pub source: LinkSource,
    pub target: String,
    /// Parameter name to the expression that supplies it.
    pub params: BTreeMap<String, String>,
}

impl PageLink {
    /// "Navigation item Orders opens Orders (region = 'East')".
    pub fn describe_with(&self, pages: &[Page]) -> String {
        let from = match &self.source {
            LinkSource::Navigation { label, .. } => format!("Navigation item {label}"),
            LinkSource::Button { page_name, title, .. } => format!("Button {title} on {page_name}"),
            LinkSource::Step { action_name, .. } => format!("Action {action_name}"),
        };
        let to = pages.iter().find(|p| p.id == self.target).map_or("a missing page".to_string(), |p| p.name.clone());
        let params: Vec<String> = self.params.iter().map(|(k, v)| format!("{k} = {v}")).collect();
        if params.is_empty() {
            format!("{from} opens {to}")
        } else {
            format!("{from} opens {to} ({})", params.join(", "))
        }
    }
}

/// Every link to a page in the document, in navigation, page and action order.
pub fn page_links(config: &DocumentConfig) -> Vec<PageLink> {
    let mut links = vec![];
    for item in flatten_navigation(&config.design.navigation) {
        if let Some(link) = nav_link(item) {
            links.push(link);
        }
    }
    for page in &config.pages {
        for c in page.components.iter().filter(|c| c.kind == ComponentKind::Button) {
            let Some(target) = c.target_page_id.clone().filter(|t| !t.is_empty()) else { continue };
            links.push(PageLink {
                source: LinkSource::Button {
                    page: page.id.clone(),
                    page_name: page.name.clone(),
                    component: c.id.clone(),
                    title: c.label.clone().filter(|l| !l.is_empty()).unwrap_or_else(|| c.title.clone()),
                },
                target,
                params: c.page_params.clone(),
            });
        }
    }
    for action in &config.actions {
        for step in &action.steps {
            let value = serde_json::to_value(step).unwrap_or(Value::Null);
            collect_open_page(&value, &mut |target, params| {
                links.push(PageLink {
                    source: LinkSource::Step { action: action.id.clone(), action_name: action.name.clone() },
                    target,
                    params,
                })
            });
        }
    }
    links
}

fn nav_link(item: &NavigationItem) -> Option<PageLink> {
    let target = item.target_id.clone().filter(|t| !t.is_empty() && item.kind == NavKind::Page)?;
    Some(PageLink {
        source: LinkSource::Navigation { item: item.id.clone(), label: item.label.clone() },
        target,
        params: item.params.clone(),
    })
}

/// Calls `found` for every `openDashboard` step in `step` and its branches.
fn collect_open_page(step: &Value, found: &mut impl FnMut(String, BTreeMap<String, String>)) {
    if step["kind"] == "openDashboard" {
        let target = step["dashboardId"].as_str().unwrap_or("").to_string();
        if !target.is_empty() {
            let params = step["params"]
                .as_object()
                .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string())).collect())
                .unwrap_or_default();
            found(target, params);
        }
    }
    for branch in ["then", "else"] {
        for child in step[branch].as_array().into_iter().flatten() {
            collect_open_page(child, found);
        }
    }
}

/// Problems with page links: a missing target page, a required parameter left out, or a
/// parameter the page does not declare. Also checks each page's own parameter list.
pub fn link_issues(config: &DocumentConfig) -> Vec<Issue> {
    let mut issues = vec![];
    for page in &config.pages {
        let mut seen = std::collections::HashSet::new();
        for param in &page.parameters {
            let name = param.name.trim();
            if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                issues.push(Issue::error("page", &page.id, format!("{}: parameter \"{name}\" needs a name of letters, digits and _", page.name)));
            } else if !seen.insert(name.to_string()) {
                issues.push(Issue::error("page", &page.id, format!("{}: parameter {name} is declared twice", page.name)));
            }
        }
    }
    for link in page_links(config) {
        let (kind, id) = match &link.source {
            LinkSource::Navigation { item, .. } => ("navigation", item.clone()),
            LinkSource::Button { component, .. } => ("pageComponent", component.clone()),
            LinkSource::Step { action, .. } => ("action", action.clone()),
        };
        let what = link.describe_with(&config.pages);
        let Some(page) = config.pages.iter().find(|p| p.id == link.target) else {
            // Steps and navigation items report missing targets themselves.
            if matches!(link.source, LinkSource::Button { .. }) {
                issues.push(Issue::error(kind, &id, format!("{what}: the page does not exist")));
            }
            continue;
        };
        for param in page.parameters.iter().filter(|p| p.required && p.default_value.is_none()) {
            if link.params.get(&param.name).is_none_or(|v| v.trim().is_empty()) {
                issues.push(Issue::error(kind, &id, format!("{what}: required parameter {} has no value", param.name)));
            }
        }
        for name in link.params.keys() {
            if !page.parameters.iter().any(|p| &p.name == name) && !page.filters.iter().any(|f| &f.param == name) {
                issues.push(Issue::warning(kind, &id, format!("{what}: {} has no parameter {name}", page.name)));
            }
        }
    }
    issues
}

/// `params` with the page's declared defaults for the names it leaves out.
pub fn with_defaults(page: &Page, mut params: Map<String, Value>) -> Map<String, Value> {
    for param in &page.parameters {
        if let Some(default) = &param.default_value {
            params.entry(param.name.clone()).or_insert_with(|| default.clone());
        }
    }
    params
}

/// Evaluates parameter expressions in `scope`. An expression that fails gives null.
pub fn evaluate_params(params: &BTreeMap<String, String>, scope: &crate::expr::Scope) -> Map<String, Value> {
    params
        .iter()
        .filter(|(_, src)| !src.trim().is_empty())
        .map(|(name, src)| {
            let value = crate::expr::evaluate(src, scope, Default::default()).unwrap_or(Value::Null);
            (name.clone(), value)
        })
        .collect()
}

#[cfg(test)]
#[path = "links_tests.rs"]
mod tests;
