// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod open_in;
mod pulls;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use oxbow_core::config::{self, ConfigScope};
use oxbow_core::{
    Action, CommitBrief, CommitDetail, ConflictFile, DeletionCheck, DiffContext, DiffOptions, Failure, FileDiff,
    History, HistoryOptions, MergePreview, Plan, Repo, Side, Source, StashCheck, WorkingTree,
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
    /// Stop of the fetch running in the background, apart from actions in the sheet.
    fetch_cancel: Arc<AtomicBool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RepoSummary {
    name: String,
    path: String,
}

type CommandResult<T> = Result<T, String>;

/// The repository, set to show a diff in full however many lines it has when `full` is set
/// (Show Diff Anyway).
fn current_full(session: &State<'_, Session>, full: Option<bool>) -> CommandResult<Repo> {
    let mut repo = current(session)?;
    if full == Some(true) {
        repo.set_diff_options(DiffOptions {
            max_lines: 0,
            ..repo.diff_options()
        });
    }
    Ok(repo)
}

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
        max_lines: settings
            .get("oxbow.diff.maxLines")
            .and_then(Value::as_u64)
            .map_or(defaults.max_lines, |n| n.min(10_000_000) as u32),
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
    oxbow_core::oplog::set_keep_days(
        settings
            .get("oxbow.undo.keepDays")
            .and_then(Value::as_u64)
            .map_or(30, |days| days.min(3650) as u32),
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
    remember_repo(&app, &summary.path);
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

/// Repositories opened before, newest first, kept in `recent-repositories.json`.
#[derive(Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RecentRepo {
    path: String,
    /// When it was last opened, in seconds since 1970.
    opened: i64,
    /// Filled in when listed: the folder name, and whether the folder is gone.
    #[serde(default)]
    name: String,
    #[serde(default)]
    missing: bool,
}

/// How many repositories Recent Repositories keeps.
const RECENT_LIMIT: usize = 40;

fn recent_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("recent-repositories.json"))
}

fn read_recent(app: &AppHandle) -> Vec<RecentRepo> {
    let saved = recent_file(app)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| serde_json::from_str::<Vec<RecentRepo>>(&text).ok());
    match saved {
        Some(list) => list,
        // Before the list existed, Oxbow remembered only the last repository.
        None => last_repo_file(app)
            .and_then(|file| std::fs::read_to_string(file).ok())
            .map(|path| path.trim().to_owned())
            .filter(|path| !path.is_empty())
            .map(|path| {
                vec![RecentRepo {
                    path,
                    opened: now(),
                    name: String::new(),
                    missing: false,
                }]
            })
            .unwrap_or_default(),
    }
}

