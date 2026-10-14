//! Port of tests/unit/grid.test.ts and the region helpers of grid-regions.test.ts.

use super::{
    GridDelta, GridItemRef, ItemCss, SpanConstraints, clamp_region, default_grid_layout,
    detect_overlaps, layout_to_css, move_placement, next_placement, placement_to_css,
    placements_to_css, region_name_problem, remap_region, resize_placement, resolve_breakpoint,
    resolve_placements, resolve_regions, track_to_css, validate_layout,
};
use crate::design::{Breakpoint, GridLayout, GridTrack, NamedRegion, Placement, TrackKind};
use serde_json::json;

fn fr(value: f64) -> GridTrack {
    GridTrack::fr(value)
}

fn track(kind: TrackKind, value: Option<f64>, min: Option<f64>, max: Option<f64>) -> GridTrack {
    GridTrack {
        kind,
        value,
        min,
        max,
    }
}

fn at(column: u16, row: u16, column_span: u16, row_span: u16) -> Placement {
    Placement {
        column,
        row,
        column_span,
        row_span,
        region: None,
    }
}

fn in_region(name: &str) -> Placement {
    Placement {
        region: Some(name.into()),
        ..at(1, 1, 1, 1)
    }
}

fn golden() -> GridLayout {
    serde_json::from_value(json!({
        "columns": [
            {"kind": "fixed", "value": 120},
            {"kind": "content"},
            {"kind": "fr", "value": 2, "min": 80},
            {"kind": "fr", "value": 1, "max": 300}
        ],
        "rows": [{"kind": "content"}, {"kind": "fixed", "value": 48, "min": 60}],
        "columnGap": 12,
        "rowGap": 8,
        "padding": 4,
        "justifyItems": "start",
        "alignItems": "center",
        "namedRegions": [
            {"name": "header", "column": 1, "row": 1, "columnSpan": 4, "rowSpan": 1},
            {"name": "side", "column": 1, "row": 2, "columnSpan": 1, "rowSpan": 2},
            {"name": "main", "column": 2, "row": 2, "columnSpan": 3, "rowSpan": 1}
        ],
        "breakpoints": [
            {"minWidth": 0, "columns": [{"kind": "fr", "value": 1}]},
            {"minWidth": 600, "columns": [{"kind": "fr", "value": 1}, {"kind": "fr", "value": 1}]},
            {"minWidth": 900, "columns": [
                {"kind": "fixed", "value": 120}, {"kind": "content"},
                {"kind": "fr", "value": 2, "min": 80}, {"kind": "fr", "value": 1, "max": 300}
            ]}
        ]
    }))
    .expect("golden layout")
}

fn region(name: &str, column: u16, column_span: u16) -> NamedRegion {
    NamedRegion {
        name: name.into(),
        column,
        row: 1,
        column_span,
        row_span: 1,
    }
}

fn lines(column: i64, column_span: i64, row: i64, row_span: i64) -> ItemCss {
    ItemCss::Lines {
        column,
        column_span,
        row,
        row_span,
    }
}

#[test]
fn derives_every_track_kind_with_min_and_max() {
    let t = |k, v, mi, ma| track_to_css(&track(k, v, mi, ma));
    assert_eq!(t(TrackKind::Fixed, Some(120.0), None, None), "120px");
    assert_eq!(t(TrackKind::Fixed, Some(120.0), Some(150.0), None), "150px");
    assert_eq!(t(TrackKind::Fixed, Some(120.0), None, Some(100.0)), "100px");
    assert_eq!(t(TrackKind::Content, None, None, None), "auto");
    assert_eq!(
        t(TrackKind::Content, None, Some(40.0), Some(200.0)),
        "minmax(40px, 200px)"
    );
    assert_eq!(
        t(TrackKind::Content, None, None, Some(200.0)),
        "minmax(auto, 200px)"
    );
    assert_eq!(t(TrackKind::Fr, Some(2.0), None, None), "2fr");
    assert_eq!(
        t(TrackKind::Fr, Some(2.0), Some(80.0), None),
        "minmax(80px, 2fr)"
    );
    assert_eq!(
        t(TrackKind::Fr, Some(1.0), None, Some(300.0)),
        "minmax(0px, 300px)"
    );
}

