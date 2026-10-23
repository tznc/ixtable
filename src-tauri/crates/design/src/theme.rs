//! The ixtable theme: each OS's native look (gpui-bootstrap's platform
//! palettes, corners and controls) with the app icon's teal as the accent and
//! one shared type scale. The "friendly grid" character (gridlines, a tinted
//! header row, rounded status pills) lives in the components.

use gpui_bootstrap::{ColorMode, Platform, Theme, theme::mix};
use gpui_kit::{App, FontWeight, Hsla, Rgba, px, rgb};

/// The app icon's teal (`branding/README.md`). Primary buttons, focus rings
/// and selection use it, always with white text.
pub const TEAL: u32 = 0x0DB9B6;
/// Link text: the teal darkened to 4.5:1 on white.
pub const LINK: u32 = 0x0A7C7A;
const LINK_HOVER: u32 = 0x086664;
/// Link text on dark surfaces.
const LINK_DARK: u32 = 0x5FD8D5;
const LINK_DARK_HOVER: u32 = 0x8FE6E3;
/// The brand navy, Bootstrap's `dark`.
pub const NAVY: u32 = 0x10171D;

/// The desktop platforms ixtable ships on, in styleguide order.
pub const DESKTOP: [Platform; 3] = [Platform::MacOS, Platform::Windows, Platform::Linux];

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

/// The ixtable theme for `platform` in `mode`.
pub fn theme(platform: Platform, mode: ColorMode) -> Theme {
    let mut theme = Theme::platform(platform, mode);
    let colors = &mut theme.colors;
    colors.primary = hex(TEAL);
    colors.dark = hex(NAVY);
    (colors.link_color, colors.link_hover_color) = match mode {
        ColorMode::Light => (hex(LINK), hex(LINK_HOVER)),
        ColorMode::Dark => (hex(LINK_DARK), hex(LINK_DARK_HOVER)),
    };
    // White on the teal is 2.4:1. Zn chose white text for the brand accent
    // (design-language.md §4), so the threshold sits just under it. Warning
    // and info fills stay below it and keep dark text.
    theme.min_contrast_ratio = 2.2;
    // One type scale on every platform, so `Type` and Bootstrap's controls
    // agree; the font family stays the platform's own.
    let type_ = &mut theme.typography;
    type_.font_size_base = px(14.);
    type_.font_size_sm = px(12.);
    type_.font_size_lg = px(16.);
    // h5 is Bootstrap's dialog and card title: `Type::Title`, so a dialog
    // reads as one. Headings share the type scale's semibold.
    type_.headings = [px(30.), px(24.), px(20.), px(16.), px(20.), px(12.)];
    type_.headings_font_weight = FontWeight::SEMIBOLD;
    theme
}

/// WCAG body text: 4.5:1.
pub const TEXT_CONTRAST: f32 = 4.5;

/// The WCAG contrast ratio between two colors, 1 to 21.
pub fn contrast_ratio(a: Hsla, b: Hsla) -> f32 {
    fn luminance(color: Hsla) -> f32 {
        let rgba: Rgba = color.into();
        let channel = |c: f32| {
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(rgba.r) + 0.7152 * channel(rgba.g) + 0.0722 * channel(rgba.b)
    }
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `color` as text on `bg`: unchanged when it reaches `min`, otherwise mixed
/// toward `toward` (usually the body text) in small steps until it does.
pub fn readable(color: Hsla, bg: Hsla, min: f32, toward: Hsla) -> Hsla {
    (0..=10)
        .map(|step| mix(toward, color, step as f32 / 10.))
        .find(|candidate| contrast_ratio(*candidate, bg) >= min)
        .unwrap_or(toward)
}

/// The theme for the OS this binary runs on.
pub fn native(mode: ColorMode) -> Theme {
    theme(Platform::current(), mode)
}

/// Installs gpui-kit, gpui-bootstrap and the native ixtable theme in light
/// mode, unless a theme is already installed.
pub fn init(cx: &mut App) {
    gpui_bootstrap::init(cx);
    let installed = Theme::global(cx);
    let (platform, mode) = match installed.platform {
        Platform::Web => (Platform::current(), installed.mode),
        platform => (platform, installed.mode),
    };
    theme(platform, mode).apply(cx);
}

/// Switches color mode on the installed platform. Use this instead of
/// `Theme::change_mode`, which would reset the link colors.
pub fn change_mode(mode: ColorMode, cx: &mut App) {
    theme(Theme::global(cx).platform, mode).apply(cx);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_bootstrap::{ActiveTheme as _, Variant};
    use gpui_kit::{TestAppContext, white};

    #[gpui_kit::test]
    fn primary_is_teal_with_white_text(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init(cx);
            let theme = cx.theme();
            assert_eq!(theme.platform, Platform::current());
            assert_eq!(theme.colors.primary, hex(TEAL));
            assert_eq!(theme.button(Variant::Primary).color, white());
            assert_eq!(
                gpui_kit::component::ActiveTheme::theme(cx).primary_foreground,
                white()
            );
        });
    }

    #[gpui_kit::test]
    fn pale_fills_keep_dark_text(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init(cx);
            for platform in DESKTOP {
                let theme = theme(platform, ColorMode::Light);
                let warning = theme.colors.variant(Variant::Warning);
                assert_eq!(theme.contrast(warning), theme.colors.black, "{platform:?}");
            }
        });
    }

    #[test]
    fn dialog_titles_use_the_title_role() {
        use crate::typography::Type;
        for platform in DESKTOP {
            let theme = theme(platform, ColorMode::Light);
            // Bootstrap's modal title is h5.
            assert_eq!(theme.typography.headings[4], px(Type::Title.size()));
            assert_eq!(theme.typography.headings_font_weight, Type::Title.weight());
        }
    }

    #[gpui_kit::test]
    fn dark_mode_keeps_the_brand(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init(cx);
            change_mode(ColorMode::Dark, cx);
            let theme = cx.theme();
            assert_eq!(theme.mode, ColorMode::Dark);
            assert_eq!(theme.colors.primary, hex(TEAL));
            assert_eq!(theme.colors.link_color, hex(LINK_DARK));
            assert_eq!(theme.platform, Platform::current());
        });
    }

    #[gpui_kit::test]
    fn every_desktop_platform_keeps_its_look_with_the_brand(cx: &mut TestAppContext) {
        cx.update(|cx| {
            init(cx);
            for platform in DESKTOP {
                for mode in [ColorMode::Light, ColorMode::Dark] {
                    let native = Theme::platform(platform, mode);
                    let ours = theme(platform, mode);
                    assert_eq!(ours.colors.primary, hex(TEAL));
                    assert_eq!(ours.contrast(ours.colors.primary), white());
                    assert_eq!(ours.colors.body_bg, native.colors.body_bg);
                    assert_eq!(ours.radius, native.radius);
                    assert_eq!(ours.typography.font_family, native.typography.font_family);
                }
            }
        });
    }
}
