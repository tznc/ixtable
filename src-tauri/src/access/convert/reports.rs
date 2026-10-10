//! Access reports → ixtable banded reports (`docs/decisions/report-engine.md`).
//!
//! Sections map to bands (report header, page header, group headers, detail,
//! group footers, page footer, report footer), twips to points (÷ 20), and
//! `BreakLevel` sorting and grouping to report groups plus the dataset's
//! ORDER BY. Combo boxes show their lookup's display value, which the dataset
//! query adds as an extra column.
use super::controls::rect;
use super::forms::{children, clean_caption, design_block, is_hidden, sections, Context};
use super::report::{ImportReport, Notes};
use super::sources::{Field, Lookup, Source};
use crate::access::model::DesignObject;
use crate::access::text_format::Node;
use crate::access::translate::expr::{field, translate, Target};
use crate::access::translate::format::format_pattern;
use crate::access::translate::sql::quote;
use serde_json::{json, Value};

fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}

fn pt(twips: i64) -> f64 {
    (twips as f64 / 20.0 * 100.0).round() / 100.0
}

/// Page size, orientation and margins from `PrtMip` and `PrtDevMode[W]`.
pub fn page_setup(block: &Node) -> (Value, f64) {
    let mip = block.binary("PrtMip").unwrap_or_default();
    let u32_at = |b: &[u8], at: usize| {
        b.get(at..at + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as i64)
    };
    let margin = |at: usize| {
        u32_at(mip, at)
            .filter(|v| *v > 0 && *v < 4320)
            .map(pt)
            .unwrap_or(36.0)
    };
    let (left, top, right, bottom) = (margin(0), margin(4), margin(8), margin(12));
    let (orientation, paper) = match (block.binary("PrtDevModeW"), block.binary("PrtDevMode")) {
        (Some(d), _) if d.len() >= 80 => (
            u16::from_le_bytes([d[76], d[77]]),
            u16::from_le_bytes([d[78], d[79]]),
        ),
        (_, Some(d)) if d.len() >= 48 => (
            u16::from_le_bytes([d[44], d[45]]),
            u16::from_le_bytes([d[46], d[47]]),
        ),
        _ => (1, 1),
    };
    let landscape = orientation == 2;
    let a4 = paper == 9;
    let (w, _) = match (a4, landscape) {
        (true, false) => (595.28, 841.89),
        (true, true) => (841.89, 595.28),
        (false, false) => (612.0, 792.0),
        (false, true) => (792.0, 612.0),
    };
    let content = (w - left - right).max(72.0);
    let setup = json!({
        "size": if a4 { "A4" } else { "Letter" },
        "orientation": if landscape { "landscape" } else { "portrait" },
        "margins": { "top": top, "right": right, "bottom": bottom, "left": left },
    });
    (setup, content)
}

struct Level {
    source: String,
    descending: bool,
    header: bool,
    footer: bool,
    group_on: i64,
    interval: i64,
}

fn break_levels(block: &Node) -> Vec<Level> {
    block
        .blocks()
        .filter(|b| b.kind.is_empty())
        .flat_map(|b| b.blocks())
        .filter(|b| b.kind == "BreakLevel")
        .filter_map(|b| {
            Some(Level {
                source: b.get("ControlSource")?.to_string(),
                descending: b.flag("SortOrder") == Some(true),
                header: b.flag("GroupHeader") == Some(true),
                footer: b.flag("GroupFooter") == Some(true),
                group_on: b.int("GroupOn").unwrap_or(0),
                interval: b.int("GroupInterval").unwrap_or(1).max(1),
            })
        })
        .collect()
}

struct ReportWriter<'c, 'a> {
    ctx: &'c mut Context<'a>,
    name: String,
    names: Vec<(String, String)>,
    notes: Notes,
    /// Lookup display columns the dataset must add: (column, lookup table, value, display).
    lookups: Vec<(String, String, String, String)>,
    /// Values computed in SQL by the dataset: (alias, DuckDB expression over its columns).
    sql_columns: Vec<(String, String)>,
    /// The dataset's columns with their kinds (for SQL fallbacks).
    dataset: Vec<(String, crate::access::translate::sql::Kind)>,
    scale: f64,
    content: f64,
}

