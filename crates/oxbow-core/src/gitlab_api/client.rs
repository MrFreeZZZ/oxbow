//! GitLab REST requests, transport and mapping into the shared pull request models.

use serde_json::{Value, json};

use super::types::*;
use crate::error::{Error, Result};
use crate::gitlab::GitLabProject;
use crate::pulls::{
    Call, Check, Comment, Entry, Loaded, Method, Person, PullCommit, PullList, PullRequest, PullState, PullSummary,
    RepoSettings, Review, Verdict, loaded_read, optional_read, parse_time, read_pages, text,
};
use crate::review::PullStatus;

/// An HTTP request, kept to show as `curl` before it is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub json: Option<Value>,
}

impl Request {
    /// The request as `curl` would send it; the token shows as `$GITLAB_TOKEN`.
    pub fn display(&self) -> String {
        let mut parts = vec!["curl".to_owned()];
        if self.method != "GET" {
            parts.push(format!("-X {}", self.method));
        }
        parts.push("-H \"PRIVATE-TOKEN: $GITLAB_TOKEN\"".to_owned());
        parts.push(self.url.clone());
        if let Some(json) = &self.json {
            parts.push(format!("-d {}", crate::cli::shell_quote(&json.to_string())));
        }
        parts.join(" ")
    }
}

/// A connection to one GitLab server.
#[derive(Clone)]
pub struct Client {
    server: Server,
    api: String,
    token: Option<String>,
    agent: ureq::Agent,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("api", &self.api)
            .field("signed_in", &self.token.is_some())
            .finish()
    }
}

impl Client {
    pub fn new(server: Server) -> Self {
        Client {
            api: format!("{}/api/v4", server.web),
            server,
            token: None,
            cancel: None,
            agent: crate::github::agent(),
        }
    }

