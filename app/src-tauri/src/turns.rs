//! Changes to a repository run one at a time, whichever window asks for them: a repository
//! window, Settings acting for it, or another window of the repository (one per worktree). Each
//! change has a number of its own, and one that waits says what it waits for.
//!
//! Reads (history, diffs, status) take no turn: they run beside anything, as git keeps its own
//! files apart with locks. A regular fetch takes one: it writes refs and FETCH_HEAD, and tags it fetched
//! while an action ran would go into that action's step in the Operation Log, for Undo to take
//! back. A fetch of the timer gives way instead of making an action wait: the action stops it,
//! and the next one fetches again. Review fetches have their own turn for their shared refs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oxbow_core::remote::FetchTarget;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Scope {
    Repository,
    ReviewFetch,
}

type TurnKey = (PathBuf, Scope);
type AutoFetchKey = (PathBuf, Vec<FetchTarget>);

pub(crate) const DEFAULT_FETCH_INTERVAL: u64 = 15;
pub(crate) const MAX_FETCH_INTERVAL: u64 = 1440;

/// The changes running now, by the repository's shared Git folder and scope.
#[derive(Default)]
pub struct Turns {
    running: Mutex<HashMap<TurnKey, Vec<Running>>>,
    next: AtomicU64,
    paused: AtomicBool,
    // None is a reserved fetch, including while it waits for its repository turn.
    auto_fetches: Mutex<HashMap<AutoFetchKey, Option<Instant>>>,
    fetch_interval: AtomicU64,
}

pub(crate) struct AutoFetch {
    turns: Arc<Turns>,
    key: AutoFetchKey,
    attempted_at: Option<Instant>,
}

impl AutoFetch {
    /// Successful and failed attempts both use their interval. Stop and giving way don't.
    pub(crate) fn attempted(&mut self, started: Instant) {
        self.attempted_at = Some(started);
    }
}

impl Drop for AutoFetch {
    fn drop(&mut self) {
        let mut fetches = self.turns.auto_fetches.lock().expect("auto fetch lock");
        if let Some(attempted_at) = self.attempted_at {
            *fetches.get_mut(&self.key).expect("reserved auto fetch") = Some(attempted_at);
        } else {
            fetches.remove(&self.key);
        }
    }
}

/// A change running in a repository, or asking to.
#[derive(Clone)]
pub struct Running {
    id: u64,
    /// The label of the repository window it is for (Settings acts for one).
    pub window: String,
    /// The window's folder name, as its title shows it.
    pub folder: String,
    pub title: String,
    gives_way: Option<GiveWay>,
}

/// How a change that gives way is stopped by one that comes after it.
#[derive(Clone)]
pub struct GiveWay {
    /// The change's own Stop.
    pub stop: Arc<AtomicBool>,
    /// Set with `stop` when another change asked for the turn, not the user.
    pub asked: Arc<AtomicBool>,
}

impl GiveWay {
    pub fn new(stop: Arc<AtomicBool>) -> Self {
        GiveWay {
            stop,
            asked: Arc::default(),
        }
    }

    /// Whether it was stopped to let another change go.
    pub fn gave_way(&self) -> bool {
        self.asked.load(Ordering::Relaxed)
    }
}

impl Running {
    pub fn new(window: &str, folder: String, title: String) -> Self {
        Running {
            id: 0,
            window: window.to_owned(),
            folder,
            title,
            gives_way: None,
        }
    }

    /// The change stops when another asks for the turn, rather than make it wait.
    pub fn giving_way(self, gives_way: GiveWay) -> Self {
        Running {
            gives_way: Some(gives_way),
            ..self
        }
    }
}

/// A change's turn: the others in its repository and scope wait until it is dropped.
pub struct Turn {
    turns: Arc<Turns>,
    key: TurnKey,
    id: u64,
}

/// The shared Git folder as one path for every worktree: a worktree reaches it through
/// `.git/worktrees/<name>/../..`.
pub fn canonical(common: &Path) -> PathBuf {
    common.canonicalize().unwrap_or_else(|_| common.to_path_buf())
}

impl Turns {
    pub(crate) fn set_fetch_interval(&self, minutes: u64) {
        self.fetch_interval.store(minutes, Ordering::Relaxed);
    }

