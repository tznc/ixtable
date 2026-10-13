//! Small layout helpers of the reports views. The form controls live in `ui::controls`.

use crate::ui::prelude::*;

/// (value, label) pairs from `(value, label)` string slices.
pub fn pairs(values: &[(&str, &str)]) -> Vec<(String, String)> {
    values.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect()
}

/// A section: a bold legend over its controls.
pub fn section(id: impl Into<ElementId>, legend: &str, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    v_flex()
        .id(id)
        .gap_1p5()
        .p_2()
        .border_1()
        .border_color(theme.border)
        .rounded_md()
        .child(div().text_sm().font_weight(FontWeight::SEMIBOLD).child(legend.to_string()))
}
