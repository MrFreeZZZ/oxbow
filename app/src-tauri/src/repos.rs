//! Opening a repository in a window, after asking whether to trust it, and following what
//! changes in it outside Oxbow.

use std::path::{Path, PathBuf};

use oxbow_core::Repo;
use oxbow_core::console;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::preferences::read_settings;
use crate::recent::{drafts_dir, forget_repo, known_repos, remember_repo};
use crate::session::{Session, blocking, logged, read};
use crate::{CommandResult, drafts, trusted, turns};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepoSummary {
    name: String,
    path: String,
    /// Open Restricted: the repository's settings that git runs with off.
    restricted: Vec<oxbow_core::TrustFinding>,
    /// Nothing was opened: the repository's settings run programs, and the user hasn't said
    /// whether to trust it.
    untrusted: Option<Vec<oxbow_core::TrustFinding>>,
}

/// The answer to «Do you really want to let it run these programs?».
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TrustAnswer {
    Trust,
    Restricted,
}

pub(crate) fn last_repo_file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|dir| dir.join("last-repository"))
}

#[tauri::command]
pub(crate) async fn open_repo(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    answer: Option<TrustAnswer>,
) -> CommandResult<RepoSummary> {
    let repo = blocking(move || Repo::open(&path)).await?;
    let workdir = repo.workdir().to_path_buf();
    // Before anything else runs git there: its settings could run programs on the first read.
    let (found, allowed) = {
        let (app, repo) = (app.clone(), repo.clone());
        tauri::async_runtime::spawn_blocking(move || trusted::check(&app, &repo))
            .await
            .map_err(|err| err.to_string())?
    };
    let restricted = match answer {
        Some(TrustAnswer::Trust) => {
            trusted::add(&app, &workdir, &found);
            false
        }
        Some(TrustAnswer::Restricted) => !found.is_empty(),
        None if allowed => false,
        None => {
            // Until the user answers, nothing of the repository's runs.
            let (dir, kept) = (workdir.clone(), found.clone());
            blocking(move || oxbow_core::trust::restrict(&dir, &kept)).await?;
            return Ok(RepoSummary {
                name: repo.name(),
                path: workdir.display().to_string(),
                restricted: Vec::new(),
                untrusted: Some(found),
            });
        }
    };
    if restricted {
        let (dir, found) = (workdir.clone(), found.clone());
        blocking(move || oxbow_core::trust::restrict(&dir, &found)).await?;
    } else {
        oxbow_core::trust::lift(&workdir);
    }
    let summary = RepoSummary {
        name: repo.name(),
        path: workdir.display().to_string(),
        restricted: if restricted { found } else { Vec::new() },
        untrusted: None,
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
    session.slot(&window, |slot| slot.open(repo));
    watch_repo(app.clone(), window.label().to_owned());
    // Settings shows this repository under This Repository.
    let _ = app.emit("repo-changed", ());
    Ok(summary)
}

/// Trust This Repository…: git runs its settings from now on, in every window that has it open.
#[tauri::command]
pub(crate) async fn trust_repo(app: AppHandle, path: String) -> CommandResult<()> {
    let dir = PathBuf::from(&path);
    let found = {
        let dir = dir.clone();
        blocking(move || oxbow_core::trust::scan(&dir))
            .await
            .unwrap_or_default()
    };
    trusted::add(&app, &dir, &found);
    oxbow_core::trust::lift(&dir);
    let _ = app.emit("trust-changed", path);
    Ok(())
}

/// Watch the repository open in window `label`, which hears each change outside it as an
/// `outside-change`. Started after the repository opened: a big folder takes a while to watch.
/// Without a watch the window still reads afresh when it comes back to the front.
fn watch_repo(app: AppHandle, label: String) {
    tauri::async_runtime::spawn_blocking(move || {
        let session = app.state::<Session>();
        let Some(repo) = session
            .windows
            .lock()
            .expect("session lock")
            .get(&label)
            .and_then(|slot| slot.repo.clone())
        else {
            return;
        };
        let events = app.clone();
        let to = label.clone();
        let turns = session.turns.clone();
        let common = turns::canonical(&repo.common_dir());
        let Ok(watch) = repo.watch(move |changed| {
            // Mostly another window's action, still running: it is told by that window's name.
            let window = turns.other(&common, &to).map(|other| other.folder);
            let change = OutsideChange {
                git: changed.git,
                files: changed.files,
                at: changed.at,
                window,
            };
            let _ = events.emit_to(to.as_str(), "outside-change", change);
        }) else {
            return;
        };
        // Another repository was opened meanwhile, or the window closed: the watch stops here.
        if let Some(slot) = session.windows.lock().expect("session lock").get_mut(&label)
            && slot.repo.as_ref().is_some_and(|open| open.workdir() == repo.workdir())
            && slot.watch.is_none()
        {
            slot.watch = Some(watch);
        }
    });
}

/// What changed in a window's repository outside it, as `outside-change` tells it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OutsideChange {
    git: bool,
    files: bool,
    /// When the last of it happened, in milliseconds since 1970.
    at: u64,
    /// The folder name of the repository's other window that changed it; none for a terminal
    /// or an editor.
    window: Option<String>,
}

