// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use oxbow_core::{
    Action, CommitBrief, CommitDetail, ConflictFile, DeletionCheck, DiffContext, DiffOptions, Failure, FileDiff,
    History, HistoryOptions, MergePreview, Plan, Repo, Side, StashCheck, WorkingTree,
};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State, Theme, WebviewUrl, WebviewWindowBuilder};

/// The repository open in the window.
#[derive(Default)]
struct Session {
    repo: Mutex<Option<Repo>>,
    /// Diff settings, given to the repository on every call.
    diff: Mutex<DiffOptions>,
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
    let mut repo = session
        .repo
        .lock()
        .expect("session lock")
        .clone()
        .ok_or_else(|| "No repository is open".to_owned())?;
    repo.set_diff_options(*session.diff.lock().expect("session lock"));
    Ok(repo)
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
fn get_setting(app: AppHandle, key: String) -> Option<Value> {
    read_settings(&app).remove(&key)
}

/// Every setting that differs from its default.
#[tauri::command]
fn all_settings(app: AppHandle) -> serde_json::Map<String, Value> {
    read_settings(&app)
}

#[derive(Clone, Serialize)]
struct SettingChanged {
    key: String,
    value: Value,
}

/// Save a setting (`null` puts it back to its default) and tell every window.
#[tauri::command]
fn set_setting(app: AppHandle, session: State<'_, Session>, key: String, value: Value) -> CommandResult<()> {
    let mut settings = read_settings(&app);
    if value.is_null() {
        settings.remove(&key);
    } else {
        settings.insert(key.clone(), value.clone());
    }
    let file = settings_file(&app)?;
    std::fs::create_dir_all(file.parent().expect("settings file has a parent")).map_err(|err| err.to_string())?;
    let text = serde_json::to_string_pretty(&settings).map_err(|err| err.to_string())?;
    std::fs::write(file, text + "\n").map_err(|err| err.to_string())?;
    apply_settings(&app, &session, &settings);
    // A window that misses this keeps showing the old value until it reloads; not an error.
    let _ = app.emit("settings-changed", SettingChanged { key, value });
    Ok(())
}

/// The settings the backend itself follows: how diffs are made and the windows' appearance.
fn apply_settings(app: &AppHandle, session: &Session, settings: &serde_json::Map<String, Value>) {
    let defaults = DiffOptions::default();
    *session.diff.lock().expect("session lock") = DiffOptions {
        context_lines: settings
            .get("oxbow.diff.contextLines")
            .and_then(Value::as_u64)
            .map_or(defaults.context_lines, |n| n.min(100) as u32),
        ignore_whitespace: settings
            .get("oxbow.diff.ignoreWhitespace")
            .and_then(Value::as_bool)
            .unwrap_or(defaults.ignore_whitespace),
    };
    let theme = appearance(settings);
    for window in app.webview_windows().values() {
        let _ = window.set_theme(theme);
    }
}

/// `oxbow.appearance`: light, dark, or `None` to follow the system.
fn appearance(settings: &serde_json::Map<String, Value>) -> Option<Theme> {
    match settings.get("oxbow.appearance").and_then(Value::as_str) {
        Some("light") => Some(Theme::Light),
        Some("dark") => Some(Theme::Dark),
        _ => None,
    }
}

/// Open the Settings window. Async, because making a window in a synchronous command
/// deadlocks on Windows.
#[tauri::command]
async fn open_settings(app: AppHandle) -> CommandResult<()> {
    show_settings(&app)
}

/// Open the Settings window, or bring it to the front.
fn show_settings(app: &AppHandle) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window("settings") {
        return window.set_focus().map_err(|err| err.to_string());
    }
    let builder = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html#settings".into()))
        .title("Settings")
        .inner_size(940.0, 720.0)
        .min_inner_size(760.0, 520.0)
        .theme(appearance(&read_settings(app)));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(22.0, 24.0));
    builder.build().map(|_| ()).map_err(|err| err.to_string())
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

/// Repository to open at start: the first command-line argument, else the last one used,
/// unless the user turned that off.
#[tauri::command]
fn initial_repo(app: AppHandle) -> Option<String> {
    let reopen = read_settings(&app)
        .get("oxbow.startup.reopenRepository")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    std::env::args()
        .nth(1)
        .filter(|arg| !arg.starts_with('-'))
        .or_else(|| {
            reopen
                .then(|| last_repo_file(&app).and_then(|file| std::fs::read_to_string(file).ok()))
                .flatten()
        })
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

/// What merging `branch` into the checked-out branch would bring, for the merge sheet.
#[tauri::command]
async fn merge_preview(session: State<'_, Session>, branch: String) -> CommandResult<MergePreview> {
    let repo = current(&session)?;
    blocking(move || repo.merge_preview(&branch)).await
}

/// Both sides of a conflicted file, for the Conflicts screen.
#[tauri::command]
async fn conflict_file(session: State<'_, Session>, path: String) -> CommandResult<ConflictFile> {
    let repo = current(&session)?;
    blocking(move || repo.conflict_file(&path)).await
}

/// Whether `stash@{index}` applies cleanly to the checked-out commit.
#[tauri::command]
async fn stash_check(session: State<'_, Session>, index: usize, id: String) -> CommandResult<StashCheck> {
    let repo = current(&session)?;
    blocking(move || repo.stash_check(index, &id)).await
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

/// The default macOS menu, with Settings… (⌘,) after About in the app menu.
#[cfg(target_os = "macos")]
fn app_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let menu = tauri::menu::Menu::default(app)?;
    if let Some(app_menu) = menu.items()?.first().and_then(|item| item.as_submenu().cloned()) {
        let settings = tauri::menu::MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
        app_menu.insert(&settings, 2)?;
        app_menu.insert(&tauri::menu::PredefinedMenuItem::separator(app)?, 3)?;
    }
    Ok(menu)
}

fn main() {
    let builder = tauri::Builder::default();
    // Other systems have no menu bar; the window's own ⌘, / Ctrl+, opens Settings there.
    #[cfg(target_os = "macos")]
    let builder = builder.menu(app_menu);
    builder
        .plugin(tauri_plugin_dialog::init())
        // Restores the window's size and position from the last session.
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .manage(Session::default())
        .on_menu_event(|app, event| {
            if event.id() == "settings" {
                let _ = show_settings(app);
            }
        })
        .setup(|app| {
            let handle = app.handle();
            apply_settings(handle, &handle.state::<Session>(), &read_settings(handle));
            Ok(())
        })
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
            merge_preview,
            conflict_file,
            stash_check,
            plan_action,
            perform_action,
            stop_action,
            get_setting,
            all_settings,
            set_setting,
            open_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running Oxbow");
}
