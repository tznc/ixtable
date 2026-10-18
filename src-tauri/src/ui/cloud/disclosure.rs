//! A "Show details" / "Hide details" toggle that keeps its open state app-wide per id, so
//! it works from render functions that only have `&App`.
//!
//! Test ids: the toggle is `<id>-toggle`; the body only exists while open.

use crate::ui::{forms::eid, prelude::*};
use std::collections::HashMap;

#[derive(Default)]
struct Open(HashMap<String, bool>);

impl Global for Open {}

/// `noun` completes the toggle label ("details" gives "Show details" / "Hide details").
pub fn disclosure(id: &str, noun: &str, body: impl FnOnce() -> AnyElement, cx: &App) -> Div {
    let open = cx.try_global::<Open>().and_then(|o| o.0.get(id)).copied().unwrap_or(false);
    let key = id.to_string();
    let label = format!("{} {noun}", if open { "Hide" } else { "Show" });
    v_flex()
        .gap_1()
        .items_start()
        .child(
            Button::new(eid(format!("{id}-toggle")))
                .link()
                .xsmall()
                .label(label)
                .on_click(move |_, window, cx| {
                    let map = &mut cx.default_global::<Open>().0;
                    let now = map.entry(key.clone()).or_insert(false);
                    *now = !*now;
                    window.refresh();
                }),
        )
        .when(open, |col| col.child(body()))
}
