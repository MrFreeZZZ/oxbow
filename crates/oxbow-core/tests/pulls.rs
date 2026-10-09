//! Pull requests: reading one from a stand-in for GitHub's API, and bringing a merge home.

mod support;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::thread;

use oxbow_core::github::Client;
use oxbow_core::pulls::{Call, Entry, GitHubRepo, Method, PullState};
use oxbow_core::{Action, Repo};
use support::Fixture;

/// Answer every request by its path: the first route whose prefix matches, else 404.
fn server(routes: Vec<(&'static str, &'static str)>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
                if let Some(value) = header.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
            let (status, answer) = routes
                .iter()
                .find(|(prefix, _)| path.starts_with(prefix))
                .map_or((404, r#"{"message":"Not Found"}"#), |(_, answer)| (200, answer));
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    address
}

fn acme() -> GitHubRepo {
    GitHubRepo {
        owner: "acme".into(),
        name: "api".into(),
        remote: "origin".into(),
    }
}

const PULL: &str = r#"{"number":419,"title":"Expose /v2/sessions endpoint","state":"open","draft":false,"node_id":"PR_419",
 "html_url":"https://github.com/acme/api/pull/419","user":{"login":"alexander","avatar_url":"https://a/1"},
 "created_at":"2026-10-08T10:00:00Z","updated_at":"2026-10-08T12:00:00Z","body":"Adds handlers.",
 "head":{"ref":"auth/2-api","sha":"e17a5c9aaaa","repo":{"owner":{"login":"acme"}}},
 "base":{"ref":"auth/1-models","repo":{"owner":{"login":"acme"}}},
 "additions":94,"deletions":12,"changed_files":3,"commits":2,"mergeable":true,"mergeable_state":"blocked",
 "merged_at":null,"auto_merge":null,
 "requested_reviewers":[{"login":"maria"}],"requested_teams":[{"slug":"core"}]}"#;

const COMMITS: &str = r#"[
 {"sha":"7c0d4f2","commit":{"message":"Validate refresh token rotation","author":{"name":"Alexander"},"committer":{"date":"2026-10-08T09:00:00Z"}}},
 {"sha":"e17a5c9","commit":{"message":"Expose /v2/sessions endpoint\n\nGET and DELETE.","author":{"name":"Alexander"},"committer":{"date":"2026-10-08T09:30:00Z"}}}]"#;

const COMMENTS: &str = r#"[
 {"id":11,"path":"src/api/sessions.rs","line":42,"original_line":42,"diff_hunk":"@@ -40,2 +40,3 @@\n pub async fn revoke() {\n+    app.sessions.revoke(id).await?;","body":"Revoking twice returns 500.","user":{"login":"kirill","avatar_url":"https://a/3"},"created_at":"2026-10-08T10:10:00Z"},
 {"id":12,"in_reply_to_id":11,"path":"src/api/sessions.rs","line":42,"diff_hunk":"","body":"Will fix.","user":{"login":"alexander","avatar_url":"https://a/1"},"created_at":"2026-10-08T10:20:00Z"}]"#;

const TIMELINE: &str = r#"[
 {"event":"committed","author":{"name":"Alexander"},"committer":{"date":"2026-10-08T09:00:00Z"}},
 {"event":"committed","author":{"name":"Alexander"},"committer":{"date":"2026-10-08T09:30:00Z"}},
 {"event":"review_requested","actor":{"login":"alexander"},"requested_reviewer":{"login":"maria"},"created_at":"2026-10-08T10:05:00Z"},
 {"event":"reviewed","user":{"login":"kirill","avatar_url":"https://a/3"},"state":"changes_requested","body":"Make revoke idempotent.","submitted_at":"2026-10-08T10:11:00Z"},
 {"event":"reviewed","user":{"login":"kirill","avatar_url":"https://a/3"},"state":"commented","body":"","submitted_at":"2026-10-08T10:12:00Z"},
 {"event":"commented","actor":{"login":"maria","avatar_url":"https://a/2"},"user":{"login":"maria","avatar_url":"https://a/2"},"body":"Looking today.","created_at":"2026-10-08T11:00:00Z"},
 {"event":"labeled","actor":{"login":"maria"},"created_at":"2026-10-08T11:01:00Z"}]"#;

