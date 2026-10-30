//! The Pull Request screen's side: which GitHub repository the open one pushes to, its pull
//! requests, one of them in full, and the calls that change them, streamed to the sheet as they
//! run. Reviewing: the pull request fetched here, files viewed, the draft review. GitLab: the
//! projects the remotes point at, and their merge request pages in the browser.

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use oxbow_core::console::EntryKind;
use oxbow_core::github::Client;
use oxbow_core::gitlab_api::{GitLabUser, Step};
use oxbow_core::pulls::{Call, GitHubRepo, PullRequest, PullSummary, started_review};
use oxbow_core::review::{PullStatus, ReviewDraft, ReviewDraftState, Reviewed, Viewing};
use oxbow_core::{ActionEvent, GitLabProject, OutputLine};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::CommandResult;
use crate::accounts::Accounts;
use crate::session::{Session, at, blocking, current, logged, read};

/// Captured by the editor, never inferred from whichever repository the window opens next.
#[derive(Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewTarget {
    workdir: String,
    place: Reviewed,
}

fn review_repo(target: &ReviewTarget) -> CommandResult<oxbow_core::Repo> {
    target.place.validate().map_err(|err| err.to_string())?;
    let repo = oxbow_core::Repo::open(&target.workdir).map_err(|err| err.to_string())?;
    if repo.workdir() != std::path::Path::new(&target.workdir) {
        return Err("The review's working folder moved. Reopen the repository before saving.".into());
    }
    Ok(repo)
}

fn check_review_target(repo: &oxbow_core::Repo, place: &Reviewed, target: Option<&ReviewTarget>) -> CommandResult<()> {
    if let Some(target) = target
        && (repo.workdir() != std::path::Path::new(&target.workdir) || place != &target.place)
    {
        return Err("The review's repository or project changed. Reopen the review.".into());
    }
    Ok(())
}

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

/// The GitLab projects the remotes point at, the default remote's first. Asks nothing of GitLab.
#[tauri::command]
pub async fn gitlab_projects(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Vec<GitLabProject>> {
    let repo = current(&session, &window)?;
    let servers = crate::gitlab_accounts::servers(&app);
    read(at(&repo), "Find the GitLab projects", move || {
        Ok(repo.gitlab_projects(&servers))
    })
    .await
}

/// The merge requests of a GitLab project.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabList {
    project: GitLabProject,
    merge_requests: Vec<PullSummary>,
}

/// The repository's GitLab project on a server Oxbow is signed in to (the default remote's
/// first), and a client signed in there; `None` without a sign-in.
async fn signed_in_project(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    session: &State<'_, Session>,
    tokens: &State<'_, crate::gitlab_accounts::GitLabTokens>,
) -> CommandResult<Option<(oxbow_core::Repo, GitLabProject, oxbow_core::gitlab_api::Client)>> {
    let repo = current(session, window)?;
    let servers = crate::gitlab_accounts::servers(app);
    let found = servers.clone();
    let here = repo.clone();
    let projects = read(at(&repo), "Find the GitLab projects", move || {
        Ok(here.gitlab_projects(&found))
    })
    .await?;
    let Some(project) = projects.into_iter().find(|p| servers.iter().any(|s| s.host == p.host)) else {
        return Ok(None);
    };
    let client = tokens.client(app, &project.host)?;
    Ok(Some((repo, project, client)))
}

/// The merge requests of the repository's GitLab project on a server Oxbow is signed in to (the
/// default remote's first); none without a sign-in, where Create Merge Request pushes instead.
#[tauri::command]
pub async fn gitlab_merge_requests(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    tokens: State<'_, crate::gitlab_accounts::GitLabTokens>,
) -> CommandResult<Option<GitLabList>> {
    let Some((_, project, client)) = signed_in_project(&app, &window, &session, &tokens).await? else {
        return Ok(None);
    };
    blocking(move || {
        let merge_requests = client.merge_requests(&project)?;
        Ok(Some(GitLabList {
            project,
            merge_requests,
        }))
    })
    .await
}

