//! Changes to a repository run one at a time, whichever window asks for them: a repository
//! window, Settings acting for it, or another window of the repository (one per worktree). Each
//! change has a number of its own, and one that waits says what it waits for.
//!
//! Reads (history, diffs, status) take no turn: they run beside anything, as git keeps its own
//! files apart with locks. A fetch takes one: it writes refs and FETCH_HEAD, and tags it fetched
//! while an action ran would go into that action's step in the Operation Log, for Undo to take
//! back. A fetch of the timer gives way instead of making an action wait: the action stops it,
//! and the next one fetches again.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The changes running now, by the repository's shared Git folder.
#[derive(Default)]
pub struct Turns {
    running: Mutex<HashMap<PathBuf, Vec<Running>>>,
    next: AtomicU64,
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

/// A change's turn: the others in its repository wait until it is dropped.
pub struct Turn {
    turns: Arc<Turns>,
    common: PathBuf,
    id: u64,
}

/// The shared Git folder as one path for every worktree: a worktree reaches it through
/// `.git/worktrees/<name>/../..`.
pub fn canonical(common: &Path) -> PathBuf {
    common.canonicalize().unwrap_or_else(|_| common.to_path_buf())
}

impl Turns {
    /// Wait until no other change runs in the repository whose shared Git folder is `common`,
    /// then take the turn for `change`. A change that gives way is stopped rather than waited
    /// for, unless `change` gives way too. `on_wait` hears what it waits for, each time that
    /// changes. Stop ends the wait with `None`.
    pub fn take(
        self: &Arc<Self>,
        common: &Path,
        mut change: Running,
        cancel: &AtomicBool,
        mut on_wait: impl FnMut(&Running),
    ) -> Option<Turn> {
        let common = canonical(common);
        let mut told = None;
        loop {
            let other = {
                let mut running = self.running.lock().expect("turns lock");
                let list = running.entry(common.clone()).or_default();
                if list.is_empty() {
                    if cancel.load(Ordering::Relaxed) {
                        running.remove(&common);
                        return None;
                    }
                    let id = self.next.fetch_add(1, Ordering::Relaxed);
                    change.id = id;
                    list.push(change);
                    return Some(Turn {
                        turns: self.clone(),
                        common,
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
        running.get(common)?.iter().find(|r| r.window != window).cloned()
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        let mut running = self.turns.running.lock().expect("turns lock");
        if let Some(list) = running.get_mut(&self.common) {
            list.retain(|r| r.id != self.id);
            if list.is_empty() {
                running.remove(&self.common);
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
