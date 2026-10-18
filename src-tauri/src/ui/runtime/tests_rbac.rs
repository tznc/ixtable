//! Role preview and enforcement (port of tests/integration/runtime-rbac.test.tsx). The React
//! test built its forms in the Design studio; here the app comes from "Generate app" and
//! the visible-when rule from the config. Imports are explicit.

use super::tests::{nav, renderer, run_as, seed, wait};
use super::window::OWNER;
use crate::data::{DataValue, NamedValue};
use crate::manager::AppError;
use crate::recordstore::WriteOp;
use crate::ui::shell::ModeId;
use crate::ui::testing::Harness;
use gpui_kit::TestAppContext;
use serde_json::json;

fn code<T>(result: Result<T, AppError>) -> String {
    match result {
        Ok(_) => "OK".into(),
        Err(error) => error.code,
    }
}

fn text(column: &str, value: &str) -> NamedValue {
    NamedValue { column: column.into(), value: DataValue::Text(value.into()) }
}

fn form_id(h: &Harness, name: &str) -> String {
    h.config().design.forms.iter().find(|f| f.name == name).expect("form").id.clone()
}

fn table_page(h: &Harness, table: &str) -> String {
    code(crate::read_table_page(h.label.clone(), table.into(), 0, 10, vec![], vec![], None))
}

#[gpui_kit::test]
fn previews_roles_hides_navigation_blocks_deletes_and_shows_fields_conditionally(
    cx: &mut TestAppContext,
) {
    let mut h = Harness::new_document(cx);
    h.create_table(
        "notes",
        &[("id", "INTEGER", 1), ("body", "TEXT", 0)],
    );
    h.create_table(
        "accounts",
        &[("id", "INTEGER", 1), ("name", "TEXT", 0), ("kind", "TEXT", 0), ("vat", "TEXT", 0)],
    );
    h.insert_row(
        "accounts",
        &[
            ("id", DataValue::Integer(1)),
            ("name", DataValue::Text("Acme".into())),
            ("kind", DataValue::Text("company".into())),
            ("vat", DataValue::Text("GB123".into())),
        ],
    );
    h.switch_mode(ModeId::Run);
    h.click("generate-app");
    wait(&mut h);
    seed(&mut h, |config| {
        let form = config.design.forms.iter_mut().find(|f| f.name == "Accounts").expect("Accounts");
        let vat = form.controls.iter_mut().find(|c| c.label == "Vat").expect("Vat control");
        vat.visible_when = Some("record.kind = 'company'".into());
    });

    // Define a Clerk role in the Roles tab.
    h.switch_mode(ModeId::App);
    h.click("tab-roles");
    assert!(h.label("rbac-note").contains("not a database security boundary"));
    h.click("add-role");
    h.fill("role-name", "Clerk");
    let (list, detail, accounts_nav) =
        (form_id(&h, "Accounts list"), form_id(&h, "Accounts"), nav(&mut h, "Accounts"));
    h.click(format!("perm:form:{list}:read"));
    h.click(format!("perm:form:{detail}:update"));
    let np = format!("nav-perm:{}", accounts_nav.trim_start_matches("nav:"));
    h.click(np);
    let names: Vec<String> = h.config().roles.iter().map(|r| r.name.clone()).collect();
    assert_eq!(names, ["Clerk"]);

    // Developer view: every page, delete available, the vat field follows the kind.
    h.switch_mode(ModeId::Run);
    wait(&mut h);
    let notes_nav = nav(&mut h, "Notes");
    assert!(h.exists(notes_nav.clone()));
    h.click(accounts_nav.clone());
    wait(&mut h);
    h.click("row:0");
    wait(&mut h);
    assert!(h.exists("field:vat"));
    {
        let page = renderer(&mut h);
        let record = h.cx.read(|cx| page.read(cx).record()).expect("a record view");
        assert!(h.cx.read(|cx| record.read(cx).can_delete(cx)));
    }
    h.click("edit");
    wait(&mut h);
    h.fill("field:kind", "person");
    assert!(!h.exists("field:vat"));
    h.click("cancel");
    wait(&mut h);

    // The Clerk: no Notes page, no New button, no Delete, no direct access to notes.
    run_as(&mut h, "Clerk");
    wait(&mut h);
    assert!(!h.exists(notes_nav));
    assert!(!h.exists("new-record"));
    h.click("row:0");
    wait(&mut h);
    {
        // Entities held here would keep the Runtime (and its role preview) alive.
        let page = renderer(&mut h);
        let record = h.cx.read(|cx| page.read(cx).record()).expect("a record view");
        assert!(h.cx.read(|cx| record.read(cx).can_edit(cx)));
        assert!(h.cx.read(|cx| !record.read(cx).can_delete(cx)));
    }
    assert!(!h.exists("delete"));

    let label = h.label.clone();
    let note = vec![text("body", "secret")];
    assert_eq!(table_page(&h, "notes"), "FORBIDDEN");
    assert_eq!(
        code(crate::recordstore::commands::insert_row(label.clone(), "notes".into(), note.clone(), None)),
        "FORBIDDEN"
    );
    let batch = vec![WriteOp::Insert { table: "notes".into(), values: note.clone() }];
    assert_eq!(code(crate::recordstore::commands::execute_write_batch(label.clone(), batch, None)), "FORBIDDEN");
    assert_eq!(code(crate::execute_read_query(label.clone(), "SELECT * FROM notes".into())), "FORBIDDEN");
    assert_eq!(table_page(&h, "accounts"), "OK");
    let identity = vec![DataValue::Integer(1)];
    let rename = vec![text("name", "Acme Ltd")];
    let expected = vec![text("name", "Acme")];
    let update = |expected: Option<Vec<NamedValue>>| {
        crate::recordstore::commands::update_row(
            label.clone(),
            "accounts".into(),
            rename.clone(),
            identity.clone(),
            expected,
            None,
        )
    };
    assert_eq!(code(update(None)), "EXPECTED_REQUIRED");
    assert_eq!(code(update(Some(expected.clone()))), "OK");
    let delete = |expected: Option<Vec<NamedValue>>| {
        crate::recordstore::commands::delete_row(label.clone(), "accounts".into(), identity.clone(), expected, None)
    };
    assert_eq!(code(delete(Some(expected))), "FORBIDDEN");
    assert_eq!(code(delete(None)), "FORBIDDEN");
    assert_eq!(
        code(crate::recordstore::commands::insert_row(label.clone(), "accounts".into(), rename, None)),
        "FORBIDDEN"
    );

    // Back to the owner, then the Clerk again, then back to Studio (enforcement ends).
    run_as(&mut h, OWNER);
    wait(&mut h);
    assert_eq!(table_page(&h, "notes"), "OK");
    run_as(&mut h, "Clerk");
    wait(&mut h);
    assert_eq!(table_page(&h, "notes"), "FORBIDDEN");
    h.switch_mode(ModeId::App);
    wait(&mut h);
    assert_eq!(table_page(&h, "notes"), "OK");
    assert_eq!(
        code(crate::recordstore::commands::insert_row(label, "notes".into(), note, None)),
        "OK"
    );
}

