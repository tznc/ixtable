// GPUI element builders nest deeply.
#![recursion_limit = "512"]
pub mod access;
pub mod archive;
pub mod archive_io;
pub mod asset_data;
pub mod assets;
pub mod authz;
pub mod automation;
pub mod automation_runner;
pub mod bundle;
pub mod bundle_export;
pub mod checkpoints;
pub mod cloud;
pub mod dashboards;
pub mod data;
pub mod design;
pub mod export;
pub mod expr;
pub mod records;
pub mod import;
pub mod installation;
pub mod installation_checks;
pub mod installation_commands;
pub mod jobs;
pub mod logging;
pub mod manager;
pub mod migrations;
pub mod paths;
pub mod postgres;
pub mod queries;
pub mod record_attachments;
pub mod recordstore;
pub mod recovery;
pub mod report_pdf;
pub mod reports;
pub mod roles;
pub mod storage;
pub mod templates;
pub mod trigger_auth;
pub mod ui;
pub mod validation;

use archive::{Attachment, DocumentConfig};
use manager::{AppError, DocumentManager, SessionState};
use serde_json::Value;
use std::{path::PathBuf, sync::OnceLock};

static MANAGER: OnceLock<DocumentManager> = OnceLock::new();
pub fn manager() -> Result<&'static DocumentManager, AppError> {
    if let Some(m) = MANAGER.get() {
        return Ok(m);
    }
    let base = paths::state_dir();
    let _ = MANAGER.set(DocumentManager::new(base.join("data"), base.join("cache"))?);
    Ok(MANAGER.get().unwrap())
}

