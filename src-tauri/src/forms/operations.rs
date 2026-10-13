//! Pure edits of a design: controls, grids, forms and navigation (port of
//! `src/design/operations.ts`). Each returns a new value; the caller applies it with
//! `ConfigStore::update`.

use super::schema::{flatten_navigation, new_control, new_id};
use crate::design::{
    Control, ControlKind, ControlParent, DesignSchema, Form, GridLayout, GridTrack, NavKind, NavigationItem, Placement, TrackKind,
};
use crate::ui::grid::engine::{clamp_regions, remap_region};
use std::collections::{HashMap, HashSet};

/// Keeps a placement inside a grid with `columns` base columns.
pub fn clamp_placement(placement: &Placement, columns: usize) -> Placement {
    if placement.region.is_some() {
        return placement.clone();
    }
    let count = columns.max(1) as i64;
    let span = (placement.column_span as i64).clamp(1, count);
    let column = (placement.column as i64).clamp(1, count - span + 1);
    Placement {
        column: column as u16,
        column_span: span as u16,
        ..placement.clone()
    }
}

/// Ids of a control and everything nested inside it (sections, tab pages). With `tab`, only
/// what sits on that tab page (the control itself is left out).
pub fn descendants(form: &Form, id: &str, tab: Option<&str>) -> HashSet<String> {
    let mut out = HashSet::new();
    if tab.is_none() {
        out.insert(id.to_string());
    }
    let mut frontier: Vec<&Control> = form
        .controls
        .iter()
        .filter(|c| {
            c.parent.as_ref().is_some_and(|p| {
                p.id == id && tab.is_none_or(|t| p.tab.as_deref() == Some(t))
            })
        })
        .collect();
    while !frontier.is_empty() {
        out.extend(frontier.iter().map(|c| c.id.clone()));
        let ids: Vec<&str> = frontier.iter().map(|c| c.id.as_str()).collect();
        frontier = form
            .controls
            .iter()
            .filter(|c| {
                c.parent
                    .as_ref()
                    .is_some_and(|p| ids.contains(&p.id.as_str()))
            })
            .collect();
        // Guards against a parent cycle in hand-edited designs.
        frontier.retain(|c| !out.contains(&c.id));
    }
    out
}

pub fn remove_control(form: &Form, id: &str) -> Form {
    let gone = descendants(form, id, None);
    Form {
        controls: form
            .controls
            .iter()
            .filter(|c| !gone.contains(&c.id))
            .cloned()
            .collect(),
        ..form.clone()
    }
}

/// Removes a tab page from a tab group together with the controls on it.
pub fn remove_tab(form: &Form, tabs_id: &str, tab_id: &str) -> Form {
    let gone = descendants(form, tabs_id, Some(tab_id));
    Form {
        controls: form
            .controls
            .iter()
            .filter(|c| !gone.contains(&c.id))
            .map(|c| {
                let mut c = c.clone();
                if c.id == tabs_id {
                    c.tabs.retain(|t| t.id != tab_id);
                }
                c
            })
            .collect(),
        ..form.clone()
    }
}

/// Replaces a container's (or the form's) grid, keeps its regions inside the columns, and
/// clamps the controls placed on it. Region references follow `renames` (old, new);
/// references to a region that no longer exists are cleared.
pub fn set_layout(
    form: &Form,
    container_id: Option<&str>,
    next: GridLayout,
    renames: &[(String, String)],
) -> Form {
    let columns = next.columns.len();
    let layout = GridLayout {
        named_regions: clamp_regions(&next.named_regions, columns),
        ..next
    };
    Form {
        layout: if container_id.is_some() {
            form.layout.clone()
        } else {
            layout.clone()
        },
        controls: form
            .controls
            .iter()
            .map(|control| {
                let on_grid = control.parent.as_ref().map(|p| p.id.as_str()) == container_id;
                let placement = remap_region(&control.placement, &layout.named_regions, renames);
                let mut updated = control.clone();
                if Some(control.id.as_str()) == container_id {
                    updated.layout = Some(layout.clone());
                }
                if on_grid {
                    updated.placement = clamp_placement(&placement, columns);
                }
                updated
            })
            .collect(),
        ..form.clone()
    }
}

