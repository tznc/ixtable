//! The ixtable theme: Bootstrap's tokens with the app icon's teal as the
//! accent, a 14px base for dense business screens, and calm surfaces.

use gpui_bootstrap::{ColorMode, Platform, Theme};
use gpui_kit::{App, Hsla, px, rgb};

/// The app icon's teal (`branding/README.md`). Primary buttons, focus rings
/// and selection use it, always with white text.
pub const TEAL: u32 = 0x0DB9B6;
/// Link text: the teal darkened to 4.5:1 on white.
pub const LINK: u32 = 0x0A7C7A;
const LINK_HOVER: u32 = 0x086664;
/// Link text on the dark surface.
const LINK_DARK: u32 = 0x5FD8D5;
/// The brand navy, Bootstrap's `dark`.
pub const NAVY: u32 = 0x10171D;

fn hex(value: u32) -> Hsla {
    rgb(value).into()
}

/// The ixtable theme for `mode`.
pub fn theme(mode: ColorMode) -> Theme {
    let mut theme = Theme::platform(Platform::Web, mode);
    let colors = &mut theme.colors;
    colors.primary = hex(TEAL);
    colors.dark = hex(NAVY);
    (colors.link_color, colors.link_hover_color) = match mode {
        ColorMode::Light => (hex(LINK), hex(LINK_HOVER)),
        ColorMode::Dark => (hex(LINK_DARK), hex(0x8FE6E3)),
    };
    if mode == ColorMode::Dark {
        colors.body_bg = hex(0x151C22);
        colors.secondary_bg = hex(0x1D262E);
        colors.tertiary_bg = hex(0x192128);
    }
    // White on the teal is 2.4:1. Zn chose white text for the brand accent
    // (design-language.md §4), so the threshold sits just under it. Warning
    // and info fills stay below it and keep dark text.
    theme.min_contrast_ratio = 2.2;
    let type_ = &mut theme.typography;
    type_.font_size_base = px(14.);
    type_.font_size_sm = px(12.);
    type_.font_size_lg = px(16.);
    type_.line_height = 1.45;
    type_.headings = [px(30.), px(24.), px(20.), px(16.), px(14.), px(12.)];
    // Controls are a touch tighter than Bootstrap's: forms and toolbars are dense.
    theme.controls.medium.padding_y = px(5.);
    theme.controls.medium.padding_x = px(12.);
    theme
}

/// Installs gpui-kit, gpui-bootstrap and the ixtable theme in light mode,
/// unless a theme is already installed.
pub fn init(cx: &mut App) {
    gpui_bootstrap::init(cx);
    let mode = Theme::global(cx).mode;
    theme(mode).apply(cx);
}

/// Switches color mode. Use this instead of `Theme::change_mode`, which
/// would reset the link colors to Bootstrap's blue.
pub fn change_mode(mode: ColorMode, cx: &mut App) {
    theme(mode).apply(cx);
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
            let theme = cx.theme();
            for variant in [Variant::Warning, Variant::Info] {
                assert_eq!(
                    theme.contrast(theme.colors.variant(variant)),
                    theme.colors.black
                );
            }
        });
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
        });
    }
}
