//! A collapsible panel section (docs/decisions/design-language.md, "Section"): a
//! title row with a chevron, an optional count, and a one-line summary while
//! collapsed. Open state is app-wide per section id, so a section the user opened
//! stays open when its panel leaves the screen and comes back.
//!
//! Test ids: the title is observed under `id`, the toggle button is `<id>-toggle`,
//! and the collapsed summary is `<id>-summary`.

use crate::ui::prelude::*;
use std::collections::HashMap;

/// Sections the user toggled, by id; untouched sections use their default.
#[derive(Default)]
struct OpenSections(HashMap<SharedString, bool>);

impl Global for OpenSections {}

pub struct Section {
    id: SharedString,
    title: String,
    count: Option<usize>,
    summary: Option<String>,
    is_open: bool,
    children: Vec<AnyElement>,
}

/// A section that is open (`default_open`) or collapsed until the user toggles it; once
/// open by default it stays open.
pub fn section(
    id: impl Into<SharedString>,
    title: &str,
    default_open: bool,
    _window: &mut Window,
    cx: &mut App,
) -> Section {
    let id: SharedString = id.into();
    // A section that opened because it has content stays open when the content goes.
    let sections = &mut cx.default_global::<OpenSections>().0;
    let is_open = match sections.get(&id) {
        Some(open) => *open,
        None if default_open => *sections.entry(id.clone()).or_insert(true),
        None => false,
    };
    Section {
        id,
        title: title.to_string(),
        count: None,
        summary: None,
        is_open,
        children: Vec::new(),
    }
}

impl Section {
    /// Whether the body shows; build expensive children only when this is true.
    pub fn is_open(&self) -> bool {
        self.is_open
    }

    /// A count beside the title, such as the number of indexes.
    pub fn count(mut self, count: usize) -> Self {
        self.count = Some(count);
        self
    }

    /// One line shown in place of the body while collapsed ("Primary key: id").
    pub fn summary(mut self, summary: impl Into<String>) -> Self {
        let summary = summary.into();
        self.summary = (!summary.is_empty()).then_some(summary);
        self
    }

    /// Body content; dropped while collapsed.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        if self.is_open {
            self.children.push(child.into_any_element());
        }
        self
    }

    pub fn render(self, cx: &App) -> Stateful<Div> {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let (id, is_open) = (self.id.clone(), self.is_open);
        let icon = if self.is_open { IconName::ChevronDown } else { IconName::ChevronRight };
        let header = h_flex()
            .gap_1()
            .items_center()
            .child(
                Button::new(SharedString::from(format!("{}-toggle", self.id)))
                    .ghost()
                    .xsmall()
                    .icon(icon)
                    .tooltip(if self.is_open { "Collapse" } else { "Expand" })
                    .on_click(move |_, window, cx| {
                        cx.default_global::<OpenSections>().0.insert(id.clone(), !is_open);
                        window.refresh();
                    }),
            )
            .child(widgets::styled_text(self.id.clone(), self.title, |d| {
                d.text_base().font_weight(FontWeight::SEMIBOLD)
            }))
            .when_some(self.count, |row, n| {
                row.child(div().text_sm().text_color(muted).child(n.to_string()))
            });
        v_flex()
            .id(ElementId::from(SharedString::from(format!("{}-section", self.id))))
            .gap_2()
            .py_2()
            .border_t_1()
            .border_color(theme.border)
            .child(header)
            .when(!self.is_open, |col| {
                col.when_some(self.summary, |col, summary| {
                    col.child(widgets::styled_text(
                        SharedString::from(format!("{}-summary", self.id)),
                        summary,
                        move |d| d.pl_6().text_sm().text_color(muted),
                    ))
                })
            })
            .children(self.children)
    }
}
