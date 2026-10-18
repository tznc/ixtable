//! Shared grid engine for forms and dashboards. Pure functions only: no GPUI.
//! Port of `src/grid/engine.ts`.
//!
//! Breakpoint rule: breakpoints are mobile-first. For a container width `w`, the active
//! column set is the breakpoint with the largest `min_width <= w` (ties: the later one in
//! the list). Breakpoints with no columns are ignored. With no match, or no width, the base
//! `layout.columns` apply. Placements and regions are always authored against the base
//! columns; breakpoints only change how they render.
//!
//! Wrapping rule (applied separately to placements and to named regions):
//! 1. If every rectangle fits the active column count, positions are kept as authored.
//! 2. Otherwise the set reflows. Rectangles are visited in authored reading order (row,
//!    column, index). Each column span is clamped to the active column count; the row span
//!    is kept. Each is placed at the first free slot scanning from (authored row + shift,
//!    column) to the end of that row, then row by row from column 1. `shift` is the largest
//!    number of rows any earlier rectangle was pushed down.
//!
//! Rust stores positions as `u16`, so the TypeScript checks for fractional or negative
//! values have no counterpart here: the type already rules them out.

use crate::design::{
    Breakpoint, GridAlign, GridLayout, GridTrack, NamedRegion, Placement, TrackKind,
};
use crate::validation::Issue;
use std::collections::HashSet;

const MAX_U16: i64 = 65535;
const RESERVED_IDENTS: [&str; 7] = ["auto", "span", "none", "inherit", "initial", "unset", "revert"];

/// A rectangle of 1-based cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub column: i64,
    pub row: i64,
    pub column_span: i64,
    pub row_span: i64,
}

impl Rect {
    pub fn new(column: i64, row: i64, column_span: i64, row_span: i64) -> Self {
        Self {
            column,
            row,
            column_span,
            row_span,
        }
    }
}

/// Anything with a grid rectangle that reflow can move.
pub trait HasRect: Clone {
    fn rect(&self) -> Rect;
    fn with_rect(&self, rect: Rect) -> Self;
}

fn to_u16(value: i64) -> u16 {
    value.clamp(0, MAX_U16) as u16
}

impl HasRect for Placement {
    fn rect(&self) -> Rect {
        Rect::new(
            self.column as i64,
            self.row as i64,
            self.column_span as i64,
            self.row_span as i64,
        )
    }
    fn with_rect(&self, rect: Rect) -> Self {
        Self {
            column: to_u16(rect.column),
            row: to_u16(rect.row),
            column_span: to_u16(rect.column_span),
            row_span: to_u16(rect.row_span),
            region: self.region.clone(),
        }
    }
}

impl HasRect for NamedRegion {
    fn rect(&self) -> Rect {
        Rect::new(
            self.column as i64,
            self.row as i64,
            self.column_span as i64,
            self.row_span as i64,
        )
    }
    fn with_rect(&self, rect: Rect) -> Self {
        Self {
            name: self.name.clone(),
            column: to_u16(rect.column),
            row: to_u16(rect.row),
            column_span: to_u16(rect.column_span),
            row_span: to_u16(rect.row_span),
        }
    }
}

/// A named region as rendered at a given width (after wrapping).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedRegion {
    pub region: NamedRegion,
    /// CSS-style identifier used in the area template (the name when it is a safe ident).
    pub ident: String,
    /// False when the region overlaps an earlier region and is left out of the template.
    pub in_template: bool,
}

/// Where one item sits: a named area or column/row line spans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemCss {
    Area(String),
    Lines {
        column: i64,
        column_span: i64,
        row: i64,
        row_span: i64,
    },
}

impl ItemCss {
    /// The CSS text (`grid-area`, or `grid-column` and `grid-row` joined by `; `).
    pub fn css(&self) -> String {
        match self {
            ItemCss::Area(name) => name.clone(),
            ItemCss::Lines {
                column,
                column_span,
                row,
                row_span,
            } => format!("{column} / span {column_span}; {row} / span {row_span}"),
        }
    }
}

