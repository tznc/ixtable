use crate::access::model::QueryKind;
use crate::access::query_def::{to_access_sql, Join, OutputColumn, QueryDef};
use crate::access::translate::ast::{parse_expression, parse_field_rule, parse_statement};
use crate::access::translate::expr::{translate, Target};
use crate::access::translate::format::format_pattern;
use crate::access::translate::sql::{Dialect, Kind, Schema, SqlWriter};

struct Fixture;

impl Schema for Fixture {
    fn columns(&self, source: &str) -> Option<Vec<(String, Kind)>> {
        let cols: &[(&str, Kind)] = match source.to_lowercase().as_str() {
            "orders" => &[
                ("ID", Kind::Number),
                ("Customer", Kind::Number),
                ("Order Date", Kind::Date),
                ("Amount", Kind::Number),
                ("Paid", Kind::Bool),
                ("Status", Kind::Text),
            ],
            "customers" => &[
                ("ID", Kind::Number),
                ("Company", Kind::Text),
                ("Logo", Kind::Other),
            ],
            "open orders" => &[("ID", Kind::Number), ("Amount", Kind::Number)],
            _ => return None,
        };
        Some(cols.iter().map(|(n, k)| (n.to_string(), *k)).collect())
    }

    fn is_query(&self, source: &str) -> bool {
        source.eq_ignore_ascii_case("Open Orders")
    }

    fn complex(&self, source: &str) -> Vec<String> {
        if source.eq_ignore_ascii_case("Customers") {
            vec!["Logo".into()]
        } else {
            vec![]
        }
    }
}

fn duck(sql: &str) -> String {
    let st = parse_statement(sql).unwrap_or_else(|e| panic!("{sql}: {e}"));
    let mut w = SqlWriter::new(Dialect::DuckDb, &Fixture);
    w.statement(&st).unwrap_or_else(|e| panic!("{sql}: {e}"))
}

fn duck_full(sql: &str, declared: &[(&str, &str)]) -> (String, Vec<(String, String)>, Vec<String>) {
    let st = parse_statement(sql).unwrap();
    let mut w = SqlWriter::new(Dialect::DuckDb, &Fixture);
    w.declare(
        &declared
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect::<Vec<_>>(),
    );
    let out = w.statement(&st).unwrap();
    (
        out,
        w.out
            .params
            .iter()
            .map(|p| (p.name.clone(), p.logical_type.clone()))
            .collect(),
        w.out.queries.clone(),
    )
}

#[test]
fn names_literals_and_operators() {
    assert_eq!(
        duck("SELECT Orders.[Order Date], Amount*2 AS Twice FROM Orders WHERE Orders.Status=\"open\" And Amount Between 1 And 10"),
        "SELECT \"Orders\".\"Order Date\", \"Amount\" * 2 AS \"Twice\" FROM \"Orders\" WHERE lower(\"Orders\".\"Status\") = lower('open') AND \"Amount\" BETWEEN 1 AND 10"
    );
    assert_eq!(
        duck("SELECT [Status] & \" #\" & ID FROM Orders WHERE [Order Date] >= #1/31/2024# And [Order Date] < #2024-03-01 10:30:00#"),
        "SELECT concat(concat(\"Status\", ' #'), \"ID\") AS \"Expr1000\" FROM \"Orders\" WHERE \"Order Date\" >= DATE '2024-01-31' AND \"Order Date\" < TIMESTAMP '2024-03-01 10:30:00'"
    );
    assert_eq!(
        duck("SELECT TOP 5 PERCENT DISTINCTROW * FROM Orders ORDER BY Amount DESC"),
        "SELECT * FROM \"Orders\" ORDER BY \"Amount\" DESC LIMIT 5%"
    );
    assert_eq!(duck("SELECT ID FROM Orders WHERE Status Like \"a*b?\" Or Status Not Like \"[!x]#*\""), "SELECT \"ID\" FROM \"Orders\" WHERE \"Status\" ILIKE 'a%b_' ESCAPE '\\' OR NOT regexp_full_match(\"Status\", '[^x][0-9].*', 'i')");
    assert_eq!(
        duck("SELECT 7 \\ 2 AS q, 7 Mod 2 AS r, 2^3 AS p FROM Orders"),
        "SELECT (7) // (2) AS \"q\", 7 % 2 AS \"r\", power(2, 3) AS \"p\" FROM \"Orders\""
    );
    assert_eq!(
        duck("SELECT Sum(Abs([Paid])) AS n FROM Orders"),
        "SELECT sum(abs((-CAST(\"Paid\" AS INTEGER)))) AS \"n\" FROM \"Orders\""
    );
}

