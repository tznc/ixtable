//! Ports of tests/unit/expr.test.ts and tests/unit/access-expressions.test.ts,
//! plus checks for the Rust-specific pieces (number printing, regex matcher).
use super::*;
use chrono::TimeZone;
use serde_json::json;

fn scope(v: Value) -> Scope {
    v.as_object().cloned().unwrap_or_default()
}

fn ev(src: &str) -> Value {
    ev_in(src, json!({}))
}

fn ev_in(src: &str, s: Value) -> Value {
    evaluate(src, &scope(s), EvaluateOptions::default()).unwrap_or_else(|e| {
        panic!("{src}: {e} ({}..{})", e.start, e.end);
    })
}

fn err(src: &str) -> ExprError {
    err_in(src, json!({}))
}

fn err_in(src: &str, s: Value) -> ExprError {
    match evaluate(src, &scope(s), EvaluateOptions::default()) {
        Err(e) => e,
        Ok(v) => panic!("{src}: expected an error but got {v}"),
    }
}

fn parse_err(src: &str) -> ExprError {
    match parse(src) {
        Err(e) => e,
        Ok(_) => panic!("{src}: expected a syntax error"),
    }
}

fn messages(src: &str, known: Option<&[&str]>) -> Vec<String> {
    check(src, known).into_iter().map(|d| d.message).collect()
}

#[test]
fn evaluates_literals() {
    assert_eq!(ev("42"), json!(42));
    assert_eq!(ev("3.25"), json!(3.25));
    assert_eq!(ev(".5"), json!(0.5));
    assert_eq!(ev("'it''s'"), json!("it's"));
    assert_eq!(ev("\"say \"\"hi\"\"\""), json!("say \"hi\""));
    assert_eq!(ev("TRUE"), json!(true));
    assert_eq!(ev("false"), json!(false));
    assert_eq!(ev("Null"), Value::Null);
    assert_eq!(ev("#2026-01-31#"), json!("2026-01-31"));
    assert_eq!(ev("#2026-01-31T09:05:00#"), json!("2026-01-31T09:05:00"));
}

#[test]
fn follows_operator_precedence() {
    assert_eq!(ev("1 + 2 * 3"), json!(7));
    assert_eq!(ev("(1 + 2) * 3"), json!(9));
    assert_eq!(ev("10 - 4 - 3"), json!(3));
    assert_eq!(ev("2 * 3 % 4"), json!(2));
    assert_eq!(ev("-2 * 3"), json!(-6));
    assert_eq!(ev("- (1 + 1)"), json!(-2));
    assert_eq!(ev("1 + 2 & 'x'"), json!("3x"));
    assert_eq!(ev("1 < 2 and 3 > 4 or true"), json!(true));
    assert_eq!(ev("not 1 = 2"), json!(true));
    assert_eq!(ev("not false and false"), json!(false));
}

#[test]
fn removes_floating_point_noise() {
    assert_eq!(ev("0.1 + 0.2"), json!(0.3));
    assert_eq!(ev("1.1 * 3"), json!(3.3));
    assert_eq!(ev("0.3 - 0.1"), json!(0.2));
    assert_eq!(ev("1 / 3"), json!(0.333333333333333));
    assert_eq!(ev("round(1.005, 2)"), json!(1.01));
    assert_eq!(ev("round(-2.5)"), json!(-3));
}

#[test]
fn returns_null_when_dividing_by_zero() {
    assert_eq!(ev("1 / 0"), Value::Null);
    assert_eq!(ev("5 % 0"), Value::Null);
}

fn names_scope() -> Value {
    json!({
        "record": { "amount": 12.5, "Unit Price": 3, "customer": { "name": "Ada" } },
        "form": { "status": "open" },
        "params": { "from": "2026-01-01" },
        "app": { "user": { "email": "a@b.c" } },
    })
}

