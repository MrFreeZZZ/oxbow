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

/// After GitHub squash-merged `feature`: delete it there and here, and bring `main` up to date.
#[test]
fn a_merged_pull_request_comes_home() {
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
    let url = url.to_str().unwrap();
    let mut me = Fixture::new();
    me.commit("a.txt", "one\n", "Root");
    me.git(&["remote", "add", "origin", url]);
    me.git(&["push", "-q", "-u", "origin", "main"]);
    me.git(&["switch", "-q", "-c", "feature"]);
    me.commit("b.txt", "two\n", "Two");
    me.commit("c.txt", "three\n", "Three");
    me.git(&["push", "-q", "-u", "origin", "feature"]);

    // GitHub squashes it into main.
    let mut github = Fixture::new();
    github.git(&["remote", "add", "origin", url]);
    github.git(&["fetch", "-q", "origin"]);
    github.git(&["checkout", "-q", "-B", "main", "origin/main"]);
    github.git(&["merge", "-q", "--squash", "origin/feature"]);
    github.git(&["commit", "-q", "-m", "Feature (#1)"]);
    github.git(&["push", "-q", "origin", "main"]);
    let squashed = github.git(&["rev-parse", "HEAD"]);

    let repo = Repo::open(me.path()).unwrap();
    let action = Action::PullRequestMerged {
        remote: "origin".into(),
        base: "main".into(),
        branch: "feature".into(),
        delete_remote: true,
        delete_local: true,
    };
    let shown: Vec<String> = repo
        .plan(&action)
        .unwrap()
        .commands
        .iter()
        .map(|c| c.display())
        .collect();
    assert_eq!(
        shown,
        [
            "git push origin --delete feature",
            "git fetch --prune origin",
            "git fetch origin main:main",
            "git switch main",
            "git branch -D feature"
        ]
    );
    repo.perform(&action).unwrap();
    assert_eq!(me.git(&["rev-parse", "main"]), squashed);
    assert_eq!(me.git(&["branch", "--show-current"]), "main");
    assert_eq!(me.git(&["branch", "--list", "feature"]), "");
    assert_eq!(me.git(&["ls-remote", "--heads", "origin", "feature"]), "");
}
