//! GitHub REST and GraphQL requests and response mapping for pull requests.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::github::{Client, Request, WEB};

use super::shared::{Loaded, loaded_read, optional_read, parse_time, read_pages, text};
use super::types::*;

impl GitHubRepo {
    fn path(&self, rest: &str) -> String {
        format!("/repos/{}/{}{rest}", self.owner, self.name)
    }
}

impl Entry {
    fn time(&self) -> i64 {
        match self {
            Entry::Comment { time, .. }
            | Entry::Review { time, .. }
            | Entry::Thread { time, .. }
            | Entry::Event { time, .. } => *time,
        }
    }
}

impl Method {
    fn rest(self) -> &'static str {
        match self {
            Method::Merge => "merge",
            Method::Squash => "squash",
            Method::Rebase => "rebase",
        }
    }

    fn graphql(self) -> &'static str {
        match self {
            Method::Merge => "MERGE",
            Method::Squash => "SQUASH",
            Method::Rebase => "REBASE",
        }
    }
}

impl Verdict {
    fn graphql(self) -> &'static str {
        match self {
            Verdict::Comment => "COMMENT",
            Verdict::Approve => "APPROVE",
            Verdict::RequestChanges => "REQUEST_CHANGES",
        }
    }
}

const READY: &str =
    "mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { pullRequest { isDraft } } }";
const DRAFT: &str =
    "mutation($id: ID!) { convertPullRequestToDraft(input: {pullRequestId: $id}) { pullRequest { isDraft } } }";
const AUTO_ON: &str = "mutation($id: ID!, $method: PullRequestMergeMethod!) { enablePullRequestAutoMerge(input: {pullRequestId: $id, mergeMethod: $method}) { clientMutationId } }";
const AUTO_OFF: &str =
    "mutation($id: ID!) { disablePullRequestAutoMerge(input: {pullRequestId: $id}) { clientMutationId } }";
const VIEWED: &str = "mutation($id: ID!, $path: String!) { markFileAsViewed(input: {pullRequestId: $id, path: $path}) { clientMutationId } }";
const UNVIEWED: &str = "mutation($id: ID!, $path: String!) { unmarkFileAsViewed(input: {pullRequestId: $id, path: $path}) { clientMutationId } }";
const RESOLVE: &str = "mutation($id: ID!) { resolveReviewThread(input: {threadId: $id}) { thread { isResolved } } }";
const UNRESOLVE: &str =
    "mutation($id: ID!) { unresolveReviewThread(input: {threadId: $id}) { thread { isResolved } } }";
const START_REVIEW: &str = "mutation($id: ID!, $commit: GitObjectID!) { addPullRequestReview(input: {pullRequestId: $id, commitOID: $commit}) { pullRequestReview { id } } }";
const REVIEW_COMMENT: &str = "mutation($review: ID!, $path: String!, $line: Int!, $side: DiffSide!, $startLine: Int, $startSide: DiffSide, $body: String!) { addPullRequestReviewThread(input: {pullRequestReviewId: $review, path: $path, line: $line, side: $side, startLine: $startLine, startSide: $startSide, body: $body}) { thread { id } } }";
const REVIEW_REPLY: &str = "mutation($review: ID!, $thread: ID!, $body: String!) { addPullRequestReviewThreadReply(input: {pullRequestReviewId: $review, pullRequestReviewThreadId: $thread, body: $body}) { comment { id } } }";
const SUBMIT_REVIEW: &str = "mutation($review: ID!, $event: PullRequestReviewEvent!, $body: String) { submitPullRequestReview(input: {pullRequestReviewId: $review, event: $event, body: $body}) { pullRequestReview { url } } }";
const DELETE_REVIEW: &str =
    "mutation($review: ID!) { deletePullRequestReview(input: {pullRequestReviewId: $review}) { clientMutationId } }";
const THREADS: &str = "query($owner: String!, $name: String!, $number: Int!, $cursor: String) { repository(owner: $owner, name: $name) { pullRequest(number: $number) { reviewThreads(first: 100, after: $cursor) { pageInfo { hasNextPage endCursor } nodes { id isResolved comments(first: 1) { nodes { databaseId } } } } } } }";

impl Client {
    // Requests, to show and then send.

    pub fn pull_request_request(&self, repo: &GitHubRepo, number: u64) -> Request {
        self.api_request("GET", &repo.path(&format!("/pulls/{number}")), None)
    }

    pub(crate) fn graphql_request(&self, query: &str, variables: Value) -> Request {
        self.api_request(
            "POST",
            "/graphql",
            Some(json!({ "query": query, "variables": variables })),
        )
    }

