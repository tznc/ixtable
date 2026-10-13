//! Lays a report out into pages: sorting and grouping, measuring, pagination with page
//! breaks, keep-together, repeated group headers, table and grown-text splitting, and
//! page numbers resolved in a second pass. Pure and deterministic: the same report, rows
//! and options always produce the same document on every platform.

use super::component::{Band, Comp, Group, Kind, Rep, TableColumn};
use super::grow::{grow_band, grown, Growth};
use super::render::{
    component_items, measure_table, table_row_items, Diagnostics, Env, RenderContext, ScopeData,
    TableGeometry,
};
use super::running::RunningSums;
use super::split::{cut_at, slice_items};
use super::subreport::{
    resolve_subreport, split_at_subreport, Loaded, SUBREPORT_EXTRA, SUBREPORT_PAGE_BAND,
};
use super::values::RunningValue;
use super::{Item, LayoutOptions, Page, ReportDocument, Row};
use crate::reports::Report;
use serde_json::{json, Map, Value};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

const EPS: f64 = 1e-6;

/// Tables in page headers and footers are not laid out (validation warns about them).
pub const PAGE_BAND_TABLE: &str = "Tables are not supported in page headers or footers";

/// `groupPage`/`groupPages` count pages since the last group with `reset_page_number`.
#[derive(Debug, Clone, Copy)]
struct PageCtx {
    page: usize,
    pages: usize,
    group_page: usize,
    group_pages: usize,
}

impl PageCtx {
    fn overrides(&self) -> [(&'static str, Value); 4] {
        [
            ("page", json!(self.page)),
            ("pages", json!(self.pages)),
            ("groupPage", json!(self.group_page)),
            ("groupPages", json!(self.group_pages)),
        ]
    }
}

type Thunk<'a> = Box<dyn Fn(&PageCtx) -> Vec<Item> + 'a>;

#[derive(Clone)]
struct Block {
    band: Rc<Band>,
    scope: Rc<ScopeData>,
    /// Horizontal offset of the band in the body: a subreport's x position.
    x: f64,
    /// Group header with keep_together: keep it on the page of the next block.
    keep_with_next: bool,
    /// Start a new page before (band option, group `new_page` or `reset_page_number`).
    break_before: bool,
    break_after: bool,
    /// Starts a page-number section (header of a group with `reset_page_number`).
    section: bool,
    /// Open group headers (outer first) to repeat at the top of continuation pages.
    repeat: Vec<Rc<Block>>,
    /// Running-sum values of this instance (detail and group bands).
    running: Option<HashMap<String, RunningValue>>,
}

struct TableInfo {
    index: usize,
    geo: TableGeometry,
}

struct Prepared {
    block: Rc<Block>,
    table: Option<TableInfo>,
    /// Can-grow text: moved and grown components (bands without a table).
    growth: Option<Growth>,
    height: f64,
    splittable: bool,
    /// Smallest first piece: whole band, or everything above the table plus header and
    /// one row.
    min_first: f64,
}

fn type_rank(v: &Value) -> u8 {
    match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        _ => 4,
    }
}

/// Total order on group keys: null, booleans, numbers, text (by code unit), anything else.
pub fn compare_keys(a: &Value, b: &Value) -> Ordering {
    let (ra, rb) = (type_rank(a), type_rank(b));
    if ra != rb {
        return ra.cmp(&rb);
    }
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => x.cmp(y),
        (Value::Number(x), Value::Number(y)) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Value::String(x), Value::String(y)) => x.encode_utf16().cmp(y.encode_utf16()),
        (Value::Null, Value::Null) => Ordering::Equal,
        (x, y) => x.to_string().encode_utf16().cmp(y.to_string().encode_utf16()),
    }
}

fn is_empty_band(band: &Band) -> bool {
    band.height <= 0.0 && band.components.is_empty()
}

struct Keyed {
    row: Row,
    keys: Vec<Value>,
}

/// State of flattening one report (the run report or a subreport) into band instances.
struct Frame {
    source: Rc<Rep>,
    scope_base: ScopeData,
    x: f64,
    stack: Vec<String>,
    nested: bool,
    open: Vec<Rc<Block>>,
    sums: RunningSums,
    row_number: i64,
}

