//! A mode change first saves the closing tabs' drafts, then persists the setting and retires
//! those sessions together. Topology and writes to the closing repositories stay reserved.
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, State, WebviewWindow};

use crate::{CommandResult, repos, session::Session};

pub(crate) const KEY: &str = "oxbow.windows.repositoryTabs";
const IN_PROGRESS: &str = "Oxbow is changing how repository tabs are shown. Try again when it finishes.";
static NEXT: AtomicU64 = AtomicU64::new(1);

pub(crate) fn enabled(settings: &Map<String, Value>) -> bool {
    settings.get(KEY).and_then(Value::as_bool).unwrap_or(true)
}

pub(crate) struct Preparation {
    generation: u64,
    pub(crate) closing: HashMap<String, Vec<u64>>,
    ready: HashSet<String>,
    problem: Option<String>,
    changed: std::sync::Arc<tokio::sync::Notify>,
}

impl Preparation {
    pub(crate) fn drafts_sealed(&self, label: &str, id: u64) -> bool {
        self.ready.contains(label) && self.closing.get(label).is_some_and(|ids| ids.contains(&id))
    }

    pub(crate) fn window_removed(&self, label: &str) {
        if self.closing.contains_key(label) {
            self.changed.notify_one();
        }
    }

    fn acknowledge(&mut self, label: &str, generation: u64, problem: Option<String>) {
        if self.generation == generation && self.closing.contains_key(label) && self.ready.insert(label.to_owned()) {
            if self.problem.is_none() {
                self.problem = problem;
            }
            self.changed.notify_one();
        }
    }
}

#[derive(Serialize, Clone)]
struct Prepare<'a> {
    generation: u64,
    closing: &'a [u64],
}

#[derive(Serialize, Clone)]
struct Finished<'a> {
    generation: u64,
    enabled: bool,
    closed: &'a [u64],
}

struct Transition<'a> {
    app: &'a AppHandle,
    session: &'a Session,
    topology: Option<tokio::sync::RwLockWriteGuard<'a, ()>>,
    generation: u64,
    closed: Vec<u64>,
}

impl Drop for Transition<'_> {
    fn drop(&mut self) {
        let mut repositories = self.session.repositories.lock().expect("session lock");
        repositories.tab_transition = None;
        let enabled = !repositories.tabs_disabled;
        drop(repositories);
        // A window can open its saved tab as soon as it receives the event.
        drop(self.topology.take());
        let _ = self.app.emit(
            "repo-tabs-finished",
            Finished {
                generation: self.generation,
                enabled,
                closed: &self.closed,
            },
        );
        if !self.closed.is_empty() {
            repos::changed(self.app);
        }
    }
}

#[tauri::command]
pub(crate) fn repo_tabs_enabled(session: State<'_, Session>) -> bool {
    !session.repositories.lock().expect("session lock").tabs_disabled
}

#[tauri::command]
pub(crate) fn repo_tabs_ready(
    window: WebviewWindow,
    session: State<'_, Session>,
    generation: u64,
    problem: Option<String>,
) {
    if let Some(preparation) = &mut session.repositories.lock().expect("session lock").tab_transition {
        preparation.acknowledge(window.label(), generation, problem);
    }
}

/// All settings writers hold settings_writes, including while this waits for acknowledgements.
pub(crate) async fn persist(
    app: &AppHandle,
    session: &Session,
    enabled: bool,
    write: impl FnOnce() -> CommandResult<()>,
) -> CommandResult<()> {
    let current = !session.repositories.lock().expect("session lock").tabs_disabled;
    if current == enabled {
        return write();
    }
    let generation = NEXT.fetch_add(1, Ordering::Relaxed);
    let changed = std::sync::Arc::new(tokio::sync::Notify::new());
    let (topology, closing, ids) = {
        let mut repositories = session.repositories.lock().expect("session lock");
        // Even briefly taking the write lock would reject a concurrent publication.
        repositories.check_tab_mode_change()?;
        let topology = session.repo_topology.try_write().map_err(|_| IN_PROGRESS)?;
        let closing = if enabled {
            HashMap::new()
        } else {
            repositories.retiring_tabs()
        };
        let ids = closing.values().flatten().copied().collect::<Vec<_>>();
        session.check_tab_writes(&repositories, &ids, false)?;
        repositories.tab_transition = Some(Preparation {
            generation,
            closing: closing.clone(),
            ready: HashSet::new(),
            problem: None,
            changed: changed.clone(),
        });
        (topology, closing, ids)
    };
    let mut transition = Transition {
        app,
        session,
        topology: Some(topology),
        generation,
        closed: Vec::new(),
    };
    for (label, ids) in &closing {
        app.emit_to(
            label,
            "repo-tabs-prepare",
            Prepare {
                generation,
                closing: ids,
            },
        )
        .map_err(|_| "Oxbow couldn’t check drafts in a repository window.")?;
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            {
                let repositories = session.repositories.lock().expect("session lock");
                let preparation = repositories.tab_transition.as_ref().expect("tab preparation");
                if let Some(problem) = &preparation.problem {
                    return Err(problem.clone());
                }
                if closing.keys().any(|label| !repositories.contains_window(label)) {
                    return Err("A repository window closed while Oxbow checked its tabs.".into());
                }
                if preparation.ready.len() == closing.len() {
                    return Ok(());
                }
            }
            changed.notified().await;
        }
    })
    .await
    .map_err(|_| "A repository window didn’t finish checking its drafts. Try again.")??;

    session.commit_tab_mode(enabled, &closing, &ids, write)?;
    transition.closed = ids;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_are_on_by_default_and_follow_persisted_false() {
        assert!(enabled(&Map::new()));
        assert!(!enabled(&Map::from_iter([(KEY.into(), Value::Bool(false))])));
    }

    #[test]
    fn every_window_must_acknowledge_once_and_an_old_ack_cannot_seal_drafts() {
        let mut prepare = Preparation {
            generation: 7,
            closing: HashMap::from([("main".into(), vec![1]), ("repo-2".into(), vec![3])]),
            ready: HashSet::new(),
            problem: None,
            changed: Default::default(),
        };
        prepare.acknowledge("main", 6, None);
        prepare.acknowledge("settings", 7, None);
        assert!(prepare.ready.is_empty());
        prepare.acknowledge("main", 7, None);
        assert!(prepare.drafts_sealed("main", 1));
        assert!(!prepare.drafts_sealed("repo-2", 3));
        prepare.acknowledge("repo-2", 7, Some("Review draft failed".into()));
        prepare.acknowledge("repo-2", 7, None);
        assert_eq!(prepare.problem.as_deref(), Some("Review draft failed"));
    }
}
