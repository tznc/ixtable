//! Port of the form operation tests of tests/unit/forms-schema.test.ts and the `setLayout`
//! tests of tests/unit/grid-regions.test.ts (the dashboard `withLayout` test belongs to the
//! dashboards port).

use super::{
    descendants, delete_form, duplicate_form, move_to_container, regroup_navigation,
    remove_control, remove_tab, set_layout, shift_navigation, target_container, with_column_count,
};
use crate::design::{
    ControlKind, ControlParent, DesignSchema, Form, FormSource, GridLayout, GridTrack, NamedRegion,
    NavKind, NavigationItem, SourceKind, TrackKind,
};
use crate::forms::schema::{flatten_navigation, new_control, new_form};
use std::collections::{HashMap, HashSet};

fn table_form() -> Form {
    new_form(
        "Orders",
        Some(FormSource {
            kind: SourceKind::Table,
            table: Some("orders".into()),
            ..Default::default()
        }),
    )
}

struct Built {
    form: Form,
    tabs: crate::design::Control,
    tab: String,
    inner: crate::design::Control,
    section: crate::design::Control,
}

fn build() -> Built {
    let mut form = table_form();
    let tabs = new_control(ControlKind::Tabs, &form, None);
    form.controls.push(tabs.clone());
    let tab = tabs.tabs[0].id.clone();
    let parent = ControlParent {
        id: tabs.id.clone(),
        tab: Some(tab.clone()),
    };
    let inner = new_control(ControlKind::Text, &form, Some(&parent));
    form.controls.push(inner.clone());
    let section = new_control(ControlKind::Section, &form, None);
    form.controls.push(section.clone());
    Built {
        form,
        tabs,
        tab,
        inner,
        section,
    }
}

fn ids(form: &Form) -> Vec<String> {
    form.controls.iter().map(|c| c.id.clone()).collect()
}

#[test]
fn removes_containers_with_their_children_and_tabs_with_their_controls() {
    let b = build();
    assert_eq!(
        descendants(&b.form, &b.tabs.id, None),
        HashSet::from([b.tabs.id.clone(), b.inner.id.clone()])
    );
    assert!(!ids(&remove_control(&b.form, &b.tabs.id)).contains(&b.inner.id));
    let without_tab = remove_tab(&b.form, &b.tabs.id, &b.tab);
    assert!(without_tab.controls.iter().all(|c| c.id != b.inner.id));
    let tabs = without_tab
        .controls
        .iter()
        .find(|c| c.id == b.tabs.id)
        .unwrap();
    assert_eq!(tabs.tabs.len(), 1);
}

fn control<'a>(form: &'a Form, id: &str) -> &'a crate::design::Control {
    form.controls.iter().find(|c| c.id == id).unwrap()
}

#[test]
fn clamps_placements_when_a_grid_loses_columns() {
    let b = build();
    let narrowed = set_layout(&b.form, None, with_column_count(&b.form.layout, 4), &[]);
    assert_eq!(narrowed.layout.columns.len(), 4);
    let tabs = control(&narrowed, &b.tabs.id);
    assert_eq!((tabs.placement.column, tabs.placement.column_span), (1, 4));
    assert_eq!(control(&narrowed, &b.inner.id).placement.column_span, 6);
    let container = set_layout(
        &b.form,
        Some(&b.tabs.id),
        with_column_count(&b.form.layout, 2),
        &[],
    );
    assert_eq!(control(&container, &b.inner.id).placement.column_span, 2);
}

#[test]
fn changes_the_column_count_without_rewriting_authored_tracks() {
    let track = |kind, value, min, max| GridTrack {
        kind,
        value,
        min,
        max,
    };
    let layout = GridLayout {
        columns: vec![
            track(TrackKind::Fixed, Some(200.0), None, None),
            track(TrackKind::Content, None, Some(80.0), Some(300.0)),
            track(TrackKind::Fr, Some(2.0), None, None),
        ],
        ..GridLayout::default()
    };
    let wider = with_column_count(&layout, 5);
    assert_eq!(wider.columns[..3], layout.columns[..]);
    assert_eq!(wider.columns[3..], [GridTrack::fr(1.0), GridTrack::fr(1.0)]);
    assert_eq!(with_column_count(&layout, 2).columns, layout.columns[..2]);
    assert_eq!(with_column_count(&layout, 0).columns, layout.columns[..1]);
    assert_eq!(with_column_count(&layout, 99).columns.len(), 24);
}

#[test]
fn duplicates_a_form_with_fresh_consistently_remapped_ids() {
    let b = build();
    let copy = duplicate_form(&b.form, None);
    assert_ne!(copy.id, b.form.id);
    assert_eq!(copy.name, "Orders copy");
    let copied_tabs = copy
        .controls
        .iter()
        .find(|c| c.kind == ControlKind::Tabs)
        .unwrap();
    let copied_inner = copy
        .controls
        .iter()
        .find(|c| c.kind == ControlKind::Text)
        .unwrap();
    assert_ne!(copied_tabs.id, b.tabs.id);
    let parent = copied_inner.parent.as_ref().unwrap();
    assert_eq!(parent.id, copied_tabs.id);
    assert_eq!(parent.tab.as_deref(), Some(copied_tabs.tabs[0].id.as_str()));
    assert_ne!(copied_inner.id, b.inner.id);
}

