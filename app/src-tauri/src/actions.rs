//! Running actions: planning one for its confirmation, performing it with Activity's
//! progress and Stop, and fetching in the background.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::Instant;

use oxbow_core::console;
use oxbow_core::remote::AutoFetchPlan;
use oxbow_core::{Action, Failure, Plan};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::CommandResult;
use crate::repos::tell_others;
use crate::session::{RepoRequest, Session, at, current, logged, read, workdir_key};
use crate::turns::{GiveWay, Running};

/// The git commands an action will run, for the confirmation sheet.
#[tauri::command]
pub(crate) async fn plan_action(
    window: RepoRequest,
    session: State<'_, Session>,
    action: Action,
) -> CommandResult<Plan> {
    let repo = current(&session, &window)?;
    read(&window, at(&repo), "Prepare an action", move || {
        let mut plan = repo.plan(&action)?;
        plan.not_kept = repo.not_kept(&action)?;
        Ok(plan)
    })
    .await
}

/// Run an action, sending each command and output line to the window as an `action-event`. A
/// failure comes back explained, so the sheet can offer the way out. With `without_undo` it
/// runs even when the state before it can't be saved for Undo, once that was chosen.
#[tauri::command]
pub(crate) async fn perform_action(
    app: AppHandle,
    window: RepoRequest,
    session: State<'_, Session>,
    action: Action,
    without_undo: Option<bool>,
) -> Result<String, Failure> {
    // Only the window that asked shows the output: Settings runs actions of its own.
    let from_settings = window.label() == "settings";
    let progress = window.clone();
    let failed = |message| Failure {
        kind: oxbow_core::FailureKind::Other,
        output: message,
        incoming: Vec::new(),
        remote_tip: None,
        hook: None,
        ssh: None,
        secrets: None,
        check: None,
        tracking: None,
    };
    let repo = current(&session, &window).map_err(failed)?;
    // Resolve aliases while the folder still exists, and keep tab-mode changes out of removal.
    let removing = match &action {
        Action::WorktreeRemove { path, .. } => Some(workdir_key(&repo.workdir().join(path))),
        _ => None,
    };
    let _topology = removing
        .as_ref()
        .map(|_| session.topology())
        .transpose()
        .map_err(failed)?;
    // Its own Stop, heard by the Stop of the window that asked until the run is over.
    let stop = if let Some(path) = &removing {
        let address = window
            .address
            .as_ref()
            .ok_or_else(|| failed("No repository is open".into()))?;
        session
            .start_worktree_remove(address, window.label(), path)
            .map_err(failed)?
    } else if matches!(action, Action::Fetch { .. }) {
        session.start_worktree_git(&window).map_err(failed)?
    } else {
        session.start_change(&window, window.label(), false).map_err(failed)?
    };
    let cancel = stop.cancel.clone();
    let events = app.clone();
    let turns = session.turns.clone();
    let turn_window = window.turn_key();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let _stop = stop;
        let title = repo.action_title(&action);
        // A change running in the repository goes first, from this window, Settings or another.
        let common = repo.common_dir();
        let change = Running::new(&turn_window, repo.name(), title.clone());
        let turn = turns.take(&common, change, &cancel, |other| {
            let event = oxbow_core::ActionEvent::Waiting {
                title: other.title.clone(),
                window: other.folder.clone(),
            };
            progress.progress("action-event", event);
        });
        let Some(_turn) = turn else {
            return Err(Failure {
                kind: oxbow_core::FailureKind::Cancelled,
                output: String::new(),
                incoming: Vec::new(),
                remote_tip: None,
                hook: None,
                ssh: None,
                secrets: None,
                check: None,
                tracking: None,
            });
        };
        if let Some(path) = &removing {
            // Git also accepts a worktree name or path suffix. Only remove the folder we reserved.
            let known = repo
                .worktree_list()
                .map_err(|err| repo.explain_failure(&action, &err))?;
            if !known.iter().any(|tree| workdir_key(Path::new(&tree.path)) == *path) {
                return Err(failed(format!(
                    "{} isn’t a worktree of this repository.",
                    path.display()
                )));
            }
        }
        let done = console::scope(Some(repo.workdir()), console::EntryKind::Action, title, || {
            let mut emit = |event| {
                progress.progress("action-event", event);
            };
            let result = if without_undo.unwrap_or(false) {
                repo.perform_without_undo(&action, &mut emit, &cancel)
            } else {
                repo.perform_with(&action, &mut emit, &cancel)
            };
            result.map_err(|err| repo.explain_failure(&action, &err))
        });
        let session = events.state::<Session>();
        if done.is_ok()
            && let Some(path) = &removing
        {
            crate::windows::worktree_removed(&events, &session, path);
        }
        // Done or not, it may have changed what the other windows show.
        tell_others(&events, &session, &repo, progress.address.as_ref());
        done
    })
    .await;
    // The repository window shows the remotes and branches Settings just changed.
    if from_settings && let Some(address) = &window.address {
        session.emit_repo(&app, address, "repo-touched", (), true);
    }
    joined.unwrap_or_else(|err| {
        Err(Failure {
            kind: oxbow_core::FailureKind::Other,
            output: err.to_string(),
            incoming: Vec::new(),
            remote_tip: None,
            hook: None,
            ssh: None,
            secrets: None,
            check: None,
            tracking: None,
        })
    })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PullSetup {
    mode: oxbow_core::PullMode,
    autostash: bool,
}

