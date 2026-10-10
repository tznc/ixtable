//! Generated forms, as Access' AutoForm makes them: a detail form for a table
//! whose list form opens no form of its own, and list plus detail forms for
//! every table when the file's forms cannot be read (binary `.accdb`/`.mdb`).
use super::controls::kind_for;
use super::forms::Context;
use super::report::{ImportReport, Status};
use super::schema::{TablePlan, TableRole};
use super::sources::Lookup;
use serde_json::{json, Value};

fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// The id of a generated detail form for an ixtable table.
pub fn detail_for(ctx: &mut Context, table: &str) -> String {
    ctx.auto_details
        .entry(table.to_string())
        .or_insert_with(id)
        .clone()
}

/// Related lists for child tables pointing at `plan`.
fn related_lists(ctx: &Context, plan: &TablePlan) -> Vec<Value> {
    let mut out = vec![];
    for child in ctx.plans {
        for fk in child
            .foreign_keys
            .iter()
            .filter(|fk| fk.target_table == plan.name && fk.columns.len() == 1)
        {
            let columns: Vec<String> = child
                .columns
                .iter()
                .filter(|c| {
                    !fk.columns.contains(&c.name)
                        && c.declared != "BLOB"
                        && !child.primary_key.contains(&c.name)
                })
                .take(5)
                .map(|c| c.name.clone())
                .collect();
            let label = match &child.role {
                TableRole::Attachments(_, c) | TableRole::Values(_, c) => c.clone(),
                TableRole::Main => child.name.clone(),
            };
            out.push(json!({
                "id": id(),
                "kind": "relatedList",
                "label": label,
                "related": { "table": child.name, "foreignKey": fk.columns[0], "parentColumn": fk.target_columns[0], "columns": columns },
            }));
        }
    }
    out
}

fn detail_form(ctx: &mut Context, plan: &TablePlan, form_id: &str) -> Value {
    let access = ctx.db.table(&plan.access).cloned();
    let mut controls = vec![];
    let (mut row, mut column) = (1, 1);
    for c in &plan.columns {
        if c.declared == "BLOB"
            || (plan.single_key() == Some(c.name.as_str()) && c.declared == "INTEGER")
        {
            continue;
        }
        let Some(a) = access.as_ref().and_then(|t| t.column(&c.name)).cloned() else {
            continue;
        };
        let kind = kind_for(c, &a);
        let wide = kind == "multiline";
        if wide && column != 1 {
            row += 1;
            column = 1;
        }
        let span = if wide { 12 } else { 6 };
        let mut v = json!({
            "id": id(),
            "kind": kind,
            "label": a.prop("Caption").unwrap_or(&a.name),
            "binding": { "column": c.name },
            "placement": { "column": column, "row": row, "columnSpan": span },
        });
        if c.not_null {
            v["validation"] = json!({ "required": true });
        }
        match ctx.column_lookup(&a) {
            Some(Lookup::Table {
                table,
                value,
                display,
            }) => {
                v["kind"] = json!("relationship");
                v["relationship"] =
                    json!({ "table": table, "valueColumn": value, "displayColumn": display });
            }
            Some(Lookup::Options(opts)) => {
                v["kind"] = json!("select");
                v["options"] = json!(opts
                    .iter()
                    .map(|(val, l)| json!({ "value": val, "label": l }))
                    .collect::<Vec<_>>());
            }
            None => {
                // A foreign key without a lookup still shows its related row.
                if let Some(fk) = plan
                    .foreign_keys
                    .iter()
                    .find(|fk| fk.columns == [c.name.clone()])
                {
                    let target = ctx.plans.iter().find(|p| p.name == fk.target_table);
                    let display = target
                        .and_then(|t| ctx.db.table(&t.access))
                        .and_then(|t| super::sources::best_display(t, &fk.target_columns[0]))
                        .unwrap_or_else(|| fk.target_columns[0].clone());
                    v["kind"] = json!("relationship");
                    v["relationship"] = json!({ "table": fk.target_table, "valueColumn": fk.target_columns[0], "displayColumn": display });
                }
            }
        }
        controls.push(v);
        if wide || column == 7 {
            row += 1;
            column = 1;
        } else {
            column = 7;
        }
    }
    if column != 1 {
        row += 1;
    }
    for mut r in related_lists(ctx, plan) {
        r["placement"] = json!({ "column": 1, "row": row, "columnSpan": 12 });
        row += 1;
        controls.push(r);
    }
    json!({
        "id": form_id,
        "name": format!("{} details", plan.name),
        "source": { "kind": "table", "table": plan.name },
        "modes": ["detail", "create", "edit"],
        "controls": controls,
    })
}

/// Forms generated after the Access forms converted.
pub fn generate(ctx: &mut Context, report: &mut ImportReport) -> Vec<Value> {
    let mut out = vec![];
    let plans: Vec<TablePlan> = ctx.plans.to_vec();
    if ctx.db.forms.is_empty() {
        // No readable forms: a list and a detail form per table.
        for p in plans.iter().filter(|p| p.role == TableRole::Main) {
            let detail = detail_for(ctx, &p.name);
            let columns: Vec<&str> = p
                .columns
                .iter()
                .filter(|c| c.declared != "BLOB")
                .take(6)
                .map(|c| c.name.as_str())
                .collect();
            out.push(json!({
                "id": id(),
                "name": p.name,
                "source": { "kind": "table", "table": p.name },
                "modes": ["list"],
                "listColumns": columns,
                "detailFormId": detail,
            }));
        }
    }
    let wanted: Vec<(String, String)> = ctx
        .auto_details
        .iter()
        .map(|(t, i)| (t.clone(), i.clone()))
        .collect();
    for (table, form_id) in wanted {
        let Some(plan) = plans.iter().find(|p| p.name == table) else {
            continue;
        };
        out.push(detail_form(ctx, plan, &form_id));
        if !ctx.db.forms.is_empty() {
            report.add(
                "form",
                &format!("{table} details"),
                Status::Converted,
                vec!["generated: no Access form edits single rows of this table".into()],
            );
        }
    }
    out
}
