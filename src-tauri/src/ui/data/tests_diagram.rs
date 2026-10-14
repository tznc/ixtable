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
fn refits_when_the_diagram_shrinks_for_the_datasheet(cx: &mut TestAppContext) {
    let mut h = shop(cx);
    let d = diagram(&mut h);
    let full = h.cx.read(|cx| d.read(cx).view());
    h.go("table:Customers");
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
