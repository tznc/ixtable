//! The desktop UI: GPUI views rendered into the Tauri window by tauri-plugin-gpui and
//! built from gpui-kit components. There is no WebView; views call the backend
//! command functions directly (docs/decisions/gpui-ui.md).

pub mod app;
pub mod assets;
pub mod automation;
pub mod autosave;
pub mod dialogs;
pub mod events;
pub mod prelude;
pub mod shell;
pub mod start;
pub mod start_sections;
pub mod store;
pub mod task;
pub mod widgets;
#[cfg(test)]
pub mod screenshot;
#[cfg(test)]
mod screenshots;
#[cfg(test)]
pub mod testing;

use gpui_kit::*;
use std::sync::Arc;

/// The label of the app's one window; backend sessions are keyed by it.
pub const MAIN_WINDOW: &str = "main";

/// App-wide setup: gpui-kit, key bindings and the dialog provider.
pub fn init(dialogs: Arc<dyn dialogs::DialogProvider>, cx: &mut App) {
    gpui_kit::init(cx);
    shell::bind_keys(cx);
    cx.set_global(dialogs::Dialogs(dialogs));
}

pub fn is_runtime_bundle(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".ixtr")
}

/// The main window's root, for OS open requests.
struct MainRoot(WeakEntity<app::AppView>);
impl Global for MainRoot {}

/// Opens the main window with the UI attached. Called from Tauri's `setup`.
pub fn launch(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::Manager;
    use tauri_plugin_gpui::{GpuiConfig, GpuiOptions, GpuiWindowExt};
    let handle = app.handle().clone();
    tauri_plugin_gpui::init_with(
        app,
        GpuiConfig::new()
            .assets(assets::AppAssets)
            .on_launch(move |cx| init(Arc::new(dialogs::TauriDialogs(handle)), cx)),
    )?;
    let window = tauri::WindowBuilder::new(app, MAIN_WINDOW)
        .title("ixtable")
        .inner_size(1200.0, 800.0)
        .min_inner_size(800.0, 600.0)
        .center()
        .build()?;
    window.attach_gpui_view(GpuiOptions::default(), |window, cx| {
        let view = cx.new(|cx| app::AppView::new(MAIN_WINDOW, window, cx));
        cx.set_global(MainRoot(view.downgrade()));
        // Files the app was launched with (an OS file association).
        let files = crate::take_launch_files(MAIN_WINDOW.into()).unwrap_or_default();
        if let Some(path) = files.into_iter().next() {
            view.update(cx, |v, cx| v.open_request(path, cx));
        }
        cx.new(|cx| base::Root::new(view, window, cx))
    })?;
    let _ = app.get_window(MAIN_WINDOW);
    Ok(())
}

/// Delivers files the OS asked to open to the main window (only the first is opened).
pub fn open_files(files: Vec<String>) {
    let Some(path) = files.into_iter().next() else {
        return;
    };
    let _ = tauri_plugin_gpui::with_app(move |cx| {
        let root = cx.try_global::<MainRoot>().and_then(|r| r.0.upgrade());
        if let Some(root) = root {
            root.update(cx, |v, cx| v.open_request(path, cx));
        }
    });
}
