use crate::access::convert::{self, report::Status, Options};
use crate::archive::{validate_config, Severity};
use crate::manager::DocumentManager;
use std::path::PathBuf;
use std::sync::OnceLock;

pub fn manager() -> &'static DocumentManager {
    static M: OnceLock<(PathBuf, DocumentManager)> = OnceLock::new();
    &M.get_or_init(|| {
        let base = std::env::temp_dir().join(format!("ixtable-access-{}", uuid::Uuid::new_v4()));
        let m = DocumentManager::new(base.join("data"), base.join("cache")).unwrap();
        (base, m)
    })
    .1
}

pub fn errors(issues: &[crate::archive::Issue]) -> Vec<String> {
    issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| format!("{} {}: {}", i.object_kind, i.object_id, i.message))
        .collect()
}

/// Imports a file into a new document and returns the report and config errors.
pub fn import(
    path: &std::path::Path,
    window: &str,
) -> (crate::access::convert::report::ImportReport, Vec<String>) {
    let mut file = crate::access::open(path).unwrap();
    let conversion = convert::convert(
        file.as_mut(),
        &path.file_name().unwrap().to_string_lossy(),
        &Options::default(),
        &mut |_, _| {},
    )
    .unwrap();
    let m = manager();
    let (_, report) = convert::create(m, window, file.as_ref(), conversion)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let config = m.config(window).unwrap();
    let errs = errors(&validate_config(&config));
    (report, errs)
}

#[test]
fn every_downloaded_template_imports() {
    let Some(dir) = super::accdt::templates_dir() else {
        return;
    };
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    let only = std::env::var("IXTABLE_ACCESS_ONLY").ok();
    for (i, path) in files
        .iter()
        .filter(|p| {
            p.extension()
                .is_some_and(|e| e == "accdt" || e == "accdb" || e == "mdb")
        })
        .enumerate()
    {
        if only
            .as_deref()
            .is_some_and(|o| !path.to_string_lossy().contains(o))
        {
            continue;
        }
        let window = format!("w{i}");
        let (report, errs) = import(path, &window);
        let count = |s: Status| report.items.iter().filter(|x| x.status == s).count();
        eprintln!(
            "{}: rows {} converted {} partial {} skipped {}",
            path.file_name().unwrap().to_string_lossy(),
            report.rows,
            count(Status::Converted),
            count(Status::Partial),
            count(Status::Skipped)
        );
        if std::env::var("IXTABLE_ACCESS_VERBOSE").is_ok() {
            for item in report
                .items
                .iter()
                .filter(|x| x.status != Status::Converted)
            {
                eprintln!(
                    "  {} {} {:?}: {}",
                    item.kind,
                    item.name,
                    item.status,
                    item.notes.join(" | ")
                );
            }
        }
        assert_eq!(errs, Vec::<String>::new(), "{}", path.display());
        manager().close(&window, true).unwrap();
    }
}

fn rows(m: &DocumentManager, window: &str, sql: &str) -> Vec<Vec<crate::data::DataValue>> {
    m.read_query(window, sql)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .rows
}

fn form<'a>(config: &'a crate::archive::DocumentConfig, name: &str) -> &'a crate::design::Form {
    config
        .design
        .forms
        .iter()
        .find(|f| f.name == name)
        .unwrap_or_else(|| panic!("no form {name}"))
}