fn write_recent(app: &AppHandle, list: &[RecentRepo]) {
    let Some(file) = recent_file(app) else { return };
    let _ = std::fs::create_dir_all(file.parent().expect("config file has a parent"));
    if let Ok(text) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(file, text + "\n");
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Put `path` at the top of Recent Repositories.
fn remember_repo(app: &AppHandle, path: &str) {
    let mut list = read_recent(app);
    list.retain(|r| r.path != path);
    list.insert(
        0,
        RecentRepo {
            path: path.to_owned(),
            opened: now(),
            name: String::new(),
            missing: false,
        },
    );
    list.truncate(RECENT_LIMIT);
    write_recent(app, &list);
}

#[tauri::command]
fn recent_repos(app: AppHandle) -> Vec<RecentRepo> {
    read_recent(&app)
        .into_iter()
        .map(|mut r| {
            let path = Path::new(&r.path);
            r.name = path
                .file_name()
                .map_or_else(|| r.path.clone(), |n| n.to_string_lossy().into_owned());
            r.missing = !path.is_dir();
            r
        })
        .collect()
}

/// Take a repository off Recent Repositories; with `to`, it moved there (Locate…).
#[tauri::command]
fn forget_repo(app: AppHandle, path: String, to: Option<String>) {
    let mut list = read_recent(&app);
    match to {
        Some(to) => {
            list.retain(|r| r.path != to);
            if let Some(entry) = list.iter_mut().find(|r| r.path == path) {
                entry.path = to;
            }
        }
        None => list.retain(|r| r.path != path),
    }
    write_recent(&app, &list);
}

/// Where a recent repository stands, for its row.
#[tauri::command]
async fn repo_glance(path: String) -> CommandResult<oxbow_core::RepoGlance> {
    blocking(move || oxbow_core::setup::glance(Path::new(&path))).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WelcomeInfo {
    version: String,
    git: oxbow_core::GitInfo,
    name: Option<String>,
    email: Option<String>,
    ssh: oxbow_core::SshSource,
    /// `init.defaultBranch`, when set.
    default_branch: Option<String>,
    home: String,
    /// Where clones and new repositories go unless the user picks another folder.
    projects: String,
    gitignores: Vec<(String, String)>,
    licenses: Vec<(String, String)>,
}

/// What the Welcome window says about this computer's Git setup.
#[tauri::command]
async fn welcome_info(app: AppHandle) -> CommandResult<WelcomeInfo> {
    let version = app.package_info().version.to_string();
    blocking(move || {
        let global = config::global_config();
        let home = config::home_dir().unwrap_or_default();
        let projects = ["Developer", "Projects", "projects", "src", "Code", "code"]
            .iter()
            .map(|name| home.join(name))
            .find(|dir| dir.is_dir())
            .unwrap_or_else(|| home.clone());
        Ok(WelcomeInfo {
            version,
            git: config::git_info(),
            name: global.get("user.name").cloned(),
            email: global.get("user.email").cloned(),
            ssh: config::ssh_source(),
            default_branch: global.get("init.defaultbranch").cloned(),
            home: home.display().to_string(),
            projects: projects.display().to_string(),
            gitignores: oxbow_core::setup::GITIGNORES
                .iter()
                .map(|(key, name, _)| ((*key).to_owned(), (*name).to_owned()))
                .collect(),
            licenses: oxbow_core::setup::LICENSES
                .iter()
                .map(|(key, name, _)| ((*key).to_owned(), (*name).to_owned()))
                .collect(),
        })
    })
    .await
}

/// Whether a folder is free to clone into: `missing`, `empty`, `files` or `file`.
#[tauri::command]
fn folder_state(path: String) -> &'static str {
    let path = Path::new(&path);
    if !path.exists() {
        "missing"
    } else if !path.is_dir() {
        "file"
    } else if std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none()) {
        "empty"
    } else {
        "files"
    }
}

#[tauri::command]
async fn probe_remote(
    app: tauri::AppHandle,
    accounts: State<'_, accounts::Accounts>,
    url: String,
) -> CommandResult<oxbow_core::RemoteProbe> {
    // Signed in, a private repository on github.com can count its commits too.
    let mut github = oxbow_core::github::Client::default();
    if let Some(token) = accounts.token(&app) {
        github = github.with_token(token);
    }
    blocking(move || oxbow_core::setup::probe_remote(&url, &github)).await
}

#[tauri::command]
fn clone_command(options: oxbow_core::CloneOptions) -> CommandResult<oxbow_core::GitCommand> {
    oxbow_core::setup::clone_command(&options).map_err(|err| err.to_string())
}

/// Clone, sending each line git prints to the window as a `clone-line`. Stop (`stop_action`)
/// ends it and removes what it made.
#[tauri::command]
async fn clone_repo(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    options: oxbow_core::CloneOptions,
) -> CommandResult<String> {
    let cancel = session.cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    let path = options.path.display().to_string();
    blocking(move || {
        oxbow_core::setup::clone(
            &options,
            &mut |line| {
                let _ = window.emit_to(window.label(), "clone-line", line);
            },
            &cancel,
        )
    })
    .await?;
    Ok(path)
}

#[tauri::command]
async fn plan_new_repo(options: oxbow_core::NewRepoOptions) -> CommandResult<oxbow_core::NewRepoPlan> {
    blocking(move || oxbow_core::setup::plan_new_repo(&options)).await
}

#[tauri::command]
async fn create_repo(options: oxbow_core::NewRepoOptions) -> CommandResult<String> {
    let path = options.path.display().to_string();
    blocking(move || oxbow_core::setup::create_repo(&options)).await?;
    Ok(path)
}