    /// Stop further pages when the window changes repository or starts a newer read.
    pub fn with_cancel(mut self, cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.cancel = Some(cancel);
        self
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn server(&self) -> &Server {
        &self.server
    }

    // Requests, to show and then send.

    pub fn user_request(&self) -> Request {
        self.api_request("GET", "/user", None)
    }

    pub fn token_request(&self) -> Request {
        self.api_request("GET", "/personal_access_tokens/self", None)
    }

    pub fn version_request(&self) -> Request {
        self.api_request("GET", "/version", None)
    }

    /// The request `call` sends to merge request `!number`. Reviewing, which can take several
    /// or none, goes through [`Client::call_requests`]; Viewed, which GitLab doesn't keep, is
    /// an error.
    pub fn call_request(&self, project: &GitLabProject, call: &Call) -> std::result::Result<Request, String> {
        let mr = |n: &u64| format!("/projects/{}/merge_requests/{n}", project_id(project));
        let update = |n: &u64, json: Value| self.api_request("PUT", &mr(n), Some(json));
        Ok(match call {
            Call::Create {
                head,
                base,
                title,
                body,
                draft,
                gitlab,
            } => {
                let mut json = json!({
                    "source_branch": head,
                    "target_branch": base,
                    "title": draft_title(title, *draft),
                    "description": body,
                });
                let extra = gitlab.clone().unwrap_or_default();
                if !extra.reviewer_ids.is_empty() {
                    json["reviewer_ids"] = json!(extra.reviewer_ids);
                }
                if !extra.assignee_ids.is_empty() {
                    json["assignee_ids"] = json!(extra.assignee_ids);
                }
                if !extra.labels.is_empty() {
                    json["labels"] = json!(extra.labels.join(","));
                }
                if extra.remove_source_branch {
                    json["remove_source_branch"] = json!(true);
                }
                if extra.squash {
                    json["squash"] = json!(true);
                }
                self.api_request(
                    "POST",
                    &format!("/projects/{}/merge_requests", project_id(project)),
                    Some(json),
                )
            }
            // GitLab takes the whole list, by account id: the screen adds to the ones it has.
            Call::RequestReviewers { number, ids, .. } => update(number, json!({ "reviewer_ids": ids })),
            Call::Merge {
                number,
                method,
                sha,
                delete_branch,
                ..
            } => {
                // GitLab writes the message from the project's templates, as its Merge button does.
                let mut body = json!({ "sha": sha, "squash": *method == Method::Squash });
                if *delete_branch {
                    body["should_remove_source_branch"] = json!(true);
                }
                self.api_request("PUT", &format!("{}/merge", mr(number)), Some(body))
            }
            Call::AutoMerge { number, method, .. } => match method {
                // `auto_merge` from GitLab 17.11, `merge_when_pipeline_succeeds` before; each
                // version ignores the other.
                Some(method) => self.api_request(
                    "PUT",
                    &format!("{}/merge", mr(number)),
                    Some(json!({ "auto_merge": true, "merge_when_pipeline_succeeds": true, "squash": *method == Method::Squash })),
                ),
                None => self.api_request("POST", &format!("{}/cancel_merge_when_pipeline_succeeds", mr(number)), None),
            },
            Call::UpdateBranch { number, .. } => self.api_request("PUT", &format!("{}/rebase", mr(number)), None),
            Call::ReadyForReview { number, title, .. } | Call::ConvertToDraft { number, title, .. } => {
                let draft = matches!(call, Call::ConvertToDraft { .. });
                let title = title.as_deref().ok_or("GitLab marks a draft by its title, and none was given")?;
                update(number, json!({ "title": draft_title(title, draft) }))
            }
            Call::Update { number, base, body } => {
                let mut json = json!({});
                if let Some(base) = base {
                    json["target_branch"] = json!(base);
                }
                if let Some(body) = body {
                    json["description"] = json!(body);
                }
                update(number, json)
            }
            Call::SetState { number, open } => update(
                number,
                json!({ "state_event": if *open { "reopen" } else { "close" } }),
            ),
            _ => return Err("Oxbow can’t do this on GitLab yet".to_owned()),
        })
    }

    /// The requests `call` sends, in order: most send one; reviewing sends none to start a
    /// review (GitLab's drafts need no start) and several to finish it. Comments on lines need
    /// the `refs` of the version of the diff they were written on.
    pub fn call_requests(
        &self,
        project: &GitLabProject,
        call: &Call,
        refs: Option<&DiffRefs>,
    ) -> std::result::Result<Vec<Request>, String> {
        let mr = |n: u64| format!("/projects/{}/merge_requests/{n}", project_id(project));
        // A review here is the merge request it is of, by number.
        let of = |review: &Option<String>| -> std::result::Result<u64, String> {
            review
                .as_deref()
                .and_then(|r| r.parse().ok())
                .ok_or_else(|| "The review isn’t started".to_owned())
        };
        let refs =
            || refs.ok_or_else(|| "GitLab no longer has the version of the diff these comments are on".to_owned());
        Ok(match call {
            Call::StartReview { .. } | Call::DeleteReview { .. } => Vec::new(),
            Call::Comment { number, spot, body, .. } => vec![self.api_request(
                "POST",
                &format!("{}/discussions", mr(*number)),
                Some(json!({ "body": body, "position": position(refs()?, spot) })),
            )],
            Call::Reply {
                number, thread, body, ..
            } => {
                let thread = thread.as_deref().ok_or("GitLab didn’t name this thread")?;
                vec![self.api_request(
                    "POST",
                    &format!("{}/discussions/{thread}/notes", mr(*number)),
                    Some(json!({ "body": body })),
                )]
            }
            Call::Resolve {
                number,
                thread,
                resolved,
            } => vec![self.api_request(
                "PUT",
                &format!("{}/discussions/{thread}", mr(*number)),
                Some(json!({ "resolved": resolved })),
            )],
            Call::ReviewComment { review, spot, body } => vec![self.api_request(
                "POST",
                &format!("{}/draft_notes", mr(of(review)?)),
                Some(json!({ "note": body, "position": position(refs()?, spot) })),
            )],
            Call::ReviewReply { review, thread, body } => vec![self.api_request(
                "POST",
                &format!("{}/draft_notes", mr(of(review)?)),
                Some(json!({ "note": body, "in_reply_to_discussion_id": thread })),
            )],
            Call::SubmitReview {
                review,
                verdict,
                body,
                revoke,
            } => {
                let at = mr(of(review)?);
                let mut out = Vec::new();
                // The summary goes as a draft of its own, so a GitLab before 17 keeps it too.
                if !body.trim().is_empty() {
                    out.push(self.api_request("POST", &format!("{at}/draft_notes"), Some(json!({ "note": body }))));
                }
                let state = match verdict {
                    Verdict::RequestChanges => json!({ "reviewer_state": "requested_changes" }),
                    _ => json!({ "reviewer_state": "reviewed" }),
                };
                out.push(self.api_request("POST", &format!("{at}/draft_notes/bulk_publish"), Some(state)));
                if *verdict == Verdict::Approve {
                    out.push(self.api_request("POST", &format!("{at}/approve"), None));
                } else if *revoke {
                    out.push(self.api_request("POST", &format!("{at}/unapprove"), None));
                }
                out
            }
            _ => vec![self.call_request(project, call)?],
        })
    }

    /// Throw away a draft comment a review made, when the review couldn't be sent.
    fn delete_draft_request(&self, project: &GitLabProject, number: u64, draft: u64) -> Request {
        self.api_request(
            "DELETE",
            &format!(
                "/projects/{}/merge_requests/{number}/draft_notes/{draft}",
                project_id(project)
            ),
            None,
        )
    }

    /// Send `call`; GitLab answers with the merge request as it is now, or with what the last
    /// request of several made.
    pub fn perform(&self, project: &GitLabProject, call: &Call, refs: Option<&DiffRefs>) -> Result<Value> {
        let requests = self
            .call_requests(project, call, refs)
            .map_err(|message| gitlab(None, message))?;
        let mut answer = Value::Null;
        for request in &requests {
            answer = self.call(request)?;
        }
        Ok(answer)
    }

    /// The requests `calls` send, as curl, for the sheet: comments on lines show the version of
    /// the diff as `$BASE_SHA` and `$START_SHA`, which GitLab names only when they are sent.
    pub fn preview(&self, project: &GitLabProject, calls: &[Call]) -> std::result::Result<Vec<String>, String> {
        let mut review: Option<String> = None;
        let mut reviewed = "$HEAD_SHA".to_owned();
        let mut out = Vec::new();
        for call in calls {
            let mut call = call.clone();
            if let Some(id) = &review {
                call.in_review(id);
            }
            let head = match &call {
                Call::StartReview { id, commit } => {
                    review = Some(id.clone());
                    reviewed = commit.clone();
                    commit.clone()
                }
                Call::Comment { commit, .. } => commit.clone(),
                _ => reviewed.clone(),
            };
            let refs = DiffRefs {
                base_sha: "$BASE_SHA".to_owned(),
                start_sha: "$START_SHA".to_owned(),
                head_sha: head,
            };
            let requests = self.call_requests(project, &call, Some(&refs))?;
            out.extend(requests.iter().map(Request::display));
        }
        Ok(out)
    }

    /// Send `calls` in order, telling `step` each request before it goes and what came of each
    /// call. A review's calls go into the merge request `StartReview` names, on the version of
    /// the diff its commit is the head of; when one fails or the run is `stopped`, the draft
    /// comments it made are deleted, so none wait on GitLab to go out with a later review.
    /// Answers with what the last call returned.
    pub fn run(
        &self,
        project: &GitLabProject,
        calls: &[Call],
        stopped: &dyn Fn() -> bool,
        step: &mut dyn FnMut(Step),
    ) -> std::result::Result<Value, String> {
        let mut review: Option<String> = None;
        let mut commit: Option<String> = None;
        let mut refs: Option<(u64, String, DiffRefs)> = None;
        let mut drafts: Vec<(u64, u64)> = Vec::new();
        let unsent = |drafts: &[(u64, u64)], why: String| {
            for (number, draft) in drafts {
                let _ = self.call(&self.delete_draft_request(project, *number, *draft));
            }
            why
        };
        let mut answer = Value::Null;
        for call in calls {
            if stopped() {
                return Err(unsent(&drafts, "Stopped. Nothing else was sent.".to_owned()));
            }
            let mut call = call.clone();
            if let Some(id) = &review {
                call.in_review(id);
            }
            if let Call::StartReview { id, commit: on } = &call {
                review = Some(id.clone());
                commit = Some(on.clone());
            }
            // Comments on lines go on the version of the diff they were written on.
            let on = match &call {
                Call::Comment { number, commit, .. } => Some((*number, commit.clone())),
                Call::ReviewComment {
                    review: Some(review), ..
                } => review.parse().ok().zip(commit.clone()),
                _ => None,
            };
            if let Some((number, head)) = on
                && refs.as_ref().is_none_or(|(n, h, _)| *n != number || *h != head)
            {
                refs = match self.diff_refs_at(project, number, &head) {
                    Ok(found) => found.map(|found| (number, head, found)),
                    Err(err) => return Err(unsent(&drafts, err.to_string())),
                };
            }
            let requests = self
                .call_requests(project, &call, refs.as_ref().map(|(_, _, r)| r))
                .map_err(|message| unsent(&drafts, message))?;
            for request in &requests {
                step(Step::Request(request.display()));
                answer = self.call(request).map_err(|err| unsent(&drafts, err.to_string()))?;
                if request.method == "POST" && request.url.ends_with("/draft_notes") {
                    let number = review.as_deref().and_then(|r| r.parse().ok()).unwrap_or(0);
                    if let Some(id) = answer.get("id").and_then(Value::as_u64) {
                        drafts.push((number, id));
                    }
                }
                if request.url.ends_with("/bulk_publish") {
                    drafts.clear();
                }
            }
            if matches!(call, Call::SubmitReview { .. } | Call::DeleteReview { .. }) {
                review = None;
            }
            step(Step::Done(match call {
                Call::Create { .. } => answer
                    .get("web_url")
                    .and_then(Value::as_str)
                    .map(|url| format!("Opened {url}")),
                _ => None,
            }));
        }
        Ok(answer)
    }

    /// The commits of the version of merge request `number`'s diff whose head is `head`: GitLab
    /// keeps every version, and a comment goes on the one it was written on.
    pub fn diff_refs_at(&self, project: &GitLabProject, number: u64, head: &str) -> Result<Option<DiffRefs>> {
        let (versions, _) = self.pages(
            &format!("/projects/{}/merge_requests/{number}/versions", project_id(project)),
            3,
        )?;
        Ok(versions
            .iter()
            .find(|v| v.get("head_commit_sha").and_then(Value::as_str) == Some(head))
            .map(|v| DiffRefs {
                base_sha: text(v, "base_commit_sha"),
                start_sha: text(v, "start_commit_sha"),
                head_sha: head.to_owned(),
            }))
    }

    pub(crate) fn api_request(&self, method: &'static str, path: &str, json: Option<Value>) -> Request {
        Request {
            method,
            url: format!("{}{path}", self.api),
            json,
        }
    }

    // Calls.

    /// Where the account can make a project: its own namespace first, then its groups'.
    pub fn namespaces(&self) -> Result<Vec<Namespace>> {
        let (list, _) = self.pages("/namespaces", 3)?;
        let mut out: Vec<Namespace> = list
            .iter()
            .map(|n| Namespace {
                id: n.get("id").and_then(Value::as_u64).unwrap_or(0),
                full_path: text(n, "full_path"),
                kind: text(n, "kind"),
            })
            .filter(|n| n.id != 0 && !n.full_path.is_empty())
            .collect();
        out.sort_by_key(|n| n.kind != "user");
        Ok(out)
    }

    /// Make the project Publish to GitLab pushes to, empty.
    pub fn create_project_request(&self, publish: &GitLabPublish) -> Request {
        let mut body = json!({
            "name": publish.name,
            "path": publish.name,
            "namespace_id": publish.namespace_id,
            "visibility": publish.visibility,
        });
        if !publish.description.trim().is_empty() {
            body["description"] = json!(publish.description.trim());
        }
        self.api_request("POST", "/projects", Some(body))
    }

    pub fn create_project(&self, publish: &GitLabPublish) -> Result<NewProject> {
        let made = self.call(&self.create_project_request(publish))?;
        Ok(NewProject {
            web_url: text(&made, "web_url"),
            visibility: text(&made, "visibility"),
        })
    }

    /// Who the token belongs to, what it may do and which GitLab the server runs.
    pub fn me(&self) -> Result<Me> {
        let user = self.call(&self.user_request())?;
        let user: User = serde_json::from_value(user).map_err(|_| self.not_gitlab())?;
        let version = self
            .call(&self.version_request())?
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        // A token of another kind (a project's, OAuth) has no such page: nothing to judge it by.
        let token = self.call(&self.token_request()).unwrap_or(Value::Null);
        let scopes = token
            .get("scopes")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).map(str::to_owned).collect());
        let expires_at = token.get("expires_at").and_then(Value::as_str).map(str::to_owned);
        Ok(Me {
            user,
            scopes,
            expires_at,
            version,
        })
    }

    /// The project's merge requests, open and recently closed, the latest updated first, in
    /// the shape of GitHub's pull requests: `number` is GitLab's `!12`.
    pub fn merge_requests(&self, project: &GitLabProject) -> Result<PullList> {
        let mut items = Vec::new();
        let mut issues = Vec::new();
        // GitLab separates closed and merged history. Each state is read independently.
        for (state, section, most) in [("opened", "Open", 10), ("closed", "Closed", 1), ("merged", "Merged", 1)] {
            let loaded = self.read_list(
                &format!(
                    "/projects/{}/merge_requests?scope=all&state={state}&order_by=updated_at&sort=desc",
                    project_id(project)
                ),
                most,
                section,
            );
            items.extend(
                loaded
                    .items
                    .iter()
                    .map(summary)
                    .filter(|p| section == "Open" || p.state != PullState::Open),
            );
            issues.extend(loaded.issue);
        }
        Ok(PullList { items, issues })
    }

    fn read_list(&self, path: &str, most: u32, section: &str) -> Loaded {
        let join = if path.contains('?') { '&' } else { '?' };
        read_pages(section, most, |page| {
            self.call(&self.api_request("GET", &format!("{path}{join}per_page=100&page={page}"), None))
        })
    }

    /// Every item of a list GitLab hands out 100 at a time, from at most `most` pages, and
    /// whether that was all of them: a page with fewer is the last one.
    fn pages(&self, path: &str, most: u32) -> Result<(Vec<Value>, bool)> {
        let mut items = Vec::new();
        let join = if path.contains('?') { '&' } else { '?' };
        for page in 1..=most {
            let request = self.api_request("GET", &format!("{path}{join}per_page=100&page={page}"), None);
            let list = match self.call(&request)? {
                Value::Array(list) => list,
                _ => return Ok((items, true)),
            };
            let last = list.len() < 100;
            items.extend(list);
            if last {
                return Ok((items, true));
            }
        }
        Ok((items, false))
    }

    /// Everything the screen shows about merge request `!iid`, in the shape of a GitHub pull
    /// request, with what only GitLab has in [`PullRequest::gitlab`]. Parts this account may not
    /// read have explicit load issues; the line counts and the code under comments come from here
    /// ([`Repo::merge_request_here`](crate::repo::Repo::merge_request_here)).
    pub fn merge_request(&self, project: &GitLabProject, iid: u64) -> Result<PullRequest> {
        let at = format!("/projects/{}", project_id(project));
        let mr = format!("{at}/merge_requests/{iid}");
        let get = |path: String| {
            let request = self.api_request("GET", &path, None);
            move || self.call(&request)
        };
        let (detail, commits, discussions, approvals, reviewers, settings) = std::thread::scope(|s| {
            let detail = s.spawn(get(format!("{mr}?include_diverged_commits_count=true")));
            let commits = s.spawn(|| self.read_list(&format!("{mr}/commits"), 5, "Commits"));
            let discussions = s.spawn(|| self.read_list(&format!("{mr}/discussions"), 10, "Discussions"));
            let approvals = s.spawn(get(format!("{mr}/approvals")));
            let reviewers = s.spawn(get(format!("{mr}/reviewers")));
            let settings = s.spawn(get(at.clone()));
            (
                detail.join(),
                commits.join(),
                discussions.join(),
                approvals.join(),
                reviewers.join(),
                settings.join(),
            )
        });
        let joined = |r: std::thread::Result<Result<Value>>| {
            r.unwrap_or_else(|_| Err(Error::Git("The request worker stopped unexpectedly.".into())))
        };
        let mut load_issues = Vec::new();
        let detail = joined(detail)?;
        let settings = joined(settings)?;
        let commits = loaded_read("Commits", commits, &mut load_issues);
        let complete = !load_issues.iter().any(|i| i.section == "Commits");
        let discussions = loaded_read("Discussions", discussions, &mut load_issues);
        let approvals = optional_read("Approvals", approvals, &mut load_issues);
        let reviewers = optional_read("Reviewers", reviewers, &mut load_issues);
        let pipeline = detail.get("head_pipeline").filter(|p| !p.is_null()).map(|p| Pipeline {
            id: p.get("id").and_then(Value::as_u64).unwrap_or(0),
            status: text(p, "status"),
            url: text(p, "web_url"),
        });
        let jobs = match &pipeline {
            Some(p) if p.id > 0 => {
                let loaded = self.read_list(&format!("{at}/pipelines/{}/jobs", p.id), 3, "Checks");
                load_issues.extend(loaded.issue);
                loaded.items
            }
            _ => Vec::new(),
        };

        let summary = summary(&detail);
        let state = summary.state;
        // GitLab lists the newest commit first; the screen shows them oldest first.
        let commits: Vec<PullCommit> = commits.iter().rev().map(commit).collect();
        let status = text(&detail, "detailed_merge_status");
        let squash = detail.get("squash").and_then(Value::as_bool) == Some(true);
        let changes = text(&detail, "changes_count");
        let digits: String = changes.chars().take_while(char::is_ascii_digit).collect();
        let merge_commit = ["merge_commit_sha", "squash_commit_sha"]
            .iter()
            .find_map(|key| detail.get(*key).and_then(Value::as_str).map(str::to_owned))
            // A fast-forward merge makes no commit of its own: the target is at the head now.
            .or_else(|| Some(summary.head_sha.clone()).filter(|sha| !sha.is_empty()));
        let required = approvals
            .get("approvals_required")
            .and_then(Value::as_u64)
            .filter(|n| *n > 0)
            .map(|n| n as u32);
        let diff = detail.get("diff_refs").filter(|d| !d.is_null()).map(|d| DiffRefs {
            base_sha: text(d, "base_sha"),
            start_sha: text(d, "start_sha"),
            head_sha: text(d, "head_sha"),
        });
        let squash_option = settings
            .get("squash_option")
            .and_then(Value::as_str)
            .unwrap_or("default_off")
            .to_owned();
        let facts = GitLabFacts {
            merge_status: status.clone(),
            merge_method: settings
                .get("merge_method")
                .and_then(Value::as_str)
                .unwrap_or("merge")
                .to_owned(),
            squash_option: squash_option.clone(),
            squash,
            remove_source_branch: detail.get("force_remove_source_branch").and_then(Value::as_bool) == Some(true),
            behind: detail
                .get("diverged_commits_count")
                .and_then(Value::as_u64)
                .map(|n| n as u32),
            pipeline,
            diff,
            line_counts: false,
            reviewer_ids: detail
                .get("reviewers")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|r| r.get("id").and_then(Value::as_u64))
                .collect(),
        };
        Ok(PullRequest {
            load_issues,
            additions: 0,
            deletions: 0,
            changed_files: digits.parse().unwrap_or(0),
            commit_count: commits.len() as u32,
            commits_complete: complete,
            mergeable: match status.as_str() {
                "conflict" | "broken_status" => Some(false),
                _ if detail.get("has_conflicts").and_then(Value::as_bool) == Some(true) => Some(false),
                "checking" | "unchecked" | "preparing" | "approvals_syncing" | "" => None,
                _ => Some(true),
            },
            mergeable_state: mergeable_state(&status).to_owned(),
            merged_at: (state == PullState::Merged).then(|| time(&detail, "merged_at")),
            merge_commit: merge_commit.filter(|_| state == PullState::Merged),
            merged_by: ["/merge_user/username", "/merged_by/username"]
                .iter()
                .find_map(|key| detail.pointer(key).and_then(Value::as_str).map(str::to_owned)),
            auto_merge: (detail.get("merge_when_pipeline_succeeds").and_then(Value::as_bool) == Some(true))
                .then(|| if squash { "squash" } else { "merge" }.to_owned()),
            commits,
            reviews: reviews(&approvals, &reviewers),
            timeline: conversation(&discussions),
            checks: checks(&jobs),
            required_approvals: required,
            settings: RepoSettings {
                merge_commit: squash_option != "always",
                squash: squash_option != "never",
                rebase: false,
                auto_merge: true,
                // GitLab deletes the source branch itself when the merge asks it to.
                delete_branch_on_merge: true,
                default_branch: settings
                    .get("default_branch")
                    .and_then(Value::as_str)
                    .unwrap_or("main")
                    .to_owned(),
                can_push: detail.pointer("/user/can_merge").and_then(Value::as_bool) == Some(true),
            },
            gitlab: Some(facts),
            summary,
        })
    }

    /// What opening a merge request needs from the project: its labels, how it squashes and
    /// whether it deletes merged branches, and the account's own id, for "assign to me".
    pub fn create_facts(&self, project: &GitLabProject) -> Result<CreateFacts> {
        let at = format!("/projects/{}", project_id(project));
        let (settings, labels, me) = std::thread::scope(|s| {
            let settings = s.spawn(|| self.call(&self.api_request("GET", &at, None)));
            let labels = s.spawn(|| self.pages(&format!("{at}/labels"), 5));
            let me = s.spawn(|| self.call(&self.user_request()));
            (settings.join(), labels.join(), me.join())
        });
        let settings = settings.unwrap_or(Ok(Value::Null))?;
        let labels = labels
            .unwrap_or(Ok((Vec::new(), true)))
            .map_or_else(|_| Vec::new(), |(l, _)| l);
        let me = me.unwrap_or(Ok(Value::Null)).unwrap_or(Value::Null);
        Ok(CreateFacts {
            labels: labels
                .iter()
                .map(|l| Label {
                    name: text(l, "name"),
                    color: text(l, "color"),
                })
                .filter(|l| !l.name.is_empty())
                .collect(),
            squash_option: settings
                .get("squash_option")
                .and_then(Value::as_str)
                .unwrap_or("default_off")
                .to_owned(),
            remove_source_branch: settings
                .get("remove_source_branch_after_merge")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            me: Some(gitlab_user(&me)).filter(|u| u.id != 0),
        })
    }

    /// People who can see the project whose name or username has `search` in it, at most 20.
    pub fn people(&self, project: &GitLabProject, search: &str) -> Result<Vec<GitLabUser>> {
        let request = self.api_request(
            "GET",
            &format!(
                "/projects/{}/users?search={}&per_page=20",
                project_id(project),
                crate::github::percent(search.trim())
            ),
            None,
        );
        Ok(self
            .call(&request)?
            .as_array()
            .into_iter()
            .flatten()
            .map(gitlab_user)
            .filter(|u| u.id != 0)
            .collect())
    }

    /// The accounts with these usernames, in their order; a name GitLab has no account for (a
    /// group, a typo) is left out.
    pub fn users_named(&self, names: &[String]) -> Vec<GitLabUser> {
        std::thread::scope(|s| {
            let asks: Vec<_> = names
                .iter()
                .take(20)
                .map(|name| {
                    let request = self.api_request(
                        "GET",
                        &format!("/users?username={}", crate::github::percent(name)),
                        None,
                    );
                    s.spawn(move || self.call(&request))
                })
                .collect();
            asks.into_iter()
                .filter_map(|ask| ask.join().ok()?.ok())
                .filter_map(|list| list.as_array()?.first().map(gitlab_user))
                .filter(|u| u.id != 0)
                .collect()
        })
    }

    /// GitLab's say on each of the open merge requests, for the Reviews screen: approvals,
    /// changes asked for and the newest pipeline. At most the first 40, six asked at a time.
    pub fn statuses(&self, project: &GitLabProject, open: &[u64]) -> Vec<PullStatus> {
        let at = format!("/projects/{}/merge_requests", project_id(project));
        let mut out = Vec::new();
        for chunk in open.iter().take(40).collect::<Vec<_>>().chunks(6) {
            let found: Vec<PullStatus> = std::thread::scope(|s| {
                let asks: Vec<_> = chunk
                    .iter()
                    .map(|&&iid| {
                        let at = &at;
                        s.spawn(move || {
                            let get =
                                |path: String| self.call(&self.api_request("GET", &path, None)).unwrap_or(Value::Null);
                            let approvals = get(format!("{at}/{iid}/approvals"));
                            let reviewers = get(format!("{at}/{iid}/reviewers"));
                            let pipelines = get(format!("{at}/{iid}/pipelines?per_page=1"));
                            status(iid, &approvals, &reviewers, &pipelines)
                        })
                    })
                    .collect();
                asks.into_iter().filter_map(|ask| ask.join().ok()).collect()
            });
            out.extend(found);
        }
        out
    }

    /// The picture at `url`, such as an account's avatar, as a `data:` address the window can
    /// show without loading anything from elsewhere; `None` when it can't be had or is too big.
    pub fn picture(&self, url: &str) -> Option<String> {
        let mut response = self.agent.get(url).call().ok()?;
        if response.status() != 200 {
            return None;
        }
        let kind = response.headers().get("content-type")?.to_str().ok()?;
        let kind = kind.split(';').next()?.trim().to_owned();
        if !kind.starts_with("image/") {
            return None;
        }
        let bytes = response.body_mut().with_config().limit(256 * 1024).read_to_vec().ok()?;
        Some(format!("data:{kind};base64,{}", base64(&bytes)))
    }

    /// Send an API request; an error status comes back in words for people.
    pub(crate) fn call(&self, request: &Request) -> Result<Value> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed))
        {
            return Err(Error::Git("Loading stopped. Retry to load the remaining data.".into()));
        }
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| gitlab(Some(401), format!("Not signed in to {}", self.server.host)))?;
        let mut builder = ureq::http::Request::builder()
            .method(request.method)
            .uri(&request.url)
            .header("Accept", "application/json")
            .header("PRIVATE-TOKEN", token);
        let body = match &request.json {
            Some(json) => {
                builder = builder.header("Content-Type", "application/json");
                json.to_string()
            }
            None => String::new(),
        };
        let http = builder.body(body).map_err(|err| gitlab(None, err.to_string()))?;
        let mut response = self.agent.run(http).map_err(|err| self.network(err))?;
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().map_err(|err| self.network(err))?;
        let body = if text.trim().is_empty() {
            Value::Null
        } else {
            // A page instead of JSON: something else answers at this address.
            serde_json::from_str(&text).map_err(|_| self.not_gitlab())?
        };
        if status >= 400 {
            return Err(gitlab(Some(status), api_error(status, &body)));
        }
        Ok(body)
    }

    fn not_gitlab(&self) -> Error {
        gitlab(
            None,
            format!(
                "{} doesn’t answer like GitLab. Check the server’s address.",
                self.server.host
            ),
        )
    }

    fn network(&self, err: ureq::Error) -> Error {
        let host = &self.server.host;
        let message = match &err {
            ureq::Error::Io(io) => format!("Can't reach {host}: {io}"),
            ureq::Error::Timeout(_) => format!("{host} didn't answer in time"),
            ureq::Error::HostNotFound => {
                format!("Can't find {host}: check the address, and whether this computer is online")
            }
            other => format!("Can't reach {host}: {other}"),
        };
        gitlab(None, message)
    }
}