/// How Pull will go here, for the words of its confirmation.
#[tauri::command]
pub(crate) async fn pull_setup(window: RepoRequest, session: State<'_, Session>) -> CommandResult<PullSetup> {
    let repo = current(&session, &window)?;
    read(&window, at(&repo), "Read pull settings", move || {
        Ok(PullSetup {
            mode: repo.pull_mode(),
            autostash: repo.pull_autostash(),
        })
    })
    .await
}

/// A fetch in the background that hears nothing from the remote for this long has lost the
/// network: it gives up rather than hold the repository's turn, and the next one tries again.
const FETCH_QUIET_LIMIT: std::time::Duration = std::time::Duration::from_secs(2 * 60);

/// How a background fetch ended.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Fetched {
    /// Remote branches and tags that are new, moved or gone.
    updated: u32,
    /// It stopped part way for an action, as a fetch of the timer does.
    gave_way: bool,
    remote: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct FetchFailure {
    #[serde(flatten)]
    failure: Box<Failure>,
    /// Present only for a Git command that ran; null means all remotes.
    #[serde(skip_serializing_if = "Option::is_none")]
    remote: Option<Option<String>>,
}

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum FetchResult {
    Fetched(Fetched),
    Skipped {
        #[serde(rename = "retryAfterMs")]
        retry_after_ms: Option<u64>,
    },
}

#[cfg(test)]
#[test]
fn fetch_result_distinguishes_skips_from_completed_fetches() {
    use serde_json::{json, to_value};
    let fetched = FetchResult::Fetched(Fetched {
        updated: 1,
        gave_way: false,
        remote: None,
    });
    assert_eq!(
        to_value(fetched).unwrap(),
        json!({ "updated": 1, "gaveWay": false, "remote": null })
    );
    for retry_after_ms in [None, Some(5000)] {
        let skipped = FetchResult::Skipped { retry_after_ms };
        assert_eq!(to_value(skipped).unwrap(), json!({ "retryAfterMs": retry_after_ms }));
    }
}

/// What a background fetch printed, with the number Activity gave the fetch: a fetch of the
/// repository open before may still be printing while the next one runs.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FetchEvent {
    fetch: Option<u64>,
    remote: Option<String>,
    event: oxbow_core::ActionEvent,
}