#[test]
fn template_package_becomes_a_working_document() {
    use crate::data::DataValue::*;
    let path = super::fixtures::template();
    let (report, errs) = import(&path, "tpl");
    assert_eq!(errs, Vec::<String>::new());
    let m = manager();
    let config = m.config("tpl").unwrap();
    assert_eq!(config.name, "Order Desk");
    assert_eq!(config.active_mode, "run");
    // Tables, child tables for the attachment and multi-value fields, the
    // make-table query's target, and data.
    let mut tables: Vec<String> = m
        .database_objects("tpl")
        .unwrap()
        .into_iter()
        .map(|o| o.name)
        .collect();
    tables.sort();
    assert_eq!(
        tables,
        [
            "Customer Totals",
            "Customers",
            "Customers Logo",
            "Customers Tags",
            "Orders"
        ]
    );
    assert_eq!(config.entities.len(), 5);
    assert_eq!(
        rows(m, "tpl", "SELECT count(*) FROM Orders"),
        [[Integer(3)]]
    );
    assert_eq!(
        rows(
            m,
            "tpl",
            "SELECT \"File Name\", \"File Type\" FROM \"Customers Logo\""
        ),
        [[Text("hello.txt".into()), Text("txt".into())]]
    );
    assert_eq!(
        rows(
            m,
            "tpl",
            "SELECT \"Value\" FROM \"Customers Tags\" ORDER BY \"ID\""
        ),
        [[Text("vip".into())], [Text("north".into())]]
    );
    assert_eq!(
        rows(m, "tpl", "SELECT Website FROM Customers WHERE ID = 1"),
        [[Text("https://contoso.example/".into())]]
    );
    assert_eq!(
        rows(m, "tpl", "SELECT \"Order Date\" FROM Orders WHERE ID = 1"),
        [[Date("2024-01-05".into())]]
    );
    // Constraints and the calculated column work in the database.
    let db = rusqlite::Connection::open(m.database_path("tpl").unwrap()).unwrap();
    db.execute_batch("PRAGMA foreign_keys=ON").unwrap();
    assert!(
        db.execute("INSERT INTO Orders (Customer, Amount) VALUES (99, 1)", [])
            .is_err(),
        "foreign key"
    );
    assert!(
        db.execute("INSERT INTO Orders (Customer, Amount) VALUES (1, -1)", [])
            .is_err(),
        "field rule"
    );
    assert!(db.execute("INSERT INTO Orders (Customer, \"Order Date\", Shipped) VALUES (1, '2024-05-02', '2024-05-01')", []).is_err(), "table rule");
    db.execute("INSERT INTO Customers (Company, \"First Name\", \"Last Name\") VALUES ('New', 'Kim', 'Ng')", []).unwrap();
    let contact: String = db
        .query_row(
            "SELECT Contact FROM Customers WHERE Company = 'New'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(contact, "Kim Ng");
    let date: String = db
        .query_row(
            "SELECT \"Order Date\" FROM Orders WHERE rowid = (SELECT max(rowid) FROM Orders)",
            [],
            |r| r.get(0),
        )
        .unwrap_or_default();
    assert!(date.is_empty() || date.len() == 10);
    db.execute("DELETE FROM Customers WHERE ID = 1", [])
        .unwrap();
    let left: i64 = db
        .query_row("SELECT count(*) FROM Orders WHERE Customer = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(left, 0, "cascade delete");
    drop(db);
    m.mark_data_dirty("tpl").unwrap();
    // Queries run in DuckDB.
    let q = |name: &str| {
        config
            .saved_queries
            .iter()
            .find(|q| q.name == name)
            .unwrap_or_else(|| panic!("no query {name}"))
    };
    let totals = rows(m, "tpl", &q("Order Totals").sql);
    assert_eq!(totals[0][0], Text("Fabrikam, Inc.".into()));
    assert!(rows(m, "tpl", &q("ContactsAndCompanies").sql).len() >= 3);
    action_queries_convert_and_run(m, &config);
    // Forms: list, detail, generated detail for the subform's rows.
    let list = form(&config, "Customer List");
    let details = form(&config, "Customer Details");
    assert_eq!(list.list_columns, ["Company", "Category", "Website"]);
    assert_eq!(list.detail_form_id.as_deref(), Some(details.id.as_str()));
    let kinds: Vec<String> = details
        .controls
        .iter()
        .map(|c| format!("{:?}:{}", c.kind, c.label))
        .collect();
    assert_eq!(
        kinds,
        [
            "Button:Print orders",
            "Button:Save and New",
            "Text:Company",
            "Select:Category",
            "Computed:Contact",
            "Tabs:",
            "Multiline:Website",
            "RelatedList:Logo",
            "RelatedList:Tags",
            "RelatedList:Orders"
        ]
        .map(|s| s.replace("Multiline:Website", "Text:Website"))
    );
    let orders_list = details
        .controls
        .iter()
        .find(|c| c.label == "Orders")
        .unwrap()
        .related
        .as_ref()
        .unwrap();
    assert_eq!(
        (
            orders_list.table.as_str(),
            orders_list.foreign_key.as_str(),
            orders_list.parent_column.as_str()
        ),
        ("Orders", "Customer", "ID")
    );
    assert!(orders_list.form_id.is_some());
    let tabs = details
        .controls
        .iter()
        .find(|c| c.label.is_empty() && !c.tabs.is_empty())
        .unwrap();
    assert_eq!(
        tabs.tabs
            .iter()
            .map(|t| t.label.as_str())
            .collect::<Vec<_>>(),
        ["Web", "Files"]
    );
    // Buttons run the converted macros.
    let action = |label: &str| {
        let id = details
            .controls
            .iter()
            .find(|c| c.label == label)
            .unwrap()
            .action_id
            .clone()
            .unwrap();
        config.actions.iter().find(|a| a.id == id).unwrap().clone()
    };
    assert_eq!(
        action("Print orders")
            .steps
            .iter()
            .map(|s| s.kind.as_str())
            .collect::<Vec<_>>(),
        ["openReport", "message"]
    );
    let save_new = action("Save and New");
    assert_eq!(save_new.steps[0].kind, "openForm");
    assert_eq!(save_new.steps[0].fields["mode"], "create");
    // The report groups by customer and shows the customer's name.
    let rep = &config.reports[0];
    assert_eq!(rep.bands.groups.len(), 1);
    assert_eq!(rep.bands.groups[0].group_by, "record.Customer");
    let data = config
        .saved_queries
        .iter()
        .find(|q| Some(&q.id) == rep.dataset_query_id.as_ref())
        .unwrap();
    let report_rows = m.read_query("tpl", &data.sql).unwrap();
    assert!(report_rows
        .columns
        .contains(&"Customer (display)".to_string()));
    // Navigation starts at the form AutoExec opens.
    let start = config.design.start_page.clone().unwrap();
    let item = config
        .design
        .navigation
        .iter()
        .find(|i| i.id == start)
        .unwrap();
    assert_eq!(item.target_id.as_deref(), Some(list.id.as_str()));
    // The VBA is kept as an asset; the report says so.
    let vba = m
        .asset_list("tpl")
        .unwrap()
        .into_iter()
        .find(|a| a.display_name == "Access VBA.txt")
        .unwrap();
    assert_eq!(config.settings["accessImport"]["vbaAsset"], vba.id);
    assert!(m.orphan_assets("tpl").unwrap().is_empty());
    assert!(config.settings["accessImport"]["report"]
        .as_array()
        .is_some_and(|r| !r.is_empty()));
    let module = report.items.iter().find(|i| i.kind == "module").unwrap();
    assert_eq!(module.status, Status::Skipped);
    m.close("tpl", true).unwrap();
}

#[test]
fn binary_databases_get_generated_forms_and_working_queries() {
    use crate::data::DataValue::*;
    for (i, name) in ["orders.accdb", "orders.mdb"].iter().enumerate() {
        let window = format!("bin{i}");
        let (report, errs) = import(&super::fixtures::binary(name), &window);
        assert_eq!(errs, Vec::<String>::new(), "{name}");
        let m = manager();
        let config = m.config(&window).unwrap();
        assert_eq!(
            rows(m, &window, "SELECT count(*) FROM Orders"),
            [[Integer(8)]]
        );
        assert_eq!(
            rows(m, &window, "SELECT Discount FROM Orders WHERE ID = 3"),
            [[Real(0.15)]]
        );
        let lists: Vec<&str> = config
            .design
            .forms
            .iter()
            .filter(|f| f.modes.len() == 1)
            .map(|f| f.name.as_str())
            .collect();
        assert_eq!(lists, ["Customers", "Orders"], "{name}");
        let detail = form(&config, "Orders details");
        let customer = detail
            .controls
            .iter()
            .find(|c| c.binding.as_ref().is_some_and(|b| b.column == "Customer"))
            .unwrap();
        let rel = customer.relationship.as_ref().unwrap();
        assert_eq!(
            (
                rel.table.as_str(),
                rel.value_column.as_str(),
                rel.display_column.as_str()
            ),
            ("Customers", "ID", "Company")
        );
        let big = config
            .saved_queries
            .iter()
            .find(|q| q.name == "Big Orders")
            .unwrap();
        assert_eq!(big.parameters[0].name, "minimum_amount");
        assert_eq!(big.parameters[0].logical_type, "number");
        let sql = big.sql.replace("$minimum_amount", "1000");
        assert_eq!(
            rows(m, &window, &format!("SELECT count(*) FROM ({sql})")),
            [[Integer(2)]]
        );
        let unpaid = report
            .items
            .iter()
            .find(|i| i.name == "Unpaid Orders")
            .unwrap();
        assert_eq!(unpaid.status, Status::Skipped);
        m.close(&window, true).unwrap();
    }
}

#[test]
fn layout_groups_rows_and_columns() {
    use crate::access::convert::layout::{place, Rect};
    let r = |left, top, width| Rect {
        left,
        top,
        width,
        height: 300,
    };
    let items = [
        r(0, 0, 5000),
        r(5000, 40, 5000),
        r(0, 500, 10000),
        r(9000, 900, 3000),
        r(0, 900, 9500),
    ];
    let (p, next) = place(&items, 0, 10000, 1);
    let cells: Vec<(i64, i64, i64)> = p.iter().map(|x| (x.row, x.column, x.span)).collect();
    // The widest control (12000 twips) sets the scale; overlaps push right.
    assert_eq!(
        cells,
        [(1, 1, 5), (1, 6, 5), (2, 1, 10), (3, 11, 2), (3, 1, 10)]
    );
    assert_eq!(next, 4);
}

#[test]
fn hyperlinks_keep_their_address() {
    use crate::access::convert::staging::hyperlink_address;
    assert_eq!(
        hyperlink_address("Contoso#https://contoso.example/#"),
        "https://contoso.example/"
    );
    assert_eq!(
        hyperlink_address("#mailto:a@b.example#"),
        "mailto:a@b.example"
    );
    assert_eq!(hyperlink_address("Doc##Sheet1!A1#"), "Doc#Sheet1!A1");
    assert_eq!(hyperlink_address("plain"), "plain");
}

#[test]
fn forms_opened_from_related_lists_hold_no_related_lists() {
    let list = |form: &str| serde_json::json!({ "kind": "relatedList", "related": { "table": "T", "formId": form } });
    let mut forms = vec![
        serde_json::json!({ "id": "a", "controls": [list("b")] }),
        serde_json::json!({ "id": "b", "controls": [list("c")] }),
        serde_json::json!({ "id": "c", "controls": [] }),
    ];
    convert::forms::unnest_related_lists(&mut forms);
    assert!(forms[0]["controls"][0]["related"].get("formId").is_none());
    assert_eq!(forms[1]["controls"][0]["related"]["formId"], "c");
}

/// Append, update and make-table queries and RunSQL/OpenQuery macro actions
/// become action queries; run them on the imported data the way `action::run` does.
fn action_queries_convert_and_run(m: &DocumentManager, config: &crate::archive::DocumentConfig) {
    use crate::archive::ActionKind;
    use crate::data::DataValue::*;
    use crate::queries::action::{plan, Plan};
    let q = |name: &str| {
        config
            .saved_queries
            .iter()
            .find(|q| q.name == name)
            .unwrap_or_else(|| panic!("no query {name}"))
    };
    let spec = |name: &str| {
        q(name)
            .action
            .clone()
            .unwrap_or_else(|| panic!("{name} is no action query"))
    };
    assert_eq!(
        (
            spec("PaidOrdersAppend").kind,
            spec("PaidOrdersAppend").table.as_str()
        ),
        (ActionKind::Insert, "Orders")
    );
    assert_eq!(spec("RaiseBigOrders").kind, ActionKind::Update);
    assert_eq!(q("RaiseBigOrders").parameters[0].name, "minimum");
    let make = spec("MakeCustomerTotals");
    assert_eq!(
        (make.kind, make.table.as_str()),
        (ActionKind::Replace, "Customer Totals")
    );
    assert_eq!(
        config.settings["accessImport"]["actionQueries"],
        serde_json::json!([])
    );
    assert_eq!(
        config.migrations.len(),
        2,
        "migration 002 creates the make-table target"
    );
    // RunSQL and OpenQuery become runQuery steps.
    let nightly = config.actions.iter().find(|a| a.name == "Nightly").unwrap();
    let ids: Vec<String> = nightly
        .steps
        .iter()
        .map(|s| s.fields["queryId"].as_str().unwrap().to_string())
        .collect();
    let run_sql = config
        .saved_queries
        .iter()
        .find(|x| x.id == ids[0])
        .unwrap();
    assert_eq!(run_sql.name, "Nightly (RunSQL 1)");
    assert_eq!(run_sql.action.as_ref().unwrap().kind, ActionKind::Delete);
    assert_eq!(ids[1], q("MakeCustomerTotals").id);
    // Run them: the make-table query fills the new table; the update goes through the copy.
    let workspace = m
        .database_path("tpl")
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let extension = crate::data::extensions::sqlite_extension_path().unwrap();
    let writer = || {
        crate::data::write::open_writer(
            &workspace,
            &extension,
            &crate::data::read::ReadTarget::Sqlite,
            false,
        )
        .unwrap()
    };
    let job = |table: &str, kind| crate::queries::action_exec::Job {
        table: format!("data.\"main\".\"{table}\""),
        name: table.into(),
        keys: vec!["ID".into()],
        rowid: true,
        kind,
        watch: false,
        dry_run: false,
        deleted: false,
        before: Default::default(),
        sqlite: true,
        logical: vec![],
        generated: vec![],
        defaulted: vec![],
    };
    let Plan::Direct(sql) = plan(q("MakeCustomerTotals"), &make, "main", true)
        .unwrap()
        .0
    else {
        panic!("replace runs directly");
    };
    let out = crate::queries::action_exec::direct(
        &writer(),
        &sql,
        &[],
        &job("Customer Totals", ActionKind::Replace),
    )
    .unwrap();
    assert!(out.changed > 0);
    m.mark_data_dirty("tpl").unwrap();
    let totals = rows(m, "tpl", "SELECT count(*) FROM \"Customer Totals\"");
    assert_eq!(totals[0][0], Integer(out.changed as i64));
    let raise = spec("RaiseBigOrders");
    let Plan::Copy(sql) = plan(q("RaiseBigOrders"), &raise, "main", true).unwrap().0 else {
        panic!("embedded-SQLite updates run on a copy");
    };
    let before = rows(m, "tpl", "SELECT count(*) FROM Orders WHERE Amount > 100");
    let w = writer();
    let computed = crate::queries::action_exec::compute_update(
        &w,
        &sql,
        &[duckdb::types::Value::Double(100.0)],
        &job("Orders", ActionKind::Update),
    )
    .unwrap();
    drop(w);
    assert_eq!(Integer(computed.outcome.changed as i64), before[0][0]);
    let written = crate::queries::action_tests::apply(
        &m.database_path("tpl").unwrap(),
        &job("Orders", ActionKind::Update),
        &computed.changes,
    )
    .unwrap();
    assert_eq!(written, computed.outcome.changed);
    for item in m.config("tpl").unwrap().settings["accessImport"]["report"]
        .as_array()
        .unwrap()
    {
        let notes = item["notes"].to_string();
        assert!(!notes.contains("DuckDB rejects"), "{notes}");
    }
}
