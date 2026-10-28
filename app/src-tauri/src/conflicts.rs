//! A file in conflict: its sides, the hand-made resolution kept as a draft, and the merge
//! tool.

use std::path::{Path, PathBuf};

use oxbow_core::console;
use oxbow_core::{ConflictFile, MergeTool, Repo};
use serde_json::Value;
use tauri::{AppHandle, State};

use crate::recent::drafts_dir;
use crate::session::{Session, at, current, logged, read};
use crate::{CommandResult, drafts};

/// Both sides of a conflicted file, for the Conflicts screen.
#[tauri::command]
pub(crate) async fn conflict_file(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
) -> CommandResult<ConflictFile> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read a conflict", move || repo.conflict_file(&path)).await
}

/// The merge tool git is set to open, for the Conflicts screen's button.
#[tauri::command]
pub(crate) async fn merge_tool(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Option<MergeTool>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the merge tool", move || Ok(repo.merge_tool())).await
}

/// The draft file of the window's working folder, in the app's data folder.
fn draft_file(app: &AppHandle, repo: &Repo) -> CommandResult<PathBuf> {
    Ok(drafts::file(&drafts_dir(app)?, repo.workdir()))
}

/// The repository at `workdir`, the working folder the Conflicts screen was opened for. A save
/// can arrive after the window opened another repository, and must still go to its own.
fn draft_repo(session: &Session, window: &tauri::WebviewWindow, workdir: &str) -> CommandResult<Repo> {
    if let Ok(repo) = current(session, window)
        && repo.workdir() == Path::new(workdir)
    {
        return Ok(repo);
    }
    let repo = Repo::open(workdir).map_err(|err| err.to_string())?;
    if repo.workdir() != Path::new(workdir) {
        return Err(format!("{workdir} isn’t a working folder any more."));
    }
    Ok(repo)
}

/// The Conflicts screen's draft saved for the working folder `workdir`, if there is one.
#[tauri::command]
pub(crate) async fn conflict_draft(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    workdir: String,
) -> CommandResult<Option<Value>> {
    let repo = draft_repo(&session, &window, &workdir)?;
    let file = draft_file(&app, &repo)?;
    tauri::async_runtime::spawn_blocking(move || drafts::read(&file, repo.workdir()))
        .await
        .map_err(|err| err.to_string())?
}

/// Save the Conflicts screen's draft for the working folder `workdir`, while its operation is
/// under way.
#[tauri::command]
pub(crate) async fn save_conflict_draft(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    workdir: String,
    draft: Value,
) -> CommandResult<()> {
    let repo = draft_repo(&session, &window, &workdir)?;
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

/// Remove the draft of the working folder `workdir`, once its operation is over.
#[tauri::command]
pub(crate) async fn drop_conflict_draft(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    workdir: String,
) -> CommandResult<()> {
    let repo = draft_repo(&session, &window, &workdir)?;
    drafts::remove(&draft_file(&app, &repo)?)
}

/// Open a conflicted file in the merge tool and wait until it quits: true when git staged it.
#[tauri::command]
pub(crate) async fn open_merge_tool(
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