fn gitlab_user(user: &Value) -> GitLabUser {
    GitLabUser {
        id: user.get("id").and_then(Value::as_u64).unwrap_or(0),
        username: text(user, "username"),
        name: text(user, "name"),
    }
}

/// One merge request's say for the Reviews screen, in GitHub's words: changes asked for wins,
/// then approved (nothing left to approve, by someone), then a review still required.
fn status(iid: u64, approvals: &Value, reviewers: &Value, pipelines: &Value) -> PullStatus {
    let asked_changes = reviewers
        .as_array()
        .into_iter()
        .flatten()
        .any(|r| r.get("state").and_then(Value::as_str) == Some("requested_changes"));
    let approved_by = approvals
        .get("approved_by")
        .and_then(Value::as_array)
        .is_some_and(|list| !list.is_empty());
    let left = approvals.get("approvals_left").and_then(Value::as_u64).unwrap_or(0);
    let required = approvals.get("approvals_required").and_then(Value::as_u64).unwrap_or(0);
    let decision = if asked_changes {
        Some("changesRequested")
    } else if approved_by && left == 0 {
        Some("approved")
    } else if required > 0 || left > 0 {
        Some("reviewRequired")
    } else {
        None
    };
    let checks = pipelines
        .as_array()
        .and_then(|list| list.first())
        .and_then(|p| p.get("status"))
        .and_then(Value::as_str)
        .map(|status| match status {
            "success" => "success",
            "failed" => "failure",
            "canceled" | "skipped" => "error",
            _ => "pending",
        });
    PullStatus {
        number: iid,
        decision: decision.map(str::to_owned),
        checks: checks.map(str::to_owned),
    }
}