const REPO: &str = r#"{"allow_merge_commit":false,"allow_squash_merge":true,"allow_rebase_merge":true,"allow_auto_merge":true,
 "delete_branch_on_merge":false,"default_branch":"main","permissions":{"push":true}}"#;

const RUNS: &str = r#"{"check_runs":[
 {"name":"test","status":"completed","conclusion":"failure","started_at":"2026-10-08T09:31:00Z","completed_at":"2026-10-08T09:33:47Z","html_url":"https://ci/1"},
 {"name":"test","status":"completed","conclusion":"success","started_at":"2026-10-08T09:20:00Z","completed_at":"2026-10-08T09:21:00Z"},
 {"name":"build","status":"in_progress","conclusion":null,"started_at":"2026-10-08T09:31:00Z","completed_at":null}]}"#;

const STATUS: &str = r#"{"statuses":[{"context":"ci/lint","state":"success","description":"No problems"}]}"#;

const RULES: &str = r#"[{"type":"pull_request","parameters":{"required_approving_review_count":1}}]"#;

const THREADS: &str = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"isResolved":false,"comments":{"nodes":[{"databaseId":11}]}}]}}}}}"#;

#[test]
fn a_pull_request_is_read_in_full() {
    let address = server(vec![
        ("/repos/acme/api/pulls/419/commits", COMMITS),
        ("/repos/acme/api/pulls/419/comments", COMMENTS),
        ("/repos/acme/api/pulls/419", PULL),
        ("/repos/acme/api/issues/419/timeline", TIMELINE),
        ("/repos/acme/api/commits/e17a5c9aaaa/check-runs", RUNS),
        ("/repos/acme/api/commits/e17a5c9aaaa/status", STATUS),
        ("/repos/acme/api/rules/branches/auth/1-models", RULES),
        ("/graphql", THREADS),
        ("/repos/acme/api", REPO),
    ]);
    let client = Client::new(&address, &address).with_token("t");
    let pr = client.pull_request(&acme(), 419).unwrap();

    assert_eq!(pr.summary.state, PullState::Open);
    assert_eq!(pr.summary.base, "auth/1-models");
    assert_eq!((pr.additions, pr.deletions, pr.commit_count), (94, 12, 2));
    assert_eq!(pr.mergeable_state, "blocked");
    assert_eq!(pr.requested, ["maria", "acme/core"]);
    assert_eq!(pr.commits[1].summary, "Expose /v2/sessions endpoint");
    assert_eq!(pr.required_approvals, Some(1));
    assert!(!pr.settings.merge_commit && pr.settings.squash && pr.settings.auto_merge && pr.settings.can_push);

    // A later plain comment doesn't take back a request for changes.
    assert_eq!(pr.reviews.len(), 1);
    assert_eq!(
        (pr.reviews[0].author.login.as_str(), pr.reviews[0].state.as_str()),
        ("kirill", "changesRequested")
    );

    let shapes: Vec<String> = pr
        .timeline
        .iter()
        .map(|e| match e {
            Entry::Event { text, .. } => format!("event: {text}"),
            Entry::Review { state, .. } => format!("review: {state}"),
            Entry::Comment { body, .. } => format!("comment: {body}"),
            Entry::Thread {
                comments,
                resolved,
                line,
                ..
            } => format!("thread on {line:?}: {} comments, resolved {resolved:?}", comments.len()),
        })
        .collect();
    assert_eq!(
        shapes,
        [
            "event: pushed 2 commits",
            "event: requested a review from maria",
            "thread on Some(42): 2 comments, resolved Some(false)",
            "review: changesRequested",
            "comment: Looking today."
        ]
    );

    let checks: Vec<(&str, &str, Option<i64>)> = pr
        .checks
        .iter()
        .map(|c| (c.name.as_str(), c.state.as_str(), c.seconds))
        .collect();
    assert_eq!(
        checks,
        [
            ("test", "fail", Some(167)),
            ("build", "run", None),
            ("ci/lint", "ok", None)
        ]
    );
}

