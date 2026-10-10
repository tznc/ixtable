//! Access forms → ixtable forms (`docs/decisions/access-import.md`, "Forms").
//!
//! Single forms become detail forms (`detail`, `create`, `edit`); continuous,
//! datasheet and split forms become list forms whose detail form is the one
//! their rows open. Absolute layouts map onto the 12-column grid
//! (`layout`), subforms become related lists, and button macros become actions.
use super::controls::{ControlWriter, FormInfo};
use super::queries::QueryConversion;
use super::report::{ImportReport, Notes, Status};
use super::schema::{DbSchema, TablePlan};
use super::sources::Source;
use crate::access::model::{AccessDb, DesignObject};
use crate::access::text_format::Node;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// An image to import as an application asset; config refers to it as `{{asset:<key>}}`.
#[derive(Debug, Clone)]
pub struct AssetRef {
    pub key: String,
    pub file_name: String,
    pub media_type: &'static str,
    pub data: Vec<u8>,
}

/// A navigation entry found on an Access navigation form.
#[derive(Debug, Clone)]
pub struct NavHint {
    pub label: String,
    /// "form" or "report"
    pub kind: String,
    pub target: String,
}

/// Shared state of the definition conversion: ids assigned up front so
/// objects can refer to each other in any order.
pub struct Context<'a> {
    pub db: &'a AccessDb,
    pub schema: &'a DbSchema<'a>,
    pub plans: &'a [TablePlan],
    pub queries: &'a QueryConversion,
    pub form_ids: BTreeMap<String, String>,
    pub report_ids: BTreeMap<String, String>,
    pub action_ids: BTreeMap<String, String>,
    pub extra_queries: Vec<Value>,
    pub lookup_views: Vec<String>,
    pub view_names: BTreeSet<String>,
    pub assets: Vec<AssetRef>,
    pub start_form: Option<String>,
    /// Forms only used inside other forms.
    pub subforms: BTreeSet<String>,
    pub nav_hints: Vec<NavHint>,
    /// Converted form info by lower-case Access name.
    pub infos: BTreeMap<String, FormInfo>,
    /// Detail forms generated for tables that had none: table → form id.
    pub auto_details: BTreeMap<String, String>,
}

fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}

impl<'a> Context<'a> {
    pub fn new(
        db: &'a AccessDb,
        schema: &'a DbSchema<'a>,
        plans: &'a [TablePlan],
        queries: &'a QueryConversion,
    ) -> Self {
        Self {
            db,
            schema,
            plans,
            queries,
            form_ids: db
                .forms
                .iter()
                .map(|f| (f.name.to_lowercase(), id()))
                .collect(),
            report_ids: db
                .reports
                .iter()
                .map(|r| (r.name.to_lowercase(), id()))
                .collect(),
            action_ids: BTreeMap::new(),
            extra_queries: vec![],
            lookup_views: vec![],
            view_names: BTreeSet::new(),
            assets: vec![],
            start_form: None,
            subforms: BTreeSet::new(),
            nav_hints: vec![],
            infos: BTreeMap::new(),
            auto_details: BTreeMap::new(),
        }
    }

    /// Adds an image asset once per content and returns its placeholder.
    pub fn add_asset(&mut self, name: &str, data: Vec<u8>) -> Option<String> {
        use sha2::Digest;
        let (ext, media) = sniff_image(&data)?;
        let key = format!("{:x}", sha2::Sha256::digest(&data))[..16].to_string();
        if !self.assets.iter().any(|a| a.key == key) {
            let stem: String = name
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c == '-' || c == '_' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect();
            self.assets.push(AssetRef {
                key: key.clone(),
                file_name: format!("{stem}-{key}.{ext}"),
                media_type: media,
                data,
            });
        }
        Some(format!("{{{{asset:{key}}}}}"))
    }
}

/// Image type from content; Access `ImageData` may carry a small header first.
pub fn sniff_image(data: &[u8]) -> Option<(&'static str, &'static str)> {
    let starts = |sig: &[u8]| data.starts_with(sig);
    if starts(b"\x89PNG") {
        Some(("png", "image/png"))
    } else if starts(b"\xFF\xD8\xFF") {
        Some(("jpg", "image/jpeg"))
    } else if starts(b"GIF8") {
        Some(("gif", "image/gif"))
    } else if starts(b"BM") {
        Some(("bmp", "image/bmp"))
    } else {
        None
    }
}

