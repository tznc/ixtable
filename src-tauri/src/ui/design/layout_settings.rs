//! Grid settings of a form or container (ports of `LayoutSettings.tsx`, `TrackList.tsx`
//! and `RegionEditor.tsx`): column and row tracks, gaps, padding, item alignment, named
//! regions and breakpoints, plus the problems `validate_layout` finds. Regions are kept
//! inside the columns, and a layout with more errors than the stored one is never applied,
//! so the editor cannot send a grid the backend would refuse.
//!
//! Test ids, with `<p>` the section prefix (`grid` for the form, `container` for a section
//! or tab group): `<p>-columns`, `<p>-padding`, `<p>-column-gap`, `<p>-row-gap`,
//! `<p>-justify` ("Horizontal alignment"), `<p>-align` ("Vertical alignment"); tracks
//! `<p>-column-N-kind`, `-size`, `-min`, `-max`, `<p>-remove-column-N`, `<p>-add-column` (and
//! the same with `row`); regions `<p>-region-N-name`, `-column`, `-row`, `-column-span`,
//! `-row-span`, `<p>-remove-region-N`, `<p>-add-region`; breakpoints `<p>-breakpoint-N-width`,
//! `-columns`, `<p>-remove-breakpoint-N`, `<p>-add-breakpoint`; problems `<p>-issue-N`.
//! A rejected draft shows `<id>-error`.

use super::{ctl::icon_button, studio::DesignStudio};
use crate::design::{Breakpoint, GridAlign, GridLayout, GridTrack, NamedRegion, TrackKind};
use crate::forms::{
    operations::{resize_tracks, with_column_count},
    track_input::{Limit, parse_track_limit, parse_track_size},
};
use crate::ui::{
    grid::engine::{GridItemRef, clamp_region, clamp_regions, region_name_problem, validate_layout},
    prelude::*,
};
use crate::validation::Severity;
use std::rc::Rc;

/// Receives the edited layout and the region renames (old, new).
type Apply = Rc<dyn Fn(&mut DesignStudio, GridLayout, Vec<(String, String)>, &mut Context<DesignStudio>)>;

#[derive(Clone)]
struct Ctx {
    p: String,
    layout: GridLayout,
    apply: Apply,
}

const ALIGNS: [(&str, &str); 4] = [("stretch", "stretch"), ("start", "start"), ("center", "center"), ("end", "end")];

fn align_key(a: &GridAlign) -> &'static str {
    match a {
        GridAlign::Stretch => "stretch",
        GridAlign::Start => "start",
        GridAlign::Center => "center",
        GridAlign::End => "end",
    }
}

fn align_from(key: &str) -> GridAlign {
    match key {
        "start" => GridAlign::Start,
        "center" => GridAlign::Center,
        "end" => GridAlign::End,
        _ => GridAlign::Stretch,
    }
}

fn error_count(layout: &GridLayout) -> usize {
    validate_layout(layout, &[], "form", "", "control").iter().filter(|i| i.severity == Severity::Error).count()
}

/// A whole number of at least 0.
fn whole(text: &str) -> Option<u32> {
    let n: f64 = text.trim().parse().ok()?;
    (n.is_finite() && n >= 0.0).then(|| n.round().min(f64::from(u32::MAX)) as u32)
}

fn kind_defaults(kind: &TrackKind) -> Option<f64> {
    match kind {
        TrackKind::Fixed => Some(120.0),
        TrackKind::Fr => Some(1.0),
        TrackKind::Content => None,
    }
}

fn fresh_region_name(regions: &[NamedRegion]) -> String {
    let mut n = regions.len() + 1;
    while regions.iter().any(|r| r.name == format!("region{n}")) {
        n += 1;
    }
    format!("region{n}")
}

fn number_text(value: Option<f64>) -> String {
    value.map(|n| n.to_string()).unwrap_or_default()
}

