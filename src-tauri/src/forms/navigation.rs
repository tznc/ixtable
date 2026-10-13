//! Runtime pages and navigation (port of the non-hook parts of `src/runtime/navigation.ts`).

use super::generate::humanize;
use super::rbac::{Operation, can};
use crate::archive::DocumentConfig;
use crate::design::{NavKind, NavigationItem};
use crate::pages::ComponentKind;
use serde_json::{Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageKind {
    Form,
    Report,
    Page,
    Table,
}

impl PageKind {
    pub fn key(self) -> &'static str {
        match self {
            PageKind::Form => "form",
            PageKind::Report => "report",
            PageKind::Page => "page",
            PageKind::Table => "table",
        }
    }
    pub fn parse(key: &str) -> Option<Self> {
        Some(match key {
            "form" => PageKind::Form,
            "report" => PageKind::Report,
            "page" | "dashboard" => PageKind::Page,
            "table" => PageKind::Table,
            _ => return None,
        })
    }
}

/// A page the Runtime shows.
#[derive(Clone, Debug, PartialEq)]
pub struct RuntimePage {
    pub kind: PageKind,
    pub id: String,
    pub mode: Option<String>,
    pub record_id: Option<Value>,
    pub params: Map<String, Value>,
    /// Navigation item that opened the page, for highlighting.
    pub nav_id: Option<String>,
}

impl RuntimePage {
    pub fn new(kind: PageKind, id: impl Into<String>) -> Self {
        Self {
            kind,
            id: id.into(),
            mode: None,
            record_id: None,
            params: Map::new(),
            nav_id: None,
        }
    }
}

/// Page opened by a navigation item; None for groups and items without a target.
pub fn page_for(item: &NavigationItem) -> Option<RuntimePage> {
    let target = item.target_id.as_ref().filter(|t| !t.is_empty())?;
    let kind = match item.kind {
        NavKind::Group => return None,
        NavKind::Form => PageKind::Form,
        NavKind::Report => PageKind::Report,
        NavKind::Page => PageKind::Page,
        NavKind::Table => PageKind::Table,
    };
    Some(RuntimePage {
        mode: item.mode.map(|m| super::schema::form_mode_key(m).to_string()),
        nav_id: Some(item.id.clone()),
        ..RuntimePage::new(kind, target.clone())
    })
}

/// True when the role may open what the page points at.
pub fn can_open(config: &DocumentConfig, role_id: Option<&str>, page: &RuntimePage) -> bool {
    can(config, role_id, page.kind.key(), &page.id, Operation::Read)
}

/// Navigation filtered by RBAC: items need `view` on the item and `read` on the target; empty
/// groups drop out.
pub fn visible_navigation(
    items: &[NavigationItem],
    config: &DocumentConfig,
    role_id: Option<&str>,
) -> Vec<NavigationItem> {
    items
        .iter()
        .filter_map(|item| {
            if !can(config, role_id, "navigation", &item.id, Operation::View) {
                return None;
            }
            if item.kind == NavKind::Group {
                let children = visible_navigation(&item.children, config, role_id);
                return (!children.is_empty()).then(|| NavigationItem {
                    children,
                    ..item.clone()
                });
            }
            page_for(item)
                .filter(|page| can_open(config, role_id, page))
                .map(|_| item.clone())
        })
        .collect()
}

fn flatten(items: &[NavigationItem]) -> Vec<&NavigationItem> {
    items
        .iter()
        .flat_map(|item| std::iter::once(item).chain(flatten(&item.children)))
        .collect()
}

/// The start page for a role: the design's start page when visible, else the first visible item.
pub fn start_page(config: &DocumentConfig, role_id: Option<&str>) -> Option<RuntimePage> {
    let visible = visible_navigation(&config.design.navigation, config, role_id);
    let flat = flatten(&visible);
    let preferred = flat
        .iter()
        .find(|item| Some(&item.id) == config.design.start_page.as_ref());
    let item = preferred
        .copied()
        .or_else(|| flat.iter().find(|i| i.kind != NavKind::Group).copied())?;
    page_for(item)
}

/// What the Runtime shows for `page`: a page holding only one form or report shows that form
/// or report full page (the navigation item's form mode wins over the component's); anything
/// else is shown as is.
pub fn solo_page(config: &DocumentConfig, page: &RuntimePage) -> RuntimePage {
    let solo = (page.kind == PageKind::Page)
        .then(|| config.pages.iter().find(|p| p.id == page.id))
        .flatten()
        .and_then(|p| match p.components.as_slice() {
            [only] => Some(only),
            _ => None,
        });
    let Some(component) = solo else {
        return page.clone();
    };
    let (kind, target) = match component.kind {
        ComponentKind::Form => (PageKind::Form, component.form_id.as_ref()),
        ComponentKind::Report => (PageKind::Report, component.report_id.as_ref()),
        _ => return page.clone(),
    };
    let Some(target) = target.filter(|t| !t.is_empty()) else {
        return page.clone();
    };
    let mode = page
        .mode
        .clone()
        .or_else(|| component.mode.map(|m| super::schema::form_mode_key(m).to_string()));
    RuntimePage {
        kind,
        id: target.clone(),
        mode: if kind == PageKind::Form { mode } else { None },
        ..page.clone()
    }
}

/// A page's title for back links: its navigation label, else the target's name.
pub fn page_title(config: &DocumentConfig, page: &RuntimePage) -> String {
    let labelled = page.nav_id.as_ref().and_then(|nav| {
        flatten(&config.design.navigation)
            .into_iter()
            .find(|item| &item.id == nav)
            .map(|item| item.label.clone())
            .filter(|l| !l.is_empty())
    });
    if let Some(label) = labelled {
        return label;
    }
    let name = match page.kind {
        PageKind::Form => config
            .design
            .forms
            .iter()
            .find(|f| f.id == page.id)
            .map(|f| f.name.clone()),
        PageKind::Report => config
            .reports
            .iter()
            .find(|r| r.id == page.id)
            .map(|r| r.name.clone()),
        PageKind::Page => config
            .pages
            .iter()
            .find(|d| d.id == page.id)
            .map(|d| d.name.clone()),
        PageKind::Table => Some(humanize(&page.id)),
    };
    name.filter(|n| !n.is_empty())
        .unwrap_or_else(|| "previous page".into())
}

#[cfg(test)]
#[path = "navigation_tests.rs"]
mod tests;
