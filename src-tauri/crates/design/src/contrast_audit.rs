//! Every text color a component draws, against the fill it sits on, in all
//! six themes. Body text and captions need 4.5:1 (WCAG AA). White on the teal
//! primary is the one accepted exception (2.4:1, design-language.md §4).

use gpui_bootstrap::{ColorMode, Theme, Variant, theme::mix};
use gpui_kit::Hsla;

use crate::action::{ButtonLook, look_colors};
use crate::theme::{DESKTOP, TEXT_CONTRAST, contrast_ratio, readable, theme};
use crate::typography::Tone;

/// `color` painted over `bg`, so translucent fills and text count as seen.
fn over(color: Hsla, bg: Hsla) -> Hsla {
    mix(color.opacity(1.), bg, color.a)
}

/// `(what, text, fill)` for everything the components draw.
fn pairs(theme: &Theme) -> Vec<(String, Hsla, Hsla)> {
    let colors = &theme.colors;
    let page = colors.body_bg;
    let mut pairs = vec![
        ("body text".into(), colors.body_color, page),
        (
            "muted text (captions, inactive tabs)".into(),
            colors.secondary_color,
            page,
        ),
        (
            "muted text on the sidebar and ribbon".into(),
            colors.secondary_color,
            colors.tertiary_bg,
        ),
        (
            "table header".into(),
            colors.emphasis_color,
            colors.tertiary_bg,
        ),
        ("link".into(), colors.link_color, page),
        ("sidebar link".into(), colors.link_color, colors.tertiary_bg),
        (
            "selected sidebar item".into(),
            colors.body_color,
            over(colors.primary.opacity(0.14), colors.tertiary_bg),
        ),
        (
            "segmented tab, inactive".into(),
            colors.secondary_color,
            colors.secondary_bg,
        ),
        (
            "pill tab, active".into(),
            colors.emphasis_color,
            over(colors.primary.opacity(0.14), page),
        ),
        (
            "field error".into(),
            readable(colors.danger, page, TEXT_CONTRAST, colors.body_color),
            page,
        ),
        (
            "neutral callout".into(),
            colors.body_color,
            colors.tertiary_bg,
        ),
    ];
    for tone in Tone::ALL.into_iter().filter(|tone| *tone != Tone::Neutral) {
        let variant = theme.variant(tone.variant());
        pairs.push((
            format!("{} callout and badge", tone.name()),
            variant.text_emphasis,
            variant.bg_subtle,
        ));
    }
    let badge = theme.variant(Variant::Secondary);
    pairs.push(("neutral badge".into(), badge.text_emphasis, badge.bg_subtle));
    for look in ButtonLook::ALL
        .into_iter()
        .filter(|look| *look != ButtonLook::Outline)
    {
        for danger in [false, true] {
            let button = look_colors(theme, look, danger);
            let kind = if danger { "danger" } else { "secondary" };
            for (state, fill) in [("", button.bg), (" hover", button.hover_bg)] {
                pairs.push((
                    format!("{} {kind} button{state}", look.name()),
                    button.color,
                    over(fill, page),
                ));
            }
        }
    }
    pairs
}

#[test]
fn every_text_color_reads_in_every_theme() {
    let mut failures = Vec::new();
    for platform in DESKTOP {
        for mode in [ColorMode::Light, ColorMode::Dark] {
            let theme = theme(platform, mode);
            for (what, text, fill) in pairs(&theme) {
                let ratio = contrast_ratio(text, fill);
                if ratio < TEXT_CONTRAST {
                    failures.push(format!("{platform:?} {mode:?}: {what} is {ratio:.2}:1"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "below 4.5:1:\n{}", failures.join("\n"));
}

#[test]
fn contrast_ratio_matches_wcag() {
    let black: Hsla = gpui_kit::rgb(0x000000).into();
    let white: Hsla = gpui_kit::rgb(0xffffff).into();
    assert!((contrast_ratio(black, white) - 21.).abs() < 0.01);
    let grey: Hsla = gpui_kit::rgb(0x767676).into();
    assert!((contrast_ratio(grey, white) - 4.54).abs() < 0.02);
}
