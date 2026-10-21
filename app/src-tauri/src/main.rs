// Hide the extra console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod accounts;
mod console_window;
mod drafts;
mod open_in;
mod pulls;
mod settings;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use oxbow_core::config::{self, ConfigScope};
use oxbow_core::console;
use oxbow_core::{
    Action, CommitBrief, CommitDetail, ConflictFile, DeletionCheck, DiffContext, DiffOptions, DiffPage, Failure,
    FileDiff, FolderCache, FolderState, History, HistoryCache, HistoryOptions, MergePreview, MergeTool, Plan, Repo,
    Side, Source, StashCheck, Submodule, WorkingTree, Worktree,
};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State, Theme, WebviewUrl, WebviewWindowBuilder};

/// What each window has open, by window label: every repository window ("main", and
/// "repo-N" for the ones Open in New Window makes) has its own repository and Stop buttons.
#[derive(Default)]
struct Session {
    windows: Mutex<HashMap<String, Slot>>,
    /// Diff settings, given to the repository on every call.
    diff: Mutex<DiffOptions>,
    /// The repository window used last: the one Settings shows and changes.
    focused: Mutex<Option<String>>,
    /// The folder a new window opens, until it asks for it.
    pending: Mutex<HashMap<String, String>>,
    /// Number of the next new window.
    next: AtomicUsize,
    /// Number of the last reading of the working tree handed out with a history.
    snapshots: AtomicU64,
    /// Commit details and diffs already worked out: a commit never changes.
    commits: Mutex<CommitCache>,
    /// Number of the last change to settings.json. Held while settings.json is read and written,
    /// so two saves at once can't lose each other's key, and sent with every change, so each
    /// window takes the changes in the order they were saved.
    settings: Mutex<u64>,
}

#[derive(Default)]
struct Slot {
    repo: Option<Repo>,
    /// Set by Stop; the running action checks it.
    cancel: Arc<AtomicBool>,
    /// Stop of the fetch running in the background, apart from actions in the sheet.
    fetch_cancel: Arc<AtomicBool>,
    /// The commit rows of the last history, for the next reload to reuse.
    history: HistoryCache,
    /// The working tree read with the last history, by number, for the Changes panel's diffs.
    changes: Option<(u64, Arc<WorkingTree>)>,
    /// The worktrees and submodules the last history listed, whose folders `folder_states` reads.
    folders: Arc<(Vec<Worktree>, Vec<Submodule>)>,
    /// What those folders had when last read.
    folder_cache: FolderCache,
    /// Stop of the read running in each lane (a search, the commit panel's diff): a newer read
    /// in the lane stops the older, whose answer would be dropped anyway.
    lanes: HashMap<String, Arc<AtomicBool>>,
}

impl Slot {
    /// Stop every read running for this window: its repository changed, or it closed.
    fn stop_reads(&mut self) {
        for (_, cancel) in self.lanes.drain() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// What [`Session::commits`] holds.
#[derive(Clone)]
enum Cached {
    Detail(Arc<CommitDetail>),
    Diffs(Arc<Vec<FileDiff>>),
    Page(Arc<DiffPage>),
}

/// Commit details and diffs by what they were asked with, the most recent last, kept under a
/// budget of memory.
#[derive(Default)]
struct CommitCache {
    entries: std::collections::VecDeque<(String, Cached, usize)>,
    bytes: usize,
}

impl CommitCache {
    /// About what the cached answers take, in bytes.
    const BUDGET: usize = 64 * 1024 * 1024;

    fn get(&mut self, key: &str) -> Option<Cached> {
        let at = self.entries.iter().position(|(k, ..)| k == key)?;
        let entry = self.entries.remove(at)?;
        let found = entry.1.clone();
        self.entries.push_back(entry);
        Some(found)
    }

    fn put(&mut self, key: String, value: Cached) {
        let size = match &value {
            Cached::Detail(detail) => 1024 + detail.files.len() * 256 + detail.body.len(),
            Cached::Diffs(diffs) => diffs_size(diffs),
            Cached::Page(page) => diffs_size(&page.diffs),
        };
        // One answer bigger than a quarter of the budget would push out everything else.
        if size > Self::BUDGET / 4 {
            return;
        }
        if let Some(at) = self.entries.iter().position(|(k, ..)| *k == key)
            && let Some((.., old)) = self.entries.remove(at)
        {
            self.bytes -= old;
        }
        self.bytes += size;
        self.entries.push_back((key, value, size));
        while self.bytes > Self::BUDGET {
            let Some((.., old)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old;
        }
    }
}

/// About what `diffs` take in memory, in bytes.
fn diffs_size(diffs: &[FileDiff]) -> usize {
    diffs
        .iter()
        .map(|d| {
            512 + d.file.path.len()
                + d.hunks
                    .iter()
                    .flat_map(|h| &h.lines)
                    .map(|l| 96 + l.text.len() * 2)
                    .sum::<usize>()
        })
        .sum()
}

impl Session {
    /// The repository window `window` acts for: itself, or for Settings the one used last.
    fn label(&self, window: &tauri::WebviewWindow) -> String {
        if window.label() == "settings" {
            self.focused
                .lock()
                .expect("session lock")
                .clone()
                .unwrap_or_else(|| "main".to_owned())
        } else {
            window.label().to_owned()
        }
    }

    fn slot<T>(&self, window: &tauri::WebviewWindow, f: impl FnOnce(&mut Slot) -> T) -> T {
        let label = self.label(window);
        f(self.windows.lock().expect("session lock").entry(label).or_default())
    }

    fn cancel(&self, window: &tauri::WebviewWindow) -> Arc<AtomicBool> {
        self.slot(window, |slot| slot.cancel.clone())
    }

    fn fetch_cancel(&self, window: &tauri::WebviewWindow) -> Arc<AtomicBool> {
        self.slot(window, |slot| slot.fetch_cancel.clone())
    }

    /// A Stop of its own for a new background fetch: one stopped before it, e.g. of the
    /// repository open before, stays stopped.
    fn new_fetch_cancel(&self, window: &tauri::WebviewWindow) -> Arc<AtomicBool> {
        self.slot(window, |slot| {
            slot.fetch_cancel = Arc::new(AtomicBool::new(false));
            slot.fetch_cancel.clone()
        })
    }

    /// A Stop for a new read in `lane`, which stops the one running there. Without a lane the
    /// read runs to its end.
    fn lane(&self, window: &tauri::WebviewWindow, lane: Option<String>) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(lane) = lane
            && let Some(older) = self.slot(window, |slot| slot.lanes.insert(lane, cancel.clone()))
        {
            older.store(true, Ordering::Relaxed);
        }
        cancel
    }

    /// The window that has the repository at `path` open, if any.
    fn window_of(&self, path: &Path) -> Option<String> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.windows
            .lock()
            .expect("session lock")
            .iter()
            .find(|(_, slot)| {
                slot.repo
                    .as_ref()
                    .is_some_and(|r| r.workdir().canonicalize().unwrap_or_else(|_| r.workdir().to_path_buf()) == path)
            })
            .map(|(label, _)| label.clone())
    }
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
fn current_full(session: &Session, window: &tauri::WebviewWindow, full: Option<bool>) -> CommandResult<Repo> {
    let mut repo = current(session, window)?;
    if full == Some(true) {
        repo.set_diff_options(DiffOptions {
            max_lines: 0,
            ..repo.diff_options()
        });
    }
    Ok(repo)
}

/// The repository open in the window `window` acts for.
fn current(session: &Session, window: &tauri::WebviewWindow) -> CommandResult<Repo> {
    let mut repo = session
        .slot(window, |slot| slot.repo.clone())
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

/// The working tree of `repo`, for the Console entry of work on it.
fn at(repo: &Repo) -> PathBuf {
    repo.workdir().to_path_buf()
}

/// [`blocking`], as one entry of the Console of the repository at `dir` (none: outside any
/// repository).
async fn logged<T: Send + 'static>(
    dir: Option<PathBuf>,
    kind: console::EntryKind,
    title: impl Into<String>,
    work: impl FnOnce() -> oxbow_core::Result<T> + Send + 'static,
) -> CommandResult<T> {
    let title = title.into();
    blocking(move || console::scope(dir.as_deref(), kind, title, work)).await
}

/// [`blocking`] for a read of the repository at `dir`, shown in the Console as `title` when it
/// runs git.
async fn read<T: Send + 'static>(
    dir: PathBuf,
    title: &'static str,
    work: impl FnOnce() -> oxbow_core::Result<T> + Send + 'static,
) -> CommandResult<T> {
    logged(Some(dir), console::EntryKind::Read, title, work).await
}

/// User settings live in `settings.json` in the app's config folder, as flat `oxbow.*` keys,
/// holding only values that differ from the defaults.
fn settings_file(app: &AppHandle) -> CommandResult<PathBuf> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join("settings.json"))
        .map_err(|err| err.to_string())
}

