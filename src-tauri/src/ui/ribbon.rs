//! The ribbon (as in Access): a row of tabs over a band of labelled command groups. Studio
//! shows it above every page (`shell::ribbon`), and the Run App window above the app
//! (`runtime::run_mode`). It spans the content column only, never the sidebar.
//!
//! Test ids: `ribbon` (the band), `tab-<key>` (tabs, from `widgets::tab_strip`) and
//! `ribbon-group-<key>`, whose accessibility label is the group's name.

use crate::ui::prelude::*;

/// The ribbon: `tabs` (with `trailing` at the far end of the tab row) over `groups`.
pub fn ribbon(
    tabs: &[(&'static str, &'static str)],
    selected: &str,
    on_select: impl Fn(&'static str, &mut Window, &mut App) + 'static,
    trailing: Option<AnyElement>,
    groups: Vec<AnyElement>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    v_flex()
        .w_full()
        .flex_none()
        .bg(theme.sidebar)
        .child(
            widgets::tab_strip("ribbon-tabs", tabs, selected, on_select, cx)
                .items_center()
                .border_b_0()
                .child(div().flex_1())
                .children(trailing),
        )
        .child(
            h_flex()
                .id("ribbon")
                .w_full()
                .items_stretch()
                .px_2()
                .pb_1()
                .border_b_1()
                .border_color(theme.border)
                .overflow_x_scroll()
                .children(groups)
                .test_support(),
        )
}

/// One labelled group of commands; a rule separates it from the next group.
pub fn group(
    key: &str,
    label: &str,
    items: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .id(SharedString::from(format!("ribbon-group-{key}")))
        .aria_label(SharedString::from(label.to_string()))
        .flex_none()
        .justify_between()
        .gap_1()
        .px_2()
        .border_r_1()
        .border_color(theme.border)
        .child(h_flex().gap_1().items_center().children(items))
        .child(
            div()
                .w_full()
                .text_center()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(label.to_string()),
        )
        .test_support()
        .into_any_element()
}

/// A ribbon command: a quiet button with an icon and a short label.
pub fn command(id: &str, icon: IconName, label: &str) -> Button {
    Button::new(SharedString::from(id.to_string()))
        .ghost()
        .small()
        .icon(Icon::new(icon))
        .label(label.to_string())
}