/// GitLab's say on merging, in GitHub's words the screen already knows: `clean`, `blocked`,
/// `behind`, `dirty`, `draft` or `unknown`.
fn mergeable_state(status: &str) -> &'static str {
    match status {
        "mergeable" => "clean",
        "checking" | "unchecked" | "preparing" | "approvals_syncing" | "" => "unknown",
        "conflict" | "broken_status" => "dirty",
        "need_rebase" => "behind",
        "draft_status" => "draft",
        "not_open" => "clean",
        _ => "blocked",
    }
}

fn time(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_str).map(parse_time).unwrap_or(0)
}

fn person(user: Option<&Value>) -> Person {
    Person {
        login: user
            .and_then(|u| u.get("username"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        // The window loads no pictures from the server.
        avatar_url: String::new(),
    }
}

fn commit(item: &Value) -> PullCommit {
    let message = text(item, "message");
    PullCommit {
        sha: text(item, "id"),
        summary: match text(item, "title") {
            title if title.is_empty() => message.lines().next().unwrap_or_default().to_owned(),
            title => title,
        },
        author: text(item, "author_name"),
        time: ["committed_date", "created_at"]
            .iter()
            .map(|key| time(item, key))
            .find(|t| *t != 0)
            .unwrap_or(0),
        message,
        additions: None,
        deletions: None,
    }
}

/// Each reviewer's say: approvals first, then GitLab 17's reviewer states (changes asked for,
/// reviewed); a reviewer who hasn't looked yet stays among those asked.
fn reviews(approvals: &Value, reviewers: &Value) -> Vec<Review> {
    let mut out: Vec<Review> = Vec::new();
    for approval in approvals
        .get("approved_by")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        out.push(Review {
            author: person(approval.get("user")),
            state: "approved".to_owned(),
            time: time(approval, "approved_at"),
        });
    }
    for reviewer in reviewers.as_array().into_iter().flatten() {
        let author = person(reviewer.get("user"));
        let state = match reviewer.get("state").and_then(Value::as_str) {
            Some("requested_changes") => "changesRequested",
            Some("reviewed") => "commented",
            _ => continue,
        };
        if out.iter().any(|r| r.author.login == author.login) {
            continue;
        }
        out.push(Review {
            author,
            state: state.to_owned(),
            time: time(reviewer, "created_at"),
        });
    }
    out
}

/// The discussions, oldest first: comments, threads (on lines of code, or about the merge
/// request as a whole) and what happened, from GitLab's system notes.
fn conversation(discussions: &[Value]) -> Vec<Entry> {
    let mut out = Vec::new();
    for discussion in discussions {
        let notes: Vec<&Value> = discussion
            .get("notes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .collect();
        let Some(first) = notes.first() else {
            continue;
        };
        let system = |note: &&Value| note.get("system").and_then(Value::as_bool) == Some(true);
        if system(first) {
            out.extend(notes.iter().filter(|n| system(n)).map(|note| event(note)));
            continue;
        }
        let comment = |note: &&Value| Comment {
            author: person(note.get("author")),
            body: text(note, "body"),
            time: time(note, "created_at"),
        };
        let threaded = discussion.get("individual_note").and_then(Value::as_bool) != Some(true);
        if !threaded {
            out.push(Entry::Comment {
                author: person(first.get("author")),
                body: text(first, "body"),
                time: time(first, "created_at"),
            });
            continue;
        }
        let position = first.get("position").filter(|p| !p.is_null());
        let at = |key: &str| {
            position
                .and_then(|p| p.get(key))
                .and_then(Value::as_u64)
                .map(|n| n as u32)
        };
        let new_line = at("new_line");
        let line = new_line.or(at("old_line"));
        let side = if new_line.is_some() { "RIGHT" } else { "LEFT" };
        let start = position
            .and_then(|p| {
                p.pointer(if new_line.is_some() {
                    "/line_range/start/new_line"
                } else {
                    "/line_range/start/old_line"
                })
            })
            .and_then(Value::as_u64)
            .map(|n| n as u32)
            .filter(|start| Some(*start) != line);
        let path = position
            .and_then(|p| p.get("new_path").or_else(|| p.get("old_path")))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let resolvable: Vec<&&Value> = notes
            .iter()
            .filter(|n| n.get("resolvable").and_then(Value::as_bool) == Some(true))
            .collect();
        out.push(Entry::Thread {
            id: first.get("id").and_then(Value::as_u64).unwrap_or(0),
            node: discussion.get("id").and_then(Value::as_str).map(str::to_owned),
            path,
            line,
            start_line: start,
            side: side.to_owned(),
            hunk: Vec::new(),
            comments: notes.iter().filter(|n| !system(n)).map(comment).collect(),
            resolved: (!resolvable.is_empty()).then(|| {
                resolvable
                    .iter()
                    .all(|n| n.get("resolved").and_then(Value::as_bool) == Some(true))
            }),
            outdated: false,
            commit: position
                .and_then(|p| p.get("head_sha"))
                .and_then(Value::as_str)
                .map(str::to_owned),
            time: time(first, "created_at"),
        });
    }
    out.sort_by_key(|e| match e {
        Entry::Comment { time, .. }
        | Entry::Review { time, .. }
        | Entry::Thread { time, .. }
        | Entry::Event { time, .. } => *time,
    });
    out
}

/// A system note in one line: "added 2 commits", "approved this merge request".
fn event(note: &Value) -> Entry {
    let body = text(note, "body");
    let line = body
        .lines()
        .next()
        .unwrap_or_default()
        .replace("**", "")
        .replace('`', "");
    let (icon, tone) = if line.starts_with("added ") && line.contains("commit") {
        ("push", None)
    } else if line.starts_with("approved") {
        ("ok", Some("ok"))
    } else if line.starts_with("requested changes") {
        ("dot", Some("bad"))
    } else if line.contains("as draft") || line.contains("Work In Progress") {
        ("draft", None)
    } else if line.starts_with("requested review") {
        ("review", None)
    } else {
        ("dot", None)
    };
    Entry::Event {
        actor: person(note.get("author")).login,
        text: line,
        icon: icon.to_owned(),
        tone: tone.map(str::to_owned),
        time: time(note, "created_at"),
    }
}

/// The jobs of the pipeline, in the order they run, as the screen's checks.
fn checks(jobs: &[Value]) -> Vec<Check> {
    let mut jobs: Vec<&Value> = jobs.iter().collect();
    jobs.sort_by_key(|job| job.get("id").and_then(Value::as_u64).unwrap_or(0));
    jobs.iter()
        .map(|job| {
            let status = text(job, "status");
            let allowed = job.get("allow_failure").and_then(Value::as_bool) == Some(true);
            let state = match status.as_str() {
                "success" => "ok",
                // GitLab merges past it, and shows it as a warning.
                "failed" if allowed => "warn",
                "failed" => "fail",
                "canceled" | "skipped" | "manual" => "skip",
                _ => "run",
            };
            let stage = text(job, "stage");
            Check {
                name: text(job, "name"),
                detail: match (state, status.as_str()) {
                    ("warn", _) => format!("{stage}, allowed to fail"),
                    (_, "manual") => format!("{stage}, started by hand"),
                    _ => stage,
                },
                state: state.to_owned(),
                seconds: (state != "run")
                    .then(|| job.get("duration").and_then(Value::as_f64).map(|d| d.round() as i64))
                    .flatten(),
                url: job.get("web_url").and_then(Value::as_str).map(str::to_owned),
            }
        })
        .collect()
}

/// Where on the diff a comment goes, as GitLab takes it: the commits of the diff's version, the
/// file, and the line by its old number (a deleted line), its new one (an added line) or both
/// (an unchanged line); several lines as a range of line codes.
fn position(refs: &DiffRefs, spot: &crate::pulls::Spot) -> Value {
    let mut p = json!({
        "base_sha": refs.base_sha,
        "start_sha": refs.start_sha,
        "head_sha": refs.head_sha,
        "position_type": "text",
        "old_path": spot.path,
        "new_path": spot.path,
    });
    match &spot.lines {
        Some(lines) => {
            let end = &lines.end;
            if end.kind != "added" {
                p["old_line"] = json!(end.old);
            }
            if end.kind != "removed" {
                p["new_line"] = json!(end.new);
            }
            if let Some(start) = &lines.start {
                p["line_range"] = json!({ "start": range_line(&spot.path, start), "end": range_line(&spot.path, end) });
            }
        }
        // Written before Oxbow kept both numbers: the one GitHub names.
        None if spot.side == "LEFT" => p["old_line"] = json!(spot.line),
        None => p["new_line"] = json!(spot.line),
    }
    p
}

/// One end of a range of lines: its line code, `new` for an added line or `old` for any other,
/// and its numbers.
fn range_line(path: &str, line: &crate::pulls::LinePair) -> Value {
    let mut out = json!({
        "line_code": line_code(path, line.old, line.new),
        "type": if line.kind == "added" { "new" } else { "old" },
    });
    if line.kind != "added" {
        out["old_line"] = json!(line.old);
    }
    if line.kind != "removed" {
        out["new_line"] = json!(line.new);
    }
    out
}

/// GitLab's name for a line of a diff: the SHA-1 of the file's path, its old and new numbers.
pub fn line_code(path: &str, old: u32, new: u32) -> String {
    let mut hasher = gix::hash::hasher(gix::hash::Kind::Sha1);
    hasher.update(path.as_bytes());
    let hash = hasher
        .try_finalize()
        .map(|id| id.to_hex().to_string())
        .unwrap_or_default();
    format!("{hash}_{old}_{new}")
}

/// `title` as a draft's, starting with `Draft:`, or without it. GitLab also reads the older
/// `[Draft]`, `(Draft)`, `WIP:` and `[WIP]`.
pub fn draft_title(title: &str, draft: bool) -> String {
    let mut bare = title.trim();
    loop {
        let lower = bare.to_lowercase();
        let Some(prefix) = ["draft:", "[draft]", "(draft)", "wip:", "[wip]"]
            .iter()
            .find(|p| lower.starts_with(**p))
        else {
            break;
        };
        bare = bare[prefix.len()..].trim_start();
    }
    if draft {
        format!("Draft: {bare}")
    } else {
        bare.to_owned()
    }
}

/// The project as the API names it: its path, `group%2Fproject`.
fn project_id(project: &GitLabProject) -> String {
    crate::github::percent(&project.path)
}

/// A merge request in the list, as a pull request.
fn summary(mr: &Value) -> PullSummary {
    let text = |key: &str| mr.get(key).and_then(Value::as_str).unwrap_or_default().to_owned();
    let time = |key: &str| mr.get(key).and_then(Value::as_str).map(parse_time).unwrap_or(0);
    let state = match mr.get("state").and_then(Value::as_str) {
        Some("merged") => PullState::Merged,
        Some("closed") => PullState::Closed,
        // `locked` is a moment while GitLab merges it.
        _ => PullState::Open,
    };
    let source = mr.get("source_project_id").and_then(Value::as_u64);
    let fork = source.is_some() && source != mr.get("target_project_id").and_then(Value::as_u64);
    let draft = ["draft", "work_in_progress"]
        .iter()
        .any(|key| mr.get(*key).and_then(Value::as_bool) == Some(true));
    PullSummary {
        number: mr.get("iid").and_then(Value::as_u64).unwrap_or(0),
        title: text("title"),
        state,
        draft,
        head: text("source_branch"),
        head_sha: text("sha"),
        // A merge request from a fork: its project's id, as GitLab's list names no owner.
        head_owner: fork.then(|| format!("project {}", source.unwrap_or_default())),
        base: text("target_branch"),
        html_url: text("web_url"),
        body: text("description"),
        author: Person {
            login: mr
                .pointer("/author/username")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            // The window loads no pictures from the server.
            avatar_url: String::new(),
        },
        created: time("created_at"),
        updated: time("updated_at"),
        node_id: mr.get("id").and_then(Value::as_u64).unwrap_or(0).to_string(),
        requested: mr
            .get("reviewers")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|r| r.get("username").and_then(Value::as_str).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn gitlab(status: Option<u16>, message: String) -> Error {
    Error::GitLab { status, message }
}

/// GitLab's own words for a failed API call, or ours where it has none.
fn api_error(status: u16, body: &Value) -> String {
    // `message` is a string, or a map of field to its problems when a write is refused.
    let message = match body.get("message") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(fields)) => fields
            .iter()
            .map(|(field, problems)| match problems {
                Value::Array(list) => format!(
                    "{field} {}",
                    list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
                ),
                other => format!("{field} {other}"),
            })
            .collect::<Vec<_>>()
            .join(". "),
        _ => body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    };
    match status {
        401 => "GitLab didn't accept the token: it is wrong, expired or revoked".to_owned(),
        403 => "The token may not do this: it needs the api scope, or access to the project".to_owned(),
        404 if message.contains("Project") => "GitLab doesn't know this project, or the token can't see it".to_owned(),
        _ if !message.is_empty() => message,
        _ => format!("GitLab answered with error {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_by_their_title() {
        assert_eq!(draft_title("Draft: Export CSV", false), "Export CSV");
        assert_eq!(draft_title("[WIP] draft: Export CSV", false), "Export CSV");
        assert_eq!(draft_title("Export CSV", true), "Draft: Export CSV");
        assert_eq!(draft_title("Draft:Export CSV", true), "Draft: Export CSV");
        assert_eq!(draft_title("Drafts of invoices", false), "Drafts of invoices");
    }

    #[test]
    fn calls_show_as_curl() {
        let client = Client::new(Server::parse("git.corp").unwrap());
        let project = GitLabProject {
            remote: "origin".into(),
            host: "git.corp".into(),
            path: "team/app".into(),
            web_url: "https://git.corp/team/app".into(),
        };
        let shown = |call: Call| client.call_request(&project, &call).unwrap().display();
        assert_eq!(
            shown(Call::Merge {
                number: 12,
                method: Method::Squash,
                title: None,
                message: None,
                sha: "abc".into(),
                delete_branch: true,
            }),
            r#"curl -X PUT -H "PRIVATE-TOKEN: $GITLAB_TOKEN" https://git.corp/api/v4/projects/team%2Fapp/merge_requests/12/merge -d '{"sha":"abc","should_remove_source_branch":true,"squash":true}'"#
        );
        assert_eq!(
            shown(Call::AutoMerge {
                number: 12,
                id: String::new(),
                method: None
            }),
            r#"curl -X POST -H "PRIVATE-TOKEN: $GITLAB_TOKEN" https://git.corp/api/v4/projects/team%2Fapp/merge_requests/12/cancel_merge_when_pipeline_succeeds"#
        );
        assert_eq!(
            shown(Call::ReadyForReview {
                number: 12,
                id: String::new(),
                title: Some("Draft: Export CSV".into())
            }),
            r#"curl -X PUT -H "PRIVATE-TOKEN: $GITLAB_TOKEN" https://git.corp/api/v4/projects/team%2Fapp/merge_requests/12 -d '{"title":"Export CSV"}'"#
        );
        assert!(
            shown(Call::Update {
                number: 12,
                base: Some("release".into()),
                body: None
            })
            .ends_with(r#"-d '{"target_branch":"release"}'"#)
        );
        assert!(
            shown(Call::SetState {
                number: 12,
                open: false
            })
            .ends_with(r#"-d '{"state_event":"close"}'"#)
        );
        assert!(
            client
                .call_request(
                    &project,
                    &Call::Viewed {
                        id: String::new(),
                        path: "a.rs".into(),
                        viewed: true
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn pictures_as_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0x00, 0x01]), "//4AAQ==");
    }

    #[test]
    fn requests_show_as_curl() {
        let client = Client::new(Server::parse("git.corp:8443").unwrap());
        assert_eq!(
            client.user_request().display(),
            "curl -H \"PRIVATE-TOKEN: $GITLAB_TOKEN\" https://git.corp:8443/api/v4/user"
        );
    }
}