    /// The request `call` sends; `created` is the number a `Create` earlier in the run made.
    pub fn call_request(&self, repo: &GitHubRepo, call: &Call, created: Option<u64>) -> Request {
        let number = |n: u64| {
            if n == 0 {
                created.map_or("$NUMBER".to_owned(), |c| c.to_string())
            } else {
                n.to_string()
            }
        };
        match call {
            Call::Merge {
                number: n,
                method,
                title,
                message,
                sha,
                ..
            } => {
                let mut body = json!({ "merge_method": method.rest(), "sha": sha });
                if let Some(title) = title {
                    body["commit_title"] = json!(title);
                }
                if let Some(message) = message {
                    body["commit_message"] = json!(message);
                }
                self.api_request("PUT", &repo.path(&format!("/pulls/{}/merge", number(*n))), Some(body))
            }
            Call::Create {
                head,
                base,
                title,
                body,
                draft,
                ..
            } => self.api_request(
                "POST",
                &repo.path("/pulls"),
                Some(json!({ "title": title, "head": head, "base": base, "body": body, "draft": draft })),
            ),
            Call::RequestReviewers {
                number: n, reviewers, ..
            } => {
                let (teams, users): (Vec<&String>, Vec<&String>) = reviewers.iter().partition(|r| r.contains('/'));
                let mut body = json!({ "reviewers": users });
                if !teams.is_empty() {
                    let slugs: Vec<&str> = teams.iter().filter_map(|t| t.split('/').nth(1)).collect();
                    body["team_reviewers"] = json!(slugs);
                }
                self.api_request(
                    "POST",
                    &repo.path(&format!("/pulls/{}/requested_reviewers", number(*n))),
                    Some(body),
                )
            }
            Call::Update { number: n, base, body } => {
                let mut json = json!({});
                if let Some(base) = base {
                    json["base"] = json!(base);
                }
                if let Some(body) = body {
                    json["body"] = json!(body);
                }
                self.api_request("PATCH", &repo.path(&format!("/pulls/{}", number(*n))), Some(json))
            }
            Call::UpdateBranch { number: n, sha } => self.api_request(
                "PUT",
                &repo.path(&format!("/pulls/{}/update-branch", number(*n))),
                Some(json!({ "expected_head_sha": sha })),
            ),
            Call::ReadyForReview { id, .. } => self.graphql_request(READY, json!({ "id": id })),
            Call::ConvertToDraft { id, .. } => self.graphql_request(DRAFT, json!({ "id": id })),
            Call::SetState { number: n, open } => self.api_request(
                "PATCH",
                &repo.path(&format!("/pulls/{}", number(*n))),
                Some(json!({ "state": if *open { "open" } else { "closed" } })),
            ),
            Call::AutoMerge { id, method, .. } => match method {
                Some(method) => self.graphql_request(AUTO_ON, json!({ "id": id, "method": method.graphql() })),
                None => self.graphql_request(AUTO_OFF, json!({ "id": id })),
            },
            Call::Viewed { id, path, viewed } => self.graphql_request(
                if *viewed { VIEWED } else { UNVIEWED },
                json!({ "id": id, "path": path }),
            ),
            Call::Comment {
                number: n,
                commit,
                spot,
                body,
            } => {
                let mut json = json!({ "body": body, "commit_id": commit, "path": spot.path, "line": spot.line, "side": spot.side });
                if let Some(start) = spot.start_line {
                    json["start_line"] = json!(start);
                    json["start_side"] = json!(spot.side);
                }
                self.api_request(
                    "POST",
                    &repo.path(&format!("/pulls/{}/comments", number(*n))),
                    Some(json),
                )
            }
            Call::Reply {
                number: n, to, body, ..
            } => self.api_request(
                "POST",
                &repo.path(&format!("/pulls/{}/comments/{to}/replies", number(*n))),
                Some(json!({ "body": body })),
            ),
            Call::Resolve { thread, resolved, .. } => {
                self.graphql_request(if *resolved { RESOLVE } else { UNRESOLVE }, json!({ "id": thread }))
            }
            Call::StartReview { id, commit } => {
                self.graphql_request(START_REVIEW, json!({ "id": id, "commit": commit }))
            }
            Call::ReviewComment { review, spot, body } => self.graphql_request(
                REVIEW_COMMENT,
                json!({
                    "review": review_id(review),
                    "path": spot.path,
                    "line": spot.line,
                    "side": spot.side,
                    "startLine": spot.start_line,
                    "startSide": spot.start_line.map(|_| &spot.side),
                    "body": body,
                }),
            ),
            Call::ReviewReply { review, thread, body } => self.graphql_request(
                REVIEW_REPLY,
                json!({ "review": review_id(review), "thread": thread, "body": body }),
            ),
            Call::SubmitReview {
                review, verdict, body, ..
            } => self.graphql_request(
                SUBMIT_REVIEW,
                json!({ "review": review_id(review), "event": verdict.graphql(), "body": body }),
            ),
            Call::DeleteReview { review } => {
                self.graphql_request(DELETE_REVIEW, json!({ "review": review_id(review) }))
            }
        }
    }