/// The `Begin Form` (or `Begin Report`) block of a design object.
pub fn design_block(o: &DesignObject) -> Option<&Node> {
    o.root.block("Form").or_else(|| o.root.block("Report"))
}

/// Section blocks (FormHeader, Section, ...) of a form or report block.
pub fn sections(form: &Node) -> Vec<&Node> {
    form.blocks()
        .filter(|b| b.kind.is_empty())
        .flat_map(|b| b.blocks())
        .filter(|b| is_section(&b.kind))
        .collect()
}

pub fn is_section(kind: &str) -> bool {
    matches!(
        kind,
        "FormHeader"
            | "FormFooter"
            | "PageHeader"
            | "PageFooter"
            | "Section"
            | "BreakHeader"
            | "BreakFooter"
    )
}

/// The controls directly inside a section or container.
pub fn children(node: &Node) -> Vec<&Node> {
    node.blocks()
        .filter(|b| b.kind.is_empty())
        .flat_map(|b| b.blocks())
        .collect()
}

pub fn is_hidden(n: &Node) -> bool {
    matches!(n.get("Visible"), Some("NotDefault" | "0" | "False"))
}

/// `Form.Name`, `Report.Name` or a bare name.
pub fn source_object(n: &Node) -> Option<(String, String)> {
    let so = n.get("SourceObject")?.trim();
    match so.split_once('.') {
        Some((kind, name)) if ["Form", "Report", "Table", "Query"].contains(&kind) => {
            Some((kind.to_lowercase(), name.to_string()))
        }
        _ => Some(("form".into(), so.to_string())),
    }
}

/// First pass: bind every form, find its view, list columns and subforms.
fn prescan(ctx: &mut Context, report: &mut ImportReport) {
    let forms: Vec<DesignObject> = ctx.db.forms.clone();
    for f in &forms {
        let Some(block) = design_block(f) else {
            report.add(
                "form",
                &f.name,
                Status::Skipped,
                vec!["the form definition could not be read".into()],
            );
            continue;
        };
        let bound = ctx.bind(block.get("RecordSource").unwrap_or(""), &f.name);
        let view = block.int("DefaultView").unwrap_or(0);
        let mut list_columns = vec![];
        let mut has_subforms = false;
        let mut detail_targets = vec![];
        for s in sections(block) {
            visit(s, &mut |c| {
                if c.kind == "Subform" {
                    has_subforms = true;
                    if let Some((kind, name)) = source_object(c) {
                        if kind == "form" {
                            ctx.subforms.insert(name.to_lowercase());
                        }
                    }
                }
                if c.kind == "NavigationButton" {
                    if let Some(target) = c.get("NavigationTargetName") {
                        let kind = if c.int("NavigationTargetType") == Some(1) {
                            "report"
                        } else {
                            "form"
                        };
                        ctx.nav_hints.push(NavHint {
                            label: clean_caption(c.get("Caption").unwrap_or(target)),
                            kind: kind.into(),
                            target: target.to_string(),
                        });
                    }
                }
                if let Some(t) = c
                    .get("Tag")
                    .and_then(|t| t.split('~').find_map(|p| p.strip_prefix("FormName=")))
                {
                    detail_targets.push(t.to_string());
                }
                for key in ["OnClickEmMacro", "OnDblClickEmMacro"] {
                    if let Some(m) = c.prop_block(key) {
                        let (stmts, _) = super::macros::read_macro(m);
                        collect_open_forms(&stmts, &mut detail_targets);
                    }
                }
            });
            if s.kind == "Section" {
                let mut cols: Vec<(i64, i64, String)> = vec![];
                for c in children(s) {
                    if is_hidden(c) {
                        continue;
                    }
                    if let Some(cs) = c.get("ControlSource").filter(|cs| !cs.starts_with('=')) {
                        let order = if view == 2 {
                            c.int("TabIndex").unwrap_or(0)
                        } else {
                            c.int("Left").unwrap_or(0)
                        };
                        cols.push((order, c.int("Top").unwrap_or(0), cs.to_string()));
                    }
                }
                cols.sort();
                list_columns = cols.into_iter().map(|(_, _, c)| c).collect();
            }
        }
        let modes = form_modes(block, view, &bound.source);
        ctx.infos.insert(
            f.name.to_lowercase(),
            FormInfo {
                id: ctx
                    .form_ids
                    .get(&f.name.to_lowercase())
                    .cloned()
                    .unwrap_or_else(id),
                name: f.name.clone(),
                view,
                bound,
                modes,
                list_columns,
                has_subforms,
                detail_targets,
            },
        );
    }
}

