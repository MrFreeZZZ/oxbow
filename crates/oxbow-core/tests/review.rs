//! Reviewing a pull request: what GitHub says of the open ones, files viewed, the calls of a
//! review, its head fetched here, and the draft kept between runs.

mod support;

use oxbow_core::Repo;
use oxbow_core::github::Client;
use oxbow_core::pulls::{Call, GitHubRepo, Spot, Verdict, started_review};
use oxbow_core::review::{DraftComment, DraftReply, ReviewDraft, pull_ref};
use support::{Fixture, stand_in};

fn acme() -> GitHubRepo {
    GitHubRepo {
        owner: "acme".into(),
        name: "api".into(),
        remote: "origin".into(),
    }
}

const STATUSES: &str = r#"{"data":{"repository":{"pullRequests":{"nodes":[
 {"number":421,"reviewDecision":"REVIEW_REQUIRED","commits":{"nodes":[{"commit":{"statusCheckRollup":{"state":"SUCCESS"}}}]}},
 {"number":419,"reviewDecision":"CHANGES_REQUESTED","commits":{"nodes":[{"commit":{"statusCheckRollup":null}}]}},
 {"number":418,"reviewDecision":null,"commits":{"nodes":[]}}]}}}}"#;

const FILES: &str = r#"{"data":{"repository":{"pullRequest":{
 "files":{"nodes":[{"path":"src/error.rs","viewerViewedState":"VIEWED"},{"path":"src/limit.rs","viewerViewedState":"UNVIEWED"},{"path":"src/routes.rs","viewerViewedState":"DISMISSED"}],
  "pageInfo":{"hasNextPage":false,"endCursor":"c1"}},
 "reviews":{"nodes":[{"id":"PRR_1","viewerDidAuthor":true,"commit":{"oid":"4e1a2b9"},"comments":{"totalCount":2}}]}}}}}"#;

#[test]
fn the_open_pull_requests_come_with_their_verdict_and_checks() {
    let (address, _) = stand_in(vec![("/graphql", STATUSES)]);
    let client = Client::new(&address, &address).with_token("t");
    let statuses = client.pull_statuses(&acme()).unwrap();
    let shapes: Vec<(u64, Option<&str>, Option<&str>)> = statuses
        .iter()
        .map(|s| (s.number, s.decision.as_deref(), s.checks.as_deref()))
        .collect();
    assert_eq!(
        shapes,
        [
            (421, Some("reviewRequired"), Some("success")),
            (419, Some("changesRequested"), None),
            (418, None, None)
        ]
    );
}

#[test]
fn files_viewed_and_a_review_started_on_github_are_read() {
    let (address, _) = stand_in(vec![("/graphql", FILES)]);
    let client = Client::new(&address, &address).with_token("t");
    let viewing = client.viewing(&acme(), 421).unwrap();
    assert_eq!(viewing.viewed, ["src/error.rs"]);
    assert_eq!(viewing.dismissed, ["src/routes.rs"]);
    let pending = viewing.pending.unwrap();
    assert_eq!((pending.id.as_str(), pending.comments), ("PRR_1", 2));
}

fn spot(line: u32, start: Option<u32>) -> Spot {
    Spot {
        path: "src/limit.rs".into(),
        line,
        side: "RIGHT".into(),
        start_line: start,
    }
}