/// Keeps the first `count` tracks and appends `1fr` tracks; authored tracks are never rewritten.
pub fn resize_tracks(tracks: &[GridTrack], count: usize) -> Vec<GridTrack> {
    let length = count.clamp(1, 24);
    (0..length)
        .map(|i| {
            tracks.get(i).cloned().unwrap_or(GridTrack {
                kind: TrackKind::Fr,
                value: Some(1.0),
                min: None,
                max: None,
            })
        })
        .collect()
}

/// Sets the number of base columns, keeping existing tracks and adding or removing at the end.
pub fn with_column_count(layout: &GridLayout, count: usize) -> GridLayout {
    GridLayout {
        columns: resize_tracks(&layout.columns, count),
        ..layout.clone()
    }
}

/// Adds a control of `kind` to the container `parent` (the form grid when None).
pub fn add_control(
    form: &Form,
    kind: ControlKind,
    parent: Option<&ControlParent>,
) -> (Form, Control) {
    let control = new_control(kind, form, parent);
    let mut next = form.clone();
    next.controls.push(control.clone());
    (next, control)
}

/// Moves a control into another container, placing it at the first free slot there.
pub fn move_to_container(form: &Form, id: &str, parent: Option<ControlParent>) -> Form {
    let Some(control) = form.controls.iter().find(|c| c.id == id) else {
        return form.clone();
    };
    if parent
        .as_ref()
        .is_some_and(|p| descendants(form, id, None).contains(&p.id))
    {
        return form.clone();
    }
    let others = Form {
        controls: form
            .controls
            .iter()
            .filter(|c| c.id != id)
            .cloned()
            .collect(),
        ..form.clone()
    };
    let placed = new_control(control.kind, &others, parent.as_ref()).placement;
    Form {
        controls: form
            .controls
            .iter()
            .map(|c| {
                if c.id != id {
                    return c.clone();
                }
                let mut moved = c.clone();
                moved.parent = parent.clone();
                moved.placement = Placement {
                    column_span: c.placement.column_span.min(placed.column_span),
                    ..placed.clone()
                };
                moved
            })
            .collect(),
        ..form.clone()
    }
}

/// A deep copy with fresh ids for the form, its controls, tabs and rules.
pub fn duplicate_form(form: &Form, name: Option<&str>) -> Form {
    let ids: HashMap<&str, String> = form
        .controls
        .iter()
        .map(|c| (c.id.as_str(), new_id()))
        .collect();
    let tabs: HashMap<&str, String> = form
        .controls
        .iter()
        .flat_map(|c| c.tabs.iter().map(|t| (t.id.as_str(), new_id())))
        .collect();
    Form {
        id: new_id(),
        name: name
            .map(str::to_string)
            .unwrap_or_else(|| format!("{} copy", form.name)),
        rules: form
            .rules
            .iter()
            .map(|r| {
                let mut r = r.clone();
                r.id = new_id();
                r
            })
            .collect(),
        controls: form
            .controls
            .iter()
            .map(|control| {
                let mut c = control.clone();
                c.id = ids.get(control.id.as_str()).cloned().unwrap_or_else(new_id);
                for tab in &mut c.tabs {
                    tab.id = tabs.get(tab.id.as_str()).cloned().unwrap_or_else(new_id);
                }
                c.parent = control.parent.as_ref().map(|p| ControlParent {
                    id: ids.get(p.id.as_str()).cloned().unwrap_or(p.id.clone()),
                    tab: p
                        .tab
                        .as_ref()
                        .map(|t| tabs.get(t.as_str()).cloned().unwrap_or(t.clone())),
                });
                c
            })
            .collect(),
        ..form.clone()
    }
}

fn prune_navigation(items: &[NavigationItem], form_id: &str) -> Vec<NavigationItem> {
    items
        .iter()
        .filter(|i| !(i.kind == NavKind::Form && i.target_id.as_deref() == Some(form_id)))
        .map(|i| NavigationItem {
            children: prune_navigation(&i.children, form_id),
            ..i.clone()
        })
        .collect()
}

