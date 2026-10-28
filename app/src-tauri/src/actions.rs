//! Running actions: planning one for its confirmation, performing it with Activity's
//! progress and Stop, and fetching in the background.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use oxbow_core::console;
use oxbow_core::{Action, Failure, Plan};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::CommandResult;
use crate::repos::tell_others;
use crate::session::{Session, at, current, logged, read};

/// The git commands an action will run, for the confirmation sheet.
#[tauri::command]
pub(crate) async fn plan_action(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    action: Action,
) -> CommandResult<Plan> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Prepare an action", move || {
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
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    action: Action,
    without_undo: Option<bool>,
) -> Result<String, Failure> {
    // Only the window that asked shows the output: Settings runs actions of its own.
    let label = window.label().to_owned();
    let from_settings = label == "settings";
    let repo_window = session.label(&window);
    let repo = current(&session, &window).map_err(|message| Failure {
        kind: oxbow_core::FailureKind::Other,
        output: message,
        incoming: Vec::new(),
        remote_tip: None,
        hook: None,
        ssh: None,
        secrets: None,
        tracking: None,
    })?;
    let cancel = session.cancel(&window);
    cancel.store(false, Ordering::Relaxed);
    let events = app.clone();
    let turns = session.turns.clone();
    let turn_window = repo_window.clone();
    let joined = tauri::async_runtime::spawn_blocking(move || {
        let title = repo.action_title(&action);
        // Another window of the repository changing it goes first.
        let common = repo.common_dir();
        let turn = turns.take(&common, &turn_window, repo.name(), title.clone(), &cancel, |other| {
            let event = oxbow_core::ActionEvent::Waiting {
                title: other.title.clone(),
                window: other.folder.clone(),
            };
            let _ = events.emit_to(label.as_str(), "action-event", event);
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
                tracking: None,
            });
        };
        let done = console::scope(Some(repo.workdir()), console::EntryKind::Action, title, || {
            let mut emit = |event| {
                let _ = events.emit_to(label.as_str(), "action-event", event);
            };
            let result = if without_undo.unwrap_or(false) {
                repo.perform_without_undo(&action, &mut emit, &cancel)
            } else {
                repo.perform_with(&action, &mut emit, &cancel)
            };
            result.map_err(|err| repo.explain_failure(&action, &err))
        });
        // Done or not, it may have changed what the other windows show.
        tell_others(&events, &events.state::<Session>(), &repo, &turn_window);
        done
    })
    .await;
    // The repository window shows the remotes and branches Settings just changed.
    if from_settings {
        let _ = app.emit_to(repo_window.as_str(), "repo-touched", ());
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
pub(crate) async fn pull_setup(window: tauri::WebviewWindow, session: State<'_, Session>) -> CommandResult<PullSetup> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read pull settings", move || {
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
}

/// What a background fetch printed, with the number Activity gave the fetch: a fetch of the
/// repository open before may still be printing while the next one runs.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FetchEvent {
    fetch: Option<u64>,
    event: oxbow_core::ActionEvent,
}

/// Fetch one remote or all in the background: no sheet, git's progress goes to the window
/// as `fetch-event`s, and a failure comes back explained, for Activity. `fetch` is Activity's
/// number for it; without one (another view's quiet fetch) Stop doesn't reach it.
#[tauri::command]
pub(crate) async fn fetch_in_background(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    remote: Option<String>,
    fetch: Option<u64>,
    auto: Option<bool>,
) -> Result<Fetched, Failure> {
    let failed = |output: String| Failure {
        kind: oxbow_core::FailureKind::Other,
        output,
        incoming: Vec::new(),
        remote_tip: None,
        hook: None,
        ssh: None,
        secrets: None,
        tracking: None,
    };
    let repo = current(&session, &window).map_err(failed)?;
    let cancel = if fetch.is_some() {
        session.new_fetch_cancel(&window)
    } else {
        Arc::new(AtomicBool::new(false))
    };
    let label = window.label().to_owned();
    let turn_window = session.label(&window);
    let turns = session.turns.clone();
    tauri::async_runtime::spawn_blocking(move || {
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
            let _ = app.emit_to(label.as_str(), "fetch-event", FetchEvent { fetch, event });
        };
        // Another window of the repository changing it goes first.
        let turn = turns.take(
            &repo.common_dir(),
            &turn_window,
            repo.name(),
            title.clone(),
            &cancel,
            |other| {
                emit(oxbow_core::ActionEvent::Waiting {
                    title: other.title.clone(),
                    window: other.folder.clone(),
                })
            },
        );
        let Some(_turn) = turn else {
            return Err(Failure {
                kind: oxbow_core::FailureKind::Cancelled,
                ..failed(String::new())
            });
        };
        let done = console::scope(Some(repo.workdir()), kind, title, || {
            let command = repo
                .fetch_command(remote.as_deref())
                .with_progress()
                .quiet_limit(FETCH_QUIET_LIMIT);
            emit(oxbow_core::ActionEvent::Command {
                display: command.display(),
            });
            let mut updated = 0;
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
            match result {
                Ok(_) => Ok(Fetched { updated }),
                Err(err) => Err(repo.explain_failure(&action, &err)),
            }
        });
        if done.as_ref().is_ok_and(|fetched| fetched.updated > 0) {
            tell_others(&app, &app.state::<Session>(), &repo, &turn_window);
        }
        done
    })
    .await
    .unwrap_or_else(|err| Err(failed(err.to_string())))
}

/// Stop the background fetch.
#[tauri::command]
pub(crate) fn stop_fetch(window: tauri::WebviewWindow, session: State<'_, Session>) {
    session.fetch_cancel(&window).store(true, Ordering::Relaxed);
}

/// The public half of an SSH key, to paste into GitHub.
#[tauri::command]
pub(crate) fn public_key(key: String) -> CommandResult<String> {
    oxbow_core::ssh::public_key(&key).map_err(|err| err.to_string())
}

/// Ask the default remote which tags it has, so tags it lacks show as local. True when that
/// changed. Without a remote there is nothing to ask.
#[tauri::command]
pub(crate) async fn refresh_remote_tags(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<bool> {
    let repo = current(&session, &window)?;
    logged(
        Some(at(&repo)),
        console::EntryKind::Background,
        "Check the remote's tags",
        move || match repo.default_remote() {
            Some(remote) => repo.refresh_remote_tags(&remote),
            None => Ok(false),
        },
    )
    .await
}