#[test]
fn calls_reach_github_and_graphql_errors_count() {
    let address = server(vec![
        (
            "/repos/acme/api/pulls",
            r#"{"number":422,"html_url":"https://github.com/acme/api/pull/422"}"#,
        ),
        (
            "/graphql",
            r#"{"errors":[{"message":"Auto merge is not allowed for this repository"}]}"#,
        ),
    ]);
    let client = Client::new(&address, &address).with_token("t");
    let created = client
        .perform(
            &acme(),
            &Call::Create {
                head: "auth/3-ui".into(),
                base: "auth/2-api".into(),
                title: "UI".into(),
                body: String::new(),
                draft: true,
            },
            None,
        )
        .unwrap();
    assert_eq!(created["number"], 422);
    let err = client
        .perform(
            &acme(),
            &Call::AutoMerge {
                number: 419,
                id: "PR_419".into(),
                method: Some(Method::Squash),
            },
            None,
        )
        .unwrap_err();
    assert!(err.to_string().contains("Auto merge is not allowed"), "{err}");
}

#[test]
fn the_github_remote_is_found() {
    let mut fixture = Fixture::new();
    fixture.commit("a.txt", "a\n", "Root");
    fixture.git(&["remote", "add", "backup", "/srv/backup.git"]);
    fixture.git(&["remote", "add", "origin", "git@github.com:acme/api.git"]);
    let repo = Repo::open(fixture.path()).unwrap();
    assert_eq!(repo.github_repo(), Some(acme()));
}

#[test]
fn code_owners_name_who_reviews_the_changed_files() {
    let mut fixture = Fixture::new();
    fixture.write(".github/CODEOWNERS", "* @acme/core\n/src/api/ @kirill\ndocs/* @maria\n");
    fixture.commit("README.md", "hi\n", "Root");
    fixture.git(&["switch", "-q", "-c", "feature"]);
    fixture.commit("src/api/sessions.rs", "fn revoke() {}\n", "Revoke");
    fixture.commit("docs/api.md", "# API\n", "Docs");
    let repo = Repo::open(fixture.path()).unwrap();
    assert_eq!(repo.code_owners("main", "feature"), ["maria", "kirill"]);
}

/// A bare repository standing in for GitHub, and a clone of it with `main` pushed.
fn hosted() -> (tempfile::TempDir, String, Fixture) {
    let remote = tempfile::tempdir().unwrap();
    let url = remote.path().join("acme.git");
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .arg(&url)
            .status()
            .unwrap()
            .success()
    );
    let url = url.to_str().unwrap().to_owned();
    let mut me = Fixture::new();
    me.commit("a.txt", "one\n", "Root");
    me.git(&["remote", "add", "origin", &url]);
    me.git(&["push", "-q", "-u", "origin", "main"]);
    (remote, url, me)
}

/// GitHub squash-merges `branch` into main; returns the new main.
fn squash_on_github(url: &str, branch: &str) -> String {
    let mut github = Fixture::new();
    github.git(&["remote", "add", "origin", url]);
    github.git(&["fetch", "-q", "origin"]);
    github.git(&["checkout", "-q", "-B", "main", "origin/main"]);
    github.git(&["merge", "-q", "--squash", &format!("origin/{branch}")]);
    github.git(&["commit", "-q", "-m", "Feature (#1)"]);
    github.git(&["push", "-q", "origin", "main"]);
    github.git(&["rev-parse", "HEAD"])
}

fn shown(repo: &Repo, action: &Action) -> Vec<String> {
    repo.plan(action)
        .unwrap()
        .commands
        .iter()
        .map(|c| c.display())
        .collect()
}

