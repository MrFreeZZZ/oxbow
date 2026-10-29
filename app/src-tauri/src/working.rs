//! Reads of the working copy: changes and their diffs, file previews, submodules and
//! worktrees, and the checks shown before an action.

use std::path::Path;
use std::sync::Arc;

use oxbow_core::{
    ChangelistEdit, Changelists, CommitBrief, DeletionCheck, DiffPage, FileDiff, MergePreview, PatchPreview, Side,
    Source, StashCheck, WorkingTree,
};
use tauri::State;

use crate::session::{Session, at, current, current_full, read};
use crate::{CommandResult, open_in};

/// Bytes of one side of a picture read at most to preview it.
const MAX_PREVIEW_BYTES: u64 = 32 * 1024 * 1024;

/// The bytes of one side of a file diff, to show an image.
#[tauri::command]
pub(crate) async fn source_bytes(
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
pub(crate) async fn source_lines(
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
pub(crate) async fn open_source(
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
            let dir = oxbow_core::cli::private_temp_dir()
                .map_err(|err| err.to_string())?
                .join("versions");
            oxbow_core::cli::create_private_dir(&dir).map_err(|err| err.to_string())?;
            let file = dir.join(named);
            oxbow_core::cli::write_private(&file, &data).map_err(|err| err.to_string())?;
            file
        }
    };
    open_in::open_url(&file.display().to_string())
}

/// Your latest commit messages, for ↑ in the commit message box.
#[tauri::command]
pub(crate) async fn recent_messages(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<String>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read recent commit messages", move || {
        repo.recent_messages(50)
    })
    .await
}

#[tauri::command]
pub(crate) async fn working_tree(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<WorkingTree> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the changes", move || repo.working_tree()).await
}

/// The files of `snapshot`, the reading of the working tree the history read last, with their
/// changed lines counted: the history lists the files at once, and they are counted after.
/// `None` when a newer reading came in since, which is counted on its own.
#[tauri::command]
pub(crate) async fn working_counts(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    snapshot: u64,
    lane: Option<String>,
) -> CommandResult<Option<WorkingTree>> {
    let repo = current(&session, &window)?;
    let kept = |slot: &mut crate::session::Slot| {
        slot.changes
            .as_ref()
            .filter(|(number, _)| *number == snapshot)
            .map(|(_, tree)| tree.clone())
    };
    let Some(tree) = session.slot(&window, kept) else {
        return Ok(None);
    };
    if tree.counted {
        return Ok(Some((*tree).clone()));
    }
    let cancel = session.lane(&window, lane);
    let counted = read(at(&repo), "Count the changed lines", move || {
        let mut tree = (*tree).clone();
        oxbow_core::cli::stoppable(&cancel, || repo.count_lines(&mut tree))?;
        Ok(tree)
    })
    .await?;
    // Kept for the diffs of the same reading, and for the next reload while nothing changes.
    let counted = Arc::new(counted);
    session.slot(&window, |slot| {
        if slot.changes.as_ref().is_some_and(|(number, _)| *number == snapshot) {
            slot.changes = Some((snapshot, counted.clone()));
            slot.history.keep_counts(&counted);
        }
    });
    Ok(Some((*counted).clone()))
}

#[tauri::command]
pub(crate) async fn working_diff(
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
pub(crate) async fn submodule_move(
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
pub(crate) async fn submodules(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<oxbow_core::Submodule>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the submodules", move || repo.read_submodules()).await
}

/// The worktrees alone, with their counts of changed files, for Retry after History couldn't
/// read them.
#[tauri::command]
pub(crate) async fn worktrees(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<oxbow_core::Worktree>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the worktrees", move || repo.worktrees()).await
}

/// Submodules whose recorded commit is on none of their remotes, for the warning before a push.
#[tauri::command]
pub(crate) async fn unpushed_submodules(
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
pub(crate) async fn working_diff_page(
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

/// The changelists and what each holds of the unstaged files: of the files the history just read
/// when `snapshot` is that reading.
#[tauri::command]
pub(crate) async fn changelists(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    snapshot: Option<u64>,
) -> CommandResult<Changelists> {
    let repo = current(&session, &window)?;
    let tree = session.slot(&window, |slot| {
        slot.changes
            .as_ref()
            .filter(|(number, _)| Some(*number) == snapshot)
            .map(|(_, tree)| tree.clone())
    });
    read(at(&repo), "Read the changelists", move || {
        // With none kept, the files needn't be read again.
        if !repo.has_changelists() {
            return Ok(Changelists::default());
        }
        let tree = match tree {
            Some(tree) => tree,
            None => Arc::new(repo.working_tree()?),
        };
        repo.changelists(&tree)
    })
    .await
}

/// Make, rename, delete or activate a changelist, or move changes between them; the lists after.
#[tauri::command]
pub(crate) async fn edit_changelists(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    edit: ChangelistEdit,
) -> CommandResult<Changelists> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Change the changelists", move || {
        repo.edit_changelists(&edit)
    })
    .await
}

/// What deleting a local branch would lose, for the confirmation sheet.
#[tauri::command]
pub(crate) async fn deletion_check(
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
pub(crate) async fn remote_deletion_check(
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
pub(crate) async fn merge_preview(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    branch: String,
) -> CommandResult<MergePreview> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Preview a merge", move || repo.merge_preview(&branch)).await
}

/// What applying the patch files at `paths` would bring, for the Apply Patches sheet.
#[tauri::command]
pub(crate) async fn patch_preview(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    paths: Vec<String>,
) -> CommandResult<PatchPreview> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Preview patches", move || repo.patch_preview(&paths)).await
}

/// The commits Save as Patches writes, for its sheet.
#[tauri::command]
pub(crate) async fn patch_range(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    to: String,
    since: Option<String>,
    count: usize,
) -> CommandResult<Vec<CommitBrief>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "List commits for patches", move || {
        repo.patch_range(&to, since.as_deref(), count)
    })
    .await
}

/// Whether `stash@{index}` applies cleanly to the checked-out commit.
#[tauri::command]
pub(crate) async fn stash_check(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    index: usize,
    id: String,
) -> CommandResult<StashCheck> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Check a stash", move || repo.stash_check(index, &id)).await
}

/// Which files the repository keeps in Git LFS, and whether git-lfs is installed.
#[tauri::command]
pub(crate) async fn lfs_status(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<oxbow_core::LfsStatus> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read Git LFS", move || repo.lfs_status()).await
}

/// What Free Up Space would delete, e.g. `3 files would be pruned (1.4 GB)`.
#[tauri::command]
pub(crate) async fn lfs_prune_preview(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Option<String>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Preview Git LFS cleanup", move || repo.lfs_prune_preview()).await
}