    /// Claim the timer's fetch before it reaches the repository queue. A skipped tick can only
    /// predict when to try again if the previous attempt has already ended.
    pub(crate) fn reserve_auto_fetch(
        self: &Arc<Self>,
        common: &Path,
        targets: Vec<FetchTarget>,
        now: Instant,
    ) -> Result<AutoFetch, Option<Duration>> {
        let key = (canonical(common), targets);
        let mut fetches = self.auto_fetches.lock().expect("auto fetch lock");
        let minutes = self.fetch_interval.load(Ordering::Relaxed);
        let interval = Duration::from_secs(60 * if minutes == 0 { DEFAULT_FETCH_INTERVAL } else { minutes });
        fetches.retain(|_, start| {
            start
                .is_none_or(|start| now.saturating_duration_since(start) < Duration::from_secs(60 * MAX_FETCH_INTERVAL))
        });
        match fetches.get(&key) {
            Some(None) => return Err(None),
            Some(Some(start)) => {
                if let Some(remaining) = interval.checked_sub(now.saturating_duration_since(*start))
                    && !remaining.is_zero()
                {
                    return Err(Some(remaining));
                }
            }
            None => {}
        }
        // A guard owns the entry until the command ends, even if it is still queued.
        fetches.insert(key.clone(), None);
        Ok(AutoFetch {
            turns: self.clone(),
            key,
            attempted_at: None,
        })
    }

    /// Reserve an idle app for installation. Checked under the same lock as new turns.
    pub(crate) fn pause_if_idle(&self) -> bool {
        let running = self.running.lock().expect("turns lock");
        if running.values().any(|list| !list.is_empty()) {
            return false;
        }
        self.paused.store(true, Ordering::SeqCst);
        true
    }

    pub(crate) fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    /// Wait until no other change runs in the repository whose shared Git folder is `common`,
    /// then take the turn for `change`. A change that gives way is stopped rather than waited
    /// for, unless `change` gives way too. `on_wait` hears what it waits for, each time that
    /// changes. Stop ends the wait with `None`.
    pub fn take(
        self: &Arc<Self>,
        common: &Path,
        change: Running,
        cancel: &AtomicBool,
        on_wait: impl FnMut(&Running),
    ) -> Option<Turn> {
        self.take_in((canonical(common), Scope::Repository), change, cancel, on_wait)
    }

    /// Serialize writes and snapshots of review refs across the repository's worktrees.
    pub(crate) fn take_review_fetch(
        self: &Arc<Self>,
        common: &Path,
        change: Running,
        cancel: &AtomicBool,
    ) -> Option<Turn> {
        self.take_in((canonical(common), Scope::ReviewFetch), change, cancel, |_| {})
    }