#[test]
fn turns_the_golden_layout_into_container_css_with_no_width() {
    let css = layout_to_css(&golden(), None);
    assert_eq!(
        css.columns.join(" "),
        "120px auto minmax(80px, 2fr) minmax(0px, 300px)"
    );
    assert_eq!(css.rows.join(" "), "auto 60px");
    assert_eq!(
        css.areas.as_deref(),
        Some("\"header header header header\" \"side main main main\" \"side . . .\"")
    );
    assert_eq!((css.column_gap, css.row_gap, css.padding), (12, 8, 4));
}

#[test]
fn matches_the_default_twelve_column_layout() {
    let css = layout_to_css(&default_grid_layout(), Some(800.0));
    assert_eq!(css.columns, vec!["1fr".to_string(); 12]);
    assert!(css.rows.is_empty());
    assert_eq!(css.areas, None);
    assert_eq!((css.column_gap, css.row_gap, css.padding), (16, 16, 0));
}

#[test]
fn placement_style_helpers() {
    assert_eq!(
        placement_to_css(&at(2, 3, 4, 2), None, None).css(),
        "2 / span 4; 3 / span 2"
    );
    assert_eq!(
        placement_to_css(&in_region("main"), None, None),
        ItemCss::Area("main".into())
    );
    assert_eq!(
        placement_to_css(&in_region("main"), Some(&golden()), None),
        ItemCss::Area("main".into())
    );
}

#[test]
fn breakpoint_picks_the_largest_min_width_at_or_below_the_width() {
    let layout = golden();
    assert_eq!(resolve_breakpoint(&layout, None).1, -1);
    assert_eq!(resolve_breakpoint(&layout, Some(0.0)).1, 0);
    assert_eq!(resolve_breakpoint(&layout, Some(599.0)).1, 0);
    assert_eq!(resolve_breakpoint(&layout, Some(600.0)).1, 1);
    assert_eq!(resolve_breakpoint(&layout, Some(2000.0)).1, 2);
    assert_eq!(
        layout_to_css(&layout, Some(700.0)).columns.join(" "),
        "1fr 1fr"
    );
}

#[test]
fn base_columns_below_every_breakpoint_and_the_later_tie_wins() {
    let layout = GridLayout {
        breakpoints: vec![
            Breakpoint {
                min_width: 500,
                columns: vec![fr(1.0)],
            },
            Breakpoint {
                min_width: 500,
                columns: vec![fr(1.0), fr(1.0)],
            },
            Breakpoint {
                min_width: 800,
                columns: vec![],
            },
        ],
        ..default_grid_layout()
    };
    assert_eq!(
        resolve_breakpoint(&layout, Some(400.0)),
        (layout.columns.clone(), -1)
    );
    assert_eq!(resolve_breakpoint(&layout, Some(500.0)).1, 1);
    assert_eq!(resolve_breakpoint(&layout, Some(900.0)).1, 1);
}

fn wrapping_layout() -> GridLayout {
    GridLayout {
        breakpoints: vec![
            Breakpoint {
                min_width: 0,
                columns: vec![fr(1.0); 4],
            },
            Breakpoint {
                min_width: 600,
                columns: vec![fr(1.0); 8],
            },
            Breakpoint {
                min_width: 1000,
                columns: vec![fr(1.0); 12],
            },
        ],
        ..default_grid_layout()
    }
}

fn wrapping_items() -> Vec<Placement> {
    vec![
        at(1, 1, 6, 1),
        at(7, 1, 6, 1),
        at(1, 2, 12, 1),
        at(1, 3, 3, 2),
        at(4, 3, 9, 1),
    ]
}

#[test]
fn wrapping_keeps_authored_positions_when_everything_fits() {
    let (layout, items) = (wrapping_layout(), wrapping_items());
    assert_eq!(resolve_placements(&layout, &items, Some(1200.0)), items);
    assert_eq!(resolve_placements(&layout, &items, None), items);
}