/// Merge request `!number` in full, as a pull request; what its commits here tell (line counts,
/// the code under comments) is filled in.
#[tauri::command]
pub async fn gitlab_merge_request(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    tokens: State<'_, crate::gitlab_accounts::GitLabTokens>,
    number: u64,
) -> CommandResult<PullRequest> {
    let (repo, project, client) = signed_in_project(&app, &window, &session, &tokens)
        .await?
        .ok_or_else(|| "Sign in to this repository’s GitLab in Settings › Accounts".to_owned())?;
    blocking(move || {
        let mut mr = client.merge_request(&project, number)?;
        repo.merge_request_here(&mut mr);
        Ok(mr)
    })
    .await
}

/// Fetch merge request `!number` and its target `base` here, quietly, to show its changes.
#[tauri::command]
pub async fn gitlab_fetch(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    tokens: State<'_, crate::gitlab_accounts::GitLabTokens>,
    number: u64,
    base: String,
    target: Option<ReviewTarget>,
) -> CommandResult<()> {
    let (repo, project, _) = signed_in_project(&app, &window, &session, &tokens)
        .await?
        .ok_or_else(|| "Sign in to this repository’s GitLab in Settings › Accounts".to_owned())?;
    let place = Reviewed::gitlab(&project);
    check_review_target(&repo, &place, target.as_ref())?;
    logged(
        Some(at(&repo)),
        EntryKind::Background,
        format!("Fetch merge request !{number}"),
        move || {
            repo.run(&repo.merge_request_fetch_command(&place, &project.remote, number, &base))
                .map(|_| ())
        },
    )
    .await
}

/// The requests `calls` send to GitLab, as curl, for the sheet.
#[tauri::command]
pub async fn gitlab_calls_preview(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    tokens: State<'_, crate::gitlab_accounts::GitLabTokens>,
    calls: Vec<Call>,
    target: Option<ReviewTarget>,
) -> CommandResult<Vec<String>> {
    let (repo, project, client) = signed_in_project(&app, &window, &session, &tokens)
        .await?
        .ok_or_else(|| "Sign in to this repository’s GitLab in Settings › Accounts".to_owned())?;
    check_review_target(&repo, &Reviewed::gitlab(&project), target.as_ref())?;
    client.preview(&project, &calls)
}

