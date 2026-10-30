//! Reviewing a pull request: what GitHub says of the open ones, files viewed, the calls of a
//! review, its head fetched here, and the draft kept between runs.

mod support;

use oxbow_core::github::Client;
use oxbow_core::pulls::{Call, GitHubRepo, Spot, Verdict, started_review};
use oxbow_core::review::{DraftComment, DraftReply, ReviewDraft, Reviewed};
use oxbow_core::{Action, Repo};
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
        lines: None,
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
            revoke: false,
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
    repo.run(&repo.pull_fetch_command(&Reviewed::github(&acme()), "origin", 421, "main"))
        .unwrap();
    assert_eq!(
        me.git(&["rev-parse", &Reviewed::github(&acme()).reference(421, "head")]),
        head
    );
    // Only the base shows, as its remote branch: git updates that one too.
    assert_eq!(me.git(&["branch", "-a"]), "remotes/origin/main");
    let compared = repo
        .compare(
            &Reviewed::github(&acme()).reference(421, "base"),
            &Reviewed::github(&acme()).reference(421, "head"),
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
    assert_eq!(repo.review_draft(&Reviewed::github(&acme()), 421).unwrap(), None);

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
    repo.save_review_draft(&Reviewed::github(&acme()), 421, &draft).unwrap();
    // Another Repo, as after a restart.
    let again = Repo::open(me.path()).unwrap();
    assert_eq!(
        again.review_draft(&Reviewed::github(&acme()), 421).unwrap().as_ref(),
        Some(&draft)
    );
    assert_eq!(again.review_drafts(&Reviewed::github(&acme())).len(), 1);
    assert_eq!(
        me.git(&["rev-parse", &Reviewed::github(&acme()).reference(421, "reviewed")]),
        head
    );

    again.drop_review_draft(&Reviewed::github(&acme()), 421).unwrap();
    assert_eq!(again.review_draft(&Reviewed::github(&acme()), 421).unwrap(), None);
    assert!(me.git(&["for-each-ref", "refs/oxbow/"]).is_empty());
    let removed = me.path().join(".git/oxbow/removed");
    let kept = std::fs::read_dir(removed).unwrap().count();
    assert_eq!(kept, 1);
}

fn team_app() -> oxbow_core::GitLabProject {
    oxbow_core::GitLabProject {
        remote: "origin".into(),
        host: "git.corp".into(),
        path: "team/app".into(),
        web_url: "https://git.corp/team/app".into(),
    }
}

#[test]
fn a_merge_requests_draft_and_viewed_ticks_stay_here() {
    let mut me = Fixture::new();
    let head = me.commit("a.txt", "one\n", "Root");
    me.commit("b.txt", "two\n", "Second");
    let head2 = me.git(&["rev-parse", "HEAD"]);
    me.git(&[
        "update-ref",
        &Reviewed::gitlab(&team_app()).reference(7, "head"),
        &head2,
    ]);
    let repo = Repo::open(me.path()).unwrap();
    let lab = Reviewed::gitlab(&team_app());

    let draft = ReviewDraft {
        commit: head.clone(),
        body: String::new(),
        verdict: None,
        comments: Vec::new(),
        replies: Vec::new(),
    };
    repo.save_review_draft(&lab, 7, &draft).unwrap();
    assert!(me.path().join(".git/oxbow/reviews").join(lab.draft_name(7)).exists());
    assert_eq!(
        me.git(&["rev-parse", &Reviewed::gitlab(&team_app()).reference(7, "reviewed")]),
        head
    );
    assert_eq!(repo.review_drafts(&lab).len(), 1);
    // A pull request of the same number on GitHub has a draft of its own.
    assert!(repo.review_drafts(&Reviewed::github(&acme())).is_empty());

    repo.set_viewed_here(&lab, 7, "a.txt", true).unwrap();
    repo.set_viewed_here(&lab, 7, "b.txt", true).unwrap();
    repo.set_viewed_here(&lab, 7, "gone.txt", true).unwrap();
    // The viewed file doesn't read as a draft.
    assert_eq!(repo.review_drafts(&lab).len(), 1);
    let v = repo.viewed_here(&lab, 7);
    assert_eq!(v.viewed, ["a.txt", "b.txt", "gone.txt"]);
    assert!(v.dismissed.is_empty());

    // The author pushes a change to b.txt and deletes nothing: b.txt is unticked, as changed.
    me.commit("b.txt", "two, changed\n", "Third");
    let head3 = me.git(&["rev-parse", "HEAD"]);
    me.git(&[
        "update-ref",
        &Reviewed::gitlab(&team_app()).reference(7, "head"),
        &head3,
    ]);
    let v = repo.viewed_here(&lab, 7);
    assert_eq!(v.viewed, ["a.txt", "gone.txt"]);
    assert_eq!(v.dismissed, ["b.txt"]);

    repo.set_viewed_here(&lab, 7, "a.txt", false).unwrap();
    assert_eq!(repo.viewed_here(&lab, 7).viewed, ["gone.txt"]);
}