#[test]
fn reads_dotted_and_bracketed_paths() {
    let s = names_scope;
    assert_eq!(ev_in("record.amount * 2", s()), json!(25));
    assert_eq!(ev_in("record.[Unit Price] * 2", s()), json!(6));
    assert_eq!(ev_in("record.customer.name", s()), json!("Ada"));
    assert_eq!(ev_in("form.status = 'open'", s()), json!(true));
    assert_eq!(ev_in("app.user.email", s()), json!("a@b.c"));
    assert_eq!(
        ev_in("[Total Due] + 1", json!({ "Total Due": 4 })),
        json!(5)
    );
}

#[test]
fn gives_null_for_missing_fields_and_an_error_for_unknown_roots() {
    assert_eq!(ev_in("record.missing", names_scope()), Value::Null);
    assert_eq!(
        ev_in("record.customer.missing.deeper", names_scope()),
        Value::Null
    );
    let e = err_in("1 + nope", names_scope());
    assert_eq!(e.message, "Unknown name 'nope'");
    assert_eq!((e.start, e.end), (4, 8));
}

#[test]
fn converts_host_values() {
    // The TypeScript test also covers Date, undefined, and bigint scope
    // values; JSON has none of those, so only the null and big-number cases
    // apply here.
    assert_eq!(ev_in("u", json!({ "u": null })), Value::Null);
    assert_eq!(ev_in("b + 1", json!({ "b": 2u64 })), json!(3));
    assert_eq!(ev_in("b + 1", json!({ "b": 2.0 })), json!(3));
}

#[test]
fn propagates_null_through_arithmetic_and_comparisons() {
    assert_eq!(ev("null + 1"), Value::Null);
    assert_eq!(ev("-null"), Value::Null);
    assert_eq!(ev("null = null"), Value::Null);
    assert_eq!(ev("null <> 1"), Value::Null);
    assert_eq!(ev("not null"), Value::Null);
}

#[test]
fn treats_null_as_empty_text_when_joining() {
    assert_eq!(ev("'a' & null & 'b'"), json!("ab"));
    assert_eq!(ev("concat('a', null, 1)"), json!("a1"));
}

#[test]
fn uses_three_valued_and_or() {
    assert_eq!(ev("null and false"), json!(false));
    assert_eq!(ev("null and true"), Value::Null);
    assert_eq!(ev("null or true"), json!(true));
    assert_eq!(ev("null or false"), Value::Null);
    assert_eq!(ev("false and (1 / 'x' = 1)"), json!(false));
}

#[test]
fn supports_is_null_in_and_between() {
    assert_eq!(ev_in("x is null", json!({ "x": null })), json!(true));
    assert_eq!(ev_in("x IS NOT NULL", json!({ "x": 0 })), json!(true));
    assert_eq!(ev("2 in (1, 2, 3)"), json!(true));
    assert_eq!(ev("5 in (1, 2)"), json!(false));
    assert_eq!(ev("5 not in (1, 2)"), json!(true));
    assert_eq!(ev("5 in (1, null)"), Value::Null);
    assert_eq!(
        ev_in("'b' in (list)", json!({ "list": ["a", "b"] })),
        json!(true)
    );
    assert_eq!(ev("5 between 1 and 5"), json!(true));
    assert_eq!(ev("6 not between 1 and 5"), json!(true));
    assert_eq!(
        ev_in("x between 1 and 5 and true", json!({ "x": 3 })),
        json!(true)
    );
    assert_eq!(ev("null between 1 and 2"), Value::Null);
}

#[test]
fn evaluate_boolean_maps_null_to_false_and_rejects_other_types() {
    let o = EvaluateOptions::default();
    assert_eq!(evaluate_boolean("null = 1", &Scope::new(), o), Ok(false));
    let s = scope(json!({ "record": { "amount": 11 } }));
    assert_eq!(evaluate_boolean("record.amount > 10", &s, o), Ok(true));
    let e = evaluate_boolean("1 + 1", &Scope::new(), o).unwrap_err();
    assert_eq!(e.message, "Expected true or false but got number");
}

