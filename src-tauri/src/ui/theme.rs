//! The brand accent (design-language.md §4): the app icon's teal on primary buttons,
//! focus rings and links, with the icon's navy as text on teal. Applied to both
//! registered themes so a light/dark switch keeps it.

use gpui_kit::component::{Theme, ThemeConfig};
use gpui_kit::{App, Hsla, rgb};
use std::rc::Rc;

/// The app icon's teal (`branding/README.md`).
pub const TEAL: u32 = 0x0DB9B6;
/// The app icon's navy, the text color on teal.
pub const NAVY: u32 = 0x10171D;

/// The accent for selection tints and other `Hsla` uses outside the theme.
pub fn teal() -> Hsla {
    rgb(TEAL).into()
}

fn branded(config: &ThemeConfig) -> Rc<ThemeConfig> {
    let mut config = config.clone();
    let colors = &mut config.colors;
    colors.primary = Some("#0DB9B6".into());
    colors.primary_hover = Some("#0BA7A4".into());
    colors.primary_active = Some("#099591".into());
    colors.primary_foreground = Some("#10171D".into());
    colors.ring = Some("#0DB9B6".into());
    // Link text darkened from the teal so it reads on white (4.5:1).
    colors.link = Some("#0A7C7A".into());
    colors.link_hover = Some("#086664".into());
    colors.link_active = Some("#086664".into());
    Rc::new(config)
}

/// Installs the brand accent into the global theme.
pub fn apply(cx: &mut App) {
    Theme::update(cx, |theme| {
        theme.light_theme = branded(&theme.light_theme);
        theme.dark_theme = branded(&theme.dark_theme);
    });
    let mode = Theme::global(cx).mode;
    Theme::change(mode, None, cx);
}

#[cfg(test)]
mod tests {
    use gpui_kit::TestAppContext;
    use gpui_kit::component::ActiveTheme as _;

    #[gpui_kit::test]
    fn primary_is_the_icon_teal_with_navy_text(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            super::apply(cx);
            let theme = cx.theme();
            assert_eq!(theme.primary, super::teal());
            assert_eq!(theme.primary_foreground, gpui_kit::rgb(super::NAVY).into());
        });
    }
}
