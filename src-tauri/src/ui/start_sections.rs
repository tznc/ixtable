//! Extra start-screen entry points owned by feature modules (Access import, runtime
//! bundles, cloud apps, crash recovery). A feature adds its button in `actions` (built
//! every render) or its panel view in `panels` (built once with the screen).

use super::{persistence::recovery_list::RecoveryList, start::StartScreen};
use gpui_kit::*;

/// Buttons next to New and Open.
pub fn actions(screen: &Entity<StartScreen>, busy: bool, cx: &mut App) -> Vec<AnyElement> {
    let _ = (screen, busy, cx);
    vec![]
}

/// Panels between the banners and the template list.
pub fn panels(screen: &Entity<StartScreen>, window: &mut Window, cx: &mut App) -> Vec<AnyView> {
    let _ = window;
    vec![cx.new(|cx| RecoveryList::new(screen, cx)).into()]
}

/// Opens a runtime bundle (.ixtr) the OS asked to open.
pub fn open_bundle(screen: &mut StartScreen, path: String, cx: &mut Context<StartScreen>) {
    let _ = path;
    screen.set_notice("Runtime bundles open from the Release flow.", cx);
}