#[test]
fn functions_and_date_arithmetic() {
    let sql = duck("SELECT IIf(IsNull(Status),Nz(Amount),Len(Status)) AS a, DateAdd(\"m\",1,[Order Date]) AS b, DateDiff(\"d\",[Order Date],Date()) AS c, [Order Date]+7 AS d, Format([Order Date],\"yyyy\") AS e, Int(Now()) AS f FROM Orders");
    assert_eq!(
        sql,
        "SELECT CASE WHEN (\"Status\" IS NULL) THEN coalesce(\"Amount\", 0) ELSE length(\"Status\") END AS \"a\", (\"Order Date\" + to_months(CAST(1 AS INTEGER))) AS \"b\", date_diff('day', \"Order Date\", CAST(CAST(now() AS TIMESTAMP) AS DATE)) AS \"c\", (CAST(\"Order Date\" AS TIMESTAMP) + to_seconds(CAST(round((7) * 86400) AS BIGINT))) AS \"d\", strftime(\"Order Date\", '%Y') AS \"e\", CAST(CAST(now() AS TIMESTAMP) AS DATE) AS \"f\" FROM \"Orders\""
    );
    assert_eq!(
        duck("SELECT DLookup(\"Company\",\"Customers\",\"ID=1\") AS c FROM Orders"),
        "SELECT (SELECT \"Company\" FROM \"Customers\" WHERE \"ID\" = 1 LIMIT 1) AS \"c\" FROM \"Orders\""
    );
    assert_eq!(
        duck("SELECT MyVbaFunction(ID) AS x FROM Orders"),
        "SELECT NULL AS \"x\" FROM \"Orders\""
    );
}

#[test]
fn parameters_form_references_and_query_sources() {
    let (sql, params, queries) = duck_full(
        "PARAMETERS [Start Date] DateTime; SELECT o.ID FROM [Open Orders] AS o WHERE o.Amount > [Forms]![Orders]![Minimum] And [Placed] >= [Start Date]",
        &[("Start Date", "DateTime")],
    );
    assert_eq!(sql, "SELECT \"o\".\"ID\" FROM \"Open Orders\" AS \"o\" WHERE \"o\".\"Amount\" > $forms_orders_minimum AND $placed >= $start_date");
    // A name no source has is a parameter, as Access prompts for it.
    assert_eq!(
        params,
        [
            ("forms_orders_minimum".to_string(), "text".to_string()),
            ("placed".to_string(), "text".to_string()),
            ("start_date".to_string(), "date".to_string())
        ]
    );
    assert_eq!(queries, ["Open Orders"]);
}

#[test]
fn joins_unions_aliases_and_complex_columns() {
    assert_eq!(
        duck("SELECT Customers.Company, Customers.Logo, Count(*) AS N FROM Customers LEFT JOIN Orders ON Customers.ID = Orders.Customer GROUP BY Customers.Company HAVING Count(*)>1"),
        "SELECT \"Customers\".\"Company\", count(*) AS \"N\" FROM (\"Customers\" LEFT JOIN \"Orders\" ON \"Customers\".\"ID\" = \"Orders\".\"Customer\") GROUP BY \"Customers\".\"Company\" HAVING count(*) > 1"
    );
    // An alias used before it is defined is inlined, as Access allows.
    assert_eq!(
        duck("SELECT [Net]*2 AS Gross, Amount-1 AS Net FROM Orders"),
        "SELECT (\"Amount\" - 1) * 2 AS \"Gross\", \"Amount\" - 1 AS \"Net\" FROM \"Orders\""
    );
    assert_eq!(
        duck("SELECT ID FROM Orders UNION ALL SELECT ID FROM Customers ORDER BY ID"),
        "SELECT \"ID\" FROM \"Orders\" UNION ALL SELECT \"ID\" FROM \"Customers\" ORDER BY \"ID\""
    );
    assert!(duck("TRANSFORM Sum(Amount) SELECT Customer FROM Orders GROUP BY Customer PIVOT Status").starts_with("SELECT * FROM (PIVOT (SELECT \"Customer\", \"Status\" AS \"__pivot\", \"Amount\" AS \"__value\" FROM \"Orders\") ON \"__pivot\" USING sum(\"__value\") GROUP BY \"Customer\")"));
}

