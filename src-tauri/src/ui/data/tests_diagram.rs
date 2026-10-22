//! Relationship diagram flows: straight routes around cards, fit-to-view, zoom and the
//! minimap. Imports are explicit: `gpui_kit::*` would shadow `#[test]`.

use super::{
    diagram::RelationshipBrowser,
    routing::{Rect, MAX_ZOOM},
    testkit::{data_mode, seed_shop},
};
use crate::ui::testing::Harness;
use gpui_kit::{Entity, TestAppContext};

fn diagram(h: &mut Harness) -> Entity<RelationshipBrowser> {
    let mode = data_mode(h);
    h.cx.read(|cx| mode.read(cx).diagram().clone())
}

fn shop(cx: &mut TestAppContext) -> Harness<'_, TestAppContext> {
    let mut h = Harness::new_document(cx);
    seed_shop(&mut h);
    assert_eq!(h.label("relationship-title"), "4 tables \u{b7} 3 relationships");
    h
}

fn inside(outer: &Rect, inner: &Rect) -> bool {
    inner.x >= outer.x - 0.5
        && inner.y >= outer.y - 0.5
        && inner.x + inner.w <= outer.x + outer.w + 0.5
        && inner.y + inner.h <= outer.y + outer.h + 0.5
}

#[gpui_kit::test]
fn routes_relationships_as_straight_lines_around_the_cards(cx: &mut TestAppContext) {
    let mut h = shop(cx);
    let d = diagram(&mut h);
    let (routes, cards) = h.cx.read(|cx| {
        let d = d.read(cx);
        let cards: Vec<Rect> = ["Customers", "Orders", "Order Items", "Products"]
            .iter()
            .map(|t| d.card(t).expect("card"))
            .collect();
        (d.route_points(), cards)
    });
    assert_eq!(routes.len(), 3);
    for route in &routes {
        for w in route.windows(2) {
            let (a, b) = (w[0], w[1]);
            assert!(a.0 == b.0 || a.1 == b.1, "segment {a:?} -> {b:?} is not straight");
            for card in &cards {
                let middle = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
                let strictly_in = middle.0 > card.x
                    && middle.0 < card.x + card.w
                    && middle.1 > card.y
                    && middle.1 < card.y + card.h;
                assert!(!strictly_in, "segment {a:?} -> {b:?} crosses {card:?}");
            }
        }
    }
    assert!(h.exists("edge-label:0"));
    assert_eq!(h.label("edge-label:0"), "1 \u{2014} \u{221e}");
}

#[gpui_kit::test]
fn fits_the_diagram_and_zooms_with_the_controls(cx: &mut TestAppContext) {
    let mut h = shop(cx);
    let d = diagram(&mut h);
    let fitted = h.cx.read(|cx| d.read(cx).view());
    let (visible, cards) = h.cx.read(|cx| {
        let d = d.read(cx);
        let cards: Vec<Rect> = ["Customers", "Orders", "Order Items", "Products"]
            .iter()
            .map(|t| d.card(t).expect("card"))
            .collect();
        (d.visible().expect("the viewport was laid out"), cards)
    });
    assert!(cards.iter().all(|c| inside(&visible, c)), "every card shows: {visible:?}");

    h.click("zoom-out");
    let out = h.cx.read(|cx| d.read(cx).view());
    assert!(out.zoom < fitted.zoom, "{out:?}");
    h.click("zoom-in");
    h.click("zoom-in");
    let zoomed = h.cx.read(|cx| d.read(cx).view());
    assert!(zoomed.zoom > fitted.zoom || zoomed.zoom == MAX_ZOOM, "{zoomed:?}");
    h.click("fit-view");
    assert_eq!(h.cx.read(|cx| d.read(cx).view()), fitted);
}

#[gpui_kit::test]
fn the_minimap_recentres_the_view(cx: &mut TestAppContext) {
    let mut h = shop(cx);
    let d = diagram(&mut h);
    assert!(h.exists("minimap"));
    h.click("zoom-in");
    h.click("zoom-in");
    // Off-centre, so recentring on the minimap's middle always moves the view.
    h.cx.update(|cx| d.update(cx, |d, _| d.pan_by(240.0, 160.0)));
    let before = h.cx.read(|cx| d.read(cx).view());
    h.click("minimap");
    let after = h.cx.read(|cx| d.read(cx).view());
    assert_eq!(after.zoom, before.zoom);
    assert_ne!(after.pan, before.pan);
}

