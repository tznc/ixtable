//! "Generate app from tables" (port of `src/design/generateApp.ts`).

use super::generate::{GenerateOptions, GeneratedCrud, generate_crud_forms, humanize};
use super::schema::{flatten_navigation, form_table, new_id};
use crate::data::TableSchema;
use crate::design::{DesignSchema, Form, FormMode, NavKind, NavigationItem};
use crate::recordstore::EntitySettings;
use std::collections::{HashMap, HashSet};

/// A form that edits records of a table: the detail form a generated app links to.
fn is_record_form(form: &Form) -> bool {
    form.modes.contains(&FormMode::Create)
}

/// Adds generated list and detail forms and a navigation item for each table in `only`
/// (every table when omitted) that is missing them. A table counts as covered when any form
/// has it as its source, generated or built by hand; it counts as navigable when a
/// navigation item opens one of its forms or the table itself. Running it again on its own
/// result changes nothing, so it never duplicates forms or navigation. Returns None when
/// nothing was missing.
pub fn add_generated_app<'a>(
    design: &DesignSchema,
    schemas: &'a [TableSchema],
    only: Option<&[String]>,
    entities: &'a [EntitySettings],
) -> Option<DesignSchema> {
    let wanted: Vec<&'a TableSchema> = schemas
        .iter()
        .filter(|s| only.is_none_or(|o| o.contains(&s.name)))
        .collect();
    let covered: HashSet<&str> = design.forms.iter().filter_map(form_table).collect();
    let targets: HashMap<String, &'a TableSchema> =
        schemas.iter().map(|s| (s.name.clone(), s)).collect();
    let options = |children: Vec<&'a TableSchema>| GenerateOptions {
        targets: targets.clone(),
        children,
        child_forms: HashMap::new(),
        entities,
    };
    let generated: Vec<(&TableSchema, GeneratedCrud)> = wanted
        .iter()
        .filter(|s| !covered.contains(s.name.as_str()))
        .map(|schema| {
            let children = schemas
                .iter()
                .filter(|other| other.foreign_keys.iter().any(|fk| fk.target_table == schema.name))
                .collect();
            (*schema, generate_crud_forms(schema, &options(children)))
        })
        .collect();
    let mut forms: Vec<Form> = design.forms.clone();
    for (_, crud) in &generated {
        forms.push(crud.list.clone());
        forms.push(crud.detail.clone());
    }
    // Embedded forms cannot nest related lists, so a nesting (or self) form gets a plain copy.
    let mut plain: Vec<(String, Form)> = vec![];
    let mut child_form = |table: &str, parent: &Form, forms: &[Form]| -> Option<String> {
        let own = forms
            .iter()
            .find(|f| form_table(f) == Some(table) && is_record_form(f));
        let nests = own.is_some_and(|f| {
            f.controls
                .iter()
                .any(|c| c.kind == crate::design::ControlKind::RelatedList)
        });
        if let Some(own) = own {
            if own.id != parent.id && !nests {
                return Some(own.id.clone());
            }
        }
        let schema = targets.get(table)?;
        if !plain.iter().any(|(t, _)| t == table) {
            let mut detail = generate_crud_forms(schema, &options(vec![])).detail;
            detail.name = format!("{} (related)", humanize(table));
            plain.push((table.to_string(), detail));
        }
        plain.iter().find(|(t, _)| t == table).map(|(_, f)| f.id.clone())
    };
    let before = forms.clone();
    forms = before
        .iter()
        .map(|form| {
            if !generated.iter().any(|(_, crud)| crud.detail.id == form.id) {
                return form.clone();
            }
            let mut next = form.clone();
            for control in &mut next.controls {
                if let Some(related) = &mut control.related {
                    if related.form_id.is_none() {
                        related.form_id = child_form(&related.table, form, &before);
                    }
                }
            }
            next
        })
        .collect();
    forms.extend(plain.into_iter().map(|(_, f)| f));
    let mut navigable: HashSet<String> = HashSet::new();
    for item in flatten_navigation(&design.navigation) {
        if item.kind == NavKind::Table {
            if let Some(target) = &item.target_id {
                navigable.insert(target.clone());
            }
        }
        if item.kind == NavKind::Form {
            let table = forms
                .iter()
                .find(|f| Some(&f.id) == item.target_id.as_ref())
                .and_then(form_table);
            if let Some(table) = table {
                navigable.insert(table.to_string());
            }
        }
    }
    let mut navigation = design.navigation.clone();
    for schema in &wanted {
        if navigable.contains(&schema.name) {
            continue;
        }
        let ours = generated
            .iter()
            .find(|(s, _)| std::ptr::eq(*s, *schema))
            .map(|(_, crud)| crud.navigation.clone());
        let list = forms
            .iter()
            .find(|f| form_table(f) == Some(schema.name.as_str()) && f.modes.contains(&FormMode::List))
            .or_else(|| forms.iter().find(|f| form_table(f) == Some(schema.name.as_str())));
        if let Some(ours) = ours {
            navigation.push(ours);
        } else if let Some(list) = list {
            navigation.push(NavigationItem {
                id: new_id(),
                label: list.name.clone(),
                kind: NavKind::Form,
                target_id: Some(list.id.clone()),
                ..Default::default()
            });
        }
    }
    if forms.len() == design.forms.len() && navigation.len() == design.navigation.len() {
        return None;
    }
    // A blank start page (none, or a form with no source and no controls) yields to the first new page.
    let start = flatten_navigation(&navigation)
        .into_iter()
        .find(|item| Some(&item.id) == design.start_page.as_ref());
    let start_form = start
        .filter(|s| s.kind == NavKind::Form)
        .and_then(|s| forms.iter().find(|f| Some(&f.id) == s.target_id.as_ref()));
    let blank = start.is_none()
        || start_form.is_some_and(|f| f.source.is_none() && f.controls.is_empty());
    let first_new = navigation.get(design.navigation.len()).map(|n| n.id.clone());
    let start_page = match (blank, first_new) {
        (true, Some(id)) => Some(id),
        _ => design.start_page.clone(),
    };
    Some(DesignSchema {
        forms,
        navigation,
        start_page,
        ..design.clone()
    })
}

#[cfg(test)]
#[path = "generate_app_tests.rs"]
mod tests;
