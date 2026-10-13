//! Config version 8: forms, reports and dashboards become pages
//! (`docs/decisions/pages.md`).
//!
//! Dashboards already are pages (serde reads the old `dashboards` key). Every form and
//! report that no page shows gets a page of its own holding it full width, and navigation
//! items and role permissions that pointed at the form or report now point at that page.
//! Wrapper page ids derive from the form or report id (UUIDv7-shaped), so loading the same
//! older document twice gives the same ids.
use super::{ComponentKind, Page, PageComponent};
use crate::archive::DocumentConfig;
use crate::design::{FormMode, GridLayout, NavKind, NavigationItem, Placement};
use crate::roles::ObjectPermission;
use std::collections::HashMap;

/// First config version that stores pages.
pub const PAGES_VERSION: u32 = 8;

/// Grid rows a wrapped form or report spans in the page designer.
const WRAPPED_ROWS: u16 = 4;

/// Applies the version 8 changes to a config read at `version`.
pub fn upgrade(config: &mut DocumentConfig, version: u32) {
    for role in &mut config.roles {
        for object in &mut role.permissions.objects {
            if object.kind == "dashboard" {
                object.kind = "page".into();
            }
        }
    }
    if version < PAGES_VERSION {
        wrap_definitions(config);
    }
}

/// Gives every form and report that no page shows a page of its own, and points
/// navigation and role permissions at it. Returns (form or report id, page id) pairs.
pub fn wrap_definitions(config: &mut DocumentConfig) -> Vec<(String, String)> {
    let placed = |kind: ComponentKind, id: &str, pages: &[Page]| {
        pages.iter().any(|p| {
            p.components.iter().any(|c| {
                c.kind == kind
                    && match kind {
                        ComponentKind::Form => c.form_id.as_deref() == Some(id),
                        _ => c.report_id.as_deref() == Some(id),
                    }
            })
        })
    };
    let mut forms = HashMap::new();
    let mut reports = HashMap::new();
    let mut added = vec![];
    for form in &config.design.forms {
        // A duplicate id (a validation error of its own) gets one page.
        if forms.contains_key(&form.id) || placed(ComponentKind::Form, &form.id, &config.pages) {
            continue;
        }
        let mode = form.modes.first().copied().unwrap_or(FormMode::List);
        let page = wrapper_page("form", &form.id, &form.name, ComponentKind::Form, |c| {
            c.form_id = Some(form.id.clone());
            c.mode = Some(mode);
        });
        forms.insert(form.id.clone(), page.id.clone());
        added.push(page);
    }
    for report in &config.reports {
        if reports.contains_key(&report.id) || placed(ComponentKind::Report, &report.id, &config.pages) {
            continue;
        }
        let page = wrapper_page("report", &report.id, &report.name, ComponentKind::Report, |c| {
            c.report_id = Some(report.id.clone());
        });
        reports.insert(report.id.clone(), page.id.clone());
        added.push(page);
    }
    config.pages.extend(added);
    retarget_navigation(&mut config.design.navigation, &forms, &reports);
    for role in &mut config.roles {
        let mut grants = vec![];
        for object in &role.permissions.objects {
            let page = match object.kind.as_str() {
                "form" => forms.get(&object.id),
                "report" => reports.get(&object.id),
                _ => None,
            };
            if let Some(page) = page {
                grants.push(ObjectPermission {
                    kind: "page".into(),
                    id: page.clone(),
                    read: object.read,
                    ..Default::default()
                });
            }
        }
        role.permissions.objects.extend(grants);
    }
    forms.into_iter().chain(reports).collect()
}

/// Keeps pages in step after forms or reports were added or removed outside the page
/// designer (generators, imports, the form list): drops pages whose only component shows a
/// form or report that no longer exists, then gives new forms and reports a page.
pub fn sync(config: &mut DocumentConfig) {
    let (forms, reports) = (&config.design.forms, &config.reports);
    config.pages.retain(|page| match page.components.as_slice() {
        [only] => match only.kind {
            ComponentKind::Form => only.form_id.as_ref().is_none_or(|id| forms.iter().any(|f| &f.id == id)),
            ComponentKind::Report => only.report_id.as_ref().is_none_or(|id| reports.iter().any(|r| &r.id == id)),
            _ => true,
        },
        _ => true,
    });
    wrap_definitions(config);
}

/// The page that holds one form or report.
pub fn wrapper_page(
    kind_key: &str,
    target_id: &str,
    name: &str,
    kind: ComponentKind,
    set: impl FnOnce(&mut PageComponent),
) -> Page {
    let layout = GridLayout::default();
    let mut component = PageComponent {
        id: stable_id(&format!("{kind_key}-component:{target_id}")),
        kind,
        placement: Placement {
            column: 1,
            row: 1,
            column_span: layout.columns.len() as u16,
            row_span: WRAPPED_ROWS,
            region: None,
        },
        ..Default::default()
    };
    set(&mut component);
    Page {
        id: stable_id(&format!("{kind_key}-page:{target_id}")),
        name: name.to_string(),
        layout,
        filters: vec![],
        components: vec![component],
        parameters: vec![],
    }
}

/// Whether `page` is the page the migration made for a form or report (its id derives
/// from the definition it holds).
#[cfg(test)]
pub(crate) fn is_wrapper(page: &Page) -> bool {
    let [only] = page.components.as_slice() else { return false };
    let target = match only.kind {
        ComponentKind::Form => only.form_id.as_ref().map(|id| ("form", id)),
        ComponentKind::Report => only.report_id.as_ref().map(|id| ("report", id)),
        _ => None,
    };
    target.is_some_and(|(key, id)| page.id == stable_id(&format!("{key}-page:{id}")))
}

/// A UUIDv7-shaped id derived from `seed` (every definition id is a UUIDv7). The bytes
/// come from a UUIDv5 of the seed, so the same seed always gives the same id.
fn stable_id(seed: &str) -> String {
    let bytes = *uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, seed.as_bytes()).as_bytes();
    uuid::Builder::from_bytes(bytes).with_version(uuid::Version::SortRand).with_variant(uuid::Variant::RFC4122).into_uuid().to_string()
}

fn retarget_navigation(
    items: &mut [NavigationItem],
    forms: &HashMap<String, String>,
    reports: &HashMap<String, String>,
) {
    for item in items {
        let map = match item.kind {
            NavKind::Form => Some(forms),
            NavKind::Report => Some(reports),
            _ => None,
        };
        if let Some(page) = map.and_then(|m| item.target_id.as_ref().and_then(|t| m.get(t))) {
            item.kind = NavKind::Page;
            item.target_id = Some(page.clone());
        }
        retarget_navigation(&mut item.children, forms, reports);
    }
}

#[cfg(test)]
#[path = "upgrade_tests.rs"]
mod tests;
