//! The state of each window, its repository and its Stop buttons, and the helpers every
//! command uses to run Git work off the main thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use oxbow_core::console;
use oxbow_core::{
    CommitDetail, DiffOptions, DiffPage, FileDiff, FolderCache, HistoryCache, Repo, Submodule, WorkingTree, Worktree,
};
use tauri::State;

use crate::{CommandResult, turns};

/// What each window has open, by window label: every repository window ("main", and
/// "repo-N" for the ones Open in New Window makes) has its own repository and Stop buttons.
#[derive(Default)]
pub(crate) struct Session {
    pub(crate) windows: Mutex<HashMap<String, Slot>>,
    /// Diff settings, given to the repository on every call.
    pub(crate) diff: Mutex<DiffOptions>,
    /// The repository window used last: the one Settings shows and changes.
    pub(crate) focused: Mutex<Option<String>>,
    /// The folder a new window opens, until it asks for it.
    pub(crate) pending: Mutex<HashMap<String, String>>,
    /// Number of the next new window.
    pub(crate) next: AtomicUsize,
    /// Number of the last reading of the working tree handed out with a history.
    pub(crate) snapshots: AtomicU64,
    /// Commit details and diffs already worked out: a commit never changes.
    pub(crate) commits: Mutex<CommitCache>,
    /// Number of the last change to settings.json. Held while settings.json is read and written,
    /// so two saves at once can't lose each other's key, and sent with every change, so each
    /// window takes the changes in the order they were saved.
    pub(crate) settings: Mutex<u64>,
    /// Changes running in each repository, one at a time.
    pub(crate) turns: Arc<turns::Turns>,
    /// The Stop of each run going now.
    pub(crate) stops: Arc<Stops>,
}

/// The Stop of each action, clone and GitHub or GitLab call running now, by the label of the
/// window whose sheet runs it: Settings has its own.
#[derive(Default)]
pub(crate) struct Stops {
    running: Mutex<HashMap<String, Vec<Arc<AtomicBool>>>>,
}

/// A run's own Stop, heard by its window's Stop button until it is dropped.
pub(crate) struct RunStop {
    stops: Arc<Stops>,
    window: String,
    pub(crate) cancel: Arc<AtomicBool>,
}

impl Stops {
    /// A Stop of its own for a run starting now in `window`: a run after it never clears it,
    /// and Stop in another window doesn't reach it.
    pub(crate) fn start(self: &Arc<Self>, window: &str) -> RunStop {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut running = self.running.lock().expect("stops lock");
        running.entry(window.to_owned()).or_default().push(cancel.clone());
        RunStop {
            stops: self.clone(),
            window: window.to_owned(),
            cancel,
        }
    }