struct Flattener<'a> {
    env: &'a Env<'a>,
    loaded: &'a Loaded,
    base: &'a ScopeData,
    params: &'a Map<String, Value>,
    blocks: Vec<Rc<Block>>,
}

fn row_value(row: Option<&Row>) -> Value {
    row.map_or(Value::Null, |r| Value::Object(r.clone()))
}

impl Flattener<'_> {
    fn block(&self, f: &Frame, band: &Rc<Band>, overrides: Vec<(&str, Value)>, rows: Option<Rc<Vec<Row>>>) -> Block {
        let mut scope = f.scope_base.with(&overrides);
        if let Some(rows) = rows {
            scope.rows = rows;
        }
        Block {
            band: band.clone(),
            scope: Rc::new(scope),
            x: f.x,
            keep_with_next: false,
            break_before: band.page_break_before,
            break_after: band.page_break_after,
            section: false,
            repeat: f.open.clone(),
            running: None,
        }
    }

    fn counted(&self, f: &mut Frame, mut b: Block) -> Block {
        b.running = f.sums.counted(&b.band, &b.scope);
        b
    }

    fn push(&mut self, b: Rc<Block>, stack: &[String]) {
        let first_sub = b.band.components.iter().find(|c| c.is_subreport()).cloned();
        if let Some(sub) = first_sub {
            for extra in b.band.components.iter().filter(|c| c.is_subreport()).skip(1) {
                self.env.diagnostics.add(&extra.id, SUBREPORT_EXTRA);
            }
            let (head, tail) = split_at_subreport(&b.band, &sub);
            let parent = b.scope.record();
            self.push(
                Rc::new(Block {
                    band: Rc::new(head),
                    break_after: false,
                    ..(*b).clone()
                }),
                stack,
            );
            let child = resolve_subreport(&sub, self.loaded, &parent, stack, &self.env.diagnostics);
            if let Some((report, rows)) = child {
                let mut entries = self.base.entries.clone();
                let mut params: Map<String, Value> = report.params.clone();
                for (k, v) in self.params {
                    params.insert(k.clone(), v.clone());
                }
                entries.insert("params".into(), Value::Object(params));
                entries.insert("report".into(), json!({ "name": report.name }));
                entries.insert("parent".into(), parent);
                let scope_base = ScopeData {
                    entries,
                    rows: self.base.rows.clone(),
                };
                let mut next = stack.to_vec();
                next.push(report.id.clone());
                self.flatten(report, rows, scope_base, b.x + sub.x, b.repeat.clone(), next);
            }
            self.push(
                Rc::new(Block {
                    band: Rc::new(tail),
                    keep_with_next: false,
                    break_before: false,
                    section: false,
                    ..(*b).clone()
                }),
                stack,
            );
            return;
        }
        // Empty bands are skipped unless they carry a page break or section start.
        if !is_empty_band(&b.band) || b.break_before || b.break_after || b.section {
            self.blocks.push(b);
        }
    }

    /// Band instances of `source` over `source_rows`: the run report, or a subreport
    /// printed at `x` below the open group headers `repeat`. `stack` holds the ids of the
    /// reports being printed, outermost first.
    fn flatten(
        &mut self,
        source: Rc<Rep>,
        source_rows: Vec<Row>,
        scope_base: ScopeData,
        x: f64,
        repeat: Vec<Rc<Block>>,
        stack: Vec<String>,
    ) {
        let now = self.env.now;
        let groups = &source.bands.groups;
        let mut keyed: Vec<Keyed> = source_rows
            .into_iter()
            .map(|row| {
                let keys = groups
                    .iter()
                    .map(|g| self.group_key(g, &row, &scope_base, now))
                    .collect();
                Keyed { row, keys }
            })
            .collect();
        keyed.sort_by(|a, b| {
            for (i, g) in groups.iter().enumerate() {
                let c = compare_keys(&a.keys[i], &b.keys[i]);
                if c != Ordering::Equal {
                    return if g.descending { c.reverse() } else { c };
                }
            }
            Ordering::Equal
        });
        let rows: Rc<Vec<Row>> = Rc::new(keyed.iter().map(|k| k.row.clone()).collect());
        let mut f = Frame {
            nested: stack.len() > 1,
            open: repeat.clone(),
            sums: RunningSums::new(now),
            row_number: 0,
            source: source.clone(),
            scope_base,
            x,
            stack: stack.clone(),
        };
        // Running sums are counted per printed report: each subreport instance starts again.
        let record = row_value(rows.first());
        let mut b = self.block(&f, &source.bands.report_header, vec![("record", record)], Some(rows.clone()));
        b.repeat = repeat.clone();
        self.push(Rc::new(b), &stack);
        self.emit(&mut f, 0, &keyed);
        let record = row_value(rows.last());
        let mut b = self.block(&f, &source.bands.report_footer, vec![("record", record)], Some(rows));
        b.repeat = repeat;
        self.push(Rc::new(b), &stack);
    }

    fn group_key(&self, g: &Group, record: &Row, base: &ScopeData, now: Option<chrono::DateTime<chrono::Utc>>) -> Value {
        if g.group_by.trim().is_empty() {
            return Value::Null;
        }
        match base.eval(&g.group_by, &[("record", Value::Object(record.clone()))], now) {
            Ok(v) => v,
            Err(e) => {
                self.env.diagnostics.add(&g.id, &e.message);
                Value::Null
            }
        }
    }

    fn emit(&mut self, f: &mut Frame, level: usize, slice: &[Keyed]) {
        let source = f.source.clone();
        let groups = &source.bands.groups;
        let stack = f.stack.clone();
        if level == groups.len() {
            let slice_rows: Rc<Vec<Row>> = Rc::new(slice.iter().map(|k| k.row.clone()).collect());
            for k in slice {
                f.row_number += 1;
                let b = self.block(
                    f,
                    &source.bands.detail,
                    vec![("record", Value::Object(k.row.clone())), ("rowNumber", json!(f.row_number))],
                    Some(slice_rows.clone()),
                );
                let b = self.counted(f, b);
                self.push(Rc::new(b), &stack);
            }
            return;
        }
        let g = &groups[level];
        let mut start = 0;
        while start < slice.len() {
            let mut end = start + 1;
            while end < slice.len()
                && compare_keys(&slice[end].keys[level], &slice[start].keys[level]) == Ordering::Equal
            {
                end += 1;
            }
            let run = &slice[start..end];
            let run_rows: Rc<Vec<Row>> = Rc::new(run.iter().map(|k| k.row.clone()).collect());
            let group = json!({ "key": run[0].keys[level], "level": level + 1, "count": run.len() });
            f.sums.restart_below(groups, level, &source.bands.detail);
            // Page numbering sections belong to the run report, not its subreports.
            let mut header = self.block(
                f,
                &g.header,
                vec![("record", row_value(run_rows.first())), ("group", group.clone())],
                Some(run_rows.clone()),
            );
            header.keep_with_next = g.header.keep_together;
            header.break_before = g.header.page_break_before || g.new_page || g.reset_page_number;
            header.section = !f.nested && g.reset_page_number;
            let header = Rc::new(self.counted(f, header));
            self.push(header.clone(), &stack);
            let repeats = g.repeat_header && !is_empty_band(&g.header);
            if repeats {
                f.open.push(header);
            }
            self.emit(f, level + 1, run);
            let footer = self.block(
                f,
                &g.footer,
                vec![("record", row_value(run_rows.last())), ("group", group)],
                Some(run_rows),
            );
            let footer = self.counted(f, footer);
            self.push(Rc::new(footer), &stack);
            if repeats {
                f.open.pop();
            }
            start = end;
        }
    }
}

