//! Bounded, cancellable reads of Recent Repositories, independent of any open Repo/Slot.

use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{CommandResult, recent};

#[derive(Default)]
pub(crate) struct OverviewRuns(Mutex<HashMap<String, (String, Arc<AtomicBool>)>>);

impl OverviewRuns {
    pub(crate) fn stop(&self, window: &str, id: Option<&str>) {
        let mut runs = self.0.lock().expect("overview lock");
        if runs
            .get(window)
            .is_some_and(|(current, _)| id.is_none_or(|id| current == id))
            && let Some((_, cancel)) = runs.remove(window)
        {
            cancel.store(true, Ordering::Relaxed);
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OverviewRow {
    repository: recent::RecentRepo,
    summary: Option<oxbow_core::overview::RepoOverview>,
    error: Option<String>,
    checked_at: u64,
}

// Three workers across all overview windows. Cancelled batches leave before taking more work.
static BATCH: Mutex<()> = Mutex::new(());

#[tauri::command]
pub(crate) async fn repository_overview(
    app: AppHandle,
    window: tauri::WebviewWindow,
    runs: State<'_, OverviewRuns>,
    id: String,
) -> CommandResult<Vec<OverviewRow>> {
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut active = runs.0.lock().expect("overview lock");
        if let Some((_, previous)) = active.insert(window.label().to_owned(), (id.clone(), cancel.clone())) {
            previous.store(true, Ordering::Relaxed);
        }
    }
    let answer = crate::session::blocking(move || {
        let _batch = BATCH.lock().expect("overview batch lock");
        let repositories = if cancel.load(Ordering::Relaxed) {
            Vec::new()
        } else {
            recent::recent_repos(app)
        };
        Ok(read_rows(repositories, &cancel))
    })
    .await;
    runs.stop(window.label(), Some(&id));
    answer
}

#[tauri::command]
pub(crate) fn cancel_repository_overview(window: tauri::WebviewWindow, runs: State<'_, OverviewRuns>, id: String) {
    runs.stop(window.label(), Some(&id));
}

fn read_rows(repositories: Vec<recent::RecentRepo>, cancel: &AtomicBool) -> Vec<OverviewRow> {
    read_rows_with(repositories, cancel, oxbow_core::overview::read)
}

fn read_rows_with(
    repositories: Vec<recent::RecentRepo>,
    cancel: &AtomicBool,
    read: impl Fn(&std::path::Path, &AtomicBool) -> oxbow_core::Result<oxbow_core::overview::RepoOverview> + Sync,
) -> Vec<OverviewRow> {
    let next = AtomicUsize::new(0);
    let rows = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..3 {
            scope.spawn(|| {
                while !cancel.load(Ordering::Relaxed) {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(repository) = repositories.get(at) else { break };
                    let result = read(std::path::Path::new(&repository.path), cancel);
                    if cancel.load(Ordering::Relaxed) {
                        break;
                    }
                    let (summary, error) = match result {
                        Ok(summary) => (Some(summary), None),
                        Err(error) => (None, Some(error.to_string())),
                    };
                    rows.lock().expect("overview rows lock").push((
                        at,
                        OverviewRow {
                            repository: repository.clone(),
                            summary,
                            error,
                            checked_at: recent::now() as u64,
                        },
                    ));
                }
            });
        }
    });
    let mut rows = rows.into_inner().expect("overview rows lock");
    rows.sort_by_key(|(at, _)| *at);
    rows.into_iter().map(|(_, row)| row).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_completion_or_cancel_does_not_stop_its_replacement_or_another_window() {
        let runs = OverviewRuns::default();
        let current = Arc::new(AtomicBool::new(false));
        let other = Arc::new(AtomicBool::new(false));
        runs.0
            .lock()
            .unwrap()
            .insert("main".into(), ("new".into(), current.clone()));
        runs.0
            .lock()
            .unwrap()
            .insert("repo-1".into(), ("other".into(), other.clone()));
        runs.stop("main", Some("old"));
        assert!(!current.load(Ordering::Relaxed));
        runs.stop("main", None);
        assert!(current.load(Ordering::Relaxed));
        assert!(!other.load(Ordering::Relaxed));
    }

    #[test]
    fn many_repositories_never_exceed_three_simultaneous_reads() {
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let repositories = (0..40)
            .map(|i| serde_json::from_value(serde_json::json!({"path": format!("/repo-{i}"), "opened": 0})).unwrap())
            .collect();
        let rows = read_rows_with(repositories, &AtomicBool::new(false), |_, _| {
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(5));
            active.fetch_sub(1, Ordering::SeqCst);
            Err(oxbow_core::Error::Git("unavailable".into()))
        });
        assert_eq!(rows.len(), 40);
        assert!(peak.load(Ordering::SeqCst) <= 3);
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn one_bad_repository_does_not_drop_other_rows_and_cancelled_batches_are_empty() {
        let dir = std::env::temp_dir().join(format!("oxbow-overview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q", "-b", "main"])
                .arg(&dir)
                .status()
                .unwrap()
                .success()
        );
        let repositories = || {
            [dir.join("missing"), dir.clone()]
                .iter()
                .map(|path| {
                    serde_json::from_value(serde_json::json!({"path": path, "opened": 0, "name": "same"})).unwrap()
                })
                .collect()
        };
        let rows = read_rows(repositories(), &AtomicBool::new(false));
        assert_eq!(rows.len(), 2);
        assert!(rows[0].error.is_some() && rows[0].summary.is_none());
        assert!(rows[1].error.is_none() && rows[1].summary.is_some());
        assert!(read_rows(repositories(), &AtomicBool::new(true)).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
