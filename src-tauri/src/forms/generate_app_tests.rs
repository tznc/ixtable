//! Port of tests/unit/generate-app.test.ts.

use super::add_generated_app;
use crate::data::TableSchema;
use crate::design::{
    ControlKind, DesignSchema, Form, FormSource, NavKind, NavigationItem, SourceKind,
};
use crate::forms::schema::{flatten_navigation, form_table, new_form};
use crate::forms::testkit::{col, fk, pk, table};
use std::collections::HashSet;

fn customers() -> TableSchema {
    table(
        "customers",
        vec![pk(col("id", "INTEGER"), 1), col("name", "TEXT")],
        vec![],
    )
}

fn orders() -> TableSchema {
    table(
        "orders",
        vec![
            pk(col("id", "INTEGER"), 1),
            col("customer_id", "INTEGER"),
            col("note", "TEXT"),
        ],
        vec![fk(&["customer_id"], "customers", &["id"])],
    )
}

fn empty() -> DesignSchema {
    DesignSchema {
        version: 3,
        forms: vec![],
        navigation: vec![],
        start_page: None,
    }
}

fn names(forms: &[Form]) -> Vec<&str> {
    forms.iter().map(|f| f.name.as_str()).collect()
}

fn labels(items: &[NavigationItem]) -> Vec<&str> {
    items.iter().map(|n| n.label.as_str()).collect()
}

fn generate(design: &DesignSchema, schemas: &[TableSchema]) -> DesignSchema {
    add_generated_app(design, &[], schemas, None, &[]).expect("something to add")
}

#[test]
fn adds_list_and_detail_forms_and_a_page_per_table_linking_lookups_and_related_lists() {
    let design = generate(&empty(), &[customers(), orders()]);
    assert_eq!(
        names(&design.forms),
        ["Customers list", "Customers", "Orders list", "Orders"]
    );
    assert_eq!(labels(&design.navigation), ["Customers", "Orders"]);
    assert_eq!(design.start_page.as_ref(), Some(&design.navigation[0].id));
    let customer_form = design.forms.iter().find(|f| f.name == "Customers").unwrap();
    let order_form = design.forms.iter().find(|f| f.name == "Orders").unwrap();
    let related = customer_form
        .controls
        .iter()
        .find(|c| c.kind == ControlKind::RelatedList)
        .unwrap()
        .related
        .clone()
        .unwrap();
    assert_eq!(related.table, "orders");
    assert_eq!(related.form_id.as_ref(), Some(&order_form.id));
    let rel = order_form
        .controls
        .iter()
        .find(|c| c.kind == ControlKind::Relationship)
        .unwrap()
        .relationship
        .clone()
        .unwrap();
    assert_eq!(
        (rel.table.as_str(), rel.value_column.as_str(), rel.display_column.as_str()),
        ("customers", "id", "name")
    );
}

#[test]
fn is_idempotent_running_it_again_changes_nothing() {
    let schemas = [customers(), orders()];
    let once = generate(&empty(), &schemas);
    assert!(add_generated_app(&once, &[], &schemas, None, &[]).is_none());
}

#[test]
fn adds_only_what_is_missing_keeping_hand_built_forms_and_pages() {
    let manual = new_form(
        "My customers",
        Some(FormSource {
            kind: SourceKind::Table,
            table: Some("customers".into()),
            ..Default::default()
        }),
    );
    let base = DesignSchema {
        forms: vec![manual.clone()],
        ..empty()
    };
    let design = generate(&base, &[customers(), orders()]);
    let of_customers: Vec<&Form> = design
        .forms
        .iter()
        .filter(|f| form_table(f) == Some("customers"))
        .collect();
    assert_eq!(of_customers, [&manual]);
    assert_eq!(
        design
            .forms
            .iter()
            .filter(|f| form_table(f) == Some("orders"))
            .count(),
        2
    );
    let pages = flatten_navigation(&design.navigation);
    assert_eq!(
        pages.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(),
        ["My customers", "Orders"]
    );
    assert_eq!(pages[0].target_id.as_ref(), Some(&manual.id));
    let mut later = customers();
    later.name = "regions".into();
    let next = generate(&design, &[customers(), orders(), later]);
    assert_eq!(next.forms[..design.forms.len()], design.forms[..]);
    assert_eq!(next.forms.len(), design.forms.len() + 2);
    assert_eq!(labels(&next.navigation), ["My customers", "Orders", "Regions"]);
}

