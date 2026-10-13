//! Headless UI test harness on gpui-kit's testing API (https://gpui-kit.com/docs/test/).
//!
//! A `Harness` opens the real `AppView` in a gpui-kit test window with its own window
//! label (so tests share the process-wide document manager without colliding),
//! installs `TestDialogs`, and drives the UI through GPUI's event dispatch. Every
//! interaction is followed by `settle`, which runs pending tasks (backend calls run on
//! the test executor) and renders a frame, so the next query sees the result.
//!
//! Elements are found by their GPUI `ElementId` (see each view's module docs for its
//! test ids). Text is read from accessibility labels (`widgets::text`). State that has
//! no element (a store's undo stack, a view's model) is read from the entities.

use super::{
    app::AppView,
    dialogs::TestDialogs,
    shell::{ModeId, Shell},
    start::StartScreen,
};
use crate::{
    archive::DocumentConfig,
    data::{DataValue, NamedValue},
    manager::SessionState,
};
use gpui_kit::test::{ElementSnapshot, TestWindowExt as _};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Bounds, ElementId, Entity, Point, TestAppContext,
    Window, WindowBounds, WindowOptions, px, size,
};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

static NEXT_WINDOW: AtomicUsize = AtomicUsize::new(1);

pub const WINDOW_SIZE: (f32, f32) = (1280.0, 900.0);

pub struct Harness<'a> {
    pub cx: &'a mut TestAppContext,
    pub window: AnyWindowHandle,
    pub root: Entity<AppView>,
    /// The backend session key of this test's window.
    pub label: String,
    pub dialogs: Arc<TestDialogs>,
}