#[test]
fn wrapping_clamps_spans_and_flows_overflowing_items_to_the_next_free_row() {
    let (layout, items) = (wrapping_layout(), wrapping_items());
    assert_eq!(
        resolve_placements(&layout, &items, Some(700.0)),
        vec![
            at(1, 1, 6, 1),
            at(1, 2, 6, 1),
            at(1, 3, 8, 1),
            at(1, 4, 3, 2),
            at(1, 6, 8, 1)
        ]
    );
    assert_eq!(
        resolve_placements(&layout, &items, Some(300.0)),
        vec![
            at(1, 1, 4, 1),
            at(1, 2, 4, 1),
            at(1, 3, 4, 1),
            at(1, 4, 3, 2),
            at(1, 6, 4, 1)
        ]
    );
}

#[test]
fn wrapping_orders_by_authored_row_and_column_not_array_order() {
    let (layout, items) = (wrapping_layout(), wrapping_items());
    let shuffled = vec![
        items[4].clone(),
        items[2].clone(),
        items[0].clone(),
        items[3].clone(),
        items[1].clone(),
    ];
    assert_eq!(
        placements_to_css(&layout, &shuffled, Some(300.0)),
        vec![
            lines(1, 4, 6, 1),
            lines(1, 4, 3, 1),
            lines(1, 4, 1, 1),
            lines(1, 3, 4, 2),
            lines(1, 4, 2, 1),
        ]
    );
}

#[test]
fn a_lone_placement_clamps_to_the_active_columns() {
    assert_eq!(
        placement_to_css(&at(7, 2, 6, 1), Some(&wrapping_layout()), Some(300.0)),
        lines(1, 4, 2, 1)
    );
}

#[test]
fn named_regions_wrap_into_template_areas() {
    assert_eq!(
        layout_to_css(&golden(), Some(700.0)).areas.as_deref(),
        Some("\"header header\" \"side .\" \"side .\" \"main main\"")
    );
    assert_eq!(
        layout_to_css(&golden(), Some(100.0)).areas.as_deref(),
        Some("\"header\" \"side\" \"side\" \"main\"")
    );
    assert_eq!(
        placement_to_css(&in_region("main"), Some(&golden()), Some(100.0)),
        ItemCss::Area("main".into())
    );
}

#[test]
fn regions_use_safe_identifiers_and_overlaps_leave_the_template() {
    let layout = GridLayout {
        columns: vec![fr(1.0); 3],
        named_regions: vec![
            NamedRegion {
                name: "Top bar".into(),
                column: 1,
                row: 1,
                column_span: 3,
                row_span: 1,
            },
            NamedRegion {
                name: "auto".into(),
                column: 1,
                row: 2,
                column_span: 1,
                row_span: 1,
            },
            NamedRegion {
                name: "clash".into(),
                column: 3,
                row: 1,
                column_span: 1,
                row_span: 2,
            },
        ],
        ..default_grid_layout()
    };
    let resolved: Vec<(String, bool)> = resolve_regions(&layout, None)
        .into_iter()
        .map(|r| (r.ident, r.in_template))
        .collect();
    assert_eq!(
        resolved,
        vec![
            ("_0".to_string(), true),
            ("_1".to_string(), true),
            ("clash".to_string(), false)
        ]
    );
    assert_eq!(
        layout_to_css(&layout, None).areas.as_deref(),
        Some("\"_0 _0 _0\" \"_1 . .\"")
    );
    assert_eq!(
        placement_to_css(&in_region("Top bar"), Some(&layout), None),
        ItemCss::Area("_0".into())
    );
    assert_eq!(
        placement_to_css(&in_region("clash"), Some(&layout), None),
        lines(3, 1, 1, 2)
    );
    let missing = Placement {
        region: Some("missing".into()),
        ..at(2, 4, 1, 1)
    };
    assert_eq!(
        placement_to_css(&missing, Some(&layout), None),
        lines(2, 1, 4, 1)
    );
}