#[test]
fn moves_controls_between_containers_but_never_into_themselves() {
    let b = build();
    let target = ControlParent {
        id: b.section.id.clone(),
        tab: None,
    };
    let moved = move_to_container(&b.form, &b.inner.id, Some(target.clone()));
    assert_eq!(control(&moved, &b.inner.id).parent, Some(target));
    let same = move_to_container(
        &b.form,
        &b.tabs.id,
        Some(ControlParent {
            id: b.tabs.id.clone(),
            tab: None,
        }),
    );
    assert_eq!(same, b.form);
}

#[test]
fn targets_the_selected_container_or_the_selected_controls_container() {
    let b = build();
    let none = HashMap::new();
    assert_eq!(target_container(&b.form, None, &none), None);
    assert_eq!(
        target_container(&b.form, Some(&b.section.id), &none),
        Some(ControlParent {
            id: b.section.id.clone(),
            tab: None
        })
    );
    let tab_parent = Some(ControlParent {
        id: b.tabs.id.clone(),
        tab: Some(b.tab.clone()),
    });
    assert_eq!(target_container(&b.form, Some(&b.tabs.id), &none), tab_parent);
    assert_eq!(target_container(&b.form, Some(&b.inner.id), &none), tab_parent);
}

#[test]
fn deletes_a_form_and_its_references() {
    let mut list = new_form("List", None);
    let detail = new_form("Detail", None);
    list.detail_form_id = Some(detail.id.clone());
    let nav = |id: &str, kind, target: Option<&str>, children| NavigationItem {
        id: id.into(),
        label: id.to_uppercase(),
        kind,
        target_id: target.map(Into::into),
        mode: None,
        params: Default::default(),
        children,
    };
    let design = DesignSchema {
        version: 3,
        forms: vec![list.clone(), detail.clone()],
        navigation: vec![
            nav(
                "g",
                NavKind::Group,
                None,
                vec![nav("d", NavKind::Form, Some(&detail.id), vec![])],
            ),
            nav("l", NavKind::Form, Some(&list.id), vec![]),
        ],
        start_page: Some("d".into()),
    };
    let next = delete_form(&design, &detail.id);
    assert_eq!(
        next.forms.iter().map(|f| f.id.clone()).collect::<Vec<_>>(),
        [list.id.clone()]
    );
    assert_eq!(next.forms[0].detail_form_id, None);
    let flat: Vec<&str> = flatten_navigation(&next.navigation)
        .iter()
        .map(|i| i.id.as_str())
        .collect();
    assert_eq!(flat, ["g", "l"]);
    assert_eq!(next.start_page.as_deref(), Some("l"));
}

#[test]
fn reorders_and_regroups_navigation_items() {
    let item = |id: &str, kind, target: Option<&str>| NavigationItem {
        id: id.into(),
        label: id.to_uppercase(),
        kind,
        target_id: target.map(Into::into),
        ..Default::default()
    };
    let items = vec![
        item("a", NavKind::Form, Some("f")),
        item("g", NavKind::Group, None),
    ];
    let order = |v: &[NavigationItem]| v.iter().map(|i| i.id.clone()).collect::<Vec<_>>();
    assert_eq!(order(&shift_navigation(&items, "g", -1)), ["g", "a"]);
    let grouped = regroup_navigation(&items, "a", Some("g"));
    assert_eq!(grouped.len(), 1);
    assert_eq!(order(&grouped[0].children), ["a"]);
    assert_eq!(order(&regroup_navigation(&grouped, "a", None)), ["g", "a"]);
    assert_eq!(regroup_navigation(&grouped, "g", Some("g")), grouped);
}

// ---- setLayout region handling (tests/unit/grid-regions.test.ts) ----------------------

fn region(name: &str, column: u16, column_span: u16) -> NamedRegion {
    NamedRegion {
        name: name.into(),
        column,
        row: 1,
        column_span,
        row_span: 1,
    }
}

fn columns(count: usize) -> GridLayout {
    GridLayout {
        columns: vec![GridTrack::fr(1.0); count],
        ..GridLayout::default()
    }
}

#[test]
fn set_layout_renames_clears_and_clamps_region_use_for_controls_on_the_grid() {
    let mut form = table_form();
    form.layout = GridLayout {
        named_regions: vec![region("a", 3, 2), region("b", 1, 1)],
        ..columns(4)
    };
    let mut in_a = new_control(ControlKind::Text, &form, None);
    in_a.placement.region = Some("a".into());
    let mut in_b = new_control(ControlKind::Text, &form, None);
    in_b.placement.region = Some("b".into());
    form.controls.push(in_a.clone());
    form.controls.push(in_b.clone());
    let next = GridLayout {
        named_regions: vec![region("main", 3, 2)],
        ..columns(2)
    };
    let out = set_layout(&form, None, next, &[("a".to_string(), "main".to_string())]);
    assert_eq!(out.layout.named_regions, [region("main", 2, 1)]);
    assert_eq!(control(&out, &in_a.id).placement.region.as_deref(), Some("main"));
    assert_eq!(control(&out, &in_b.id).placement.region, None);
}
