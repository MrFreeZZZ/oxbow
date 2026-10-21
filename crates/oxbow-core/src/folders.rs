//! What is in the repository's other folders: uncommitted changes in the other worktrees, and
//! what each submodule's folder has checked out. Each takes a `git status` or more in that
//! folder, so the history doesn't wait for them: it lists the folders as pending, and they are
//! read after it, a few at once, each answered as soon as it is known.
//!
//! A folder read before is not read again while its `HEAD`, its index and the commit it should
//! have stay as they were, unless asked for afresh: an edit to a file there moves none of them,
//! so after the window comes back to the front the app asks for every folder again.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime};

use serde::Serialize;

use crate::cli;
use crate::console::Helpers;
use crate::error::{Error, Result};
use crate::repo::Repo;
use crate::submodules::{Checkout, Submodule};
use crate::worktrees::Worktree;

/// Folders read at once: more `git status` side by side only fight over the disk.
pub const AT_ONCE: usize = 4;
/// How long a folder's state is kept while nothing shows it changed, as a safety net.
const KEEP: Duration = Duration::from_secs(5 * 60);

/// One folder's state, read after the history.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FolderState {
    /// The worktree's path as git lists it, or the submodule's path in the repository.
    pub path: String,
    pub submodule: bool,
    /// Files with uncommitted changes there, untracked ones included; `None` when `git status`
    /// failed, and `status_error` says why.
    pub changed: Option<usize>,
    pub status_error: Option<String>,
    /// What a submodule's folder has checked out.
    pub checkout: Option<Checkout>,
}

/// The folder states read last, to answer the next ask without reading a folder that didn't
/// change. One per window.
#[derive(Debug, Clone, Default)]
pub struct FolderCache(Arc<Mutex<HashMap<(bool, String), Kept>>>);

#[derive(Debug)]
struct Kept {
    stamp: u64,
    at: SystemTime,
    state: FolderState,
}

/// A folder to read.
struct Folder {
    path: String,
    dir: PathBuf,
    submodule: bool,
    /// For a submodule, the commit the repository records for it.
    recorded: Option<String>,
    stamp: u64,
}

impl Worktree {
    /// Take what [`Repo::folder_states`] read of its folder.
    pub fn take_state(&mut self, state: &FolderState) {
        self.changed = state.changed;
        self.status_error = state.status_error.clone();
        self.pending = false;
    }
}

impl Submodule {
    /// Take what [`Repo::folder_states`] read of its folder.
    pub fn take_state(&mut self, state: &FolderState) {
        self.changed = state.changed;
        self.status_error = state.status_error.clone();
        let checkout = state.checkout.clone().unwrap_or_default();
        self.current = checkout.current;
        self.branch = checkout.branch;
        self.ahead = checkout.ahead;
        self.behind = checkout.behind;
        self.missing = checkout.missing;
        self.pending = false;
    }
}