#[test]
fn resizing_clamps_within_the_grid_and_item_constraints() {
    let layout = default_grid_layout();
    let cols = |c| GridDelta { columns: c, rows: 0 };
    let none = SpanConstraints::default();
    assert_eq!(
        resize_placement(&at(1, 1, 4, 1), cols(2), &layout, none),
        at(1, 1, 6, 1)
    );
    assert_eq!(
        resize_placement(&at(10, 1, 3, 1), cols(5), &layout, none),
        at(10, 1, 3, 1)
    );
    assert_eq!(
        resize_placement(
            &at(1, 1, 1, 1),
            GridDelta {
                columns: -3,
                rows: -2
            },
            &layout,
            none
        ),
        at(1, 1, 1, 1)
    );
    let limits = SpanConstraints {
        min_column_span: Some(2),
        max_column_span: Some(6),
        min_row_span: Some(1),
        max_row_span: Some(3),
    };
    assert_eq!(
        resize_placement(
            &at(1, 1, 4, 2),
            GridDelta {
                columns: 9,
                rows: 9
            },
            &layout,
            limits
        ),
        at(1, 1, 6, 3)
    );
    assert_eq!(
        resize_placement(&at(1, 1, 4, 1), cols(-9), &layout, limits),
        at(1, 1, 2, 1)
    );
    assert_eq!(
        resize_placement(&at(12, 1, 1, 1), cols(1), &layout, limits),
        at(12, 1, 1, 1)
    );
    let regioned = in_region("main");
    assert_eq!(
        resize_placement(&regioned, cols(1), &layout, none),
        regioned
    );
}

#[test]
fn moving_stays_within_the_grid() {
    let layout = default_grid_layout();
    assert_eq!(
        move_placement(
            &at(1, 1, 4, 1),
            GridDelta {
                columns: 20,
                rows: -3
            },
            &layout
        ),
        at(9, 1, 4, 1)
    );
    assert_eq!(
        move_placement(
            &at(5, 2, 4, 1),
            GridDelta {
                columns: -1,
                rows: 1
            },
            &layout
        ),
        at(4, 3, 4, 1)
    );
}

#[test]
fn finds_the_first_free_slot() {
    let layout = default_grid_layout();
    assert_eq!(next_placement(&layout, &[], None, None), at(1, 1, 12, 1));
    assert_eq!(
        next_placement(&layout, &[at(1, 1, 6, 1), at(1, 3, 12, 1)], None, None),
        at(1, 2, 12, 1)
    );
    assert_eq!(
        next_placement(&layout, &[at(1, 1, 6, 1)], Some(6), None),
        at(7, 1, 6, 1)
    );
    assert_eq!(
        next_placement(&golden(), &[in_region("header")], Some(1), None),
        at(1, 2, 1, 1)
    );
}

#[test]
fn detects_overlaps_including_shared_regions() {
    let item = |id: &str, placement| GridItemRef {
        id: id.into(),
        placement,
    };
    let items = vec![
        item("a", at(1, 1, 6, 1)),
        item("b", at(6, 1, 2, 1)),
        item("c", at(8, 1, 2, 1)),
        item("d", in_region("header")),
        item("e", in_region("header")),
    ];
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(
        detect_overlaps(&items, None),
        vec![pair("a", "b"), pair("d", "e")]
    );
    assert_eq!(
        detect_overlaps(&items[..4], Some(&golden())),
        vec![pair("a", "b"), pair("a", "d")]
    );
}

#[test]
fn accepts_the_golden_and_default_layouts() {
    let one = |p| {
        vec![GridItemRef {
            id: "x".into(),
            placement: p,
        }]
    };
    assert!(validate_layout(&golden(), &one(in_region("main")), "layout", "", "item").is_empty());
    assert!(
        validate_layout(
            &default_grid_layout(),
            &one(at(1, 1, 12, 1)),
            "layout",
            "",
            "item"
        )
        .is_empty()
    );
}