#[test]
fn compares_numbers_text_booleans_and_dates() {
    assert_eq!(ev("2 >= 2"), json!(true));
    assert_eq!(ev("'abc' < 'abd'"), json!(true));
    assert_eq!(ev("'A' = 'a'"), json!(false));
    assert_eq!(ev("1 == 1 && 2 != 3"), json!(true));
    assert_eq!(ev("#2026-01-31# = '2026-01-31T00:00:00'"), json!(true));
    assert_eq!(ev("#2026-02-01# > #2026-01-31T23:59:59#"), json!(true));
    assert_eq!(ev("1 = '1'"), json!(false));
    assert_eq!(ev("!(1 = 1) || false"), json!(false));
}

#[test]
fn rejects_ordering_across_types_and_chained_comparisons() {
    assert_eq!(err("1 < 'a'").message, "Can't compare number with text");
    let e = parse_err("1 < 2 < 3");
    assert!(e.message.contains("can't be chained"));
    assert_eq!(e.start, 6);
}

#[test]
fn handles_text() {
    assert_eq!(ev("len('héllo')"), json!(5));
    assert_eq!(ev("UPPER('ab') & lower('CD')"), json!("ABcd"));
    assert_eq!(ev("trim('  x ')"), json!("x"));
    assert_eq!(ev("left('abcdef', 2)"), json!("ab"));
    assert_eq!(ev("right('abcdef', 3)"), json!("def"));
    assert_eq!(ev("mid('abcdef', 2, 3)"), json!("bcd"));
    assert_eq!(ev("substr('abcdef', 4)"), json!("def"));
    assert_eq!(ev("contains('Hello World', 'world')"), json!(true));
    assert_eq!(ev("startswith('Hello', 'he')"), json!(true));
    assert_eq!(ev("endswith('Hello', 'x')"), json!(false));
    assert_eq!(ev("replace('a-b-c', '-', '+')"), json!("a+b+c"));
    assert_eq!(ev("len(null)"), Value::Null);
}

#[test]
fn handles_conditionals_and_nulls() {
    assert_eq!(ev("if(1 > 0, 'yes', 'no')"), json!("yes"));
    assert_eq!(ev("IF(null, 'yes', 'no')"), json!("no"));
    assert_eq!(ev("if(false, 1)"), Value::Null);
    assert_eq!(ev("if(true, 1, 1 / 'x')"), json!(1));
    assert_eq!(ev("coalesce(null, null, 3)"), json!(3));
    assert_eq!(ev_in("nz(x, 0)", json!({ "x": null })), json!(0));
    assert_eq!(ev("isnull(null)"), json!(true));
    assert_eq!(ev("isblank('  ')"), json!(true));
    assert_eq!(ev("isblank('a')"), json!(false));
}

#[test]
fn handles_numbers() {
    assert_eq!(ev("round(2.345, 2)"), json!(2.35));
    assert_eq!(ev("round(1234, -2)"), json!(1200));
    assert_eq!(ev("floor(2.7)"), json!(2));
    assert_eq!(ev("ceil(2.1)"), json!(3));
    assert_eq!(ev("floor(2.789, 1)"), json!(2.7));
    assert_eq!(ev("abs(-4)"), json!(4));
    assert_eq!(ev("min(3, 1, 2)"), json!(1));
    assert_eq!(ev("max(3, null, 7)"), json!(7));
    assert_eq!(ev("max('2026-01-01', '2026-03-01')"), json!("2026-03-01"));
    assert_eq!(ev("number(' 1,234.5 ')"), json!(1234.5));
    assert_eq!(ev("number('abc')"), Value::Null);
    assert_eq!(ev("number(true)"), json!(1));
    assert_eq!(ev("text(1.5) & '!'"), json!("1.5!"));
}