    /// Send `call`. A `Create` answers with the new pull request.
    pub fn perform(&self, repo: &GitHubRepo, call: &Call, created: Option<u64>) -> Result<Value> {
        let request = self.call_request(repo, call, created);
        let response = self.call(&request)?;
        if let Some(message) = graphql_error(&response.body) {
            return Err(Error::GitHub {
                status: Some(response.status),
                message,
            });
        }
        Ok(response.body)
    }

    // Reading.

    /// Open requests are independent of recently closed history: old open requests cannot
    /// be displaced by a busy closed history. Reads are bounded to avoid monopolizing quota.
    pub fn pulls(&self, repo: &GitHubRepo) -> Result<PullList> {
        let mut items = Vec::new();
        let mut issues = Vec::new();
        for (state, section, most) in [("open", "Open", 10), ("closed", "Closed", 1)] {
            let loaded = self.pages(
                repo,
                &format!("/pulls?state={state}&sort=updated&direction=desc"),
                most,
                section,
                None,
            );
            items.extend(loaded.items.iter().map(summary));
            issues.extend(loaded.issue);
        }
        Ok(PullList { items, issues })
    }

    fn pages(&self, repo: &GitHubRepo, path: &str, most: u32, section: &str, key: Option<&str>) -> Loaded {
        let join = if path.contains('?') { '&' } else { '?' };
        read_pages(section, most, |page| {
            let request = self.api_request(
                "GET",
                &repo.path(&format!("{path}{join}per_page=100&page={page}")),
                None,
            );
            let body = self.call(&request)?.body;
            Ok(match key {
                Some(key) => body.get(key).cloned().unwrap_or(Value::Null),
                None => body,
            })
        })
    }

    fn threads(&self, repo: &GitHubRepo, number: u64) -> Loaded {
        let mut cursor = Value::Null;
        let mut items = Vec::new();
        for _ in 0..10 {
            let page = (|| -> Result<(Vec<Value>, bool, Value)> {
                let request = self.graphql_request(
                    THREADS,
                    json!({ "owner": repo.owner, "name": repo.name, "number": number, "cursor": cursor }),
                );
                let body = self.call(&request)?.body;
                if let Some(message) = graphql_error(&body) {
                    return Err(Error::Git(message));
                }
                let connection = body
                    .pointer("/data/repository/pullRequest/reviewThreads")
                    .ok_or_else(|| Error::Git("Threads were not returned.".into()))?;
                let nodes = connection
                    .get("nodes")
                    .and_then(Value::as_array)
                    .cloned()
                    .ok_or_else(|| Error::Git("Threads were not returned.".into()))?;
                let more = connection
                    .pointer("/pageInfo/hasNextPage")
                    .and_then(Value::as_bool)
                    .ok_or_else(|| Error::Git("Thread pagination was not returned.".into()))?;
                let next = connection
                    .pointer("/pageInfo/endCursor")
                    .cloned()
                    .unwrap_or(Value::Null);
                if more && (!next.is_string() || next == cursor) {
                    return Err(Error::Git("Thread pagination could not continue.".into()));
                }
                Ok((nodes, more, next))
            })();
            match page {
                Ok((nodes, more, next)) => {
                    items.extend(nodes);
                    if !more {
                        return Loaded { items, issue: None };
                    }
                    cursor = next;
                }
                Err(err) => {
                    let state = if items.is_empty() { "unavailable" } else { "partial" };
                    return Loaded {
                        items,
                        issue: Some(LoadIssue {
                            section: "Threads".into(),
                            state: state.into(),
                            message: err.to_string(),
                        }),
                    };
                }
            }
        }
        Loaded {
            items,
            issue: Some(LoadIssue {
                section: "Threads".into(),
                state: "partial".into(),
                message: "Showing the first 10 pages. Continue on the provider’s website.".into(),
            }),
        }
    }