#[test]
fn field_rules_and_sqlite_constraints() {
    let cols = vec![
        ("Amount".to_string(), Kind::Number),
        ("Email".to_string(), Kind::Text),
    ];
    let sqlite = |rule: &str, field: Option<&str>| {
        crate::access::convert::schema::rule_sql(rule, field, &cols)
    };
    assert_eq!(
        sqlite(">=0 And <=100", Some("Amount")).unwrap(),
        "\"Amount\" >= 0 AND \"Amount\" <= 100"
    );
    assert_eq!(
        sqlite("Is Null Or Like \"*@*.*\"", Some("Email")).unwrap(),
        "\"Email\" IS NULL OR \"Email\" LIKE '%@%.%' ESCAPE '\\'"
    );
    assert_eq!(
        sqlite("[Amount]>0 Or [Email] Is Not Null", None).unwrap(),
        "\"Amount\" > 0 OR \"Email\" IS NOT NULL"
    );
    assert!(sqlite("<=Date()", Some("Amount"))
        .unwrap_err()
        .contains("current date"));
    assert!(sqlite("[Missing]>0", None).is_err());
    let default = |v: &str| crate::access::convert::schema::default_sql(v, &cols).unwrap();
    assert_eq!(default("=Date()"), "(CURRENT_DATE)");
    assert_eq!(default("Now()"), "(CURRENT_TIMESTAMP)");
    assert_eq!(default("\"New\""), "'New'");
    assert_eq!(default("Yes"), "1");
    assert_eq!(default("0"), "0");
    let e = parse_field_rule("Not Between 1 And 5", "Amount").unwrap();
    assert!(matches!(
        e,
        crate::access::translate::ast::Expr::Between { not: true, .. }
    ));
}

#[test]
fn ixtable_expressions_for_forms_and_reports() {
    let fields = |n: &str| {
        ["Amount", "First Name", "Last Name", "Due", "Status"]
            .iter()
            .find(|f| f.eq_ignore_ascii_case(n))
            .map(|f| crate::access::translate::expr::field("record", f))
    };
    let form =
        |e: &str| translate(e, Target::Form, &fields).unwrap_or_else(|err| panic!("{e}: {err}"));
    assert_eq!(
        form("=[First Name] & \" \" & [Last Name]"),
        "record.[First Name] & ' ' & record.[Last Name]"
    );
    assert_eq!(
        form("IIf(IsNull([Due]),\"none\",Format([Due],\"Short Date\"))"),
        "if(isnull(record.Due), 'none', format(record.Due, 'M/d/yyyy'))"
    );
    assert_eq!(
        form("Nz([Amount],0)*2 >= 10 And [Status] Like \"Open*\""),
        "coalesce(record.Amount, 0) * 2 >= 10 and startswith(record.Status, 'Open')"
    );
    assert_eq!(
        form("DateAdd(\"ww\",2,[Due]) < Date()"),
        "dateadd('week', 2, record.Due) < today()"
    );
    assert_eq!(form("[TempVars]![User] = \"x\""), "app.User = 'x'");
    assert_eq!(
        form("InStr([Status],\"x\")>0"),
        "contains(record.Status, 'x')"
    );
    assert!(translate("Forms!Other!Field", Target::Form, &fields).is_err());
    assert!(translate("DCount(\"*\",\"T\")", Target::Form, &fields).is_err());
    let report = |e: &str| translate(e, Target::Report, &fields).unwrap();
    assert_eq!(report("=Sum([Amount])"), "sum(rows.Amount)");
    assert_eq!(report("=Sum([Amount]*2)"), "sumof(rows, Amount * 2)");
    assert_eq!(report("=Count(*)"), "count(rows)");
    assert_eq!(
        report("=\"Page \" & [Page] & \" of \" & [Pages]"),
        "'Page ' & page & ' of ' & pages"
    );
    assert_eq!(
        report("=IIf([Report].[FilterOn],[Report].[Filter],\"\")"),
        "if(false, '', '')"
    );
    assert!(translate("=Sum([Amount])", Target::Form, &fields).is_err());
}