/// Which components of a band instance `band_items` returns.
#[derive(Clone, Copy, PartialEq)]
enum Part {
    /// Every component.
    All,
    /// Everything not below the table, without the table.
    Above,
    /// Components under the table, placed relative to the band origin.
    Below,
    /// Every non-table component at its design position, for repeated headers.
    Repeat,
}

fn table_columns(c: &Comp) -> &[TableColumn] {
    match &c.kind {
        Kind::Table { columns, .. } => columns,
        _ => &[],
    }
}

fn rows_between(c: &Comp, geo: &TableGeometry, from: usize, to: usize, x: f64, mut y: f64) -> Vec<Item> {
    let mut items = vec![];
    for i in from..to {
        items.extend(table_row_items(c, table_columns(c), geo, Some(i), x, y));
        y += geo.row_heights[i];
    }
    items
}

/// Items of one band instance at origin (left, oy).
fn band_items(p: &Prepared, left: f64, oy: f64, page: &PageCtx, env: &Env, part: Part) -> Vec<Item> {
    let scope = p.block.scope.with(&page.overrides());
    let ctx = RenderContext {
        env,
        scope: &scope,
        running: p.block.running.as_ref(),
    };
    let ox = left + p.block.x;
    let table = p.table.as_ref();
    let table_comp = table.map(|t| &p.block.band.components[t.index]);
    let below_start = table_comp.map_or(f64::INFINITY, |c| c.y + c.h);
    let growth = table.map_or(0.0, |t| (t.geo.height - table_comp.map_or(0.0, |c| c.h)).max(0.0));
    let mut out = vec![];
    for (idx, c) in p.block.band.components.iter().enumerate() {
        // Subreports were expanded into blocks of their own.
        if c.is_subreport() {
            continue;
        }
        if !c.is_table() && p.growth.is_some() {
            out.extend(component_items(&grown(c, p.growth.as_ref()), ox, oy, &ctx));
            continue;
        }
        if part == Part::Repeat {
            if !c.is_table() {
                out.extend(component_items(c, ox, oy, &ctx));
            }
            continue;
        }
        if c.is_table() {
            let Some(t) = table.filter(|t| t.index == idx && part == Part::All) else {
                continue;
            };
            out.extend(table_row_items(c, table_columns(c), &t.geo, None, ox + c.x, oy + c.y));
            out.extend(rows_between(
                c,
                &t.geo,
                0,
                t.geo.row_heights.len(),
                ox + c.x,
                oy + c.y + t.geo.header_height,
            ));
            continue;
        }
        let below = c.y >= below_start - EPS;
        if part == Part::Above && below {
            continue;
        }
        if part == Part::Below {
            if below {
                out.extend(component_items(c, ox, oy, &ctx));
            }
            continue;
        }
        let shift = if below && part == Part::All { growth } else { 0.0 };
        out.extend(component_items(c, ox, oy + shift, &ctx));
    }
    out
}

