//! Repository sessions, their Stop buttons and the helpers every
//! command uses to run Git work off the main thread.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use oxbow_core::console;
use oxbow_core::{
    CommitDetail, DiffOptions, DiffPage, FileDiff, FolderCache, HistoryCache, Repo, Submodule, WorkingTree, Worktree,
};
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, CommandArg, CommandItem, InvokeBody, InvokeError};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

use crate::{CommandResult, turns};

/// Shared app settings and the repository sessions owned by each window.
#[derive(Default)]
pub(crate) struct Session {
    pub(crate) repositories: Mutex<Repositories>,
    pub(crate) opening: tokio::sync::Mutex<()>,
    pub(crate) repo_topology: tokio::sync::RwLock<()>,
    pub(crate) settings_writes: tokio::sync::Mutex<()>,
    /// Diff settings, given to the repository on every call.
    pub(crate) diff: Mutex<DiffOptions>,
    /// The repository window used last: the one Settings shows and changes.
    pub(crate) focused: Mutex<Option<String>>,
    /// Number of the next new window.
    pub(crate) next: AtomicUsize,
    /// Number of the last reading of the working tree handed out with a history.
    pub(crate) snapshots: AtomicU64,
    /// Number of the last change to settings.json. Held while settings.json is read and written,
    /// so two saves at once can't lose each other's key, and sent with every change, so each
    /// window takes the changes in the order they were saved.
    pub(crate) settings: Mutex<u64>,
    /// Changes running in each repository, one at a time.
    pub(crate) turns: Arc<turns::Turns>,
    /// The Stop of each run going now.
    pub(crate) stops: Arc<Stops>,
}

/// The Stop of each action, clone and GitHub or GitLab call running now.
#[derive(Default)]
pub(crate) struct Stops {
    running: Mutex<HashMap<StopScope, Vec<Arc<AtomicBool>>>>,
    paused: AtomicBool,
}

#[derive(Clone, Hash, PartialEq, Eq)]
struct StopScope {
    window: String,
    session: Option<u64>,
    change: Option<RepoChange>,
    worktree: Option<PathBuf>,
    removing: Option<PathBuf>,
}

#[derive(Clone, Hash, PartialEq, Eq)]
enum RepoChange {
    Draft,
    Git(PathBuf),
}

/// A run's own Stop, heard by its sheet's Stop button until it is dropped.
pub(crate) struct RunStop {
    stops: Arc<Stops>,
    scope: StopScope,
    pub(crate) cancel: Arc<AtomicBool>,
}

impl Stops {
    pub(crate) fn pause_if_idle(&self) -> bool {
        let running = self.running.lock().expect("stops lock");
        if !running.is_empty() {
            return false;
        }
        self.paused.store(true, Ordering::SeqCst);
        true
    }

    pub(crate) fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    /// A Stop of its own for a run starting now in `window`: a run after it never clears it,
    /// and Stop in another window doesn't reach it.
    pub(crate) fn start(self: &Arc<Self>, window: &str) -> RunStop {
        self.start_for(window, None, true)
    }

    fn start_for(self: &Arc<Self>, window: &str, session: Option<u64>, live: bool) -> RunStop {
        self.start_scoped(
            StopScope {
                window: window.to_owned(),
                session,
                change: None,
                worktree: None,
                removing: None,
            },
            live,
        )
    }

    fn start_scoped(self: &Arc<Self>, scope: StopScope, live: bool) -> RunStop {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut running = self.running.lock().expect("stops lock");
        if !live || self.paused.load(Ordering::SeqCst) {
            cancel.store(true, Ordering::Relaxed);
        } else {
            running.entry(scope.clone()).or_default().push(cancel.clone());
        }
        RunStop {
            stops: self.clone(),
            scope,
            cancel,
        }
    }

    /// Stop what `window` runs.
    pub(crate) fn stop(&self, window: &str) {
        self.stop_where(|scope| scope.window == window);
    }

    fn stop_where(&self, matches: impl Fn(&StopScope) -> bool) {
        let running = self.running.lock().expect("stops lock");
        for (_, runs) in running.iter().filter(|(scope, _)| matches(scope)) {
            for cancel in runs {
                cancel.store(true, Ordering::Relaxed);
            }
        }
    }

    fn check_worktree(&self, path: &Path, removing: bool) -> CommandResult<()> {
        let running = self.running.lock().expect("stops lock");
        if running.keys().any(|scope| scope.removing.as_deref() == Some(path)) {
            return Err("Oxbow is removing this worktree. Try again when it finishes.".into());
        }
        if removing && let Some(scope) = running.keys().find(|scope| scope.worktree.as_deref() == Some(path)) {
            let reason = if scope.change == Some(RepoChange::Draft) {
                "a draft is still being saved"
            } else {
                "a Git command is running or waiting"
            };
            return Err(format!("Oxbow can’t remove {} while {reason}.", path.display()));
        }
        Ok(())
    }
}

impl Drop for RunStop {
    fn drop(&mut self) {
        let mut running = self.stops.running.lock().expect("stops lock");
        if let Some(list) = running.get_mut(&self.scope) {
            list.retain(|cancel| !Arc::ptr_eq(cancel, &self.cancel));
            if list.is_empty() {
                running.remove(&self.scope);
            }
        }
    }
}

#[derive(Default)]
pub(crate) struct Slot {
    pub(crate) repo: Option<Repo>,
    pub(crate) commits: CommitCache,
    /// The canonical worktree and common Git directories serve different purposes.
    workdir: PathBuf,
    common: PathBuf,
    owner: String,
    legacy: bool,
    published: bool,
    /// Stop of the fetch running in the background, apart from actions in the sheet.
    pub(crate) fetch_cancel: Arc<AtomicBool>,
    pub(crate) close_cancel: Arc<AtomicBool>,
    fetch_id: u64,
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
    /// The latest Files fetch in this session, even if its IPC call has already finished.
    files_fetch_id: u64,
    /// Tells the window what changes in its repository outside it; stops when dropped.
    pub(crate) watch: Option<oxbow_core::Watch>,
}

impl Slot {
    pub(crate) fn start_fetch(&mut self, id: u64) -> Option<Arc<AtomicBool>> {
        if id <= self.fetch_id {
            return None;
        }
        self.stop_fetch(id);
        self.fetch_cancel = Arc::default();
        Some(self.fetch_cancel.clone())
    }

    pub(crate) fn fetch_is_current(&self, id: Option<u64>) -> bool {
        id.is_none_or(|id| id == self.fetch_id)
    }