/// The settings, or none when settings.json can't be read.
fn read_settings(app: &AppHandle) -> serde_json::Map<String, serde_json::Value> {
    settings_file(app)
        .and_then(|file| settings::read(&file))
        .unwrap_or_default()
}

#[tauri::command]
fn get_setting(app: AppHandle, key: String) -> Option<Value> {
    read_settings(&app).remove(&key)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AllSettings {
    /// The number of the last change in them, as `settings-changed` sends it.
    revision: u64,
    /// Every setting that differs from its default.
    values: serde_json::Map<String, Value>,
    /// Why settings.json can't be read; the defaults are used meanwhile.
    problem: Option<String>,
}

#[tauri::command]
fn all_settings(app: AppHandle, session: State<'_, Session>) -> AllSettings {
    let revision = session.settings.lock().expect("session lock");
    let (values, problem) = match settings_file(&app).and_then(|file| settings::read(&file)) {
        Ok(values) => (values, None),
        Err(problem) => (serde_json::Map::new(), Some(problem)),
    };
    AllSettings {
        revision: *revision,
        values,
        problem,
    }
}

/// The system's Accessibility › Display switches the page can't read itself: WebKit has no media
/// query for Reduce Transparency, and Increase Contrast is read here too so both come from one place.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DisplayOptions {
    reduce_transparency: bool,
    increase_contrast: bool,
}