#[tauri::command]
async fn history(session: State<'_, Session>) -> CommandResult<History> {
    let repo = current(&session)?;
    // Open the repository afresh: the built-in engine reads .git/config once, so a remote that
    // Publish or Settings just added would stay unseen.
    let workdir = repo.workdir().to_path_buf();
    let fresh = blocking(move || Repo::open(&workdir)).await.unwrap_or(repo);
    {
        let mut open = session.repo.lock().expect("session lock");
        if open.as_ref().is_some_and(|open| open.workdir() == fresh.workdir()) {
            *open = Some(fresh.clone());
        }
    }
    let mut repo = fresh;
    repo.set_diff_options(*session.diff.lock().expect("session lock"));
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
    full: Option<bool>,
) -> CommandResult<Vec<FileDiff>> {
    let repo = current_full(&session, full)?;
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
    full: Option<bool>,
) -> CommandResult<Vec<FileDiff>> {
    let repo = current_full(&session, full)?;
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
async fn files(session: State<'_, Session>) -> CommandResult<Vec<String>> {
    let repo = current(&session)?;
    blocking(move || repo.files()).await
}

#[tauri::command]
async fn file_pickaxe(
    session: State<'_, Session>,
    path: String,
    text: String,
    match_case: bool,
    rev: String,
) -> CommandResult<Vec<String>> {
    let repo = current(&session)?;
    blocking(move || repo.file_pickaxe(&path, &text, match_case, &rev)).await
}

/// The Operation Log, newest step first.
#[tauri::command]
async fn operation_log(session: State<'_, Session>) -> CommandResult<Vec<oxbow_core::OpEntry>> {
    let repo = current(&session)?;
    blocking(move || repo.operation_log()).await
}

/// The bytes of one side of a file diff, to show an image.
#[tauri::command]
async fn source_bytes(session: State<'_, Session>, source: Source) -> CommandResult<tauri::ipc::Response> {
    let repo = current(&session)?;
    blocking(move || repo.read_source(&source))
        .await
        .map(tauri::ipc::Response::new)
}

/// Open one side of a file diff in the app the system picks for it: the working-tree file as it
/// is, any other version from a copy in a temporary folder named after it.
#[tauri::command]
async fn open_source(session: State<'_, Session>, source: Source, path: String, label: String) -> CommandResult<()> {
    let repo = current(&session)?;
    let file = match &source {
        Source::Worktree { path } => repo.workdir().join(Path::new(path)),
        Source::Blob { id } => {
            let data = blocking({
                let source = source.clone();
                move || repo.read_source(&source)
            })
            .await?;
            let name = Path::new(&path)
                .file_name()
                .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
            let short: String = id.chars().take(7).collect();
            // `logo.png` becomes `logo (old a3f9c21).png`, so the app's window says which one it is.
            let named = match name.rsplit_once('.') {
                Some((stem, ext)) if !stem.is_empty() => format!("{stem} ({label} {short}).{ext}"),
                _ => format!("{name} ({label} {short})"),
            };
            let dir = std::env::temp_dir().join("oxbow-versions");
            std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
            let file = dir.join(named);
            std::fs::write(&file, data).map_err(|err| err.to_string())?;
            file
        }
    };
    open_in::open_url(&file.display().to_string())
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
    full: Option<bool>,
) -> CommandResult<FileDiff> {
    let repo = current_full(&session, full)?;
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

/// The stack `branch` belongs to (the checked-out branch without one), for Edit Stack.
#[tauri::command]
async fn stack(session: State<'_, Session>, branch: Option<String>) -> CommandResult<oxbow_core::Stack> {
    let repo = current(&session)?;
    blocking(move || repo.stack(branch.as_deref())).await
}

/// A dry run of an Edit Stack plan: what it changes, where it would stop, what to push.
#[tauri::command]
async fn stack_preview(
    session: State<'_, Session>,
    plan: oxbow_core::StackPlan,
) -> CommandResult<oxbow_core::StackPreview> {
    let repo = current(&session)?;
    blocking(move || repo.stack_preview(&plan)).await
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
        hook: None,
        ssh: None,
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
            hook: None,
            ssh: None,
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

/// How a background fetch ended.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Fetched {
    /// Remote branches and tags that are new, moved or gone.
    updated: u32,
}

/// Fetch one remote or all in the background: no sheet, git's progress goes to the main window
/// as `fetch-event`s, and a failure comes back explained, for Activity.
#[tauri::command]
async fn fetch_in_background(
    app: AppHandle,
    session: State<'_, Session>,
    remote: Option<String>,
) -> Result<Fetched, Failure> {
    let failed = |output: String| Failure {
        kind: oxbow_core::FailureKind::Other,
        output,
        incoming: Vec::new(),
        remote_tip: None,
        hook: None,
        ssh: None,
    };
    let repo = current(&session).map_err(failed)?;
    let cancel = session.fetch_cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        let action = Action::Fetch { remote: remote.clone() };
        let command = repo.fetch_command(remote.as_deref()).with_progress();
        let _ = app.emit_to(
            "main",
            "fetch-event",
            oxbow_core::ActionEvent::Command {
                display: command.display(),
            },
        );
        let mut updated = 0;
        let result = repo.run_streaming(
            &command,
            &mut |line| {
                if !line.progress && oxbow_core::remote::is_ref_update(&line.text) {
                    updated += 1;
                }
                let _ = app.emit_to("main", "fetch-event", oxbow_core::ActionEvent::Line(line));
            },
            &cancel,
        );
        match result {
            Ok(_) => Ok(Fetched { updated }),
            Err(err) => Err(repo.explain_failure(&action, &err)),
        }
    })
    .await
    .unwrap_or_else(|err| Err(failed(err.to_string())))
}

