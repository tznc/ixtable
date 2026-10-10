//! Access form controls → ixtable controls.
use super::forms::{children, clean_caption, is_hidden, sections, source_object, Context};
use super::layout::{grid_layout, place, Rect};
use super::macros::{read_macro, StepWriter};
use super::report::Notes;
use super::schema::{ColumnPlan, Conv, TablePlan};
use super::sources::{Bound, Field, Lookup, Source};
use crate::access::model::{ColType, Column, Complex};
use crate::access::text_format::Node;
use crate::access::translate::ast::parse_field_rule;
use crate::access::translate::expr::{field, translate, ExprWriter, Target};
use crate::access::translate::format::format_pattern;
use serde_json::{json, Value};

/// What the first pass learned about a form.
#[derive(Debug, Clone)]
pub struct FormInfo {
    pub id: String,
    pub name: String,
    /// Access `DefaultView`: 0 single, 1 continuous, 2 datasheet, 5 split.
    pub view: i64,
    pub bound: Bound,
    pub modes: Vec<&'static str>,
    pub list_columns: Vec<String>,
    pub has_subforms: bool,
    /// Forms its controls open (detail form candidates).
    pub detail_targets: Vec<String>,
}

fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub fn rect(n: &Node) -> Rect {
    Rect {
        left: n.int("Left").unwrap_or(0),
        top: n.int("Top").unwrap_or(0),
        width: n.int("Width").unwrap_or(1440).max(1),
        height: n.int("Height").unwrap_or(300).max(1),
    }
}

/// The label attached to a control (nested in its block).
pub fn attached_label(n: &Node) -> Option<&Node> {
    children(n).into_iter().find(|c| c.kind == "Label")
}

fn locked(n: &Node) -> bool {
    matches!(n.get("Locked"), Some("NotDefault" | "-1" | "1" | "True"))
        || matches!(n.get("Enabled"), Some("NotDefault" | "0" | "False"))
}

/// A converted control with its rectangle (for grid placement) and children.
struct Placed {
    control: Value,
    rect: Rect,
    /// Controls inside a tab control, already placed in its own grid.
    inner: Vec<Value>,
}

pub struct ControlWriter<'c, 'a> {
    pub ctx: &'c mut Context<'a>,
    pub info: &'c FormInfo,
    pub notes: &'c mut Notes,
    pub actions: Vec<Value>,
    /// Name → ixtable expression for fields and controls of the form.
    names: Vec<(String, String)>,
}

impl<'c, 'a> ControlWriter<'c, 'a> {
    pub fn new(ctx: &'c mut Context<'a>, info: &'c FormInfo, notes: &'c mut Notes) -> Self {
        let mut names = vec![];
        for (n, f) in &info.bound.fields {
            match f {
                Field::Column(c) => names.push((n.clone(), field("record", c))),
                Field::Computed(e) => names.push((n.clone(), format!("({e})"))),
            }
        }
        Self {
            ctx,
            info,
            notes,
            actions: vec![],
            names,
        }
    }