#[tauri::command]
fn display_options() -> DisplayOptions {
    #[cfg(target_os = "macos")]
    {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        DisplayOptions {
            reduce_transparency: workspace.accessibilityDisplayShouldReduceTransparency(),
            increase_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    DisplayOptions {
        reduce_transparency: false,
        increase_contrast: false,
    }
}

#[derive(Clone, Serialize)]
struct SettingChanged {
    key: String,
    value: Value,
    revision: u64,
}

/// Save a setting (`null` puts it back to its default) and tell every window. Returns the
/// number of the change. Nothing is written when settings.json can't be read, so the other
/// settings in it stay.
#[tauri::command]
fn set_setting(app: AppHandle, session: State<'_, Session>, key: String, value: Value) -> CommandResult<u64> {
    let mut revision = session.settings.lock().expect("session lock");
    let file = settings_file(&app)?;
    let mut settings = settings::read(&file).map_err(|problem| {
        format!("{problem} Oxbow doesn’t write over it until it is fixed in Edit in settings.json.")
    })?;
    if value.is_null() {
        settings.remove(&key);
    } else {
        settings.insert(key.clone(), value.clone());
    }
    settings::write(&file, &settings)?;
    *revision += 1;
    apply_settings(&app, &session, &settings);
    // A window that misses this keeps showing the old value until it reloads; not an error.
    let _ = app.emit(
        "settings-changed",
        SettingChanged {
            key,
            value,
            revision: *revision,
        },
    );
    Ok(*revision)
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
    oxbow_core::set_secrets_check(
        settings
            .get("oxbow.commit.checkSecrets")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        &settings
            .get("oxbow.secrets.patterns")
            .and_then(|list| serde_json::from_value::<Vec<oxbow_core::SecretPattern>>(list.clone()).ok())
            .unwrap_or_default(),
    );
    oxbow_core::oplog::set_keep_days(
        settings
            .get("oxbow.undo.keepDays")
            .and_then(Value::as_u64)
            .map_or(30, |days| days.min(3650) as u32),
    );
    oxbow_core::graph::set_branch_colors(match settings.get("oxbow.branchPalette").and_then(Value::as_str) {
        Some("oxbow") => oxbow_core::graph::MAX_BRANCH_COLORS,
        _ => oxbow_core::graph::DEFAULT_BRANCH_COLORS,
    });
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
async fn open_repo(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
) -> CommandResult<RepoSummary> {
    let repo = blocking(move || Repo::open(&path)).await?;
    let summary = RepoSummary {
        name: repo.name(),
        path: repo.workdir().display().to_string(),
    };
    // The next start opens what the first window had; another worktree's window doesn't count.
    if window.label() == "main"
        && let Some(file) = last_repo_file(&app)
    {
        // Remembering the repository is a convenience; failing to save it is not an error.
        let _ = std::fs::create_dir_all(file.parent().expect("config file has a parent"));
        let _ = std::fs::write(&file, &summary.path);
    }
    remember_repo(&app, &summary.path);
    session.slot(&window, |slot| {
        // What runs for the repository open before is no longer wanted.
        slot.stop_reads();
        slot.repo = Some(repo);
        slot.history = HistoryCache::default();
        slot.changes = None;
        slot.folders = Arc::default();
        slot.folder_cache = FolderCache::default();
    });
    // Settings shows this repository under This Repository.
    let _ = app.emit("repo-changed", ());
    Ok(summary)
}

/// Repository to open at start: the first command-line argument, else the last one used,
/// unless the user turned that off. A window Open in New Window made opens its folder.
#[tauri::command]
fn initial_repo(app: AppHandle, window: tauri::WebviewWindow, session: State<'_, Session>) -> Option<String> {
    if window.label() != "main" {
        return session.pending.lock().expect("session lock").remove(window.label());
    }
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

/// Empty Recent Repositories but for the repositories in `keep` (the one open now).
#[tauri::command]
fn clear_recent(app: AppHandle, keep: Vec<String>) {
    let mut list = read_recent(&app);
    list.retain(|r| keep.contains(&r.path));
    write_recent(&app, &list);
}

/// Where a recent repository stands, for its row.
#[tauri::command]
async fn repo_glance(path: String) -> CommandResult<oxbow_core::RepoGlance> {
    read(PathBuf::from(&path), "Look at a repository", move || {
        oxbow_core::setup::glance(Path::new(&path))
    })
    .await
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
async fn welcome_info() -> CommandResult<WelcomeInfo> {
    let version = format!("{} ({})", env!("OXBOW_VERSION"), env!("OXBOW_COMMIT"));
    logged(None, console::EntryKind::Read, "Read the Git setup", move || {
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
    logged(None, console::EntryKind::Read, "Check the remote", move || {
        oxbow_core::setup::probe_remote(&url, &github)
    })
    .await
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
    let cancel = session.cancel(&window);
    cancel.store(false, Ordering::Relaxed);
    let path = options.path.display().to_string();
    let title = format!("Clone {}", console::mask(&options.url));
    logged(
        Some(options.path.clone()),
        console::EntryKind::Action,
        title,
        move || {
            oxbow_core::setup::clone(
                &options,
                &mut |line| {
                    let _ = window.emit_to(window.label(), "clone-line", line);
                },
                &cancel,
            )
        },
    )
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
    logged(
        Some(options.path.clone()),
        console::EntryKind::Action,
        "Create a repository",
        move || oxbow_core::setup::create_repo(&options),
    )
    .await?;
    Ok(path)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn history(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    limit: Option<usize>,
    until: Option<String>,
    known: Option<String>,
    focus: Option<bool>,
    lane: Option<String>,
) -> CommandResult<History> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    // Open the repository afresh: the built-in engine reads .git/config once, so a remote that
    // Publish or Settings just added would stay unseen.
    let old = repo.clone();
    let fresh = blocking(move || old.reopen()).await.unwrap_or(repo);
    {
        session.slot(&window, |slot| {
            if slot.repo.as_ref().is_some_and(|open| open.workdir() == fresh.workdir()) {
                slot.repo = Some(fresh.clone());
            }
        });
    }
    let mut repo = fresh;
    repo.set_diff_options(*session.diff.lock().expect("session lock"));
    let options = HistoryOptions {
        limit: limit.unwrap_or(oxbow_core::PAGE),
        until,
        focus: focus.unwrap_or(false),
    };
    let mut cache = session.slot(&window, |slot| slot.history.clone());
    let title = if cache.is_empty() {
        "Read the repository"
    } else {
        "Read the repository again"
    };
    let (mut history, cache) = read(at(&repo), title, move || {
        // Stopped by a newer read of the history or another repository: the git it runs is too.
        let history =
            oxbow_core::cli::stoppable(&cancel, || repo.history_with(&options, &mut cache, known.as_deref()))?;
        Ok((history, cache))
    })
    .await?;
    // The diffs the Changes panel asks for next are of these files.
    history.snapshot = session.snapshots.fetch_add(1, Ordering::Relaxed) + 1;
    let changes = history.changes.clone().map(|tree| (history.snapshot, Arc::new(tree)));
    let folders = Arc::new((history.worktrees.clone(), history.submodules.clone()));
    session.slot(&window, |slot| {
        slot.history = cache;
        slot.changes = changes;
        slot.folders = folders;
    });
    Ok(history)
}

/// What the other worktrees and the submodules' folders the last history listed have, each sent
/// on `on_state` as soon as it is read. A folder that shows no change since it was last read is
/// answered from then, unless that was before `since` (ms since 1970): the window came back to
/// the front, and its files may have changed.
#[tauri::command]
async fn folder_states(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    since: Option<u64>,
    lane: Option<String>,
    on_state: tauri::ipc::Channel<FolderState>,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    let (folders, cache) = session.slot(&window, |slot| (slot.folders.clone(), slot.folder_cache.clone()));
    let since = since.map(|ms| std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms));
    read(at(&repo), "Read the worktrees and submodules", move || {
        repo.folder_states(
            &folders.0,
            &folders.1,
            &cache,
            since,
            oxbow_core::folders::AT_ONCE,
            &cancel,
            &mut |state| {
                // The window may be closing; what it no longer shows needn't arrive.
                let _ = on_state.send(state);
            },
        )
    })
    .await
}

#[tauri::command]
async fn commit_detail(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    id: String,
    quick: Option<bool>,
    count_from: Option<usize>,
    lane: Option<String>,
) -> CommandResult<CommitDetail> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    // Files before `count_from` are counted by the diffs shown of them.
    let count = (quick != Some(true)).then_some(count_from.unwrap_or(0));
    // A commit where a shallow clone stops shows no parent until a deeper fetch.
    let key = format!(
        "detail\0{}\0{id}\0{count:?}\0{:?}\0{}",
        repo.workdir().display(),
        repo.diff_options(),
        repo.shallow_stamp()
    );
    if let Some(Cached::Detail(detail)) = session.commits.lock().expect("session lock").get(&key) {
        return Ok((*detail).clone());
    }
    let detail = read(at(&repo), "Read a commit", move || {
        repo.commit_detail_with(&id, count, &cancel)
    })
    .await?;
    // What a submodule says depends on its clone here, which changes.
    if !detail.files.iter().any(|f| f.submodule.is_some()) {
        let cached = Cached::Detail(Arc::new(detail.clone()));
        session.commits.lock().expect("session lock").put(key, cached);
    }
    Ok(detail)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn commit_diff(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    id: String,
    path: Option<String>,
    whole_file: bool,
    full: Option<bool>,
    lane: Option<String>,
) -> CommandResult<Vec<FileDiff>> {
    let repo = current_full(&session, &window, full)?;
    let cancel = session.lane(&window, lane);
    let context = if whole_file {
        DiffContext::WholeFile
    } else {
        DiffContext::Compact
    };
    let key = format!(
        "diff\0{}\0{id}\0{path:?}\0{whole_file}\0{:?}\0{}",
        repo.workdir().display(),
        repo.diff_options(),
        repo.shallow_stamp()
    );
    if let Some(Cached::Diffs(diffs)) = session.commits.lock().expect("session lock").get(&key) {
        return Ok((*diffs).clone());
    }
    let diffs = read(at(&repo), "Read a commit's changes", move || {
        repo.commit_diff_with(&id, path.as_deref(), context, &cancel)
    })
    .await?;
    if !diffs.iter().any(|d| d.submodule.is_some()) {
        let cached = Cached::Diffs(Arc::new(diffs.clone()));
        session.commits.lock().expect("session lock").put(key, cached);
    }
    Ok(diffs)
}

/// The diffs of a commit's files from the `from`th on, as many as fit `budget` lines.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn commit_diff_page(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    id: String,
    from: usize,
    budget: usize,
    whole_file: bool,
    lane: Option<String>,
) -> CommandResult<DiffPage> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    let context = if whole_file {
        DiffContext::WholeFile
    } else {
        DiffContext::Compact
    };
    let key = format!(
        "page\0{}\0{id}\0{from}\0{budget}\0{whole_file}\0{:?}\0{}",
        repo.workdir().display(),
        repo.diff_options(),
        repo.shallow_stamp()
    );
    if let Some(Cached::Page(page)) = session.commits.lock().expect("session lock").get(&key) {
        return Ok((*page).clone());
    }
    let page = read(at(&repo), "Read a commit's changes", move || {
        repo.commit_diff_page(&id, from, budget, context, &cancel)
    })
    .await?;
    if !page.diffs.iter().any(|d| d.submodule.is_some()) {
        let cached = Cached::Page(Arc::new(page.clone()));
        session.commits.lock().expect("session lock").put(key, cached);
    }
    Ok(page)
}

#[tauri::command]
async fn compare(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    base: String,
    target: String,
    mode: oxbow_core::CompareMode,
) -> CommandResult<oxbow_core::Comparison> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Compare", move || repo.compare(&base, &target, mode)).await
}

/// The changed files of a comparison with their line counts, from the `count_from`th on: the
/// ones before are counted by the first page of diffs.
#[tauri::command]
async fn compare_counts(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    from: String,
    to: String,
    count_from: usize,
    lane: Option<String>,
) -> CommandResult<Vec<oxbow_core::FileChange>> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Compare", move || {
        repo.tree_files_with(&from, &to, Some(count_from), &cancel)
    })
    .await
}