/// Container settings derived from a layout at a width.
#[derive(Clone, Debug, PartialEq)]
pub struct ContainerCss {
    pub columns: Vec<String>,
    pub rows: Vec<String>,
    pub areas: Option<String>,
    pub column_gap: u16,
    pub row_gap: u16,
    pub padding: u16,
    pub justify_items: GridAlign,
    pub align_items: GridAlign,
}

/// Resize limits supplied by the caller. Not persisted; the grid always wins.
#[derive(Clone, Copy, Debug, Default)]
pub struct SpanConstraints {
    pub min_column_span: Option<i64>,
    pub max_column_span: Option<i64>,
    pub min_row_span: Option<i64>,
    pub max_row_span: Option<i64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GridDelta {
    pub columns: i64,
    pub rows: i64,
}

/// A placed item, identified by a stable id.
#[derive(Clone, Debug)]
pub struct GridItemRef {
    pub id: String,
    pub placement: Placement,
}

pub fn default_grid_layout() -> GridLayout {
    GridLayout::default()
}

/// One track to CSS. `fixed` is clamped px; `content` is `auto`; `fr` is `Nfr`. With min or
/// max, `content` and `fr` become `minmax(min, max)`.
pub fn track_to_css(track: &GridTrack) -> String {
    match &track.kind {
        TrackKind::Fixed => {
            let lo = track.min.unwrap_or(f64::NEG_INFINITY);
            let hi = track.max.unwrap_or(f64::INFINITY);
            let size = track.value.unwrap_or(0.0).max(lo).min(hi);
            px(size)
        }
        kind => {
            let fr = *kind == TrackKind::Fr;
            let base = if fr {
                format!("{}fr", num(track.value.unwrap_or(1.0)))
            } else {
                "auto".to_string()
            };
            if track.min.is_none() && track.max.is_none() {
                return base;
            }
            let min = match track.min {
                Some(m) => px(m),
                None if fr => "0px".into(),
                None => "auto".into(),
            };
            let max = track.max.map(px).unwrap_or(base);
            format!("minmax({min}, {max})")
        }
    }
}

fn num(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

fn px(value: f64) -> String {
    format!("{}px", num(value))
}

/// Active column tracks for a container width; the index is -1 for the base columns.
pub fn resolve_breakpoint(layout: &GridLayout, width: Option<f64>) -> (Vec<GridTrack>, i32) {
    let mut index: i32 = -1;
    if let Some(width) = width.filter(|w| w.is_finite()) {
        let mut best = f64::NEG_INFINITY;
        for (i, bp) in layout.breakpoints.iter().enumerate() {
            let min = bp.min_width as f64;
            if !bp.columns.is_empty() && min <= width && min >= best {
                best = min;
                index = i as i32;
            }
        }
    }
    let columns = if index < 0 {
        &layout.columns
    } else {
        &layout.breakpoints[index as usize].columns
    };
    if columns.is_empty() {
        (vec![GridTrack::fr(1.0)], index)
    } else {
        (columns.clone(), index)
    }
}

fn active_count(layout: &GridLayout, width: Option<f64>) -> i64 {
    resolve_breakpoint(layout, width).0.len() as i64
}

fn rect_fits(rect: Rect, count: i64) -> bool {
    rect.column >= 1
        && rect.row >= 1
        && rect.column_span >= 1
        && rect.row_span >= 1
        && rect.column + rect.column_span - 1 <= count
}

fn cells_of(rect: Rect) -> impl Iterator<Item = (i64, i64)> {
    (rect.row..rect.row + rect.row_span)
        .flat_map(move |r| (rect.column..rect.column + rect.column_span).map(move |c| (r, c)))
}

fn is_free(occupied: &HashSet<(i64, i64)>, rect: Rect) -> bool {
    cells_of(rect).all(|cell| !occupied.contains(&cell))
}

fn occupy(occupied: &mut HashSet<(i64, i64)>, rect: Rect) {
    occupied.extend(cells_of(rect));
}

/// Applies the wrapping rule documented at the top of this file.
pub fn reflow<T: HasRect>(items: &[T], count: i64) -> Vec<T> {
    let count = count.max(1);
    if items.iter().all(|item| rect_fits(item.rect(), count)) {
        return items.to_vec();
    }
    let mut order: Vec<usize> = (0..items.len()).collect();
    order.sort_by_key(|&i| {
        let r = items[i].rect();
        (r.row, r.column, i)
    });
    let mut occupied = HashSet::new();
    let mut out = items.to_vec();
    let mut shift = 0;
    for i in order {
        let source = items[i].rect();
        let authored_row = source.row.max(1);
        let column_span = source.column_span.clamp(1, count);
        let row_span = source.row_span.max(1);
        let last_column = count - column_span + 1;
        let mut rect = Rect::new(
            source.column.clamp(1, last_column),
            authored_row + shift,
            column_span,
            row_span,
        );
        while !is_free(&occupied, rect) {
            rect.column += 1;
            if rect.column > last_column {
                rect.column = 1;
                rect.row += 1;
            }
        }
        occupy(&mut occupied, rect);
        shift = shift.max(rect.row - authored_row);
        out[i] = items[i].with_rect(rect);
    }
    out
}

fn is_safe_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    first_ok
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !RESERVED_IDENTS.contains(&name.to_ascii_lowercase().as_str())
}

/// Named regions as rendered at `width`, with their identifiers.
pub fn resolve_regions(layout: &GridLayout, width: Option<f64>) -> Vec<ResolvedRegion> {
    let count = active_count(layout, width);
    let mut occupied = HashSet::new();
    let mut seen: HashSet<String> = HashSet::new();
    reflow(&layout.named_regions, count)
        .into_iter()
        .enumerate()
        .map(|(index, region)| {
            let rect = region.rect();
            let in_template = !region.name.is_empty()
                && !seen.contains(&region.name)
                && rect_fits(rect, count)
                && is_free(&occupied, rect);
            if in_template {
                occupy(&mut occupied, rect);
            }
            seen.insert(region.name.clone());
            let ident = if is_safe_ident(&region.name) {
                region.name.clone()
            } else {
                format!("_{index}")
            };
            ResolvedRegion {
                region,
                ident,
                in_template,
            }
        })
        .collect()
}

fn template_areas(regions: &[ResolvedRegion], count: i64) -> Option<String> {
    let placed: Vec<&ResolvedRegion> = regions.iter().filter(|r| r.in_template).collect();
    if placed.is_empty() {
        return None;
    }
    let rows = placed
        .iter()
        .map(|r| r.region.row as i64 + r.region.row_span as i64 - 1)
        .max()
        .unwrap_or(0);
    let mut matrix = vec![vec![".".to_string(); count as usize]; rows as usize];
    for region in placed {
        for (r, c) in cells_of(region.region.rect()) {
            matrix[(r - 1) as usize][(c - 1) as usize] = region.ident.clone();
        }
    }
    Some(
        matrix
            .iter()
            .map(|line| format!("\"{}\"", line.join(" ")))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// Container settings for a layout at an optional container width (content-box pixels).
pub fn layout_to_css(layout: &GridLayout, width: Option<f64>) -> ContainerCss {
    let (columns, _) = resolve_breakpoint(layout, width);
    let areas = template_areas(&resolve_regions(layout, width), columns.len() as i64);
    ContainerCss {
        columns: columns.iter().map(track_to_css).collect(),
        rows: layout.rows.iter().map(track_to_css).collect(),
        areas,
        column_gap: layout.column_gap,
        row_gap: layout.row_gap,
        padding: layout.padding,
        justify_items: layout.justify_items.clone(),
        align_items: layout.align_items.clone(),
    }
}

fn lines(rect: Rect) -> ItemCss {
    ItemCss::Lines {
        column: rect.column,
        column_span: rect.column_span,
        row: rect.row,
        row_span: rect.row_span,
    }
}

fn clamp_rect(rect: Rect, count: i64) -> Rect {
    let column_span = rect.column_span.clamp(1, count.max(1));
    Rect::new(
        rect.column.clamp(1, (count - column_span + 1).max(1)),
        rect.row.max(1),
        column_span,
        rect.row_span.max(1),
    )
}

fn item_css(placement: &Placement, regions: &[ResolvedRegion], count: i64) -> ItemCss {
    let region = placement
        .region
        .as_ref()
        .and_then(|name| regions.iter().find(|r| &r.region.name == name));
    if let Some(region) = region {
        if region.in_template {
            return ItemCss::Area(region.ident.clone());
        }
    }
    let rect = region.map_or(placement.rect(), |r| r.region.rect());
    lines(clamp_rect(rect, count))
}

/// Placement for one item. Region placements use the area; others use line spans clamped to
/// the active columns. `layout: None` uses a region name as-is when it is a safe identifier.
pub fn placement_to_css(
    placement: &Placement,
    layout: Option<&GridLayout>,
    width: Option<f64>,
) -> ItemCss {
    match layout {
        None => match &placement.region {
            Some(name) if is_safe_ident(name) => ItemCss::Area(name.clone()),
            _ => lines(clamp_rect(placement.rect(), i64::MAX / 4)),
        },
        Some(layout) => item_css(
            placement,
            &resolve_regions(layout, width),
            active_count(layout, width),
        ),
    }
}

/// Placements as rendered at `width`: explicit placements wrap; region placements are kept.
pub fn resolve_placements(
    layout: &GridLayout,
    placements: &[Placement],
    width: Option<f64>,
) -> Vec<Placement> {
    let names: HashSet<&str> = layout.named_regions.iter().map(|r| r.name.as_str()).collect();
    let uses_region = |p: &Placement| p.region.as_deref().is_some_and(|r| names.contains(r));
    let explicit: Vec<Placement> = placements
        .iter()
        .filter(|p| !uses_region(p))
        .cloned()
        .collect();
    let wrapped = reflow(&explicit, active_count(layout, width));
    let mut next = wrapped.into_iter();
    placements
        .iter()
        .map(|p| {
            if uses_region(p) {
                p.clone()
            } else {
                next.next().unwrap_or_else(|| p.clone())
            }
        })
        .collect()
}

/// Item placement for a set of siblings at `width`, with wrapping applied.
pub fn placements_to_css(
    layout: &GridLayout,
    placements: &[Placement],
    width: Option<f64>,
) -> Vec<ItemCss> {
    let regions = resolve_regions(layout, width);
    let count = active_count(layout, width);
    resolve_placements(layout, placements, width)
        .iter()
        .map(|p| item_css(p, &regions, count))
        .collect()
}

/// Grows or shrinks a placement by whole tracks within the base columns and the limits.
/// Region placements are sized by their region and are returned unchanged.
pub fn resize_placement(
    placement: &Placement,
    delta: GridDelta,
    layout: &GridLayout,
    constraints: SpanConstraints,
) -> Placement {
    if placement.region.is_some() {
        return placement.clone();
    }
    let count = (layout.columns.len() as i64).max(1);
    let column = (placement.column as i64).clamp(1, count);
    let max_columns = (count - column + 1)
        .min(constraints.max_column_span.unwrap_or(count))
        .max(1);
    let min_columns = constraints
        .min_column_span
        .unwrap_or(1)
        .clamp(1, max_columns);
    let max_rows = constraints
        .max_row_span
        .unwrap_or(MAX_U16)
        .min(MAX_U16)
        .max(1);
    let min_rows = constraints.min_row_span.unwrap_or(1).clamp(1, max_rows);
    Placement {
        column: to_u16(column),
        column_span: to_u16(
            (placement.column_span as i64 + delta.columns).clamp(min_columns, max_columns),
        ),
        row_span: to_u16((placement.row_span as i64 + delta.rows).clamp(min_rows, max_rows)),
        ..placement.clone()
    }
}

/// Moves a placement by whole tracks, keeping it inside the base columns with row >= 1.
pub fn move_placement(placement: &Placement, delta: GridDelta, layout: &GridLayout) -> Placement {
    if placement.region.is_some() {
        return placement.clone();
    }
    let count = (layout.columns.len() as i64).max(1);
    let column_span = (placement.column_span as i64).clamp(1, count);
    Placement {
        column_span: to_u16(column_span),
        column: to_u16((placement.column as i64 + delta.columns).clamp(1, count - column_span + 1)),
        row: to_u16((placement.row as i64 + delta.rows).clamp(1, MAX_U16)),
        ..placement.clone()
    }
}

fn rect_of(placement: &Placement, layout: Option<&GridLayout>) -> Option<Rect> {
    match &placement.region {
        None => Some(placement.rect()),
        Some(name) => layout?
            .named_regions
            .iter()
            .find(|r| &r.name == name)
            .map(|r| r.rect()),
    }
}

fn intersects(a: Rect, b: Rect) -> bool {
    a.column < b.column + b.column_span
        && b.column < a.column + a.column_span
        && a.row < b.row + b.row_span
        && b.row < a.row + a.row_span
}

/// First free slot (row-major from row 1, column 1) in the base columns for a new item.
/// The default size is the full width and one row.
pub fn next_placement(
    layout: &GridLayout,
    existing: &[Placement],
    column_span: Option<i64>,
    row_span: Option<i64>,
) -> Placement {
    let count = (layout.columns.len() as i64).max(1);
    let mut occupied = HashSet::new();
    for placement in existing {
        if let Some(rect) = rect_of(placement, Some(layout)) {
            occupy(&mut occupied, rect);
        }
    }
    let mut rect = Rect::new(
        1,
        1,
        column_span.unwrap_or(count).clamp(1, count),
        row_span.unwrap_or(1).max(1),
    );
    while !is_free(&occupied, rect) {
        rect.column += 1;
        if rect.column > count - rect.column_span + 1 {
            rect.column = 1;
            rect.row += 1;
        }
    }
    Placement::default().with_rect(rect)
}

/// Pairs of item ids whose authored cells overlap (items in the same region overlap).
pub fn detect_overlaps(
    items: &[GridItemRef],
    layout: Option<&GridLayout>,
) -> Vec<(String, String)> {
    let rects: Vec<Option<Rect>> = items.iter().map(|i| rect_of(&i.placement, layout)).collect();
    let mut out = vec![];
    for i in 0..items.len() {
        for j in i + 1..items.len() {
            let (a, b) = (&items[i].placement, &items[j].placement);
            let same_region = a.region.is_some() && a.region == b.region;
            let hit = matches!((rects[i], rects[j]), (Some(ra), Some(rb)) if intersects(ra, rb));
            if same_region || hit {
                out.push((items[i].id.clone(), items[j].id.clone()));
            }
        }
    }
    out
}

struct Target<'a> {
    kind: &'a str,
    id: &'a str,
}

fn track_issues(tracks: &[GridTrack], label: &str, t: &Target) -> Vec<Issue> {
    let mut issues = vec![];
    let mut error = |m: String| issues.push(Issue::error(t.kind, t.id, m));
    if tracks.is_empty() && label == "columns" {
        error("grid layout requires at least one column track".into());
    }
    for track in tracks {
        if track.kind != TrackKind::Content && track.value.unwrap_or(0.0) <= 0.0 {
            error(format!("{label} tracks need a positive size"));
        }
        if let (Some(min), Some(max)) = (track.min, track.max) {
            if min > max {
                error(format!("{label} track minimum cannot exceed maximum"));
            }
        }
    }
    issues
}

fn span_issue(rect: Rect, columns: i64) -> Option<&'static str> {
    if [rect.column, rect.row, rect.column_span, rect.row_span].contains(&0) {
        return Some("grid placement uses 1-based positions and positive spans");
    }
    if rect.column + rect.column_span - 1 > columns {
        return Some("grid placement exceeds declared columns");
    }
    None
}

fn breakpoint_issues(breakpoints: &[Breakpoint], t: &Target) -> Vec<Issue> {
    let mut issues = vec![];
    let mut widths = HashSet::new();
    for bp in breakpoints {
        issues.extend(track_issues(&bp.columns, "breakpoint columns", t));
        if bp.columns.is_empty() {
            issues.push(Issue::warning(
                t.kind,
                t.id,
                "breakpoint has no columns and is ignored",
            ));
        }
        if !widths.insert(bp.min_width) {
            issues.push(Issue::warning(
                t.kind,
                t.id,
                format!(
                    "more than one breakpoint starts at {}px; the last one wins",
                    bp.min_width
                ),
            ));
        }
    }
    issues
}

fn region_issues(regions: &[NamedRegion], columns: i64, t: &Target) -> Vec<Issue> {
    let mut issues = vec![];
    let mut names = HashSet::new();
    for (index, region) in regions.iter().enumerate() {
        if region.name.is_empty() || !names.insert(region.name.clone()) {
            issues.push(Issue::error(
                t.kind,
                t.id,
                "named grid regions must be unique",
            ));
        }
        if let Some(m) = span_issue(region.rect(), columns) {
            issues.push(Issue::error(
                t.kind,
                t.id,
                format!("region {}: {m}", region.name),
            ));
        }
        if let Some(clash) = regions[..index]
            .iter()
            .find(|o| intersects(o.rect(), region.rect()))
        {
            issues.push(Issue::warning(
                t.kind,
                t.id,
                format!(
                    "region {} overlaps region {} and is left out of the template",
                    region.name, clash.name
                ),
            ));
        }
    }
    issues
}

/// Mirrors the grid rules of the Rust design validation as errors; rendering problems Rust
/// accepts (overlapping items or regions, ignored breakpoints) are warnings.
pub fn validate_layout(
    layout: &GridLayout,
    items: &[GridItemRef],
    object_kind: &str,
    object_id: &str,
    item_kind: &str,
) -> Vec<Issue> {
    let t = Target {
        kind: object_kind,
        id: object_id,
    };
    let columns = layout.columns.len() as i64;
    let mut issues = track_issues(&layout.columns, "columns", &t);
    issues.extend(track_issues(&layout.rows, "rows", &t));
    issues.extend(breakpoint_issues(&layout.breakpoints, &t));
    issues.extend(region_issues(&layout.named_regions, columns, &t));
    let names: HashSet<&str> = layout.named_regions.iter().map(|r| r.name.as_str()).collect();
    for item in items {
        let p = &item.placement;
        let message = match &p.region {
            Some(region) if names.contains(region.as_str()) => None,
            Some(_) => Some("placement references an unknown region"),
            None => span_issue(p.rect(), columns),
        };
        if let Some(m) = message {
            issues.push(Issue::error(item_kind, &item.id, m));
        }
    }
    for (a, b) in detect_overlaps(items, Some(layout)) {
        issues.push(Issue::warning(item_kind, &a, format!("overlaps {b}")));
    }
    issues
}

/// Keeps a region inside a grid with `columns` base columns.
pub fn clamp_region(region: &NamedRegion, columns: usize) -> NamedRegion {
    let count = (columns as i64).max(1);
    let column = (region.column as i64).clamp(1, count);
    NamedRegion {
        name: region.name.clone(),
        column: to_u16(column),
        row: region.row.max(1),
        column_span: to_u16((region.column_span as i64).clamp(1, count - column + 1)),
        row_span: region.row_span.max(1),
    }
}

pub fn clamp_regions(regions: &[NamedRegion], columns: usize) -> Vec<NamedRegion> {
    regions.iter().map(|r| clamp_region(r, columns)).collect()
}

/// Follows a placement's region through renames (old, new); a region that no longer exists
/// is cleared so the item falls back to its column and row placement.
pub fn remap_region(
    placement: &Placement,
    regions: &[NamedRegion],
    renames: &[(String, String)],
) -> Placement {
    let Some(current) = &placement.region else {
        return placement.clone();
    };
    let name = renames
        .iter()
        .find(|(from, _)| from == current)
        .map_or(current.clone(), |(_, to)| to.clone());
    let exists = regions.iter().any(|r| r.name == name);
    Placement {
        region: exists.then_some(name),
        ..placement.clone()
    }
}

/// Why `name` cannot name region `index` ("" when it can).
pub fn region_name_problem(regions: &[NamedRegion], index: usize, name: &str) -> String {
    if name.trim().is_empty() {
        return "Area name is required.".into();
    }
    if name != name.trim() {
        return "Area name cannot start or end with spaces.".into();
    }
    if regions
        .iter()
        .enumerate()
        .any(|(i, r)| i != index && r.name == name)
    {
        return format!("Another area is already named {name}.");
    }
    String::new()
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