fn collect_open_forms(stmts: &[super::macros::Statement], out: &mut Vec<String>) {
    for s in stmts {
        match s {
            super::macros::Statement::Action { name, args } if name == "OpenForm" => {
                if let Some((_, f)) = args.iter().find(|(n, _)| n == "FormName") {
                    out.push(f.clone());
                }
            }
            super::macros::Statement::If { branches } => {
                for (_, b) in branches {
                    collect_open_forms(b, out);
                }
            }
            _ => {}
        }
    }
}

/// Calls `f` for every control, descending into tabs, pages and option groups.
pub fn visit<'n>(node: &'n Node, f: &mut dyn FnMut(&'n Node)) {
    for c in children(node) {
        f(c);
        if matches!(c.kind.as_str(), "Tab" | "Page" | "OptionGroup") {
            visit(c, f);
        }
    }
}

pub fn clean_caption(s: &str) -> String {
    s.replace("&&", "\u{0}")
        .replace('&', "")
        .replace('\u{0}', "&")
        .trim()
        .trim_end_matches(':')
        .trim()
        .to_string()
}

fn form_modes(block: &Node, view: i64, source: &Source) -> Vec<&'static str> {
    if matches!(source, Source::Query { .. }) {
        return if view == 0 {
            vec!["detail"]
        } else {
            vec!["list"]
        };
    }
    if view != 0 && view != 3 {
        return vec!["list"];
    }
    if block.flag("DataEntry") == Some(true) {
        return vec!["create"];
    }
    let mut modes = vec!["detail"];
    let denied = |k: &str| matches!(block.get(k), Some("NotDefault" | "0" | "False"));
    if !denied("AllowAdditions") {
        modes.push("create");
    }
    if !denied("AllowEdits") {
        modes.push("edit");
    }
    modes
}

/// Converts every form. Returns form definitions and the actions their buttons run.
pub fn convert_all(ctx: &mut Context, report: &mut ImportReport) -> (Vec<Value>, Vec<Value>) {
    prescan(ctx, report);
    // Unreadable forms cannot be opened.
    let infos = ctx.infos.clone();
    ctx.form_ids.retain(|name, _| infos.contains_key(name));
    let mut forms = vec![];
    let mut actions = vec![];
    let objects: Vec<DesignObject> = ctx.db.forms.clone();
    for f in &objects {
        let Some(info) = ctx.infos.get(&f.name.to_lowercase()).cloned() else {
            continue;
        };
        let Some(block) = design_block(f) else {
            continue;
        };
        let mut notes = Notes::default();
        for n in &info.bound.notes {
            notes.push(n.clone());
        }
        if info.bound.source == Source::None && !has_bound_controls(block) {
            // Menus and dashboards: their buttons and navigation become app navigation.
            let mut w = ControlWriter::new(ctx, &info, &mut notes);
            let controls = w.form_controls(block);
            actions.append(&mut w.actions);
            forms.push(
                json!({ "id": info.id, "name": f.name, "modes": ["detail"], "controls": controls }),
            );
            report_form(report, &f.name, has_code(f), notes);
            continue;
        }
        let mut w = ControlWriter::new(ctx, &info, &mut notes);
        let controls = w.form_controls(block);
        actions.append(&mut w.actions);
        let rules = w.form_rules();
        let mut form = json!({ "id": info.id, "name": f.name, "modes": info.modes, "controls": controls, "rules": rules });
        match &info.bound.source {
            Source::Table(t) => form["source"] = json!({ "kind": "table", "table": t }),
            Source::Query { id, .. } => form["source"] = json!({ "kind": "query", "queryId": id }),
            Source::None => {}
        }
        if info.modes == ["list"] {
            let columns = list_columns(ctx, &info);
            form["listColumns"] = json!(columns);
            if let Some(d) = detail_form(ctx, &info) {
                form["detailFormId"] = json!(d);
            }
        }
        event_notes(ctx, block, &mut notes);
        forms.push(form);
        report_form(report, &f.name, has_code(f), notes);
    }
    forms.extend(super::autoforms::generate(ctx, report));
    unnest_related_lists(&mut forms);
    (forms, actions)
}

