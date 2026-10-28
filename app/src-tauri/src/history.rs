//! Reads of the history: the graph, a commit and its diff, comparing, file history and
//! blame, search, the Operation Log and Edit Stack.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use oxbow_core::{CommitDetail, DiffContext, DiffPage, FileDiff, FolderState, History, HistoryOptions};
use tauri::State;

use crate::CommandResult;
use crate::session::{Cached, Session, at, blocking, current, current_full, read};

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn history(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    limit: Option<usize>,
    until: Option<String>,
    known: Option<String>,
    focus: Option<bool>,
    quiet: Option<bool>,
    lane: Option<String>,
) -> CommandResult<History> {
    // Taken together: a repository opened in between must not get this one's answer. What the
    // watch noticed is counted before the read begins: a change after it is told again.
    let (repo, number, seen) = session.slot(&window, |slot| {
        let (repo, number) = slot.start_history();
        (repo, number, slot.watch.as_ref().and_then(oxbow_core::Watch::seen))
    });
    let repo = repo.ok_or_else(|| "No repository is open".to_owned())?;
    let cancel = session.lane(&window, lane);
    // Open the repository afresh: the built-in engine reads .git/config once, so a remote that
    // Publish or Settings just added would stay unseen.
    let old = repo.clone();
    let fresh = blocking(move || old.reopen()).await.unwrap_or(repo);
    {
        session.slot(&window, |slot| {
            if slot.repo.as_ref().is_some_and(|open| open.workdir() == fresh.workdir()) {
                slot.repo = Some(fresh.clone());
            }
        });
    }
    let mut repo = fresh;
    repo.set_diff_options(*session.diff.lock().expect("session lock"));
    let options = HistoryOptions {
        limit: limit.unwrap_or(oxbow_core::PAGE),
        until,
        focus: focus.unwrap_or(false),
        seen,
        quiet: quiet.unwrap_or(false),
    };
    let mut cache = session.slot(&window, |slot| slot.history.clone());
    let title = if cache.is_empty() {
        "Read the repository"
    } else {
        "Read the repository again"
    };
    let (mut history, cache) = read(at(&repo), title, move || {
        // Stopped by a newer read of the history or another repository: the git it runs is too.
        let history =
            oxbow_core::cli::stoppable(&cancel, || repo.history_with(&options, &mut cache, known.as_deref()))?;
        Ok((history, cache))
    })
    .await?;
    // The diffs the Changes panel asks for next are of these files; when nothing changed, the
    // panel keeps the reading it has.
    let kept = session
        .slot(&window, |slot| slot.changes.clone())
        .filter(|_| history.unchanged);
    let changes = match kept {
        Some(kept) => {
            history.snapshot = kept.0;
            Some(kept)
        }
        None => {
            history.snapshot = session.snapshots.fetch_add(1, Ordering::Relaxed) + 1;
            history.changes.clone().map(|tree| (history.snapshot, Arc::new(tree)))
        }
    };
    let folders = Arc::new((history.worktrees.clone(), history.submodules.clone()));
    session.slot(&window, |slot| slot.keep_history(number, cache, changes, folders));
    Ok(history)
}

/// What the other worktrees and the submodules' folders the last history listed have, each sent
/// on `on_state` as soon as it is read. A folder that shows no change since it was last read is
/// answered from then, unless that was before `since` (ms since 1970): the window came back to
/// the front, and its files may have changed.
#[tauri::command]
pub(crate) async fn folder_states(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    since: Option<u64>,
    lane: Option<String>,
    on_state: tauri::ipc::Channel<FolderState>,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    let (folders, cache) = session.slot(&window, |slot| (slot.folders.clone(), slot.folder_cache.clone()));
    let since = since.map(|ms| std::time::UNIX_EPOCH + std::time::Duration::from_millis(ms));
    read(at(&repo), "Read the worktrees and submodules", move || {
        repo.folder_states(
            &folders.0,
            &folders.1,
            &cache,
            since,
            oxbow_core::folders::AT_ONCE,
            &cancel,
            &mut |state| {
                // The window may be closing; what it no longer shows needn't arrive.
                let _ = on_state.send(state);
            },
        )
    })
    .await
}