    /// Stop what `window` runs.
    pub(crate) fn stop(&self, window: &str) {
        let running = self.running.lock().expect("stops lock");
        for cancel in running.get(window).into_iter().flatten() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

impl Drop for RunStop {
    fn drop(&mut self) {
        let mut running = self.stops.running.lock().expect("stops lock");
        if let Some(list) = running.get_mut(&self.window) {
            list.retain(|cancel| !Arc::ptr_eq(cancel, &self.cancel));
            if list.is_empty() {
                running.remove(&self.window);
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Slot {
    pub(crate) repo: Option<Repo>,
    /// Stop of the fetch running in the background, apart from actions in the sheet.
    pub(crate) fetch_cancel: Arc<AtomicBool>,
    /// The commit rows of the last history, for the next reload to reuse.
    pub(crate) history: HistoryCache,
    /// Number of the last history read started here, or of the last repository opened here:
    /// a read started before that is of the repository open before.
    history_reads: u64,
    /// Number of the read whose answer `history`, `changes` and `folders` hold.
    history_kept: u64,
    /// The working tree read with the last history, by number, for the Changes panel's diffs.
    pub(crate) changes: Option<(u64, Arc<WorkingTree>)>,
    /// The worktrees and submodules the last history listed, whose folders `folder_states` reads.
    pub(crate) folders: Arc<(Vec<Worktree>, Vec<Submodule>)>,
    /// What those folders had when last read.
    pub(crate) folder_cache: FolderCache,
    /// Stop of the read running in each lane (a search, the commit panel's diff): a newer read
    /// in the lane stops the older, whose answer would be dropped anyway.
    lanes: HashMap<String, Arc<AtomicBool>>,
    /// Tells the window what changes in its repository outside it; stops when dropped.
    pub(crate) watch: Option<oxbow_core::Watch>,
}

impl Slot {
    /// Open `repo` in this window: what was read of the repository open before goes.
    pub(crate) fn open(&mut self, repo: Repo) {
        self.close();
        self.repo = Some(repo);
    }

    /// Forget the repository, including late reads and its background fetch.
    pub(crate) fn close(&mut self) {
        self.stop_reads();
        self.fetch_cancel.store(true, Ordering::Relaxed);
        self.repo = None;
        self.history = HistoryCache::default();
        self.changes = None;
        self.folders = Arc::default();
        self.folder_cache = FolderCache::default();
        self.watch = None;
        self.history_reads += 1;
        self.history_kept = self.history_reads;
    }

    /// The repository open here and the number of a history read of it starting now, for
    /// [`Slot::keep_history`].
    pub(crate) fn start_history(&mut self) -> (Option<Repo>, u64) {
        self.history_reads += 1;
        (self.repo.clone(), self.history_reads)
    }

    /// Keep what history read `number` found, for the next reload and the Changes panel, unless
    /// another repository was opened since it started or a newer read was kept: a late answer of
    /// the repository open before would hand its rows, files and folders to this one.
    pub(crate) fn keep_history(
        &mut self,
        number: u64,
        cache: HistoryCache,
        changes: Option<(u64, Arc<WorkingTree>)>,
        folders: Arc<(Vec<Worktree>, Vec<Submodule>)>,
    ) {
        if number <= self.history_kept {
            return;
        }
        self.history_kept = number;
        self.history = cache;
        self.changes = changes;
        self.folders = folders;
    }

    /// Stop every read running for this window: its repository changed, or it closed.
    pub(crate) fn stop_reads(&mut self) {
        for (_, cancel) in self.lanes.drain() {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// What [`Session::commits`] holds.
#[derive(Clone)]
pub(crate) enum Cached {
    Detail(Arc<CommitDetail>),
    Diffs(Arc<Vec<FileDiff>>),
    Page(Arc<DiffPage>),
}

/// Commit details and diffs by what they were asked with, the most recent last, kept under a
/// budget of memory.
#[derive(Default)]
pub(crate) struct CommitCache {
    entries: std::collections::VecDeque<(String, Cached, usize)>,
    bytes: usize,
}

impl CommitCache {
    /// About what the cached answers take, in bytes.
    const BUDGET: usize = 64 * 1024 * 1024;

    pub(crate) fn get(&mut self, key: &str) -> Option<Cached> {
        let at = self.entries.iter().position(|(k, ..)| k == key)?;
        let entry = self.entries.remove(at)?;
        let found = entry.1.clone();
        self.entries.push_back(entry);
        Some(found)
    }

    pub(crate) fn put(&mut self, key: String, value: Cached) {
        let size = match &value {
            Cached::Detail(detail) => 1024 + detail.files.len() * 256 + detail.body.len(),
            Cached::Diffs(diffs) => diffs_size(diffs),
            Cached::Page(page) => diffs_size(&page.diffs),
        };
        // One answer bigger than a quarter of the budget would push out everything else.
        if size > Self::BUDGET / 4 {
            return;
        }
        if let Some(at) = self.entries.iter().position(|(k, ..)| *k == key)
            && let Some((.., old)) = self.entries.remove(at)
        {
            self.bytes -= old;
        }
        self.bytes += size;
        self.entries.push_back((key, value, size));
        while self.bytes > Self::BUDGET {
            let Some((.., old)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= old;
        }
    }
}

/// About what `diffs` take in memory, in bytes.
fn diffs_size(diffs: &[FileDiff]) -> usize {
    diffs
        .iter()
        .map(|d| {
            512 + d.file.path.len()
                + d.hunks
                    .iter()
                    .flat_map(|h| &h.lines)
                    .map(|l| 96 + l.text.len() * 2)
                    .sum::<usize>()
        })
        .sum()
}

impl Session {
    /// The repository window `window` acts for: itself, or for Settings the one used last.
    pub(crate) fn label(&self, window: &tauri::WebviewWindow) -> String {
        if window.label() == "settings" {
            self.focused
                .lock()
                .expect("session lock")
                .clone()
                .unwrap_or_else(|| "main".to_owned())
        } else {
            window.label().to_owned()
        }
    }

    pub(crate) fn slot<T>(&self, window: &tauri::WebviewWindow, f: impl FnOnce(&mut Slot) -> T) -> T {
        let label = self.label(window);
        f(self.windows.lock().expect("session lock").entry(label).or_default())
    }

    /// A Stop of its own for a run starting now in `window`, which its Stop button reaches.
    pub(crate) fn new_stop(&self, window: &tauri::WebviewWindow) -> RunStop {
        self.stops.start(window.label())
    }

    pub(crate) fn fetch_cancel(&self, window: &tauri::WebviewWindow) -> Arc<AtomicBool> {
        self.slot(window, |slot| slot.fetch_cancel.clone())
    }

    /// A Stop of its own for a new background fetch: one stopped before it, e.g. of the
    /// repository open before, stays stopped.
    pub(crate) fn new_fetch_cancel(&self, window: &tauri::WebviewWindow) -> Arc<AtomicBool> {
        self.slot(window, |slot| {
            slot.fetch_cancel = Arc::new(AtomicBool::new(false));
            slot.fetch_cancel.clone()
        })
    }

    /// A Stop for a new read in `lane`, which stops the one running there. Without a lane the
    /// read runs to its end.
    pub(crate) fn lane(&self, window: &tauri::WebviewWindow, lane: Option<String>) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        if let Some(lane) = lane
            && let Some(older) = self.slot(window, |slot| slot.lanes.insert(lane, cancel.clone()))
        {
            older.store(true, Ordering::Relaxed);
        }
        cancel
    }

    /// The window that has the repository at `path` open, if any.
    pub(crate) fn window_of(&self, path: &Path) -> Option<String> {
        let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.windows
            .lock()
            .expect("session lock")
            .iter()
            .find(|(_, slot)| {
                slot.repo
                    .as_ref()
                    .is_some_and(|r| r.workdir().canonicalize().unwrap_or_else(|_| r.workdir().to_path_buf()) == path)
            })
            .map(|(label, _)| label.clone())
    }
}

/// The repository, set to show a diff in full however many lines it has when `full` is set
/// (Show Diff Anyway).
pub(crate) fn current_full(
    session: &Session,
    window: &tauri::WebviewWindow,
    full: Option<bool>,
) -> CommandResult<Repo> {
    let mut repo = current(session, window)?;
    if full == Some(true) {
        repo.set_diff_options(DiffOptions {
            max_lines: 0,
            ..repo.diff_options()
        });
    }
    Ok(repo)
}

/// The repository open in the window `window` acts for.
pub(crate) fn current(session: &Session, window: &tauri::WebviewWindow) -> CommandResult<Repo> {
    let mut repo = session
        .slot(window, |slot| slot.repo.clone())
        .ok_or_else(|| "No repository is open".to_owned())?;
    repo.set_diff_options(*session.diff.lock().expect("session lock"));
    Ok(repo)
}

/// Run blocking Git work off the main thread so the window stays responsive.
pub(crate) async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> oxbow_core::Result<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())
}

/// A write must still target the repository whose controls the user saw.
pub(crate) fn expected(repo: Repo, path: Option<&str>) -> CommandResult<Repo> {
    if path != Some(repo.workdir().to_string_lossy().as_ref()) {
        return Err("The repository changed. Reload Settings and try again.".to_owned());
    }
    Ok(repo)
}

/// The working tree of `repo`, for the Console entry of work on it.
pub(crate) fn at(repo: &Repo) -> PathBuf {
    repo.workdir().to_path_buf()
}

/// [`blocking`], as one entry of the Console of the repository at `dir` (none: outside any
/// repository).
pub(crate) async fn logged<T: Send + 'static>(
    dir: Option<PathBuf>,
    kind: console::EntryKind,
    title: impl Into<String>,
    work: impl FnOnce() -> oxbow_core::Result<T> + Send + 'static,
) -> CommandResult<T> {
    let title = title.into();
    blocking(move || console::scope(dir.as_deref(), kind, title, work)).await
}

/// [`blocking`] for a read of the repository at `dir`, shown in the Console as `title` when it
/// runs git.
pub(crate) async fn read<T: Send + 'static>(
    dir: PathBuf,
    title: &'static str,
    work: impl FnOnce() -> oxbow_core::Result<T> + Send + 'static,
) -> CommandResult<T> {
    logged(Some(dir), console::EntryKind::Read, title, work).await
}

/// Stop the reads running in `lanes`: what they would answer is no longer wanted, as the
/// search was cleared or the commit panel closed.
#[tauri::command]
pub(crate) fn stop_reads(window: tauri::WebviewWindow, session: State<'_, Session>, lanes: Vec<String>) {
    session.slot(&window, |slot| {
        for lane in lanes {
            if let Some(cancel) = slot.lanes.remove(&lane) {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    });
}

/// Stop the action running in this window's sheet.
#[tauri::command]
pub(crate) fn stop_action(window: tauri::WebviewWindow, session: State<'_, Session>) {
    session.stops.stop(window.label());
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxbow_core::HistoryOptions;

    /// A repository with one commit in a folder of its own.
    fn repo(name: &str) -> Repo {
        let dir = std::env::temp_dir().join(format!("oxbow-slot-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
                .args(args)
                .current_dir(&dir)
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), name).unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", name]);
        Repo::open(&dir).unwrap()
    }

    /// What the `history` command does in a window: take the repository and the cache, read
    /// (`meanwhile` runs while the read is under way), then keep what it found. Its files are
    /// kept under the read's number.
    fn read_history(slot: &mut Slot, meanwhile: impl FnOnce(&mut Slot)) {
        let (repo, number) = slot.start_history();
        let mut cache = slot.history.clone();
        let history = repo
            .unwrap()
            .history_with(&HistoryOptions::default(), &mut cache, None)
            .unwrap();
        meanwhile(slot);
        let changes = history.changes.clone().map(|tree| (number, Arc::new(tree)));
        let folders = Arc::new((history.worktrees.clone(), history.submodules.clone()));
        slot.keep_history(number, cache, changes, folders);
    }

    #[test]
    fn each_run_has_a_stop_of_its_own() {
        let stops = Arc::new(Stops::default());
        let main = stops.start("main");
        // Settings acts for the main window, but its Stop is its own.
        let settings = stops.start("settings");
        stops.stop("settings");
        assert!(settings.cancel.load(Ordering::Relaxed));
        assert!(
            !main.cancel.load(Ordering::Relaxed),
            "Stop in Settings stopped the main window's action"
        );
        // A run started after Stop doesn't take it back from the one stopped.
        let later = stops.start("settings");
        assert!(settings.cancel.load(Ordering::Relaxed));
        assert!(!later.cancel.load(Ordering::Relaxed));
        drop((main, settings, later));
        assert!(stops.running.lock().unwrap().is_empty());
    }

    #[test]
    fn a_late_history_of_the_repository_open_before_keeps_nothing() {
        let (a, b) = (repo("late-a"), repo("late-b"));
        let mut slot = Slot::default();
        slot.open(a.clone());
        // The window opens B while the history of A is still being read.
        read_history(&mut slot, |slot| slot.open(b.clone()));
        assert!(slot.history.is_empty(), "the rows of A are kept for B's next reload");
        assert!(slot.changes.is_none(), "the files of A are kept for B's Changes panel");
        assert!(
            slot.folders.0.is_empty(),
            "B lists the folders of A: {:?}",
            slot.folders.0
        );
        // B's own reading is kept as usual.
        read_history(&mut slot, |_| {});
        assert!(!slot.history.is_empty());
        assert_eq!(
            Path::new(&slot.folders.0[0].path).canonicalize().unwrap(),
            b.workdir().canonicalize().unwrap()
        );
        for repo in [a, b] {
            std::fs::remove_dir_all(repo.workdir()).unwrap();
        }
    }

    #[test]
    fn an_older_history_finishing_last_leaves_the_newer() {
        let a = repo("older");
        let mut slot = Slot::default();
        slot.open(a.clone());
        let mut newer = 0;
        // A reload starts while the one before it is still reading, and ends first.
        read_history(&mut slot, |slot| {
            read_history(slot, |_| {});
            newer = slot.changes.as_ref().unwrap().0;
        });
        assert_eq!(slot.changes.as_ref().unwrap().0, newer);
        // The window opens the same repository again: a read started before keeps nothing.
        read_history(&mut slot, |slot| slot.open(a.clone()));
        assert!(slot.history.is_empty() && slot.changes.is_none());
        std::fs::remove_dir_all(a.workdir()).unwrap();
    }
    #[test]
    fn closing_forgets_the_repository_and_rejects_late_history() {
        let repo = repo("closed");
        let mut slot = Slot::default();
        slot.open(repo.clone());
        let fetch = slot.fetch_cancel.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        slot.lanes.insert("search".into(), cancel.clone());
        read_history(&mut slot, |slot| slot.close());
        assert!(slot.repo.is_none() && slot.watch.is_none());
        assert!(slot.history.is_empty() && slot.changes.is_none());
        assert!(slot.folders.0.is_empty() && slot.folders.1.is_empty());
        assert!(fetch.load(Ordering::Relaxed) && cancel.load(Ordering::Relaxed));
        assert!(expected(repo.clone(), Some("/another/repository")).is_err());
        assert!(expected(repo.clone(), None).is_err());
        assert!(expected(repo.clone(), Some(&repo.workdir().to_string_lossy())).is_ok());
        std::fs::remove_dir_all(repo.workdir()).unwrap();
    }
}
