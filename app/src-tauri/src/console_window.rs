//! The Console window: every process Oxbow ran, with its output, per repository. The log itself
//! is kept by [`oxbow_core::console`]; this opens the window and answers what it asks.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use oxbow_core::console::{self, Entry};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::{CommandResult, Session};

/// A repository whose log the Console can show.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleRepo {
    name: String,
    /// Its working tree; none for what ran outside any repository.
    path: Option<String>,
    /// Entries in its log.
    count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleRepos {
    /// The repository of the window used last: This Repository.
    current: Option<String>,
    repos: Vec<ConsoleRepo>,
}

/// A new or changed entry, for the Console window to follow.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct Followed<'a> {
    repo: Option<String>,
    entry: &'a Entry,
}

/// Send each entry the log takes to the Console window while it is open.
pub fn follow(app: &AppHandle) {
    if let Ok(dir) = app.path().app_config_dir() {
        console::set_outside_dir(dir);
    }
    let handle = app.clone();
    console::set_listener(move |repo, entry| {
        if handle.get_webview_window("console").is_some() {
            let _ = handle.emit_to(
                "console",
                "console-entry",
                Followed {
                    repo: repo.map(|p| p.display().to_string()),
                    entry,
                },
            );
        }
    });
}

/// Open the Console window, or bring it to the front. Async, because making a window in a
/// synchronous command deadlocks on Windows.
#[tauri::command]
pub async fn open_console(app: AppHandle) -> CommandResult<()> {
    show_console(&app)
}

pub fn show_console(app: &AppHandle) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window("console") {
        let _ = window.unminimize();
        return window.set_focus().map_err(|err| err.to_string());
    }
    let builder = WebviewWindowBuilder::new(app, "console", WebviewUrl::App("index.html#console".into()))
        .title("Console")
        .inner_size(1000.0, 760.0)
        .min_inner_size(640.0, 400.0)
        .theme(crate::appearance(&crate::read_settings(app)));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(22.0, 30.0));
    builder.build().map(|_| ()).map_err(|err| err.to_string())
}

/// The repositories with a log: the open ones and Recent Repositories, and what ran outside any.
#[tauri::command]
pub async fn console_repos(app: AppHandle, session: State<'_, Session>) -> CommandResult<ConsoleRepos> {
    let (current, open) = {
        let windows = session.windows.lock().expect("session lock");
        let focused = session.focused.lock().expect("session lock").clone();
        let path_of = |label: &str| {
            windows
                .get(label)
                .and_then(|slot| slot.repo.as_ref())
                .map(|repo| repo.workdir().display().to_string())
        };
        let current = focused.as_deref().and_then(path_of).or_else(|| path_of("main"));
        let open: Vec<String> = windows
            .values()
            .filter_map(|slot| slot.repo.as_ref())
            .map(|repo| repo.workdir().display().to_string())
            .collect();
        (current, open)
    };
    let recent: Vec<String> = crate::recent_repos(app).into_iter().map(|r| r.path).collect();
    tauri::async_runtime::spawn_blocking(move || {
        let mut seen = HashSet::new();
        let mut repos = Vec::new();
        for path in current.iter().chain(&open).chain(&recent) {
            // Worktrees of one repository share its log.
            let Some(file) = console::log_file(Path::new(path)) else {
                continue;
            };
            if !seen.insert(file) {
                continue;
            }
            let count = console::entries(Some(Path::new(path))).len();
            if count == 0 && Some(path) != current.as_ref() {
                continue;
            }
            repos.push(ConsoleRepo {
                name: Path::new(path)
                    .file_name()
                    .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned()),
                path: Some(path.clone()),
                count,
            });
        }
        let outside = console::entries(None).len();
        if outside > 0 {
            repos.push(ConsoleRepo {
                name: "Outside Repositories".into(),
                path: None,
                count: outside,
            });
        }
        ConsoleRepos { current, repos }
    })
    .await
    .map_err(|err| err.to_string())
}

/// The log of the repository at `path` (none: what ran outside any), oldest first.
#[tauri::command]
pub async fn console_entries(path: Option<String>) -> CommandResult<Vec<Entry>> {
    tauri::async_runtime::spawn_blocking(move || console::entries(path.as_deref().map(Path::new)))
        .await
        .map_err(|err| err.to_string())
}

/// Clear the logs of these repositories (none: of what ran outside any).
#[tauri::command]
pub async fn console_clear(paths: Vec<Option<String>>) -> CommandResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        for path in paths {
            console::clear(path.as_deref().map(PathBuf::from).as_deref()).map_err(|err| err.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|err| err.to_string())?
}
