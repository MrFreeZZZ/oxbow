//! Changes to a repository open in several windows (one per worktree) run one at a time: an
//! action or a fetch in one window waits for the one running in another, and says what it waits
//! for. Within a window nothing waits here: the window itself puts a push or pull after its fetch.

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

/// A change running in a repository.
#[derive(Clone)]
pub struct Running {
    id: u64,
    /// The window's label.
    pub window: String,
    /// The window's folder name, as its title shows it.
    pub folder: String,
    pub title: String,
}

/// A change's turn: the other windows of its repository wait until it is dropped.
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
    /// Wait until no other window changes the repository whose shared Git folder is `common`,
    /// then take the turn. `on_wait` hears what it waits for, each time that changes. Stop ends
    /// the wait with `None`.
    pub fn take(
        self: &Arc<Self>,
        common: &Path,
        window: &str,
        folder: String,
        title: String,
        cancel: &AtomicBool,
        mut on_wait: impl FnMut(&Running),
    ) -> Option<Turn> {
        let common = canonical(common);
        let mut told = None;
        loop {
            let other = {
                let mut running = self.running.lock().expect("turns lock");
                let list = running.entry(common.clone()).or_default();
                match list.iter().find(|r| r.window != window) {
                    Some(other) => other.clone(),
                    None => {
                        let id = self.next.fetch_add(1, Ordering::Relaxed);
                        list.push(Running {
                            id,
                            window: window.to_owned(),
                            folder,
                            title,
                        });
                        return Some(Turn {
                            turns: self.clone(),
                            common,
                            id,
                        });
                    }
                }
            };
            if told != Some(other.id) {
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

    #[test]
    fn a_window_waits_for_another_but_not_for_itself() {
        let turns = Arc::new(Turns::default());
        let common = Path::new("/nowhere/repo/.git");
        let never = AtomicBool::new(false);
        let first = turns.take(common, "main", "repo".into(), "Rebase".into(), &never, |_| {});
        assert!(first.is_some());
        // The same window's fetch runs beside its action.
        let fetch = turns.take(common, "main", "repo".into(), "Fetch".into(), &never, |_| {
            panic!("waited")
        });
        assert!(fetch.is_some());
        assert_eq!(turns.other(common, "repo-2").unwrap().title, "Rebase");
        assert!(turns.other(common, "main").is_none());

        // Another window waits, and says for what, until Stop.
        let stop = AtomicBool::new(true);
        let mut heard = Vec::new();
        let waiting = turns.take(common, "repo-2", "feature".into(), "Commit".into(), &stop, |r| {
            heard.push(format!("{} in {}", r.title, r.folder))
        });
        assert!(waiting.is_none());
        assert_eq!(heard, ["Rebase in repo"]);

        // Once both are done, it goes.
        drop(first);
        drop(fetch);
        let go = turns.take(common, "repo-2", "feature".into(), "Commit".into(), &never, |_| {
            panic!("waited")
        });
        assert!(go.is_some());
        drop(go);
        assert!(turns.running.lock().unwrap().is_empty());
    }

    #[test]
    fn the_wait_ends_when_the_other_change_does() {
        let turns = Arc::new(Turns::default());
        let common = Path::new("/nowhere/repo/.git");
        let never = AtomicBool::new(false);
        let first = turns.take(common, "main", "repo".into(), "Pull".into(), &never, |_| {});
        let other = turns.clone();
        let waiter = std::thread::spawn(move || {
            let never = AtomicBool::new(false);
            other
                .take(common, "repo-2", "feature".into(), "Commit".into(), &never, |_| {})
                .is_some()
        });
        std::thread::sleep(Duration::from_millis(150));
        drop(first);
        assert!(waiter.join().unwrap());
    }
}
