//! File pickers and confirmations. Production uses tauri-plugin-dialog; headless tests
//! install `TestDialogs` with queued answers. Every call returns a GPUI task, because
//! native dialogs block and must not run on the UI thread.

use gpui_kit::*;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug)]
pub struct FileFilter {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

pub const IXT: FileFilter = FileFilter {
    name: "ixtable Document",
    extensions: &["ixt"],
};

pub trait DialogProvider: Send + Sync {
    /// Blocking: runs on a background thread.
    fn pick_file(&self, title: &str, filters: &[FileFilter]) -> Option<PathBuf>;
    fn pick_folder(&self, title: &str) -> Option<PathBuf>;
    fn save_file(&self, title: &str, default_name: &str, filters: &[FileFilter])
    -> Option<PathBuf>;
    /// OK/Cancel question; true for OK.
    fn confirm(&self, title: &str, message: &str) -> bool;
}

#[derive(Clone)]
pub struct Dialogs(pub Arc<dyn DialogProvider>);
impl Global for Dialogs {}

fn provider(cx: &App) -> Arc<dyn DialogProvider> {
    cx.global::<Dialogs>().0.clone()
}

pub fn pick_file(cx: &App, title: &str, filters: &'static [FileFilter]) -> Task<Option<PathBuf>> {
    let (p, title) = (provider(cx), title.to_string());
    cx.background_spawn(async move { p.pick_file(&title, filters) })
}

pub fn pick_folder(cx: &App, title: &str) -> Task<Option<PathBuf>> {
    let (p, title) = (provider(cx), title.to_string());
    cx.background_spawn(async move { p.pick_folder(&title) })
}

pub fn save_file(
    cx: &App,
    title: &str,
    default_name: &str,
    filters: &'static [FileFilter],
) -> Task<Option<PathBuf>> {
    let (p, title, name) = (provider(cx), title.to_string(), default_name.to_string());
    cx.background_spawn(async move { p.save_file(&title, &name, filters) })
}

pub fn confirm(cx: &App, title: &str, message: &str) -> Task<bool> {
    let (p, title, message) = (provider(cx), title.to_string(), message.to_string());
    cx.background_spawn(async move { p.confirm(&title, &message) })
}

/// Native dialogs through tauri-plugin-dialog.
pub struct TauriDialogs(pub tauri::AppHandle);

impl DialogProvider for TauriDialogs {
    fn pick_file(&self, title: &str, filters: &[FileFilter]) -> Option<PathBuf> {
        use tauri_plugin_dialog::DialogExt;
        let mut builder = self.0.dialog().file().set_title(title);
        for f in filters {
            builder = builder.add_filter(f.name, f.extensions);
        }
        builder.blocking_pick_file()?.into_path().ok()
    }
    fn pick_folder(&self, title: &str) -> Option<PathBuf> {
        use tauri_plugin_dialog::DialogExt;
        let builder = self.0.dialog().file().set_title(title);
        builder.blocking_pick_folder()?.into_path().ok()
    }
    fn save_file(
        &self,
        title: &str,
        default_name: &str,
        filters: &[FileFilter],
    ) -> Option<PathBuf> {
        use tauri_plugin_dialog::DialogExt;
        let mut builder = self
            .0
            .dialog()
            .file()
            .set_title(title)
            .set_file_name(default_name);
        for f in filters {
            builder = builder.add_filter(f.name, f.extensions);
        }
        builder.blocking_save_file()?.into_path().ok()
    }
    fn confirm(&self, title: &str, message: &str) -> bool {
        use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
        self.0
            .dialog()
            .message(message)
            .title(title)
            .buttons(MessageDialogButtons::OkCancel)
            .blocking_show()
    }
}

/// Queued answers for headless tests. An empty queue answers None (canceled) or, for
/// `confirm`, true. Every request is recorded in `calls`.
#[derive(Default)]
pub struct TestDialogs {
    pub opens: Mutex<VecDeque<Option<PathBuf>>>,
    pub saves: Mutex<VecDeque<Option<PathBuf>>>,
    pub confirms: Mutex<VecDeque<bool>>,
    pub calls: Mutex<Vec<String>>,
}

impl TestDialogs {
    pub fn queue_open(&self, path: Option<PathBuf>) {
        self.opens.lock().unwrap().push_back(path);
    }
    pub fn queue_save(&self, path: Option<PathBuf>) {
        self.saves.lock().unwrap().push_back(path);
    }
    pub fn queue_confirm(&self, answer: bool) {
        self.confirms.lock().unwrap().push_back(answer);
    }
    /// How many requests started with `kind` ("open", "folder", "save", "confirm").
    pub fn count(&self, kind: &str) -> usize {
        let calls = self.calls.lock().unwrap();
        calls.iter().filter(|c| c.starts_with(kind)).count()
    }
    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }
}

impl DialogProvider for TestDialogs {
    fn pick_file(&self, title: &str, _: &[FileFilter]) -> Option<PathBuf> {
        self.record(format!("open {title}"));
        self.opens.lock().unwrap().pop_front().flatten()
    }
    fn pick_folder(&self, title: &str) -> Option<PathBuf> {
        self.record(format!("folder {title}"));
        self.opens.lock().unwrap().pop_front().flatten()
    }
    fn save_file(&self, title: &str, _: &str, _: &[FileFilter]) -> Option<PathBuf> {
        self.record(format!("save {title}"));
        self.saves.lock().unwrap().pop_front().flatten()
    }
    fn confirm(&self, title: &str, message: &str) -> bool {
        self.record(format!("confirm {title}: {message}"));
        self.confirms.lock().unwrap().pop_front().unwrap_or(true)
    }
}