struct Measure<'a> {
    env: &'a Env<'a>,
    body_height: f64,
    prepared_of: RefCell<HashMap<*const Block, Rc<Prepared>>>,
}

impl Measure<'_> {
    /// Repeated headers print their non-table items at design height, or grown height
    /// without a table.
    fn repeated_height(&self, p: &Prepared) -> f64 {
        if p.table.is_some() {
            p.block.band.height
        } else {
            p.height
        }
    }

    fn repeat_height(&self, b: &Block) -> f64 {
        b.repeat
            .iter()
            .map(|h| self.repeated_height(&self.prepare_once(h)))
            .sum()
    }

    fn prepare_once(&self, b: &Rc<Block>) -> Rc<Prepared> {
        if let Some(p) = self.prepared_of.borrow().get(&Rc::as_ptr(b)) {
            return p.clone();
        }
        let p = Rc::new(self.prepare(b));
        self.prepared_of.borrow_mut().insert(Rc::as_ptr(b), p.clone());
        p
    }

    fn prepare(&self, block: &Rc<Block>) -> Prepared {
        let ctx = RenderContext {
            env: self.env,
            scope: &block.scope,
            running: block.running.as_ref(),
        };
        let table_index = block.band.components.iter().position(Comp::is_table);
        let growth = grow_band(&block.band, &ctx);
        let Some(index) = table_index else {
            let h = block.band.height + growth.as_ref().map_or(0.0, |g| g.extra);
            // Grown text splits between lines unless keep_together holds and the band
            // fits a page.
            let splittable = growth.is_some()
                && (!block.band.keep_together || h > self.body_height - self.repeat_height(block) + EPS);
            return Prepared {
                block: block.clone(),
                growth,
                height: h,
                splittable,
                min_first: if splittable { block.band.height } else { h },
                table: None,
            };
        };
        let comp = &block.band.components[index];
        let query_id = match &comp.kind {
            Kind::Table { query_id, .. } => query_id.as_deref(),
            _ => None,
        };
        let geo = measure_table(comp, query_id, table_columns(comp), &ctx);
        let h = block.band.height + (geo.height - comp.h).max(0.0);
        let splittable = !block.band.keep_together || h > self.body_height - self.repeat_height(block) + EPS;
        let min_first = if splittable {
            comp.y + geo.header_height + geo.row_heights.first().copied().unwrap_or(0.0)
        } else {
            h
        };
        Prepared {
            block: block.clone(),
            table: Some(TableInfo { index, geo }),
            growth: None,
            height: h,
            splittable,
            min_first,
        }
    }
}