/// A page of a comparison's diffs: from the `start`th file on, until they have `budget` lines.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn compare_diff_page(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    from: String,
    to: String,
    start: usize,
    budget: usize,
    whole_file: bool,
    lane: Option<String>,
) -> CommandResult<DiffPage> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    let context = if whole_file {
        DiffContext::WholeFile
    } else {
        DiffContext::Compact
    };
    read(at(&repo), "Compare", move || {
        repo.tree_diff_page(&from, &to, start, budget, context, &cancel)
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn compare_diff(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    from: String,
    to: String,
    path: Option<String>,
    whole_file: bool,
    full: Option<bool>,
    lane: Option<String>,
) -> CommandResult<Vec<FileDiff>> {
    let repo = current_full(&session, &window, full)?;
    let cancel = session.lane(&window, lane);
    let context = if whole_file {
        DiffContext::WholeFile
    } else {
        DiffContext::Compact
    };
    read(at(&repo), "Compare", move || {
        repo.tree_diff_with(&from, &to, path.as_deref(), context, &cancel)
    })
    .await
}

#[tauri::command]
async fn file_history(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    rev: Option<String>,
    lane: Option<String>,
) -> CommandResult<oxbow_core::FileHistory> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Read a file's history", move || {
        repo.file_history_with(&path, rev.as_deref(), &cancel)
    })
    .await
}

#[tauri::command]
async fn blame(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    rev: String,
    lane: Option<String>,
) -> CommandResult<oxbow_core::Blame> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Blame", move || repo.blame_with(&path, &rev, &cancel)).await
}

#[tauri::command]
async fn line_history(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    line: u32,
    rev: String,
    lane: Option<String>,
) -> CommandResult<Vec<String>> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Read a line's history", move || {
        repo.line_history_with(&path, line, &rev, &cancel)
    })
    .await
}

#[tauri::command]
async fn files(window: tauri::WebviewWindow, session: State<'_, Session>) -> CommandResult<Vec<String>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "List the files", move || repo.files()).await
}

#[tauri::command]
async fn file_pickaxe(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    text: String,
    match_case: bool,
    rev: String,
    lane: Option<String>,
) -> CommandResult<Vec<String>> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Search a file's history", move || {
        repo.file_pickaxe_with(&path, &text, match_case, &rev, &cancel)
    })
    .await
}

/// A series of commits before and after a rewrite, paired by `git range-diff`.
#[tauri::command]
async fn series(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    old: String,
    new: String,
    rewritten: Vec<String>,
) -> CommandResult<oxbow_core::Series> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Compare the series", move || {
        repo.series(&old, &new, &rewritten)
    })
    .await
}

/// The latest rewrite of a branch as its remote branch knows it.
#[tauri::command]
async fn last_rewrite(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    branch: String,
) -> CommandResult<Option<oxbow_core::Rewrite>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Find the latest rewrite", move || repo.last_rewrite(&branch)).await
}

/// The Operation Log, newest step first.
#[tauri::command]
async fn operation_log(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<oxbow_core::OpEntry>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the Undo history", move || repo.operation_log()).await
}

/// Lost work: the commits git's reflog of `HEAD` and the local branches points at, newest first.
#[tauri::command]
async fn reflog_entries(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<oxbow_core::ReflogEntry>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the reflog", move || repo.reflog_entries()).await
}

/// What Fix a Mistake plans from: the state to check again before a fix runs, the changed files
/// and commits only the reflog keeps.
#[tauri::command]
async fn fix_state(window: tauri::WebviewWindow, session: State<'_, Session>) -> CommandResult<oxbow_core::FixState> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Look for mistakes to fix", move || repo.fix_state()).await
}

/// The repository now beside how it was before step `id`, or right after it with `after`.
#[tauri::command]
async fn compare_snapshot(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    id: String,
    after: bool,
) -> CommandResult<oxbow_core::SnapshotComparison> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Compare with a step of Undo", move || {
        repo.compare_snapshot(&id, after)
    })
    .await
}

/// Bytes of one side of a picture read at most to preview it.
const MAX_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;

/// The bytes of one side of a file diff, to show an image.
#[tauri::command]
async fn source_bytes(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    source: Source,
) -> CommandResult<tauri::ipc::Response> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read a file", move || {
        repo.read_source_within(&source, MAX_PREVIEW_BYTES)
    })
    .await
    .map(tauri::ipc::Response::new)
}

/// Lines of one side of a file diff and how many it has, for the unchanged lines between hunks.
#[tauri::command]
async fn source_lines(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    source: Source,
    start: u32,
    count: u32,
) -> CommandResult<oxbow_core::SourceLines> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read unchanged lines", move || {
        repo.read_lines(&source, start, count)
    })
    .await
}

