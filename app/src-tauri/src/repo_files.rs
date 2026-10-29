//! Settings › This Repository › Repository Files: the shared files committed with the
//! repository, read, checked as they are typed and saved.

use oxbow_core::console;
use oxbow_core::repofiles::{FileCheck, RepoFile, RepoFileKind, RepoFileText};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::CommandResult;
use crate::session::{Session, at, current, logged, read};

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
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<RepoFiles> {
    let repo = current(&session, &window)?;
    let servers = crate::gitlab_accounts::servers(&app);
    read(at(&repo), "Read the repository files", move || {
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
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    kind: RepoFileKind,
    gitlab: bool,
) -> CommandResult<RepoFileText> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read a repository file", move || {
        repo.repo_file_text(kind, gitlab)
    })
    .await
}

/// What is wrong in `text`, and the notes beside its lines; `fresh` reads the working copy and
/// the history again.
#[tauri::command]
pub(crate) async fn check_repo_file(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    kind: RepoFileKind,
    gitlab: bool,
    text: String,
    fresh: bool,
) -> CommandResult<FileCheck> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Check a repository file", move || {
        repo.check_repo_file(kind, gitlab, &text, fresh)
    })
    .await
}

/// Write the file; nothing is staged. Returns its path.
#[tauri::command]
pub(crate) async fn save_repo_file(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    kind: RepoFileKind,
    gitlab: bool,
    text: String,
) -> CommandResult<String> {
    let repo = current(&session, &window)?;
    let path = logged(
        Some(at(&repo)),
        console::EntryKind::Action,
        "Save a repository file",
        move || repo.save_repo_file(kind, gitlab, &text),
    )
    .await?;
    // Changes shows the file at once.
    let _ = app.emit_to(session.label(&window).as_str(), "repo-touched", ());
    Ok(path)
}
