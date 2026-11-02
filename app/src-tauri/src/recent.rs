//! The recent repositories: the Welcome list, the menu and the folders that went away.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

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

// Serializes read-change-write across every window, including the shared .new file.
static LOCK: Mutex<()> = Mutex::new(());

fn recent_file(app: &AppHandle) -> CommandResult<PathBuf> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join("recent-repositories.json"))
        .map_err(|err| err.to_string())
}

fn read(file: &Path, legacy: Option<&Path>) -> CommandResult<Vec<RecentRepo>> {
    match std::fs::read_to_string(file) {
        Ok(text) => {
            return serde_json::from_str(&text)
                .map_err(|err| format!("recent-repositories.json isn't a list Oxbow can read: {err}."));
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(format!("Can't read recent-repositories.json: {err}.")),
    }
    // Before the list existed, Oxbow remembered only the last repository.
    Ok(legacy
        .and_then(|file| std::fs::read_to_string(file).ok())
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty())
        .map(|path| vec![entry(&path)])
        .unwrap_or_default())
}

fn try_read_recent(app: &AppHandle) -> CommandResult<Vec<RecentRepo>> {
    read(&recent_file(app)?, last_repo_file(app).as_deref())
}

/// Best-effort hints for repository discovery and migration of trusted repositories.
pub(crate) fn read_recent(app: &AppHandle) -> Vec<RecentRepo> {
    try_read_recent(app).unwrap_or_else(|err| {
        eprintln!("{err}");
        Vec::new()
    })
}

fn update(file: &Path, legacy: Option<&Path>, change: impl FnOnce(&mut Vec<RecentRepo>)) -> CommandResult<()> {
    let _lock = LOCK.lock().expect("recent lock");
    let mut list = read(file, legacy)?;
    change(&mut list);
    let text = serde_json::to_string_pretty(&list).map_err(|err| err.to_string())?;
    crate::settings::replace(file, &(text + "\n"))
        .map_err(|err| format!("Can't write recent-repositories.json: {err}."))
}

fn update_recent(app: &AppHandle, change: impl FnOnce(&mut Vec<RecentRepo>)) -> CommandResult<()> {
    update(&recent_file(app)?, last_repo_file(app).as_deref(), change)?;
    // Updating the menu reads the list again, after the write lock has been released.
    menu::recent_changed(app);
    Ok(())
}

fn entry(path: &str) -> RecentRepo {
    RecentRepo {
        path: path.to_owned(),
        opened: now(),
        name: String::new(),
        missing: false,
        gone: None,
    }
}

fn remember(list: &mut Vec<RecentRepo>, path: &str) {
    list.retain(|r| r.path != path);
    list.insert(0, entry(path));
    list.truncate(RECENT_LIMIT);
}

pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Put `path` at the top of Recent Repositories.
pub(crate) fn remember_repo(app: &AppHandle, path: &str) {
    if let Err(err) = update_recent(app, |list| remember(list, path)) {
        // Saving recents is a convenience: a failure must not prevent opening the repository.
        eprintln!("{err}");
    }
}