#[test]
fn validates_regex_patterns_safely() {
    assert_eq!(ev("regexmatch('AB-123', '^[A-Z]{2}-\\d+$')"), json!(true));
    assert_eq!(ev("regexmatch('x', '^\\d+$')"), json!(false));
    assert!(err("regexmatch('aaaa', '(a+)+$')")
        .message
        .contains("nested repetition"));
    assert!(err("regexmatch('aa', '(a)\\1')")
        .message
        .contains("back-references"));
    assert!(err("regexmatch('a', '(')")
        .message
        .contains("invalid pattern"));
    assert_eq!(ev("regexmatch('ab', '(?:ab)+')"), json!(true));
}

fn rows() -> Value {
    json!([
        { "amount": 10, "qty": 2, "price": 1.1, "paid": true },
        { "amount": 2.5, "qty": 1, "price": 0.2, "paid": false },
        { "amount": null, "qty": 3, "price": 0.1, "paid": true },
    ])
}

#[test]
fn aggregates_projected_fields() {
    let s = || json!({ "rows": rows() });
    assert_eq!(ev_in("sum(rows.amount)", s()), json!(12.5));
    assert_eq!(ev_in("avg(rows.amount)", s()), json!(6.25));
    assert_eq!(ev_in("count(rows.amount)", s()), json!(2));
    assert_eq!(ev_in("count(rows)", s()), json!(3));
    assert_eq!(ev_in("max(rows.qty)", s()), json!(3));
    assert_eq!(ev("sum(1, 2, 3)"), json!(6));
    assert_eq!(ev_in("sum(empty.amount)", json!({ "empty": [] })), json!(0));
    assert_eq!(
        ev_in("avg(empty.amount)", json!({ "empty": [] })),
        Value::Null
    );
}

#[test]
fn evaluates_an_expression_per_item() {
    let s = || json!({ "rows": rows(), "rate": 2 });
    assert_eq!(ev_in("sumof(rows, qty * price)", s()), json!(2.7));
    assert_eq!(ev_in("countof(rows, paid)", s()), json!(2));
    assert_eq!(ev_in("maxof(rows, item.qty * 10)", s()), json!(30));
    assert_eq!(ev_in("sumof(rows, qty * rate)", s()), json!(12));
    assert_eq!(
        err("sumof(5, 1)").message,
        "sumof: expected a list but got number"
    );
}

fn at(src: &str) -> Value {
    let now = Utc.with_ymd_and_hms(2026, 10, 3, 14, 30, 15).unwrap();
    evaluate(src, &Scope::new(), EvaluateOptions::at(now)).unwrap()
}

#[test]
fn uses_the_injected_clock_in_utc() {
    assert_eq!(at("today()"), json!("2026-10-03"));
    assert_eq!(at("now()"), json!("2026-10-03T14:30:15"));
    assert_eq!(at("year(today()) * 100 + month(today())"), json!(202610));
}

#[test]
fn builds_and_splits_dates() {
    assert_eq!(ev("date(2026, 2, 28)"), json!("2026-02-28"));
    assert_eq!(ev("date(2026, 13, 1)"), json!("2027-01-01"));
    assert_eq!(ev("day(#2026-01-31#)"), json!(31));
    assert_eq!(ev("hour('2026-01-31T10:20:00Z')"), json!(10));
    assert_eq!(ev("weekday(#2026-10-03#)"), json!(7));
    assert_eq!(
        err("year('soon')").message,
        "year: expected a date but got \"soon\""
    );
}

