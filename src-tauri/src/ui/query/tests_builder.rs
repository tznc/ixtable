//! Query editor behaviors beyond the two ported integration files: the shell selection,
//! discard confirmations, name checks, export, undo, and the reusable `QueryPicker`.

use super::{
    QueryPicker,
    tests_support::{
        click, exists, expect_grid, fill, open_query_mode, pick, type_sql, wait_until, with_mode,
    },
};
use crate::{
    archive::{ActionKind, ActionSpec, SavedQuery},
    ui::{
        shell::{ModeId, SelectionKind},
        testing::Harness,
    },
};
use gpui_kit::{AppContext as _, TestAppContext};

fn add_queries(h: &mut Harness, queries: Vec<SavedQuery>) {
    let shell = h.shell();
    h.cx.update(|cx| {
        shell.update(cx, |s, cx| {
            s.store.update(cx, |store, cx| {
                store
                    .update("Add queries", |c| c.saved_queries.extend(queries), cx)
                    .unwrap();
            })
        })
    });
    h.settle();
}

fn query(id: &str, name: &str, sql: &str) -> SavedQuery {
    SavedQuery {
        id: id.into(),
        name: name.into(),
        sql: sql.into(),
        ..Default::default()
    }
}

fn action_query(id: &str, name: &str) -> SavedQuery {
    SavedQuery {
        action: Some(ActionSpec {
            kind: ActionKind::Delete,
            table: "orders".into(),
        }),
        ..query(id, name, "DELETE FROM orders WHERE id = 1")
    }
}

fn draft_name(h: &mut Harness) -> String {
    with_mode(h, |q, _| q.draft().map(|d| d.query.name.clone()).unwrap_or_default())
}

fn draft_id(h: &mut Harness) -> Option<String> {
    with_mode(h, |q, _| q.draft().map(|d| d.query.id.clone()))
}

#[gpui_kit::test]
fn the_object_browser_opens_new_and_saved_queries_on_the_data_page(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    add_queries(&mut h, vec![query("q1", "Everyone", "SELECT 1 AS one")]);
    assert_eq!(ModeId::from_key("query"), ModeId::Data, "documents saved in Query mode open on Data");
    assert!(!h.exists("mode-query"), "queries have no mode of their own");
    click(&mut h, "new-query");
    assert_eq!(h.mode(), ModeId::Data);
    assert!(with_mode(&mut h, |q, _| q.draft().is_some_and(|d| d.is_new)));
    let id = draft_id(&mut h).unwrap();
    let selection = h.read_shell(|s, _| s.selection.clone()).unwrap();
    assert_eq!((selection.kind, selection.id), (SelectionKind::NewQuery, id));

    click(&mut h, "query:q1");
    assert_eq!(h.mode(), ModeId::Data);
    assert_eq!(draft_id(&mut h).as_deref(), Some("q1"));
    click(&mut h, "query-run");
    expect_grid(&mut h, &[&["1"]]);
    assert_eq!(h.label("run-status").split(" · ").next(), Some("1 row"));
}

#[gpui_kit::test]
fn a_query_draft_survives_a_visit_to_a_table(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    add_queries(&mut h, vec![query("q1", "First", "SELECT 1")]);
    click(&mut h, "query:q1");
    fill(&mut h, "query-name", "First, edited");
    click(&mut h, "table:customers");
    assert!(h.exists("table-name"), "the datasheet replaces the query editor");
    assert!(!h.exists("query-name"));

    click(&mut h, "query:q1");
    assert!(h.dialogs.calls.lock().unwrap().is_empty(), "nothing to discard");
    assert_eq!(draft_name(&mut h), "First, edited");
    assert!(with_mode(&mut h, |q, _| q.is_dirty()));
}

#[gpui_kit::test]
fn query_this_table_starts_a_builder_query_on_the_table(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    click(&mut h, "table:orders");
    click(&mut h, "query-table");
    let model = with_mode(&mut h, |q, _| q.model()).expect("a builder query");
    assert_eq!(model.sources[0].table, "orders");
    assert_eq!(draft_name(&mut h), "orders query");
    assert_eq!(h.label("query-draft"), "orders query (unsaved)");
    assert!(h.read_shell(|s, _| s.selection.as_ref().is_some_and(|s| s.kind == SelectionKind::NewQuery)));
}

#[gpui_kit::test]
fn asks_before_discarding_unsaved_changes(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    add_queries(
        &mut h,
        vec![query("q1", "First", "SELECT 1"), query("q2", "Second", "SELECT 2")],
    );
    click(&mut h, "query:q1");
    fill(&mut h, "query-name", "First, edited");
    assert!(with_mode(&mut h, |q, _| q.is_dirty()));
    assert!(exists(&mut h, "query-unsaved"));

    h.dialogs.queue_confirm(false);
    click(&mut h, "query:q2");
    assert_eq!(draft_id(&mut h).as_deref(), Some("q1"));
    assert!(h
        .dialogs
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|c| c.contains("Discard unsaved changes to First, edited?")));

    h.dialogs.queue_confirm(true);
    click(&mut h, "query:q2");
    assert_eq!(draft_id(&mut h).as_deref(), Some("q2"));
    assert!(!with_mode(&mut h, |q, _| q.is_dirty()));
}