#[test]
fn format_patterns() {
    assert_eq!(format_pattern("Currency").as_deref(), Some("$#,##0.00"));
    assert_eq!(format_pattern("Short Date").as_deref(), Some("M/d/yyyy"));
    assert_eq!(
        format_pattern("mm/dd/yyyy hh:nn").as_deref(),
        Some("MM/dd/yyyy HH:mm")
    );
    assert_eq!(format_pattern("h:nn AM/PM").as_deref(), Some("h:mm tt"));
    assert_eq!(
        format_pattern("#,##0.00;(#,##0.00);\"Zero\"").as_deref(),
        Some("#,##0.00")
    );
    assert_eq!(format_pattern("\\$0.0").as_deref(), Some("$0.0"));
    assert_eq!(format_pattern("General Number"), None);
    assert_eq!(format_pattern("@;\"(none)\""), None);
}

#[test]
fn expression_parser_precedence() {
    use crate::access::translate::ast::{BinOp, Expr};
    match parse_expression("1 + 2 * 3 & \"x\" = \"7x\" Or Not True").unwrap() {
        Expr::Bin(BinOp::Or, l, r) => {
            assert!(matches!(*l, Expr::Bin(BinOp::Eq, ..)));
            assert!(matches!(*r, Expr::Not(_)));
        }
        other => panic!("{other:?}"),
    }
    assert!(parse_expression("Left([x],2)").is_ok());
    assert!(parse_expression("[unclosed").is_err());
    assert!(parse_expression("1 +").is_err());
}

#[test]
fn structured_queries_render_access_sql() {
    let mut def = QueryDef::new(QueryKind::Select);
    def.flags = 0x02 | 0x10;
    def.top = Some("3".into());
    def.inputs = vec![
        ("A".into(), None),
        ("B".into(), None),
        ("C Table".into(), Some("c".into())),
    ];
    def.columns = vec![OutputColumn {
        expression: "A.x".into(),
        alias: Some("X Value".into()),
        name: None,
    }];
    def.joins = vec![
        Join {
            left: "A".into(),
            right: "B".into(),
            expression: "A.id=B.a".into(),
            kind: 1,
        },
        Join {
            left: "B".into(),
            right: "c".into(),
            expression: "B.id=c.b".into(),
            kind: 2,
        },
        Join {
            left: "A".into(),
            right: "B".into(),
            expression: "A.k=B.k".into(),
            kind: 1,
        },
    ];
    def.where_clause = Some("A.x>1".into());
    assert_eq!(
        to_access_sql(&def),
        "SELECT DISTINCT TOP 3 A.x AS [X Value]\nFROM (A INNER JOIN B ON ((A.id=B.a) AND (A.k=B.k))) LEFT JOIN [C Table] AS c ON B.id=c.b\nWHERE A.x>1"
    );
    let mut upd = QueryDef::new(QueryKind::Update);
    upd.inputs = vec![("T".into(), None)];
    upd.columns = vec![OutputColumn {
        expression: "0".into(),
        alias: None,
        name: Some("T.n".into()),
    }];
    assert_eq!(to_access_sql(&upd), "UPDATE T\nSET T.n = 0");
}

