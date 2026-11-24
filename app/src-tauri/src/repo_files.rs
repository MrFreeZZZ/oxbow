//! Settings › This Repository › Repository Files: the shared files committed with the
//! repository, read, checked as they are typed and saved.

use oxbow_core::console;
use oxbow_core::repofiles::{FileCheck, RepoFile, RepoFileKind, RepoFileText, Saved};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::CommandResult;
use crate::session::{RepoRequest, Session, at, current, logged, read};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepoFiles {
    /// The remotes are on GitLab, which reads CODEOWNERS from other places than GitHub.
    gitlab: bool,
    files: Vec<RepoFile>,
}

/// The shared files, created or not.
#[tauri::command]
pub(crate) async fn repo_files(
    app: AppHandle,
    window: RepoRequest,
    session: State<'_, Session>,
) -> CommandResult<RepoFiles> {
    let repo = current(&session, &window)?;
    let servers = crate::gitlab_accounts::servers(&app);
    read(&window, at(&repo), "Read the repository files", move || {
        let gitlab = !repo.gitlab_projects(&servers).is_empty();
        Ok(RepoFiles {
            gitlab,
            files: repo.repo_files(gitlab)?,
        })
    })
    .await
}

/// One file's text, or what a new one starts with.
#[tauri::command]
pub(crate) async fn repo_file_text(
    window: RepoRequest,
    session: State<'_, Session>,
    kind: RepoFileKind,
    gitlab: bool,
) -> CommandResult<RepoFileText> {
    let repo = current(&session, &window)?;
    read(&window, at(&repo), "Read a repository file", move || {
        repo.repo_file_text(kind, gitlab)
    })
    .await
}

/// What is wrong in `text`, and the notes beside its lines; `fresh` reads the working copy and
/// the history again. Without `wait`, the text alone while they are being read (`pending`); with
/// it, after they are read, which a newer check in the same `lane` stops.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn check_repo_file(
    window: RepoRequest,
    session: State<'_, Session>,
    kind: RepoFileKind,
    gitlab: bool,
    text: String,
    fresh: bool,
    wait: Option<bool>,
    lane: Option<String>,
) -> CommandResult<FileCheck> {
    let repo = current(&session, &window)?;
    if wait == Some(false) {
        return read(&window, at(&repo), "Check a repository file", move || {
            Ok(repo.check_repo_file_now(kind, gitlab, &text, fresh))
        })
        .await;
    }
    let cancel = session.lane(&window, lane);
    read(&window, at(&repo), "Check a repository file", move || {
        oxbow_core::cli::stoppable(&cancel, || repo.check_repo_file(kind, gitlab, &text, fresh))
    })
    .await
}

/// Write the file, unless something else changed it since it was opened; nothing is staged.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn save_repo_file(
    app: AppHandle,
    window: RepoRequest,
    session: State<'_, Session>,
    kind: RepoFileKind,
    gitlab: bool,
    text: String,
    over: Option<String>,
    expected_repo: String,
) -> CommandResult<Saved> {
    let repo = crate::session::expected(current(&session, &window)?, Some(&expected_repo))?;
    let turns = session.turns.clone();
    let turn_label = window.turn_key();
    let address = window.address.clone();
    let stop = session.start_change(&window, window.label(), false)?;
    let events = app.clone();
    let saved = logged(
        Some(at(&repo)),
        console::EntryKind::Action,
        "Save a repository file",
        move || {
            let _turn = turns
                .take(
                    &repo.common_dir(),
                    crate::turns::Running::new(&turn_label, repo.name(), "Save a repository file".into()),
                    &stop.cancel,
                    |_| {},
                )
                .ok_or(oxbow_core::Error::Cancelled)?;
            let saved = repo.save_repo_file(kind, gitlab, &text, over.as_deref())?;
            if matches!(saved, Saved::Written { .. }) {
                crate::repos::tell_others(&events, &events.state::<Session>(), &repo, address.as_ref());
            }
            Ok(saved)
        },
    )
    .await?;
    if matches!(saved, Saved::Written { .. })
        && let Some(address) = &window.address
    {
        session.emit_repo(&app, address, "repo-touched", (), true);
    }
    Ok(saved)
}