    /// Details and successful pages, with explicit issues for unreadable or bounded sections.
    pub fn pull_request(&self, repo: &GitHubRepo, number: u64) -> Result<PullRequest> {
        let get = |path: String| {
            let request = self.api_request("GET", &repo.path(&path), None);
            move || self.call(&request).map(|r| r.body)
        };
        let all = |path: String, most: u32, section: &'static str| move || self.pages(repo, &path, most, section, None);
        let (pull, commits, comments, timeline, settings) = std::thread::scope(|s| {
            let pull = s.spawn(get(format!("/pulls/{number}")));
            // GitHub lists at most 250 commits of a pull request, however many pages are asked for.
            let commits = s.spawn(all(format!("/pulls/{number}/commits"), 3, "Commits"));
            let comments = s.spawn(all(format!("/pulls/{number}/comments"), 10, "Comments"));
            let timeline = s.spawn(all(format!("/issues/{number}/timeline"), 10, "Timeline"));
            let settings = s.spawn(get(String::new()));
            (
                pull.join(),
                commits.join(),
                comments.join(),
                timeline.join(),
                settings.join(),
            )
        });
        let joined = |r: std::thread::Result<Result<Value>>| {
            r.unwrap_or_else(|_| Err(Error::Git("The request worker stopped unexpectedly.".into())))
        };
        let pull = joined(pull)?;
        let summary = summary(&pull);
        let sha = summary.head_sha.clone();
        let base = summary.base.clone();
        let (runs, statuses, rules, threads) = std::thread::scope(|s| {
            let runs = s.spawn(|| {
                self.pages(
                    repo,
                    &format!("/commits/{sha}/check-runs"),
                    10,
                    "Checks",
                    Some("check_runs"),
                )
            });
            let statuses = s.spawn(|| self.pages(repo, &format!("/commits/{sha}/statuses"), 10, "Statuses", None));
            let rules = s.spawn(get(format!("/rules/branches/{}", path_part(&base))));
            let threads = s.spawn(|| self.threads(repo, number));
            (runs.join(), statuses.join(), rules.join(), threads.join())
        });
        let mut load_issues = Vec::new();
        let settings = repo_settings(&joined(settings)?);
        let commits = Value::Array(loaded_read("Commits", commits, &mut load_issues));
        let comments = Value::Array(loaded_read("Comments", comments, &mut load_issues));
        let timeline = Value::Array(loaded_read("Timeline", timeline, &mut load_issues));
        let threads = loaded_read("Threads", threads, &mut load_issues);
        let resolved = resolved_threads(
            &json!({ "data": { "repository": { "pullRequest": { "reviewThreads": { "nodes": threads } } } } }),
        );
        let runs = loaded_read("Checks", runs, &mut load_issues);
        let statuses = loaded_read("Statuses", statuses, &mut load_issues);
        let rules = optional_read("Merge rules", rules, &mut load_issues);

        let commits: Vec<PullCommit> = commits
            .as_array()
            .map(|c| c.iter().map(pull_commit).collect())
            .unwrap_or_default();
        let commit_count = pull.get("commits").and_then(Value::as_u64).unwrap_or(0);
        if (commits.len() as u64) < commit_count && !load_issues.iter().any(|i| i.section == "Commits") {
            load_issues.push(LoadIssue {
                section: "Commits".into(),
                state: "partial".into(),
                message: "GitHub does not expose every commit here. Continue on GitHub.".into(),
            });
        }
        let mut entries = conversation(&timeline, &comments, &resolved);
        entries.sort_by_key(Entry::time);
        let reviews = latest_reviews(&timeline);
        let checks = checks(&json!({"check_runs": runs}), &json!({"statuses": statuses}));
        let required_approvals = required_approvals(&rules);

        let number_field = |key: &str| pull.get(key).and_then(Value::as_u64).unwrap_or(0) as u32;
        let summary_state = summary.state;
        Ok(PullRequest {
            load_issues,
            additions: number_field("additions"),
            deletions: number_field("deletions"),
            changed_files: number_field("changed_files"),
            commit_count: number_field("commits"),
            commits_complete: commits.len() as u64 >= commit_count,
            mergeable: pull.get("mergeable").and_then(Value::as_bool),
            mergeable_state: pull
                .get("mergeable_state")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            merged_at: (summary_state == PullState::Merged).then(|| time(&pull, "merged_at")),
            merge_commit: (summary_state == PullState::Merged)
                .then(|| pull.get("merge_commit_sha").and_then(Value::as_str).map(str::to_owned))
                .flatten(),
            merged_by: pull
                .pointer("/merged_by/login")
                .and_then(Value::as_str)
                .map(str::to_owned),
            auto_merge: pull
                .pointer("/auto_merge/merge_method")
                .and_then(Value::as_str)
                .map(str::to_owned),
            commits,
            reviews,
            timeline: entries,
            checks,
            required_approvals,
            settings,
            gitlab: None,
            summary,
        })
    }
}