#[test]
fn action_statements_become_duckdb_statements_that_run() {
    let db = duckdb::Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE orders(\"ID\" INTEGER PRIMARY KEY, \"Customer\" INTEGER, \"Order Date\" DATE, \"Amount\" DOUBLE, \"Paid\" BOOLEAN, \"Status\" VARCHAR);
         CREATE TABLE customers(\"ID\" INTEGER PRIMARY KEY, \"Company\" VARCHAR, \"Logo\" BLOB);
         INSERT INTO customers VALUES (1, 'Contoso', NULL), (2, 'Fabrikam', NULL);
         INSERT INTO orders VALUES (1, 1, DATE '2024-01-05', 10, false, 'open'), (2, 2, DATE '2024-02-05', 20, false, 'open');",
    )
    .unwrap();
    let cases = [
        (
            "INSERT INTO Orders ( ID, Customer, Amount ) SELECT DISTINCTROW Orders.ID + 10, Orders.Customer, Orders.Amount * 2 FROM Orders WHERE Orders.Paid = False;",
            "INSERT INTO \"Orders\" (\"ID\", \"Customer\", \"Amount\") SELECT \"Orders\".\"ID\" + 10 AS \"Expr1000\", \"Orders\".\"Customer\", \"Orders\".\"Amount\" * 2 AS \"Expr1001\" FROM \"Orders\" WHERE \"Orders\".\"Paid\" = FALSE",
            2,
        ),
        (
            "INSERT INTO Customers (ID, Company) VALUES (3, \"Northwind\")",
            "INSERT INTO \"Customers\" (\"ID\", \"Company\") VALUES (3, 'Northwind')",
            1,
        ),
        (
            "UPDATE Orders INNER JOIN Customers ON Orders.Customer = Customers.ID SET Orders.Status = \"Paid by \" & Customers.Company, Orders.Paid = True WHERE Customers.Company Like \"Con*\";",
            "",
            2,
        ),
        (
            "UPDATE DISTINCTROW Orders SET Orders.[Order Date] = DateAdd(\"d\", 7, [Order Date]) WHERE Orders.ID > [Minimum ID]",
            "UPDATE \"Orders\" SET \"Order Date\" = (\"Order Date\" + to_days(CAST(7 AS INTEGER))) WHERE (\"Orders\".\"ID\" > $minimum_id)",
            4,
        ),
        (
            "DELETE Orders.* FROM Orders INNER JOIN Customers ON Orders.Customer = Customers.ID WHERE Customers.Company = \"Fabrikam\"",
            "",
            2,
        ),
        ("DELETE * FROM Customers WHERE ID = 3", "DELETE FROM \"Customers\" WHERE (\"ID\" = 3)", 1),
    ];
    for (access, expected, rows) in cases {
        let st = parse_statement(access).unwrap_or_else(|e| panic!("{access}: {e}"));
        let mut w = SqlWriter::new(Dialect::DuckDb, &Fixture);
        let sql = w.statement(&st).unwrap_or_else(|e| panic!("{access}: {e}"));
        if !expected.is_empty() {
            assert_eq!(sql, expected, "{access}");
        }
        let target = w.out.target.clone().unwrap();
        assert!(
            ["Orders", "Customers"].contains(&target.as_str()),
            "{access}: {target}"
        );
        // The bare parameter becomes $minimum_id; bind it so the statement runs.
        let sql = sql.replace("$minimum_id", "0");
        let changed = db
            .execute(&sql, [])
            .unwrap_or_else(|e| panic!("{sql}: {e}"));
        assert_eq!(changed, rows, "{sql}");
    }
    let status: Vec<String> = db
        .prepare("SELECT \"Status\" FROM orders ORDER BY \"ID\"")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    // Orders 1 and 11 (Contoso) were updated; Fabrikam's 2 and 12 were deleted.
    assert_eq!(status, ["Paid by Contoso", "Paid by Contoso"]);
    let outer = parse_statement("UPDATE Orders LEFT JOIN Customers ON Orders.Customer = Customers.ID SET Orders.Status = Customers.Company").unwrap();
    let err = SqlWriter::new(Dialect::DuckDb, &Fixture)
        .statement(&outer)
        .unwrap_err();
    assert!(err.contains("outer joins"), "{err}");
    let make = parse_statement("SELECT Orders.Customer, Sum(Orders.Amount) AS Total INTO [Customer Totals] FROM Orders GROUP BY Orders.Customer").unwrap();
    let mut w = SqlWriter::new(Dialect::DuckDb, &Fixture);
    w.statement(&make).unwrap();
    assert_eq!(w.out.target.as_deref(), Some("Customer Totals"));
}
