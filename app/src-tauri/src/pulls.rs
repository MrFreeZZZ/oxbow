//! The Pull Request screen's side: which GitHub repository the open one pushes to, its pull
//! requests, one of them in full, and the calls that change them, streamed to the sheet as they
//! run.

use std::sync::atomic::Ordering;

use oxbow_core::github::Client;
use oxbow_core::pulls::{Call, GitHubRepo, PullRequest, PullSummary};
use oxbow_core::{ActionEvent, OutputLine};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::accounts::Accounts;
use crate::{CommandResult, Session, blocking, current};

fn github_repo_of(session: &State<'_, Session>) -> CommandResult<(oxbow_core::Repo, GitHubRepo)> {
    let repo = current(session)?;
    let github = repo
        .github_repo()
        .ok_or_else(|| "This repository has no remote on github.com".to_owned())?;
    Ok((repo, github))
}

/// The GitHub repository the open one pushes to, if any. Asks nothing of GitHub.
#[tauri::command]
pub async fn github_repo(session: State<'_, Session>) -> CommandResult<Option<GitHubRepo>> {
    let repo = current(&session)?;
    blocking(move || Ok(repo.github_repo())).await
}

/// Its pull requests, open and recently closed, the latest updated first.
#[tauri::command]
pub async fn github_pulls(
    app: AppHandle,
    session: State<'_, Session>,
    accounts: State<'_, Accounts>,
) -> CommandResult<Vec<PullSummary>> {
    let (_, github) = github_repo_of(&session)?;
    let client = accounts.client(&app)?;
    blocking(move || client.pulls(&github)).await
}

/// One pull request in full; its commits that are here get their line counts.
#[tauri::command]
pub async fn github_pull(
    app: AppHandle,
    session: State<'_, Session>,
    accounts: State<'_, Accounts>,
    number: u64,
) -> CommandResult<PullRequest> {
    let (repo, github) = github_repo_of(&session)?;
    let client = accounts.client(&app)?;
    blocking(move || {
        let mut pull = client.pull_request(&github, number)?;
        let shas: Vec<String> = pull.commits.iter().map(|c| c.sha.clone()).collect();
        let stats = repo.commit_stats(&shas);
        for commit in &mut pull.commits {
            if let Some((add, del)) = stats.get(&commit.sha) {
                commit.additions = Some(*add);
                commit.deletions = Some(*del);
            }
        }
        Ok(pull)
    })
    .await
}

/// The requests `calls` send, as curl, for the sheet.
#[tauri::command]
pub async fn github_calls_preview(session: State<'_, Session>, calls: Vec<Call>) -> CommandResult<Vec<String>> {
    let (_, github) = github_repo_of(&session)?;
    let client = Client::default();
    Ok(calls
        .iter()
        .map(|call| client.call_request(&github, call, None).display())
        .collect())
}

/// Send `calls` in order, each shown in the sheet as it goes. Answers with what the last one
/// returned, or the pull request a `Create` made.
#[tauri::command]
pub async fn github_run(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    accounts: State<'_, Accounts>,
    calls: Vec<Call>,
) -> CommandResult<Value> {
    let (_, github) = github_repo_of(&session)?;
    let client = accounts.client(&app)?;
    let label = window.label().to_owned();
    let cancel = app.state::<Session>().cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |event: ActionEvent| {
            let _ = app.emit_to(label.as_str(), "action-event", event);
        };
        let mut created: Option<u64> = None;
        let mut answer = Value::Null;
        for call in &calls {
            if cancel.load(Ordering::Relaxed) {
                return Err("Stopped. Nothing else was sent.".to_owned());
            }
            emit(ActionEvent::Command {
                display: client.call_request(&github, call, created).display(),
            });
            let body = client.perform(&github, call, created).map_err(|err| err.to_string())?;
            let said = match call {
                Call::Create { .. } => {
                    created = body.get("number").and_then(Value::as_u64);
                    body.get("html_url")
                        .and_then(Value::as_str)
                        .map(|url| format!("Opened {url}"))
                }
                Call::Merge { .. } => body.get("message").and_then(Value::as_str).map(str::to_owned),
                _ => None,
            };
            emit(ActionEvent::Line(OutputLine {
                text: said.unwrap_or_else(|| "Done.".to_owned()),
                stderr: false,
                progress: false,
            }));
            if matches!(call, Call::Create { .. }) || created.is_none() {
                answer = body;
            }
        }
        Ok(answer)
    })
    .await
    .map_err(|err| err.to_string())?
}

/// Who CODEOWNERS asks to review what `branch` changes against `base`.
#[tauri::command]
pub async fn code_owners(session: State<'_, Session>, base: String, branch: String) -> CommandResult<Vec<String>> {
    let repo = current(&session)?;
    blocking(move || Ok(repo.code_owners(&base, &branch))).await
}