/// A review goes as: start it on the commit read, its comments and replies, then the verdict;
/// until it is started, the calls after show `$REVIEW` for its id.
#[test]
fn a_review_is_sent_in_steps() {
    let (address, seen) = stand_in(vec![
        (
            "/graphql addPullRequestReview(",
            r#"{"data":{"addPullRequestReview":{"pullRequestReview":{"id":"PRR_9"}}}}"#,
        ),
        ("/graphql", r#"{"data":{}}"#),
        ("/repos/acme/api/pulls/421/comments", r#"{"id":31}"#),
    ]);
    let client = Client::new(&address, &address).with_token("t");
    let mut calls = [
        Call::StartReview {
            id: "PR_421".into(),
            commit: "4e1a2b9".into(),
        },
        Call::ReviewComment {
            review: None,
            spot: spot(14, Some(13)),
            body: "From config?".into(),
        },
        Call::ReviewReply {
            review: None,
            thread: "PRRT_5".into(),
            body: "Fine by me.".into(),
        },
        Call::SubmitReview {
            review: None,
            verdict: Verdict::RequestChanges,
            body: "Two things.".into(),
        },
    ];
    assert!(
        client
            .call_request(&acme(), &calls[1], None)
            .display()
            .contains("$REVIEW")
    );

    let answer = client.perform(&acme(), &calls[0], None).unwrap();
    let review = started_review(&answer).unwrap();
    assert_eq!(review, "PRR_9");
    for call in &mut calls[1..] {
        call.in_review(&review);
        client.perform(&acme(), call, None).unwrap();
    }
    let bodies: Vec<String> = seen.lock().unwrap().iter().map(|(_, body)| body.clone()).collect();
    assert!(
        bodies[1].contains(r#""startLine":13"#) && bodies[1].contains(r#""review":"PRR_9""#),
        "{}",
        bodies[1]
    );
    assert!(bodies[2].contains(r#""thread":"PRRT_5""#), "{}", bodies[2]);
    assert!(bodies[3].contains(r#""event":"REQUEST_CHANGES""#), "{}", bodies[3]);

    // A comment sent at once goes to REST, on the commit read.
    client
        .perform(
            &acme(),
            &Call::Comment {
                number: 421,
                commit: "4e1a2b9".into(),
                spot: spot(9, None),
                body: "Prune?".into(),
            },
            None,
        )
        .unwrap();
    let (path, body) = seen.lock().unwrap().last().unwrap().clone();
    assert_eq!(path, "/repos/acme/api/pulls/421/comments");
    assert!(
        body.contains(r#""commit_id":"4e1a2b9""#) && !body.contains("start_line"),
        "{body}"
    );
}

#[test]
fn a_pull_request_is_fetched_out_of_the_branch_lists() {
    let remote = tempfile::tempdir().unwrap();
    let url = remote.path().join("acme.git");
    let url = url.to_str().unwrap().to_owned();
    let mut github = Fixture::new();
    github.commit("a.txt", "one\n", "Root");
    github.git(&["clone", "-q", "--bare", ".", &url]);
    github.git(&["switch", "-q", "-c", "fork-work"]);
    let head = github.commit("limit.rs", "struct Limiter;\n", "Rate limit");
    github.git(&["push", "-q", &url, "fork-work:refs/pull/421/head"]);

    let mut me = Fixture::new();
    me.git(&["remote", "add", "origin", &url]);
    let repo = Repo::open(me.path()).unwrap();
    repo.run(&repo.pull_fetch_command("origin", 421, "main")).unwrap();
    assert_eq!(me.git(&["rev-parse", &pull_ref(421, "head")]), head);
    // Only the base shows, as its remote branch: git updates that one too.
    assert_eq!(me.git(&["branch", "-a"]), "remotes/origin/main");
    let compared = repo
        .compare(
            &pull_ref(421, "base"),
            &pull_ref(421, "head"),
            oxbow_core::CompareMode::Split,
        )
        .unwrap();
    let paths: Vec<&str> = compared.files.iter().map(|f| f.file.path.as_str()).collect();
    assert_eq!(paths, ["limit.rs"]);
}

#[test]
fn a_draft_lasts_until_it_is_put_away() {
    let mut me = Fixture::new();
    let head = me.commit("a.txt", "one\n", "Root");
    let repo = Repo::open(me.path()).unwrap();
    assert_eq!(repo.review_draft(&acme(), 421).unwrap(), None);

    let draft = ReviewDraft {
        commit: head.clone(),
        body: "Two things.".into(),
        verdict: Some(Verdict::Comment),
        comments: vec![DraftComment {
            id: "c1".into(),
            spot: spot(14, Some(13)),
            body: "From config?".into(),
            time: 1,
        }],
        replies: vec![DraftReply {
            id: "r1".into(),
            to: 11,
            thread: "PRRT_5".into(),
            body: "Fine by me.".into(),
            time: 2,
        }],
    };
    repo.save_review_draft(&acme(), 421, &draft).unwrap();
    // Another Repo, as after a restart.
    let again = Repo::open(me.path()).unwrap();
    assert_eq!(again.review_draft(&acme(), 421).unwrap().as_ref(), Some(&draft));
    assert_eq!(again.review_drafts(&acme()).len(), 1);
    assert_eq!(me.git(&["rev-parse", &pull_ref(421, "reviewed")]), head);

    again.drop_review_draft(&acme(), 421).unwrap();
    assert_eq!(again.review_draft(&acme(), 421).unwrap(), None);
    assert!(me.git(&["for-each-ref", "refs/oxbow/"]).is_empty());
    let removed = me.path().join(".git/oxbow/removed");
    let kept = std::fs::read_dir(removed).unwrap().count();
    assert_eq!(kept, 1);
}