#[test]
fn adds_and_diffs_dates() {
    assert_eq!(ev("dateadd('day', 1, #2026-12-31#)"), json!("2027-01-01"));
    assert_eq!(ev("dateadd('month', 1, #2026-01-31#)"), json!("2026-02-28"));
    assert_eq!(ev("dateadd('year', 1, #2024-02-29#)"), json!("2025-02-28"));
    assert_eq!(
        ev("dateadd('hour', 2, #2026-01-01#)"),
        json!("2026-01-01T02:00:00")
    );
    assert_eq!(
        ev("dateadd('m', -2, #2026-01-15T08:00:00#)"),
        json!("2025-11-15T08:00:00")
    );
    assert_eq!(ev("datediff('day', #2026-01-01#, #2026-03-01#)"), json!(59));
    assert_eq!(
        ev("datediff('month', #2026-01-31#, #2026-02-01#)"),
        json!(1)
    );
    assert_eq!(ev("datediff('year', #2025-12-31#, #2026-01-01#)"), json!(1));
    assert_eq!(ev("datediff('week', #2026-01-01#, #2026-01-20#)"), json!(2));
    assert_eq!(
        ev("datediff('minute', #2026-01-01T10:00:00#, #2026-01-01T11:30:00#)"),
        json!(90)
    );
    assert_eq!(ev("datediff('day', null, #2026-01-01#)"), Value::Null);
}

#[test]
fn rejects_impossible_date_literals() {
    let e = parse_err("x > #2026-02-30#");
    assert!(e.message.contains("Invalid date '2026-02-30'"));
    assert_eq!((e.start, e.end), (4, 16));
}

#[test]
fn formats_numbers() {
    assert_eq!(ev("format(1234.5, '#,##0.00')"), json!("1,234.50"));
    assert_eq!(ev("format(1234.5, '0.00')"), json!("1234.50"));
    assert_eq!(ev("format(0.256, '0%')"), json!("26%"));
    assert_eq!(ev("format(0.2567, '0.0%')"), json!("25.7%"));
    assert_eq!(
        ev("format(-1234567.891, '$#,##0.00')"),
        json!("-$1,234,567.89")
    );
    assert_eq!(ev("format(3.1, '0.##')"), json!("3.1"));
    assert_eq!(ev("format(7, '000')"), json!("007"));
    assert_eq!(ev("format(1.005, '0.00')"), json!("1.01"));
    assert_eq!(ev("format(null, '0.00')"), Value::Null);
}

#[test]
fn formats_dates() {
    assert_eq!(
        ev("format(#2026-01-05#, 'yyyy-MM-dd')"),
        json!("2026-01-05")
    );
    assert_eq!(
        ev("format(#2026-01-05#, 'dd/MM/yyyy')"),
        json!("05/01/2026")
    );
    assert_eq!(
        ev("format(#2026-01-05#, 'MMM d, yyyy')"),
        json!("Jan 5, 2026")
    );
    assert_eq!(
        ev("format(#2026-01-05T15:07:00#, 'dddd h:mm tt')"),
        json!("Monday 3:07 PM")
    );
    assert_eq!(ev("text(#2026-01-05#, 'MMMM yyyy')"), json!("January 2026"));
}

#[test]
fn format_value_formats_host_values() {
    assert_eq!(
        format_value(&json!(1234.567), Some("#,##0.0")).as_deref(),
        Ok("1,234.6")
    );
    assert_eq!(format_value(&Value::Null, Some("0.00")).as_deref(), Ok(""));
    assert_eq!(format_value(&json!(0.1 + 0.2), None).as_deref(), Ok("0.3"));
    assert_eq!(
        format_value(&json!("2026-03-04"), Some("d MMM yyyy")).as_deref(),
        Ok("4 Mar 2026")
    );
    assert!(format_value(&json!(true), Some("0.00")).is_err());
}

