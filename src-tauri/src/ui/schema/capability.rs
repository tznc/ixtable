//! Read-only summary of the capability categories the type and DDL tables do not cover
//! (port of `src/schema/CapabilitySummary.tsx`, PRD 9.4), plus the type-mapping and
//! schema-change tables the Datasource tab shows under it.
//!
//! Test ids: `cap:<Name>` (label = detail) for each summary row, `error-code:<n>`
//! (label = "code (constraint) | native | meaning"), `type-map:<logical type>` (label =
//! "logical | stored as | enforcement") and `ddl:<operation>` (label = "operation | mode |
//! notes"). `capability_rows` is the pure part under test.

use super::logical::mode_label;
use crate::recordstore::StoreCapabilities;
use crate::ui::{prelude::*, widgets};

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// (name, detail) rows of the summary table.
pub fn capability_rows(c: &StoreCapabilities) -> Vec<(String, String)> {
    let join = |parts: Vec<String>| {
        parts
            .into_iter()
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(" \u{b7} ")
    };
    vec![
        ("Constraints".into(), c.constraints.join(", ")),
        ("Relationship actions".into(), c.foreign_key_actions.join(", ")),
        (
            "Indexes".into(),
            format!(
                "unique {} \u{b7} multi-column {} \u{b7} partial {} \u{b7} expression {}",
                yes_no(c.indexes.unique),
                yes_no(c.indexes.multi_column),
                yes_no(c.indexes.partial),
                yes_no(c.indexes.expression)
            ),
        ),
        (
            "Transactions".into(),
            format!(
                "atomic batches {} \u{b7} transactional schema changes {} \u{b7} savepoints {} \u{b7} isolation {}",
                yes_no(c.transactions.atomic_batches),
                yes_no(c.transactions.transactional_ddl),
                yes_no(c.transactions.savepoints),
                c.transactions.isolation
            ),
        ),
        ("Parameter style".into(), c.parameter_style.clone()),
        ("Generated values".into(), c.generated_values.join(", ")),
        (
            "Migrations".into(),
            join(vec![
                format!("transactional {}", yes_no(c.migrations.transactional_ddl)),
                format!("dry run: {}", c.migrations.dry_run),
                format!(
                    "health checks: {}",
                    if c.migrations.health_checks.is_empty() {
                        "none".to_string()
                    } else {
                        c.migrations.health_checks.join(", ")
                    }
                ),
                c.migrations.notes.clone(),
            ]),
        ),
        (
            "Concurrency".into(),
            join(vec![
                format!("policies: {}", c.concurrency.policies.join(", ")),
                format!("row locking {}", yes_no(c.concurrency.row_locking)),
                format!("multi user {}", yes_no(c.concurrency.multi_user)),
                c.concurrency.notes.clone(),
            ]),
        ),
    ]
}

// One table row: the cells side by side, readable in tests as a single joined label.
fn row(id: String, cells: &[String], cx: &App) -> AnyElement {
    let border = cx.theme().border;
    let label = cells.join(" | ");
    h_flex()
        .gap_3()
        .py_1()
        .border_b_1()
        .border_color(border)
        .text_sm()
        .child(widgets::text(SharedString::from(id), label))
        .into_any_element()
}

fn table(title: &str, rows: Vec<AnyElement>, cx: &App) -> Div {
    v_flex()
        .gap_1()
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().muted_foreground)
                .child(title.to_string()),
        )
        .children(rows)
}

/// The capability summary and error-code tables.
pub fn capability_summary(c: &StoreCapabilities, cx: &App) -> impl IntoElement {
    let rows = capability_rows(c)
        .into_iter()
        .map(|(name, detail)| row(format!("cap:{name}"), &[name, detail], cx))
        .collect();
    let errors = c
        .error_codes
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let code = match &e.constraint {
                Some(k) => format!("{} ({k})", e.code),
                None => e.code.clone(),
            };
            row(format!("error-code:{i}"), &[code, e.native.clone(), e.description.clone()], cx)
        })
        .collect();
    v_flex()
        .gap_3()
        .child(table("Store capabilities", rows, cx))
        .child(table("Error codes", errors, cx))
}

/// The logical type mapping and schema change mode tables.
pub fn mapping_tables(c: &StoreCapabilities, cx: &App) -> impl IntoElement {
    let types = c
        .logical_types
        .iter()
        .map(|t| {
            let enforcement = match t.max_precision {
                Some(p) => format!("{} \u{b7} up to {p} digits", t.enforcement),
                None => t.enforcement.clone(),
            };
            row(
                format!("type-map:{}", t.logical_type),
                &[t.logical_type.clone(), t.physical_type.clone(), enforcement],
                cx,
            )
        })
        .collect();
    let ddl = c
        .ddl
        .iter()
        .map(|d| {
            row(
                format!("ddl:{}", d.operation),
                &[
                    d.operation.replace('_', " "),
                    mode_label(&d.mode).to_string(),
                    d.notes.clone(),
                ],
                cx,
            )
        })
        .collect();
    v_flex()
        .gap_3()
        .child(table("Logical type mapping", types, cx))
        .child(table("Schema change modes", ddl, cx))
}