/// A review's id, or what stands for the one the run starts until it has one.
fn review_id(review: &Option<String>) -> &str {
    review.as_deref().unwrap_or("$REVIEW")
}

/// The id of the review a `StartReview` answered with.
pub fn started_review(answer: &Value) -> Option<String> {
    answer
        .pointer("/data/addPullRequestReview/pullRequestReview/id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The message of a GraphQL answer that failed, which comes with status 200.
pub(crate) fn graphql_error(body: &Value) -> Option<String> {
    let errors = body.get("errors")?.as_array()?;
    let messages: Vec<&str> = errors
        .iter()
        .filter_map(|e| e.get("message").and_then(Value::as_str))
        .collect();
    (!messages.is_empty()).then(|| messages.join(". "))
}

/// A branch name in a URL path: slashes stay, other unsafe characters are encoded.
fn path_part(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn time(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_str).map(parse_time).unwrap_or(0)
}

fn person(value: Option<&Value>) -> Person {
    let field = |key: &str| {
        value
            .and_then(|v| v.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Person {
        login: field("login"),
        avatar_url: field("avatar_url"),
    }
}

fn summary(pull: &Value) -> PullSummary {
    let merged = pull.get("merged_at").is_some_and(|m| !m.is_null())
        || pull.get("merged").and_then(Value::as_bool) == Some(true);
    let state = match pull.get("state").and_then(Value::as_str) {
        _ if merged => PullState::Merged,
        Some("closed") => PullState::Closed,
        _ => PullState::Open,
    };
    PullSummary {
        number: pull.get("number").and_then(Value::as_u64).unwrap_or(0),
        title: text(pull, "title"),
        state,
        draft: pull.get("draft").and_then(Value::as_bool).unwrap_or(false),
        head: pull
            .pointer("/head/ref")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        head_sha: pull
            .pointer("/head/sha")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        head_owner: pull
            .pointer("/head/repo/owner/login")
            .and_then(Value::as_str)
            .map(str::to_owned),
        base: pull
            .pointer("/base/ref")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        html_url: text(pull, "html_url"),
        body: text(pull, "body"),
        author: person(pull.get("user")),
        created: time(pull, "created_at"),
        updated: time(pull, "updated_at"),
        node_id: text(pull, "node_id"),
        requested: requested(pull),
    }
}

fn pull_commit(item: &Value) -> PullCommit {
    let message = item
        .pointer("/commit/message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    PullCommit {
        sha: text(item, "sha"),
        summary: message.lines().next().unwrap_or_default().to_owned(),
        author: item
            .pointer("/commit/author/name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        time: item
            .pointer("/commit/committer/date")
            .and_then(Value::as_str)
            .map(parse_time)
            .unwrap_or(0),
        message,
        additions: None,
        deletions: None,
    }
}

fn repo_settings(repo: &Value) -> RepoSettings {
    // Fields an account without admin rights doesn't get count as GitHub's defaults.
    let flag = |key: &str, default: bool| repo.get(key).and_then(Value::as_bool).unwrap_or(default);
    RepoSettings {
        merge_commit: flag("allow_merge_commit", true),
        squash: flag("allow_squash_merge", true),
        rebase: flag("allow_rebase_merge", true),
        auto_merge: flag("allow_auto_merge", false),
        delete_branch_on_merge: flag("delete_branch_on_merge", false),
        default_branch: repo
            .get("default_branch")
            .and_then(Value::as_str)
            .unwrap_or("main")
            .to_owned(),
        can_push: repo
            .pointer("/permissions/push")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn requested(pull: &Value) -> Vec<String> {
    let users = pull
        .get("requested_reviewers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|u| u.get("login").and_then(Value::as_str).map(str::to_owned));
    let owner = pull
        .pointer("/base/repo/owner/login")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let teams = pull
        .get("requested_teams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| {
            t.get("slug")
                .and_then(Value::as_str)
                .map(|slug| format!("{owner}/{slug}"))
        });
    users.chain(teams).collect()
}

fn review_state(state: &str) -> String {
    match state.to_ascii_lowercase().as_str() {
        "approved" => "approved",
        "changes_requested" => "changesRequested",
        "dismissed" => "dismissed",
        "pending" => "pending",
        _ => "commented",
    }
    .to_owned()
}

/// Each reviewer's latest review that counts: an approval or a request for changes outweighs a
/// later plain comment, as on GitHub.
fn latest_reviews(timeline: &Value) -> Vec<Review> {
    let mut order: Vec<String> = Vec::new();
    let mut latest: HashMap<String, Review> = HashMap::new();
    for item in timeline.as_array().into_iter().flatten() {
        if item.get("event").and_then(Value::as_str) != Some("reviewed") {
            continue;
        }
        let author = person(item.get("user"));
        let state = review_state(item.get("state").and_then(Value::as_str).unwrap_or_default());
        if state == "pending" || author.login.is_empty() {
            continue;
        }
        let review = Review {
            author: author.clone(),
            state,
            time: time(item, "submitted_at"),
        };
        match latest.get(&author.login) {
            None => {
                order.push(author.login.clone());
                latest.insert(author.login, review);
            }
            Some(before) if review.state != "commented" || before.state == "commented" => {
                latest.insert(author.login, review);
            }
            Some(_) => {}
        }
    }
    order.into_iter().filter_map(|login| latest.remove(&login)).collect()
}

/// The conversation from the timeline and the review comments: comments, reviews with something
/// to say, threads on lines of code and one-line events. Commits in a row are one event.
fn conversation(timeline: &Value, comments: &Value, resolved: &HashMap<u64, (bool, String)>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut pushed: Option<(String, usize, i64)> = None;
    let mut merged = false;
    let flush = |pushed: &mut Option<(String, usize, i64)>, out: &mut Vec<Entry>| {
        if let Some((actor, count, at)) = pushed.take() {
            out.push(Entry::Event {
                actor,
                text: format!("pushed {count} {}", if count == 1 { "commit" } else { "commits" }),
                icon: "push".into(),
                tone: None,
                time: at,
            });
        }
    };
    for item in timeline.as_array().into_iter().flatten() {
        let event = item.get("event").and_then(Value::as_str).unwrap_or_default();
        if event == "committed" {
            let author = item
                .pointer("/author/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let at = item
                .pointer("/committer/date")
                .and_then(Value::as_str)
                .map(parse_time)
                .unwrap_or(0);
            pushed = Some(match pushed.take() {
                Some((who, count, _)) => (who, count + 1, at),
                None => (author, 1, at),
            });
            continue;
        }
        flush(&mut pushed, &mut out);
        let actor = item
            .pointer("/actor/login")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let at = time(item, "created_at");
        merged |= event == "merged";
        let line = |text: String, icon: &str, tone: Option<&str>| Entry::Event {
            actor: actor.clone(),
            text,
            icon: icon.into(),
            tone: tone.map(str::to_owned),
            time: at,
        };
        let entry = match event {
            "commented" => Some(Entry::Comment {
                author: person(item.get("user").or(item.get("actor"))),
                body: text(item, "body"),
                time: at,
            }),
            "reviewed" => {
                let state = review_state(item.get("state").and_then(Value::as_str).unwrap_or_default());
                let body = text(item, "body");
                // A plain comment review with no words of its own only holds line comments,
                // which show as threads.
                (state != "pending" && !(state == "commented" && body.trim().is_empty())).then(|| Entry::Review {
                    author: person(item.get("user")),
                    state,
                    body,
                    time: time(item, "submitted_at"),
                })
            }
            "review_requested" => {
                let who = item
                    .pointer("/requested_reviewer/login")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        item.pointer("/requested_team/name")
                            .and_then(Value::as_str)
                            .map(|t| format!("team {t}"))
                    })
                    .unwrap_or_default();
                Some(line(format!("requested a review from {who}"), "review", None))
            }
            "merged" => {
                let sha = item.get("commit_id").and_then(Value::as_str).unwrap_or_default();
                Some(line(
                    format!("merged it as {}", &sha[..sha.len().min(7)]),
                    "ok",
                    Some("ok"),
                ))
            }
            // GitHub closes a pull request as it merges it: the merge says it all.
            "closed" if merged => None,
            "closed" => Some(line("closed it".into(), "fail", Some("bad"))),
            "reopened" => Some(line("reopened it".into(), "dot", None)),
            "ready_for_review" => Some(line("marked it ready for review".into(), "review", None)),
            "convert_to_draft" => Some(line("turned it back into a draft".into(), "draft", None)),
            "head_ref_force_pushed" => Some(line("force-pushed the branch".into(), "push", None)),
            "head_ref_deleted" => Some(line("deleted the branch".into(), "dot", None)),
            "head_ref_restored" => Some(line("restored the branch".into(), "dot", None)),
            "base_ref_changed" => Some(line("changed the base branch".into(), "dot", None)),
            "renamed" => {
                let to = item.pointer("/rename/to").and_then(Value::as_str).unwrap_or_default();
                Some(line(format!("renamed it to “{to}”"), "dot", None))
            }
            "auto_merge_enabled" => Some(line("turned on auto-merge".into(), "ok", None)),
            "auto_merge_disabled" => Some(line("turned off auto-merge".into(), "dot", None)),
            _ => None,
        };
        out.extend(entry);
    }
    flush(&mut pushed, &mut out);
    out.extend(threads(comments, resolved));
    out
}

/// Review comments grouped into threads: each first comment with the replies to it.
fn threads(comments: &Value, resolved: &HashMap<u64, (bool, String)>) -> Vec<Entry> {
    let all: Vec<&Value> = comments.as_array().into_iter().flatten().collect();
    let mut out = Vec::new();
    for first in all
        .iter()
        .filter(|c| c.get("in_reply_to_id").is_none_or(Value::is_null))
    {
        let id = first.get("id").and_then(Value::as_u64).unwrap_or(0);
        let comment = |c: &Value| Comment {
            author: person(c.get("user")),
            body: text(c, "body"),
            time: time(c, "created_at"),
        };
        let mut list = vec![comment(first)];
        list.extend(
            all.iter()
                .filter(|c| c.get("in_reply_to_id").and_then(Value::as_u64) == Some(id))
                .map(|c| comment(c)),
        );
        let number = |key: &str| first.get(key).and_then(Value::as_u64).map(|n| n as u32);
        let line = number("line");
        let thread = resolved.get(&id);
        out.push(Entry::Thread {
            id,
            node: thread.map(|(_, node)| node.clone()),
            path: text(first, "path"),
            hunk: hunk_tail(
                &text(first, "diff_hunk"),
                4,
                number("start_line").filter(|_| line.is_some() && text(first, "side") != "LEFT"),
            ),
            outdated: line.is_none(),
            start_line: if line.is_some() {
                number("start_line")
            } else {
                number("original_start_line")
            },
            side: first.get("side").and_then(Value::as_str).unwrap_or("RIGHT").to_owned(),
            line: line.or_else(|| number("original_line")),
            resolved: thread.map(|(resolved, _)| *resolved),
            commit: first.get("commit_id").and_then(Value::as_str).map(str::to_owned),
            time: list[0].time,
            comments: list,
        });
    }
    out
}

/// The last `count` lines of a review comment's hunk, numbered as in the new file; the last one
/// is the line commented on. A comment on new lines `from` and on keeps all of them, marked.
pub fn hunk_tail(hunk: &str, count: usize, from: Option<u32>) -> Vec<HunkLine> {
    let mut lines = hunk.lines();
    let start = lines
        .next()
        .and_then(|header| header.split('+').nth(1))
        .and_then(|new| new.split([',', ' ']).next())
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(1);
    let mut number = start;
    let mut out: Vec<HunkLine> = Vec::new();
    for line in lines {
        let (mark, rest) = line.split_at(line.len().min(1));
        let numbered = if mark == "-" {
            None
        } else {
            number += 1;
            Some(number - 1)
        };
        out.push(HunkLine {
            number: numbered,
            text: rest.to_owned(),
            mark: false,
        });
    }
    let mut skip = out.len().saturating_sub(count);
    if let Some(from) = from
        && let Some(at) = out.iter().position(|l| l.number == Some(from))
    {
        skip = skip.min(at);
        for line in &mut out[at..] {
            line.mark = line.number.is_some();
        }
    }
    let mut tail: Vec<HunkLine> = out.into_iter().skip(skip).collect();
    if let Some(last) = tail.last_mut() {
        last.mark = true;
    }
    tail
}

/// Whether each review thread is resolved, and its GraphQL id, by the id of its first comment.
fn resolved_threads(answer: &Value) -> HashMap<u64, (bool, String)> {
    answer
        .pointer("/data/repository/pullRequest/reviewThreads/nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|thread| {
            let id = thread.pointer("/comments/nodes/0/databaseId").and_then(Value::as_u64)?;
            Some((
                id,
                (
                    thread.get("isResolved").and_then(Value::as_bool).unwrap_or(false),
                    text(thread, "id"),
                ),
            ))
        })
        .collect()
}

/// Check runs and commit statuses as one list; a check run again only counts its latest run.
fn checks(runs: &Value, statuses: &Value) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut list: Vec<&Value> = runs
        .get("check_runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    list.sort_by_key(|run| std::cmp::Reverse(time(run, "started_at")));
    for run in list {
        let name = text(run, "name");
        if !seen.insert(name.clone()) {
            continue;
        }
        let state = match (
            run.get("status").and_then(Value::as_str),
            run.get("conclusion").and_then(Value::as_str),
        ) {
            (Some("completed"), Some("success")) => "ok",
            (Some("completed"), Some("skipped" | "neutral" | "stale")) => "skip",
            (Some("completed"), _) => "fail",
            _ => "run",
        };
        let (started, finished) = (time(run, "started_at"), time(run, "completed_at"));
        out.push(Check {
            name,
            detail: String::new(),
            state: state.into(),
            seconds: (state != "run" && started > 0 && finished >= started).then_some(finished - started),
            url: run.get("html_url").and_then(Value::as_str).map(str::to_owned),
        });
    }
    for status in statuses.get("statuses").and_then(Value::as_array).into_iter().flatten() {
        let name = text(status, "context");
        if !seen.insert(name.clone()) {
            continue;
        }
        let state = match status.get("state").and_then(Value::as_str) {
            Some("success") => "ok",
            Some("pending") => "run",
            _ => "fail",
        };
        out.push(Check {
            name,
            detail: text(status, "description"),
            state: state.into(),
            seconds: None,
            url: status.get("target_url").and_then(Value::as_str).map(str::to_owned),
        });
    }
    out
}

fn required_approvals(rules: &Value) -> Option<u32> {
    rules
        .as_array()?
        .iter()
        .filter(|rule| rule.get("type").and_then(Value::as_str) == Some("pull_request"))
        .filter_map(|rule| {
            rule.pointer("/parameters/required_approving_review_count")
                .and_then(Value::as_u64)
        })
        .max()
        .map(|n| n as u32)
}

/// The address of pull request `number` on github.com.
pub fn pull_url(repo: &GitHubRepo, number: u64) -> String {
    format!("{WEB}/{}/pull/{number}", repo.full_name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hunk_keeps_its_last_lines_numbered() {
        let hunk = "@@ -6,4 +6,5 @@ pub struct RefreshToken {\n pub struct RefreshToken {\n-    pub hash: String,\n+    pub hash: [u8; 32],\n+    pub family: Uuid,";
        let tail = hunk_tail(hunk, 3, None);
        let shown: Vec<(Option<u32>, &str, bool)> = tail.iter().map(|l| (l.number, l.text.as_str(), l.mark)).collect();
        assert_eq!(
            shown,
            [
                (None, "    pub hash: String,", false),
                (Some(7), "    pub hash: [u8; 32],", false),
                (Some(8), "    pub family: Uuid,", true)
            ]
        );
    }

    #[test]
    fn a_hunk_of_a_comment_on_lines_keeps_them_all_marked() {
        let hunk = "@@ -1,2 +1,6 @@\n a\n+b\n+c\n+d\n-x\n+e\n f";
        let tail = hunk_tail(hunk, 4, Some(2));
        let shown: Vec<(Option<u32>, bool)> = tail.iter().map(|l| (l.number, l.mark)).collect();
        assert_eq!(
            shown,
            [
                (Some(2), true),
                (Some(3), true),
                (Some(4), true),
                (None, false),
                (Some(5), true),
                (Some(6), true)
            ]
        );
    }

    #[test]
    fn calls_show_as_curl() {
        let repo = GitHubRepo {
            owner: "acme".into(),
            name: "api".into(),
            remote: "origin".into(),
        };
        let client = Client::default();
        let merge = Call::Merge {
            number: 418,
            method: Method::Squash,
            title: Some("Add models (#418)".into()),
            message: None,
            sha: "2d8b6e1".into(),
            delete_branch: true,
        };
        assert_eq!(
            client.call_request(&repo, &merge, None).display(),
            r#"curl -X PUT -H "Authorization: Bearer $GITHUB_TOKEN" https://api.github.com/repos/acme/api/pulls/418/merge -d '{"commit_title":"Add models (#418)","merge_method":"squash","sha":"2d8b6e1"}'"#
        );
        let reviewers = Call::RequestReviewers {
            number: 0,
            reviewers: vec!["maria".into(), "acme/core".into()],
            ids: vec![],
        };
        assert_eq!(
            client.call_request(&repo, &reviewers, None).display(),
            r#"curl -X POST -H "Authorization: Bearer $GITHUB_TOKEN" https://api.github.com/repos/acme/api/pulls/$NUMBER/requested_reviewers -d '{"reviewers":["maria"],"team_reviewers":["core"]}'"#
        );
        let update = Call::UpdateBranch {
            number: 419,
            sha: "9c1f2e7".into(),
        };
        assert_eq!(
            client.call_request(&repo, &update, None).display(),
            r#"curl -X PUT -H "Authorization: Bearer $GITHUB_TOKEN" https://api.github.com/repos/acme/api/pulls/419/update-branch -d '{"expected_head_sha":"9c1f2e7"}'"#
        );
        assert!(
            client
                .call_request(&repo, &reviewers, Some(422))
                .url
                .ends_with("/pulls/422/requested_reviewers")
        );
    }
}
