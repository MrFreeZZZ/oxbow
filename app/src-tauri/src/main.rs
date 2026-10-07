// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod open_in;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use oxbow_core::config::{self, ConfigScope};
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
    config::set_git_program(
        settings
            .get("oxbow.git.path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from),
    );
    config::set_english_output(
        settings
            .get("oxbow.git.englishOutput")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    );
    config::set_run_hooks(
        settings
            .get("oxbow.git.runHooks")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    );
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
    // Settings shows this repository under This Repository.
    let _ = app.emit("repo-changed", ());
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
async fn compare(
    session: State<'_, Session>,
    base: String,
    target: String,
    mode: oxbow_core::CompareMode,
) -> CommandResult<oxbow_core::Comparison> {
    let repo = current(&session)?;
    blocking(move || repo.compare(&base, &target, mode)).await
}

#[tauri::command]
async fn compare_diff(
    session: State<'_, Session>,
    from: String,
    to: String,
    path: Option<String>,
    whole_file: bool,
) -> CommandResult<Vec<FileDiff>> {
    let repo = current(&session)?;
    let context = if whole_file {
        DiffContext::WholeFile
    } else {
        DiffContext::Compact
    };
    blocking(move || repo.tree_diff(&from, &to, path.as_deref(), context)).await
}

#[tauri::command]
async fn file_history(
    session: State<'_, Session>,
    path: String,
    rev: Option<String>,
) -> CommandResult<oxbow_core::FileHistory> {
    let repo = current(&session)?;
    blocking(move || repo.file_history(&path, rev.as_deref())).await
}

#[tauri::command]
async fn blame(session: State<'_, Session>, path: String, rev: String) -> CommandResult<oxbow_core::Blame> {
    let repo = current(&session)?;
    blocking(move || repo.blame(&path, &rev)).await
}

#[tauri::command]
async fn line_history(session: State<'_, Session>, path: String, line: u32, rev: String) -> CommandResult<Vec<String>> {
    let repo = current(&session)?;
    blocking(move || repo.line_history(&path, line, &rev)).await
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
async fn perform_action(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    action: Action,
) -> Result<String, Failure> {
    // Only the window that asked shows the output: Settings runs actions of its own.
    let label = window.label().to_owned();
    let from_settings = label != "main";
    let repo = current(&session).map_err(|message| Failure {
        kind: oxbow_core::FailureKind::Other,
        output: message,
        incoming: Vec::new(),
        remote_tip: None,
    })?;
    let cancel = session.cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    let events = app.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let result = repo.perform_with(
            &action,
            &mut |event| {
                let _ = events.emit_to(label.as_str(), "action-event", event);
            },
            &cancel,
        );
        result.map_err(|err| repo.explain_failure(&action, &err))
    })
    .await;
    // The main window shows the remotes and branches Settings just changed.
    if from_settings {
        let _ = app.emit_to("main", "repo-touched", ());
    }
    joined.unwrap_or_else(|err| {
        Err(Failure {
            kind: oxbow_core::FailureKind::Other,
            output: err.to_string(),
            incoming: Vec::new(),
            remote_tip: None,
        })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitSettings {
    git: oxbow_core::GitInfo,
    /// Everything in ~/.gitconfig.
    global: std::collections::BTreeMap<String, String>,
    ssh_keys: Vec<oxbow_core::SshKey>,
    editors: Vec<open_in::App>,
    terminals: Vec<open_in::App>,
}

/// What Settings shows from Git's own config and the computer.
#[tauri::command]
async fn git_settings() -> CommandResult<GitSettings> {
    blocking(|| {
        Ok(GitSettings {
            git: config::git_info(),
            global: config::global_config(),
            ssh_keys: config::ssh_keys(),
            editors: open_in::editors(),
            terminals: open_in::terminals(),
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoSettings {
    name: String,
    path: String,
    /// Everything in the repository's .git/config.
    local: std::collections::BTreeMap<String, String>,
    remotes: Vec<oxbow_core::RemoteInfo>,
    storage: Option<oxbow_core::Storage>,
}

/// Settings › This Repository, or nothing when no repository is open.
#[tauri::command]
async fn repo_settings(session: State<'_, Session>) -> CommandResult<Option<RepoSettings>> {
    let Ok(repo) = current(&session) else {
        return Ok(None);
    };
    blocking(move || {
        Ok(Some(RepoSettings {
            name: repo.name(),
            path: repo.workdir().display().to_string(),
            local: repo.local_config(),
            remotes: repo.remotes_info()?,
            storage: repo.storage().ok(),
        }))
    })
    .await
}

/// Change a value in ~/.gitconfig or the open repository's .git/config; `None` removes it.
#[tauri::command]
async fn set_git_config(
    session: State<'_, Session>,
    scope: ConfigScope,
    key: String,
    value: Option<String>,
) -> CommandResult<()> {
    let repo = match scope {
        ConfigScope::Local => Some(current(&session)?),
        ConfigScope::Global => None,
    };
    blocking(move || match repo {
        Some(repo) => repo.set_local_config(&key, value.as_deref()),
        None => config::set_global_config(&key, value.as_deref()),
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PullSetup {
    mode: oxbow_core::PullMode,
    autostash: bool,
}

/// How Pull will go here, for the words of its confirmation.
#[tauri::command]
async fn pull_setup(session: State<'_, Session>) -> CommandResult<PullSetup> {
    let repo = current(&session)?;
    blocking(move || {
        Ok(PullSetup {
            mode: repo.pull_mode(),
            autostash: repo.pull_autostash(),
        })
    })
    .await
}

/// Fetch every remote in the background: no sheet, and a failure is only reported.
#[tauri::command]
async fn background_fetch(session: State<'_, Session>) -> CommandResult<()> {
    let repo = current(&session)?;
    blocking(move || {
        let mut command = repo.fetch_command(None);
        command.args.insert(1, "--quiet".to_owned());
        repo.run(&command).map(|_| ())
    })
    .await
}

/// Search the history: messages, code changes, authors or file paths.
#[tauri::command]
async fn search(
    session: State<'_, Session>,
    query: oxbow_core::SearchQuery,
) -> CommandResult<oxbow_core::SearchResult> {
    let repo = current(&session)?;
    blocking(move || repo.search(&query)).await
}

/// Ask the default remote which tags it has, so tags it lacks show as local. True when that
/// changed. Without a remote there is nothing to ask.
#[tauri::command]
async fn refresh_remote_tags(session: State<'_, Session>) -> CommandResult<bool> {
    let repo = current(&session)?;
    blocking(move || match repo.default_remote() {
        Some(remote) => repo.refresh_remote_tags(&remote),
        None => Ok(false),
    })
    .await
}

/// Open a file of the repository in the editor chosen in Settings.
#[tauri::command]
fn open_in_editor(app: AppHandle, session: State<'_, Session>, path: String, line: Option<u32>) -> CommandResult<()> {
    let repo = current(&session)?;
    let editor = read_settings(&app)
        .get("oxbow.openIn.editor")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| open_in::editors().first().map(|app| app.id.to_owned()))
        .ok_or("No editor found. Install one, then pick it in Settings › Integrations.")?;
    open_in::open_file(&editor, &repo.workdir().join(Path::new(&path)), line)
}

/// The monospaced font families installed on this computer, sorted, for Settings › Diff & Text.
#[tauri::command]
async fn monospace_fonts() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut fonts = fontdb::Database::new();
        fonts.load_system_fonts();
        let mut families: Vec<String> = fonts
            .faces()
            .filter(|face| face.monospaced)
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            // Hidden system fonts (".SF NS Mono") are not meant to be picked by name, and emoji
            // fonts call themselves monospaced.
            .filter(|name| !name.starts_with('.') && !name.contains("Emoji"))
            .collect();
        families.sort_by_key(|name| name.to_lowercase());
        families.dedup();
        families
    })
    .await
    .unwrap_or_default()
}

/// Open the repository folder in the terminal chosen in Settings.
#[tauri::command]
fn open_in_terminal(app: AppHandle, session: State<'_, Session>) -> CommandResult<()> {
    let repo = current(&session)?;
    let terminal = read_settings(&app)
        .get("oxbow.openIn.terminal")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| open_in::terminals().first().map(|app| app.id.to_owned()))
        .ok_or("No terminal found.")?;
    open_in::open_terminal(&terminal, repo.workdir())
}

/// settings.json as it is on disk, for editing it as text.
#[tauri::command]
fn settings_text(app: AppHandle) -> CommandResult<String> {
    let file = settings_file(&app)?;
    Ok(std::fs::read_to_string(file).unwrap_or_else(|_| "{}\n".to_owned()))
}

/// Save settings.json edited as text. It must be a JSON object; every window takes the new
/// values at once.
#[tauri::command]
fn save_settings_text(app: AppHandle, session: State<'_, Session>, text: String) -> CommandResult<()> {
    let settings: serde_json::Map<String, Value> = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    let before = read_settings(&app);
    let file = settings_file(&app)?;
    std::fs::create_dir_all(file.parent().expect("settings file has a parent")).map_err(|err| err.to_string())?;
    std::fs::write(&file, if text.ends_with('\n') { text } else { text + "\n" }).map_err(|err| err.to_string())?;
    apply_settings(&app, &session, &settings);
    let removed = before
        .keys()
        .filter(|key| !settings.contains_key(*key))
        .map(|key| (key.clone(), Value::Null));
    for (key, value) in settings.clone().into_iter().chain(removed) {
        if before.get(&key) != Some(&value) {
            let _ = app.emit("settings-changed", SettingChanged { key, value });
        }
    }
    Ok(())
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
            compare,
            compare_diff,
            file_history,
            blame,
            line_history,
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
            open_settings,
            git_settings,
            repo_settings,
            set_git_config,
            background_fetch,
            pull_setup,
            open_in_editor,
            open_in_terminal,
            monospace_fonts,
            refresh_remote_tags,
            search,
            settings_text,
            save_settings_text
        ])
        .run(tauri::generate_context!())
        .expect("error while running Oxbow");
}
