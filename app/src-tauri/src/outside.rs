//! The repository's files outside Oxbow: in an editor, a terminal, Finder or Quick Look.

use std::path::{Path, PathBuf};

use oxbow_core::Repo;
use serde_json::Value;
use tauri::{AppHandle, State};

use crate::preferences::read_settings;
use crate::session::{Session, current};
use crate::{CommandResult, open_in};

/// The Git LFS download page, for installing it without Homebrew.
#[tauri::command]
pub(crate) fn open_lfs_download() -> CommandResult<()> {
    open_in::open_url("https://git-lfs.com")
}

/// Preview a file of the working copy: Quick Look on macOS, the default app elsewhere.
#[tauri::command]
pub(crate) fn quick_look(window: tauri::WebviewWindow, session: State<'_, Session>, path: String) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    open_in::quick_look(&repo.workdir().join(Path::new(&path)))
}

/// Open a file of the repository in the editor chosen in Settings.
#[tauri::command]
pub(crate) fn open_in_editor(
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
pub(crate) fn reveal_file(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    open_in::reveal(&repo.workdir().join(Path::new(&path)))
}

/// Open the repository folder in the terminal chosen in Settings; with `worktree`, that other
/// working folder of it.
#[tauri::command]
pub(crate) fn open_in_terminal(
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
pub(crate) fn worktree_folder(repo: &Repo, path: &str) -> CommandResult<PathBuf> {
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
pub(crate) fn reveal_worktree(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    open_in::reveal(&worktree_folder(&repo, &path)?)
}