impl DesignStudio {
    /// The grid section. `items` (id, placement, label) enable the list of layout problems.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn layout_settings(
        &mut self,
        prefix: &str,
        title: &str,
        layout: &GridLayout,
        items: &[(GridItemRef, String)],
        on_change: impl Fn(&mut Self, GridLayout, Vec<(String, String)>, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let stored = layout.clone();
        let on_change = Rc::new(on_change);
        let apply: Apply = Rc::new(move |this, next, renames, cx| {
            let clamped = GridLayout { named_regions: clamp_regions(&next.named_regions, next.columns.len()), ..next };
            // An already-invalid stored grid can still be repaired one edit at a time.
            if error_count(&clamped) > error_count(&stored) {
                return;
            }
            on_change(this, clamped, renames, cx);
        });
        let c = Ctx { p: prefix.to_string(), layout: layout.clone(), apply };
        let muted = cx.theme().muted_foreground;
        let heading = |text: &str| div().text_xs().text_color(muted).child(text.to_string());
        let hint = |text: &str| div().text_xs().text_color(muted).child(text.to_string());

        let refs: Vec<GridItemRef> = items.iter().map(|(r, _)| r.clone()).collect();
        let issues = if items.is_empty() { vec![] } else { validate_layout(layout, &refs, "form", "", "control") };
        let label_of = |id: &str| items.iter().find(|(r, _)| r.id == id).map(|(_, l)| l.clone()).unwrap_or_else(|| id.to_string());

        let columns = self.count_field(&c, "columns", "Columns", layout.columns.len() as u32, window, cx);
        let padding = self.count_field(&c, "padding", "Padding", u32::from(layout.padding), window, cx);
        let column_gap = self.count_field(&c, "column-gap", "Column gap", u32::from(layout.column_gap), window, cx);
        let row_gap = self.count_field(&c, "row-gap", "Row gap", u32::from(layout.row_gap), window, cx);
        let justify = self.align_pick(&c, "justify", "Horizontal alignment", true, cx);
        let align = self.align_pick(&c, "align", "Vertical alignment", false, cx);
        let column_tracks = self.track_list(&c, "column", "Column", &layout.columns, 1, window, cx);
        let row_tracks = self.track_list(&c, "row", "Row", &layout.rows, 0, window, cx);
        let regions = self.region_editor(&c, window, cx);
        let breakpoints = self.breakpoint_editor(&c, window, cx);

        let danger = cx.theme().danger;
        let warning = cx.theme().warning;
        let summary = format!(
            "{}, {}",
            crate::ui::persistence::format::plural(layout.columns.len(), "column"),
            crate::ui::persistence::format::plural(layout.rows.len(), "row")
        );
        let section = crate::ui::section::section(format!("{prefix}-settings"), title, prefix == "container", window, cx).summary(summary);
        section
            .child(
                v_flex()
                    .gap_2()
                    .child(h_flex().gap_2().child(columns).child(padding))
                    .child(h_flex().gap_2().child(column_gap).child(row_gap))
                    .child(h_flex().gap_2().child(justify).child(align))
                    .child(heading("Column widths"))
                    .child(column_tracks)
                    .child(heading("Row heights"))
                    .child(hint("Rows without a size fit their content."))
                    .child(row_tracks)
                    .child(heading("Areas"))
                    .child(regions)
                    .child(heading("Responsive columns"))
                    .child(hint("Use a different number of columns from a minimum window width; items wrap to fit."))
                    .child(breakpoints),
            )
            .render(cx)
            .when(!issues.is_empty(), |el| {
                el.child(
                    v_flex().id(SharedString::from(format!("{prefix}-issues"))).aria_label(format!("{title} problems")).children(
                        issues.iter().enumerate().map(|(i, issue)| {
                            let text = if issue.object_kind == "control" {
                                let message = match issue.message.strip_prefix("overlaps ") {
                                    Some(other) => format!("overlaps {}", label_of(other)),
                                    None => issue.message.clone(),
                                };
                                format!("{}: {message}", label_of(&issue.object_id))
                            } else {
                                issue.message.clone()
                            };
                            let color = if issue.severity == Severity::Error { danger } else { warning };
                            widgets::styled_text(SharedString::from(format!("{prefix}-issue-{}", i + 1)), text, move |d| {
                                d.text_xs().text_color(color)
                            })
                        }),
                    ),
                )
            })
    }