/// Fetch one remote or all in the background: no sheet, git's progress goes to the window
/// as `fetch-event`s, and a failure comes back explained, for Activity. `fetch` is Activity's
/// number for it; without one (another view's quiet fetch) Stop doesn't reach it.
#[tauri::command]
pub(crate) async fn fetch_in_background(
    app: AppHandle,
    window: RepoRequest,
    session: State<'_, Session>,
    remote: Option<String>,
    fetch: Option<u64>,
    auto: Option<bool>,
) -> Result<FetchResult, FetchFailure> {
    let failure = |output: String| Failure {
        kind: oxbow_core::FailureKind::Other,
        output,
        incoming: Vec::new(),
        remote_tip: None,
        hook: None,
        ssh: None,
        secrets: None,
        check: None,
        tracking: None,
    };
    let failed = move |output: String| FetchFailure {
        failure: Box::new(failure(output)),
        remote: None,
    };
    let cancelled = move || FetchFailure {
        failure: Box::new(Failure {
            kind: oxbow_core::FailureKind::Cancelled,
            ..failure(String::new())
        }),
        remote: None,
    };
    let worktree_run = session.start_worktree_git(&window).map_err(failed)?;
    let (repo, cancel) = session
        .slot(&window, |slot| {
            let repo = slot
                .repo
                .clone()
                .ok_or_else(|| failed("No repository is open".to_owned()))?;
            let cancel = match fetch {
                Some(id) => slot.start_fetch(id).ok_or_else(cancelled)?,
                None => slot.close_cancel.clone(),
            };
            Ok::<_, FetchFailure>((repo, cancel))
        })
        .ok_or_else(|| failed("No repository is open".to_owned()))??;
    // A fetch of the timer stops for an action rather than make it wait.
    let gives_way = (auto == Some(true)).then(|| GiveWay::new(cancel.clone()));
    let progress = window.clone();
    let turn_window = window.turn_key();
    let turns = session.turns.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let _worktree_run = worktree_run;
        if cancel.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let repo = if auto == Some(true) {
            repo.reopen().map_err(|err| failed(err.to_string()))?
        } else {
            repo
        };
        let common = repo.common_dir();
        let (remote, command, mut reservation) = if auto == Some(true) {
            let Some(AutoFetchPlan {
                targets,
                remote,
                command,
            }) = repo.auto_fetch()
            else {
                return Ok(FetchResult::Skipped { retry_after_ms: None });
            };
            match turns.reserve_auto_fetch(&common, targets, Instant::now()) {
                Ok(reservation) => (remote, command, Some(reservation)),
                Err(retry) => {
                    return Ok(FetchResult::Skipped {
                        retry_after_ms: retry.map(|delay| delay.as_millis() as u64 + 1),
                    });
                }
            }
        } else {
            let command = repo.fetch_command(remote.as_deref());
            (remote, command, None)
        };
        let action = Action::Fetch { remote: remote.clone() };
        let (kind, title) = if auto == Some(true) {
            (console::EntryKind::Background, "Auto fetch".to_owned())
        } else {
            (
                console::EntryKind::Action,
                format!("Fetch {}", remote.as_deref().unwrap_or("all remotes")),
            )
        };
        let emit = |event| {
            progress.progress(
                "fetch-event",
                FetchEvent {
                    fetch,
                    remote: remote.clone(),
                    event,
                },
            );
        };
        // A change running in the repository goes first, from any window.
        let mut change = Running::new(&turn_window, repo.name(), title.clone());
        if let Some(way) = &gives_way {
            change = change.giving_way(way.clone());
        }
        let turn = turns.take(&common, change, &cancel, |other| {
            emit(oxbow_core::ActionEvent::Waiting {
                title: other.title.clone(),
                window: other.folder.clone(),
            })
        });
        let gave_way = || gives_way.as_ref().is_some_and(GiveWay::gave_way);
        let Some(_turn) = turn else {
            return Err(cancelled());
        };
        let mut updated = 0;
        let done = console::scope(Some(repo.workdir()), kind, title, || {
            let command = command.with_progress().quiet_limit(FETCH_QUIET_LIMIT);
            emit(oxbow_core::ActionEvent::Command {
                display: command.display(),
            });
            let started = Instant::now();
            let result = console::main(|| {
                repo.run_streaming(
                    &command,
                    &mut |line| {
                        if !line.progress && oxbow_core::remote::is_ref_update(&line.text) {
                            updated += 1;
                        }
                        emit(oxbow_core::ActionEvent::Line(line));
                    },
                    &cancel,
                )
            });
            if !matches!(result, Err(oxbow_core::Error::Cancelled))
                && let Some(reservation) = &mut reservation
            {
                reservation.attempted(started);
            }
            match result {
                Ok(_) => Ok(Fetched {
                    updated,
                    gave_way: false,
                    remote: remote.clone(),
                }),
                // Stopped for an action: the next fetch of the timer tries again.
                Err(oxbow_core::Error::Cancelled) if gave_way() => Ok(Fetched {
                    updated,
                    gave_way: true,
                    remote: remote.clone(),
                }),
                Err(err) => Err(FetchFailure {
                    failure: Box::new(repo.explain_failure(&action, &err)),
                    remote: Some(remote.clone()),
                }),
            }
        });
        if updated > 0 {
            tell_others(&app, &app.state::<Session>(), &repo, progress.address.as_ref());
        }
        done.map(FetchResult::Fetched)
    })
    .await
    .unwrap_or_else(|err| Err(failed(err.to_string())));
    if !session
        .slot(&window, |slot| slot.fetch_is_current(fetch))
        .unwrap_or(false)
    {
        return Err(cancelled());
    }
    result
}

/// Stop the background fetch.
#[tauri::command]
pub(crate) fn stop_fetch(window: RepoRequest, session: State<'_, Session>, fetch: u64) {
    session.slot(&window, |slot| slot.stop_fetch(fetch));
}

/// The public half of an SSH key, to paste into GitHub.
#[tauri::command]
pub(crate) fn public_key(key: String) -> CommandResult<String> {
    oxbow_core::ssh::public_key(&key).map_err(|err| err.to_string())
}

/// Ask the default remote which tags it has, so tags it lacks show as local. True when that
/// changed. Without a remote there is nothing to ask.
#[tauri::command]
pub(crate) async fn refresh_remote_tags(window: RepoRequest, session: State<'_, Session>) -> CommandResult<bool> {
    let repo = current(&session, &window)?;
    let worktree_run = session.start_worktree_git(&window)?;
    logged(
        Some(at(&repo)),
        console::EntryKind::Background,
        "Check the remote's tags",
        move || {
            let _worktree_run = worktree_run;
            match repo.default_remote() {
                Some(remote) => repo.refresh_remote_tags(&remote),
                None => Ok(false),
            }
        },
    )
    .await
}