struct Pager<'a> {
    env: &'a Env<'a>,
    measure: &'a Measure<'a>,
    left: f64,
    body_top: f64,
    body_bottom: f64,
    pages: Vec<Vec<Thunk<'a>>>,
    /// Page-number section of each page (see `groupPage`).
    page_section: Vec<usize>,
    sections: usize,
    cursor: f64,
    /// Cursor after the repeated group headers of the current page.
    page_top: f64,
}

impl<'a> Pager<'a> {
    fn add(&mut self, thunk: Thunk<'a>) {
        if let Some(page) = self.pages.last_mut() {
            page.push(thunk);
        }
    }

    fn new_page(&mut self, repeat: &[Rc<Block>]) {
        self.pages.push(vec![]);
        self.page_section.push(self.sections);
        self.cursor = self.body_top;
        for h in repeat {
            let r = self.measure.prepare_once(h);
            let oy = self.cursor;
            let (env, left) = (self.env, self.left);
            let height = self.measure.repeated_height(&r);
            self.add(Box::new(move |ctx| band_items(&r, left, oy, ctx, env, Part::Repeat)));
            self.cursor += height;
        }
        self.page_top = self.cursor;
    }

    fn at_top(&self) -> bool {
        self.cursor <= self.page_top + EPS
    }

    fn first_need(p: &Prepared) -> f64 {
        if p.splittable {
            p.min_first
        } else {
            p.height
        }
    }

    // Split a band with grown text at line boundaries, continuing on the next pages.
    fn split_grown(&mut self, p: &Rc<Prepared>) {
        let no_page = PageCtx {
            page: 0,
            pages: 0,
            group_page: 0,
            group_pages: 0,
        };
        let shape = band_items(p, self.left, 0.0, &no_page, self.env, Part::All);
        let mut from = 0.0;
        loop {
            let room = self.body_bottom - self.cursor;
            if p.height - from <= room + EPS {
                self.piece(p, from, f64::INFINITY, self.cursor);
                self.cursor += p.height - from;
                return;
            }
            let mut cut = cut_at(&shape, from, from + room);
            if cut <= from + EPS {
                if !self.at_top() {
                    self.new_page(&p.block.repeat);
                    continue;
                }
                // Not even one line fits an empty page: cut at the body bottom.
                cut = from + room;
            }
            self.piece(p, from, cut, self.cursor);
            from = cut;
            self.new_page(&p.block.repeat);
        }
    }

    fn piece(&mut self, p: &Rc<Prepared>, from: f64, to: f64, oy: f64) {
        let (p, env, left) = (p.clone(), self.env, self.left);
        self.add(Box::new(move |ctx| {
            slice_items(&band_items(&p, left, 0.0, ctx, env, Part::All), from, to, oy - from)
        }));
    }

    // A block that fits a page only without the repeated headers gets a page without them.
    fn page_for(&mut self, p: &Prepared) {
        let without = Self::first_need(p) > self.measure.body_height - self.measure.repeat_height(&p.block) + EPS;
        let repeat = if without { vec![] } else { p.block.repeat.clone() };
        self.new_page(&repeat);
    }
}