impl Repo {
    /// Read the folders of `worktrees` and `submodules` left pending, at most `at_once` at a
    /// time, passing each state to `on_state` as it is known. One `cache` has and the folder
    /// shows no change since comes from there at once, unless it was read before `since`.
    /// Setting `cancel` stops the reading: what is still running is killed, and the answer is
    /// [`Error::Cancelled`].
    #[allow(clippy::too_many_arguments)]
    pub fn folder_states(
        &self,
        worktrees: &[Worktree],
        submodules: &[Submodule],
        cache: &FolderCache,
        since: Option<SystemTime>,
        at_once: usize,
        cancel: &Arc<AtomicBool>,
        on_state: &mut dyn FnMut(FolderState),
    ) -> Result<()> {
        let mut folders = Vec::new();
        for tree in worktrees.iter().filter(|t| t.pending) {
            let dir = PathBuf::from(&tree.path);
            let stamp = stamp(&dir, (&tree.head, &tree.branch));
            folders.push(Folder {
                path: tree.path.clone(),
                dir,
                submodule: false,
                recorded: None,
                stamp,
            });
        }
        for sub in submodules.iter().filter(|s| s.pending) {
            let dir = self.workdir().join(&sub.path);
            let stamp = stamp(&dir, &sub.recorded);
            folders.push(Folder {
                path: sub.path.clone(),
                dir,
                submodule: true,
                recorded: sub.recorded.clone(),
                stamp,
            });
        }
        // Folders that didn't change are answered first, without a process.
        let now = SystemTime::now();
        let mut queue = Vec::new();
        {
            let kept = cache.0.lock().expect("folder cache lock");
            for folder in folders {
                let found = kept.get(&(folder.submodule, folder.path.clone())).filter(|k| {
                    k.stamp == folder.stamp
                        && since.is_none_or(|since| k.at >= since)
                        && now.duration_since(k.at).is_ok_and(|age| age < KEEP)
                });
                match found {
                    Some(kept) => on_state(kept.state.clone()),
                    None => queue.push(folder),
                }
            }
        }
        if queue.is_empty() {
            return Ok(());
        }
        let next = AtomicUsize::new(0);
        let helpers = Helpers::default();
        let (send, receive) = mpsc::channel::<(usize, FolderState, SystemTime)>();
        std::thread::scope(|scope| {
            for _ in 0..at_once.clamp(1, queue.len()) {
                let send = send.clone();
                let (queue, next, helpers) = (&queue, &next, &helpers);
                scope.spawn(move || {
                    helpers.help(|| {
                        cli::stoppable(cancel, || {
                            while !cancel.load(Ordering::Relaxed) {
                                let i = next.fetch_add(1, Ordering::Relaxed);
                                let Some(folder) = queue.get(i) else { break };
                                let at = SystemTime::now();
                                match read_folder(folder) {
                                    Ok(state) => {
                                        let _ = send.send((i, state, at));
                                    }
                                    Err(_) => break,
                                }
                            }
                        })
                    })
                });
            }
            drop(send);
            for (i, state, at) in receive {
                let folder = &queue[i];
                cache.0.lock().expect("folder cache lock").insert(
                    (folder.submodule, folder.path.clone()),
                    Kept {
                        stamp: folder.stamp,
                        at,
                        state: state.clone(),
                    },
                );
                on_state(state);
            }
        });
        helpers.join();
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        Ok(())
    }

    /// Read every folder of `worktrees` and `submodules` left pending, a few at once, and put
    /// what they have in them.
    pub(crate) fn fill_folders(&self, worktrees: &mut [Worktree], submodules: &mut [Submodule]) -> Result<()> {
        let cancel = cli::current_stop().unwrap_or_default();
        let mut states = HashMap::new();
        self.folder_states(
            worktrees,
            submodules,
            &FolderCache::default(),
            None,
            AT_ONCE,
            &cancel,
            &mut |state| {
                states.insert((state.submodule, state.path.clone()), state);
            },
        )?;
        for tree in worktrees.iter_mut() {
            if let Some(state) = states.get(&(false, tree.path.clone())) {
                tree.take_state(state);
            }
        }
        for sub in submodules.iter_mut() {
            if let Some(state) = states.get(&(true, sub.path.clone())) {
                sub.take_state(state);
            }
        }
        Ok(())
    }
}

/// Read one folder: its changed files, and for a submodule what it has checked out. A failing
/// `git status` is part of the state; only a stop is an error.
fn read_folder(folder: &Folder) -> Result<FolderState> {
    let checkout = folder
        .submodule
        .then(|| crate::submodules::read_checkout(&folder.dir, folder.recorded.as_deref()));
    let (changed, status_error) = match crate::worktrees::changed_files(&folder.dir) {
        Ok(count) => (Some(count), None),
        Err(Error::Cancelled) => return Err(Error::Cancelled),
        Err(err) => (None, Some(err.to_string())),
    };
    // A stop while reading the checkout reads as no commit: not an answer to give.
    cli::check_stop()?;
    Ok(FolderState {
        path: folder.path.clone(),
        submodule: folder.submodule,
        changed,
        status_error,
        checkout,
    })
}

/// Stands for what a `git status` in the folder at `dir` depends on, short of its files: its
/// `HEAD`, its index and the history of its `HEAD` (each commit, checkout and reset adds to
/// it), with `what`, e.g. the commit it should have.
fn stamp(dir: &Path, what: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    what.hash(&mut hasher);
    let meta = |path: PathBuf| std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()));
    if let Some(git) = git_dir(dir) {
        std::fs::read(git.join("HEAD")).ok().hash(&mut hasher);
        meta(git.join("index")).hash(&mut hasher);
        meta(git.join("logs").join("HEAD")).hash(&mut hasher);
    }
    hasher.finish()
}

/// The folder git keeps the folder at `dir`'s `HEAD` and index in: its `.git`, or where the
/// `.git` file of a linked worktree or a submodule points.
fn git_dir(dir: &Path) -> Option<PathBuf> {
    let dot = dir.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let target = text.strip_prefix("gitdir:")?.trim();
    Some(dir.join(target))
}