    /// A whole-number draft for one of the grid's scalar settings.
    fn count_field(&mut self, c: &Ctx, key: &str, label: &str, value: u32, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let (layout, apply) = (c.layout.clone(), c.apply.clone());
        let key_name = key.to_string();
        self.draft(
            &format!("{}-{key}", c.p),
            label,
            &value.to_string(),
            "",
            false,
            move |this, text, cx| {
                let Some(n) = whole(&text) else {
                    return "Enter a whole number.".to_string();
                };
                let small = n.min(u32::from(u16::MAX)) as u16;
                let next = match key_name.as_str() {
                    "columns" => with_column_count(&layout, (n as usize).max(1)),
                    "padding" => GridLayout { padding: small, ..layout.clone() },
                    "column-gap" => GridLayout { column_gap: small, ..layout.clone() },
                    _ => GridLayout { row_gap: small, ..layout.clone() },
                };
                apply(this, next, vec![], cx);
                String::new()
            },
            window,
            cx,
        )
    }

    fn align_pick(&self, c: &Ctx, key: &str, label: &str, horizontal: bool, cx: &mut Context<Self>) -> Div {
        let (layout, apply) = (c.layout.clone(), c.apply.clone());
        let current = align_key(if horizontal { &layout.justify_items } else { &layout.align_items });
        self.pick(
            &format!("{}-{key}", c.p),
            label,
            current,
            super::studio::options(&ALIGNS),
            "",
            move |this, value, cx| {
                let mut next = layout.clone();
                if horizontal {
                    next.justify_items = align_from(value);
                } else {
                    next.align_items = align_from(value);
                }
                apply(this, next, vec![], cx);
            },
            cx,
        )
    }

