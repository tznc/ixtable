//! Screenshot harness for visual QA (the `/app-qa` skill), replacing the
//! former jsdom + Chromium capture.
//!
//! A screenshot spec drives the real `AppView` through the same `Harness` as
//! the UI tests. The harness runs on GPUI's `HeadlessAppContext`, which has the
//! platform text system and the headless GPU renderer: Metal on macOS, and
//! wgpu on Linux (Vulkan; Mesa's lavapipe works without a GPU). `capture`
//! renders the window to a PNG and writes a JSON manifest next to it. The
//! manifest lists the visual expectations a reviewer checks against the
//! pixels.
//!
//! Specs live in `ui::screenshots` and are `#[ignore]`d. Run them with
//! `cargo screenshot` (an alias in `.cargo/config.toml`). Output goes to
//! `$IXTABLE_SCREENSHOT_DIR`, or by default to
//! `scripts/screenshot/.generated/`.

use super::testing::{Harness, UiContext};
use gpui_kit::{AnyView, AnyWindowHandle, App, HeadlessAppContext, Window};
use serde_json::json;
use std::path::PathBuf;

/// A headless app context that renders real pixels.
pub fn context() -> HeadlessAppContext {
    let text_system = gpui_kit::platform::current_platform(true).text_system();
    let cx = HeadlessAppContext::with_platform(
        text_system,
        std::sync::Arc::new(super::assets::AppAssets),
        gpui_kit::platform::current_headless_renderer,
    );
    // Image and font assets load on real I/O.
    cx.allow_parking();
    cx
}

impl UiContext for HeadlessAppContext {
    fn update<R>(&mut self, f: impl FnOnce(&mut App) -> R) -> R {
        HeadlessAppContext::update(self, f)
    }
    fn read<R>(&self, f: impl FnOnce(&App) -> R) -> R {
        f(&self.app.borrow())
    }
    fn update_window<R>(
        &mut self,
        window: AnyWindowHandle,
        f: impl FnOnce(AnyView, &mut Window, &mut App) -> R,
    ) -> gpui_kit::Result<R> {
        HeadlessAppContext::update_window(self, window, f)
    }
    fn run_until_parked(&self) {
        HeadlessAppContext::run_until_parked(self)
    }
    fn advance_clock(&self, duration: std::time::Duration) {
        HeadlessAppContext::advance_clock(self, duration)
    }
    fn screenshot(&mut self, window: AnyWindowHandle) -> Option<image::RgbaImage> {
        Some(
            self.capture_screenshot(window)
                .expect("headless renderer (Linux needs a Vulkan driver, e.g. mesa-vulkan-drivers)"),
        )
    }
}

/// Captures that are also documentation images, as (capture, file under `web/docs/assets/`).
/// `IXTABLE_SCREENSHOT_DOCS=1 cargo screenshot` refreshes them.
pub const DOCS_IMAGES: &[(&str, &str)] = &[
    ("app-qa-03-data", "data-view.png"),
    ("app-qa-03-data-canvas", "data-view-canvas.png"),
    ("app-qa-04-inline-record", "inline-record.png"),
    ("app-qa-06-form-builder", "design-view.png"),
    ("app-qa-07-app-preview", "app-preview.png"),
];

/// Where captures go.
pub fn output_dir() -> PathBuf {
    std::env::var_os("IXTABLE_SCREENSHOT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/screenshot/.generated")
        })
}

impl<C: UiContext> Harness<'_, C> {
    /// Renders the window to `<name>.png` with a `<name>.json` manifest listing
    /// 1–3 plain-English `expectations` to check against the pixels. Returns the PNG path.
    pub fn capture(&mut self, name: &str, expectations: &[&str]) -> PathBuf {
        assert!(
            (1..=3).contains(&expectations.len()),
            "{name}: give 1-3 visual expectations"
        );
        self.settle();
        let image = self
            .cx
            .screenshot(self.window)
            .expect("capture needs a screenshot context (ui::screenshot::context)");
        let dir = output_dir();
        std::fs::create_dir_all(&dir).expect("create screenshot dir");
        let png = dir.join(format!("{name}.png"));
        image.save(&png).expect("write screenshot");
        let manifest = json!({
            "name": name,
            "image": format!("{name}.png"),
            "width": image.width(),
            "height": image.height(),
            "expectations": expectations,
        });
        std::fs::write(
            dir.join(format!("{name}.json")),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .expect("write manifest");
        let docs = std::env::var("IXTABLE_SCREENSHOT_DOCS").is_ok_and(|v| v == "1");
        if let Some((_, file)) = DOCS_IMAGES.iter().find(|(capture, _)| *capture == name) {
            if docs {
                let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../web/docs/assets")
                    .join(file);
                std::fs::copy(&png, target).expect("copy docs image");
            }
        }
        png
    }
}