#[gpui_kit::test]
fn checks_names_when_saving_and_renaming(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    add_queries(&mut h, vec![query("q1", "Taken", "SELECT 1")]);
    click(&mut h, "new-query");
    click(&mut h, "tab-sql");
    type_sql(&mut h, "SELECT 2");
    fill(&mut h, "query-name", "");
    click(&mut h, "query-save");
    assert_eq!(h.label("query-error"), "Give the query a name");
    fill(&mut h, "query-name", "taken");
    click(&mut h, "query-save");
    assert_eq!(h.label("query-error"), "Another query is already named taken");

    fill(&mut h, "query-name", "Fresh");
    click(&mut h, "query-save");
    assert!(!exists(&mut h, "query-error"));
    fill(&mut h, "query-name", "TAKEN");
    click(&mut h, "query-save");
    assert_eq!(h.label("query-error"), "Another query is already named TAKEN");
    assert_eq!(h.config().saved_queries[0].name, "Taken");
}

#[gpui_kit::test]
fn a_clean_draft_follows_undo_and_a_new_draft_is_listed_as_unsaved(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    click(&mut h, "new-query");
    assert_eq!(h.label("query-draft"), "Untitled query (unsaved)");
    click(&mut h, "tab-sql");
    type_sql(&mut h, "SELECT 1");
    click(&mut h, "query-save");
    let id = draft_id(&mut h).unwrap();
    assert!(!exists(&mut h, "query-draft"));
    assert!(exists(&mut h, &format!("query:{id}")));
    assert_eq!(h.read_shell(|s, _| s.selection.clone()).map(|s| s.id), Some(id.clone()));

    click(&mut h, "undo");
    assert!(h.config().saved_queries.is_empty());
    assert_eq!(draft_id(&mut h), None, "the draft of a query that no longer exists closes");
    assert!(exists(&mut h, "create-query"));
}

#[gpui_kit::test]
fn switching_a_builder_query_to_sql_confirms_and_keeps_the_generated_sql(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    click(&mut h, "new-query");
    pick(&mut h, "source-table", "customers");
    click(&mut h, "col:c.name");
    click(&mut h, "tab-sql");
    h.dialogs.queue_confirm(false);
    click(&mut h, "switch-to-sql");
    assert!(with_mode(&mut h, |q, _| q.model().is_some()), "declined: still a builder query");
    click(&mut h, "switch-to-sql");
    assert!(with_mode(&mut h, |q, _| q.model().is_none()));
    assert_eq!(h.label("query-mode-badge"), "SQL");
    let sql = with_mode(&mut h, |q, _| q.draft().unwrap().query.sql.clone());
    assert_eq!(sql, "SELECT \"c\".\"name\" AS \"name\"\nFROM \"customers\" AS \"c\"");
}

#[gpui_kit::test]
fn exports_the_full_result_of_the_query_shown(cx: &mut TestAppContext) {
    let mut h = open_query_mode(cx);
    click(&mut h, "new-query");
    click(&mut h, "tab-sql");
    type_sql(&mut h, "SELECT name FROM customers ORDER BY id");
    assert!(!exists(&mut h, "export"), "no export before a run");
    click(&mut h, "query-run");
    expect_grid(&mut h, &[&["ACME"], &["Bolt"], &["Core"]]);

    let destination = h.temp_path("customers.csv");
    h.dialogs.queue_save(Some(destination.clone()));
    pick(&mut h, "export", "csv");
    wait_until(&mut h, "the export", |h| h.notice().is_some());
    assert_eq!(h.notice().as_deref(), Some("Exported 3 rows"));
    let written = std::fs::read_to_string(&destination).unwrap();
    assert!(written.contains("Bolt"), "{written}");

    h.dialogs.queue_save(None);
    pick(&mut h, "export", "json");
    assert!(!exists(&mut h, "export-error"), "canceling the save dialog exports nothing");
}

#[gpui_kit::test]
fn the_query_picker_lists_read_queries_and_marks_a_missing_one(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    add_queries(
        &mut h,
        vec![query("q1", "Everyone", "SELECT 1"), action_query("a1", "Purge")],
    );
    let store = h.read_shell(|s, _| s.store.clone());
    let picker = h.cx.update(|cx| cx.new(|cx| QueryPicker::new("picker", store.clone(), cx).value("gone")));
    let labels = |h: &mut Harness, picker: &gpui_kit::Entity<QueryPicker>| {
        h.cx.read(|cx| {
            picker
                .read(cx)
                .options(cx)
                .into_iter()
                .map(|o| (o.value, o.label))
                .collect::<Vec<_>>()
        })
    };
    assert_eq!(
        labels(&mut h, &picker),
        [
            ("".to_string(), "No query".to_string()),
            ("gone".to_string(), "Missing query".to_string()),
            ("q1".to_string(), "Everyone".to_string()),
        ]
    );
    h.cx.update(|cx| {
        picker.update(cx, |p, cx| {
            *p = QueryPicker::new("picker", store, cx).actions(true).allow_none(false);
        })
    });
    let all = labels(&mut h, &picker);
    assert_eq!(
        all.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>(),
        ["Everyone", "Purge"]
    );
}