/// After GitHub squash-merged `feature`: bring `main` up to date, then delete the branch there
/// and here, each only while it is still at the merged commit.
#[test]
fn a_merged_pull_request_comes_home() {
    let (_remote, url, mut me) = hosted();
    me.git(&["switch", "-q", "-c", "feature"]);
    me.commit("b.txt", "two\n", "Two");
    let tip = me.commit("c.txt", "three\n", "Three");
    me.git(&["push", "-q", "-u", "origin", "feature"]);
    let squashed = squash_on_github(&url, "feature");

    let repo = Repo::open(me.path()).unwrap();
    let landed = Action::PullRequestMerged {
        remote: "origin".into(),
        base: "main".into(),
        branch: "feature".into(),
    };
    assert_eq!(
        shown(&repo, &landed),
        ["git fetch --prune origin", "git fetch origin main:main"]
    );
    let remote = Action::DeleteRemoteBranch {
        remote: "origin".into(),
        branch: "feature".into(),
        expect: Some(tip.clone()),
    };
    assert_eq!(
        shown(&repo, &remote),
        [format!(
            "git push --force-with-lease=refs/heads/feature:{tip} origin --delete feature"
        )]
    );
    let local = Action::DeleteMergedBranch {
        base: "main".into(),
        branch: "feature".into(),
        sha: tip.clone(),
    };
    assert_eq!(
        shown(&repo, &local),
        [
            "git switch main".to_owned(),
            format!("git update-ref -d refs/heads/feature {tip}"),
            "git config --remove-section branch.feature".to_owned(),
        ]
    );
    repo.perform(&landed).unwrap();
    repo.perform(&remote).unwrap();
    repo.perform(&local).unwrap();
    assert_eq!(me.git(&["rev-parse", "main"]), squashed);
    assert_eq!(me.git(&["branch", "--show-current"]), "main");
    assert_eq!(me.git(&["branch", "--list", "feature"]), "");
    assert!(!me.git(&["config", "--get-regexp", "^branch\\."]).contains("feature"));
    assert_eq!(me.git(&["ls-remote", "--heads", "origin", "feature"]), "");
}

/// A main that is checked out catches up too, with a fast-forward merge, not a fetch into it.
#[test]
fn a_checked_out_base_catches_up() {
    let (_remote, url, mut me) = hosted();
    me.git(&["switch", "-q", "-c", "feature"]);
    me.commit("b.txt", "two\n", "Two");
    me.git(&["push", "-q", "-u", "origin", "feature"]);
    me.git(&["switch", "-q", "main"]);
    let squashed = squash_on_github(&url, "feature");

    let repo = Repo::open(me.path()).unwrap();
    let landed = Action::PullRequestMerged {
        remote: "origin".into(),
        base: "main".into(),
        branch: "feature".into(),
    };
    assert_eq!(
        shown(&repo, &landed),
        ["git fetch --prune origin", "git merge --ff-only origin/main"]
    );
    repo.perform(&landed).unwrap();
    assert_eq!(me.git(&["rev-parse", "HEAD"]), squashed);
    assert!(me.path().join("b.txt").exists());
}

/// Commits pushed or made after the merge keep the branch, here and on the remote.
#[test]
fn a_branch_that_moved_since_the_merge_stays() {
    let (_remote, url, mut me) = hosted();
    me.git(&["switch", "-q", "-c", "feature"]);
    let merged = me.commit("b.txt", "two\n", "Two");
    me.git(&["push", "-q", "-u", "origin", "feature"]);
    squash_on_github(&url, "feature");
    // Someone pushes to the branch after the merge, and a commit is made here.
    let mut other = Fixture::new();
    other.git(&["remote", "add", "origin", &url]);
    other.git(&["fetch", "-q", "origin"]);
    other.git(&["checkout", "-q", "-B", "feature", "origin/feature"]);
    let theirs = other.commit("late.txt", "late\n", "Late");
    other.git(&["push", "-q", "origin", "feature"]);
    let mine = me.commit("mine.txt", "mine\n", "Mine");
    me.git(&["switch", "-q", "main"]);

    let repo = Repo::open(me.path()).unwrap();
    let remote = Action::DeleteRemoteBranch {
        remote: "origin".into(),
        branch: "feature".into(),
        expect: Some(merged.clone()),
    };
    assert!(repo.perform(&remote).is_err());
    assert!(
        me.git(&["ls-remote", "--heads", "origin", "feature"])
            .starts_with(&theirs)
    );
    let local = Action::DeleteMergedBranch {
        base: "main".into(),
        branch: "feature".into(),
        sha: merged,
    };
    assert!(repo.perform(&local).is_err());
    assert_eq!(me.git(&["rev-parse", "feature"]), mine);
    assert_eq!(me.git(&["config", "--get", "branch.feature.remote"]), "origin");
}