/// Open one side of a file diff in the app the system picks for it: the working-tree file as it
/// is, any other version from a copy in a temporary folder named after it.
#[tauri::command]
async fn open_source(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    source: Source,
    path: String,
    label: String,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    let file = match &source {
        Source::Worktree { path } => repo.workdir().join(Path::new(path)),
        Source::Blob { id } => {
            let data = read(at(&repo), "Read a file", {
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
async fn working_tree(window: tauri::WebviewWindow, session: State<'_, Session>) -> CommandResult<WorkingTree> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the changes", move || repo.working_tree()).await
}

#[tauri::command]
async fn working_diff(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    side: Side,
    whole_file: bool,
    full: Option<bool>,
) -> CommandResult<FileDiff> {
    let repo = current_full(&session, &window, full)?;
    read(at(&repo), "Read a change", move || {
        repo.working_diff(&path, side, whole_file)
    })
    .await
}

/// How the submodule at `path` moved from `old` to `new`, for its card in the sidebar's panel.
#[tauri::command]
async fn submodule_move(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    old: Option<String>,
    new: Option<String>,
    changed: Option<usize>,
) -> CommandResult<oxbow_core::SubmoduleMove> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read a submodule", move || {
        Ok(repo.submodule_move(&path, old.as_deref(), new.as_deref(), changed))
    })
    .await
}

/// The submodules alone, for Retry after History couldn't read them.
#[tauri::command]
async fn submodules(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<oxbow_core::Submodule>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the submodules", move || repo.read_submodules()).await
}

/// The worktrees alone, with their counts of changed files, for Retry after History couldn't
/// read them.
#[tauri::command]
async fn worktrees(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<oxbow_core::Worktree>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the worktrees", move || repo.worktrees()).await
}

/// Submodules whose recorded commit is on none of their remotes, for the warning before a push.
#[tauri::command]
async fn unpushed_submodules(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<oxbow_core::submodules::UnpushedSubmodule>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Check submodules for unpushed commits", move || {
        Ok(repo.unpushed_submodules())
    })
    .await
}

/// The diffs of several files on one side of the working copy, for the Summary tab.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn working_diff_page(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    paths: Vec<String>,
    side: Side,
    whole_file: bool,
    snapshot: Option<u64>,
    from: usize,
    budget: usize,
    lane: Option<String>,
) -> CommandResult<DiffPage> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    // The files as the history just read them, when the panel shows that reading.
    let tree = session.slot(&window, |slot| {
        slot.changes
            .as_ref()
            .filter(|(number, _)| Some(*number) == snapshot)
            .map(|(_, tree)| tree.clone())
    });
    read(at(&repo), "Read a change", move || {
        let tree = match tree {
            Some(tree) => tree,
            None => Arc::new(repo.working_tree()?),
        };
        repo.working_diff_page(&tree, &paths, side, whole_file, from, budget, &cancel)
    })
    .await
}

/// What deleting a local branch would lose, for the confirmation sheet.
#[tauri::command]
async fn deletion_check(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    branch: String,
) -> CommandResult<DeletionCheck> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Check a branch before deleting", move || {
        repo.deletion_check(&branch)
    })
    .await
}

/// Commits that deleting a remote branch (`origin/x`) on its remote would lose.
#[tauri::command]
async fn remote_deletion_check(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    branch: String,
) -> CommandResult<Vec<CommitBrief>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Check a branch before deleting", move || {
        repo.remote_deletion_check(&branch)
    })
    .await
}

/// What merging `branch` into the checked-out branch would bring, for the merge sheet.
#[tauri::command]
async fn merge_preview(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    branch: String,
) -> CommandResult<MergePreview> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Preview a merge", move || repo.merge_preview(&branch)).await
}

/// Both sides of a conflicted file, for the Conflicts screen.
#[tauri::command]
async fn conflict_file(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
) -> CommandResult<ConflictFile> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read a conflict", move || repo.conflict_file(&path)).await
}

/// The merge tool git is set to open, for the Conflicts screen's button.
#[tauri::command]
async fn merge_tool(window: tauri::WebviewWindow, session: State<'_, Session>) -> CommandResult<Option<MergeTool>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the merge tool", move || Ok(repo.merge_tool())).await
}

/// The draft file of the window's working folder, in the app's data folder.
fn draft_file(app: &AppHandle, repo: &Repo) -> CommandResult<PathBuf> {
    let dir = app.path().app_data_dir().map_err(|err| err.to_string())?;
    Ok(drafts::file(&dir.join("conflict-drafts"), repo.workdir()))
}

/// The Conflicts screen's draft saved for the window's working folder, if there is one.
#[tauri::command]
async fn conflict_draft(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Option<Value>> {
    let repo = current(&session, &window)?;
    let file = draft_file(&app, &repo)?;
    tauri::async_runtime::spawn_blocking(move || drafts::read(&file, repo.workdir()))
        .await
        .map_err(|err| err.to_string())?
}

/// Save the Conflicts screen's draft for the window's working folder, while its operation is
/// under way.
#[tauri::command]
async fn save_conflict_draft(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    draft: Value,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    let file = draft_file(&app, &repo)?;
    tauri::async_runtime::spawn_blocking(move || {
        // A save arriving after the operation was finished or aborted would bring its draft back.
        if repo.operation().is_ok_and(|op| op.is_none()) {
            return drafts::remove(&file);
        }
        drafts::write(&file, repo.workdir(), draft)
    })
    .await
    .map_err(|err| err.to_string())?
}

/// Remove the draft of the window's working folder, once its operation is over.
#[tauri::command]
async fn drop_conflict_draft(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    drafts::remove(&draft_file(&app, &repo)?)
}

/// Open a conflicted file in the merge tool and wait until it quits: true when git staged it.
#[tauri::command]
async fn open_merge_tool(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
) -> CommandResult<bool> {
    let repo = current(&session, &window)?;
    let title = format!("Open {path} in the merge tool");
    logged(Some(at(&repo)), console::EntryKind::Action, title, move || {
        repo.open_merge_tool(&path)
    })
    .await
}

/// Whether `stash@{index}` applies cleanly to the checked-out commit.
#[tauri::command]
async fn stash_check(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    index: usize,
    id: String,
) -> CommandResult<StashCheck> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Check a stash", move || repo.stash_check(index, &id)).await
}

/// The stack `branch` belongs to (the checked-out branch without one), for Edit Stack.
#[tauri::command]
async fn stack(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    branch: Option<String>,
) -> CommandResult<oxbow_core::Stack> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the stack", move || repo.stack(branch.as_deref())).await
}

/// Which of `candidates` has the merged pull request's commit `merged`, for a restack to move
/// onto; none until a fetch brought it.
#[tauri::command]
async fn landed_on(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    merged: String,
    candidates: Vec<String>,
) -> CommandResult<Option<oxbow_core::Landing>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the stack", move || {
        Ok(repo.landed_on(&merged, &candidates))
    })
    .await
}

/// A dry run of an Edit Stack plan: what it changes, where it would stop, what to push.
#[tauri::command]
async fn stack_preview(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    plan: oxbow_core::StackPlan,
) -> CommandResult<oxbow_core::StackPreview> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Preview the stack", move || repo.stack_preview(&plan)).await
}