    fn take_in(
        self: &Arc<Self>,
        key: TurnKey,
        mut change: Running,
        cancel: &AtomicBool,
        mut on_wait: impl FnMut(&Running),
    ) -> Option<Turn> {
        let mut told = None;
        loop {
            let other = {
                let mut running = self.running.lock().expect("turns lock");
                if self.paused.load(Ordering::SeqCst) {
                    return None;
                }
                let list = running.entry(key.clone()).or_default();
                if list.is_empty() {
                    if cancel.load(Ordering::Relaxed) {
                        running.remove(&key);
                        return None;
                    }
                    let id = self.next.fetch_add(1, Ordering::Relaxed);
                    change.id = id;
                    list.push(change);
                    return Some(Turn {
                        turns: self.clone(),
                        key,
                        id,
                    });
                }
                if change.gives_way.is_none() {
                    for way in list.iter().filter_map(|r| r.gives_way.as_ref()) {
                        way.asked.store(true, Ordering::Relaxed);
                        way.stop.store(true, Ordering::Relaxed);
                    }
                }
                // One stopped for this change ends in a moment: not worth saying.
                list.iter()
                    .find(|r| r.gives_way.is_none() || change.gives_way.is_some())
                    .cloned()
            };
            if let Some(other) = other
                && told != Some(other.id)
            {
                told = Some(other.id);
                on_wait(&other);
            }
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// What a window other than `window` is changing in the repository at `common` now.
    pub fn other(&self, common: &Path, window: &str) -> Option<Running> {
        let running = self.running.lock().expect("turns lock");
        running
            .get(&(common.to_path_buf(), Scope::Repository))?
            .iter()
            .find(|r| r.window != window)
            .cloned()
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut running = self.turns.running.lock().expect("turns lock");
        if let Some(list) = running.get_mut(&self.key) {
            list.retain(|r| r.id != self.id);
            if list.is_empty() {
                running.remove(&self.key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(window: &str, title: &str) -> Running {
        Running::new(window, "repo".into(), title.into())
    }

    #[test]
    fn auto_fetches_share_a_worktree_target_before_and_after_the_turn() {
        use oxbow_core::{Repo, cli::GitCommand};

        let fixture = oxbow_core::cli::private_temp_folder("oxbow-auto-fetch-").unwrap();
        let dir = fixture.path();
        let out = std::process::Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .current_dir(dir)
            .output()
            .unwrap();
        assert!(out.status.success());
        let repo = Repo::open(dir).unwrap();
        let git = |args: &[&str]| repo.run(&GitCommand::new(args.iter().copied())).unwrap();
        git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "Init",
            "--allow-empty",
        ]);
        git(&["remote", "add", "origin", "https://example.invalid/repo"]);
        git(&["worktree", "add", "-qb", "linked", "linked"]);
        let repo = repo.reopen().unwrap();
        git(&["remote", "add", "mirror", "https://example.invalid/mirror"]);
        let linked = Repo::open(dir.join("linked")).unwrap();
        assert_eq!(repo.remotes(), ["origin"]);
        assert_eq!(linked.remotes(), ["mirror", "origin"]);
        let main_plan = repo.reopen().unwrap().auto_fetch().unwrap();
        let linked_plan = linked.reopen().unwrap().auto_fetch().unwrap();
        assert_eq!(main_plan.command, linked_plan.command);
        assert!(main_plan.remote.is_none());
        let common = repo.common_dir();
        let alternative = common.join("worktrees/linked/../..");
        assert_eq!(canonical(&alternative), canonical(&common));

        let turns = Arc::new(Turns::default());
        turns.set_fetch_interval(2);
        let now = Instant::now();
        let reserve = |at| turns.reserve_auto_fetch(&alternative, linked_plan.targets.clone(), at);
        let never = AtomicBool::new(false);
        let manual = || turns.take(&common, change("main", "Fetch"), &never, |_| panic!("waited"));
        let manual_running = manual().unwrap();
        let stopped = turns.reserve_auto_fetch(&common, main_plan.targets, now).unwrap();
        assert!(matches!(reserve(now), Err(None)), "duplicate reached a busy repository");
        drop(stopped);
        drop(manual_running);

        let mut attempt = reserve(now).unwrap();
        assert!(matches!(reserve(now + Duration::from_secs(180)), Err(None)));
        assert!(
            turns
                .reserve_auto_fetch(&dir.join("other"), repo.fetch_targets(None), now)
                .is_ok()
        );
        assert!(
            turns
                .reserve_auto_fetch(&common, repo.fetch_targets(Some("different")), now)
                .is_ok()
        );
        // A regular error consumes the interval just like a success.
        attempt.attempted(now);
        drop(attempt);
        let next = now + Duration::from_secs(1);
        assert!(matches!(reserve(next), Err(Some(delay)) if delay == Duration::from_secs(119)));
        assert!(manual().is_some(), "manual fetch had to wait for the cooldown");
        assert!(reserve(now + Duration::from_secs(120)).is_ok());
        turns.set_fetch_interval(1);
        assert!(reserve(now + Duration::from_secs(60)).is_ok());
    }

    #[test]
    fn installation_reserves_only_an_idle_app_and_blocks_new_turns_until_resumed() {
        let turns = Arc::new(Turns::default());
        let never = AtomicBool::new(false);
        let common = Path::new("/nowhere/update-test/.git");
        let active = turns.take(common, change("main", "Push"), &never, |_| {}).unwrap();
        assert!(!turns.pause_if_idle());
        drop(active);
        assert!(turns.pause_if_idle());
        assert!(turns.take(common, change("main", "Commit"), &never, |_| {}).is_none());
        turns.resume();
        assert!(turns.take(common, change("main", "Commit"), &never, |_| {}).is_some());
    }

    #[test]
    fn a_change_waits_for_another_of_the_same_window() {
        // Settings acts for the main window: its change and the main window's have one label.
        let turns = Arc::new(Turns::default());
        let common = Path::new("/nowhere/repo/.git");
        let never = AtomicBool::new(false);
        let first = turns.take(common, change("main", "Rebase"), &never, |_| {});
        assert!(first.is_some());
        let stop = AtomicBool::new(true);
        let mut heard = Vec::new();
        let waiting = turns.take(common, change("main", "Delete Remote"), &stop, |r| {
            heard.push(format!("{} in {}", r.title, r.folder))
        });
        assert!(waiting.is_none(), "two changes of one window ran at once");
        assert_eq!(heard, ["Rebase in repo"]);
        // So does a fetch asked for, and another window's change.
        assert!(turns.take(common, change("main", "Fetch"), &stop, |_| {}).is_none());
        assert!(turns.take(common, change("repo-2", "Commit"), &stop, |_| {}).is_none());
        assert_eq!(turns.other(common, "repo-2").unwrap().title, "Rebase");
        assert!(turns.other(common, "main").is_none());

        drop(first);
        let go = turns.take(common, change("main", "Delete Remote"), &never, |_| panic!("waited"));
        assert!(go.is_some());
        drop(go);
        assert!(turns.running.lock().unwrap().is_empty());
    }

    #[test]
    fn a_review_fetch_waits_only_for_review_fetches_of_the_same_repository() {
        let turns = Arc::new(Turns::default());
        let dir = std::env::temp_dir().join(format!("oxbow-review-turn-{}", std::process::id()));
        let common = dir.join(".git");
        std::fs::create_dir_all(common.join("worktrees/linked")).unwrap();
        let alternate = common.join("worktrees/linked/../..");
        let never = AtomicBool::new(false);
        let review = turns
            .take_review_fetch(&alternate, change("worktree", "Fetch PR"), &never)
            .unwrap();
        let common = canonical(&common);
        assert!(turns.other(&common, "main").is_none());
        assert!(
            turns
                .take_review_fetch(&dir.join("other"), change("other", "Fetch PR"), &never)
                .is_some()
        );
        let commit = turns
            .take(&common, change("main", "Commit"), &never, |_| {
                panic!("waited for a review")
            })
            .unwrap();
        assert_eq!(turns.other(&common, "worktree").unwrap().title, "Commit");

        let stop = AtomicBool::new(false);
        let mut waited_for = Vec::new();
        let pending = turns.take_in(
            (common.clone(), Scope::ReviewFetch),
            change("main", "Fetch MR"),
            &stop,
            |other| {
                waited_for.push(other.window.clone());
                stop.store(true, Ordering::Relaxed);
            },
        );
        assert!(pending.is_none());
        assert_eq!(waited_for, ["worktree"]);
        drop(review);
        assert!(
            turns
                .take_review_fetch(&common, change("main", "Fetch MR"), &never)
                .is_some()
        );
        drop(commit);
        assert!(turns.running.lock().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_wait_ends_when_the_other_change_does() {
        let turns = Arc::new(Turns::default());
        let common = Path::new("/nowhere/repo/.git");
        let never = AtomicBool::new(false);
        let first = turns.take(common, change("main", "Pull"), &never, |_| {});
        let other = turns.clone();
        let waiter = std::thread::spawn(move || {
            let never = AtomicBool::new(false);
            other.take(common, change("repo-2", "Commit"), &never, |_| {}).is_some()
        });
        std::thread::sleep(Duration::from_millis(150));
        drop(first);
        assert!(waiter.join().unwrap());
    }

    #[test]
    fn a_fetch_of_the_timer_gives_way_to_an_action() {
        let turns = Arc::new(Turns::default());
        let common = Path::new("/nowhere/repo/.git");
        let never = AtomicBool::new(false);
        let way = GiveWay::new(Arc::default());
        let fetch = turns.take(
            common,
            change("main", "Auto fetch").giving_way(way.clone()),
            &never,
            |_| {},
        );
        assert!(fetch.is_some());
        // Another fetch of the timer waits for it, and doesn't stop it.
        let stop = AtomicBool::new(true);
        let second = GiveWay::new(Arc::default());
        let mut heard = Vec::new();
        let waiting = turns.take(common, change("repo-2", "Auto fetch").giving_way(second), &stop, |r| {
            heard.push(r.title.clone())
        });
        assert!(waiting.is_none());
        assert_eq!(heard, ["Auto fetch"]);
        assert!(!way.stop.load(Ordering::Relaxed));

        // An action stops it, without saying it waits, and goes once it has ended.
        let other = turns.clone();
        let action = std::thread::spawn(move || {
            let never = AtomicBool::new(false);
            other
                .take(common, change("main", "Commit"), &never, |r| {
                    panic!("waited for {}", r.title)
                })
                .is_some()
        });
        while !way.stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(way.gave_way(), "Stop, not giving way");
        drop(fetch);
        assert!(action.join().unwrap());
    }
    #[test]
    fn a_cancelled_change_never_takes_a_free_turn() {
        let turns = Arc::new(Turns::default());
        assert!(
            turns
                .take(
                    Path::new("/nowhere/cancelled/.git"),
                    change("main", "Save"),
                    &AtomicBool::new(true),
                    |_| {}
                )
                .is_none()
        );
        assert!(turns.running.lock().unwrap().is_empty());
    }
}