/// A form opened from a related list cannot hold related lists itself, so
/// such rows open nothing rather than a form that breaks the one-level rule.
pub fn unnest_related_lists(forms: &mut [Value]) {
    let is_list = |c: &Value| c["kind"] == "relatedList";
    let holders: BTreeSet<String> = forms
        .iter()
        .filter(|f| {
            f["controls"]
                .as_array()
                .is_some_and(|c| c.iter().any(is_list))
        })
        .filter_map(|f| f["id"].as_str().map(str::to_string))
        .collect();
    for form in forms.iter_mut() {
        let controls = form.get_mut("controls").and_then(Value::as_array_mut);
        for c in controls.into_iter().flatten() {
            let nested = c["related"]["formId"]
                .as_str()
                .is_some_and(|id| holders.contains(id));
            if is_list(c) && nested {
                if let Some(r) = c["related"].as_object_mut() {
                    r.remove("formId");
                }
            }
        }
    }
}

/// Form events have no ixtable hook. Macros that only manage Access windows
/// lose nothing; others are reported.
fn event_notes(ctx: &Context, block: &Node, notes: &mut Notes) {
    for event in [
        "OnLoad",
        "OnOpen",
        "OnCurrent",
        "BeforeUpdate",
        "AfterUpdate",
        "BeforeInsert",
        "AfterInsert",
        "OnDelete",
        "OnClose",
        "OnActivate",
    ] {
        if let Some(m) = block.prop_block(&format!("{event}EmMacro")) {
            let (stmts, _) = super::macros::read_macro(m);
            let none = |_: &str| None;
            let mut w = super::macros::StepWriter {
                ctx,
                resolve: &none,
                form_id: None,
                notes: Notes::default(),
                owner: String::new(),
                queries: vec![],
            };
            let steps = w.steps(&stmts);
            if !steps.is_empty() || !w.notes.0.is_empty() {
                notes.push(format!(
                    "the {event} macro does not run (ixtable forms have no {event} event)"
                ));
            }
        } else if block
            .get(event)
            .is_some_and(|v| v.eq_ignore_ascii_case("[Event Procedure]"))
        {
            notes.push(format!("the {event} VBA procedure does not run"));
        }
    }
}

fn has_code(f: &DesignObject) -> bool {
    f.root.code.as_deref().is_some_and(|c| {
        c.lines().any(|l| {
            l.trim_start().starts_with("Private Sub") || l.trim_start().starts_with("Sub ")
        })
    })
}

fn report_form(report: &mut ImportReport, name: &str, has_code: bool, mut notes: Notes) {
    if has_code {
        notes.push("its VBA code-behind does not run (kept in the asset \"Access VBA.txt\")");
    }
    report.add("form", name, notes.status(), notes.0);
}

fn has_bound_controls(block: &Node) -> bool {
    let mut found = false;
    for s in sections(block) {
        visit(s, &mut |c| {
            if c.get("ControlSource")
                .is_some_and(|cs| !cs.starts_with('='))
            {
                found = true;
            }
        });
    }
    found
}

/// List columns that are real columns of the bound table.
fn list_columns(ctx: &Context, info: &FormInfo) -> Vec<String> {
    let Source::Table(t) = &info.bound.source else {
        return info.list_columns.clone();
    };
    let plan = ctx.plans.iter().find(|p| &p.name == t);
    let mut out: Vec<String> = vec![];
    for c in &info.list_columns {
        if let Some(super::sources::Field::Column(col)) = info.bound.field(c) {
            if plan.is_some_and(|p| p.column(&col).is_some()) && !out.contains(&col) {
                out.push(col);
            }
        }
    }
    if out.is_empty() {
        if let Some(p) = plan {
            out = p.columns.iter().take(6).map(|c| c.name.clone()).collect();
        }
    }
    out
}

/// The detail form a list form's rows open: one its macros open, else any
/// single form on the same table, else a generated one.
pub fn detail_form(ctx: &mut Context, info: &FormInfo) -> Option<String> {
    let Source::Table(table) = &info.bound.source else {
        return None;
    };
    let is_detail =
        |i: &FormInfo| i.modes.contains(&"detail") && i.bound.source == info.bound.source;
    for t in &info.detail_targets {
        if let Some(i) = ctx.infos.get(&t.to_lowercase()) {
            if is_detail(i) {
                return Some(i.id.clone());
            }
        }
    }
    if let Some(i) = ctx
        .infos
        .values()
        .find(|i| is_detail(i) && !ctx.subforms.contains(&i.name.to_lowercase()))
    {
        return Some(i.id.clone());
    }
    Some(super::autoforms::detail_for(ctx, table))
}