/// Lays out a report into pages. Rows are sorted (stable) by the group keys; group
/// headers and footers see the group's rows as `rows`; page headers and footers repeat
/// on every page; `page` and `pages` are resolved after pagination.
pub fn layout_report(report: &Report, input_rows: &[Row], options: &LayoutOptions) -> ReportDocument {
    let rep = Rc::new(Rep::from_stored(report));
    let (width, height) = rep.page.dimensions();
    let m = rep.page.margins.clone();
    let bands = &rep.bands;
    let params = options.params.clone().unwrap_or_else(|| rep.params.clone());
    let env = Env {
        now: options.now,
        tables: &options.tables,
        assets: &options.assets,
        diagnostics: Diagnostics::default(),
    };
    let loaded: Loaded = options
        .subreports
        .iter()
        .map(|(id, d)| (id.clone(), (Rc::new(Rep::from_stored(&d.report)), d.rows.clone())))
        .collect();
    let mut entries = Map::new();
    entries.insert("params".into(), Value::Object(params.clone()));
    entries.insert("report".into(), json!({ "name": rep.name }));
    for name in ["record", "group", "rowNumber", "parent", "page", "pages", "groupPage", "groupPages"] {
        entries.insert(name.into(), Value::Null);
    }
    let base = ScopeData {
        entries,
        rows: Rc::new(input_rows.to_vec()),
    };

    // 1-2. Sort rows by group keys (stable) and flatten into band instances.
    let blocks = {
        let mut flattener = Flattener {
            env: &env,
            loaded: &loaded,
            base: &base,
            params: &params,
            blocks: vec![],
        };
        flattener.flatten(rep.clone(), input_rows.to_vec(), base.clone(), 0.0, vec![], vec![rep.id.clone()]);
        flattener.blocks
    };

    // 3. Measure.
    let body_top = m.top + bands.page_header.height;
    let body_bottom = height - m.bottom - bands.page_footer.height;
    let body_height = body_bottom - body_top;
    let measure = Measure {
        env: &env,
        body_height,
        prepared_of: RefCell::new(HashMap::new()),
    };
    let prepared: Vec<Rc<Prepared>> = blocks.iter().map(|b| measure.prepare_once(b)).collect();

    // 4. Paginate.
    let mut pager = Pager {
        env: &env,
        measure: &measure,
        left: m.left,
        body_top,
        body_bottom,
        pages: vec![],
        page_section: vec![],
        sections: 0,
        cursor: body_top,
        page_top: body_top,
    };
    pager.new_page(&[]);
    let mut pending_break = false;
    for i in 0..prepared.len() {
        let p = &prepared[i];
        let mut need = Pager::first_need(p);
        if p.block.keep_with_next {
            need = 0.0;
            let mut j = i;
            while j < prepared.len() && prepared[j].block.keep_with_next {
                need += prepared[j].height;
                j += 1;
            }
            if j < prepared.len() {
                need += Pager::first_need(&prepared[j]);
            }
        }
        if (pending_break || p.block.break_before) && !pager.at_top() {
            pager.page_for(p);
        }
        pending_break = p.block.break_after;
        if p.block.section {
            pager.sections += 1;
            let last = pager.pages.len() - 1;
            pager.page_section[last] = pager.sections;
        }
        if pager.cursor + need > pager.body_bottom + EPS && !pager.at_top() {
            pager.page_for(p);
        }
        if p.growth.is_some() && p.splittable && pager.cursor + p.height > pager.body_bottom + EPS {
            pager.split_grown(p);
            continue;
        }
        if p.table.is_none() || pager.cursor + p.height <= pager.body_bottom + EPS || !p.splittable {
            let oy = pager.cursor;
            let (q, env, left) = (p.clone(), &env, pager.left);
            pager.add(Box::new(move |ctx| band_items(&q, left, oy, ctx, env, Part::All)));
            pager.cursor += p.height;
            continue;
        }
        paginate_table(&mut pager, p);
    }

    // 5. Render with page numbers known.
    let Pager {
        pages,
        page_section,
        left,
        ..
    } = pager;
    let total = pages.len();
    let page_band = |band: &Band, oy: f64, ctx: &PageCtx| -> Vec<Item> {
        let scope = base.with(&ctx.overrides());
        let rc = RenderContext {
            env: &env,
            scope: &scope,
            running: None,
        };
        let mut items = vec![];
        for c in &band.components {
            if c.is_subreport() {
                env.diagnostics.add(&c.id, SUBREPORT_PAGE_BAND);
            } else if c.is_table() {
                env.diagnostics.add(&c.id, PAGE_BAND_TABLE);
            } else {
                items.extend(component_items(c, left, oy, &rc));
            }
        }
        items
    };
    let out: Vec<Page> = pages
        .iter()
        .enumerate()
        .map(|(index, thunks)| {
            let section = page_section[index];
            let first = page_section.iter().position(|s| *s == section).unwrap_or(0);
            let last = page_section.iter().rposition(|s| *s == section).unwrap_or(first);
            let ctx = PageCtx {
                page: index + 1,
                pages: total,
                group_page: index - first + 1,
                group_pages: last - first + 1,
            };
            let mut items = page_band(&bands.page_header, m.top, &ctx);
            for thunk in thunks {
                items.extend(thunk(&ctx));
            }
            items.extend(page_band(&bands.page_footer, height - m.bottom - bands.page_footer.height, &ctx));
            Page {
                number: index + 1,
                items,
            }
        })
        .collect();
    drop(pages);
    ReportDocument {
        width,
        height,
        pages: out,
        diagnostics: env.diagnostics.into_vec(),
    }
}

