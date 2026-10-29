//! Noticing what changes in a repository outside Oxbow: a commit, stage, checkout or stash in a
//! terminal or an editor, another window's action, a file saved in the working copy.
//!
//! The folder and its Git folders are watched; what matters is told after a short quiet spell,
//! so a build or `npm install` writing thousands of files is one change, and the app reloads at
//! most every few seconds while it lasts. What doesn't matter is left out: files `.gitignore`
//! ignores, Git's objects, lock files and logs, and Oxbow's own files in `.git/oxbow`.
//!
//! Reading the status rewrites the index when only the files' times changed. Such a rewrite is
//! no change: the index counts only when what it stages changed, or the app's own reload would
//! wake the next one, and two windows of one repository would wake each other.
//!
//! What the watch notices is also counted as it comes, before the quiet spell: while the count
//! stays the same, a reload needn't ask git what changed in the files (see `history.rs`).
//!
//! The files it tells changed are kept as they are by Local History (see `localhistory.rs`).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use notify::{EventKind, RecursiveMode, Watcher};
use serde::Serialize;

use crate::error::{Error, Result};
use crate::localhistory::{self, Keeper};
use crate::repo::Repo;

/// Quiet after a change in Git's folders before it is told: a commit writes several files.
const QUIET_GIT: Duration = Duration::from_millis(300);
/// Quiet after a file in the working copy changed: an editor saves, a formatter follows.
const QUIET_FILES: Duration = Duration::from_millis(1000);
/// Longest a change waits while more keep coming, as during a build.
const LONGEST: Duration = Duration::from_secs(3);

/// What changed outside Oxbow since it was last told.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    /// Branches, tags, `HEAD`, the stashes, what is staged, an operation under way, settings.
    pub git: bool,
    /// Files in the working copy, or what is ignored.
    pub files: bool,
    /// When the last of it happened, in milliseconds since 1970: a reload begun after it
    /// already sees it.
    pub at: u64,
}

/// Watches a repository until dropped.
pub struct Watch {
    _watcher: notify::RecommendedWatcher,
    /// Whether the working copy's files are watched, not only Git's folders: a folder too big
    /// for the system's watches leaves its files to the window coming back to the front.
    pub files: bool,
    /// Changes noticed so far, see [`Watch::seen`].
    seen: Arc<AtomicU64>,
}

impl Watch {
    /// How many changes in the working copy's files and Git's folders the watch has noticed,
    /// counted as they come, before they are told; `None` when it doesn't watch the files. A
    /// rewritten index isn't counted: whether it stages anything else is for the reader to tell.
    pub fn seen(&self) -> Option<u64> {
        self.files.then(|| self.seen.load(Ordering::SeqCst))
    }
}

/// What a changed path is to the repository.
#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Git,
    /// An index: it counts only when what it stages changed.
    Index,
    Files,
    Nothing,
}

impl Repo {
    /// The Git folder shared by all the repository's worktrees. Without the `..` a worktree's
    /// `commondir` file gives (`.git/worktrees/<name>/../..`), so its parent is the main folder.
    pub fn common_dir(&self) -> PathBuf {
        let mut dir = PathBuf::new();
        for part in self.local().common_dir().components() {
            match part {
                Component::ParentDir if dir.file_name().is_some() => {
                    dir.pop();
                }
                Component::CurDir => {}
                part => dir.push(part),
            }
        }
        dir
    }

    /// Watch the repository: `on_change` hears what changed, from a thread of its own, until the
    /// [`Watch`] is dropped.
    pub fn watch(&self, mut on_change: impl FnMut(Changed) + Send + 'static) -> Result<Watch> {
        let local = self.local();
        let git_dirs = dedup(vec![local.git_dir().to_path_buf(), local.common_dir().to_path_buf()]);
        let workdir = self.workdir().to_path_buf();
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(tx).map_err(|err| Error::Git(err.to_string()))?;
        let files = watcher.watch(&workdir, RecursiveMode::Recursive).is_ok();
        for dir in &git_dirs {
            if !files || !dir.starts_with(&workdir) {
                watcher
                    .watch(dir, RecursiveMode::Recursive)
                    .map_err(|err| Error::Git(err.to_string()))?;
            }
        }
        let counted = Arc::new(AtomicU64::new(0));
        let mut seen = Seen::new(self, &workdir, &git_dirs);
        seen.counted = counted.clone();
        // Without the files watched there are no saves to keep.
        seen.keeper = if files { localhistory::keeper(self) } else { None };
        std::thread::Builder::new()
            .name("oxbow-watch".into())
            .spawn(move || seen.run(rx, &mut on_change))
            .map_err(|err| Error::Git(err.to_string()))?;
        Ok(Watch {
            _watcher: watcher,
            files,
            seen: counted,
        })
    }
}

