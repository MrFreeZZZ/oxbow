// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use oxbow_core::{
    Action, CommitBrief, CommitDetail, DeletionCheck, DiffContext, Failure, FileDiff, History, HistoryOptions, Plan,
    Repo, Side, WorkingTree,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

/// The repository open in the window.
#[derive(Default)]
struct Session {
    repo: Mutex<Option<Repo>>,
    /// Set by Stop; the running action checks it.
    cancel: Arc<AtomicBool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoSummary {
    name: String,
    path: String,
}

type CommandResult<T> = Result<T, String>;

fn current(session: &State<'_, Session>) -> CommandResult<Repo> {
    session
        .repo
        .lock()
        .expect("session lock")
        .clone()
        .ok_or_else(|| "No repository is open".to_owned())
}

/// Run blocking Git work off the main thread so the window stays responsive.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> oxbow_core::Result<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())
}

/// User settings live in `settings.json` in the app's config folder, as flat `oxbow.*` keys,
/// holding only values that differ from the defaults.
fn settings_file(app: &AppHandle) -> CommandResult<PathBuf> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join("settings.json"))
        .map_err(|err| err.to_string())
}

fn read_settings(app: &AppHandle) -> serde_json::Map<String, serde_json::Value> {
    settings_file(app)
        .ok()
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

#[tauri::command]
fn get_setting(app: AppHandle, key: String) -> Option<serde_json::Value> {
    read_settings(&app).remove(&key)
}

#[tauri::command]
fn set_setting(app: AppHandle, key: String, value: serde_json::Value) -> CommandResult<()> {
    let mut settings = read_settings(&app);
    if value.is_null() {
        settings.remove(&key);
    } else {
        settings.insert(key, value);
    }
    let file = settings_file(&app)?;
    std::fs::create_dir_all(file.parent().expect("settings file has a parent")).map_err(|err| err.to_string())?;
    let text = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    std::fs::write(file, text + "\n").map_err(|err| err.to_string())
}

fn last_repo_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("last-repository"))
}

#[tauri::command]
async fn open_repo(app: AppHandle, session: State<'_, Session>, path: String) -> CommandResult<RepoSummary> {
    let repo = blocking(move || Repo::open(&path)).await?;
    let summary = RepoSummary {
        name: repo.name(),
        path: repo.workdir().display().to_string(),
    };
    if let Some(file) = last_repo_file(&app) {
        // Remembering the repository is a convenience; failing to save it is not an error.
        let _ = std::fs::create_dir_all(file.parent().expect("config file has a parent"));
        let _ = std::fs::write(&file, &summary.path);
    }
    *session.repo.lock().expect("session lock") = Some(repo);
    Ok(summary)
}

/// Repository to open at start: the first command-line argument, else the last one used.
#[tauri::command]
fn initial_repo(app: AppHandle) -> Option<String> {
    std::env::args()
        .nth(1)
        .filter(|arg| !arg.starts_with('-'))
        .or_else(|| last_repo_file(&app).and_then(|file| std::fs::read_to_string(file).ok()))
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
}

#[tauri::command]
async fn history(session: State<'_, Session>) -> CommandResult<History> {
    let repo = current(&session)?;
    blocking(move || repo.history(&HistoryOptions::default())).await
}

#[tauri::command]
async fn commit_detail(session: State<'_, Session>, id: String) -> CommandResult<CommitDetail> {
    let repo = current(&session)?;
    blocking(move || repo.commit_detail(&id)).await
}

#[tauri::command]
async fn commit_diff(
    session: State<'_, Session>,
    id: String,
    path: Option<String>,
    whole_file: bool,
) -> CommandResult<Vec<FileDiff>> {
    let repo = current(&session)?;
    let context = if whole_file {
        DiffContext::WholeFile
    } else {
        DiffContext::Compact
    };
    blocking(move || repo.commit_diff(&id, path.as_deref(), context)).await
}

#[tauri::command]
async fn working_tree(session: State<'_, Session>) -> CommandResult<WorkingTree> {
    let repo = current(&session)?;
    blocking(move || repo.working_tree()).await
}

#[tauri::command]
async fn working_diff(
    session: State<'_, Session>,
    path: String,
    side: Side,
    whole_file: bool,
) -> CommandResult<FileDiff> {
    let repo = current(&session)?;
    blocking(move || repo.working_diff(&path, side, whole_file)).await
}

/// What deleting a local branch would lose, for the confirmation sheet.
#[tauri::command]
async fn deletion_check(session: State<'_, Session>, branch: String) -> CommandResult<DeletionCheck> {
    let repo = current(&session)?;
    blocking(move || repo.deletion_check(&branch)).await
}

/// Commits that deleting a remote branch (`origin/x`) on its remote would lose.
#[tauri::command]
async fn remote_deletion_check(session: State<'_, Session>, branch: String) -> CommandResult<Vec<CommitBrief>> {
    let repo = current(&session)?;
    blocking(move || repo.remote_deletion_check(&branch)).await
}

/// The git commands an action will run, for the confirmation sheet.
#[tauri::command]
async fn plan_action(session: State<'_, Session>, action: Action) -> CommandResult<Plan> {
    let repo = current(&session)?;
    blocking(move || repo.plan(&action)).await
}

/// Run an action, sending each command and output line to the window as an `action-event`. A
/// failure comes back explained, so the sheet can offer the way out.
#[tauri::command]
async fn perform_action(app: AppHandle, session: State<'_, Session>, action: Action) -> Result<String, Failure> {
    let repo = current(&session).map_err(|message| Failure {
        kind: oxbow_core::FailureKind::Other,
        output: message,
        incoming: Vec::new(),
        remote_tip: None,
    })?;
    let cancel = session.cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let result = repo.perform_with(
            &action,
            &mut |event| {
                let _ = app.emit("action-event", event);
            },
            &cancel,
        );
        result.map_err(|err| repo.explain_failure(&action, &err))
    })
    .await;
    joined.unwrap_or_else(|err| {
        Err(Failure {
            kind: oxbow_core::FailureKind::Other,
            output: err.to_string(),
            incoming: Vec::new(),
            remote_tip: None,
        })
    })
}

/// Stop the running action.
#[tauri::command]
fn stop_action(session: State<'_, Session>) {
    session.cancel.store(true, Ordering::Relaxed);
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Restores the window's size and position from the last session.
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(Session::default())
        .invoke_handler(tauri::generate_handler![
            open_repo,
            initial_repo,
            history,
            commit_detail,
            commit_diff,
            working_tree,
            working_diff,
            deletion_check,
            remote_deletion_check,
            plan_action,
            perform_action,
            stop_action,
            get_setting,
            set_setting
        ])
        .run(tauri::generate_context!())
        .expect("error while running Oxbow");
}
