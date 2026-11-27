//! Opening a repository in a window, after asking whether to trust it, and following what
//! changes in it outside Oxbow.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use oxbow_core::Repo;
use oxbow_core::console;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::preferences::read_settings;
use crate::recent::{drafts_dir, forget_repo, known_repos, remember_repo};
use crate::session::{Address, Located, Opened, RepoRequest, Session, blocking, logged, workdir_key};
use crate::{CommandResult, drafts, trusted, turns};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepoSummary {
    name: String,
    path: String,
    canonical_path: String,
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
    match open(&app, &session, window.label(), path, answer, true).await? {
        TabOpen::Opened { repo, .. } | TabOpen::Existing { repo, .. } | TabOpen::Untrusted { repo } => Ok(repo),
        TabOpen::OtherWindow { .. } => Err("This repository is already open in another window.".into()),
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum TabOpen {
    Opened { session: u64, repo: RepoSummary },
    Existing { session: u64, repo: RepoSummary },
    OtherWindow { window: String },
    Untrusted { repo: RepoSummary },
}

/// Opening pins a session to this window. Activation is a separate IPC command, so the UI
/// chooses when to commit a context switch.
#[tauri::command]
pub(crate) async fn open_repo_tab(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    answer: Option<TrustAnswer>,
) -> CommandResult<TabOpen> {
    open(&app, &session, window.label(), path, answer, false).await
}

async fn open(
    app: &AppHandle,
    session: &Session,
    label: &str,
    path: String,
    answer: Option<TrustAnswer>,
    legacy: bool,
) -> CommandResult<TabOpen> {
    if label != "main" && !label.starts_with("repo-") {
        return Err("Open repositories from a repository window.".into());
    }
    let is_reserved_startup = session
        .repositories
        .lock()
        .expect("session lock")
        .pending(label)
        .is_some();
    let _topology = if is_reserved_startup {
        session.repo_topology.read().await
    } else {
        session.topology()?
    };
    let single = session.repositories.lock().expect("session lock").tabs_disabled;
    if legacy && !single {
        return Err("Open this repository as a tab.".into());
    }
    let replacing = session.begin_open(label, legacy)?;
    let repo = blocking(move || Repo::open(&path)).await?;
    let workdir = repo.workdir().to_path_buf();
    // A second open cannot apply a different trust answer to a session opened while it waited.
    let _opening = session.opening.lock().await;
    let canonical = workdir_key(&workdir);
    let found = {
        let repositories = session.repositories.lock().expect("session lock");
        Session::check_open(&repositories, label, replacing)?;
        repositories.opening_target(&canonical, label, single)
    };
    if let Some(target) = found {
        return duplicate(app, session, label, replacing, &repo, target);
    }
    // Before anything else runs git there: its settings could run programs on the first read.
    let (found, allowed) = {
        let (app, repo) = (app.clone(), repo.clone());
        tauri::async_runtime::spawn_blocking(move || trusted::check(&app, &repo))
            .await
            .map_err(|err| err.to_string())??
    };
    Session::check_open(&session.repositories.lock().expect("session lock"), label, replacing)?;
    let restricted = match answer {
        Some(TrustAnswer::Trust) => false,
        Some(TrustAnswer::Restricted) => !found.is_empty(),
        None if allowed => false,
        None => {
            // Until the user answers, nothing of the repository's runs.
            let (dir, kept) = (workdir.clone(), found.clone());
            blocking(move || oxbow_core::trust::restrict(&dir, &kept)).await?;
            return Ok(TabOpen::Untrusted {
                repo: RepoSummary {
                    name: repo.name(),
                    path: workdir.display().to_string(),
                    canonical_path: canonical.display().to_string(),
                    restricted: Vec::new(),
                    untrusted: Some(found),
                },
            });
        }
    };
    if restricted {
        let (dir, found) = (workdir.clone(), found.clone());
        blocking(move || oxbow_core::trust::restrict(&dir, &found)).await?;
    } else if matches!(answer, Some(TrustAnswer::Trust)) {
        save_trust(app, &workdir, &found)?;
    } else {
        oxbow_core::trust::lift(&workdir);
    }
    let summary = RepoSummary {
        name: repo.name(),
        path: workdir.display().to_string(),
        canonical_path: canonical.display().to_string(),
        restricted: if restricted { found } else { Vec::new() },
        untrusted: None,
    };
    let address = match session.finish_open(label, replacing, repo.clone())? {
        Opened::New(address) => address,
        Opened::Existing(target) => return duplicate(app, session, label, replacing, &repo, target),
    };
    if legacy {
        remember_main(app, label, &summary.path);
    }
    remember_repo(app, &summary.path);
    watch_repo(app.clone(), address.clone(), repo);
    changed(app);
    Ok(TabOpen::Opened {
        session: address.id,
        repo: summary,
    })
}

fn duplicate(
    app: &AppHandle,
    session: &Session,
    label: &str,
    replacing: Option<u64>,
    repo: &Repo,
    target: Located,
) -> CommandResult<TabOpen> {
    if let Located::Open(address) = &target
        && address.window == label
    {
        if replacing.is_some() {
            session.activate(address, replacing)?;
            let path = repo.workdir().display().to_string();
            remember_main(app, label, &path);
            remember_repo(app, &path);
            changed(app);
        }
        return Ok(existing_repo(repo, address));
    }
    crate::windows::raise_repo(app, &target)?;
    Ok(TabOpen::OtherWindow {
        window: target.window().to_owned(),
    })
}

fn existing_repo(repo: &Repo, address: &Address) -> TabOpen {
    let workdir = repo.workdir();
    TabOpen::Existing {
        session: address.id,
        repo: RepoSummary {
            name: repo.name(),
            path: workdir.display().to_string(),
            canonical_path: workdir_key(workdir).display().to_string(),
            restricted: oxbow_core::trust::restricted_findings(workdir),
            untrusted: None,
        },
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TabLocation {
    window: String,
    /// Present only for an open session owned by the calling window.
    session: Option<u64>,
}

/// Look up an open or reserved working folder without opening or activating anything.
#[tauri::command]
pub(crate) fn find_repo_tab(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
) -> Option<TabLocation> {
    let path = workdir_key(Path::new(&path));
    let repositories = session.repositories.lock().expect("session lock");
    let target = repositories.locate(&path, Some(window.label()))?;
    let id = match &target {
        Located::Open(address) if address.window == window.label() => Some(address.id),
        _ => None,
    };
    Some(TabLocation {
        window: target.window().to_owned(),
        session: id,
    })
}

/// Activate an owned session, or publish it in the background after History has loaded.
#[tauri::command]
pub(crate) fn activate_repo_tab(
    app: AppHandle,
    window: RepoRequest,
    session: State<'_, Session>,
    background: Option<bool>,
) -> CommandResult<()> {
    let _topology = session.topology()?;
    let address = window
        .address
        .as_ref()
        .filter(|_| window.explicit)
        .ok_or("A repository session is required.")?;
    if background == Some(true) {
        return session.publish_tab(address);
    }
    let path = session
        .repositories
        .lock()
        .expect("session lock")
        .repo(address)
        .map(|repo| repo.workdir().display().to_string())
        .ok_or_else(|| oxbow_core::Error::Cancelled.to_string())?;
    session.activate(address, None)?;
    remember_main(&app, window.label(), &path);
    changed(&app);
    Ok(())
}

fn remember_main(app: &AppHandle, label: &str, path: &str) {
    // The next start opens what the first window had; another worktree's window doesn't count.
    if label == "main"
        && let Some(file) = last_repo_file(app)
    {
        let _ = std::fs::create_dir_all(file.parent().expect("config file has a parent"));
        let _ = std::fs::write(&file, path);
    }
}

pub(crate) fn changed(app: &AppHandle) {
    let _ = app.emit("repo-changed", ());
}

/// Close the addressed repository. An unaddressed legacy close also supersedes a pending open.
#[tauri::command]
pub(crate) async fn close_repo(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    repo_session: Option<u64>,
    provisional: Option<bool>,
) -> CommandResult<()> {
    let provisional = provisional == Some(true);
    let _topology = if provisional {
        session.repo_topology.read().await
    } else {
        session.topology()?
    };
    let address = match session.resolve(window.label(), repo_session) {
        Ok(address) => address,
        Err(_) if provisional => return Ok(()),
        Err(error) => return Err(error),
    };
    if let Some(address) = address.filter(|address| address.window == window.label()) {
        session.close_one(&address, repo_session.is_none(), provisional)?;
        changed(&app);
    }
    Ok(())
}

#[tauri::command]
pub(crate) fn close_repo_tabs(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    closing: Vec<u64>,
    active: Option<u64>,
    previous: Option<u64>,
) -> CommandResult<()> {
    if let Some(path) = session.close_tabs(window.label(), &closing, active, previous)? {
        remember_main(&app, window.label(), &path.display().to_string());
    }
    changed(&app);
    Ok(())
}

/// Trust This Repository…: git runs its settings from now on, in every window that has it open.
#[tauri::command]
pub(crate) async fn trust_repo(app: AppHandle, path: String) -> CommandResult<()> {
    let dir = PathBuf::from(&path);
    let found = {
        let dir = dir.clone();
        blocking(move || oxbow_core::trust::scan(&dir)).await?
    };
    save_trust(&app, &dir, &found)
}

fn save_trust(app: &AppHandle, dir: &Path, found: &[oxbow_core::TrustFinding]) -> CommandResult<()> {
    trusted::add(app, dir, found).map_err(|err| format!("Couldn’t save repository trust. {err}"))?;
    oxbow_core::trust::lift(dir);
    let _ = app.emit("trust-changed", dir.to_string_lossy());
    Ok(())
}

/// Watch this particular opening of the repository. A slow watch cannot attach after a close
/// and reopen of the same working directory.
fn watch_repo(app: AppHandle, address: Address, repo: Repo) {
    tauri::async_runtime::spawn_blocking(move || {
        let session = app.state::<Session>();
        if !session.live(&address) {
            return;
        }
        let events = app.clone();
        let to = address.clone();
        let turns = session.turns.clone();
        let common = turns::canonical(&repo.common_dir());
        let key = to.turn_key();
        let Ok(watch) = repo.watch(move |changed| {
            let window = turns.other(&common, &key).map(|other| other.folder);
            let change = OutsideChange {
                git: changed.git,
                files: changed.files,
                at: changed.at,
                window,
            };
            events
                .state::<Session>()
                .emit_repo(&events, &to, "outside-change", change, true);
        }) else {
            return;
        };
        session.with_slot(&address, |slot| slot.watch = Some(watch));
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

/// Tell the other sessions of the same shared Git directory as soon as a change finishes.
pub(crate) fn tell_others(app: &AppHandle, session: &Session, repo: &Repo, address: Option<&Address>) {
    let Some(address) = address else { return };
    let common = turns::canonical(&repo.common_dir());
    let others = session
        .repositories
        .lock()
        .expect("session lock")
        .siblings(&common, address);
    let change = OutsideChange {
        git: true,
        files: true,
        at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64),
        window: Some(repo.name()),
    };
    for other in others {
        session.emit_repo(app, &other, "outside-change", change.clone(), true);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InitialRepo {
    path: Option<String>,
    restore_tabs: bool,
    explicit: bool,
}

/// An explicit folder takes priority over reopening the main window's previous session.
#[tauri::command]
pub(crate) fn initial_repo(app: AppHandle, window: tauri::WebviewWindow, session: State<'_, Session>) -> InitialRepo {
    if window.label() != "main" {
        let path = session
            .repositories
            .lock()
            .expect("session lock")
            .pending(window.label())
            .map(|path| path.display().to_string());
        return InitialRepo {
            path,
            restore_tabs: false,
            explicit: false,
        };
    }
    let reopen = read_settings(&app)
        .get("oxbow.startup.reopenRepository")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    if let Some(path) = std::env::args().nth(1).filter(|arg| !arg.starts_with('-')) {
        let restore_tabs = reopen && !session.repositories.lock().expect("session lock").tabs_disabled;
        // A remounted window restores the latest selection instead of applying argv again.
        if !restore_tabs || !session.restored_startup_argument.swap(true, Ordering::AcqRel) {
            return InitialRepo {
                path: Some(path),
                restore_tabs,
                explicit: true,
            };
        }
    }
    let path = reopen
        .then(|| last_repo_file(&app).and_then(|file| std::fs::read_to_string(file).ok()))
        .flatten()
        .map(|path| path.trim().to_owned())
        .filter(|path| !path.is_empty());
    InitialRepo {
        path,
        restore_tabs: reopen,
        explicit: false,
    }
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
    window: RepoRequest,
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
    window: RepoRequest,
    session: State<'_, Session>,
    old: String,
    new: String,
) -> CommandResult<Reconnected> {
    let _topology = session.topology()?;
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
    forget_repo(app, old, Some(path.clone()))?;
    Ok(Reconnected { path, conflict_draft })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxbow_core::{Error, cli, trust};
    use std::sync::{Arc, atomic::AtomicBool};

    #[test]
    fn reopening_a_restricted_tab_uses_the_findings_behind_its_active_restriction() {
        let folder = cli::private_temp_folder("oxbow-restricted-tab-").unwrap();
        let dir = folder.path();
        cli::run_in(dir, &cli::GitCommand::new(["init", "-q"]), None).unwrap();
        cli::run_in(
            dir,
            &cli::GitCommand::new(["config", "--local", "core.fsmonitor", "./watch.sh"]),
            None,
        )
        .unwrap();
        let repo = Repo::open(dir).unwrap();
        let found = trust::scan(repo.workdir()).unwrap();
        assert!(found.iter().any(|finding| finding.key == "core.fsmonitor"));
        trust::restrict(repo.workdir(), &found).unwrap();

        let session = Session::default();
        let Opened::New(opened) = session.finish_open("main", None, repo.clone()).unwrap() else {
            panic!("expected a new tab");
        };
        let target = session
            .repositories
            .lock()
            .unwrap()
            .opening_target(&workdir_key(repo.workdir()), "main", false);
        assert_eq!(target, Some(Located::Open(opened.clone())));

        let stopped = Arc::new(AtomicBool::new(true));
        cli::stoppable(&stopped, || {
            assert!(matches!(trust::scan(repo.workdir()), Err(Error::Cancelled)));
            let TabOpen::Existing { session, repo } = existing_repo(&repo, &opened) else {
                panic!("expected the existing tab");
            };
            assert_eq!(session, opened.id);
            assert_eq!(repo.restricted, found);
            assert!(repo.untrusted.is_none());
        });

        trust::lift(repo.workdir());
        let TabOpen::Existing { repo, .. } = existing_repo(&repo, &opened) else {
            panic!("expected the existing tab");
        };
        assert!(repo.restricted.is_empty());
    }
}
