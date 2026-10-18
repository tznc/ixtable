//! Extra OS windows beside the main one (the Run App window). The app's windows are Tauri
//! windows with GPUI attached (GPUI's own `open_window` is unsupported under
//! tauri-plugin-gpui), so `ui::launch` installs the Tauri host. Without it (tests,
//! screenshots) windows open through GPUI.

use gpui_kit::*;

/// How new windows are created.
pub enum WindowHost {
    Gpui,
    Tauri(tauri::AppHandle),
}
impl Global for WindowHost {}

/// A window to open: `key` is the Tauri window label (unique per window).
pub struct WindowSpec {
    pub key: String,
    pub title: String,
    pub size: (f32, f32),
}

/// Opens a window whose root view `build` creates, wrapped in gpui-kit's `Root`. Under
/// Tauri the view is built after the current GPUI update returns. A GPUI window takes the
/// size of the first open window, so test and screenshot layouts match the main window.
pub fn open<V: Render>(
    spec: WindowSpec,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
    cx: &mut App,
) -> Result<(), String> {
    match cx.try_global::<WindowHost>() {
        Some(WindowHost::Tauri(app)) => open_tauri(app.clone(), spec, build),
        _ => {
            let first = cx.windows().into_iter().next();
            let viewport = first.and_then(|w| w.update(cx, |_, window, _| window.viewport_size()).ok());
            let bounds = Bounds {
                origin: Point::default(),
                size: viewport.unwrap_or(size(px(spec.size.0), px(spec.size.1))),
            };
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some(spec.title.into()),
                    ..Default::default()
                }),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, build).map(|_| ()).map_err(|e| e.to_string())
        }
    }
}

fn open_tauri<V: Render>(
    app: tauri::AppHandle,
    spec: WindowSpec,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V> + 'static,
) -> Result<(), String> {
    use tauri::Manager as _;
    use tauri_plugin_gpui::{GpuiOptions, GpuiWindowExt as _};
    // A window still being mounted or torn down keeps its label.
    if app.get_window(&spec.key).is_some() {
        return Ok(());
    }
    let window = tauri::WindowBuilder::new(&app, &spec.key)
        .title(&spec.title)
        .inner_size(f64::from(spec.size.0), f64::from(spec.size.1))
        .min_inner_size(640.0, 480.0)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
    window
        .attach_gpui_view(GpuiOptions::default(), move |window, cx| {
            let view = build(window, cx);
            cx.new(|cx| base::Root::new(view, window, cx))
        })
        .map_err(|e| e.to_string())
}