/// Deletes a form and every reference to it (navigation, detail forms, related lists).
pub fn delete_form(design: &DesignSchema, form_id: &str) -> DesignSchema {
    let navigation = prune_navigation(&design.navigation, form_id);
    let flat = flatten_navigation(&navigation);
    let still_there = design
        .start_page
        .as_ref()
        .is_some_and(|id| flat.iter().any(|i| &i.id == id));
    let start_page = if still_there {
        design.start_page.clone()
    } else {
        flat.iter()
            .find(|i| i.kind != NavKind::Group)
            .map(|i| i.id.clone())
    };
    DesignSchema {
        forms: design
            .forms
            .iter()
            .filter(|f| f.id != form_id)
            .map(|f| {
                let mut f = f.clone();
                if f.detail_form_id.as_deref() == Some(form_id) {
                    f.detail_form_id = None;
                }
                for c in &mut f.controls {
                    if let Some(related) = &mut c.related {
                        if related.form_id.as_deref() == Some(form_id) {
                            related.form_id = None;
                        }
                    }
                }
                f
            })
            .collect(),
        navigation,
        start_page,
        ..design.clone()
    }
}

/// Container a new control should go into, given the selected control and the active tab per
/// tab group.
pub fn target_container(
    form: &Form,
    selected_id: Option<&str>,
    active_tabs: &HashMap<String, String>,
) -> Option<ControlParent> {
    let selected = form.controls.iter().find(|c| Some(c.id.as_str()) == selected_id)?;
    if selected.kind.is_container() {
        if selected.kind == ControlKind::Section {
            return Some(ControlParent {
                id: selected.id.clone(),
                tab: None,
            });
        }
        let tab = active_tabs
            .get(&selected.id)
            .cloned()
            .or_else(|| selected.tabs.first().map(|t| t.id.clone()))?;
        return Some(ControlParent {
            id: selected.id.clone(),
            tab: Some(tab),
        });
    }
    selected.parent.clone()
}

/// Applies `change` to the navigation item `id` anywhere in the tree.
pub fn map_navigation(
    items: &[NavigationItem],
    id: &str,
    change: &dyn Fn(NavigationItem) -> NavigationItem,
) -> Vec<NavigationItem> {
    items
        .iter()
        .map(|item| {
            if item.id == id {
                change(item.clone())
            } else {
                NavigationItem {
                    children: map_navigation(&item.children, id, change),
                    ..item.clone()
                }
            }
        })
        .collect()
}

/// Removes the item `id` (and its children) from the tree; returns the tree and the item.
pub fn take_navigation(
    items: &[NavigationItem],
    id: &str,
) -> (Vec<NavigationItem>, Option<NavigationItem>) {
    let mut taken = None;
    fn walk(
        list: &[NavigationItem],
        id: &str,
        taken: &mut Option<NavigationItem>,
    ) -> Vec<NavigationItem> {
        list.iter()
            .filter_map(|item| {
                if item.id == id {
                    *taken = Some(item.clone());
                    return None;
                }
                Some(NavigationItem {
                    children: walk(&item.children, id, taken),
                    ..item.clone()
                })
            })
            .collect()
    }
    let next = walk(items, id, &mut taken);
    (next, taken)
}

/// Moves an item up or down among its siblings.
pub fn shift_navigation(items: &[NavigationItem], id: &str, delta: i64) -> Vec<NavigationItem> {
    if let Some(index) = items.iter().position(|i| i.id == id) {
        let target = index as i64 + delta;
        if target < 0 || target >= items.len() as i64 {
            return items.to_vec();
        }
        let mut next = items.to_vec();
        next.swap(index, target as usize);
        return next;
    }
    items
        .iter()
        .map(|item| NavigationItem {
            children: shift_navigation(&item.children, id, delta),
            ..item.clone()
        })
        .collect()
}

/// Moves an item into a group (or to the top level when `group_id` is None).
pub fn regroup_navigation(
    items: &[NavigationItem],
    id: &str,
    group_id: Option<&str>,
) -> Vec<NavigationItem> {
    if group_id == Some(id) {
        return items.to_vec();
    }
    let (mut rest, taken) = take_navigation(items, id);
    let Some(moved) = taken else {
        return items.to_vec();
    };
    let Some(group_id) = group_id else {
        rest.push(moved);
        return rest;
    };
    if flatten_navigation(&moved.children)
        .iter()
        .any(|child| child.id == group_id)
    {
        return items.to_vec();
    }
    map_navigation(&rest, group_id, &|group| {
        let mut group = group;
        group.children.push(moved.clone());
        group
    })
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