pub fn app_info() -> Value {
    serde_json::json!({"name":"ixtable","runtime":"tauri"})
}
pub fn new_document(window_label: String) -> Result<SessionState, AppError> {
    manager()?.new_session(&window_label)
}
pub fn open_document(window_label: String, path: String) -> Result<SessionState, AppError> {
    manager()?.open(&window_label, &PathBuf::from(path))
}
pub fn document_state(window_label: String) -> Result<SessionState, AppError> {
    manager()?.state(&window_label)
}
pub fn read_document_config(window_label: String) -> Result<DocumentConfig, AppError> {
    manager()?.config(&window_label)
}
pub fn update_document_config(
    window_label: String,
    config: DocumentConfig,
) -> Result<SessionState, AppError> {
    manager()?.update_config(&window_label, config)
}
pub fn read_document_config_yaml(window_label: String) -> Result<String, AppError> {
    manager()?.config_yaml(&window_label)
}
pub fn apply_document_config_yaml(
    window_label: String,
    yaml: String,
) -> Result<SessionState, AppError> {
    manager()?.apply_config_yaml(&window_label, &yaml)
}
pub fn save_document(window_label: String) -> Result<SessionState, AppError> {
    manager()?.save(&window_label, None)
}
pub fn save_document_as(window_label: String, path: String) -> Result<SessionState, AppError> {
    manager()?.save(&window_label, Some(PathBuf::from(path)))
}
pub fn reload_document(window_label: String) -> Result<SessionState, AppError> {
    manager()?.reload(&window_label)
}
pub fn close_document(window_label: String, force: bool) -> Result<(), AppError> {
    manager()?.close(&window_label, force)
}
pub fn import_attachment(
    window_label: String,
    path: String,
    media_type: String,
) -> Result<SessionState, AppError> {
    manager()?.import_attachment(&window_label, &PathBuf::from(path), &media_type)
}
pub fn list_attachments(window_label: String) -> Result<Vec<Attachment>, AppError> {
    manager()?.asset_list(&window_label)
}
pub fn export_attachment(window_label: String, id: String, path: String) -> Result<(), AppError> {
    authz::require_unrestricted(&window_label, "export attachments")?;
    manager()?.export_attachment(&window_label, &id, &PathBuf::from(path))
}
pub fn remove_attachment(window_label: String, id: String) -> Result<SessionState, AppError> {
    manager()?.remove_attachment(&window_label, &id)
}
pub fn get_preference(key: String) -> Result<Option<Value>, AppError> {
    manager()?
        .global
        .preference(&key)
        .map_err(|e| AppError::new("IO_ERROR", e))
}
pub fn set_preference(key: String, value: Value) -> Result<(), AppError> {
    manager()?
        .global
        .set_preference(&key, &value)
        .map_err(|e| AppError::new("IO_ERROR", e))
}
pub fn list_recent_files() -> Result<Vec<storage::RecentFile>, AppError> {
    manager()?
        .global
        .recents()
        .map_err(|e| AppError::new("IO_ERROR", e))
}
pub fn list_recovery_sessions() -> Result<Vec<storage::RecoveryRecord>, AppError> {
    manager()?.recoverable_sessions()
}
pub fn discard_recovery(session_id: String) -> Result<(), AppError> {
    manager()?.discard_recovery(&session_id)
}
/// Under a role, only the tables (and views) it may read are listed.
pub fn list_database_objects(window_label: String) -> Result<Vec<data::DbObject>, AppError> {
    let objects = manager()?.database_objects(&window_label)?;
    manager()?.with_session(&window_label, |s| {
        Ok(objects
            .into_iter()
            .filter(|o| authz::can_read_table(s, &o.name))
            .collect())
    })
}
/// `trigger` marks a read made by an app-mode trigger step (trigger_auth.rs).
pub fn inspect_table(
    window_label: String,
    table: String,
    trigger: Option<trigger_auth::TriggerWrite>,
) -> Result<data::TableSchema, AppError> {
    trigger_auth::authorize(&window_label, &table, authz::Op::Read, trigger.as_ref())?;
    manager()?.table_schema(&window_label, &table)
}
#[allow(clippy::too_many_arguments)]
pub fn read_table_page(
    window_label: String,
    table: String,
    offset: u64,
    limit: u64,
    sorts: Vec<data::Sort>,
    filters: Vec<data::Filter>,
    trigger: Option<trigger_auth::TriggerWrite>,
) -> Result<data::Page, AppError> {
    trigger_auth::authorize(&window_label, &table, authz::Op::Read, trigger.as_ref())?;
    manager()?.table_page(&window_label, &table, offset, limit, &sorts, &filters)
}
/// The datasheet totals row (src/data/sheet): one aggregate per column over the filtered rows.
pub fn read_table_totals(
    window_label: String,
    table: String,
    filters: Vec<data::Filter>,
    totals: Vec<data::totals::TotalSpec>,
) -> Result<Vec<data::DataValue>, AppError> {
    trigger_auth::authorize(&window_label, &table, authz::Op::Read, None)?;
    manager()?.table_totals(&window_label, &table, &filters, &totals)
}
pub fn execute_read_query(window_label: String, sql: String) -> Result<data::QueryResult, AppError> {
    authz::require_unrestricted(&window_label, "run ad hoc SQL")?;
    manager()?.read_query(&window_label, &sql)
}
pub fn create_database_table(
    window_label: String,
    spec: data::CreateTable,
) -> Result<SessionState, AppError> {
    installation::ensure_studio(&window_label)?;
    recordstore::create_table(&window_label, &spec)
}
pub fn alter_database_table(
    window_label: String,
    table: String,
    operation: data::AlterTable,
) -> Result<SessionState, AppError> {
    installation::ensure_studio(&window_label)?;
    recordstore::alter_table(&window_label, &table, &[operation])
}
pub fn save_query(
    window_label: String,
    id: Option<String>,
    name: String,
    sql: String,
    filter_state: Option<Value>,
) -> Result<DocumentConfig, AppError> {
    manager()?.save_query(&window_label, id, name, sql, filter_state)
}
pub fn delete_saved_query(window_label: String, id: String) -> Result<DocumentConfig, AppError> {
    let mut c = manager()?.config(&window_label)?;
    let before = c.saved_queries.len();
    c.saved_queries.retain(|x| x.id != id);
    if before == c.saved_queries.len() {
        return Err(AppError::new("NOT_FOUND", "Saved query not found"));
    }
    manager()?.update_config(&window_label, c.clone())?;
    Ok(c)
}

/// Event the main window receives with the `.ixt`/`.ixtr` paths a later launch (or the OS) asked to open.
pub const OPEN_FILES_EVENT: &str = "ixtable://open-files";

/// Whether a path names a file ixtable opens: an `.ixt` document or an `.ixtr` runtime bundle.
pub fn is_openable_file(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ixt") || e.eq_ignore_ascii_case("ixtr"))
}