// --- Apply Suggestion ---

const TEN: &str = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";

fn suggest(commit: &str, start: u32, end: u32, lines: &str) -> Action {
    Action::ApplySuggestion {
        commit: commit.into(),
        path: "a.txt".into(),
        start,
        end,
        // Written out as a block would be: none for "".
        lines: if lines.is_empty() {
            Vec::new()
        } else {
            lines.split('\n').map(str::to_owned).collect()
        },
    }
}

fn read(fx: &Fixture) -> String {
    std::fs::read_to_string(fx.path().join("a.txt")).unwrap()
}

#[test]
fn a_suggestion_replaces_its_lines_on_disk_and_undo_takes_it_back() {
    let mut fx = Fixture::new();
    let head = fx.commit("a.txt", TEN, "Ten");
    let repo = Repo::open(fx.path()).unwrap();
    let action = suggest(&head, 4, 5, "four\nfive");
    let plan = repo.plan(&action).unwrap();
    assert_eq!(plan.commands[0].display(), "git apply -");
    repo.perform(&action).unwrap();
    assert_eq!(read(&fx), TEN.replace("4\n5\n", "four\nfive\n"));
    // Nothing is staged or committed: the author commits it with the rest.
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), head);
    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, "Apply a suggestion to a.txt");
    repo.perform(&Action::Restore {
        id: log[0].id.clone(),
        after: false,
    })
    .unwrap();
    assert_eq!(read(&fx), TEN);
}

#[test]
fn a_suggestion_finds_its_lines_after_edits_above_them() {
    let mut fx = Fixture::new();
    let head = fx.commit("a.txt", TEN, "Ten");
    // Two lines added at the top since: the suggested ones moved down.
    fx.write("a.txt", &format!("new\nnewer\n{TEN}"));
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&suggest(&head, 7, 7, "seven")).unwrap();
    assert_eq!(read(&fx), format!("new\nnewer\n{}", TEN.replace("7\n", "seven\n")));
}

#[test]
fn a_suggestion_is_merged_in_when_its_context_changed() {
    let mut fx = Fixture::new();
    let head = fx.commit("a.txt", TEN, "Ten");
    fx.commit("a.txt", &TEN.replace("3\n", "three\n"), "Spell three");
    let repo = Repo::open(fx.path()).unwrap();
    let action = suggest(&head, 5, 5, "five");
    assert_eq!(repo.plan(&action).unwrap().commands[0].display(), "git apply --3way -");
    repo.perform(&action).unwrap();
    assert_eq!(read(&fx), TEN.replace("3\n", "three\n").replace("5\n", "five\n"));
}

#[test]
fn a_suggestion_on_lines_changed_since_is_refused_before_anything_runs() {
    let mut fx = Fixture::new();
    let head = fx.commit("a.txt", TEN, "Ten");
    fx.commit("a.txt", &TEN.replace("5\n", "FIVE\n"), "Shout five");
    let repo = Repo::open(fx.path()).unwrap();
    let steps = repo.operation_log().unwrap().len();
    let err = repo.perform(&suggest(&head, 5, 5, "five")).unwrap_err().to_string();
    assert!(err.contains("conflicts with the file as it is now"), "{err}");
    assert!(err.contains("Change the file by hand"), "{err}");
    assert_eq!(repo.operation_log().unwrap().len(), steps);
    assert_eq!(read(&fx), TEN.replace("5\n", "FIVE\n"));
}