#[tauri::command]
pub(crate) async fn commit_detail(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    id: String,
    quick: Option<bool>,
    count_from: Option<usize>,
    lane: Option<String>,
) -> CommandResult<CommitDetail> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    // Files before `count_from` are counted by the diffs shown of them.
    let count = (quick != Some(true)).then_some(count_from.unwrap_or(0));
    // A commit where a shallow clone stops shows no parent until a deeper fetch.
    let key = format!(
        "detail\0{}\0{id}\0{count:?}\0{:?}\0{}",
        repo.workdir().display(),
        repo.diff_options(),
        repo.shallow_stamp()
    );
    if let Some(Cached::Detail(detail)) = session.commits.lock().expect("session lock").get(&key) {
        return Ok((*detail).clone());
    }
    let detail = read(at(&repo), "Read a commit", move || {
        repo.commit_detail_with(&id, count, &cancel)
    })
    .await?;
    // What a submodule says depends on its clone here, which changes.
    if !detail.files.iter().any(|f| f.submodule.is_some()) {
        let cached = Cached::Detail(Arc::new(detail.clone()));
        session.commits.lock().expect("session lock").put(key, cached);
    }
    Ok(detail)
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn commit_diff(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    id: String,
    path: Option<String>,
    whole_file: bool,
    full: Option<bool>,
    lane: Option<String>,
) -> CommandResult<Vec<FileDiff>> {
    let repo = current_full(&session, &window, full)?;
    let cancel = session.lane(&window, lane);
    let context = if whole_file {
        DiffContext::WholeFile
    } else {
        DiffContext::Compact
    };
    let key = format!(
        "diff\0{}\0{id}\0{path:?}\0{whole_file}\0{:?}\0{}",
        repo.workdir().display(),
        repo.diff_options(),
        repo.shallow_stamp()
    );
    if let Some(Cached::Diffs(diffs)) = session.commits.lock().expect("session lock").get(&key) {
        return Ok((*diffs).clone());
    }
    let diffs = read(at(&repo), "Read a commit's changes", move || {
        repo.commit_diff_with(&id, path.as_deref(), context, &cancel)
    })
    .await?;
    if !diffs.iter().any(|d| d.submodule.is_some()) {
        let cached = Cached::Diffs(Arc::new(diffs.clone()));
        session.commits.lock().expect("session lock").put(key, cached);
    }
    Ok(diffs)
}

/// The diffs of a commit's files from the `from`th on, as many as fit `budget` lines.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn commit_diff_page(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    id: String,
    from: usize,
    budget: usize,
    whole_file: bool,
    lane: Option<String>,
) -> CommandResult<DiffPage> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    let context = if whole_file {
        DiffContext::WholeFile
    } else {
        DiffContext::Compact
    };
    let key = format!(
        "page\0{}\0{id}\0{from}\0{budget}\0{whole_file}\0{:?}\0{}",
        repo.workdir().display(),
        repo.diff_options(),
        repo.shallow_stamp()
    );
    if let Some(Cached::Page(page)) = session.commits.lock().expect("session lock").get(&key) {
        return Ok((*page).clone());
    }
    let page = read(at(&repo), "Read a commit's changes", move || {
        repo.commit_diff_page(&id, from, budget, context, &cancel)
    })
    .await?;
    if !page.diffs.iter().any(|d| d.submodule.is_some()) {
        let cached = Cached::Page(Arc::new(page.clone()));
        session.commits.lock().expect("session lock").put(key, cached);
    }
    Ok(page)
}

#[tauri::command]
pub(crate) async fn compare(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    base: String,
    target: String,
    mode: oxbow_core::CompareMode,
) -> CommandResult<oxbow_core::Comparison> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Compare", move || repo.compare(&base, &target, mode)).await
}

/// The changed files of a comparison with their line counts, from the `count_from`th on: the
/// ones before are counted by the first page of diffs.
#[tauri::command]
pub(crate) async fn compare_counts(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    from: String,
    to: String,
    count_from: usize,
    lane: Option<String>,
) -> CommandResult<Vec<oxbow_core::FileChange>> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Compare", move || {
        repo.tree_files_with(&from, &to, Some(count_from), &cancel)
    })
    .await
}

/// A page of a comparison's diffs: from the `start`th file on, until they have `budget` lines.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn compare_diff_page(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    from: String,
    to: String,
    start: usize,
    budget: usize,
    whole_file: bool,
    lane: Option<String>,
) -> CommandResult<DiffPage> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    let context = if whole_file {
        DiffContext::WholeFile
    } else {
        DiffContext::Compact
    };
    read(at(&repo), "Compare", move || {
        repo.tree_diff_page(&from, &to, start, budget, context, &cancel)
    })
    .await
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn compare_diff(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    from: String,
    to: String,
    path: Option<String>,
    whole_file: bool,
    full: Option<bool>,
    lane: Option<String>,
) -> CommandResult<Vec<FileDiff>> {
    let repo = current_full(&session, &window, full)?;
    let cancel = session.lane(&window, lane);
    let context = if whole_file {
        DiffContext::WholeFile
    } else {
        DiffContext::Compact
    };
    read(at(&repo), "Compare", move || {
        repo.tree_diff_with(&from, &to, path.as_deref(), context, &cancel)
    })
    .await
}

#[tauri::command]
pub(crate) async fn file_history(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    rev: Option<String>,
    lane: Option<String>,
) -> CommandResult<oxbow_core::FileHistory> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Read a file's history", move || {
        repo.file_history_with(&path, rev.as_deref(), &cancel)
    })
    .await
}

#[tauri::command]
pub(crate) async fn blame(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    rev: String,
    lane: Option<String>,
) -> CommandResult<oxbow_core::Blame> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Blame", move || repo.blame_with(&path, &rev, &cancel)).await
}