impl ReportWriter<'_, '_> {
    fn expr(&self, text: &str) -> Result<String, String> {
        let names = &self.names;
        let resolve = |n: &str| {
            names
                .iter()
                .find(|(x, _)| x.eq_ignore_ascii_case(n))
                .map(|(_, e)| e.clone())
        };
        translate(text, Target::Report, &resolve)
    }

    fn style(&self, c: &Node, numeric: bool) -> Value {
        let mut s = json!({});
        if let Some(size) = c.int("FontSize") {
            s["fontSize"] = json!(size);
        }
        if c.int("FontWeight").unwrap_or(400) >= 600 {
            s["bold"] = json!(true);
        }
        match c.int("TextAlign") {
            Some(1) => s["align"] = json!("left"),
            Some(2) => s["align"] = json!("center"),
            Some(3) => s["align"] = json!("right"),
            _ if numeric => s["align"] = json!("right"),
            _ => {}
        }
        s
    }

    fn component(&mut self, c: &Node, band_height: f64) -> Option<Value> {
        if is_hidden(c) {
            return None;
        }
        let r = rect(c);
        let x = (pt(r.left) * self.scale).min(self.content - 1.0).max(0.0);
        let mut w = (pt(r.width) * self.scale).max(1.0);
        if x + w > self.content {
            w = (self.content - x).max(1.0);
        }
        let y = pt(r.top).min((band_height - 1.0).max(0.0)).max(0.0);
        let mut h = pt(r.height).max(1.0);
        if y + h > band_height {
            h = (band_height - y).max(1.0);
        }
        let mut v = json!({ "id": id(), "x": x, "y": y, "w": w, "h": h });
        match c.kind.as_str() {
            "Label" => {
                let text = clean_caption(c.get("Caption")?);
                v["kind"] = json!("staticText");
                v["text"] = json!(text);
                v["style"] = self.style(c, false);
            }
            "TextBox" | "ComboBox" | "CheckBox" | "ListBox" => {
                let cs = c.get("ControlSource")?.trim().to_string();
                let format = c.get("Format").and_then(format_pattern);
                let (kind, expression) = if let Some(e) = cs.strip_prefix('=') {
                    match self.expr(e) {
                        Ok(x) => ("calculated", x),
                        Err(err) => match self.sql_value(e, c.get("Name").unwrap_or("value")) {
                            // The dataset query computes it per row instead.
                            Some(alias) => ("field", field("record", &alias)),
                            None => {
                                self.notes.push(format!(
                                    "the calculated text box {} ({e}) was not converted: {err}",
                                    c.get("Name").unwrap_or("")
                                ));
                                return None;
                            }
                        },
                    }
                } else {
                    let base = self
                        .names
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case(&cs))
                        .map(|(_, e)| e.clone());
                    let Some(base) = base else {
                        self.notes
                            .push(format!("{cs} is not a field of the report's record source"));
                        return None;
                    };
                    if c.kind == "CheckBox" {
                        ("calculated", format!("if({base}, 'Yes', 'No')"))
                    } else {
                        match self.display_column(c, &cs) {
                            Some(display) => ("field", field("record", &display)),
                            None => ("field", base),
                        }
                    }
                };
                v["kind"] = json!(kind);
                v["expression"] = json!(expression);
                if let Some(f) = format {
                    v["format"] = json!(f);
                }
                v["style"] = self.style(c, false);
                if c.flag("CanGrow") == Some(true) {
                    v["canGrow"] = json!(true);
                }
            }
            "Line" => {
                v["kind"] = json!("line");
                v["orientation"] = json!(if r.width >= r.height {
                    "horizontal"
                } else {
                    "vertical"
                });
            }
            "Rectangle" => v["kind"] = json!("rectangle"),
            "Image" => {
                let data = c.binary("ImageData").map(<[u8]>::to_vec).or_else(|| {
                    let pic = c.get("Picture")?;
                    self.ctx
                        .db
                        .resources
                        .iter()
                        .find(|x| x.name.eq_ignore_ascii_case(pic))
                        .map(|x| x.data.clone())
                })?;
                let start = (0..data.len().min(64))
                    .find(|&i| super::forms::sniff_image(&data[i..]).is_some())?;
                let asset = self
                    .ctx
                    .add_asset(c.get("Name").unwrap_or("image"), data[start..].to_vec())?;
                v["kind"] = json!("image");
                v["assetId"] = json!(asset);
            }
            "EmptyCell" | "PageBreak" => return None,
            "Subform" => {
                self.notes.push(format!(
                    "the subreport {} was left out (ixtable reports have no subreports)",
                    c.get("SourceObject").unwrap_or("")
                ));
                return None;
            }
            other => {
                self.notes
                    .push(format!("{other} controls have no report equivalent"));
                return None;
            }
        }
        Some(v)
    }

    /// A per-row expression the expression language cannot compute, as a DuckDB dataset column.
    fn sql_value(&mut self, expr: &str, control: &str) -> Option<String> {
        use crate::access::translate::ast::parse_expression;
        use crate::access::translate::sql::{Dialect, SqlWriter};
        let e = parse_expression(expr).ok()?;
        let mut w = SqlWriter::new(Dialect::DuckDb, self.ctx.schema);
        w.table_columns = self.dataset.clone();
        let sql = w.expr(&e).ok()?;
        if !w.out.params.is_empty() || crate::access::translate::functions::contains_aggregate(&e) {
            return None;
        }
        let alias = format!("{control} (value)");
        self.sql_columns.push((alias.clone(), sql));
        Some(alias)
    }

    /// A combo box shows its lookup's display value: a dataset column.
    fn display_column(&mut self, c: &Node, column: &str) -> Option<String> {
        if c.kind != "ComboBox" {
            return None;
        }
        let owner = format!("{} {}", self.name, c.get("Name").unwrap_or(""));
        let lookup = self.ctx.lookup(
            &owner,
            c.get("RowSourceType").unwrap_or("Table/Query"),
            c.get("RowSource").unwrap_or(""),
            c.int("BoundColumn").unwrap_or(1),
            c.get("ColumnWidths").unwrap_or(""),
            c.int("ColumnCount").unwrap_or(1),
        )?;
        let Lookup::Table {
            table,
            value,
            display,
        } = lookup
        else {
            return None;
        };
        let name = format!("{column} (display)");
        if !self.lookups.iter().any(|l| l.0 == column) {
            self.lookups
                .push((column.to_string(), table, value, display));
        }
        Some(name)
    }

    fn band(&mut self, section: Option<&Node>) -> Value {
        let Some(s) = section.filter(|s| !is_hidden(s)) else {
            return json!({ "height": 0, "components": [] });
        };
        let height = pt(s.int("Height").unwrap_or(0));
        let components: Vec<Value> = children(s)
            .into_iter()
            .filter_map(|c| self.component(c, height))
            .collect();
        let mut band = json!({ "height": height, "components": components });
        if s.flag("KeepTogether") == Some(true) {
            band["keepTogether"] = json!(true);
        }
        if matches!(s.int("ForceNewPage"), Some(1 | 3)) {
            band["pageBreakBefore"] = json!(true);
        }
        band
    }
}