/// The watch's own thread: gathers changes until a quiet spell, then tells them.
struct Seen {
    repo: Repo,
    roots: Roots,
    /// What each index staged when last looked at.
    indexes: HashMap<PathBuf, u64>,
    /// Changes noticed, for [`Watch::seen`].
    counted: Arc<AtomicU64>,
    /// Where the files that changed go to be kept.
    keeper: Option<Arc<Keeper>>,
}

/// Changes gathered since the last one told.
#[derive(Default)]
struct Batch {
    first: Option<Instant>,
    last: Option<Instant>,
    at: u64,
    git: bool,
    files: bool,
    indexes: Vec<PathBuf>,
    /// The files of the working copy that changed, in it, for Local History.
    saved: Vec<String>,
    /// Whether each folder of the working copy is ignored, found out for this batch.
    ignored: HashMap<PathBuf, bool>,
}

impl Seen {
    fn new(repo: &Repo, workdir: &Path, git_dirs: &[PathBuf]) -> Self {
        let mut seen = Seen {
            repo: repo.clone(),
            roots: Roots::new(workdir, git_dirs),
            indexes: HashMap::new(),
            counted: Arc::default(),
            keeper: None,
        };
        // What the index stages now, so that the first reload's rewrite of it counts for nothing.
        let own = repo.local().index_path();
        if let Some(print) = seen.fingerprint(&own) {
            seen.indexes.insert(index_key(&own), print);
        }
        seen
    }