// Split a table band across pages, repeating the header row.
fn paginate_table<'a>(pager: &mut Pager<'a>, p: &Rc<Prepared>) {
    let Some(table) = p.table.as_ref() else {
        return;
    };
    let band = &p.block.band;
    let comp = &band.components[table.index];
    let geo = &table.geo;
    let (env, left) = (pager.env, pager.left);
    let origin = pager.cursor;
    {
        let p = p.clone();
        pager.add(Box::new(move |ctx| band_items(&p, left, origin, ctx, env, Part::Above)));
    }
    let mut y = origin + comp.y;
    let mut row = 0usize;
    let mut chunk_at_top = origin + comp.y <= pager.page_top + EPS;
    let mut broke = false;
    loop {
        let top = y;
        let first = row;
        y += geo.header_height;
        while row < geo.row_heights.len()
            && (y + geo.row_heights[row] <= pager.body_bottom + EPS || (row == first && chunk_at_top))
        {
            y += geo.row_heights[row];
            row += 1;
        }
        let last = row;
        {
            let (p, x) = (p.clone(), left + p.block.x + comp.x);
            pager.add(Box::new(move |_| {
                let Some(t) = p.table.as_ref() else {
                    return vec![];
                };
                let c = &p.block.band.components[t.index];
                let mut items = table_row_items(c, table_columns(c), &t.geo, None, x, top);
                items.extend(rows_between(c, &t.geo, first, last, x, top + t.geo.header_height));
                items
            }));
        }
        if row >= geo.row_heights.len() {
            break;
        }
        pager.new_page(&p.block.repeat);
        broke = true;
        chunk_at_top = true;
        y = pager.cursor;
    }
    let below_start = comp.y + comp.h;
    if !broke {
        y = y.max(origin + below_start);
    }
    let rest = band.height - below_start;
    let has_below = band
        .components
        .iter()
        .enumerate()
        .any(|(i, c)| i != table.index && c.y >= below_start - EPS);
    if has_below && y + rest > pager.body_bottom + EPS && y > pager.page_top + EPS {
        pager.new_page(&p.block.repeat);
        y = pager.cursor;
    }
    let oy = y - below_start;
    if has_below {
        let p = p.clone();
        pager.add(Box::new(move |ctx| band_items(&p, left, oy, ctx, env, Part::Below)));
    }
    pager.cursor = y + rest.max(0.0);
}