    pub fn resolve(&self, name: &str) -> Option<String> {
        self.names
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, e)| e.clone())
    }

    fn expr(&self, text: &str) -> Result<String, String> {
        let names = &self.names;
        let resolve = |n: &str| {
            names
                .iter()
                .find(|(x, _)| x.eq_ignore_ascii_case(n))
                .map(|(_, e)| e.clone())
        };
        translate(text, Target::Form, &resolve)
    }

    /// The plan and Access column behind a bound name (table sources only).
    fn column(&self, name: &str) -> Option<(TablePlan, Column)> {
        let Some(Field::Column(col)) = self.info.bound.field(name) else {
            return None;
        };
        let table = self.info.bound.table.as_ref()?;
        let plan = self
            .ctx
            .plans
            .iter()
            .find(|p| p.access.eq_ignore_ascii_case(table))?
            .clone();
        let access = self.ctx.db.table(table)?.column(&col)?.clone();
        Some((plan, access))
    }

    /// Learns control names first so expressions may refer to other controls.
    fn learn_names(&mut self, block: &Node) {
        let mut pending = vec![];
        for s in sections(block) {
            super::forms::visit(s, &mut |c| {
                if let (Some(name), Some(cs)) = (c.get("Name"), c.get("ControlSource")) {
                    pending.push((name.to_string(), cs.to_string()));
                }
            });
        }
        for _ in 0..2 {
            for (name, cs) in &pending {
                if self.resolve(name).is_some() {
                    continue;
                }
                let e = match cs.strip_prefix('=') {
                    Some(expr) => self.expr(expr).map(|e| format!("({e})")).ok(),
                    None => self.resolve(cs),
                };
                if let Some(e) = e {
                    self.names.push((name.clone(), e));
                }
            }
        }
    }

    /// All controls of the form, placed on the grid section by section.
    pub fn form_controls(&mut self, block: &Node) -> Vec<Value> {
        self.learn_names(block);
        let width = block.int("Width").unwrap_or(10_000).max(1440);
        let mut out = vec![];
        let mut row = 1;
        let order = ["FormHeader", "Section", "FormFooter"];
        for kind in order {
            for s in sections(block).into_iter().filter(|s| s.kind == kind) {
                if is_hidden(s) || s.int("Height") == Some(0) {
                    continue;
                }
                let items: Vec<Placed> = children(s)
                    .into_iter()
                    .filter_map(|c| self.control(c, kind == "FormHeader"))
                    .collect();
                let rects: Vec<Rect> = items.iter().map(|p| p.rect).collect();
                let (placements, next) = place(&rects, 0, width, row);
                row = next;
                for (mut p, pl) in items.into_iter().zip(placements) {
                    p.control["placement"] = pl.json();
                    out.push(p.control);
                    out.extend(p.inner);
                }
            }
        }
        for s in sections(block)
            .into_iter()
            .filter(|s| s.kind == "PageHeader" || s.kind == "PageFooter")
        {
            if !children(s).is_empty() {
                self.notes.push(
                    "page header and footer sections only print in Access; they were left out",
                );
            }
        }
        out
    }

    /// Table validation rule → form rule.
    pub fn form_rules(&mut self) -> Vec<Value> {
        let Some(table) = self
            .info
            .bound
            .table
            .as_ref()
            .and_then(|t| self.ctx.db.table(t))
        else {
            return vec![];
        };
        let Some(rule) = table.prop("ValidationRule") else {
            return vec![];
        };
        match self.expr(rule) {
            Ok(e) => vec![
                json!({ "id": id(), "expression": e, "message": table.prop("ValidationText").unwrap_or("The record breaks the table's validation rule.") }),
            ],
            Err(err) => {
                self.notes.push(format!(
                    "the table validation rule {rule} was not converted ({err})"
                ));
                vec![]
            }
        }
    }

    fn label_of(&self, c: &Node, column: Option<&Column>) -> String {
        if let Some(l) = attached_label(c).and_then(|l| l.get("Caption")) {
            return clean_caption(l);
        }
        if let Some(cap) = column.and_then(|c| c.prop("Caption")) {
            return cap.to_string();
        }
        column
            .map(|c| c.name.clone())
            .or_else(|| c.get("ControlSource").map(str::to_string))
            .or_else(|| c.get("Name").map(str::to_string))
            .unwrap_or_default()
    }

    /// The control's own rectangle joined with its attached label's.
    fn full_rect(c: &Node) -> Rect {
        match attached_label(c) {
            Some(l) => rect(c).union(&rect(l)),
            None => rect(c),
        }
    }

    fn control(&mut self, c: &Node, header: bool) -> Option<Placed> {
        if is_hidden(c) {
            return None;
        }
        let name = c.get("Name").unwrap_or("").to_string();
        let r = Self::full_rect(c);
        let v = match c.kind.as_str() {
            "Label" => {
                if name.starts_with("Auto_Title") || name.starts_with("Auto_Header") {
                    return None;
                }
                let text = clean_caption(c.get("Caption")?);
                if text.is_empty() {
                    return None;
                }
                json!({ "id": id(), "kind": "label", "label": "", "text": text })
            }
            "TextBox" | "ComboBox" | "ListBox" | "CheckBox" | "ToggleButton" | "OptionButton"
            | "OptionGroup" | "Attachment" => self.data_control(c)?,
            "CommandButton" => self.button(c, header)?,
            "Subform" => self.subform(c)?,
            "Tab" => return self.tabs(c),
            "Image" => self.image(c)?,
            "Rectangle" | "Line" | "EmptyCell" | "PageBreak" | "Edge" => return None,
            "NavigationControl" | "NavigationButton" => {
                self.notes
                    .push("the navigation control became app navigation");
                return None;
            }
            other => {
                self.notes
                    .push(format!("{other} controls have no ixtable equivalent"));
                return None;
            }
        };
        Some(Placed {
            control: v,
            rect: r,
            inner: vec![],
        })
    }

    fn data_control(&mut self, c: &Node) -> Option<Value> {
        let cs = c.get("ControlSource").unwrap_or("").trim().to_string();
        let readonly_source = matches!(self.info.bound.source, Source::Query { .. });
        if cs.is_empty() {
            if c.kind != "TextBox" {
                self.notes
                    .push("unbound boxes (search, go-to and filter boxes) were left out");
            }
            return None;
        }
        if let Some(expr) = cs.strip_prefix('=') {
            return match self.expr(expr) {
                Ok(e) => {
                    let mut v = json!({ "id": id(), "kind": "computed", "label": self.label_of(c, None), "computed": e });
                    if let Some(f) = c.get("Format").and_then(format_pattern) {
                        v["format"] = json!(f);
                    }
                    Some(v)
                }
                Err(err) => {
                    self.notes.push(format!(
                        "the calculated control {} ({expr}) was not converted: {err}",
                        c.get("Name").unwrap_or("")
                    ));
                    None
                }
            };
        }
        let column = self.column(&cs);
        let label = self.label_of(c, column.as_ref().map(|(_, a)| a));
        let mut v = json!({ "id": id(), "kind": "text", "label": label });
        match self.info.bound.field(&cs) {
            Some(Field::Computed(e)) => {
                v["kind"] = json!("computed");
                v["computed"] = json!(e);
                return Some(v);
            }
            Some(Field::Column(col)) => v["binding"] = json!({ "column": col }),
            None => {
                self.notes
                    .push(format!("{cs} is not a field of the record source"));
                return None;
            }
        }
        if readonly_source || locked(c) {
            v["readOnly"] = json!(true);
        }
        let Some((plan, access)) = column else {
            return Some(v);
        };
        let plan_col = plan.column(&access.name).cloned();
        if let Some(complex) = &access.complex {
            return self.complex_list(&plan, &access, complex, label);
        }
        let Some(pc) = plan_col else { return Some(v) };
        v["kind"] = json!(kind_for(&pc, &access));
        if access.ty == ColType::Ole || pc.declared == "BLOB" {
            self.notes.push(format!(
                "{}: OLE object fields cannot be shown",
                access.name
            ));
            return None;
        }
        // Lookups: the control's own row source, else the field's.
        let lookup = match c
            .get("RowSource")
            .filter(|_| matches!(c.kind.as_str(), "ComboBox" | "ListBox"))
        {
            Some(rs) => {
                let rs = rs.to_string();
                let owner = format!("{} {}", self.info.name, c.get("Name").unwrap_or(""));
                self.ctx.lookup(
                    &owner,
                    c.get("RowSourceType").unwrap_or("Table/Query"),
                    &rs,
                    c.int("BoundColumn").unwrap_or(1),
                    c.get("ColumnWidths").unwrap_or(""),
                    c.int("ColumnCount").unwrap_or(1),
                )
            }
            None => self.ctx.column_lookup(&access),
        };
        match lookup {
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
            None => {}
        }
        if c.kind == "OptionGroup" {
            let opts: Vec<Value> = children(c)
                .into_iter()
                .filter(|b| {
                    matches!(
                        b.kind.as_str(),
                        "OptionButton" | "ToggleButton" | "CheckBox"
                    )
                })
                .filter_map(|b| {
                    let value = b.get("OptionValue")?.to_string();
                    let label = attached_label(b)
                        .and_then(|l| l.get("Caption"))
                        .or(b.get("Caption"))
                        .map(clean_caption)
                        .unwrap_or_else(|| value.clone());
                    Some(json!({ "value": value, "label": label }))
                })
                .collect();
            if !opts.is_empty() {
                v["kind"] = json!("select");
                v["options"] = json!(opts);
            }
        }
        if c.kind == "ToggleButton" && v["kind"] == "boolean" {
            v["variant"] = json!("toggle");
        }
        let mut validation = json!({});
        if pc.not_null {
            validation["required"] = json!(true);
        }
        let rule = c.get("ValidationRule").map(|r| {
            (
                r.to_string(),
                c.get("ValidationText").unwrap_or("").to_string(),
            )
        });
        let rule = rule.or_else(|| {
            access.prop("ValidationRule").map(|r| {
                (
                    r.to_string(),
                    access.prop("ValidationText").unwrap_or("").to_string(),
                )
            })
        });
        if let Some((rule, message)) = rule {
            match parse_field_rule(&rule, &access.name).and_then(|e| {
                let names = &self.names;
                let resolve = |n: &str| {
                    names
                        .iter()
                        .find(|(x, _)| x.eq_ignore_ascii_case(n))
                        .map(|(_, e)| e.clone())
                };
                ExprWriter::new(Target::Form, &resolve).write(&e)
            }) {
                Ok(e) => {
                    validation["expression"] = json!(e);
                    if !message.is_empty() {
                        validation["message"] = json!(message);
                    }
                }
                Err(e) => self.notes.push(format!(
                    "{}: validation rule {rule} was not converted ({e})",
                    access.name
                )),
            }
        }
        if validation.as_object().is_some_and(|o| !o.is_empty()) {
            v["validation"] = validation;
        }
        if let Some(d) = c
            .get("DefaultValue")
            .or_else(|| access.prop("DefaultValue"))
        {
            match self.expr(d) {
                Ok(e) => v["defaultValue"] = json!(e),
                Err(e) => self.notes.push(format!(
                    "{}: default value {d} was not converted ({e})",
                    access.name
                )),
            }
        }
        if let Some(f) = c
            .get("Format")
            .or_else(|| access.prop("Format"))
            .and_then(format_pattern)
        {
            if matches!(
                v["kind"].as_str(),
                Some("number" | "decimal" | "date" | "datetime" | "time" | "computed")
            ) {
                v["format"] = json!(f);
            }
        }
        Some(v)
    }

    /// Attachment and multi-value fields: a related list of their child table.
    fn complex_list(
        &mut self,
        plan: &TablePlan,
        access: &Column,
        complex: &Complex,
        label: String,
    ) -> Option<Value> {
        let child = self.ctx.plans.iter().find(|p| {
            p.access
                .eq_ignore_ascii_case(&format!("{}.{}", plan.access, access.name))
        })?;
        let key = plan.single_key()?;
        let columns: Vec<&str> = match complex {
            Complex::Attachment => vec!["File Name", "File Type"],
            Complex::MultiValue(_) => vec!["Value"],
        };
        Some(json!({
            "id": id(),
            "kind": "relatedList",
            "label": label,
            "related": { "table": child.name, "foreignKey": child.columns[1].name, "parentColumn": key, "columns": columns },
        }))
    }

    fn button(&mut self, c: &Node, header: bool) -> Option<Value> {
        let label = clean_caption(
            c.get("Caption")
                .or(c.get("ControlTipText"))
                .or(c.get("Name"))
                .unwrap_or("Button"),
        );
        let action_id = if let Some(block) = c.prop_block("OnClickEmMacro") {
            let (stmts, _) = read_macro(block);
            let names = self.names.clone();
            let resolve = move |n: &str| {
                names
                    .iter()
                    .find(|(x, _)| x.eq_ignore_ascii_case(n))
                    .map(|(_, e)| e.clone())
            };
            let mut w = StepWriter {
                ctx: self.ctx,
                resolve: &resolve,
                form_id: Some(self.info.id.clone()),
                notes: Notes::default(),
                owner: format!("{}: {label}", self.info.name),
                queries: vec![],
            };
            let steps = w.steps(&stmts);
            let (lost, made) = (w.notes.0, w.queries);
            if !steps.is_empty() {
                self.ctx.extra_queries.extend(made);
            }
            if steps.is_empty() {
                if !header || !lost.is_empty() {
                    self.notes.push(format!(
                        "the button {label} was left out: {}",
                        if lost.is_empty() {
                            "its macro only acts on Access windows".to_string()
                        } else {
                            lost.join("; ")
                        }
                    ));
                }
                return None;
            }
            for n in lost {
                self.notes.push(format!("button {label}: {n}"));
            }
            let aid = id();
            self.actions.push(json!({ "id": aid, "name": format!("{}: {label}", self.info.name), "description": "Converted from an Access embedded macro.", "steps": steps, "onError": "stop" }));
            aid
        } else if let Some(on_click) = c.get("OnClick") {
            if on_click.eq_ignore_ascii_case("[Event Procedure]") {
                self.notes.push(format!(
                    "the button {label} runs VBA, which was not converted"
                ));
                return None;
            }
            match self.ctx.action_ids.get(&on_click.to_lowercase()) {
                Some(a) => a.clone(),
                None => {
                    self.notes.push(format!(
                        "the button {label} runs {on_click}, which was not converted"
                    ));
                    return None;
                }
            }
        } else {
            return None;
        };
        Some(json!({ "id": id(), "kind": "button", "label": label, "actionId": action_id }))
    }

    fn subform(&mut self, c: &Node) -> Option<Value> {
        let (kind, target) = source_object(c)?;
        let parent_table = self.info.bound.table.clone()?;
        let label = attached_label(c)
            .and_then(|l| l.get("Caption"))
            .map(clean_caption)
            .unwrap_or_else(|| target.clone());
        let (child_table, columns, form_id) = match kind.as_str() {
            "form" => {
                let info = self.ctx.infos.get(&target.to_lowercase())?.clone();
                let table = info.bound.table.clone()?;
                // Rows open the subform itself when it edits one row, else the table's detail form.
                let form_id = if info.modes.contains(&"detail") && !info.has_subforms {
                    Some(info.id.clone())
                } else if info.modes == ["list"] {
                    super::forms::detail_form(self.ctx, &info).filter(|id| {
                        // A form inside a related list cannot hold related lists itself.
                        let generated = self.ctx.auto_details.values().any(|x| x == id);
                        let nested = self
                            .ctx
                            .infos
                            .values()
                            .any(|i| &i.id == id && i.has_subforms);
                        let child_tables = self.ctx.plans.iter().any(|p| {
                            p.foreign_keys.iter().any(|fk| {
                                self.ctx
                                    .table_name(&table)
                                    .is_some_and(|t| fk.target_table == t)
                            })
                        });
                        !nested && !(generated && child_tables)
                    })
                } else {
                    None
                };
                let cols: Vec<String> = info
                    .list_columns
                    .iter()
                    .filter_map(|n| match info.bound.field(n) {
                        Some(Field::Column(col)) => Some(col),
                        _ => None,
                    })
                    .collect();
                (table, cols, form_id)
            }
            "table" => (target.clone(), vec![], None),
            _ => {
                self.notes.push(format!(
                    "the {kind} {target} shown inside the form was left out"
                ));
                return None;
            }
        };
        let masters: Vec<String> = c
            .get("LinkMasterFields")
            .map(|s| s.split(';').map(|x| x.trim().to_string()).collect())
            .unwrap_or_default();
        let childs: Vec<String> = c
            .get("LinkChildFields")
            .map(|s| s.split(';').map(|x| x.trim().to_string()).collect())
            .unwrap_or_default();
        let (fk, parent_col) = match (childs.first(), masters.first()) {
            (Some(ch), Some(m)) if !ch.is_empty() => (ch.clone(), m.clone()),
            _ => {
                // Access links by the relationship between the two tables.
                let rel = self.ctx.db.relationships.iter().find(|r| {
                    r.table.eq_ignore_ascii_case(&child_table)
                        && r.ref_table.eq_ignore_ascii_case(&parent_table)
                })?;
                (
                    rel.columns.first()?.clone(),
                    rel.ref_columns.first()?.clone(),
                )
            }
        };
        // Link fields may name controls or query columns; map them to table columns.
        let parent_col = match self.info.bound.field(&parent_col) {
            Some(Field::Column(col)) => col,
            _ => parent_col,
        };
        let child_name = self.ctx.table_name(&child_table)?;
        let mut related = json!({ "table": child_name, "foreignKey": fk, "parentColumn": parent_col, "columns": columns });
        if let Some(f) = form_id {
            related["formId"] = json!(f);
        }
        Some(json!({ "id": id(), "kind": "relatedList", "label": label, "related": related }))
    }

    fn tabs(&mut self, c: &Node) -> Option<Placed> {
        let tab_id = id();
        let r = rect(c);
        let mut tabs = vec![];
        let mut inner = vec![];
        for page in children(c).into_iter().filter(|p| p.kind == "Page") {
            if is_hidden(page) {
                continue;
            }
            let page_id = id();
            tabs.push(json!({ "id": page_id, "label": clean_caption(page.get("Caption").or(page.get("Name")).unwrap_or("Page")) }));
            let items: Vec<Placed> = children(page)
                .into_iter()
                .filter_map(|x| self.control(x, false))
                .collect();
            let rects: Vec<Rect> = items.iter().map(|p| p.rect).collect();
            let (placements, _) = place(&rects, r.left, r.width, 1);
            for (mut p, pl) in items.into_iter().zip(placements) {
                if p.control["kind"] == "tabs" {
                    self.notes
                        .push("tab controls inside tab controls were flattened");
                }
                p.control["placement"] = pl.json();
                p.control["parent"] = json!({ "id": tab_id, "tab": page_id });
                inner.push(p.control);
                inner.extend(p.inner.into_iter().map(|mut x| {
                    x["parent"] = json!({ "id": tab_id, "tab": page_id });
                    x
                }));
            }
        }
        if tabs.is_empty() {
            return None;
        }
        let control = json!({ "id": tab_id, "kind": "tabs", "label": "", "tabs": tabs, "layout": grid_layout() });
        Some(Placed {
            control,
            rect: r,
            inner,
        })
    }

    fn image(&mut self, c: &Node) -> Option<Value> {
        let r = rect(c);
        // Small images are icons next to buttons.
        if r.width < 1080 || r.height < 720 {
            return None;
        }
        let name = c.get("Name").unwrap_or("image").to_string();
        let data = match c.binary("ImageData") {
            Some(d) => d.to_vec(),
            None => {
                let pic = c.get("Picture")?;
                self.ctx
                    .db
                    .resources
                    .iter()
                    .find(|x| x.name.eq_ignore_ascii_case(pic))?
                    .data
                    .clone()
            }
        };
        let start =
            (0..data.len().min(64)).find(|&i| super::forms::sniff_image(&data[i..]).is_some())?;
        let asset = self.ctx.add_asset(&name, data[start..].to_vec())?;
        Some(json!({ "id": id(), "kind": "image", "label": "", "assetId": asset }))
    }
}

/// The ixtable control kind for a column.
pub fn kind_for(c: &ColumnPlan, access: &Column) -> &'static str {
    match (c.conv, c.declared.as_str()) {
        (Conv::DateOnly, _) => "date",
        (Conv::TimeOnly, _) => "time",
        (_, "BOOLEAN") => "boolean",
        (_, "INTEGER") => "number",
        (_, "REAL") => "decimal",
        (_, d) if d.starts_with("DECIMAL") => "decimal",
        (_, "TIMESTAMP") => "datetime",
        _ if access.ty == ColType::Memo && !access.hyperlink => "multiline",
        _ => "text",
    }
}