/// The git commands an action will run, for the confirmation sheet.
#[tauri::command]
async fn plan_action(window: tauri::WebviewWindow, session: State<'_, Session>, action: Action) -> CommandResult<Plan> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Prepare an action", move || {
        let mut plan = repo.plan(&action)?;
        plan.not_kept = repo.not_kept(&action)?;
        Ok(plan)
    })
    .await
}

/// Run an action, sending each command and output line to the window as an `action-event`. A
/// failure comes back explained, so the sheet can offer the way out. With `without_undo` it
/// runs even when the state before it can't be saved for Undo, once that was chosen.
#[tauri::command]
async fn perform_action(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    action: Action,
    without_undo: Option<bool>,
) -> Result<String, Failure> {
    // Only the window that asked shows the output: Settings runs actions of its own.
    let label = window.label().to_owned();
    let from_settings = label == "settings";
    let repo_window = session.label(&window);
    let repo = current(&session, &window).map_err(|message| Failure {
        kind: oxbow_core::FailureKind::Other,
        output: message,
        incoming: Vec::new(),
        remote_tip: None,
        hook: None,
        ssh: None,
        secrets: None,
        tracking: None,
    })?;
    let cancel = session.cancel(&window);
    cancel.store(false, Ordering::Relaxed);
    let events = app.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let title = repo.action_title(&action);
        console::scope(Some(repo.workdir()), console::EntryKind::Action, title, || {
            let mut emit = |event| {
                let _ = events.emit_to(label.as_str(), "action-event", event);
            };
            let result = if without_undo.unwrap_or(false) {
                repo.perform_without_undo(&action, &mut emit, &cancel)
            } else {
                repo.perform_with(&action, &mut emit, &cancel)
            };
            result.map_err(|err| repo.explain_failure(&action, &err))
        })
    })
    .await;
    // The repository window shows the remotes and branches Settings just changed.
    if from_settings {
        let _ = app.emit_to(repo_window.as_str(), "repo-touched", ());
    }
    joined.unwrap_or_else(|err| {
        Err(Failure {
            kind: oxbow_core::FailureKind::Other,
            output: err.to_string(),
            incoming: Vec::new(),
            remote_tip: None,
            hook: None,
            ssh: None,
            secrets: None,
            tracking: None,
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
    logged(None, console::EntryKind::Read, "Read the Git setup", || {
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
    production: Production,
}

/// Settings › This Repository › Production branch.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Production {
    /// The chosen branch as a full ref; `None` is Auto.
    chosen: Option<String>,
    /// Whether the chosen branch exists.
    found: bool,
    /// Full ref of the branch Auto picks, if any.
    auto: Option<String>,
    /// Local and remote branches to choose from, as full refs.
    branches: Vec<String>,
}

/// Settings › This Repository, or nothing when no repository is open.
#[tauri::command]
async fn repo_settings(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Option<RepoSettings>> {
    let Ok(repo) = current(&session, &window) else {
        return Ok(None);
    };
    read(at(&repo), "Read repository settings", move || {
        Ok(Some(RepoSettings {
            name: repo.name(),
            path: repo.workdir().display().to_string(),
            local: repo.local_config(),
            remotes: repo.remotes_info()?,
            storage: repo.storage().ok(),
            production: {
                let refs = repo.refs()?;
                let chosen = repo.production_setting();
                Production {
                    found: chosen
                        .as_deref()
                        .is_some_and(|c| oxbow_core::history::pick_trunk(&refs, Some(c)).is_some()),
                    auto: oxbow_core::history::pick_trunk(&refs, None).and_then(oxbow_core::history::full_ref),
                    branches: refs
                        .iter()
                        .filter_map(oxbow_core::history::full_ref)
                        .filter(|r| !r.ends_with("/HEAD"))
                        .collect(),
                    chosen,
                }
            },
        }))
    })
    .await
}

/// Change a value in ~/.gitconfig or the open repository's .git/config; `None` removes it.
#[tauri::command]
async fn set_git_config(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    scope: ConfigScope,
    key: String,
    value: Option<String>,
) -> CommandResult<()> {
    let repo = match scope {
        ConfigScope::Local => Some(current(&session, &window)?),
        ConfigScope::Global => None,
    };
    // The production branch changes how the main window draws the graph.
    let redraw = key.eq_ignore_ascii_case(oxbow_core::history::PRODUCTION_KEY);
    let dir = repo.as_ref().map(at);
    let title = match value {
        Some(_) => format!("Set {key}"),
        None => format!("Unset {key}"),
    };
    logged(dir, console::EntryKind::Action, title, move || match repo {
        Some(repo) => repo.set_local_config(&key, value.as_deref()),
        None => config::set_global_config(&key, value.as_deref()),
    })
    .await?;
    if redraw {
        let _ = app.emit_to(session.label(&window).as_str(), "repo-touched", ());
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PullSetup {
    mode: oxbow_core::PullMode,
    autostash: bool,
}

/// How Pull will go here, for the words of its confirmation.
#[tauri::command]
async fn pull_setup(window: tauri::WebviewWindow, session: State<'_, Session>) -> CommandResult<PullSetup> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read pull settings", move || {
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

/// What a background fetch printed, with the number Activity gave the fetch: a fetch of the
/// repository open before may still be printing while the next one runs.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FetchEvent {
    fetch: Option<u64>,
    event: oxbow_core::ActionEvent,
}

/// Fetch one remote or all in the background: no sheet, git's progress goes to the window
/// as `fetch-event`s, and a failure comes back explained, for Activity. `fetch` is Activity's
/// number for it; without one (another view's quiet fetch) Stop doesn't reach it.
#[tauri::command]
async fn fetch_in_background(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    remote: Option<String>,
    fetch: Option<u64>,
    auto: Option<bool>,
) -> Result<Fetched, Failure> {
    let failed = |output: String| Failure {
        kind: oxbow_core::FailureKind::Other,
        output,
        incoming: Vec::new(),
        remote_tip: None,
        hook: None,
        ssh: None,
        secrets: None,
        tracking: None,
    };
    let repo = current(&session, &window).map_err(failed)?;
    let cancel = if fetch.is_some() {
        session.new_fetch_cancel(&window)
    } else {
        Arc::new(AtomicBool::new(false))
    };
    let label = window.label().to_owned();
    tauri::async_runtime::spawn_blocking(move || {
        let action = Action::Fetch { remote: remote.clone() };
        let (kind, title) = if auto == Some(true) {
            (console::EntryKind::Background, "Auto fetch".to_owned())
        } else {
            (
                console::EntryKind::Action,
                format!("Fetch {}", remote.as_deref().unwrap_or("all remotes")),
            )
        };
        console::scope(Some(repo.workdir()), kind, title, || {
            let command = repo.fetch_command(remote.as_deref()).with_progress();
            let emit = |event| {
                let _ = app.emit_to(label.as_str(), "fetch-event", FetchEvent { fetch, event });
            };
            emit(oxbow_core::ActionEvent::Command {
                display: command.display(),
            });
            let mut updated = 0;
            let result = console::main(|| {
                repo.run_streaming(
                    &command,
                    &mut |line| {
                        if !line.progress && oxbow_core::remote::is_ref_update(&line.text) {
                            updated += 1;
                        }
                        emit(oxbow_core::ActionEvent::Line(line));
                    },
                    &cancel,
                )
            });
            match result {
                Ok(_) => Ok(Fetched { updated }),
                Err(err) => Err(repo.explain_failure(&action, &err)),
            }
        })
    })
    .await
    .unwrap_or_else(|err| Err(failed(err.to_string())))
}

/// Stop the background fetch.
#[tauri::command]
fn stop_fetch(window: tauri::WebviewWindow, session: State<'_, Session>) {
    session.fetch_cancel(&window).store(true, Ordering::Relaxed);
}

/// The public half of an SSH key, to paste into GitHub.
#[tauri::command]
fn public_key(key: String) -> CommandResult<String> {
    oxbow_core::ssh::public_key(&key).map_err(|err| err.to_string())
}

/// Search the history: messages, code changes, authors or file paths.
#[tauri::command]
async fn search(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    query: oxbow_core::SearchQuery,
    lane: Option<String>,
) -> CommandResult<oxbow_core::SearchResult> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Search", move || repo.search_with(&query, &cancel)).await
}

/// Tell which of `ids`, loaded commits the history could not, are unpushed, for the next load
/// to mark: whether it found out anything.
#[tauri::command]
async fn settle_unpushed(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    ids: Vec<String>,
    lane: Option<String>,
) -> CommandResult<bool> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Check unpushed commits", move || {
        repo.settle_unpushed(&ids, &cancel)
    })
    .await
}

/// Stop the reads running in `lanes`: what they would answer is no longer wanted, as the
/// search was cleared or the commit panel closed.
#[tauri::command]
fn stop_reads(window: tauri::WebviewWindow, session: State<'_, Session>, lanes: Vec<String>) {
    session.slot(&window, |slot| {
        for lane in lanes {
            if let Some(cancel) = slot.lanes.remove(&lane) {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    });
}

#[tauri::command]
async fn resolve_commit(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    prefix: String,
) -> CommandResult<Option<String>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Find a commit", move || Ok(repo.resolve_commit(&prefix))).await
}

/// Which files the repository keeps in Git LFS, and whether git-lfs is installed.
#[tauri::command]
async fn lfs_status(window: tauri::WebviewWindow, session: State<'_, Session>) -> CommandResult<oxbow_core::LfsStatus> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read Git LFS", move || repo.lfs_status()).await
}

/// What Free Up Space would delete, e.g. `3 files would be pruned (1.4 GB)`.
#[tauri::command]
async fn lfs_prune_preview(window: tauri::WebviewWindow, session: State<'_, Session>) -> CommandResult<Option<String>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Preview Git LFS cleanup", move || repo.lfs_prune_preview()).await
}

/// The Git LFS download page, for installing it without Homebrew.
#[tauri::command]
fn open_lfs_download() -> CommandResult<()> {
    open_in::open_url("https://git-lfs.com")
}

/// Preview a file of the working copy: Quick Look on macOS, the default app elsewhere.
#[tauri::command]
fn quick_look(window: tauri::WebviewWindow, session: State<'_, Session>, path: String) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    open_in::quick_look(&repo.workdir().join(Path::new(&path)))
}

/// Ask the default remote which tags it has, so tags it lacks show as local. True when that
/// changed. Without a remote there is nothing to ask.
#[tauri::command]
async fn refresh_remote_tags(window: tauri::WebviewWindow, session: State<'_, Session>) -> CommandResult<bool> {
    let repo = current(&session, &window)?;
    logged(
        Some(at(&repo)),
        console::EntryKind::Background,
        "Check the remote's tags",
        move || match repo.default_remote() {
            Some(remote) => repo.refresh_remote_tags(&remote),
            None => Ok(false),
        },
    )
    .await
}

/// Open a file of the repository in the editor chosen in Settings.
#[tauri::command]
fn open_in_editor(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    line: Option<u32>,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
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
fn reveal_file(window: tauri::WebviewWindow, session: State<'_, Session>, path: String) -> CommandResult<()> {
    let repo = current(&session, &window)?;
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

/// Open the repository folder in the terminal chosen in Settings; with `worktree`, that other
/// working folder of it.
#[tauri::command]
fn open_in_terminal(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    worktree: Option<String>,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    let dir = match worktree {
        Some(path) => worktree_folder(&repo, &path)?,
        None => repo.workdir().to_path_buf(),
    };
    let terminal = read_settings(&app)
        .get("oxbow.openIn.terminal")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| open_in::terminals().first().map(|app| app.id.to_owned()))
        .ok_or("No terminal found.")?;
    open_in::open_terminal(&terminal, &dir)
}

/// `path` when it is one of the repository's worktrees or cloned submodules; commands that open
/// things take no other.
fn worktree_folder(repo: &Repo, path: &str) -> CommandResult<PathBuf> {
    let known = repo.worktree_list().map_err(|err| err.to_string())?;
    // A submodule's path is relative to the repository's folder.
    let full = repo.workdir().join(path);
    let wanted = full.canonicalize().ok();
    let same = |folder: &Path| folder == full || wanted.is_some() && folder.canonicalize().ok() == wanted;
    if let Some(tree) = known.iter().filter(|t| !t.missing).find(|t| same(Path::new(&t.path))) {
        return Ok(PathBuf::from(&tree.path));
    }
    // A submodule's folder is a repository of its own, opened and shown like a worktree.
    repo.submodule_list()
        .unwrap_or_default()
        .iter()
        .filter(|s| s.cloned)
        .map(|s| repo.workdir().join(&s.path))
        .find(|folder| same(folder))
        .ok_or_else(|| format!("{path} isn’t a worktree or submodule of this repository any more"))
}

/// Show a worktree's folder in Finder (Explorer, or the file manager on Linux).
#[tauri::command]
fn reveal_worktree(window: tauri::WebviewWindow, session: State<'_, Session>, path: String) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    open_in::reveal(&worktree_folder(&repo, &path)?)
}

/// Open the worktree at `path` in a window of its own, or bring forward the window that has it.
/// Async, because making a window in a synchronous command deadlocks on Windows.
#[tauri::command]
async fn open_window(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    let folder = worktree_folder(&repo, &path)?;
    if let Some(open) = session
        .window_of(&folder)
        .and_then(|label| app.get_webview_window(&label))
    {
        let _ = open.unminimize();
        return open.set_focus().map_err(|err| err.to_string());
    }
    let label = format!("repo-{}", session.next.fetch_add(1, Ordering::Relaxed) + 1);
    session
        .pending
        .lock()
        .expect("session lock")
        .insert(label.clone(), folder.display().to_string());
    // Named after the folder, so the Window menu and Mission Control tell the windows apart.
    let title = folder
        .file_name()
        .map_or_else(|| "Oxbow".to_owned(), |name| name.to_string_lossy().into_owned());
    let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(1360.0, 860.0)
        .min_inner_size(900.0, 560.0)
        .theme(appearance(&read_settings(&app)));
    // A little down and to the right of the window it was opened from.
    if let (Ok(at), Ok(scale)) = (window.outer_position(), window.scale_factor()) {
        let at = at.to_logical::<f64>(scale);
        builder = builder.position(at.x + 32.0, at.y + 32.0);
    }
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(22.0, 30.0));
    builder.build().map(|_| ()).map_err(|err| err.to_string())
}

/// Close the window of the worktree that was at `path`, after it was removed.
#[tauri::command]
fn close_window_of(app: AppHandle, window: tauri::WebviewWindow, session: State<'_, Session>, path: String) {
    if let Some(open) = session
        .window_of(Path::new(&path))
        .filter(|label| label != window.label())
        .and_then(|label| app.get_webview_window(&label))
    {
        let _ = open.close();
    }
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
    let mut revision = session.settings.lock().expect("session lock");
    let before = read_settings(&app);
    let file = settings_file(&app)?;
    settings::write_text(&file, &if text.ends_with('\n') { text } else { text + "\n" })?;
    settings_replaced(&app, &session, &mut revision, &before, &settings);
    Ok(())
}

/// Follow settings that replaced `before` all at once, and tell every window which keys changed.
/// `revision` is the held number of the last change.
fn settings_replaced(
    app: &AppHandle,
    session: &Session,
    revision: &mut u64,
    before: &serde_json::Map<String, Value>,
    after: &serde_json::Map<String, Value>,
) {
    *revision += 1;
    apply_settings(app, session, after);
    let removed = before
        .keys()
        .filter(|key| !after.contains_key(*key))
        .map(|key| (key.clone(), Value::Null));
    for (key, value) in after.clone().into_iter().chain(removed) {
        if before.get(&key) != Some(&value) {
            let _ = app.emit(
                "settings-changed",
                SettingChanged {
                    key,
                    value,
                    revision: *revision,
                },
            );
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
    let mut revision = session.settings.lock().expect("session lock");
    let before = read_settings(&app);
    let file = settings_file(&app)?;
    if file.exists() {
        move_file(&file, &settings_backup(&file))?;
    }
    settings_replaced(&app, &session, &mut revision, &before, &serde_json::Map::new());
    Ok(())
}

/// Bring back the settings that Restore Defaults moved aside.
#[tauri::command]
fn restore_settings_backup(app: AppHandle, session: State<'_, Session>) -> CommandResult<()> {
    let file = settings_file(&app)?;
    let backup = settings_backup(&file);
    let text = std::fs::read_to_string(&backup).map_err(|_| "There is no copy of the old settings.".to_owned())?;
    let after: serde_json::Map<String, Value> = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    let mut revision = session.settings.lock().expect("session lock");
    let before = read_settings(&app);
    move_file(&backup, &file)?;
    settings_replaced(&app, &session, &mut revision, &before, &after);
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
fn stop_action(window: tauri::WebviewWindow, session: State<'_, Session>) {
    session.cancel(&window).store(true, Ordering::Relaxed);
}

/// The default macOS menu, with Settings… (⌘,) after About in the app menu. About shows the
/// version, the commit the build is of and when it was built (build.rs).
#[cfg(target_os = "macos")]
fn app_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let menu = tauri::menu::Menu::default(app)?;
    if let Some(app_menu) = menu.items()?.first().and_then(|item| item.as_submenu().cloned()) {
        let about = tauri::menu::AboutMetadata {
            name: Some(app.package_info().name.clone()),
            // "Version 0.1.113 (b5cc367)": macOS puts the short version in the parentheses.
            version: Some(env!("OXBOW_VERSION").to_owned()),
            short_version: Some(env!("OXBOW_COMMIT").to_owned()),
            copyright: Some(format!("Built {}", env!("OXBOW_BUILT"))),
            // No credits: macOS draws them larger than the version. Third-party notices ship in
            // the bundle's Resources (THIRD-PARTY-NOTICES.md).
            ..Default::default()
        };
        app_menu.remove_at(0)?;
        app_menu.insert(&tauri::menu::PredefinedMenuItem::about(app, None, Some(about))?, 0)?;
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
        .on_window_event(|window, event| {
            let session = window.state::<Session>();
            let label = window.label().to_owned();
            match event {
                // Settings shows and changes the repository of the window used last.
                tauri::WindowEvent::Focused(true) if label != "settings" && label != "console" => {
                    let changed = session.focused.lock().expect("session lock").replace(label.clone()) != Some(label);
                    if changed {
                        let _ = window.emit_to("settings", "repo-changed", ());
                        let _ = window.emit_to("console", "repo-changed", ());
                    }
                }
                tauri::WindowEvent::Destroyed => {
                    // Its reads stop with it.
                    if let Some(mut slot) = session.windows.lock().expect("session lock").remove(&label) {
                        slot.stop_reads();
                    }
                    session.pending.lock().expect("session lock").remove(&label);
                    let mut focused = session.focused.lock().expect("session lock");
                    if focused.as_deref() == Some(label.as_str()) {
                        *focused = None;
                    }
                }
                _ => {}
            }
        })
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
            console_window::follow(handle);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            display_options,
            open_repo,
            initial_repo,
            recent_repos,
            forget_repo,
            clear_recent,
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
            commit_diff_page,
            stop_reads,
            folder_states,
            settle_unpushed,
            compare,
            compare_diff,
            compare_diff_page,
            compare_counts,
            file_history,
            blame,
            line_history,
            file_pickaxe,
            files,
            working_tree,
            working_diff,
            working_diff_page,
            source_bytes,
            source_lines,
            open_source,
            deletion_check,
            remote_deletion_check,
            merge_preview,
            conflict_file,
            merge_tool,
            open_merge_tool,
            conflict_draft,
            save_conflict_draft,
            drop_conflict_draft,
            stash_check,
            stack,
            landed_on,
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
            reveal_worktree,
            open_window,
            submodule_move,
            submodules,
            worktrees,
            unpushed_submodules,
            close_window_of,
            reveal_file,
            quick_look,
            lfs_status,
            lfs_prune_preview,
            open_lfs_download,
            operation_log,
            series,
            last_rewrite,
            reflog_entries,
            fix_state,
            compare_snapshot,
            monospace_fonts,
            refresh_remote_tags,
            search,
            resolve_commit,
            settings_text,
            save_settings_text,
            settings_location,
            restore_default_settings,
            restore_settings_backup,
            reveal_settings,
            console_window::open_console,
            console_window::console_repos,
            console_window::console_entries,
            console_window::console_clear,
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