    /// Kind, size and pixel limits of each track (columns or rows).
    #[allow(clippy::too_many_arguments)]
    fn track_list(
        &mut self,
        c: &Ctx,
        key: &'static str,
        name: &'static str,
        tracks: &[GridTrack],
        min_count: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let set = move |layout: &GridLayout, tracks: Vec<GridTrack>| {
            if key == "column" {
                GridLayout { columns: tracks, ..layout.clone() }
            } else {
                GridLayout { rows: tracks, ..layout.clone() }
            }
        };
        let mut list = v_flex()
            .id(SharedString::from(format!("{}-{key}-tracks", c.p)))
            .aria_label(if key == "column" { "Column widths" } else { "Row heights" })
            .gap_1();
        for (index, track) in tracks.iter().enumerate() {
            let n = index + 1;
            let label = format!("{name} {n}");
            let id = format!("{}-{key}-{n}", c.p);
            let edit = {
                let (c, tracks) = (c.clone(), tracks.to_vec());
                move |this: &mut Self, patch: Box<dyn FnOnce(&mut GridTrack)>, cx: &mut Context<Self>| {
                    let mut next = tracks.clone();
                    patch(&mut next[index]);
                    (c.apply)(this, set(&c.layout, next), vec![], cx);
                }
            };
            let kind = {
                let edit = edit.clone();
                self.pick_bare(
                    &format!("{id}-kind"),
                    match track.kind {
                        TrackKind::Fr => "fr",
                        TrackKind::Fixed => "fixed",
                        TrackKind::Content => "content",
                    },
                    super::studio::options(&[("fr", "Flexible (share)"), ("fixed", "Fixed size (px)"), ("content", "Fit content")]),
                    move |this, value, cx| {
                        let kind = match value {
                            "fixed" => TrackKind::Fixed,
                            "content" => TrackKind::Content,
                            _ => TrackKind::Fr,
                        };
                        edit(this, Box::new(move |t| {
                            t.value = kind_defaults(&kind);
                            t.kind = kind;
                        }), cx);
                    },
                    cx,
                )
            };
            let remove = {
                let (c, tracks) = (c.clone(), tracks.to_vec());
                icon_button(format!("{}-remove-{key}-{n}", c.p), IconName::Trash, &format!("Remove {}", label.to_lowercase()))
                    .disabled(tracks.len() <= min_count)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let mut next = tracks.clone();
                        next.remove(index);
                        (c.apply)(this, set(&c.layout, next), vec![], cx);
                    }))
            };
            let size = (track.kind != TrackKind::Content).then(|| {
                let edit = edit.clone();
                let kind = track.kind.clone();
                self.draft(
                    &format!("{id}-size"),
                    if track.kind == TrackKind::Fr { "Flexible share" } else { "Size (px)" },
                    &number_text(track.value),
                    "",
                    false,
                    move |this, text, cx| match parse_track_size(&kind, &text) {
                        Ok(value) => {
                            edit(this, Box::new(move |t| t.value = Some(value)), cx);
                            String::new()
                        }
                        Err(problem) => problem,
                    },
                    window,
                    cx,
                )
            });
            let limit = |which: Limit, suffix: &str, label: &str, value: Option<f64>, this: &mut Self, window: &mut Window, cx: &mut Context<Self>| {
                let edit = edit.clone();
                let track = track.clone();
                this.draft(
                    &format!("{id}-{suffix}"),
                    label,
                    &number_text(value),
                    "–",
                    false,
                    move |this, text, cx| match parse_track_limit(&track, which, &text) {
                        Ok(parsed) => {
                            edit(this, Box::new(move |t| match which {
                                Limit::Min => t.min = parsed,
                                Limit::Max => t.max = parsed,
                            }), cx);
                            String::new()
                        }
                        Err(problem) => problem,
                    },
                    window,
                    cx,
                )
            };
            let min = limit(Limit::Min, "min", "Min (px)", track.min, self, window, cx);
            let max = limit(Limit::Max, "max", "Max (px)", track.max, self, window, cx);
            list = list.child(
                v_flex()
                    .gap_1()
                    .p_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().flex_1().child(widgets::text(SharedString::from(format!("{id}-name")), label)))
                            .child(kind)
                            .child(remove),
                    )
                    .child(h_flex().gap_2().children(size).child(min).child(max)),
            );
        }
        let (layout, apply, existing) = (c.layout.clone(), c.apply.clone(), tracks.to_vec());
        list.child(
            Button::new(SharedString::from(format!("{}-add-{key}", c.p)))
                .outline()
                .small()
                .icon(IconName::Plus)
                .label(format!("Add {}", name.to_lowercase()))
                .disabled(tracks.len() >= 24)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let mut next = existing.clone();
                    next.push(GridTrack { kind: TrackKind::Fr, value: Some(1.0), min: None, max: None });
                    apply(this, set(&layout, next), vec![], cx);
                })),
        )
    }

    /// Name, position and spans of each named region.
    fn region_editor(&mut self, c: &Ctx, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let regions = c.layout.named_regions.clone();
        let columns = c.layout.columns.len();
        let mut group = v_flex().id(SharedString::from(format!("{}-regions", c.p))).aria_label("Areas").gap_1();
        for (index, region) in regions.iter().enumerate() {
            let n = index + 1;
            let id = format!("{}-region-{n}", c.p);
            let set = {
                let (c, regions) = (c.clone(), regions.clone());
                move |this: &mut Self, patch: Box<dyn FnOnce(&mut NamedRegion)>, renames: Vec<(String, String)>, cx: &mut Context<Self>| {
                    let mut next = regions.clone();
                    patch(&mut next[index]);
                    next[index] = clamp_region(&next[index], columns);
                    (c.apply)(this, GridLayout { named_regions: next, ..c.layout.clone() }, renames, cx);
                }
            };
            let name = {
                let (set, regions, old) = (set.clone(), regions.clone(), region.name.clone());
                self.draft(
                    &format!("{id}-name"),
                    "Name",
                    &region.name,
                    "",
                    false,
                    move |this, name, cx| {
                        let problem = region_name_problem(&regions, index, &name);
                        if problem.is_empty() {
                            let renames = vec![(old.clone(), name.clone())];
                            set(this, Box::new(move |r| r.name = name), renames, cx);
                        }
                        problem
                    },
                    window,
                    cx,
                )
            };
            let remove = {
                let (c, regions) = (c.clone(), regions.clone());
                icon_button(format!("{}-remove-region-{n}", c.p), IconName::Trash, &format!("Remove area {n}")).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let mut next = regions.clone();
                        next.remove(index);
                        (c.apply)(this, GridLayout { named_regions: next, ..c.layout.clone() }, vec![], cx);
                    }),
                )
            };
            let mut numbers = vec![];
            for (suffix, label, value) in [
                ("column", "Column", region.column),
                ("row", "Row", region.row),
                ("column-span", "Column span", region.column_span),
                ("row-span", "Row span", region.row_span),
            ] {
                let set = set.clone();
                numbers.push(self.draft(
                    &format!("{id}-{suffix}"),
                    label,
                    &value.to_string(),
                    "",
                    false,
                    move |this, text, cx| {
                        let Some(parsed) = text.trim().parse::<f64>().ok().filter(|n| n.is_finite()) else {
                            return "Enter a whole number.".to_string();
                        };
                        let parsed = parsed.round().clamp(0.0, f64::from(u16::MAX)) as u16;
                        set(
                            this,
                            Box::new(move |r| match suffix {
                                "column" => r.column = parsed,
                                "row" => r.row = parsed,
                                "column-span" => r.column_span = parsed,
                                _ => r.row_span = parsed,
                            }),
                            vec![],
                            cx,
                        );
                        String::new()
                    },
                    window,
                    cx,
                ));
            }
            let mut numbers = numbers.into_iter();
            let (a, b, d, e) = (numbers.next(), numbers.next(), numbers.next(), numbers.next());
            group = group.child(
                v_flex()
                    .gap_1()
                    .p_1()
                    .child(h_flex().gap_2().items_end().child(div().flex_1().child(name)).child(remove))
                    .child(h_flex().gap_2().children(a).children(b))
                    .child(h_flex().gap_2().children(d).children(e)),
            );
        }
        let (layout, apply) = (c.layout.clone(), c.apply.clone());
        group.child(
            Button::new(SharedString::from(format!("{}-add-region", c.p)))
                .outline()
                .small()
                .icon(IconName::Plus)
                .label("Add area")
                .on_click(cx.listener(move |this, _, _, cx| {
                    let mut next = layout.named_regions.clone();
                    let name = fresh_region_name(&next);
                    next.push(NamedRegion { name, column: 1, row: 1, column_span: layout.columns.len() as u16, row_span: 1 });
                    apply(this, GridLayout { named_regions: next, ..layout.clone() }, vec![], cx);
                })),
        )
    }

    /// From-width and column count of each breakpoint.
    fn breakpoint_editor(&mut self, c: &Ctx, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let breakpoints = c.layout.breakpoints.clone();
        let mut group = v_flex().gap_1();
        for (index, breakpoint) in breakpoints.iter().enumerate() {
            let n = index + 1;
            let id = format!("{}-breakpoint-{n}", c.p);
            let width = {
                let (c, list) = (c.clone(), breakpoints.clone());
                self.draft(
                    &format!("{id}-width"),
                    "From width",
                    &breakpoint.min_width.to_string(),
                    "",
                    false,
                    move |this, text, cx| {
                        let Some(min_width) = whole(&text) else {
                            return "Enter a whole number.".to_string();
                        };
                        let mut next = list.clone();
                        next[index].min_width = min_width;
                        (c.apply)(this, GridLayout { breakpoints: next, ..c.layout.clone() }, vec![], cx);
                        String::new()
                    },
                    window,
                    cx,
                )
            };
            let count = {
                let (c, list) = (c.clone(), breakpoints.clone());
                self.draft(
                    &format!("{id}-columns"),
                    "Columns",
                    &breakpoint.columns.len().to_string(),
                    "",
                    false,
                    move |this, text, cx| {
                        let Some(n) = whole(&text) else {
                            return "Enter a whole number.".to_string();
                        };
                        let mut next = list.clone();
                        next[index].columns = resize_tracks(&next[index].columns, (n as usize).max(1));
                        (c.apply)(this, GridLayout { breakpoints: next, ..c.layout.clone() }, vec![], cx);
                        String::new()
                    },
                    window,
                    cx,
                )
            };
            let remove = {
                let (c, list) = (c.clone(), breakpoints.clone());
                icon_button(format!("{}-remove-breakpoint-{n}", c.p), IconName::Trash, &format!("Remove width rule {n}")).on_click(
                    cx.listener(move |this, _, _, cx| {
                        let mut next = list.clone();
                        next.remove(index);
                        (c.apply)(this, GridLayout { breakpoints: next, ..c.layout.clone() }, vec![], cx);
                    }),
                )
            };
            group = group.child(h_flex().gap_2().items_end().child(width).child(count).child(remove));
        }
        let (layout, apply) = (c.layout.clone(), c.apply.clone());
        group.child(
            Button::new(SharedString::from(format!("{}-add-breakpoint", c.p)))
                .outline()
                .small()
                .icon(IconName::Plus)
                .label("Add width rule")
                .on_click(cx.listener(move |this, _, _, cx| {
                    let mut next = layout.breakpoints.clone();
                    next.push(Breakpoint { min_width: 960, columns: layout.columns.clone() });
                    apply(this, GridLayout { breakpoints: next, ..layout.clone() }, vec![], cx);
                })),
        )
    }
}
