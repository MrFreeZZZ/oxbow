//! The recent repositories: the Welcome list, the menu and the folders that went away.

use std::path::{Path, PathBuf};

use oxbow_core::Repo;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::repos::last_repo_file;
use crate::session::Session;
use crate::{CommandResult, menu};

/// Repositories opened before, newest first, kept in `recent-repositories.json`.
#[derive(Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecentRepo {
    pub(crate) path: String,
    /// When it was last opened, in seconds since 1970.
    opened: i64,
    /// Filled in when listed: the folder name, and whether the folder is gone.
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) missing: bool,
    /// Why a missing one is gone, when listed.
    #[serde(skip_deserializing, skip_serializing_if = "Option::is_none")]
    gone: Option<oxbow_core::GoneFolder>,
}

/// How many repositories Recent Repositories keeps.
const RECENT_LIMIT: usize = 40;

fn recent_file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("recent-repositories.json"))
}

pub(crate) fn read_recent(app: &AppHandle) -> Vec<RecentRepo> {
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
                    gone: None,
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
    menu::recent_changed(app);
}

pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Put `path` at the top of Recent Repositories.
pub(crate) fn remember_repo(app: &AppHandle, path: &str) {
    let mut list = read_recent(app);
    list.retain(|r| r.path != path);
    list.insert(
        0,
        RecentRepo {
            path: path.to_owned(),
            opened: now(),
            name: String::new(),
            missing: false,
            gone: None,
        },
    );
    list.truncate(RECENT_LIMIT);
    write_recent(app, &list);
}

#[tauri::command]
pub(crate) fn recent_repos(app: AppHandle) -> Vec<RecentRepo> {
    let list = read_recent(&app);
    let known: Vec<PathBuf> = list.iter().map(|r| PathBuf::from(&r.path)).collect();
    list.into_iter()
        .map(|mut r| {
            let path = Path::new(&r.path);
            r.name = path
                .file_name()
                .map_or_else(|| r.path.clone(), |n| n.to_string_lossy().into_owned());
            r.missing = !path.is_dir();
            if r.missing {
                r.gone = oxbow_core::gone_folder(path, &known);
            }
            r
        })
        .collect()
}

/// Folders of the repositories a gone folder may be a worktree of: the window's own, whose
/// records outlive the folder, and the recent ones.
pub(crate) fn known_repos(app: &AppHandle, session: &Session, window: &tauri::WebviewWindow) -> Vec<PathBuf> {
    let open = session.slot(window, |slot| slot.repo.as_ref().map(Repo::common_dir));
    open.and_then(|common| {
        (common.file_name().is_some_and(|name| name == ".git"))
            .then(|| common.parent().map(Path::to_path_buf))
            .flatten()
    })
    .into_iter()
    .chain(read_recent(app).into_iter().map(|r| PathBuf::from(r.path)))
    .collect()
}

/// Where the window's Conflicts drafts are kept.
pub(crate) fn drafts_dir(app: &AppHandle) -> CommandResult<PathBuf> {
    let dir = app.path().app_data_dir().map_err(|err| err.to_string())?;
    Ok(dir.join("conflict-drafts"))
}

/// Take a repository off Recent Repositories; with `to`, it moved there (Locate…).
#[tauri::command]
pub(crate) fn forget_repo(app: AppHandle, path: String, to: Option<String>) {
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
pub(crate) fn clear_recent(app: AppHandle, keep: Vec<String>) {
    let mut list = read_recent(&app);
    list.retain(|r| keep.contains(&r.path));
    write_recent(&app, &list);
}