#[test]
fn reports_syntax_errors_with_positions() {
    let cases = [
        ("1 +", "Unexpected end of expression", 3, 3),
        ("(1 + 2", "Expected ')' but the expression ended", 6, 6),
        ("'abc", "Unterminated string", 0, 4),
        ("1 @ 2", "Unexpected character '@'", 2, 3),
        ("x is 3", "Expected 'null' after 'is'", 5, 6),
        ("", "Expression is empty", 0, 0),
        ("a b", "Unexpected 'b'", 2, 3),
    ];
    for (src, message, start, end) in cases {
        let e = parse_err(src);
        assert_eq!(
            (e.message.as_str(), e.start, e.end),
            (message, start, end),
            "{src}"
        );
    }
}

#[test]
fn reports_type_errors_with_positions() {
    let e = err("1 + ('a' + 2)");
    assert_eq!(e.message, "Can't add text and number; use & to join text");
    assert_eq!((e.start, e.end), (4, 13));
    let f = err("round('x')");
    assert_eq!(f.message, "round: expected a number but got text");
    assert_eq!(f.start, 6);
    assert_eq!(err("frob(1)").end, 4);
}

#[test]
fn limits_nesting_depth() {
    let src = format!("{}1{}", "(".repeat(200), ")".repeat(200));
    assert!(parse_err(&src).message.contains("nested too deeply"));
}

#[test]
fn blocks_prototype_names_at_parse_time() {
    assert_eq!(
        parse_err("constructor").message,
        "The name 'constructor' is not allowed"
    );
    assert_eq!(
        parse_err("record.__proto__").message,
        "The name '__proto__' is not allowed"
    );
    assert!(parse_err("record.[constructor].name")
        .message
        .contains("not allowed"));
    assert!(parse_err("x.prototype").message.contains("not allowed"));
}

#[test]
fn never_reads_inherited_properties_or_calls_host_functions() {
    // JSON objects have no prototype chain; the rest of the TypeScript test
    // carries over unchanged.
    let record = json!({ "own": 1 });
    assert_eq!(
        ev_in("record.secret", json!({ "record": record })),
        Value::Null
    );
    assert_eq!(
        ev_in("record.toString", json!({ "record": { "own": 1 } })),
        Value::Null
    );
    assert_eq!(
        ev_in("record.own", json!({ "record": { "own": 1 } })),
        json!(1)
    );
    assert_eq!(err("toString").message, "Unknown name 'toString'");
    assert_eq!(err("valueOf()").message, "Unknown function 'valueof'");
    assert!(err("hasOwnProperty('x')")
        .message
        .contains("Unknown function"));
    assert_eq!(
        err_in("x.length", json!({ "x": "abc" })).message,
        "Can't read 'length' from text"
    );
}

#[test]
fn check_returns_no_diagnostics_for_a_valid_expression() {
    let known = Some(&["record"][..]);
    assert!(check("record.amount > 0 and contains(record.name, 'x')", known).is_empty());
}

#[test]
fn check_reports_syntax_function_and_arity_errors() {
    assert_eq!(
        check("1 +", None),
        vec![Diagnostic {
            severity: Severity::Error,
            message: "Unexpected end of expression".into(),
            start: 3,
            end: 3,
        }]
    );
    assert_eq!(
        messages("foo(1) + left()", None),
        [
            "Unknown function 'foo'",
            "left() takes 1 to 2 arguments but got 0"
        ]
    );
    assert_eq!(
        messages("today(1)", None)[0],
        "today() takes 0 arguments but got 1"
    );
    assert_eq!(
        messages("if(a)", None)[0],
        "if() takes 2 to 3 arguments but got 1"
    );
}

#[test]
fn check_checks_names_against_known_names() {
    let known = Some(&["record", "params.from"][..]);
    assert!(check("record.x + params.from", known).is_empty());
    assert_eq!(messages("params.to", known)[0], "Unknown field 'params.to'");
    assert_eq!(
        check("1 + recrod.amount", known)[0],
        Diagnostic {
            severity: Severity::Error,
            message: "Unknown name 'recrod'".into(),
            start: 4,
            end: 17,
        }
    );
    assert!(check("sumof(record.lines, qty * price)", known).is_empty());
    assert!(check("anything", None).is_empty());
}

