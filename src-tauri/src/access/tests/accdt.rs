use crate::access::accdt::TemplatePackage;
use crate::access::model::AccessFile;

/// Directory of downloaded Microsoft templates (`scripts/access/fetch-templates.mjs`).
pub fn templates_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("IXTABLE_ACCESS_TEMPLATES_DIR").map(Into::into)
}

#[test]
fn every_downloaded_template_reads() {
    let Some(dir) = templates_dir() else { return };
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .collect();
    files.sort();
    for path in files
        .iter()
        .filter(|p| p.extension().is_some_and(|e| e == "accdt"))
    {
        let mut pkg = TemplatePackage::open(std::fs::File::open(path).unwrap()).unwrap();
        let tables: Vec<String> = pkg.db().tables.iter().map(|t| t.name.clone()).collect();
        let mut rows = 0;
        for t in &tables {
            pkg.rows(t, &mut |_| {
                rows += 1;
                Ok(())
            })
            .unwrap();
        }
        let db = pkg.db();
        eprintln!(
            "{}: {} tables, {} rows, {} rels, {} queries, {} forms, {} reports, {} macros, {} modules, warnings {:?}",
            path.file_name().unwrap().to_string_lossy(),
            db.tables.len(),
            rows,
            db.relationships.len(),
            db.queries.len(),
            db.forms.len(),
            db.reports.len(),
            db.macros.len(),
            db.modules.len(),
            db.warnings
        );
        assert!(!db.tables.is_empty());
    }
}

#[test]
fn template_fixture_reads_every_object() {
    use crate::access::model::{ColType, Complex, QueryKind, Value};
    let path = super::fixtures::template();
    let mut pkg = TemplatePackage::open(std::fs::File::open(path).unwrap()).unwrap();
    let db = pkg.db().clone();
    assert!(db.warnings.is_empty(), "{:?}", db.warnings);
    assert_eq!(
        db.props.get("AppTitle").map(String::as_str),
        Some("Order Desk")
    );
    let customers = db.table("Customers").unwrap();
    let names: Vec<&str> = customers.columns.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "ID",
            "Company",
            "Category",
            "First Name",
            "Last Name",
            "Contact",
            "Website",
            "Logo",
            "Tags"
        ]
    );
    let col = |n: &str| customers.column(n).unwrap();
    assert!(col("ID").auto_number);
    assert!(col("Company").required());
    assert_eq!(col("Company").size, 100);
    assert_eq!(col("Company").prop("Caption"), Some("Company name"));
    assert_eq!(col("Category").prop("RowSourceType"), Some("Value List"));
    assert_eq!(
        col("Contact").expression.as_deref(),
        Some("[First Name] & \" \" & [Last Name]")
    );
    assert!(col("Website").hyperlink);
    assert_eq!(col("Logo").complex, Some(Complex::Attachment));
    assert_eq!(
        col("Tags").complex,
        Some(Complex::MultiValue(ColType::Text))
    );
    assert!(customers
        .indexes
        .iter()
        .any(|i| i.name == "Company" && i.unique && !i.primary));
    let orders = db.table("Orders").unwrap();
    assert_eq!(
        orders.prop("ValidationRule"),
        Some("[Shipped] Is Null Or [Shipped]>=[Order Date]")
    );
    assert_eq!(orders.column("Amount").unwrap().ty, ColType::Currency);
    assert_eq!(db.relationships.len(), 1);
    let r = &db.relationships[0];
    assert_eq!(
        (
            r.table.as_str(),
            r.columns[0].as_str(),
            r.ref_table.as_str(),
            r.ref_columns[0].as_str()
        ),
        ("Orders", "Customer", "Customers", "ID")
    );
    assert!(r.enforced());
    let kinds: Vec<(&str, QueryKind)> = db
        .queries
        .iter()
        .map(|q| (q.name.as_str(), q.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("ContactsAndCompanies", QueryKind::Union),
            ("MakeCustomerTotals", QueryKind::MakeTable),
            ("Order Totals", QueryKind::Select),
            ("PaidOrdersAppend", QueryKind::Append),
            ("RaiseBigOrders", QueryKind::Update)
        ]
    );
    let totals = db.query("Order Totals").unwrap();
    assert_eq!(
        totals.sql,
        "SELECT Customers.Company, Sum(Orders.Amount) AS Total, Sum(IIf([Paid],0,[Amount])) AS Unpaid, Max(Orders.[Order Date]) AS [Last order]\nFROM Customers LEFT JOIN Orders ON Customers.ID = Orders.Customer\nGROUP BY Customers.Company\nORDER BY Sum(Orders.Amount) DESC"
    );
    let forms: Vec<&str> = db.forms.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        forms,
        ["Customer Details", "Customer List", "Customer Orders"]
    );
    let details = db.form("Customer Details").unwrap();
    assert!(details
        .root
        .code
        .as_deref()
        .is_some_and(|c| c.contains("Form_Current")));
    assert_eq!(db.reports[0].name, "Orders by Customer");
    assert_eq!(db.macros[0].name, "AutoExec");
    assert_eq!(db.modules[0].name, "Helpers");
    let mut rows = vec![];
    pkg.rows("Customers", &mut |r| {
        rows.push(r);
        Ok(())
    })
    .unwrap();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0][1], Value::Text("Contoso Ltd".into()));
    match &rows[0][7] {
        Value::Attachments(files) => {
            assert_eq!(files.len(), 1);
            assert_eq!(
                (
                    files[0].file_name.as_str(),
                    files[0].file_type.as_str(),
                    files[0].data.as_slice()
                ),
                ("hello.txt", "txt", &b"hello"[..])
            );
        }
        other => panic!("attachments: {other:?}"),
    }
    assert_eq!(
        rows[0][8],
        Value::Multi(vec![Value::Text("vip".into()), Value::Text("north".into())])
    );
    assert_eq!(rows[2][1], Value::Text("Northwind Café".into()));
}

#[test]
fn package_errors_are_clear() {
    let not_zip = super::fixtures::temp("x.accdt");
    std::fs::write(&not_zip, b"hello").unwrap();
    let err = TemplatePackage::open(std::fs::File::open(&not_zip).unwrap())
        .err()
        .unwrap();
    assert!(err.contains("not an Access template package"), "{err}");
}

#[test]
fn xml_entities_and_escaped_names() {
    use crate::access::xml::{parse, unescape_name};
    let root = parse("<?xml version=\"1.0\"?><!-- c --><a x=\"1 &amp; 2\"><b>&lt;&#65;&#x42;&gt;</b><![CDATA[<raw>]]><c/></a>").unwrap();
    assert_eq!(root.attr("x"), Some("1 & 2"));
    assert_eq!(root.child("b").unwrap().text(), "<AB>");
    assert_eq!(root.text(), "<AB><raw>");
    assert_eq!(unescape_name("Order_x0020_Date"), "Order Date");
    assert_eq!(unescape_name("plain_x_name"), "plain_x_name");
}