    fn run(mut self, rx: mpsc::Receiver<notify::Result<notify::Event>>, on_change: &mut impl FnMut(Changed)) {
        let local = self.repo.local();
        let mut batch = Batch::default();
        // Made when a batch first needs it: the ignore rules as they are now.
        let mut excludes = None;
        loop {
            let event = match batch.due() {
                None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
                Some(due) => rx.recv_timeout(due.saturating_duration_since(Instant::now())),
            };
            match event {
                Ok(Ok(event)) => {
                    if matches!(event.kind, EventKind::Access(_)) {
                        continue;
                    }
                    let (mut noted, mut counts) = (false, false);
                    for path in &event.paths {
                        let kind = match self.roots.kind(path) {
                            Kind::Files if batch.ignored(&local, &mut excludes, &self.roots, path) => Kind::Nothing,
                            kind => kind,
                        };
                        counts |= matches!(kind, Kind::Git | Kind::Files);
                        if kind == Kind::Files && self.keeper.is_some() {
                            batch.save(&self.roots, path);
                        }
                        noted |= batch.note(kind, path);
                    }
                    // Counted before it is told: a reload beginning after this sees it.
                    if counts {
                        self.counted.fetch_add(1, Ordering::SeqCst);
                    }
                    if noted {
                        batch.touch();
                    }
                }
                // The system lost track, e.g. too many changes at once: anything may have changed.
                Ok(Err(_)) => {
                    self.counted.fetch_add(1, Ordering::SeqCst);
                    batch.git = true;
                    batch.files = true;
                    batch.touch();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let done = std::mem::take(&mut batch);
                    excludes = None;
                    let git = done.git || done.indexes.iter().any(|index| self.restaged(index));
                    if let Some(keeper) = &self.keeper
                        && !done.saved.is_empty()
                    {
                        keeper.tell(done.saved);
                    }
                    if git || done.files {
                        on_change(Changed {
                            git,
                            files: done.files,
                            at: done.at,
                        });
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Whether the index at `path` stages something else than when last looked at.
    fn restaged(&mut self, path: &Path) -> bool {
        let print = self.fingerprint(path);
        let key = index_key(path);
        let before = match print {
            Some(print) => self.indexes.insert(key, print),
            None => self.indexes.remove(&key),
        };
        before != print || print.is_none()
    }

    fn fingerprint(&self, path: &Path) -> Option<u64> {
        staged(path, self.repo.local().object_hash())
    }
}

/// What the index at `path` stages, without the files' times it also keeps: their paths, ids,
/// modes and stages. `None` when it can't be read.
pub(crate) fn staged(path: &Path, hash: gix::hash::Kind) -> Option<u64> {
    let index = gix::index::File::at(path, hash, true, Default::default()).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for entry in index.entries() {
        entry.path(&index).hash(&mut hasher);
        entry.id.hash(&mut hasher);
        entry.mode.bits().hash(&mut hasher);
        entry.flags.bits().hash(&mut hasher);
    }
    Some(hasher.finish())
}

impl Batch {
    /// When the batch is to be told, if it has anything.
    fn due(&self) -> Option<Instant> {
        let (first, last) = (self.first?, self.last?);
        let quiet = if self.files && !self.git && self.indexes.is_empty() {
            QUIET_FILES
        } else {
            QUIET_GIT
        };
        Some((last + quiet).min(first + LONGEST))
    }

    /// Note a changed path; false when it doesn't matter.
    fn note(&mut self, kind: Kind, path: &Path) -> bool {
        match kind {
            Kind::Git => self.git = true,
            Kind::Files => self.files = true,
            Kind::Index => {
                if !self.indexes.iter().any(|p| p == path) {
                    self.indexes.push(path.to_path_buf());
                }
            }
            Kind::Nothing => return false,
        }
        true
    }

    /// Note a file of the working copy that changed, for Local History.
    fn save(&mut self, roots: &Roots, path: &Path) {
        if self.saved.len() >= localhistory::MAX_TOLD {
            return;
        }
        let Some(relative) = roots.in_workdir(path) else {
            return;
        };
        let parts: Option<Vec<&str>> = relative
            .components()
            .map(|c| match c {
                Component::Normal(part) => part.to_str(),
                _ => None,
            })
            .collect();
        if let Some(parts) = parts.filter(|parts| !parts.is_empty()) {
            self.saved.push(parts.join("/"));
        }
    }

    fn touch(&mut self) {
        let now = Instant::now();
        self.first.get_or_insert(now);
        self.last = Some(now);
        self.at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
    }

    /// Whether `path` in the working copy, or a folder it is in, is ignored.
    fn ignored<'r>(
        &mut self,
        repo: &'r gix::Repository,
        excludes: &mut Option<Option<gix::AttributeStack<'r>>>,
        roots: &Roots,
        path: &Path,
    ) -> bool {
        let Some(relative) = roots.in_workdir(path) else {
            return false;
        };
        let stack = excludes.get_or_insert_with(|| {
            let index = repo.index_or_empty().ok()?;
            repo.excludes(
                &index,
                None,
                gix::worktree::stack::state::ignore::Source::WorktreeThenIdMappingIfNotSkipped,
            )
            .ok()
        });
        // Rules that can't be read ignore nothing: the change is told.
        let Some(stack) = stack else {
            return false;
        };
        let mut folder = PathBuf::new();
        let parts: Vec<_> = relative.components().collect();
        for (at, part) in parts.iter().enumerate() {
            folder.push(part);
            let last = at + 1 == parts.len();
            if !last || path.is_dir() {
                let ignored = *self.ignored.entry(folder.clone()).or_insert_with(|| {
                    stack
                        .at_path(&folder, Some(gix::index::entry::Mode::DIR))
                        .is_ok_and(|p| p.is_excluded())
                });
                if ignored {
                    return true;
                }
            } else {
                return stack
                    .at_path(&folder, Some(gix::index::entry::Mode::FILE))
                    .is_ok_and(|p| p.is_excluded());
            }
        }
        false
    }
}

/// The watched folders, each as given and as the system names it (macOS reports /private/var
/// for /var).
struct Roots {
    workdir: Vec<PathBuf>,
    git_dirs: Vec<PathBuf>,
}

impl Roots {
    fn new(workdir: &Path, git_dirs: &[PathBuf]) -> Self {
        let both = |path: &Path| {
            dedup(vec![
                path.to_path_buf(),
                path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
            ])
        };
        Roots {
            workdir: both(workdir),
            git_dirs: git_dirs.iter().flat_map(|dir| both(dir)).collect(),
        }
    }

    /// `path` relative to the working copy, unless it is in a Git folder.
    fn in_workdir<'p>(&self, path: &'p Path) -> Option<&'p Path> {
        self.workdir.iter().find_map(|root| path.strip_prefix(root).ok())
    }

    fn kind(&self, path: &Path) -> Kind {
        // The deepest Git folder holding it: a worktree's own one is in the shared one.
        if let Some(relative) = self
            .git_dirs
            .iter()
            .filter_map(|root| path.strip_prefix(root).ok())
            .min_by_key(|relative| relative.components().count())
        {
            return git_kind(relative);
        }
        match self.in_workdir(path) {
            // Another repository's Git folder in the working copy, e.g. one cloned inside it.
            Some(relative) if relative.components().any(|c| c.as_os_str() == ".git") => Kind::Nothing,
            Some(relative) if relative.as_os_str().is_empty() => Kind::Nothing,
            Some(_) => Kind::Files,
            None => Kind::Nothing,
        }
    }
}

/// What a path in a Git folder is: `relative` to that folder.
fn git_kind(relative: &Path) -> Kind {
    let parts: Vec<&str> = relative
        .components()
        .filter_map(|c| match c {
            Component::Normal(part) => part.to_str(),
            _ => None,
        })
        .collect();
    let Some(name) = parts.last() else {
        return Kind::Nothing;
    };
    if name.ends_with(".lock") {
        return Kind::Nothing;
    }
    match parts.as_slice() {
        // Another worktree's Git folder, or a submodule's: its HEAD and what it stages.
        ["worktrees", _, rest @ ..] if !rest.is_empty() => git_kind(&rest.iter().collect::<PathBuf>()),
        ["modules", .., "HEAD"] => Kind::Git,
        ["modules", .., "index"] => Kind::Index,
        // A commit in a submodule moves its branch, not its HEAD: the folder shows other commits.
        ["modules", rest @ ..] if rest.contains(&"refs") || rest.last() == Some(&"packed-refs") => Kind::Git,
        ["modules", ..] => Kind::Nothing,
        ["index"] => Kind::Index,
        ["refs", ..] | ["logs", "refs", "stash"] => Kind::Git,
        ["rebase-merge", ..] | ["rebase-apply", ..] | ["sequencer", ..] => Kind::Git,
        ["info", "exclude"] => Kind::Files,
        [name] if matches!(*name, "HEAD" | "packed-refs" | "config" | "shallow") => Kind::Git,
        // MERGE_HEAD, CHERRY_PICK_HEAD and the like say an operation is under way; FETCH_HEAD and
        // ORIG_HEAD come with changes told otherwise.
        [name] if name.ends_with("_HEAD") && !matches!(*name, "FETCH_HEAD" | "ORIG_HEAD") => Kind::Git,
        [name] if name.starts_with("BISECT_") => Kind::Git,
        _ => Kind::Nothing,
    }
}

/// The same index however its path is spelled: the system may tell a path through a link by
/// where the link leads, as macOS tells its temp folder's `/var` as `/private/var`.
fn index_key(path: &Path) -> PathBuf {
    match (path.parent().and_then(|dir| dir.canonicalize().ok()), path.file_name()) {
        (Some(dir), Some(name)) => dir.join(name),
        _ => path.to_path_buf(),
    }
}

fn dedup(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = Vec::new();
    paths.retain(|p| {
        let new = !seen.contains(p);
        seen.push(p.clone());
        new
    });
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_folder_paths() {
        let kind = |p: &str| git_kind(Path::new(p));
        assert_eq!(kind("HEAD"), Kind::Git);
        assert_eq!(kind("index"), Kind::Index);
        assert_eq!(kind("index.lock"), Kind::Nothing);
        assert_eq!(kind("refs/heads/main"), Kind::Git);
        assert_eq!(kind("refs/heads/main.lock"), Kind::Nothing);
        assert_eq!(kind("logs/refs/stash"), Kind::Git);
        assert_eq!(kind("logs/HEAD"), Kind::Nothing);
        assert_eq!(kind("objects/ab/cdef"), Kind::Nothing);
        assert_eq!(kind("oxbow/oplog"), Kind::Nothing);
        assert_eq!(kind("MERGE_HEAD"), Kind::Git);
        assert_eq!(kind("FETCH_HEAD"), Kind::Nothing);
        assert_eq!(kind("COMMIT_EDITMSG"), Kind::Nothing);
        assert_eq!(kind("rebase-merge/done"), Kind::Git);
        assert_eq!(kind("worktrees/feature/HEAD"), Kind::Git);
        assert_eq!(kind("worktrees/feature/index"), Kind::Index);
        assert_eq!(kind("worktrees/feature/logs/HEAD"), Kind::Nothing);
        assert_eq!(kind("modules/lib/HEAD"), Kind::Git);
        assert_eq!(kind("modules/lib/objects/pack/x"), Kind::Nothing);
        assert_eq!(kind("modules/lib/refs/heads/main"), Kind::Git);
        assert_eq!(kind("modules/lib/packed-refs"), Kind::Git);
        assert_eq!(kind("modules/lib/refs/heads/main.lock"), Kind::Nothing);
        assert_eq!(kind("info/exclude"), Kind::Files);
    }

    #[cfg(unix)]
    #[test]
    fn an_index_told_by_where_a_link_leads_is_the_one_looked_at_first() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, dir.path().join("link")).unwrap();
        std::fs::write(real.join("a.txt"), "a\n").unwrap();
        for args in [&["init", "-q"][..], &["add", "a.txt"]] {
            let status = std::process::Command::new("git").args(args).current_dir(&real).status();
            assert!(status.unwrap().success());
        }
        let opened = dir.path().join("link");
        let repo = Repo::open(&opened).unwrap();
        let mut seen = Seen::new(&repo, &opened, &[opened.join(".git")]);
        // A `git status` rewrote the index; the system tells it by its real path, or as opened.
        assert!(!seen.restaged(&real.join(".git/index")));
        assert!(!seen.restaged(&opened.join(".git/index")));
    }
}