#[test]
fn a_suggestion_on_lines_the_file_has_not_got_says_so() {
    let mut fx = Fixture::new();
    let head = fx.commit("a.txt", TEN, "Ten");
    let repo = Repo::open(fx.path()).unwrap();
    let err = repo.plan(&suggest(&head, 11, 12, "x")).unwrap_err().to_string();
    assert!(err.contains("no lines 11–12"), "{err}");
    let err = repo.plan(&suggest(&head, 2, 2, "2")).unwrap_err().to_string();
    assert!(err.contains("already has the suggested lines"), "{err}");
    let err = repo
        .plan(&suggest("0123456789012345678901234567890123456789", 1, 1, "x"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("fetch the pull request first"), "{err}");
}

fn local_draft(commit: &str, body: &str) -> ReviewDraft {
    ReviewDraft {
        commit: commit.into(),
        body: body.into(),
        verdict: None,
        comments: vec![],
        replies: vec![],
    }
}

#[test]
fn review_projects_with_ambiguous_old_names_have_independent_files_and_refs() {
    let mut fx = Fixture::new();
    let head = fx.commit("a.txt", "one\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    let mut one = team_app();
    one.path = "a-b/c".into();
    let mut two = one.clone();
    two.path = "a/b-c".into();
    let mut three = one.clone();
    three.host = "other.example".into();
    let places = [
        Reviewed::gitlab(&one),
        Reviewed::gitlab(&two),
        Reviewed::gitlab(&three),
        Reviewed::github(&GitHubRepo {
            owner: "a-b".into(),
            name: "c".into(),
            remote: "origin".into(),
        }),
        Reviewed::github(&GitHubRepo {
            owner: "a".into(),
            name: "b-c".into(),
            remote: "origin".into(),
        }),
    ];
    for (i, place) in places.iter().enumerate() {
        repo.save_review_draft(place, 7, &local_draft(&head, &i.to_string()))
            .unwrap();
    }
    for (i, place) in places.iter().enumerate() {
        assert_eq!(repo.review_draft(place, 7).unwrap().unwrap().body, i.to_string());
        assert_eq!(repo.review_drafts(place).len(), 1);
        for other in places.iter().skip(i + 1) {
            assert_ne!(place.draft_name(7), other.draft_name(7));
            assert_ne!(place.reference(7, "head"), other.reference(7, "head"));
        }
    }
}

#[test]
fn concurrent_writers_replace_only_the_revision_they_read() {
    let mut fx = Fixture::new();
    let head = fx.commit("a.txt", "one\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    let place = Reviewed::github(&acme());
    let initial = repo.review_draft_state(&place, 7).unwrap().revision;
    let barrier = std::sync::Barrier::new(8);
    let successes = std::thread::scope(|scope| {
        let tasks: Vec<_> = (0..8)
            .map(|n| {
                let (repo, place, initial, head, barrier) = (&repo, &place, &initial, &head, &barrier);
                scope.spawn(move || {
                    let draft = local_draft(head, &n.to_string().repeat(100_000));
                    barrier.wait();
                    repo.update_review_draft(place, 7, Some(&draft), initial).is_ok()
                })
            })
            .collect();
        tasks
            .into_iter()
            .map(|task| usize::from(task.join().unwrap()))
            .sum::<usize>()
    });
    assert_eq!(successes, 1);
    assert!(repo.review_draft(&place, 7).unwrap().is_some());
    // The deletion revision survives: neither an old existing draft nor a missing one revives it.
    let written = repo.review_draft_state(&place, 7).unwrap().revision;
    let deleted = repo.update_review_draft(&place, 7, None, &written).unwrap();
    for stale in [&initial, &written] {
        assert!(
            repo.update_review_draft(&place, 7, Some(&local_draft(&head, "stale")), stale)
                .is_err()
        );
    }
    assert!(repo.review_draft(&place, 7).unwrap().is_none());
    repo.update_review_draft(&place, 7, Some(&local_draft(&head, "new editor")), &deleted)
        .unwrap();
}

#[test]
fn parallel_unchecked_saves_use_separate_temporary_files() {
    let mut fx = Fixture::new();
    let head = fx.commit("a.txt", "one\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    let place = Reviewed::github(&acme());
    let barrier = std::sync::Barrier::new(8);
    std::thread::scope(|scope| {
        let tasks: Vec<_> = (0..8)
            .map(|n| {
                let (repo, place, head, barrier) = (&repo, &place, &head, &barrier);
                scope.spawn(move || {
                    barrier.wait();
                    repo.save_review_draft(place, 7, &local_draft(head, &n.to_string().repeat(100_000)))
                        .unwrap();
                })
            })
            .collect();
        for task in tasks {
            task.join().unwrap();
        }
    });
    assert_eq!(repo.review_draft(&place, 7).unwrap().unwrap().body.len(), 100_000);
}

#[test]
fn an_unattributed_legacy_draft_is_preserved_and_never_assigned_by_guessing() {
    let mut fx = Fixture::new();
    let head = fx.commit("a.txt", "one\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    let place = Reviewed::gitlab(&team_app());
    let folder = fx.path().join(".git/oxbow/reviews");
    std::fs::create_dir_all(&folder).unwrap();
    let old = folder.join("gitlab-team-app-7.json");
    let bytes = serde_json::to_vec(&local_draft(&head, "keep this draft")).unwrap();
    std::fs::write(&old, &bytes).unwrap();
    let err = repo.review_draft_state(&place, 7).unwrap_err().to_string();
    assert!(err.contains("project was not recorded"), "{err}");
    assert_eq!(std::fs::read(&old).unwrap(), bytes);
    let attributed = folder.join(place.draft_name(7));
    assert!(!attributed.exists());
    // Explicit attribution preserves the old plain JSON; the next save upgrades its envelope.
    std::fs::rename(&old, &attributed).unwrap();
    let imported = repo.review_draft_state(&place, 7).unwrap();
    assert_eq!(imported.draft.as_ref().unwrap().body, "keep this draft");
    repo.update_review_draft(&place, 7, imported.draft.as_ref(), &imported.revision)
        .unwrap();
    assert_ne!(repo.review_draft_state(&place, 7).unwrap().revision, imported.revision);
}