fn group_by(
    level: &Level,
    names: &[(String, String)],
    expr: &dyn Fn(&str) -> Result<String, String>,
) -> Result<String, String> {
    let base = match level.source.strip_prefix('=') {
        Some(e) => expr(e)?,
        None => names
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(&level.source))
            .map(|(_, e)| e.clone())
            .ok_or_else(|| format!("{} is not a field", level.source))?,
    };
    Ok(match level.group_on {
        1 => format!("left({base}, {})", level.interval),
        2 => format!("year({base})"),
        3 => format!("floor((month({base}) - 1) / 3) + 1"),
        4 => format!("year({base}) * 100 + month({base})"),
        6 => format!("date(year({base}), month({base}), day({base}))"),
        7 => format!("hour({base})"),
        9 => format!("floor({base} / {0}) * {0}", level.interval),
        _ => base,
    })
}

fn convert_report(ctx: &mut Context, r: &DesignObject, report: &mut ImportReport) -> Option<Value> {
    let block = design_block(r)?;
    let report_id = ctx.report_ids.get(&r.name.to_lowercase())?.clone();
    // Reports read queries as they are: computed columns stay available.
    let record_source = block.get("RecordSource").unwrap_or("").trim().to_string();
    let bound = match ctx
        .db
        .query(&record_source)
        .and_then(|q| ctx.queries.queries.get(&q.name.to_lowercase()))
        .filter(|q| q.action.is_none())
    {
        Some(q) => super::sources::Bound {
            source: Source::Query {
                id: q.id.clone(),
                columns: q.columns.clone(),
            },
            table: None,
            fields: q
                .columns
                .iter()
                .map(|c| (c.clone(), Field::Column(c.clone())))
                .collect(),
            notes: vec![],
        },
        None => ctx.bind(&record_source, &r.name),
    };
    let dataset: Vec<(String, crate::access::translate::sql::Kind)> = match &bound.source {
        Source::Table(t) => ctx
            .plans
            .iter()
            .find(|p| &p.name == t)
            .map(|p| p.columns.iter().map(|c| (c.name.clone(), c.kind)).collect())
            .unwrap_or_default(),
        Source::Query { columns, .. } => columns
            .iter()
            .map(|c| (c.clone(), crate::access::translate::sql::Kind::Other))
            .collect(),
        Source::None => vec![],
    };
    let mut notes = Notes::default();
    for n in bound.notes.iter().filter(|n| !n.contains("read-only")) {
        notes.push(n.clone());
    }
    let names: Vec<(String, String)> = match &bound.source {
        Source::Query { columns, .. } => columns
            .iter()
            .map(|c| (c.clone(), field("record", c)))
            .collect(),
        _ => bound
            .fields
            .iter()
            .map(|(n, f)| match f {
                Field::Column(c) => (n.clone(), field("record", c)),
                Field::Computed(e) => (n.clone(), format!("({e})")),
            })
            .collect(),
    };
    let (page, content) = page_setup(block);
    let width = pt(block.int("Width").unwrap_or(0));
    let scale = if width > content && width > 0.0 {
        content / width
    } else {
        1.0
    };
    if scale < 0.95 {
        notes.push(format!(
            "the report is wider than the page; it was scaled to {:.0}%",
            scale * 100.0
        ));
    }
    let mut w = ReportWriter {
        ctx,
        name: r.name.clone(),
        names,
        notes,
        lookups: vec![],
        sql_columns: vec![],
        dataset,
        scale,
        content,
    };
    let all = sections(block);
    let find = |kind: &str| {
        all.iter()
            .copied()
            .filter(|s| s.kind == kind)
            .collect::<Vec<_>>()
    };
    let levels = break_levels(block);
    let headers = find("BreakHeader");
    let footers = find("BreakFooter");
    let mut groups = vec![];
    let (mut hi, mut fi) = (0, 0);
    let grouped: Vec<&Level> = levels.iter().filter(|l| l.header || l.footer).collect();
    let footer_count = grouped.iter().filter(|l| l.footer).count();
    for (i, level) in grouped.iter().enumerate() {
        let header = if level.header {
            hi += 1;
            headers.get(hi - 1).copied()
        } else {
            None
        };
        // Footers are listed innermost first.
        let footer = if level.footer {
            fi += 1;
            footers.get(footer_count - fi).copied()
        } else {
            None
        };
        let names = w.names.clone();
        let grouping = {
            let expr = |e: &str| w.expr(e);
            group_by(level, &names, &expr)
        };
        match grouping {
            Ok(g) => {
                let h = w.band(header);
                let f = w.band(footer);
                groups.push(json!({ "id": id(), "groupBy": g, "descending": level.descending, "header": h, "footer": f }));
            }
            Err(e) => w
                .notes
                .push(format!("group {} was not converted ({e})", i + 1)),
        }
    }
    let bands = json!({
        "reportHeader": w.band(find("FormHeader").first().copied()),
        "pageHeader": w.band(find("PageHeader").first().copied()),
        "groups": groups,
        "detail": w.band(find("Section").first().copied()),
        "pageFooter": w.band(find("PageFooter").first().copied()),
        "reportFooter": w.band(find("FormFooter").first().copied()),
    });
    // Sorting levels and lookup display columns need a dataset query.
    let order: Vec<String> = levels
        .iter()
        .filter_map(|l| {
            let col = l
                .source
                .strip_prefix('=')
                .is_none()
                .then(|| quote(&l.source))?;
            Some(format!("{col}{}", if l.descending { " DESC" } else { "" }))
        })
        .collect();
    let lookups = std::mem::take(&mut w.lookups);
    let sql_columns = std::mem::take(&mut w.sql_columns);
    let mut notes = w.notes;
    let mut out = json!({ "id": report_id, "name": r.name, "page": page, "bands": bands });
    match &bound.source {
        Source::Table(t) if order.is_empty() && lookups.is_empty() && sql_columns.is_empty() => {
            out["table"] = json!(t)
        }
        Source::None => notes.push("the report has no record source"),
        source => {
            let from = match source {
                Source::Table(t) => quote(t),
                Source::Query { id, .. } => {
                    let sql = ctx
                        .extra_queries
                        .iter()
                        .chain(super::queries::saved_queries(ctx.queries).iter())
                        .find(|q| q["id"] == json!(id))
                        .and_then(|q| q["sql"].as_str().map(str::to_string));
                    match sql {
                        Some(s) => format!("({s})"),
                        None => return None,
                    }
                }
                Source::None => unreachable!(),
            };
            let mut cols = vec!["src.*".to_string()];
            for (col, table, value, display) in &lookups {
                cols.push(format!(
                    "(SELECT l.{} FROM {} AS l WHERE l.{} = src.{} LIMIT 1) AS {}",
                    quote(display),
                    quote(table),
                    quote(value),
                    quote(col),
                    quote(&format!("{col} (display)"))
                ));
            }
            for (alias, expr) in &sql_columns {
                cols.push(format!("{expr} AS {}", quote(alias)));
            }
            let mut sql = format!("SELECT {} FROM {from} AS src", cols.join(", "));
            if !order.is_empty() {
                sql.push_str(&format!(
                    " ORDER BY {}",
                    order
                        .iter()
                        .map(|o| format!("src.{o}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            let params = match source {
                Source::Query { id, .. } => ctx
                    .extra_queries
                    .iter()
                    .chain(super::queries::saved_queries(ctx.queries).iter())
                    .find(|q| q["id"] == json!(id))
                    .map(|q| q["parameters"].clone())
                    .unwrap_or(json!([])),
                _ => json!([]),
            };
            let qid = id();
            ctx.extra_queries.push(json!({ "id": qid, "name": format!("{} (report data)", r.name), "sql": sql, "parameters": params }));
            out["datasetQueryId"] = json!(qid);
        }
    }
    for event in ["OnOpen", "OnNoData", "OnFormat", "OnPrint"] {
        if block.get(event).is_some() || block.prop_block(&format!("{event}EmMacro")).is_some() {
            notes.push(format!("the {event} event has no ixtable equivalent"));
        }
    }
    report.add("report", &r.name, notes.status(), notes.0);
    Some(out)
}

pub fn convert_all(ctx: &mut Context, report: &mut ImportReport) -> Vec<Value> {
    let reports: Vec<DesignObject> = ctx.db.reports.clone();
    let out: Vec<Value> = reports
        .iter()
        .filter_map(|r| convert_report(ctx, r, report))
        .collect();
    // Only converted reports can be opened.
    ctx.report_ids
        .retain(|_, id| out.iter().any(|r| r["id"] == json!(id)));
    out
}