/// A stack moves only onto a branch that has the merge: not a local main that doesn't have it yet.
#[test]
fn a_restack_lands_where_the_merge_is() {
    let (_remote, url, mut me) = hosted();
    me.git(&["switch", "-q", "-c", "feature"]);
    me.commit("b.txt", "two\n", "Two");
    me.git(&["push", "-q", "-u", "origin", "feature"]);
    let squashed = squash_on_github(&url, "feature");
    let candidates = ["main".to_owned(), "origin/main".to_owned()];

    let repo = Repo::open(me.path()).unwrap();
    // The merge commit isn't here at all yet.
    assert!(repo.landed_on(&squashed, &candidates).is_none());
    me.git(&["fetch", "-q", "origin"]);
    let landing = repo.landed_on(&squashed, &candidates).unwrap();
    assert_eq!(
        (landing.name.as_str(), landing.tip.id.as_str()),
        ("origin/main", squashed.as_str())
    );
    me.git(&["fetch", "-q", "origin", "main:main"]);
    assert_eq!(repo.landed_on(&squashed, &candidates).unwrap().name, "main");
}

/// Answers a list page by page: `per_page` items on each page until `total`.
fn paged_server(total: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
            }
            let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
            let answer = if path.contains("/commits?") || path.contains("/timeline?") || path.contains("/comments?") {
                let page: usize = path.rsplit("page=").next().and_then(|p| p.parse().ok()).unwrap_or(1);
                let from = (page - 1) * 100;
                let items: Vec<String> = if path.contains("/commits?") {
                    (from..total.min(from + 100))
                        .map(|i| {
                            format!(
                                r#"{{"sha":"c{i}","commit":{{"message":"Commit {i}","author":{{"name":"A"}},"committer":{{"date":"2026-10-08T09:00:00Z"}}}}}}"#
                            )
                        })
                        .collect()
                } else if path.contains("/timeline?") && page == 2 {
                    vec![r#"{"event":"reviewed","user":{"login":"maria","avatar_url":"https://a/2"},"state":"approved","body":"","submitted_at":"2026-10-09T10:00:00Z"}"#.to_owned()]
                } else if path.contains("/timeline?") && page == 1 {
                    (0..100)
                        .map(|_| {
                            r#"{"event":"labeled","actor":{"login":"maria"},"created_at":"2026-10-08T11:01:00Z"}"#
                                .to_owned()
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                format!("[{}]", items.join(","))
            } else if path.starts_with("/repos/acme/api/pulls/419") {
                PULL.replace(r#""commits":2"#, &format!(r#""commits":{total}"#))
            } else if path == "/repos/acme/api" {
                REPO.to_owned()
            } else {
                "{}".to_owned()
            };
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 200 X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    address
}

/// Lists longer than a page are read to the end: the 101st commit and a review on page two.
#[test]
fn every_page_of_a_pull_request_is_read() {
    let address = paged_server(130);
    let client = Client::new(&address, &address).with_token("t");
    let pr = client.pull_request(&acme(), 419).unwrap();
    assert_eq!(pr.commits.len(), 130);
    assert_eq!(pr.commits[129].summary, "Commit 129");
    assert!(pr.commits_complete);
    assert_eq!(pr.reviews.len(), 1);
    assert_eq!(pr.reviews[0].state, "approved");
}

/// GitHub stops at 250 commits: a bigger pull request says its list is not complete.
#[test]
fn more_commits_than_github_lists_are_marked() {
    let address = paged_server(320);
    let client = Client::new(&address, &address).with_token("t");
    let pr = client.pull_request(&acme(), 419).unwrap();
    assert_eq!(pr.commits.len(), 300);
    assert!(!pr.commits_complete);
}