/// Tell the other windows of `repo` that window `label` changed it, right as it is done.
pub(crate) fn tell_others(app: &AppHandle, session: &Session, repo: &Repo, label: &str) {
    let common = turns::canonical(&repo.common_dir());
    let others: Vec<String> = session
        .windows
        .lock()
        .expect("session lock")
        .iter()
        .filter(|(other, slot)| {
            other.as_str() != label
                && slot
                    .repo
                    .as_ref()
                    .is_some_and(|open| turns::canonical(&open.common_dir()) == common)
        })
        .map(|(other, _)| other.clone())
        .collect();
    let change = OutsideChange {
        git: true,
        files: true,
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        window: Some(repo.name()),
    };
    for other in others {
        let _ = app.emit_to(other.as_str(), "outside-change", change.clone());
    }
}

/// Repository to open at start: the first command-line argument, else the last one used,
/// unless the user turned that off. A window Open in New Window made opens its folder.
#[tauri::command]
pub(crate) fn initial_repo(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> Option<String> {
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

/// A folder that isn't there any more, for the screen that says so.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Gone {
    #[serde(flatten)]
    folder: oxbow_core::GoneFolder,
    /// The Conflicts screen has a draft saved for it.
    conflict_draft: bool,
}

/// Why the repository folder at `path` can't be opened, when it isn't there; none when it is.
#[tauri::command]
pub(crate) async fn gone_folder(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
) -> CommandResult<Option<Gone>> {
    let known = known_repos(&app, &session, &window);
    let dir = drafts_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(path);
        oxbow_core::gone_folder(&path, &known).map(|folder| Gone {
            folder,
            conflict_draft: drafts::read(&drafts::file(&dir, &path), &path).is_ok_and(|draft| draft.is_some()),
        })
    })
    .await
    .map_err(|err| err.to_string())
}

/// What [`reconnect_folder`] did.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Reconnected {
    /// The folder to open.
    path: String,
    /// The Conflicts draft of the old place came along.
    conflict_draft: bool,
}

/// The folder that was at `old` is at `new`: point git at it (see [`oxbow_core::reconnect_folder`]),
/// and move its Conflicts draft and its row in Recent Repositories along. Only the same folder
/// is taken, so a draft never lands on another branch.
#[tauri::command]
pub(crate) async fn reconnect_folder(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    old: String,
    new: String,
) -> CommandResult<Reconnected> {
    let known = known_repos(&app, &session, &window);
    let (from, to) = (PathBuf::from(&old), PathBuf::from(&new));
    let path = logged(
        Some(to.clone()),
        console::EntryKind::Action,
        "Reconnect a moved folder",
        move || oxbow_core::reconnect_folder(&from, &to, &known),
    )
    .await?;
    // A draft that couldn't move stays where it was, for the old place.
    let conflict_draft = drafts::moved(&drafts_dir(&app)?, Path::new(&old), &path).unwrap_or(false);
    let path = path.display().to_string();
    forget_repo(app, old, Some(path.clone()));
    Ok(Reconnected { path, conflict_draft })
}

/// Where a recent repository stands, for its row.
#[tauri::command]
pub(crate) async fn repo_glance(app: AppHandle, path: String) -> CommandResult<oxbow_core::RepoGlance> {
    read(PathBuf::from(&path), "Look at a repository", move || {
        let repo = Repo::open(&path)?;
        // Not trusted yet: `git status` runs restricted, as the window would until asked.
        if let (found, false) = trusted::check(&app, &repo) {
            oxbow_core::trust::restrict(repo.workdir(), &found)?;
        }
        oxbow_core::setup::glance(Path::new(&path))
    })
    .await
}