    /// Stop can arrive before this fetch starts, or after a newer fetch has replaced it.
    pub(crate) fn stop_fetch(&mut self, id: u64) {
        if id >= self.fetch_id {
            self.fetch_id = id;
            self.fetch_cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Open `repo` in this window: what was read of the repository open before goes.
    pub(crate) fn open(&mut self, repo: Repo) {
        self.close();
        self.close_cancel = Arc::default();
        self.repo = Some(repo);
    }

    /// Forget the repository, including late reads and its background fetch.
    pub(crate) fn close(&mut self) {
        self.cancel_reads();

        self.repo = None;
        self.history = HistoryCache::default();
        self.changes = None;
        self.folders = Arc::default();
        self.folder_cache = FolderCache::default();
        self.commits = CommitCache::default();
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

    pub(crate) fn keep_repo(&mut self, number: u64, fresh: &Repo) {
        if number == self.history_reads {
            self.repo = Some(fresh.clone());
        }
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
    ) -> bool {
        if number <= self.history_kept {
            return false;
        }
        self.history_kept = number;
        self.history = cache;
        self.changes = changes;
        self.folders = folders;
        true
    }

    /// Stop every read running for this window: its repository changed, or it closed.
    fn cancel_reads(&mut self) {
        self.stop_reads();
        self.fetch_cancel.store(true, Ordering::Relaxed);
        self.close_cancel.store(true, Ordering::Relaxed);
    }

    pub(crate) fn stop_reads(&mut self) {
        for (_, cancel) in self.lanes.drain() {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    pub(crate) fn lane(&mut self, lane: String) -> Arc<AtomicBool> {
        let cancel = Arc::new(AtomicBool::new(false));
        self.replace_lane(lane, &cancel);
        cancel
    }

    fn replace_lane(&mut self, lane: String, cancel: &Arc<AtomicBool>) {
        if let Some(older) = self.lanes.insert(lane, cancel.clone()) {
            older.store(true, Ordering::Relaxed);
        }
    }

    fn start_files_fetch(&mut self, id: u64, stop: &RunStop) -> bool {
        if id <= self.files_fetch_id {
            return false;
        }
        self.files_fetch_id = id;
        self.replace_lane("review-files-fetch".into(), &stop.cancel);
        true
    }
}

/// What [`Slot::commits`] holds.
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

pub(crate) struct Repositories {
    windows: HashMap<String, RepoWindow>,
    slots: HashMap<u64, Slot>,
    next: u64,
    pub(crate) tabs_disabled: bool,
    pub(crate) tab_transition: Option<crate::tab_settings::Preparation>,
}

impl Default for Repositories {
    fn default() -> Self {
        Self {
            windows: HashMap::from([("main".into(), RepoWindow::default())]),
            slots: HashMap::new(),
            next: 0,
            tabs_disabled: false,
            tab_transition: None,
        }
    }
}

#[derive(Default)]
struct RepoWindow {
    active: Option<u64>,
    notices: Option<Channel<RepoNotice>>,
    pending: Option<(PathBuf, PathBuf)>,
    // A legacy open replaces the active session. A later open or close supersedes it.
    replacing: u64,
}

#[derive(Serialize)]
pub(crate) struct RepoNotice {
    session: u64,
    event: &'static str,
    payload: serde_json::Value,
}

impl RepoNotice {
    fn send(channel: &Channel<Self>, address: &Address, event: &'static str, payload: impl Serialize) {
        if let Ok(payload) = serde_json::to_value(payload) {
            let _ = channel.send(Self {
                session: address.id,
                event,
                payload,
            });
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Address {
    pub(crate) window: String,
    pub(crate) id: u64,
}

impl Address {
    pub(crate) fn turn_key(&self) -> String {
        self.id.to_string()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Located {
    Open(Address),
    Pending(String),
}

impl Located {
    pub(crate) fn window(&self) -> &str {
        match self {
            Self::Open(address) => &address.window,
            Self::Pending(window) => window,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum WindowTarget {
    Existing(Located),
    New(String),
}

pub(crate) enum Opened {
    New(Address),
    Existing(Located),
}

/// Keep the working-folder key usable after Git removes the last path component.
pub(crate) fn workdir_key(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if let Ok(canonical) = ancestor.canonicalize() {
            let tail = path.strip_prefix(ancestor).expect("ancestor");
            return if tail.as_os_str().is_empty() {
                canonical
            } else {
                canonical.join(tail)
            };
        }
    }
    path.to_path_buf()
}

impl Repositories {
    fn address(&self, id: u64) -> Option<Address> {
        self.slots.get(&id).map(|slot| Address {
            window: slot.owner.clone(),
            id,
        })
    }

    pub(crate) fn active(&self, window: &str) -> Option<Address> {
        self.address(self.windows.get(window)?.active?)
    }

    fn resolve(&self, owner: &str, id: Option<u64>) -> CommandResult<Option<Address>> {
        match id {
            Some(id) => self
                .address(id)
                .filter(|address| address.window == owner)
                .map(Some)
                .ok_or_else(|| "That repository session is closed or belongs to another window.".into()),
            None => Ok(self.active(owner)),
        }
    }

    pub(crate) fn repo(&self, address: &Address) -> Option<&Repo> {
        self.slots.get(&address.id)?.repo.as_ref()
    }

    pub(crate) fn open_paths(&self) -> impl Iterator<Item = &Path> {
        self.slots
            .values()
            .filter_map(|slot| slot.repo.as_ref().map(Repo::workdir))
    }

    pub(crate) fn pending(&self, window: &str) -> Option<&Path> {
        self.windows
            .get(window)?
            .pending
            .as_ref()
            .map(|(path, _)| path.as_path())
    }

    pub(crate) fn locate(&self, canonical: &Path, preferred: Option<&str>) -> Option<Located> {
        let preferred_first = |label: &str| Some(label) != preferred;
        if let Some((&id, slot)) = self
            .slots
            .iter()
            .filter(|(_, slot)| slot.workdir == canonical)
            .min_by_key(|(id, slot)| (preferred_first(&slot.owner), !slot.published, **id))
        {
            return Some(Located::Open(Address {
                window: slot.owner.clone(),
                id,
            }));
        }
        self.windows.iter().find_map(|(label, window)| {
            window
                .pending
                .as_ref()
                .filter(|(_, key)| key == canonical)
                .map(|_| Located::Pending(label.clone()))
        })
    }

    /// Legacy Open Recent replaces this window's repository even when another window has the
    /// same folder. Tab opens share the deduplication used by Open in New Window.
    pub(crate) fn opening_target(&self, canonical: &Path, label: &str, legacy: bool) -> Option<Located> {
        self.locate(canonical, Some(label)).filter(|target| match target {
            Located::Open(address) => !legacy || address.window == label,
            Located::Pending(owner) => !legacy && owner != label,
        })
    }

    pub(crate) fn siblings(&self, common: &Path, except: &Address) -> Vec<Address> {
        self.slots
            .iter()
            .filter(|(id, slot)| **id != except.id && slot.common == common)
            .map(|(&id, slot)| Address {
                window: slot.owner.clone(),
                id,
            })
            .collect()
    }

    pub(crate) fn retiring_tabs(&self) -> HashMap<String, Vec<u64>> {
        let mut closing = HashMap::<String, Vec<u64>>::new();
        for (&id, slot) in &self.slots {
            if self.windows[&slot.owner].active != Some(id) {
                closing.entry(slot.owner.clone()).or_default().push(id);
            }
        }
        for ids in closing.values_mut() {
            ids.sort_unstable();
        }
        closing
    }

    pub(crate) fn window_starting(&self) -> bool {
        self.slots
            .values()
            .any(|slot| !slot.published && self.windows[&slot.owner].active.is_none())
    }

    pub(crate) fn contains_window(&self, label: &str) -> bool {
        self.windows.contains_key(label)
    }

    pub(crate) fn common(&self, id: u64) -> Option<&Path> {
        self.slots.get(&id).map(|slot| slot.common.as_path())
    }

    pub(crate) fn set_tabs_disabled(&mut self, disabled: bool) {
        self.tabs_disabled = disabled;
        for slot in self.slots.values_mut() {
            slot.legacy = disabled;
        }
    }

    fn next_id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

/// Resolved at IPC entry. No command looks up the active session again after an await.
#[derive(Clone)]
pub(crate) struct RepoRequest {
    window: WebviewWindow,
    pub(crate) address: Option<Address>,
    pub(crate) explicit: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Target {
    repo_session: Option<u64>,
}

impl<'a> CommandArg<'a, tauri::Wry> for RepoRequest {
    fn from_command(command: CommandItem<'a, tauri::Wry>) -> Result<Self, InvokeError> {
        let id = match command.message.payload() {
            InvokeBody::Json(value) if value.get("repoSession").is_some() => {
                Target::deserialize(value).map_err(|err| err.to_string())?.repo_session
            }
            _ => None,
        };
        let window = WebviewWindow::from_command(command)?;
        let address = window.state::<Session>().resolve(window.label(), id)?;
        Ok(Self {
            window,
            address,
            explicit: id.is_some(),
        })
    }
}

impl std::ops::Deref for RepoRequest {
    type Target = WebviewWindow;

    fn deref(&self) -> &WebviewWindow {
        &self.window
    }
}

impl RepoRequest {
    pub(crate) fn turn_key(&self) -> String {
        self.address
            .as_ref()
            .map_or_else(|| self.label().to_owned(), Address::turn_key)
    }

    pub(crate) fn output_owner(&self) -> String {
        match self.address.as_ref().filter(|_| self.explicit) {
            Some(address) => format!("{}-{}", self.label(), address.id),
            None => self.label().to_owned(),
        }
    }

    /// Routes with a path in their payload still validate it against an explicit session.
    pub(crate) fn check_workdir(&self, workdir: &str) -> CommandResult<()> {
        if self.explicit {
            expected(current(&self.state::<Session>(), self)?, Some(workdir))?;
        }
        Ok(())
    }

    /// Progress from Settings stays in Settings, even when another repository is focused.
    pub(crate) fn progress(&self, event: &'static str, payload: impl Serialize + Clone) {
        let session = self.state::<Session>();
        if let Some(address) = &self.address {
            if self.label() == address.window {
                session.emit_repo(self.app_handle(), address, event, payload, !self.explicit);
            } else if session.live(address) {
                let _ = self.emit_to(self.label(), event, payload);
            }
        } else {
            let _ = self.emit_to(self.label(), event, payload);
        }
    }

    /// A closed session's late reply is obsolete. The caller of an explicit request owns the
    /// reply even if the session became inactive while it was in flight.
    pub(crate) fn ensure_open(&self) -> CommandResult<()> {
        if self
            .address
            .as_ref()
            .is_some_and(|address| self.state::<Session>().live(address))
        {
            Ok(())
        } else {
            Err(oxbow_core::Error::Cancelled.to_string())
        }
    }
}

impl Session {
    pub(crate) fn resolve(&self, caller: &str, id: Option<u64>) -> CommandResult<Option<Address>> {
        // Settings may use the focused repository implicitly. An explicit ID is owner-only.
        let owner = if caller == "settings" && id.is_none() {
            self.focused
                .lock()
                .expect("session lock")
                .clone()
                .unwrap_or_else(|| "main".into())
        } else {
            caller.to_owned()
        };
        self.repositories.lock().expect("session lock").resolve(&owner, id)
    }

    pub(crate) fn slot<T>(&self, request: &RepoRequest, f: impl FnOnce(&mut Slot) -> T) -> Option<T> {
        self.with_slot(request.address.as_ref()?, f)
    }

    pub(crate) fn with_slot<T>(&self, address: &Address, f: impl FnOnce(&mut Slot) -> T) -> Option<T> {
        self.repositories
            .lock()
            .expect("session lock")
            .slots
            .get_mut(&address.id)
            .map(f)
    }

    pub(crate) fn live(&self, address: &Address) -> bool {
        self.repositories
            .lock()
            .expect("session lock")
            .slots
            .contains_key(&address.id)
    }

    pub(crate) fn emit_repo(
        &self,
        app: &AppHandle,
        address: &Address,
        name: &'static str,
        payload: impl Serialize + Clone,
        unscoped: bool,
    ) {
        let (channel, legacy) = {
            let repositories = self.repositories.lock().expect("session lock");
            let Some(slot) = repositories.slots.get(&address.id) else {
                return;
            };
            let owner = &repositories.windows[&slot.owner];
            (
                owner.notices.clone(),
                unscoped && slot.legacy && owner.active == Some(address.id),
            )
        };
        if let Some(channel) = channel {
            RepoNotice::send(&channel, address, name, &payload);
        }
        if legacy {
            let _ = app.emit_to(address.window.as_str(), name, payload);
        }
    }

    fn set_notices(&self, label: &str, notices: Channel<RepoNotice>) -> CommandResult<()> {
        let mut repositories = self.repositories.lock().expect("session lock");
        let owner = repositories
            .windows
            .get_mut(label)
            .ok_or("A repository window is required.")?;
        let previous = owner.notices.replace(notices);
        // The channel's drop can call into its WebView.
        drop(repositories);
        drop(previous);
        Ok(())
    }

    pub(crate) fn emit_closed(&self, address: &Address) {
        let channel = self
            .repositories
            .lock()
            .expect("session lock")
            .windows
            .get(&address.window)
            .and_then(|window| window.notices.clone());
        if let Some(channel) = channel {
            RepoNotice::send(&channel, address, "repo-closed", ());
        }
    }

    pub(crate) fn new_stop(&self, request: &RepoRequest) -> RunStop {
        self.new_stop_in(request, request.label())
    }

    pub(crate) fn new_stop_in(&self, request: &RepoRequest, sheet: &str) -> RunStop {
        let repositories = self.repositories.lock().expect("session lock");
        let id = request.address.as_ref().map(|address| address.id);
        let live = id.is_none_or(|id| repositories.slots.contains_key(&id));
        self.stops.start_for(sheet, id, live)
    }

    /// Register before dispatch to a worker, including the time a write spends waiting for Git.
    pub(crate) fn start_change(&self, request: &RepoRequest, sheet: &str, draft: bool) -> CommandResult<RunStop> {
        let address = request.address.as_ref().ok_or("No repository is open")?;
        self.start_repo_write(address, sheet, Some(draft), None)
    }

    pub(crate) fn start_worktree_remove(&self, address: &Address, sheet: &str, path: &Path) -> CommandResult<RunStop> {
        self.start_repo_write(address, sheet, Some(false), Some(path))
    }

    /// Fetches can be stopped on a normal tab close, but cannot race worktree removal.
    pub(crate) fn start_worktree_git(&self, request: &RepoRequest) -> CommandResult<RunStop> {
        let address = request.address.as_ref().ok_or("No repository is open")?;
        self.start_repo_write(address, request.label(), None, None)
    }

    fn start_repo_write(
        &self,
        address: &Address,
        sheet: &str,
        draft: Option<bool>,
        removing: Option<&Path>,
    ) -> CommandResult<RunStop> {
        let repositories = self.repositories.lock().expect("session lock");
        let slot = repositories
            .slots
            .get(&address.id)
            .filter(|slot| slot.published)
            .ok_or_else(|| oxbow_core::Error::Cancelled.to_string())?;
        let common = slot.common.as_path();
        if let Some(draft) = draft
            && repositories.tab_transition.as_ref().is_some_and(|transition| {
                if draft {
                    transition.drafts_sealed(&address.window, address.id)
                } else {
                    transition
                        .closing
                        .values()
                        .flatten()
                        .any(|&id| repositories.common(id) == Some(common))
                }
            })
        {
            return Err("Oxbow is closing tabs. Try again when it finishes.".into());
        }
        // All worktree writes and removal reservations register under the repository lock.
        self.stops.check_worktree(&slot.workdir, false)?;
        if let Some(path) = removing {
            self.stops.check_worktree(path, true)?;
        }
        let stop = self.stops.start_scoped(
            StopScope {
                window: sheet.to_owned(),
                session: Some(address.id),
                change: draft.map(|draft| {
                    if draft {
                        RepoChange::Draft
                    } else {
                        RepoChange::Git(common.to_owned())
                    }
                }),
                worktree: Some(slot.workdir.clone()),
                removing: removing.map(Path::to_path_buf),
            },
            true,
        );
        if stop.cancel.load(Ordering::Relaxed) {
            return Err(oxbow_core::Error::Cancelled.to_string());
        }
        Ok(stop)
    }

    pub(crate) fn start_worktree_write_at(
        &self,
        request: &RepoRequest,
        workdir: &Path,
        draft: Option<bool>,
    ) -> CommandResult<RunStop> {
        let path = workdir_key(workdir);
        if let Some(address) = &request.address
            && (request.explicit || self.with_slot(address, |slot| slot.workdir == path) == Some(true))
        {
            return self.start_repo_write(address, request.label(), draft, None);
        }
        // Commands with their own path can arrive after a legacy window switched repositories.
        let _repositories = self.repositories.lock().expect("session lock");
        self.stops.check_worktree(&path, false)?;
        if !path.is_dir() {
            return Err(oxbow_core::Error::Cancelled.to_string());
        }
        let stop = self.stops.start_scoped(
            StopScope {
                window: request.label().to_owned(),
                session: None,
                // With no associated session only worktree removal can inspect this draft.
                change: (draft == Some(true)).then_some(RepoChange::Draft),
                worktree: Some(path),
                removing: None,
            },
            true,
        );
        if stop.cancel.load(Ordering::Relaxed) {
            return Err(oxbow_core::Error::Cancelled.to_string());
        }
        Ok(stop)
    }

    /// Called with the repository lock held so a write cannot start between this check and detach.
    pub(crate) fn check_tab_writes(
        &self,
        repositories: &Repositories,
        ids: &[u64],
        include_drafts: bool,
    ) -> CommandResult<()> {
        let running = self.stops.running.lock().expect("stops lock");
        for id in ids {
            let slot = &repositories.slots[id];
            if let Some(draft) = running.keys().find_map(|scope| match scope.change.as_ref()? {
                RepoChange::Draft if include_drafts && scope.session == Some(*id) => Some(true),
                RepoChange::Git(common) if *common == slot.common => Some(false),
                _ => None,
            }) {
                let name = slot
                    .repo
                    .as_ref()
                    .map_or_else(|| slot.workdir.display().to_string(), Repo::name);
                let reason = if draft {
                    "a draft is still being saved"
                } else {
                    "a Git command is running or waiting"
                };
                return Err(format!("Oxbow can’t close {name} while {reason}."));
            }
        }
        Ok(())
    }

    pub(crate) fn topology(&self) -> CommandResult<tokio::sync::RwLockReadGuard<'_, ()>> {
        self.repo_topology
            .try_read()
            .map_err(|_| "Oxbow is changing how repository tabs are shown. Try again when it finishes.".into())
    }

    /// Commit a batch and its next active tab under one lock.
    pub(crate) fn close_tabs(
        &self,
        label: &str,
        ids: &[u64],
        active: Option<u64>,
        previous: Option<u64>,
    ) -> CommandResult<Option<PathBuf>> {
        let _topology = self.topology()?;
        let mut repositories = self.repositories.lock().expect("session lock");
        let owner = |id| repositories.slots.get(&id).is_some_and(|slot| slot.owner == label);
        if !repositories.contains_window(label)
            || ids.iter().any(|&id| !owner(id))
            || active.is_some_and(|id| !owner(id) || ids.contains(&id))
            || repositories.windows[label].active != previous
            || (!repositories.tabs_disabled && active.is_some_and(|id| !repositories.slots[&id].published))
            || (previous.is_some_and(|id| !ids.contains(&id)) && active.is_none())
        {
            return Err("The open repositories changed. Try again.".into());
        }
        self.check_tab_writes(&repositories, ids, true)?;
        let path = active.map(|id| {
            let slot = repositories.slots.get_mut(&id).expect("active tab");
            slot.published = true;
            slot.workdir.clone()
        });
        let revision = repositories.next_id();
        let window = repositories.windows.get_mut(label).expect("owner window");
        window.active = active;
        window.replacing = revision;
        let removed = self.detach_tabs(&mut repositories, ids);
        drop(repositories);
        drop(removed);
        Ok(path)
    }

    pub(crate) fn commit_tab_mode(
        &self,
        enabled: bool,
        closing: &HashMap<String, Vec<u64>>,
        ids: &[u64],
        write: impl FnOnce() -> CommandResult<()>,
    ) -> CommandResult<()> {
        let mut repositories = self.repositories.lock().expect("session lock");
        if !enabled && *closing != repositories.retiring_tabs() {
            return Err("The open repositories changed. Try again.".into());
        }
        self.check_tab_writes(&repositories, ids, true)?;
        write()?;
        let removed = self.detach_tabs(&mut repositories, ids);
        repositories.set_tabs_disabled(!enabled);
        drop(repositories);
        drop(removed);
        Ok(())
    }

    fn detach_tabs(&self, repositories: &mut Repositories, ids: &[u64]) -> Vec<Slot> {
        ids.iter()
            .filter_map(|&id| self.detach(repositories, id))
            .map(|mut slot| {
                // Cancel while the address is still locked against new requests.
                slot.cancel_reads();
                slot
            })
            .collect()
    }

    pub(crate) fn new_files_fetch(&self, request: &RepoRequest, id: Option<u64>) -> CommandResult<Option<RunStop>> {
        let Some(id) = id else { return Ok(None) };
        let stop = self.start_worktree_git(request)?;
        if self.slot(request, |slot| slot.start_files_fetch(id, &stop)) != Some(true) {
            return Err(oxbow_core::Error::Cancelled.to_string());
        }
        Ok(Some(stop))
    }

    pub(crate) fn lane(&self, request: &RepoRequest, lane: Option<String>) -> Arc<AtomicBool> {
        self.slot(request, |slot| match lane {
            Some(lane) => slot.lane(lane),
            None => slot.close_cancel.clone(),
        })
        .unwrap_or_else(|| Arc::new(AtomicBool::new(true)))
    }

    pub(crate) fn remove_window(&self, label: &str) {
        let (window, removed) = {
            let mut repositories = self.repositories.lock().expect("session lock");
            let window = repositories.windows.remove(label);
            if let Some(preparation) = &repositories.tab_transition {
                preparation.window_removed(label);
            }
            self.stops.stop(label);
            let removed = repositories
                .slots
                .extract_if(|_, slot| slot.owner == label)
                .collect::<Vec<_>>();
            (window, removed)
        };
        drop(window);
        for (id, mut slot) in removed {
            self.stops.stop_where(|scope| scope.session == Some(id));
            slot.close();
        }
    }

    /// Reserve the canonical working directory under the same lock used to publish tabs.
    pub(crate) fn reserve_window(&self, path: &Path) -> CommandResult<WindowTarget> {
        let canonical = workdir_key(path);
        let mut repositories = self.repositories.lock().expect("session lock");
        self.stops.check_worktree(&canonical, false)?;
        if !canonical.is_dir() {
            return Err(oxbow_core::Error::Cancelled.to_string());
        }
        if let Some(target) = repositories.locate(&canonical, None) {
            return Ok(WindowTarget::Existing(target));
        }
        let label = format!("repo-{}", self.next.fetch_add(1, Ordering::Relaxed) + 1);
        repositories.windows.insert(
            label.clone(),
            RepoWindow {
                pending: Some((path.to_path_buf(), canonical)),
                ..RepoWindow::default()
            },
        );
        Ok(WindowTarget::New(label))
    }

    /// Register a live window's open before any disk work. Only legacy opens supersede one another.
    pub(crate) fn begin_open(&self, label: &str, legacy: bool) -> CommandResult<Option<u64>> {
        let mut repositories = self.repositories.lock().expect("session lock");
        let number = legacy.then(|| repositories.next_id());
        let window = repositories
            .windows
            .get_mut(label)
            .ok_or_else(|| oxbow_core::Error::Cancelled.to_string())?;
        if let Some(number) = number {
            window.replacing = number;
        }
        Ok(number)
    }

    pub(crate) fn check_open(repositories: &Repositories, label: &str, replacing: Option<u64>) -> CommandResult<()> {
        if repositories
            .windows
            .get(label)
            .is_some_and(|window| replacing.is_none_or(|number| number == window.replacing))
        {
            Ok(())
        } else {
            Err(oxbow_core::Error::Cancelled.to_string())
        }
    }

    pub(crate) fn finish_open(&self, label: &str, replacing: Option<u64>, repo: Repo) -> CommandResult<Opened> {
        let workdir = workdir_key(repo.workdir());
        let common = turns::canonical(&repo.common_dir());
        let mut repositories = self.repositories.lock().expect("session lock");
        Self::check_open(&repositories, label, replacing)?;
        self.stops.check_worktree(&workdir, false)?;
        if !workdir.is_dir() {
            return Err(oxbow_core::Error::Cancelled.to_string());
        }
        let legacy = replacing.is_some();
        let single = legacy || repositories.tabs_disabled;
        if let Some(target) = repositories.opening_target(&workdir, label, single) {
            return Ok(Opened::Existing(target));
        }
        let claims_pending = repositories.windows[label]
            .pending
            .as_ref()
            .is_some_and(|(_, key)| *key == workdir);
        if legacy && let Some(active) = repositories.windows[label].active {
            self.check_tab_writes(&repositories, &[active], true)?;
        }
        let id = repositories.next_id();
        let mut slot = Slot {
            workdir,
            common,
            owner: label.to_owned(),
            legacy: single,
            published: legacy,
            ..Slot::default()
        };
        slot.open(repo);
        repositories.slots.insert(id, slot);
        let window = repositories.windows.get_mut(label).expect("open window");
        if legacy || claims_pending {
            window.pending = None;
        }
        let replaced = if legacy { window.active.replace(id) } else { None };
        let old = replaced.and_then(|id| self.detach(&mut repositories, id));
        drop(repositories);
        if let Some(mut old) = old {
            old.close();
        }
        Ok(Opened::New(Address {
            window: label.to_owned(),
            id,
        }))
    }

    pub(crate) fn publish_tab(&self, address: &Address) -> CommandResult<()> {
        let mut repositories = self.repositories.lock().expect("session lock");
        let slot = repositories
            .slots
            .get_mut(&address.id)
            .filter(|slot| slot.owner == address.window && slot.repo.is_some())
            .ok_or_else(|| oxbow_core::Error::Cancelled.to_string())?;
        slot.published = true;
        Ok(())
    }

    pub(crate) fn activate(&self, address: &Address, replacing: Option<u64>) -> CommandResult<()> {
        let mut repositories = self.repositories.lock().expect("session lock");
        Self::check_open(&repositories, &address.window, replacing)?;
        if repositories.tabs_disabled
            && replacing.is_none()
            && repositories.windows[&address.window]
                .active
                .is_some_and(|id| id != address.id)
        {
            return Err("Replace the active repository before activating another.".into());
        }
        if replacing.is_some()
            && let Some(active) = repositories.windows[&address.window].active
            && active != address.id
        {
            self.check_tab_writes(&repositories, &[active], true)?;
        }
        let slot = repositories
            .slots
            .get_mut(&address.id)
            .ok_or_else(|| oxbow_core::Error::Cancelled.to_string())?;
        let legacy = replacing.is_some();
        slot.legacy |= legacy;
        slot.published = true;
        if repositories.windows[&address.window].active == Some(address.id) {
            return Ok(());
        }
        let revision = repositories.next_id();
        let owner = repositories.windows.get_mut(&address.window).expect("owner window");
        owner.replacing = revision;
        let previous = owner.active.replace(address.id);
        let old = previous
            .filter(|_| legacy)
            .and_then(|id| self.detach(&mut repositories, id));
        drop(repositories);
        if let Some(mut old) = old {
            old.close();
        }
        Ok(())
    }

    fn detach(&self, repositories: &mut Repositories, id: u64) -> Option<Slot> {
        let slot = repositories.slots.remove(&id)?;
        self.stops.stop_where(|scope| scope.session == Some(id));
        Some(slot)
    }

    pub(crate) fn close_one(&self, address: &Address, legacy: bool, provisional: bool) -> CommandResult<()> {
        let mut repositories = self.repositories.lock().expect("session lock");
        if repositories.repo(address).is_none() || (provisional && repositories.slots[&address.id].published) {
            return Ok(());
        }
        if !provisional {
            self.check_tab_writes(&repositories, &[address.id], true)?;
        }
        let revision = repositories.next_id();
        let window = repositories.windows.get_mut(&address.window).expect("owner window");
        if window.active == Some(address.id) || legacy {
            window.replacing = revision;
        }
        if window.active == Some(address.id) {
            window.active = None;
        }
        let removed = self.detach_tabs(&mut repositories, &[address.id]);
        drop(repositories);
        drop(removed);
        Ok(())
    }

    /// Closing the active tab clears the backend's active address. The UI picks its successor.
    #[cfg(test)]
    fn close(&self, label: &str, address: Option<&Address>, legacy: bool) {
        let removed = {
            let mut repositories = self.repositories.lock().expect("session lock");
            let closing_active = address.is_some_and(|address| {
                repositories
                    .windows
                    .get(label)
                    .is_some_and(|window| window.active == Some(address.id))
            });
            if legacy || closing_active {
                let number = repositories.next_id();
                if let Some(window) = repositories.windows.get_mut(label) {
                    window.replacing = number;
                }
            }
            address.and_then(|address| {
                let window = repositories.windows.get_mut(&address.window)?;
                if window.active == Some(address.id) {
                    window.active = None;
                }
                self.detach(&mut repositories, address.id)
            })
        };
        if let Some(mut slot) = removed {
            slot.close();
        }
    }

    /// Detach every instance before allowing writes again, using the key captured before Git runs.
    pub(crate) fn finish_worktree_remove(&self, path: &Path) -> (Vec<Address>, Vec<String>) {
        let mut repositories = self.repositories.lock().expect("session lock");
        let mut tabs = repositories
            .slots
            .iter()
            .filter(|(_, slot)| slot.workdir == path)
            .filter_map(|(&id, _)| repositories.address(id))
            .collect::<Vec<_>>();
        let ids = tabs.iter().map(|tab| tab.id).collect::<Vec<_>>();
        let removed = self.detach_tabs(&mut repositories, &ids);
        let mut windows = Vec::new();
        let mut retired = Vec::new();
        let owners = repositories.windows.keys().cloned().collect::<Vec<_>>();
        for owner in owners {
            let window = &repositories.windows[&owner];
            let pending = window.pending.as_ref().is_some_and(|(_, key)| key == path);
            if !pending && !tabs.iter().any(|tab| tab.window == owner) {
                continue;
            }
            if owner != "main" && !repositories.slots.values().any(|slot| slot.owner == owner) {
                retired.push(repositories.windows.remove(&owner));
                windows.push(owner);
            } else {
                let revision = repositories.next_id();
                let window = repositories.windows.get_mut(&owner).expect("owner window");
                if pending {
                    window.pending = None;
                }
                if window.active.is_some_and(|id| ids.contains(&id)) {
                    window.active = None;
                    window.replacing = revision;
                }
            }
        }
        drop(repositories);
        drop((removed, retired));
        tabs.retain(|tab| !windows.contains(&tab.window));
        (tabs, windows)
    }
}

/// The repository, set to show a diff in full however many lines it has when `full` is set
/// (Show Diff Anyway).
pub(crate) fn current_full(session: &Session, window: &RepoRequest, full: Option<bool>) -> CommandResult<Repo> {
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
pub(crate) fn current(session: &Session, window: &RepoRequest) -> CommandResult<Repo> {
    let mut repo = session
        .slot(window, |slot| slot.repo.clone())
        .flatten()
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
    request: &RepoRequest,
    dir: PathBuf,
    title: &'static str,
    work: impl FnOnce() -> oxbow_core::Result<T> + Send + 'static,
) -> CommandResult<T> {
    let result = logged(Some(dir), console::EntryKind::Read, title, work).await;
    request.ensure_open()?;
    result
}

/// A private channel for addressed notices. Register it in each repository window before
/// opening tabs so it also receives activation requests from other windows.
#[tauri::command]
pub(crate) fn repo_events(
    window: WebviewWindow,
    session: State<'_, Session>,
    on_notice: Channel<RepoNotice>,
) -> CommandResult<()> {
    session.set_notices(window.label(), on_notice)
}

/// Stop the reads running in `lanes`: what they would answer is no longer wanted, as the
/// search was cleared or the commit panel closed.
#[tauri::command]
pub(crate) fn stop_reads(window: RepoRequest, session: State<'_, Session>, lanes: Vec<String>) {
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
pub(crate) fn stop_action(window: RepoRequest, session: State<'_, Session>) {
    if window.label() == "settings" || crate::user_command_window::is_command_window(window.label()) {
        session.stops.stop(window.label());
    } else {
        let id = window.address.as_ref().map(|address| address.id);
        session
            .stops
            .stop_where(|scope| scope.window == window.label() && scope.session == id);
    }
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
    fn restart_does_not_interrupt_commands_and_rejects_new_runs_until_resumed() {
        let stops = Arc::new(Stops::default());
        let active = stops.start("main");
        assert!(!stops.pause_if_idle());
        assert!(!active.cancel.load(Ordering::Relaxed));
        drop(active);
        assert!(stops.pause_if_idle());
        assert!(stops.start("settings").cancel.load(Ordering::Relaxed));
        stops.resume();
        assert!(!stops.start("settings").cancel.load(Ordering::Relaxed));
    }

    #[test]
    fn background_fetch_stop_and_start_are_scoped_to_the_latest_request() {
        let mut slot = Slot::default();
        let auto = slot.start_fetch(1).unwrap();
        let manual = slot.start_fetch(2).unwrap();
        assert!(auto.load(Ordering::Relaxed));
        slot.stop_fetch(1);
        assert!(slot.start_fetch(1).is_none());
        assert!(!manual.load(Ordering::Relaxed));

        slot.stop_fetch(3);
        slot.close();
        assert!(slot.start_fetch(3).is_none());
        let reopened = slot.start_fetch(4).unwrap();
        slot.stop_fetch(3);
        assert!(!reopened.load(Ordering::Relaxed));
        slot.stop_fetch(4);
        assert!(reopened.load(Ordering::Relaxed));
    }

    #[test]
    fn a_files_fetch_only_supersedes_older_files_fetches_in_its_window() {
        let stops = Arc::new(Stops::default());
        let mut main = Slot::default();
        let mut other = Slot::default();
        let first = stops.start("main");
        let worktree = stops.start("main");
        let other_files = stops.start("other");
        assert!(main.start_files_fetch(1, &first));
        assert!(other.start_files_fetch(2, &other_files));

        let newest = stops.start("main");
        assert!(main.start_files_fetch(3, &newest));
        assert!(first.cancel.load(Ordering::Relaxed));
        drop(first);
        let late = stops.start("main");
        assert!(!main.start_files_fetch(2, &late));
        for run in [&newest, &worktree, &other_files] {
            assert!(!run.cancel.load(Ordering::Relaxed));
        }

        stops.stop("main");
        assert!(newest.cancel.load(Ordering::Relaxed));
        assert!(worktree.cancel.load(Ordering::Relaxed));
        assert!(!other_files.cancel.load(Ordering::Relaxed));
        other.close();
        assert!(other_files.cancel.load(Ordering::Relaxed));
    }

    fn open(session: &Session, window: &str, repo: &Repo, legacy: bool) -> Address {
        let ticket = session.begin_open(window, legacy).unwrap();
        let Opened::New(address) = session.finish_open(window, ticket, repo.clone()).unwrap() else {
            panic!("expected a new repository session");
        };
        address
    }

    fn linked_worktree(repo: &Repo, name: &str) -> Repo {
        repo.run(&oxbow_core::cli::GitCommand::new([
            "worktree", "add", "-qb", name, name,
        ]))
        .unwrap();
        Repo::open(repo.workdir().join(name)).unwrap()
    }

    #[test]
    fn worktree_removal_reserves_the_target_before_git_and_releases_it_after_failure() {
        use oxbow_core::Action;

        let repo = repo("remove-guard");
        let linked = linked_worktree(&repo, "linked");
        let sibling = linked_worktree(&repo, "sibling");
        let session = Session::default();
        let source = open(&session, "main", &repo, true);
        let target = open(&session, "main", &linked, false);
        let other = open(&session, "main", &sibling, false);
        session.activate(&target, None).unwrap();
        session.activate(&other, None).unwrap();
        let path = workdir_key(linked.workdir());
        let remove = || session.start_worktree_remove(&source, "main", &path);
        let write = |draft| session.start_repo_write(&target, "main", draft, None);
        for kind in [Some(true), Some(false), None] {
            let running_or_queued = write(kind).unwrap();
            let error = remove().err().unwrap();
            assert!(error.contains(if kind == Some(true) { "draft" } else { "Git command" }));
            assert!(!running_or_queued.cancel.load(Ordering::Relaxed));
        }

        let sibling_write = session.start_repo_write(&other, "main", Some(true), None).unwrap();
        let reservation = remove().unwrap();
        assert!(!reservation.cancel.load(Ordering::Relaxed));
        assert!(!sibling_write.cancel.load(Ordering::Relaxed));
        assert!(write(Some(true)).is_err());
        assert!(write(Some(false)).is_err());
        assert!(write(None).is_err());
        assert!(remove().is_err());
        assert!(session.start_repo_write(&other, "main", Some(false), None).is_ok());

        std::fs::write(linked.workdir().join("a.txt"), "Keep me in the stash").unwrap();
        let action = |stash| Action::WorktreeRemove {
            path: "linked".into(),
            force: false,
            stash,
        };
        assert!(
            repo.perform_with(&action(false), &mut |_| {}, &reservation.cancel)
                .is_err()
        );
        drop(reservation);
        assert!(session.live(&target) && linked.workdir().is_dir());
        assert!(write(Some(true)).is_ok());
        let reservation = remove().unwrap();
        repo.perform_without_undo(&action(true), &mut |_| {}, &reservation.cancel)
            .unwrap();
        assert!(!linked.workdir().exists());
        assert_eq!(session.finish_worktree_remove(&path), (vec![target.clone()], vec![]));
        assert!(!session.live(&target));
        assert!(session.live(&source));
        assert_eq!(session.resolve("main", None).unwrap(), Some(other));
        let stash = repo
            .run(&oxbow_core::cli::GitCommand::new(["show", "stash@{0}:a.txt"]))
            .unwrap();
        assert_eq!(stash.stdout, "Keep me in the stash");
        std::fs::remove_dir_all(repo.workdir()).unwrap();
    }

    #[test]
    fn removal_retires_standalone_windows_and_cancels_late_opens() {
        use oxbow_core::{Action, cli::GitCommand};

        let repo = repo("remove-window");
        let session = Session::default();
        let source = open(&session, "main", &repo, true);
        for (name, opened) in [("standalone", true), ("pending", false)] {
            let linked = linked_worktree(&repo, name);
            let WindowTarget::New(window) = session.reserve_window(linked.workdir()).unwrap() else {
                panic!("expected a new window");
            };
            let ticket = session.begin_open(&window, false).unwrap();
            if opened {
                let address = open(&session, &window, &linked, false);
                session.activate(&address, None).unwrap();
            }
            #[cfg(not(unix))]
            let path = repo.workdir().join(name);
            #[cfg(unix)]
            let alias = {
                let temporary = oxbow_core::cli::private_temp_folder("oxbow-remove-alias-").unwrap();
                let parent = temporary.path().join("parent");
                std::os::unix::fs::symlink(repo.workdir(), &parent).unwrap();
                let alias = parent.join(name);
                (temporary, alias)
            };
            #[cfg(unix)]
            let path = alias.1;
            let key = workdir_key(&path);
            let reservation = session.start_worktree_remove(&source, "main", &key).unwrap();
            assert!(session.finish_open(&window, ticket, linked.clone()).is_err());
            repo.perform_with(
                &Action::WorktreeRemove {
                    path: path.display().to_string(),
                    force: false,
                    stash: false,
                },
                &mut |_| {},
                &reservation.cancel,
            )
            .unwrap();
            assert_eq!(session.finish_worktree_remove(&key), (vec![], vec![window.clone()]));
            assert!(!session.repositories.lock().unwrap().contains_window(&window));
            drop(reservation);
            assert!(session.finish_open(&window, ticket, linked.clone()).is_err());
            assert!(session.reserve_window(linked.workdir()).is_err());
        }
        assert!(session.live(&source));
        assert_eq!(
            repo.run(&GitCommand::new(["worktree", "list", "--porcelain"]))
                .unwrap()
                .stdout
                .matches("worktree ")
                .count(),
            1
        );
        std::fs::remove_dir_all(repo.workdir()).unwrap();
    }

    #[test]
    fn a_new_window_stays_reserved_through_legacy_startup() {
        let repo = repo("window-startup");
        let session = Session::default();
        let WindowTarget::New(label) = session.reserve_window(repo.workdir()).unwrap() else {
            panic!("first invocation must create a window");
        };
        // The old UI closes the empty slot after initial_repo reads the pending path.
        assert_eq!(
            session.repositories.lock().unwrap().pending(&label),
            Some(repo.workdir())
        );
        session.close(&label, None, true);
        let alias = repo.workdir().join(".");
        let key = workdir_key(&alias);
        {
            let repositories = session.repositories.lock().unwrap();
            assert_eq!(
                repositories.opening_target(&key, "main", false),
                Some(Located::Pending(label.clone()))
            );
            assert_eq!(repositories.opening_target(&key, "main", true), None);
            assert_eq!(repositories.opening_target(&key, &label, false), None);
        }
        assert_eq!(
            session.reserve_window(&alias).unwrap(),
            WindowTarget::Existing(Located::Pending(label.clone()))
        );
        let address = open(&session, &label, &repo, true);
        assert_eq!(session.resolve(&label, None).unwrap(), Some(address.clone()));
        assert!(session.repositories.lock().unwrap().pending(&label).is_none());
        assert_eq!(
            session.reserve_window(&alias).unwrap(),
            WindowTarget::Existing(Located::Open(address.clone()))
        );

        let other = self::repo("window-other");
        let replacement = open(&session, &label, &other, true);
        assert!(!session.live(&address));
        assert_eq!(session.resolve(&label, None).unwrap(), Some(replacement.clone()));
        session.close(&label, Some(&replacement), true);
        assert_eq!(session.resolve(&label, None).unwrap(), None);
        for repo in [repo, other] {
            std::fs::remove_dir_all(repo.workdir()).unwrap();
        }
    }

    #[test]
    fn legacy_open_recent_replaces_the_local_repo_and_tabs_prefer_the_local_copy() {
        let before = repo("legacy-replace-local");
        let shared = repo("legacy-replace-shared");
        let session = Session::default();
        let WindowTarget::New(other_window) = session.reserve_window(shared.workdir()).unwrap() else {
            unreachable!()
        };
        let other = open(&session, &other_window, &shared, true);
        let previous = open(&session, "main", &before, true);
        let key = workdir_key(shared.workdir());
        {
            let repositories = session.repositories.lock().unwrap();
            assert_eq!(repositories.opening_target(&key, "main", true), None);
            assert_eq!(
                repositories.opening_target(&key, "main", false),
                Some(Located::Open(other.clone()))
            );
        }

        let local = open(&session, "main", &shared, true);
        assert!(!session.live(&previous));
        assert_eq!(session.resolve("main", None).unwrap(), Some(local.clone()));
        assert_eq!(session.resolve(&other_window, None).unwrap(), Some(other.clone()));
        let repositories = session.repositories.lock().unwrap();
        assert_eq!(
            repositories.locate(&key, Some("main")),
            Some(Located::Open(local.clone()))
        );
        assert_eq!(
            repositories.opening_target(&key, "main", false),
            Some(Located::Open(local.clone()))
        );
        assert_eq!(
            repositories.opening_target(&key, &other_window, false),
            Some(Located::Open(other.clone()))
        );
        drop(repositories);
        let tab = session.begin_open("main", false).unwrap();
        assert!(matches!(session.finish_open("main", tab, shared.clone()).unwrap(),
            Opened::Existing(Located::Open(address)) if address == local));
        assert_eq!(
            session.reserve_window(shared.workdir()).unwrap(),
            WindowTarget::Existing(Located::Open(other))
        );
        for repo in [before, shared] {
            std::fs::remove_dir_all(repo.workdir()).unwrap();
        }
    }

    #[test]
    fn a_replaced_or_closed_legacy_open_cannot_finish_late() {
        let repo = repo("late-open");
        let session = Session::default();
        let old = session.begin_open("main", true).unwrap();
        let newer = session.begin_open("main", true).unwrap();
        assert!(session.finish_open("main", old, repo.clone()).is_err());
        session.close("main", None, true);
        assert!(session.finish_open("main", newer, repo.clone()).is_err());

        let pending = session.begin_open("main", false).unwrap();
        session.remove_window("main");
        assert!(session.finish_open("main", pending, repo.clone()).is_err());
        std::fs::remove_dir_all(repo.workdir()).unwrap();
    }

    #[test]
    fn simultaneous_tab_and_window_opens_share_one_canonical_reservation() {
        let repo = repo("window-concurrent");
        let session = Session::default();
        let barrier = std::sync::Barrier::new(8);
        let targets = std::thread::scope(|scope| {
            let requests: Vec<_> = (0..8)
                .map(|index| {
                    let session = &session;
                    let repo = &repo;
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        if index % 2 == 0 {
                            session.reserve_window(&repo.workdir().join(".")).unwrap()
                        } else {
                            let label = format!("owner-{index}");
                            session
                                .repositories
                                .lock()
                                .unwrap()
                                .windows
                                .insert(label.clone(), RepoWindow::default());
                            let ticket = session.begin_open(&label, false).unwrap();
                            match session.finish_open(&label, ticket, repo.clone()).unwrap() {
                                Opened::New(address) => WindowTarget::New(address.window),
                                Opened::Existing(target) => WindowTarget::Existing(target),
                            }
                        }
                    })
                })
                .collect();
            requests
                .into_iter()
                .map(|request| request.join().unwrap())
                .collect::<Vec<_>>()
        });
        let new: Vec<_> = targets
            .iter()
            .filter_map(|target| match target {
                WindowTarget::New(label) => Some(label),
                WindowTarget::Existing(_) => None,
            })
            .collect();
        assert_eq!(new.len(), 1);
        assert!(targets.iter().all(|target| match target {
            WindowTarget::New(_) => true,
            WindowTarget::Existing(target) => target.window() == new[0],
        }));
        session.remove_window(new[0]);
        assert!(matches!(
            session.reserve_window(repo.workdir()).unwrap(),
            WindowTarget::New(_)
        ));
        std::fs::remove_dir_all(repo.workdir()).unwrap();
    }

    #[test]
    fn tabs_pin_reads_fetches_and_stop_across_activation_and_reopen() {
        let repo_a = repo("tab-a");
        let repo_b = repo("tab-b");
        let session = Session::default();
        let a = open(&session, "main", &repo_a, false);
        let b = open(&session, "main", &repo_b, false);
        assert_eq!(session.resolve("main", None).unwrap(), None);
        assert!(session.resolve("settings", Some(a.id)).is_err());
        assert!(session.resolve("repo-1", Some(a.id)).is_err());

        session.activate(&a, None).unwrap();
        let pinned = session.resolve("main", None).unwrap().unwrap();
        assert_eq!(session.resolve("settings", None).unwrap(), Some(a.clone()));
        let side_a = session.stops.start_for("main", Some(a.id), true);
        let side_b = session.stops.start_for("main", Some(b.id), true);
        let (read_a, fetch_a) = session
            .with_slot(&a, |slot| (slot.lane("history".into()), slot.start_fetch(1).unwrap()))
            .unwrap();
        let (read_b, fetch_b) = session
            .with_slot(&b, |slot| (slot.lane("history".into()), slot.start_fetch(1).unwrap()))
            .unwrap();
        let replacing = session.begin_open("main", true).unwrap();
        session.activate(&b, None).unwrap();
        assert!(session.finish_open("main", replacing, repo_a.clone()).is_err());
        assert!(session.activate(&a, replacing).is_err());
        session.with_slot(&pinned, |slot| read_history(slot, |_| {}));
        assert!(session.with_slot(&a, |slot| slot.changes.is_some()).unwrap());
        assert!(session.with_slot(&b, |slot| slot.changes.is_none()).unwrap());

        session
            .stops
            .stop_where(|scope| scope.window == "main" && scope.session == Some(a.id));
        assert!(side_a.cancel.load(Ordering::Relaxed));
        assert!(!side_b.cancel.load(Ordering::Relaxed));
        session.activate(&a, None).unwrap();
        assert_eq!(session.resolve("main", None).unwrap(), Some(pinned.clone()));

        session.close("main", Some(&a), false);
        let reopened = open(&session, "main", &repo_a, false);
        assert_ne!(a, reopened);
        assert!(session.resolve("main", Some(a.id)).is_err());
        assert!(
            session
                .with_slot(&pinned, |_| panic!("late reply reached new slot"))
                .is_none()
        );
        for cancelled in [&read_a, &fetch_a] {
            assert!(cancelled.load(Ordering::Relaxed));
        }
        for unchanged in [&read_b, &fetch_b, &side_b.cancel] {
            assert!(!unchanged.load(Ordering::Relaxed));
        }
        assert!(
            session
                .with_slot(&reopened, |slot| slot.start_fetch(1).is_some())
                .unwrap()
        );
        session.remove_window("main");
        assert!(side_b.cancel.load(Ordering::Relaxed));
        assert!(fetch_b.load(Ordering::Relaxed));
        for repo in [repo_a, repo_b] {
            std::fs::remove_dir_all(repo.workdir()).unwrap();
        }
    }

    #[test]
    fn disabling_tabs_checks_every_window_and_persists_before_retiring_anything() {
        let a = repo("mode-a");
        let b = repo("mode-b");
        let c = repo("mode-c");
        let d = repo("mode-d");
        let session = Session::default();
        let WindowTarget::New(second) = session.reserve_window(c.workdir()).unwrap() else {
            unreachable!()
        };
        let keep_a = open(&session, "main", &a, false);
        let close_b = open(&session, "main", &b, false);
        let keep_c = open(&session, &second, &c, false);
        let close_d = open(&session, &second, &d, false);
        session.activate(&keep_a, None).unwrap();
        session.activate(&keep_c, None).unwrap();
        let plan = session.repositories.lock().unwrap().retiring_tabs();
        let closing = [close_b.id, close_d.id];
        let running = session.stops.start_scoped(
            StopScope {
                window: "settings".into(),
                session: Some(close_d.id),
                change: Some(RepoChange::Git(turns::canonical(&d.common_dir()))),
                worktree: Some(workdir_key(d.workdir())),
                removing: None,
            },
            true,
        );
        assert!(
            session
                .commit_tab_mode(false, &plan, &closing, || panic!("a Git write is queued"))
                .is_err()
        );
        assert!(session.live(&close_b) && session.live(&close_d));
        drop(running);

        assert!(
            session
                .commit_tab_mode(false, &plan, &closing, || Err("Disk write failed".into()))
                .is_err()
        );
        assert!(session.live(&close_b) && session.live(&close_d));
        assert!(!session.repositories.lock().unwrap().tabs_disabled);

        session.commit_tab_mode(false, &plan, &closing, || Ok(())).unwrap();
        assert!(!session.live(&close_b) && !session.live(&close_d));
        assert_eq!(session.resolve("main", None).unwrap(), Some(keep_a.clone()));
        assert_eq!(session.resolve(&second, None).unwrap(), Some(keep_c.clone()));
        session.commit_tab_mode(true, &HashMap::new(), &[], || Ok(())).unwrap();
        assert!(session.live(&keep_a) && session.live(&keep_c));
        for repo in [a, b, c, d] {
            std::fs::remove_dir_all(repo.workdir()).unwrap();
        }
    }

    #[test]
    fn close_other_tabs_keeps_the_clicked_tab_only_after_every_write_is_idle() {
        let a = repo("batch-a");
        let b = repo("batch-b");
        let c = repo("batch-c");
        let session = Session::default();
        let a_id = open(&session, "main", &a, false);
        let b_id = open(&session, "main", &b, false);
        let c_id = open(&session, "main", &c, false);
        session.activate(&a_id, None).unwrap();
        session.activate(&c_id, None).unwrap();
        let close = || session.close_tabs("main", &[a_id.id, c_id.id], Some(b_id.id), Some(c_id.id));
        assert!(close().is_err(), "a provisional slot cannot be the fallback");
        let wrong_owner = Address {
            window: "other".into(),
            id: b_id.id,
        };
        assert!(session.publish_tab(&wrong_owner).is_err());
        assert!(close().is_err());
        session.publish_tab(&b_id).unwrap();
        assert_eq!(session.resolve("main", None).unwrap(), Some(c_id.clone()));

        let write = session.stops.start_scoped(
            StopScope {
                window: "command-main".into(),
                session: Some(c_id.id),
                change: Some(RepoChange::Git(turns::canonical(&c.common_dir()))),
                worktree: Some(workdir_key(c.workdir())),
                removing: None,
            },
            true,
        );
        assert!(close().is_err());
        assert!(session.live(&a_id) && session.live(&c_id));
        assert_eq!(session.resolve("main", None).unwrap(), Some(c_id.clone()));
        drop(write);
        let fetch = session.with_slot(&c_id, |slot| slot.start_fetch(1).unwrap()).unwrap();
        close().unwrap();
        assert!(fetch.load(Ordering::Relaxed));
        assert!(!session.live(&a_id) && !session.live(&c_id));
        assert_eq!(session.resolve("main", None).unwrap(), Some(b_id));
        for repo in [a, b, c] {
            std::fs::remove_dir_all(repo.workdir()).unwrap();
        }
    }

    #[test]
    fn a_worktree_closed_in_another_window_notifies_only_its_owner() {
        fn listen(session: &Session, label: &str) -> std::sync::mpsc::Receiver<serde_json::Value> {
            let (send, receive) = std::sync::mpsc::channel();
            session
                .set_notices(
                    label,
                    Channel::new(move |message| {
                        send.send(message.deserialize()?).unwrap();
                        Ok(())
                    }),
                )
                .unwrap();
            receive
        }

        let repo = repo("close-notice");
        let session = Session::default();
        let WindowTarget::New(owner) = session.reserve_window(repo.workdir()).unwrap() else {
            unreachable!()
        };
        let main_events = listen(&session, "main");
        let owner_events = listen(&session, &owner);
        let address = open(&session, &owner, &repo, false);
        session.close(&owner, Some(&address), false);
        session.emit_closed(&address);
        assert!(!session.live(&address));
        assert_eq!(
            owner_events.try_recv().unwrap(),
            serde_json::json!({
                "session": address.id, "event": "repo-closed", "payload": null,
            })
        );
        assert!(main_events.try_recv().is_err());
        std::fs::remove_dir_all(repo.workdir()).unwrap();
    }

    #[test]
    fn closing_a_worktree_session_releases_the_shared_git_turn() {
        let repo = repo("worktree-turn");
        let linked = repo
            .workdir()
            .with_file_name(format!("oxbow-slot-linked-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&linked);
        let output = std::process::Command::new("git")
            .args(["worktree", "add", "--detach"])
            .arg(&linked)
            .current_dir(repo.workdir())
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let worktree = Repo::open(&linked).unwrap();
        let session = Session::default();
        let a = open(&session, "main", &repo, false);
        let b = open(&session, "main", &worktree, false);
        let common = turns::canonical(&repo.common_dir());
        assert_eq!(
            session.repositories.lock().unwrap().siblings(&common, &a),
            std::slice::from_ref(&b)
        );
        let cancel = session.with_slot(&b, |slot| slot.start_fetch(1).unwrap()).unwrap();
        let turn = session
            .turns
            .take(
                &worktree.common_dir(),
                turns::Running::new(&b.turn_key(), "linked".into(), "Fetch".into()),
                &cancel,
                |_| panic!("unexpected wait"),
            )
            .unwrap();
        assert_eq!(
            session.turns.other(&common, &a.turn_key()).unwrap().window,
            b.turn_key()
        );

        let removed_key = workdir_key(&linked);
        std::fs::remove_dir_all(&linked).unwrap();
        assert_eq!(workdir_key(&linked), removed_key);
        assert_eq!(
            session.repositories.lock().unwrap().locate(&workdir_key(&linked), None),
            Some(Located::Open(b.clone()))
        );
        session.close("main", Some(&b), false);
        assert!(cancel.load(Ordering::Relaxed));
        drop(turn);
        assert!(
            session
                .turns
                .take(
                    &common,
                    turns::Running::new(&a.turn_key(), "main".into(), "Commit".into()),
                    &AtomicBool::default(),
                    |_| panic!("still waiting for the closed session"),
                )
                .is_some()
        );
        std::fs::remove_dir_all(repo.workdir()).unwrap();
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