#[gpui_kit::test]
fn checks_query_read_access_on_paged_saved_queries_like_full_runs(cx: &mut TestAppContext) {
    let mut h = Harness::new_document(cx);
    h.create_table("notes", &[("id", "INTEGER", 1), ("body", "TEXT", 0)]);
    h.insert_row(
        "notes",
        &[("id", DataValue::Integer(1)), ("body", DataValue::Text("hello".into()))],
    );
    seed(&mut h, |config| {
        let sql = "SELECT id, body FROM notes";
        config.saved_queries = serde_json::from_value(json!([
            {"id": "q-open", "name": "Open notes", "sql": sql},
            {"id": "q-secret", "name": "Secret notes", "sql": sql},
        ]))
        .expect("saved queries");
        config.roles = serde_json::from_value(json!([{
            "id": "clerk", "name": "Clerk",
            "permissions": {"objects": [{"kind": "query", "id": "q-open", "read": true}]},
        }]))
        .expect("roles");
    });
    let label = h.label.clone();
    let page = |id: &str| {
        code(tauri::async_runtime::block_on(crate::queries::run_saved_query_page(
            label.clone(),
            id.into(),
            vec![],
            0,
            10,
            vec![],
            vec![],
            None,
        )))
    };
    let run = |id: &str| {
        code(tauri::async_runtime::block_on(crate::queries::run_saved_query(
            label.clone(),
            id.into(),
            vec![],
            None,
            None,
        )))
    };
    assert_eq!(page("q-secret"), "OK");
    crate::authz::set_runtime_role_preview(label.clone(), Some("clerk".into())).expect("preview");
    assert_eq!(page("q-open"), "OK");
    assert_eq!(run("q-open"), "OK");
    assert_eq!(page("q-secret"), "FORBIDDEN");
    assert_eq!(run("q-secret"), "FORBIDDEN");
    crate::authz::set_runtime_role_preview(label.clone(), None).expect("end preview");
    assert_eq!(page("q-secret"), "OK");
}