/// Stop the background fetch.
#[tauri::command]
fn stop_fetch(session: State<'_, Session>) {
    session.fetch_cancel.store(true, Ordering::Relaxed);
}

/// The public half of an SSH key, to paste into GitHub.
#[tauri::command]
fn public_key(key: String) -> CommandResult<String> {
    oxbow_core::ssh::public_key(&key).map_err(|err| err.to_string())
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

/// Which files the repository keeps in Git LFS, and whether git-lfs is installed.
#[tauri::command]
async fn lfs_status(session: State<'_, Session>) -> CommandResult<oxbow_core::LfsStatus> {
    let repo = current(&session)?;
    blocking(move || repo.lfs_status()).await
}

/// What Free Up Space would delete, e.g. `3 files would be pruned (1.4 GB)`.
#[tauri::command]
async fn lfs_prune_preview(session: State<'_, Session>) -> CommandResult<Option<String>> {
    let repo = current(&session)?;
    blocking(move || repo.lfs_prune_preview()).await
}

/// The Git LFS download page, for installing it without Homebrew.
#[tauri::command]
fn open_lfs_download() -> CommandResult<()> {
    open_in::open_url("https://git-lfs.com")
}

/// Preview a file of the working copy: Quick Look on macOS, the default app elsewhere.
#[tauri::command]
fn quick_look(session: State<'_, Session>, path: String) -> CommandResult<()> {
    let repo = current(&session)?;
    open_in::quick_look(&repo.workdir().join(Path::new(&path)))
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

/// Show a file of the repository in Finder (Explorer, or the file manager on Linux).
#[tauri::command]
fn reveal_file(session: State<'_, Session>, path: String) -> CommandResult<()> {
    let repo = current(&session)?;
    open_in::reveal(&repo.workdir().join(Path::new(&path)))
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
    settings_replaced(&app, &session, &before, &settings);
    Ok(())
}

/// Follow settings that replaced `before` all at once, and tell every window which keys changed.
fn settings_replaced(
    app: &AppHandle,
    session: &Session,
    before: &serde_json::Map<String, Value>,
    after: &serde_json::Map<String, Value>,
) {
    apply_settings(app, session, after);
    let removed = before
        .keys()
        .filter(|key| !after.contains_key(*key))
        .map(|key| (key.clone(), Value::Null));
    for (key, value) in after.clone().into_iter().chain(removed) {
        if before.get(&key) != Some(&value) {
            let _ = app.emit("settings-changed", SettingChanged { key, value });
        }
    }
}

/// Where the old settings go when they are put back to the defaults.
fn settings_backup(file: &Path) -> PathBuf {
    file.with_file_name("settings.backup.json")
}

/// Rename `from` to `to`, replacing `to` (which a rename on Windows won't do by itself).
fn move_file(from: &Path, to: &Path) -> CommandResult<()> {
    #[cfg(windows)]
    let _ = std::fs::remove_file(to);
    std::fs::rename(from, to).map_err(|err| err.to_string())
}

#[derive(Serialize)]
struct SettingsLocation {
    /// settings.json and its backup as typed in a shell, with `~` for the home folder.
    file: String,
    backup: String,
}

#[tauri::command]
fn settings_location(app: AppHandle) -> CommandResult<SettingsLocation> {
    let file = settings_file(&app)?;
    let home = app.path().home_dir().ok();
    let shown = |path: &Path| match home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    };
    Ok(SettingsLocation {
        file: shown(&file),
        backup: shown(&settings_backup(&file)),
    })
}

/// Put every Oxbow setting back to its default. settings.json moves aside to
/// settings.backup.json, so the old values can come back; Git's own settings are not touched.
#[tauri::command]
fn restore_default_settings(app: AppHandle, session: State<'_, Session>) -> CommandResult<()> {
    let before = read_settings(&app);
    let file = settings_file(&app)?;
    if file.exists() {
        move_file(&file, &settings_backup(&file))?;
    }
    settings_replaced(&app, &session, &before, &serde_json::Map::new());
    Ok(())
}

/// Bring back the settings that Restore Defaults moved aside.
#[tauri::command]
fn restore_settings_backup(app: AppHandle, session: State<'_, Session>) -> CommandResult<()> {
    let file = settings_file(&app)?;
    let backup = settings_backup(&file);
    let text = std::fs::read_to_string(&backup).map_err(|_| "There is no copy of the old settings.".to_owned())?;
    let after: serde_json::Map<String, Value> = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    let before = read_settings(&app);
    move_file(&backup, &file)?;
    settings_replaced(&app, &session, &before, &after);
    Ok(())
}

/// Show settings.json in Finder, Explorer or the file manager; an empty one is made first, so
/// there is a file to show.
#[tauri::command]
fn reveal_settings(app: AppHandle) -> CommandResult<()> {
    let file = settings_file(&app)?;
    if !file.exists() {
        std::fs::create_dir_all(file.parent().expect("settings file has a parent")).map_err(|err| err.to_string())?;
        std::fs::write(&file, "{}\n").map_err(|err| err.to_string())?;
    }
    open_in::reveal(&file)
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
        .manage(accounts::Accounts::default())
        .on_menu_event(|app, event| {
            if event.id() == "settings" {
                let _ = show_settings(app);
            }
        })
        .setup(|app| {
            // Before anything runs git: it must find git-lfs where Homebrew put it.
            oxbow_core::lfs::extend_path();
            let handle = app.handle();
            apply_settings(handle, &handle.state::<Session>(), &read_settings(handle));
            accounts::lend_to_git(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_repo,
            initial_repo,
            recent_repos,
            forget_repo,
            repo_glance,
            welcome_info,
            folder_state,
            probe_remote,
            clone_command,
            clone_repo,
            plan_new_repo,
            create_repo,
            history,
            commit_detail,
            commit_diff,
            compare,
            compare_diff,
            file_history,
            blame,
            line_history,
            file_pickaxe,
            files,
            working_tree,
            working_diff,
            source_bytes,
            open_source,
            deletion_check,
            remote_deletion_check,
            merge_preview,
            conflict_file,
            stash_check,
            stack,
            stack_preview,
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
            fetch_in_background,
            stop_fetch,
            public_key,
            pull_setup,
            open_in_editor,
            open_in_terminal,
            reveal_file,
            quick_look,
            lfs_status,
            lfs_prune_preview,
            open_lfs_download,
            operation_log,
            monospace_fonts,
            refresh_remote_tags,
            search,
            settings_text,
            save_settings_text,
            settings_location,
            restore_default_settings,
            restore_settings_backup,
            reveal_settings,
            accounts::github_account,
            accounts::github_sign_in_setup,
            accounts::github_device_start,
            accounts::github_device_wait,
            accounts::github_device_stop,
            accounts::github_sign_in_token,
            accounts::github_sign_out,
            accounts::open_github,
            accounts::github_owners,
            accounts::github_publish,
            pulls::github_repo,
            pulls::github_pulls,
            pulls::github_pull,
            pulls::github_calls_preview,
            pulls::github_run,
            pulls::code_owners
        ])
        .run(tauri::generate_context!())
        .expect("error while running Oxbow");
}