impl<'a> Harness<'a> {
    /// The start screen in a fresh window.
    pub fn start(cx: &'a mut TestAppContext) -> Self {
        let label = format!("test-{}", NEXT_WINDOW.fetch_add(1, Ordering::SeqCst));
        let dialogs = Arc::new(TestDialogs::default());
        let (window, root) = cx.update(|cx| {
            super::init(dialogs.clone(), cx);
            let bounds = Bounds {
                origin: Point::default(),
                size: size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)),
            };
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                cx,
                |window, cx| cx.new(|cx| AppView::new(&label, window, cx)),
            )
            .expect("open test window")
        });
        let mut harness = Self {
            cx,
            window,
            root,
            label,
            dialogs,
        };
        harness.settle();
        harness
    }

    /// A new untitled document, open in the shell.
    pub fn new_document(cx: &'a mut TestAppContext) -> Self {
        let mut h = Self::start(cx);
        h.click("new-document");
        assert!(h.has_shell(), "New document did not open the shell");
        h
    }

    /// Runs pending tasks and renders a frame.
    pub fn settle(&mut self) {
        for _ in 0..3 {
            self.cx.run_until_parked();
            self.update(|window, cx| window.render_frame(cx));
        }
    }

    /// Runs `f` against the window.
    pub fn update<R>(&mut self, f: impl FnOnce(&mut Window, &mut App) -> R) -> R {
        self.cx
            .update_window(self.window, |_, window, cx| f(window, cx))
            .expect("test window is open")
    }

    pub fn click(&mut self, id: impl Into<ElementId>) {
        let id = id.into();
        self.update(|w, cx| {
            w.render_frame(cx);
            w.click(id, cx)
        });
        self.settle();
    }

    /// Clicks `id` inside the scope `scope` (for ids repeated in several places).
    pub fn click_in(&mut self, scope: impl Into<ElementId>, id: impl Into<ElementId>) {
        let (scope, id) = (scope.into(), id.into());
        self.update(|w, cx| {
            w.render_frame(cx);
            w.within(scope).click(id, cx)
        });
        self.settle();
    }

    pub fn double_click(&mut self, id: impl Into<ElementId>) {
        let id = id.into();
        self.update(|w, cx| {
            w.render_frame(cx);
            w.double_click(id, cx)
        });
        self.settle();
    }

    pub fn right_click(&mut self, id: impl Into<ElementId>) {
        let id = id.into();
        self.update(|w, cx| {
            w.render_frame(cx);
            w.right_click(id, cx)
        });
        self.settle();
    }

    /// Sends a key such as "enter", "escape", "ctrl-z" to the focused element.
    pub fn press(&mut self, key: &str) {
        self.update(|w, cx| w.press(key, cx));
        self.settle();
    }

    /// Select-all for the platform (cmd-a on macOS, ctrl-a elsewhere).
    pub fn select_all(&mut self) {
        self.press(if cfg!(target_os = "macos") { "cmd-a" } else { "ctrl-a" });
    }

    /// Types text into the focused element.
    pub fn input(&mut self, text: &str) {
        self.update(|w, cx| w.input(text, cx));
        self.settle();
    }

    /// Focuses the input `id`, replaces its whole text with `text`.
    pub fn fill(&mut self, id: impl Into<ElementId>, text: &str) {
        self.click(id);
        self.select_all();
        self.press("backspace");
        if !text.is_empty() {
            self.input(text);
        }
    }

    /// Focuses the input `id` and appends `text`.
    pub fn type_into(&mut self, id: impl Into<ElementId>, text: &str) {
        self.click(id);
        self.input(text);
    }

    pub fn find(&mut self, id: impl Into<ElementId>) -> ElementSnapshot {
        let id = id.into();
        self.update(|w, _| w.find(id))
    }

    pub fn try_find(&mut self, id: impl Into<ElementId>) -> Option<ElementSnapshot> {
        let id = id.into();
        self.update(|w, _| w.try_find(id))
    }

    pub fn exists(&mut self, id: impl Into<ElementId>) -> bool {
        self.try_find(id).is_some()
    }

    /// The accessibility label of `id` (its text for `widgets::text` elements).
    pub fn label(&mut self, id: impl Into<ElementId>) -> String {
        self.find(id).label().unwrap_or_default().to_string()
    }

    /// The label if `id` exists.
    pub fn try_label(&mut self, id: impl Into<ElementId>) -> Option<String> {
        self.try_find(id)
            .map(|s| s.label().unwrap_or_default().to_string())
    }

    /// Advances GPUI's fake clock (debounced autosave, timers) and settles.
    pub fn advance(&mut self, duration: std::time::Duration) {
        self.cx.executor().advance_clock(duration);
        self.settle();
    }

    // ---- Views --------------------------------------------------------------

    pub fn has_shell(&mut self) -> bool {
        self.cx.read(|cx| self.root.read(cx).shell().is_some())
    }

    pub fn shell(&mut self) -> Entity<Shell> {
        self.cx
            .read(|cx| self.root.read(cx).shell().cloned())
            .expect("a document is open")
    }

    pub fn start_screen(&mut self) -> Entity<StartScreen> {
        self.cx
            .read(|cx| self.root.read(cx).start().cloned())
            .expect("the start screen is showing")
    }

    pub fn read_shell<R>(&mut self, f: impl FnOnce(&Shell, &App) -> R) -> R {
        let shell = self.shell();
        self.cx.read(|cx| f(shell.read(cx), cx))
    }

    pub fn mode(&mut self) -> ModeId {
        self.read_shell(|s, _| s.doc.mode)
    }

    /// Whether the toolbar's Undo has a step (disabled buttons expose no flag to tests).
    pub fn can_undo(&mut self) -> bool {
        self.read_shell(|s, cx| s.store.read(cx).can_undo())
    }

    pub fn can_redo(&mut self) -> bool {
        self.read_shell(|s, cx| s.store.read(cx).can_redo())
    }

    /// Clicks a mode-switch button.
    pub fn switch_mode(&mut self, mode: ModeId) {
        self.click(mode.button_id());
    }

    /// The shell's error banner text ("CODE: message"), if shown.
    pub fn error(&mut self) -> Option<String> {
        self.try_label("error")
    }

    pub fn notice(&mut self) -> Option<String> {
        self.try_label("notice")
    }

    // ---- Backend --------------------------------------------------------------

    pub fn config(&self) -> DocumentConfig {
        crate::read_document_config(self.label.clone()).expect("read config")
    }

    pub fn state(&self) -> SessionState {
        crate::document_state(self.label.clone()).expect("document state")
    }

    /// A path in this test's state dir (unique per window).
    pub fn temp_path(&self, name: &str) -> PathBuf {
        let dir = crate::paths::state_dir().join("ui-tests").join(&self.label);
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir.join(name)
    }

    /// Creates a table through the backend and announces the change to the UI.
    /// `columns`: (name, declared type, primary key position; 0 for none).
    pub fn create_table(&mut self, name: &str, columns: &[(&str, &str, u32)]) {
        let spec = json!({
            "name": name,
            "columns": columns.iter().map(|(n, t, pk)| json!({
                "name": n, "declaredType": t, "nullable": true, "primaryKeyPosition": pk,
                "unique": false, "defaultExpression": null, "generatedExpression": null,
            })).collect::<Vec<_>>(),
            "foreignKeys": [], "checks": [], "withoutRowid": false,
        });
        crate::create_database_table(
            self.label.clone(),
            serde_json::from_value(spec).expect("table spec"),
        )
        .expect("create table");
        self.database_changed();
    }

    /// Inserts a row through the backend (no record hooks) and announces it.
    pub fn insert_row(&mut self, table: &str, values: &[(&str, DataValue)]) {
        let values = values
            .iter()
            .map(|(column, value)| NamedValue {
                column: column.to_string(),
                value: value.clone(),
            })
            .collect();
        crate::recordstore::commands::insert_row(self.label.clone(), table.into(), values, None)
        .expect("insert row");
        self.records_changed();
    }

    /// Announces a DDL change (the React tests' `ixtable:database-changed`).
    pub fn database_changed(&mut self) {
        let shell = self.shell();
        self.cx.update(|cx| shell.update(cx, |s, cx| s.database_changed(cx)));
        self.settle();
    }

    pub fn records_changed(&mut self) {
        let shell = self.shell();
        self.cx.update(|cx| shell.update(cx, |s, cx| s.records_changed(cx)));
        self.settle();
    }

    /// Reads up to 100 rows of a table.
    pub fn read_page(&self, table: &str) -> crate::data::Page {
        crate::read_table_page(
            self.label.clone(),
            table.into(),
            0,
            100,
            vec![],
            vec![],
            None,
        )
        .expect("read page")
    }
}

/// A DataValue's display string (for assertions).
pub fn shown(value: &DataValue) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.get("value").cloned())
        .map(|v| match v {
            Value::String(s) => s,
            other => other.to_string(),
        })
        .unwrap_or_default()
}