#[tauri::command]
pub(crate) async fn line_history(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    line: u32,
    rev: String,
    lane: Option<String>,
) -> CommandResult<Vec<String>> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Read a line's history", move || {
        repo.line_history_with(&path, line, &rev, &cancel)
    })
    .await
}

#[tauri::command]
pub(crate) async fn files(window: tauri::WebviewWindow, session: State<'_, Session>) -> CommandResult<Vec<String>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "List the files", move || repo.files()).await
}

#[tauri::command]
pub(crate) async fn file_pickaxe(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
    text: String,
    match_case: bool,
    rev: String,
    lane: Option<String>,
) -> CommandResult<Vec<String>> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Search a file's history", move || {
        repo.file_pickaxe_with(&path, &text, match_case, &rev, &cancel)
    })
    .await
}

/// A series of commits before and after a rewrite, paired by `git range-diff`.
#[tauri::command]
pub(crate) async fn series(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    old: String,
    new: String,
    rewritten: Vec<String>,
) -> CommandResult<oxbow_core::Series> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Compare the series", move || {
        repo.series(&old, &new, &rewritten)
    })
    .await
}

/// The latest rewrite of a branch as its remote branch knows it.
#[tauri::command]
pub(crate) async fn last_rewrite(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    branch: String,
) -> CommandResult<Option<oxbow_core::Rewrite>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Find the latest rewrite", move || repo.last_rewrite(&branch)).await
}

/// The Operation Log, newest step first.
#[tauri::command]
pub(crate) async fn operation_log(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<oxbow_core::OpEntry>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the Undo history", move || repo.operation_log()).await
}

/// Lost work: the commits git's reflog of `HEAD` and the local branches points at, newest first.
#[tauri::command]
pub(crate) async fn reflog_entries(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<oxbow_core::ReflogEntry>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the reflog", move || repo.reflog_entries()).await
}

/// What Fix a Mistake plans from: the state to check again before a fix runs, the changed files
/// and commits only the reflog keeps.
#[tauri::command]
pub(crate) async fn fix_state(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<oxbow_core::FixState> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Look for mistakes to fix", move || repo.fix_state()).await
}

/// The repository now beside how it was before step `id`, or right after it with `after`.
#[tauri::command]
pub(crate) async fn compare_snapshot(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    id: String,
    after: bool,
) -> CommandResult<oxbow_core::SnapshotComparison> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Compare with a step of Undo", move || {
        repo.compare_snapshot(&id, after)
    })
    .await
}

/// The stack `branch` belongs to (the checked-out branch without one), for Edit Stack.
#[tauri::command]
pub(crate) async fn stack(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    branch: Option<String>,
) -> CommandResult<oxbow_core::Stack> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the stack", move || repo.stack(branch.as_deref())).await
}

/// The restack under way after a pull request of a stack merged, and how far it got.
#[tauri::command]
pub(crate) async fn restack(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Option<oxbow_core::RestackState>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the restack", move || Ok(repo.restack())).await
}

/// Keep a restack for its next step, or forget it once it is done.
#[tauri::command]
pub(crate) async fn save_restack(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    restack: Option<oxbow_core::Restack>,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    blocking(move || repo.save_restack(restack.as_ref())).await
}

/// Which of `candidates` has the merged pull request's commit `merged`, for a restack to move
/// onto; none until a fetch brought it.
#[tauri::command]
pub(crate) async fn landed_on(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    merged: String,
    candidates: Vec<String>,
) -> CommandResult<Option<oxbow_core::Landing>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the stack", move || {
        Ok(repo.landed_on(&merged, &candidates))
    })
    .await
}

/// A dry run of an Edit Stack plan: what it changes, where it would stop, what to push.
#[tauri::command]
pub(crate) async fn stack_preview(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    plan: oxbow_core::StackPlan,
) -> CommandResult<oxbow_core::StackPreview> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Preview the stack", move || repo.stack_preview(&plan)).await
}

/// Search the history: messages, code changes, authors or file paths.
#[tauri::command]
pub(crate) async fn search(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    query: oxbow_core::SearchQuery,
    lane: Option<String>,
) -> CommandResult<oxbow_core::SearchResult> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Search", move || repo.search_with(&query, &cancel)).await
}

/// Tell which of `ids`, loaded commits the history could not, are unpushed, for the next load
/// to mark: whether it found out anything.
#[tauri::command]
pub(crate) async fn settle_unpushed(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    ids: Vec<String>,
    lane: Option<String>,
) -> CommandResult<bool> {
    let repo = current(&session, &window)?;
    let cancel = session.lane(&window, lane);
    read(at(&repo), "Check unpushed commits", move || {
        repo.settle_unpushed(&ids, &cancel)
    })
    .await
}

#[tauri::command]
pub(crate) async fn resolve_commit(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    prefix: String,
) -> CommandResult<Option<String>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Find a commit", move || Ok(repo.resolve_commit(&prefix))).await
}
