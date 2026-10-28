//! The Pull Request screen's side: which GitHub repository the open one pushes to, its pull
//! requests, one of them in full, and the calls that change them, streamed to the sheet as they
//! run. Reviewing: the pull request fetched here, files viewed, the draft review.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use oxbow_core::console::EntryKind;
use oxbow_core::github::Client;
use oxbow_core::pulls::{Call, GitHubRepo, PullRequest, PullSummary, started_review};
use oxbow_core::review::{PullStatus, ReviewDraft, Viewing};
use oxbow_core::{ActionEvent, OutputLine};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::CommandResult;
use crate::accounts::Accounts;
use crate::session::{Session, at, blocking, current, logged, read};

fn github_repo_of(
    session: &State<'_, Session>,
    window: &tauri::WebviewWindow,
) -> CommandResult<(oxbow_core::Repo, GitHubRepo)> {
    let repo = current(session, window)?;
    let github = repo
        .github_repo()
        .ok_or_else(|| "This repository has no remote on github.com".to_owned())?;
    Ok((repo, github))
}

/// The GitHub repository the open one pushes to, if any. Asks nothing of GitHub.
#[tauri::command]
pub async fn github_repo(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Option<GitHubRepo>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Find the GitHub repository", move || Ok(repo.github_repo())).await
}

/// Its pull requests, open and recently closed, the latest updated first.
#[tauri::command]
pub async fn github_pulls(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    accounts: State<'_, Accounts>,
) -> CommandResult<Vec<PullSummary>> {
    let (_, github) = github_repo_of(&session, &window)?;
    let client = accounts.client(&app)?;
    blocking(move || client.pulls(&github)).await
}

/// One pull request in full; its commits that are here get their line counts.
#[tauri::command]
pub async fn github_pull(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    accounts: State<'_, Accounts>,
    number: u64,
) -> CommandResult<PullRequest> {
    let (repo, github) = github_repo_of(&session, &window)?;
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
pub async fn github_calls_preview(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    calls: Vec<Call>,
) -> CommandResult<Vec<String>> {
    let (_, github) = github_repo_of(&session, &window)?;
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
    let (_, github) = github_repo_of(&session, &window)?;
    let client = accounts.client(&app)?;
    let label = window.label().to_owned();
    let cancel = app.state::<Session>().cancel(&window);
    cancel.store(false, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |event: ActionEvent| {
            let _ = app.emit_to(label.as_str(), "action-event", event);
        };
        let mut created: Option<u64> = None;
        // A review this run started: what follows goes into it, and it goes again if they fail.
        let mut review: Option<String> = None;
        let unsent = |review: &Option<String>| {
            if let Some(id) = review {
                let _ = client.perform(
                    &github,
                    &Call::DeleteReview {
                        review: Some(id.clone()),
                    },
                    None,
                );
            }
        };
        let mut answer = Value::Null;
        for call in &calls {
            if cancel.load(Ordering::Relaxed) {
                unsent(&review);
                return Err("Stopped. Nothing else was sent.".to_owned());
            }
            let mut call = call.clone();
            if let Some(id) = &review {
                call.in_review(id);
            }
            let call = &call;
            emit(ActionEvent::Command {
                display: client.call_request(&github, call, created).display(),
            });
            let body = match client.perform(&github, call, created) {
                Ok(body) => body,
                Err(err) => {
                    unsent(&review);
                    return Err(err.to_string());
                }
            };
            if matches!(call, Call::StartReview { .. }) {
                review = started_review(&body);
            }
            if matches!(call, Call::SubmitReview { .. } | Call::DeleteReview { .. }) {
                review = None;
            }
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
pub async fn code_owners(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    base: String,
    branch: String,
) -> CommandResult<Vec<String>> {
    let repo = current(&session, &window)?;
    read(at(&repo), "Read the code owners", move || {
        Ok(repo.code_owners(&base, &branch))
    })
    .await
}

/// GitHub's verdict and checks for each open pull request, for the Reviews screen.
#[tauri::command]
pub async fn github_pull_statuses(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    accounts: State<'_, Accounts>,
) -> CommandResult<Vec<PullStatus>> {
    let (_, github) = github_repo_of(&session, &window)?;
    let client = accounts.client(&app)?;
    blocking(move || client.pull_statuses(&github)).await
}

/// Fetch pull request `number` and its base `base` here, quietly, to show its changes.
#[tauri::command]
pub async fn pull_fetch(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    number: u64,
    base: String,
) -> CommandResult<()> {
    let (repo, github) = github_repo_of(&session, &window)?;
    logged(
        Some(at(&repo)),
        EntryKind::Background,
        format!("Fetch pull request #{number}"),
        move || {
            repo.run(&repo.pull_fetch_command(&github.remote, number, &base))
                .map(|_| ())
        },
    )
    .await
}

/// The files of pull request `number` this account marked viewed, and its review there not sent.
#[tauri::command]
pub async fn github_viewing(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    accounts: State<'_, Accounts>,
    number: u64,
) -> CommandResult<Viewing> {
    let (_, github) = github_repo_of(&session, &window)?;
    let client = accounts.client(&app)?;
    blocking(move || client.viewing(&github, number)).await
}

/// Send one small call quietly, like ticking Viewed: no sheet, no Activity.
#[tauri::command]
pub async fn github_send(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    accounts: State<'_, Accounts>,
    call: Call,
) -> CommandResult<Value> {
    let (_, github) = github_repo_of(&session, &window)?;
    let client = accounts.client(&app)?;
    blocking(move || client.perform(&github, &call, None)).await
}

/// The draft review of pull request `number` kept here, if any.
#[tauri::command]
pub async fn review_draft(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    number: u64,
) -> CommandResult<Option<ReviewDraft>> {
    let (repo, github) = github_repo_of(&session, &window)?;
    blocking(move || repo.review_draft(&github, number)).await
}

/// Every draft review kept here, by pull request number.
#[tauri::command]
pub async fn review_drafts(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<HashMap<u64, ReviewDraft>> {
    let (repo, github) = github_repo_of(&session, &window)?;
    blocking(move || Ok(repo.review_drafts(&github))).await
}

/// Keep the draft review of pull request `number`; with none, put it away (sent or thrown away).
#[tauri::command]
pub async fn save_review_draft(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    number: u64,
    draft: Option<ReviewDraft>,
) -> CommandResult<()> {
    let (repo, github) = github_repo_of(&session, &window)?;
    blocking(move || match draft {
        Some(draft) => repo.save_review_draft(&github, number, &draft),
        None => repo.drop_review_draft(&github, number),
    })
    .await
}