/// Paths among a launch's arguments that name `.ixt` documents or `.ixtr` bundles, resolved against its cwd.
pub fn open_file_args(args: &[String], cwd: &str) -> Vec<String> {
    args.iter()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .filter(|a| is_openable_file(a))
        .map(|a| {
            std::path::Path::new(cwd)
                .join(a)
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

/// Files this process was asked to open before the UI could listen: the first
/// launch's arguments (an OS file association) and, on macOS, `Opened` events.
fn launch_files() -> &'static std::sync::Mutex<Option<Vec<String>>> {
    static FILES: OnceLock<std::sync::Mutex<Option<Vec<String>>>> = OnceLock::new();
    FILES.get_or_init(|| {
        let args: Vec<String> = std::env::args().collect();
        let cwd = std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default();
        std::sync::Mutex::new(Some(open_file_args(&args, &cwd)))
    })
}

/// Returns the files the app was launched with, once; later calls return none
/// (later requests arrive as `ixtable://open-files` events).
pub fn take_launch_files(window_label: String) -> Result<Vec<String>, AppError> {
    let _ = window_label;
    let mut files = launch_files().lock().unwrap_or_else(|e| e.into_inner());
    Ok(files.replace(vec![]).unwrap_or_default())
}

/// Delivers files the OS asked to open to the main window.
#[cfg_attr(not(any(target_os = "macos", target_os = "ios")), allow(dead_code))]
fn deliver_open_files(_app: &tauri::AppHandle, files: Vec<String>) {
    if !files.is_empty() {
        ui::open_files(files);
    }
}

fn forward_open_args(app: &tauri::AppHandle, args: Vec<String>, cwd: String) {
    use tauri::Manager;
    if let Some(window) = app.get_window(ui::MAIN_WINDOW) {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    deliver_open_files(app, open_file_args(&args, &cwd));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Registered first: a second launch hands its file arguments to this process and exits, so two processes never share the state dir.
        .plugin(tauri_plugin_single_instance::init(forward_open_args))
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use tauri::Manager;
            // Durable state lives in the app's local data dir, resolved before any command runs.
            paths::init_app_dir(app.path().app_local_data_dir()?);
            // Panics leave a redacted record in the local diagnostic log (PRD §27.5).
            logging::install_panic_hook();
            // Read the first launch's file arguments before anything changes the cwd.
            let _ = launch_files();
            ui::launch(app)?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building ixtable")
        .run(|_app, _event| {
            // macOS hands file-association opens to the running app as events, not arguments.
            #[cfg(any(target_os = "macos", target_os = "ios"))]
            if let tauri::RunEvent::Opened { urls, .. } = _event {
                let files = urls
                    .iter()
                    .filter_map(|u| u.to_file_path().ok())
                    .map(|p| p.to_string_lossy().into_owned())
                    .filter(|p| is_openable_file(p))
                    .collect();
                deliver_open_files(_app, files);
            }
        })
}

#[cfg(test)]
mod durability_tests;
// build.rs owns this module; the lib compiles it only for its unit tests.
#[cfg(test)]
mod perf_tests;
#[cfg(test)]
mod release_keys;
#[cfg(test)]
mod test_env;

#[cfg(test)]
mod launch_args_tests {
    #[test]
    fn a_second_launch_forwards_only_ixt_paths_resolved_against_its_cwd() {
        let args = [
            "ixtable",
            "--flag",
            "notes.IXT",
            "/abs/b.ixt",
            "readme.txt",
            "app.ixtr",
        ]
        .map(String::from);
        let files = super::open_file_args(&args, "/home/u");
        assert_eq!(files.len(), 3);
        assert_eq!(
            std::path::Path::new(&files[2]),
            std::path::Path::new("/home/u/app.ixtr")
        );
        assert_eq!(
            std::path::Path::new(&files[0]),
            std::path::Path::new("/home/u/notes.IXT")
        );
        assert_eq!(
            std::path::Path::new(&files[1]),
            std::path::Path::new("/abs/b.ixt")
        );
    }

    #[test]
    fn launch_files_are_taken_once() {
        // The test binary's own arguments name no documents.
        assert!(super::take_launch_files("main".into()).unwrap().is_empty());
        super::launch_files()
            .lock()
            .unwrap()
            .replace(vec!["/docs/a.ixt".into()]);
        assert_eq!(
            super::take_launch_files("main".into()).unwrap(),
            vec!["/docs/a.ixt".to_string()]
        );
        assert!(super::take_launch_files("main".into()).unwrap().is_empty());
    }
}