#[tauri::command]
pub(crate) fn recent_repos(app: AppHandle) -> CommandResult<Vec<RecentRepo>> {
    let list = try_read_recent(&app)?;
    let known: Vec<PathBuf> = list.iter().map(|r| PathBuf::from(&r.path)).collect();
    Ok(list
        .into_iter()
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
        .collect())
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
pub(crate) fn forget_repo(app: AppHandle, path: String, to: Option<String>) -> CommandResult<()> {
    update_recent(&app, |list| match to {
        Some(to) => {
            list.retain(|r| r.path != to);
            if let Some(entry) = list.iter_mut().find(|r| r.path == path) {
                entry.path = to;
            }
        }
        None => list.retain(|r| r.path != path),
    })
}

/// Empty Recent Repositories but for the repositories in `keep` (the one open now).
#[tauri::command]
pub(crate) fn clear_recent(app: AppHandle, keep: Vec<String>) -> CommandResult<()> {
    update_recent(&app, |list| list.retain(|r| keep.contains(&r.path)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oxbow-recent-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn only_an_absent_list_falls_back_to_the_legacy_repository() {
        let dir = folder("legacy");
        let file = dir.join("recent-repositories.json");
        let legacy = dir.join("last-repository");
        std::fs::write(&legacy, " /legacy/repo\n").unwrap();
        assert_eq!(read(&file, Some(&legacy)).unwrap()[0].path, "/legacy/repo");
        update(&file, Some(&legacy), |list| remember(list, "/new/repo")).unwrap();
        let list = read(&file, Some(&legacy)).unwrap();
        assert_eq!(
            list.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            ["/new/repo", "/legacy/repo"]
        );
        update(&file, Some(&legacy), Vec::clear).unwrap();
        assert!(read(&file, Some(&legacy)).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn broken_and_unreadable_lists_are_never_replaced_by_an_update() {
        let dir = folder("broken");
        let file = dir.join("recent-repositories.json");
        let legacy = dir.join("last-repository");
        std::fs::write(&legacy, "/legacy/repo").unwrap();
        for broken in ["", "[", "{}", "[{\"path\":\"/repo\"}]"] {
            std::fs::write(&file, broken).unwrap();
            assert!(read(&file, Some(&legacy)).is_err());
            assert!(update(&file, Some(&legacy), |_| panic!("changed an unreadable list")).is_err());
            assert_eq!(std::fs::read_to_string(&file).unwrap(), broken);
        }
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        assert!(read(&file, Some(&legacy)).is_err());
        assert!(update(&file, Some(&legacy), |_| panic!("changed an unreadable list")).is_err());
        assert!(file.is_dir());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn simultaneous_updates_keep_every_entry_and_readers_only_see_complete_lists() {
        let dir = folder("concurrent");
        let file = dir.join("recent-repositories.json");
        let barrier = std::sync::Barrier::new(16);
        let done = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            let reader = scope.spawn(|| {
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    read(&file, None).unwrap();
                    std::thread::yield_now();
                }
            });
            let writers: Vec<_> = (0..16)
                .map(|i| {
                    let (file, barrier) = (&file, &barrier);
                    scope.spawn(move || {
                        barrier.wait();
                        update(file, None, |list| remember(list, &format!("/repo-{i}"))).unwrap();
                    })
                })
                .collect();
            for writer in writers {
                writer.join().unwrap();
            }
            done.store(true, std::sync::atomic::Ordering::Relaxed);
            reader.join().unwrap();
        });
        let list = read(&file, None).unwrap();
        assert_eq!(list.len(), 16);
        for i in 0..16 {
            assert!(list.iter().any(|entry| entry.path == format!("/repo-{i}")));
        }
        assert!(!dir.join("recent-repositories.json.new").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_writes_preserve_the_list_and_successful_updates_keep_the_limit_and_order() {
        let dir = folder("write");
        let file = dir.join("recent-repositories.json");
        update(&file, None, |list| {
            for i in 0..50 {
                remember(list, &format!("/repo-{i}"));
            }
            remember(list, "/repo-20");
        })
        .unwrap();
        let list = read(&file, None).unwrap();
        assert_eq!(list.len(), RECENT_LIMIT);
        assert_eq!(list[0].path, "/repo-20");
        assert_eq!(list[1].path, "/repo-49");
        assert_eq!(list.iter().filter(|r| r.path == "/repo-20").count(), 1);
        let original = std::fs::read(&file).unwrap();
        let next = dir.join("recent-repositories.json.new");
        std::fs::create_dir(&next).unwrap();
        assert!(update(&file, None, Vec::clear).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), original);
        std::fs::remove_dir(&next).unwrap();
        update(&file, None, Vec::clear).unwrap();
        assert!(read(&file, None).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