#[test]
fn mirrors_the_rust_errors_and_adds_rendering_warnings() {
    let layout = GridLayout {
        columns: vec![fr(1.0), track(TrackKind::Fixed, Some(0.0), None, None)],
        rows: vec![track(TrackKind::Fr, Some(1.0), Some(50.0), Some(10.0))],
        named_regions: vec![
            NamedRegion {
                name: "a".into(),
                column: 1,
                row: 1,
                column_span: 2,
                row_span: 1,
            },
            NamedRegion {
                name: "a".into(),
                column: 2,
                row: 1,
                column_span: 1,
                row_span: 1,
            },
        ],
        breakpoints: vec![
            Breakpoint {
                min_width: 10,
                columns: vec![],
            },
            Breakpoint {
                min_width: 10,
                columns: vec![fr(1.0)],
            },
        ],
        ..default_grid_layout()
    };
    let item = |id: &str, placement| GridItemRef {
        id: id.into(),
        placement,
    };
    let issues = validate_layout(
        &layout,
        &[
            item("wide", at(2, 1, 2, 1)),
            item("zero", at(0, 1, 1, 1)),
            item("lost", in_region("nowhere")),
            // The TS case used a fractional column (1.5), which u16 cannot hold.
            item("half", at(2, 1, 1, 1)),
        ],
        "form",
        "f1",
        "control",
    );
    let summary: Vec<String> = issues
        .iter()
        .map(|i| {
            format!(
                "{} {}:{} {}",
                serde_json::to_value(&i.severity).unwrap().as_str().unwrap(),
                i.object_kind,
                i.object_id,
                i.message
            )
        })
        .collect();
    // The "gaps and padding must be whole pixels" and fractional-position errors cannot
    // occur: the Rust model stores whole u16 values.
    assert_eq!(
        summary,
        vec![
            "error form:f1 columns tracks need a positive size",
            "error form:f1 rows track minimum cannot exceed maximum",
            "warning form:f1 breakpoint has no columns and is ignored",
            "warning form:f1 more than one breakpoint starts at 10px; the last one wins",
            "error form:f1 named grid regions must be unique",
            "warning form:f1 region a overlaps region a and is left out of the template",
            "error control:wide grid placement exceeds declared columns",
            "error control:zero grid placement uses 1-based positions and positive spans",
            "error control:lost placement references an unknown region",
            "warning control:wide overlaps half",
        ]
    );
    let no_columns = GridLayout {
        columns: vec![],
        ..default_grid_layout()
    };
    assert_eq!(
        validate_layout(&no_columns, &[], "layout", "", "item")[0].message,
        "grid layout requires at least one column track"
    );
}

#[test]
fn layouts_round_trip_through_json_without_drift() {
    let layout = golden();
    let json = serde_json::to_value(&layout).unwrap();
    let back: GridLayout = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(back, layout);
    assert_eq!(serde_json::to_value(&back).unwrap(), json);
    let empty: GridLayout = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty, default_grid_layout());
    let placement: Placement = serde_json::from_value(json!({})).unwrap();
    assert_eq!(placement, at(1, 1, 1, 1));
    let polluted: GridLayout = serde_json::from_value(json!({
        "gridTemplateColumns": "1fr 1fr",
        "style": {"display": "grid"},
        "columns": [{"kind": "fr", "value": 1, "css": "minmax(0, 1fr)"}],
    }))
    .unwrap();
    let text = serde_json::to_string(&polluted).unwrap();
    assert!(!text.contains("gridTemplate") && !text.contains("minmax") && !text.contains("css"));
}

#[test]
fn region_helpers_clamp_inside_the_grid() {
    let clamped = clamp_region(&region("a", 5, 9), 4);
    assert_eq!((clamped.column, clamped.column_span), (4, 1));
    let zero = clamp_region(
        &NamedRegion {
            name: "a".into(),
            column: 0,
            row: 0,
            column_span: 0,
            row_span: 0,
        },
        4,
    );
    assert_eq!(
        (zero.column, zero.row, zero.column_span, zero.row_span),
        (1, 1, 1, 1)
    );
}

#[test]
fn region_references_follow_renames_and_clear_when_removed() {
    let placement = Placement {
        region: Some("a".into()),
        ..at(2, 3, 1, 1)
    };
    let renames = vec![("a".to_string(), "b".to_string())];
    assert_eq!(
        remap_region(&placement, &[region("b", 1, 1)], &renames).region,
        Some("b".into())
    );
    assert_eq!(
        remap_region(&placement, &[region("a", 1, 1)], &[]).region,
        Some("a".into())
    );
    assert_eq!(
        remap_region(&placement, &[], &[]),
        Placement {
            region: None,
            ..placement.clone()
        }
    );
}

#[test]
fn region_names_reject_empty_padded_and_duplicate() {
    let regions = vec![region("a", 1, 1), region("b", 1, 1)];
    assert_eq!(
        region_name_problem(&regions, 1, ""),
        "Area name is required."
    );
    assert!(region_name_problem(&regions, 1, " c").contains("spaces"));
    assert!(region_name_problem(&regions, 1, "a").contains("already named a"));
    assert_eq!(region_name_problem(&regions, 1, "b"), "");
}