#[gpui_kit::test]
fn a_new_table_refits_the_view(cx: &mut TestAppContext) {
    let mut h = shop(cx);
    let d = diagram(&mut h);
    h.click("zoom-in");
    h.click("zoom-in");
    let zoomed = h.cx.read(|cx| d.read(cx).view());
    let spec = serde_json::json!({
        "name": "Suppliers",
        "columns": [{ "name": "id", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1 }],
    });
    crate::create_database_table(h.label.clone(), serde_json::from_value(spec).expect("spec"))
        .expect("create table");
    h.database_changed();
    h.settle();
    let refitted = h.cx.read(|cx| d.read(cx).view());
    assert_ne!(refitted, zoomed);
    let (visible, card) = h.cx.read(|cx| {
        let d = d.read(cx);
        (d.visible().expect("laid out"), d.card("Suppliers").expect("card"))
    });
    assert!(inside(&visible, &card), "the new table shows: {visible:?} {card:?}");
}

#[gpui_kit::test]
fn collapses_the_diagram_for_the_datasheet_and_refits_when_expanded(cx: &mut TestAppContext) {
    let mut h = shop(cx);
    let d = diagram(&mut h);
    let full = h.cx.read(|cx| d.read(cx).view());
    h.go("table:Customers");
    h.settle();
    assert!(h.exists("relationship-toggle"), "the header stays with a toggle");
    assert!(h.exists("relationship-title"));
    h.click("relationship-toggle");
    h.settle();
    let (view, visible, cards) = h.cx.read(|cx| {
        let d = d.read(cx);
        let cards: Vec<Rect> = ["Customers", "Orders", "Order Items", "Products"]
            .iter()
            .map(|t| d.card(t).expect("card"))
            .collect();
        (d.view(), d.visible().expect("laid out"), cards)
    });
    assert!(view.zoom < full.zoom, "{view:?}");
    assert!(cards.iter().all(|c| inside(&visible, c)), "every card shows: {visible:?}");
}

#[gpui_kit::test]
fn relationships_sharing_a_column_get_their_own_ports(cx: &mut TestAppContext) {
    let mut h = shop(cx);
    super::testkit::make_table(
        &h,
        "Shipments",
        serde_json::json!([
            { "name": "Shipment ID", "declaredType": "INTEGER", "nullable": false, "primaryKeyPosition": 1 },
            { "name": "Order ID", "declaredType": "TEXT", "nullable": true, "primaryKeyPosition": 0 },
        ]),
        serde_json::json!([{ "columns": ["Order ID"], "targetTable": "Orders", "targetColumns": ["Order ID"] }]),
    );
    h.database_changed();
    h.settle();
    let d = diagram(&mut h);
    let routes = h.cx.read(|cx| d.read(cx).route_points());
    assert_eq!(routes.len(), 4);
    // Orders.Order ID feeds Order Items and Shipments: their routes leave from different points.
    let starts: Vec<(f32, f32)> = routes.iter().map(|r| r[0]).collect();
    let ends: Vec<(f32, f32)> = routes.iter().map(|r| *r.last().unwrap()).collect();
    for (i, a) in starts.iter().enumerate() {
        for b in &starts[i + 1..] {
            assert_ne!(a, b, "two relationships leave from one port: {starts:?}");
        }
    }
    for (i, a) in ends.iter().enumerate() {
        for b in &ends[i + 1..] {
            assert_ne!(a, b, "two relationships enter one port: {ends:?}");
        }
    }
}

#[test]
fn labels_do_not_stack_or_touch_cards() {
    use super::diagram::place_labels;
    let route = vec![(0.0, 100.0), (100.0, 100.0), (100.0, 300.0)];
    let routes = vec![
        (Some("1 \u{2014} \u{221e}".to_string()), None, route.clone()),
        (Some("1 \u{2014} \u{221e}".to_string()), None, route.clone()),
        (None, None, route.clone()),
    ];
    let card = Rect { x: 40.0, y: 60.0, w: 120.0, h: 80.0 };
    let at = place_labels(&routes, &[card]);
    assert_eq!(at[2], None);
    let rects: Vec<Rect> = at[..2]
        .iter()
        .map(|p| {
            let (x, y) = p.expect("placed");
            Rect { x, y, w: 44.0, h: 20.0 }
        })
        .collect();
    let touches = |a: &Rect, b: &Rect| a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
    assert!(!touches(&rects[0], &rects[1]), "{rects:?}");
    assert!(rects.iter().all(|r| !touches(r, &card)), "{rects:?}");
}

#[gpui_kit::test]
fn each_relationship_label_explains_itself_in_a_tooltip(cx: &mut TestAppContext) {
    let h = shop(cx);
    let schemas: Vec<_> = ["Customers", "Products", "Orders", "Order Items"]
        .iter()
        .map(|t| super::testkit::inspect(&h, t))
        .collect();
    let edges = super::relationships::relationship_edges(&schemas);
    let tips: Vec<String> = edges.iter().filter_map(|e| e.tooltip.clone()).collect();
    assert!(tips.contains(&"One order \u{2192} many order items".to_string()), "{tips:?}");
    assert!(tips.contains(&"One customer \u{2192} many orders".to_string()), "{tips:?}");
}