#[test]
fn check_validates_literal_units_patterns_and_constant_expressions() {
    assert_eq!(
        messages("dateadd('fortnight', 1, today())", None)[0],
        "Unknown date unit 'fortnight'"
    );
    assert!(messages("regexmatch(x, '(a*)*')", None)[0].contains("nested repetition"));
    assert!(messages("1 + 'a'", None)[0].contains("Can't add number and text"));
}

#[test]
fn referenced_names_lists_dotted_names_once_in_order() {
    assert_eq!(
        referenced_names("record.a + record.[B c] + record.a * params.x"),
        ["record.a", "record.B c", "params.x"]
    );
    assert_eq!(
        referenced_names("sumof(rows, qty * price) + tax"),
        ["rows", "tax"]
    );
    assert!(referenced_names("1 + 2").is_empty());
}

#[test]
fn caches_parsed_expressions() {
    let a = parse_cached("1 + 1").unwrap();
    let b = parse_cached("1 + 1").unwrap();
    assert!(Arc::ptr_eq(&a, &b));
}

// Rows of `it.each` in access-expressions.test.ts.
#[test]
fn translated_access_expressions() {
    let record = json!({
        "Amount": 42, "Due": "2024-08-15", "Status": "Open order",
        "Find": "order", "Priority": 2,
    });
    let cases = [
        ("contains(record.Status, record.Find)", json!(true)),
        ("startswith(record.Status, record.Find)", json!(false)),
        ("endswith(record.Status, record.Find)", json!(true)),
        ("upper(record.Status)", json!("OPEN ORDER")),
        (
            "format(date(2000, month(record.Due), 1), 'MMMM')",
            json!("August"),
        ),
        ("format(date(2000, 3, 1), 'MMM')", json!("Mar")),
        ("floor((month(record.Due) - 1) / 3) + 1", json!(3)),
        ("year(record.Due)", json!(2024)),
        (
            "if(record.Amount > 100, 'High', if(record.Amount > 10, 'Mid', if(true, 'Low', null)))",
            json!("Mid"),
        ),
        (
            "if(record.Priority = 1, 'Low', if(record.Priority = 2, 'Normal', if(record.Priority = 3, 'High', null)))",
            json!("Normal"),
        ),
        (
            "if(record.Amount > 0, 1, if(record.Amount < 0, -1, 0))",
            json!(1),
        ),
    ];
    for (src, expected) in cases {
        assert_eq!(ev_in(src, json!({ "record": record })), expected, "{src}");
    }
}

// Rust-specific coverage below.

#[test]
fn prints_numbers_like_javascript() {
    use values::js_number_to_string as js;
    assert_eq!(js(0.0), "0");
    assert_eq!(js(100.0), "100");
    assert_eq!(js(1.5), "1.5");
    assert_eq!(js(-0.000001), "-0.000001");
    assert_eq!(js(1e-7), "1e-7");
    assert_eq!(js(1.5e-7), "1.5e-7");
    assert_eq!(js(1e21), "1e+21");
    assert_eq!(js(1.25e22), "1.25e+22");
    assert_eq!(js(123456789012345680000.0), "123456789012345680000");
    assert_eq!(ev("text(1000000 * 1000000)"), json!("1000000000000"));
    assert_eq!(ev("text(0.1 + 0.2)"), json!("0.3"));
}

#[test]
fn reports_positions_in_utf16_units() {
    // 😀 is two UTF-16 units but one char
    let e = parse_err("'😀' + @");
    assert_eq!((e.start, e.end), (7, 8));
    let e = err("'é' + nope");
    assert_eq!((e.start, e.end), (6, 10));
}

#[test]
fn orders_text_by_utf16_code_unit() {
    // U+FF5E (BMP) sorts after U+1F600 (surrogate pair) in UTF-16 order
    assert_eq!(ev("'\u{ff5e}' > '😀'"), json!(true));
}

