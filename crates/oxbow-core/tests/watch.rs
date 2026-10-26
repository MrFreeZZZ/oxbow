mod support;

use std::sync::mpsc;
use std::time::Duration;

use oxbow_core::{Changed, Repo};
use support::Fixture;

/// Long enough for a change to be told after its quiet spell.
const TOLD: Duration = Duration::from_secs(5);
/// Long enough that a change that matters would have been told.
const QUIET: Duration = Duration::from_millis(2500);

fn watched(fixture: &Fixture) -> (oxbow_core::Watch, mpsc::Receiver<Changed>) {
    let repo = Repo::open(fixture.path()).unwrap();
    let (tx, rx) = mpsc::channel();
    let watch = repo
        .watch(move |changed| {
            let _ = tx.send(changed);
        })
        .unwrap();
    assert!(watch.files);
    (watch, rx)
}

fn base() -> Fixture {
    let mut fixture = Fixture::new();
    fixture.write(".gitignore", "target/\n*.log\n");
    fixture.commit("a.txt", "one\n", "First");
    fixture
}

#[test]
fn a_commit_outside_is_one_change_in_git() {
    let mut fixture = base();
    let (_watch, rx) = watched(&fixture);
    fixture.commit("a.txt", "two\n", "Second");
    let changed = rx.recv_timeout(TOLD).expect("the commit is told");
    assert!(changed.git);
    assert!(changed.at > 0);
    // Its writes were told together.
    assert!(rx.recv_timeout(QUIET).is_err());
}

#[test]
fn staging_and_stashing_are_changes_in_git() {
    let mut fixture = base();
    let (_watch, rx) = watched(&fixture);
    fixture.write("a.txt", "edited\n");
    let changed = rx.recv_timeout(TOLD).expect("the edit is told");
    assert!(changed.files && !changed.git);
    fixture.git(&["add", "a.txt"]);
    assert!(rx.recv_timeout(TOLD).expect("staging is told").git);
    fixture.git(&["stash", "-q"]);
    assert!(rx.recv_timeout(TOLD).expect("the stash is told").git);
}

#[test]
fn ignored_files_and_status_rewriting_the_index_are_not_changes() {
    let mut fixture = base();
    let (_watch, rx) = watched(&fixture);
    std::fs::create_dir_all(fixture.path().join("target/debug/deps")).unwrap();
    for n in 0..50 {
        fixture.write(&format!("target/debug/deps/{n}.o"), "built");
    }
    fixture.write("build.log", "lines");
    assert!(rx.recv_timeout(QUIET).is_err(), "ignored files aren't told");
    // Touching a tracked file without changing it, then `git status`, rewrites the index's times.
    fixture.write("a.txt", "two\n");
    let _ = rx.recv_timeout(TOLD);
    fixture.write("a.txt", "one\n");
    // Whatever the edits are told as, all of it, also when a busy machine tells it late.
    while rx.recv_timeout(QUIET).is_ok() {}
    fixture.git(&["status", "--porcelain"]);
    // An edit the system tells very late may still come; the index is no change in git.
    if let Ok(changed) = rx.recv_timeout(QUIET) {
        assert!(!changed.git, "a rewritten index that stages the same isn't told");
    }
}

#[test]
fn a_commit_in_another_worktree_is_told_in_the_main_one() {
    let mut fixture = base();
    let other = fixture.path().parent().unwrap().join("feature");
    fixture.git(&["worktree", "add", "-q", "-b", "feature", other.to_str().unwrap()]);
    let (_watch, rx) = watched(&fixture);
    std::fs::write(other.join("b.txt"), "b\n").unwrap();
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(&other)
            .env("GIT_AUTHOR_NAME", "A")
            .env("GIT_AUTHOR_EMAIL", "a@example.com")
            .env("GIT_COMMITTER_NAME", "A")
            .env("GIT_COMMITTER_EMAIL", "a@example.com")
            .status()
            .unwrap();
        assert!(status.success());
    };
    git(&["add", "b.txt"]);
    git(&["commit", "-q", "-m", "In the other folder"]);
    let changed = rx.recv_timeout(TOLD).expect("the other worktree's commit is told");
    assert!(changed.git);
}

#[test]
fn the_watch_stops_when_dropped() {
    let mut fixture = base();
    let (watch, rx) = watched(&fixture);
    drop(watch);
    fixture.commit("a.txt", "two\n", "Second");
    assert!(rx.recv_timeout(QUIET).is_err());
}

#[test]
fn seen_counts_changes_as_they_come() {
    let mut fixture = base();
    let (watch, rx) = watched(&fixture);
    let start = watch.seen().unwrap();
    // `git status` rewrites the index and an ignored file changes: nothing to count.
    fixture.git(&["status", "--porcelain"]);
    fixture.write("debug.log", "noise\n");
    std::thread::sleep(QUIET);
    assert_eq!(watch.seen(), Some(start));
    fixture.write("a.txt", "edited\n");
    // Counted by the time it is told.
    rx.recv_timeout(TOLD).expect("the edit is told");
    let edited = watch.seen().unwrap();
    assert!(edited > start);
    fixture.git(&["branch", "other"]);
    rx.recv_timeout(TOLD).expect("the branch is told");
    assert!(watch.seen().unwrap() > edited);
}
