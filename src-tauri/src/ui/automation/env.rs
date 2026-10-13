//! What the editors offer to pick from, read from the open document: tables, forms,
//! reports, dashboards, saved queries and actions as (id, label) pairs.

use crate::ui::shell::Shell;
use gpui_kit::App;

#[derive(Clone, Debug, Default)]
pub struct Env {
    pub tables: Vec<(String, String)>,
    pub forms: Vec<(String, String)>,
    pub reports: Vec<(String, String)>,
    pub dashboards: Vec<(String, String)>,
    pub queries: Vec<(String, String)>,
    pub actions: Vec<(String, String)>,
}

fn pairs<'a>(items: impl Iterator<Item = (&'a str, &'a str)>, fallback: &str) -> Vec<(String, String)> {
    items
        .map(|(id, name)| {
            let label = if name.is_empty() { fallback_label(id, fallback) } else { name.to_string() };
            (id.to_string(), label)
        })
        .collect()
}

fn fallback_label(id: &str, fallback: &str) -> String {
    if fallback.is_empty() {
        id.to_string()
    } else {
        fallback.to_string()
    }
}

impl Env {
    pub fn read(shell: &Shell, cx: &App) -> Self {
        let config = shell.config(cx);
        Self {
            tables: shell
                .objects
                .iter()
                .filter(|o| o.object_type == "table" && !o.name.starts_with("_ixtable_"))
                .map(|o| (o.name.clone(), o.name.clone()))
                .collect(),
            forms: pairs(config.design.forms.iter().map(|f| (f.id.as_str(), f.name.as_str())), ""),
            reports: pairs(config.reports.iter().map(|r| (r.id.as_str(), r.name.as_str())), ""),
            dashboards: pairs(
                config.dashboards.iter().map(|d| (d.id.as_str(), d.name.as_str())),
                "",
            ),
            queries: pairs(
                config.saved_queries.iter().map(|q| (q.id.as_str(), q.name.as_str())),
                "Untitled query",
            ),
            actions: pairs(config.actions.iter().map(|a| (a.id.as_str(), a.name.as_str())), ""),
        }
    }
}

/// The form modes a form step can open, with their labels.
pub const FORM_MODES: [(&str, &str); 6] = [
    ("list", "List"),
    ("detail", "Detail"),
    ("create", "Create"),
    ("edit", "Edit"),
    ("continuous", "Continuous"),
    ("split", "Split"),
];