#[test]
fn lists_function_names_sorted() {
    let names = function_names();
    assert!(names.contains(&"sumof") && names.contains(&"iif") && names.contains(&"ceiling"));
    assert!(names.windows(2).all(|w| w[0] < w[1]));
}

#[test]
fn matches_javascript_regex_syntax() {
    let m = |p: &str, t: &str| regex::compile_safe_regex(p).unwrap().is_match(t).unwrap();
    assert!(m("^a.c$", "abc"));
    assert!(!m("^a.c$", "a\nc"));
    assert!(m("colou?r", "my color"));
    assert!(m("^(?:ab|cd){2}$", "abcd"));
    assert!(!m("^(?:ab|cd){2}$", "abc"));
    assert!(m("a{2,3}?b", "aab"));
    assert!(m("\\bcat\\b", "the cat sat"));
    assert!(!m("\\bcat\\b", "concatenate"));
    assert!(m("[^a-c]+", "abcd"));
    assert!(m("foo(?=bar)", "foobar"));
    assert!(!m("foo(?!bar)", "foobar"));
    assert!(m("(?<=\\$)\\d+", "cost $15"));
    assert!(m("(?<year>\\d{4})-\\d{2}", "2026-05"));
    assert!(m("\\u0041\\x42\\u{43}", "ABC"));
    assert!(m("[\\w.-]+@[\\w-]+\\.\\w+", "a.b@c-d.com"));
    assert!(m("^$", ""));
    assert!(regex::compile_safe_regex("a**").is_err());
    assert!(regex::compile_safe_regex("[b-a]").is_err());
    assert!(regex::compile_safe_regex("a)").is_err());
    assert!(regex::compile_safe_regex(&"a".repeat(201))
        .unwrap_err()
        .contains("longer than 200"));
}

#[test]
fn regexmatch_limits_input_and_work() {
    let long = "a".repeat(10_001);
    let e = err_in("regexmatch(t, 'a')", json!({ "t": long }));
    assert!(e.message.contains("longer than 10000"));
    // a repeated group nests per iteration; short inputs finish, long ones
    // are refused instead of overflowing the stack
    let run = |n: usize| {
        let t = json!({ "t": "ab".repeat(n) });
        evaluate(
            "regexmatch(t, '^(?:a|b)*$')",
            &scope(t),
            EvaluateOptions::default(),
        )
    };
    assert_eq!(run(100), Ok(json!(true)));
    assert!(run(900).unwrap_err().message.contains("too complex"));
    // a repeated single character has no such limit
    let t = json!({ "t": "a".repeat(9000) });
    assert_eq!(ev_in("regexmatch(t, '^a*$')", t), json!(true));
}

#[test]
fn rejects_runaway_nesting_when_evaluating() {
    let src = vec!["1"; 3000].join("+");
    let e = err(&src);
    assert_eq!(e.message, "Expression is nested too deeply");
}

#[test]
fn evaluates_long_operator_chains_within_the_depth_limit() {
    let src = vec!["1"; 150].join("+");
    assert_eq!(ev(&src), json!(150));
}

#[test]
fn matches_unicode_property_escapes() {
    let m = |p: &str, t: &str| regex::compile_safe_regex(p).unwrap().is_match(t).unwrap();
    assert!(m("^\\p{L}+$", "héllo"));
    assert!(!m("^\\p{L}+$", "he llo"));
    assert!(m("\\p{Lu}", "abC"));
    assert!(m("^[\\p{N}x]+$", "12x"));
    assert!(m("\\P{L}", "a1"));
    assert!(regex::compile_safe_regex("\\p{Nope}").is_err());
}

#[test]
fn evaluates_nested_calls_within_the_depth_limit() {
    let src = format!("{}1{}", "abs(".repeat(60), ")".repeat(60));
    assert_eq!(ev(&src), json!(1));
}