#[test]
fn limits_generation_to_the_named_tables() {
    let only = vec!["orders".to_string()];
    let design = add_generated_app(&empty(), &[], &[customers(), orders()], Some(&only), &[]).unwrap();
    assert_eq!(names(&design.forms), ["Orders list", "Orders"]);
    assert_eq!(labels(&design.navigation), ["Orders"]);
}

#[test]
fn counts_a_table_navigation_item_as_a_page_for_that_table() {
    let base = DesignSchema {
        navigation: vec![NavigationItem {
            id: "t".into(),
            label: "Raw".into(),
            kind: NavKind::Table,
            target_id: Some("customers".into()),
            ..Default::default()
        }],
        ..empty()
    };
    let design = generate(&base, &[customers()]);
    assert_eq!(design.navigation.len(), 1);
    assert_eq!(design.forms.len(), 2);
}

fn embedded_nests_nothing(design: &DesignSchema) {
    let embedded: HashSet<&String> = design
        .forms
        .iter()
        .flat_map(|f| f.controls.iter().filter_map(|c| c.related.as_ref()?.form_id.as_ref()))
        .collect();
    for form in &design.forms {
        if embedded.contains(&form.id) {
            assert!(
                form.controls.iter().all(|c| c.kind != ControlKind::RelatedList),
                "{} nests a related list",
                form.name
            );
        }
    }
}

fn related_form_id(form: &Form) -> Option<String> {
    form.controls
        .iter()
        .find(|c| c.kind == ControlKind::RelatedList)?
        .related
        .as_ref()?
        .form_id
        .clone()
}

#[test]
fn embeds_a_plain_child_form_when_the_childs_own_form_has_related_lists() {
    let lines = table(
        "order_lines",
        vec![pk(col("id", "INTEGER"), 1), col("order_id", "INTEGER")],
        vec![fk(&["order_id"], "orders", &["id"])],
    );
    let schemas = [customers(), orders(), lines];
    let design = generate(&empty(), &schemas);
    embedded_nests_nothing(&design);
    let by_name = |name: &str| design.forms.iter().find(|f| f.name == name).unwrap();
    assert_eq!(
        related_form_id(by_name("Customers")),
        Some(by_name("Orders (related)").id.clone())
    );
    assert_eq!(
        related_form_id(by_name("Orders")),
        Some(by_name("Order lines").id.clone())
    );
    assert!(add_generated_app(&design, &[], &schemas, None, &[]).is_none());
}

#[test]
fn embeds_a_plain_form_for_a_self_referencing_key_instead_of_the_form_itself() {
    let employees = table(
        "employees",
        vec![
            pk(col("id", "INTEGER"), 1),
            col("name", "TEXT"),
            col("manager_id", "INTEGER"),
        ],
        vec![fk(&["manager_id"], "employees", &["id"])],
    );
    let design = generate(&empty(), &[employees]);
    embedded_nests_nothing(&design);
    let own = design.forms.iter().find(|f| f.name == "Employees").unwrap();
    let related = related_form_id(own).unwrap();
    assert_ne!(related, own.id);
    assert_eq!(
        design.forms.iter().find(|f| f.id == related).unwrap().name,
        "Employees (related)"
    );
}

#[test]
fn a_blank_form_page_yields_the_start_page_and_a_form_page_makes_its_table_navigable() {
    let mut design = empty();
    design.forms.push(new_form("Main form", None));
    let mut config = crate::archive::DocumentConfig { design, ..Default::default() };
    config.pages.clear();
    config.design.navigation = vec![NavigationItem { id: "main".into(), label: "Main".into(), kind: NavKind::Form, target_id: Some(config.design.forms[0].id.clone()), ..Default::default() }];
    config.design.start_page = Some("main".into());
    crate::pages::upgrade::wrap_definitions(&mut config);
    assert_eq!(config.design.navigation[0].kind, NavKind::Page);
    let design = add_generated_app(&config.design, &config.pages, &[customers()], None, &[]).expect("generated");
    assert_eq!(design.start_page.as_ref(), Some(&design.navigation[1].id));

    // Once the customers list is on a page that navigation opens, nothing more is added.
    config.design = design;
    crate::pages::upgrade::sync(&mut config);
    assert!(add_generated_app(&config.design, &config.pages, &[customers()], None, &[]).is_none());
}