/// Send `calls` to GitLab in order, each shown in the sheet as it goes. Answers with what the
/// last one returned: the merge request as it is now.
#[tauri::command]
pub async fn gitlab_run(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    tokens: State<'_, crate::gitlab_accounts::GitLabTokens>,
    calls: Vec<Call>,
    target: Option<ReviewTarget>,
) -> CommandResult<Value> {
    let (repo, project, client) = signed_in_project(&app, &window, &session, &tokens)
        .await?
        .ok_or_else(|| "Sign in to this repository’s GitLab in Settings › Accounts".to_owned())?;
    check_review_target(&repo, &Reviewed::gitlab(&project), target.as_ref())?;
    let label = window.label().to_owned();
    // Dropped once the run is over: Stop no longer reaches it.
    let stop = app.state::<Session>().new_stop(&window);
    let cancel = stop.cancel.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let stopped = || cancel.load(Ordering::Relaxed);
        client.run(&project, &calls, &stopped, &mut |step| {
            let event = match step {
                Step::Request(display) => ActionEvent::Command { display },
                Step::Done(said) => ActionEvent::Line(OutputLine {
                    text: said.unwrap_or_else(|| "Done.".to_owned()),
                    stderr: false,
                    progress: false,
                }),
            };
            let _ = app.emit_to(label.as_str(), "action-event", event);
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

/// What the form that opens a merge request needs: GitLab's labels and settings for the project,
/// the account's id, and the merge request templates on `rev` (the trunk, where GitLab reads them).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabCreateFacts {
    #[serde(flatten)]
    facts: oxbow_core::gitlab_api::CreateFacts,
    templates: Vec<Template>,
}

#[derive(serde::Serialize)]
pub struct Template {
    name: String,
    text: String,
}

#[tauri::command]
pub async fn gitlab_create_facts(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    tokens: State<'_, crate::gitlab_accounts::GitLabTokens>,
    rev: String,
) -> CommandResult<GitLabCreateFacts> {
    let (repo, project, client) = signed_in_project(&app, &window, &session, &tokens)
        .await?
        .ok_or_else(|| "Sign in to this repository’s GitLab in Settings › Accounts".to_owned())?;
    blocking(move || {
        let facts = client.create_facts(&project)?;
        let templates = repo
            .merge_request_templates(&rev)
            .into_iter()
            .map(|(name, text)| Template { name, text })
            .collect();
        Ok(GitLabCreateFacts { facts, templates })
    })
    .await
}

/// Who GitLab's CODEOWNERS asks to review what `branch` changes against `base`, as accounts:
/// groups and names GitLab has no account for are left out.
#[tauri::command]
pub async fn gitlab_code_owners(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    tokens: State<'_, crate::gitlab_accounts::GitLabTokens>,
    base: String,
    branch: String,
) -> CommandResult<Vec<GitLabUser>> {
    let (repo, _, client) = signed_in_project(&app, &window, &session, &tokens)
        .await?
        .ok_or_else(|| "Sign in to this repository’s GitLab in Settings › Accounts".to_owned())?;
    blocking(move || {
        let names = repo.code_owners(&base, &branch, true);
        Ok(if names.is_empty() {
            Vec::new()
        } else {
            client.users_named(&names)
        })
    })
    .await
}

/// People who can see the project, by part of their name or username.
#[tauri::command]
pub async fn gitlab_people(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    tokens: State<'_, crate::gitlab_accounts::GitLabTokens>,
    search: String,
) -> CommandResult<Vec<GitLabUser>> {
    let (_, project, client) = signed_in_project(&app, &window, &session, &tokens)
        .await?
        .ok_or_else(|| "Sign in to this repository’s GitLab in Settings › Accounts".to_owned())?;
    blocking(move || client.people(&project, &search)).await
}

/// GitLab's say on the open merge requests `numbers`, for the Reviews screen.
#[tauri::command]
pub async fn gitlab_statuses(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    tokens: State<'_, crate::gitlab_accounts::GitLabTokens>,
    numbers: Vec<u64>,
) -> CommandResult<Vec<PullStatus>> {
    let Some((_, project, client)) = signed_in_project(&app, &window, &session, &tokens).await? else {
        return Ok(Vec::new());
    };
    blocking(move || Ok(client.statuses(&project, &numbers))).await
}

/// Open a page of one of the repository's GitLab projects in the browser: a merge request's, or
/// a pipeline's or job's from the screen's checks.
#[tauri::command]
pub async fn open_gitlab(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    url: String,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    let servers = crate::gitlab_accounts::servers(&app);
    let projects = read(at(&repo), "Find the GitLab projects", move || {
        Ok(repo.gitlab_projects(&servers))
    })
    .await?;
    let pages = ["merge_requests", "pipelines", "jobs"];
    if !projects.iter().any(|p| {
        pages
            .iter()
            .any(|page| url.starts_with(&format!("{}/-/{page}/", p.web_url)))
    }) {
        return Err(
            "Only merge request, pipeline and job pages of this repository's GitLab projects open from here".into(),
        );
    }
    crate::open_in::open_url(&url)
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
    target: Option<ReviewTarget>,
) -> CommandResult<Vec<String>> {
    let (repo, github) = github_repo_of(&session, &window)?;
    check_review_target(&repo, &Reviewed::github(&github), target.as_ref())?;
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
    target: Option<ReviewTarget>,
) -> CommandResult<Value> {
    let (repo, github) = github_repo_of(&session, &window)?;
    check_review_target(&repo, &Reviewed::github(&github), target.as_ref())?;
    let client = accounts.client(&app)?;
    let label = window.label().to_owned();
    // Dropped once the run is over: Stop no longer reaches it.
    let stop = app.state::<Session>().new_stop(&window);
    let cancel = stop.cancel.clone();
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
        Ok(repo.code_owners(&base, &branch, false))
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
    target: Option<ReviewTarget>,
) -> CommandResult<()> {
    let (repo, github) = github_repo_of(&session, &window)?;
    let place = Reviewed::github(&github);
    check_review_target(&repo, &place, target.as_ref())?;
    logged(
        Some(at(&repo)),
        EntryKind::Background,
        format!("Fetch pull request #{number}"),
        move || {
            repo.run(&repo.pull_fetch_command(&place, &github.remote, number, &base))
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
    target: Option<ReviewTarget>,
) -> CommandResult<Viewing> {
    let (repo, github) = github_repo_of(&session, &window)?;
    check_review_target(&repo, &Reviewed::github(&github), target.as_ref())?;
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
    target: Option<ReviewTarget>,
) -> CommandResult<Value> {
    let (repo, github) = github_repo_of(&session, &window)?;
    check_review_target(&repo, &Reviewed::github(&github), target.as_ref())?;
    let client = accounts.client(&app)?;
    blocking(move || client.perform(&github, &call, None)).await
}

/// Where the reviews of the open repository's pull requests are kept, or with `gitlab` those of
/// its merge requests on the GitLab it is signed in to.
async fn reviewed_place(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    session: &State<'_, Session>,
    tokens: &State<'_, crate::gitlab_accounts::GitLabTokens>,
    gitlab: bool,
) -> CommandResult<(oxbow_core::Repo, Reviewed)> {
    if !gitlab {
        let (repo, github) = github_repo_of(session, window)?;
        return Ok((repo, Reviewed::github(&github)));
    }
    let (repo, project, _) = signed_in_project(app, window, session, tokens)
        .await?
        .ok_or_else(|| "Sign in to this repository’s GitLab in Settings › Accounts".to_owned())?;
    Ok((repo, Reviewed::gitlab(&project)))
}

/// The draft review of pull request `number` kept here, if any.
#[tauri::command]
pub async fn review_draft(target: ReviewTarget, number: u64) -> CommandResult<ReviewDraftState> {
    blocking(move || {
        let repo = review_repo(&target).map_err(oxbow_core::Error::Git)?;
        repo.review_draft_state(&target.place, number)
    })
    .await
}

/// Every draft review kept here, by pull request number.
#[tauri::command]
pub async fn review_drafts(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    tokens: State<'_, crate::gitlab_accounts::GitLabTokens>,
    gitlab: Option<bool>,
) -> CommandResult<HashMap<u64, ReviewDraft>> {
    let (repo, place) = reviewed_place(&app, &window, &session, &tokens, gitlab.unwrap_or(false)).await?;
    blocking(move || Ok(repo.review_drafts(&place))).await
}

/// Keep the draft review of pull request `number`; with none, put it away (sent or thrown away).
#[tauri::command]
pub async fn save_review_draft(
    target: ReviewTarget,
    number: u64,
    draft: Option<ReviewDraft>,
    expected: String,
) -> CommandResult<String> {
    blocking(move || {
        let repo = review_repo(&target).map_err(oxbow_core::Error::Git)?;
        repo.update_review_draft(&target.place, number, draft.as_ref(), &expected)
    })
    .await
}

/// The files of merge request `number` ticked Viewed: GitLab keeps no ticks, so they are kept
/// here.
#[tauri::command]
pub async fn gitlab_viewing(target: ReviewTarget, number: u64) -> CommandResult<Viewing> {
    blocking(move || {
        let repo = review_repo(&target).map_err(oxbow_core::Error::Git)?;
        Ok(repo.viewed_here(&target.place, number))
    })
    .await
}

/// Tick or untick a file of merge request `number` Viewed, here.
#[tauri::command]
pub async fn gitlab_set_viewed(target: ReviewTarget, number: u64, path: String, viewed: bool) -> CommandResult<()> {
    blocking(move || {
        let repo = review_repo(&target).map_err(oxbow_core::Error::Git)?;
        repo.set_viewed_here(&target.place, number, &path, viewed)
    })
    .await
}

#[cfg(test)]
mod review_target_tests {
    use super::*;

    #[test]
    fn a_review_keeps_its_working_folder_and_project_when_the_window_switches() {
        let dir = oxbow_core::cli::private_temp_folder("oxbow-review-target-").unwrap();
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        for path in [&a, &b] {
            std::fs::create_dir(path).unwrap();
            assert!(
                std::process::Command::new("git")
                    .args(["init", "-q"])
                    .arg(path)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let first = oxbow_core::Repo::open(&a).unwrap();
        let other = oxbow_core::Repo::open(&b).unwrap();
        let place = Reviewed::github(&GitHubRepo {
            owner: "acme".into(),
            name: "api".into(),
            remote: "origin".into(),
        });
        let target = ReviewTarget {
            workdir: first.workdir().display().to_string(),
            place: place.clone(),
        };
        assert_eq!(review_repo(&target).unwrap().workdir(), first.workdir());
        assert!(check_review_target(&other, &place, Some(&target)).is_err());
        let different = Reviewed::github(&GitHubRepo {
            owner: "other".into(),
            name: "api".into(),
            remote: "origin".into(),
        });
        assert!(check_review_target(&first, &different, Some(&target)).is_err());
        assert!(check_review_target(&first, &place, Some(&target)).is_ok());
    }
}
