mod support;

use oxbow_core::overview::{RepoOverview, read};
use std::sync::atomic::AtomicBool;
use support::Fixture;

fn summary(fx: &Fixture) -> RepoOverview {
    read(fx.path(), &AtomicBool::new(false)).unwrap()
}

#[test]
fn unborn_detached_and_changes_are_explicit_and_files_are_not_counted_twice() {
    let mut fx = Fixture::new();
    let empty = summary(&fx);
    assert_eq!(empty.branch.as_deref(), Some("main"));
    assert!(empty.commit.is_none() && empty.upstream.is_none() && empty.ahead.is_none());
    fx.commit("file", "one\n", "first");
    fx.write("file", "two\n");
    fx.git(&["add", "file"]);
    fx.write("file", "three\n");
    fx.write("untracked\nname", "new\n");
    assert_eq!(summary(&fx).changed, 2);
    fx.git(&["checkout", "--detach", "-q"]);
    let detached = summary(&fx);
    assert!(detached.branch.is_none() && detached.commit.is_some());
    assert!(detached.upstream.is_none() && detached.behind.is_none());
}

#[test]
fn tracking_uses_only_local_refs_and_missing_upstream_is_unknown() {
    let mut fx = Fixture::new();
    let first = fx.commit("file", "one\n", "first");
    fx.git(&["remote", "add", "origin", "https://invalid.example/never-contact"]);
    fx.git(&["update-ref", "refs/remotes/origin/main", &first]);
    fx.git(&["branch", "--set-upstream-to=origin/main"]);
    assert_eq!((summary(&fx).ahead, summary(&fx).behind), (Some(0), Some(0)));
    fx.commit("local", "local", "local");
    assert_eq!(summary(&fx).ahead, Some(1));
    fx.git(&["checkout", "-b", "other", &first]);
    let remote = fx.commit("remote", "remote", "remote");
    fx.git(&["checkout", "main"]);
    fx.git(&["update-ref", "refs/remotes/origin/main", &remote]);
    let diverged = summary(&fx);
    assert_eq!((diverged.ahead, diverged.behind), (Some(1), Some(1)));
    fx.git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    let gone = summary(&fx);
    assert_eq!(gone.upstream.as_deref(), Some("origin/main"));
    assert_eq!((gone.ahead, gone.behind), (None, None));
}

#[test]
fn conflicts_are_distinct_from_other_changes() {
    let mut fx = Fixture::new();
    fx.commit("file", "base\n", "base");
    fx.git(&["checkout", "-b", "other"]);
    fx.commit("file", "other\n", "other");
    fx.git(&["checkout", "main"]);
    fx.commit("file", "main\n", "main");
    let output = std::process::Command::new("git")
        .args(["merge", "other"])
        .current_dir(fx.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    let s = summary(&fx);
    assert_eq!((s.changed, s.conflicts), (1, 1));
}

#[test]
fn passive_read_never_runs_helpers_or_changes_existing_restriction() {
    let mut fx = Fixture::new();
    fx.commit("file", "one\n", "first");
    // Would leave an observable marker even if status otherwise succeeded.
    fx.git(&["config", "core.fsmonitor", "touch helper-ran"]);
    fx.git(&["config", "filter.bad.clean", "touch helper-ran; cat"]);
    fx.git(&["config", "filter.bad.process", "touch helper-ran; cat"]);
    fx.git(&["config", "filter.bad.required", "true"]);
    fx.write(".gitattributes", "file filter=bad\n");
    fx.write("file", "changed\n");
    let restricted = oxbow_core::trust::is_restricted(fx.path());
    let s = summary(&fx);
    assert!(s.filters_disabled);
    assert_eq!(s.changed, 2);
    assert!(!fx.path().join("helper-ran").exists());
    assert_eq!(oxbow_core::trust::is_restricted(fx.path()), restricted);
    // The same guarantee when this repo is already open Restricted in another window.
    oxbow_core::trust::restrict(fx.path(), &oxbow_core::trust::scan(fx.path()).unwrap()).unwrap();
    assert_eq!(summary(&fx).changed, 2);
    assert!(oxbow_core::trust::is_restricted(fx.path()));
    oxbow_core::trust::lift(fx.path());
}

#[test]
fn cancellation_and_non_repository_child_do_not_return_a_misleading_summary() {
    let fx = Fixture::new();
    assert!(matches!(
        read(fx.path(), &AtomicBool::new(true)),
        Err(oxbow_core::Error::Cancelled)
    ));
    std::fs::create_dir(fx.path().join("ordinary")).unwrap();
    assert!(read(&fx.path().join("ordinary"), &AtomicBool::new(false)).is_err());
    assert!(read(&fx.path().join("missing"), &AtomicBool::new(false)).is_err());
}

#[test]
fn shallow_history_does_not_claim_a_complete_comparison() {
    let mut fx = Fixture::new();
    let tip = fx.commit("file", "one\n", "first");
    fx.git(&["remote", "add", "origin", "."]);
    fx.git(&["update-ref", "refs/remotes/origin/main", &tip]);
    fx.git(&["branch", "--set-upstream-to=origin/main"]);
    fx.write(".git/shallow", &format!("{tip}\n"));
    let s = summary(&fx);
    assert!(s.upstream.is_some());
    assert_eq!((s.ahead, s.behind), (None, None));
}

#[test]
fn welcome_keeps_stash_counts_and_paused_operation_without_loading_their_commits() {
    let mut fx = Fixture::new();
    fx.commit("file", "base\n", "base");
    for text in ["one\n", "two\n"] {
        fx.write("file", text);
        fx.git(&["stash", "push", "-q"]);
    }
    assert_eq!(summary(&fx).stashes, Some(2));
    // A read of the summary needs only these rebase markers, no patches or commit details.
    fx.git(&["checkout", "--detach", "-q"]);
    fx.write(".git/rebase-merge/head-name", "refs/heads/topic\n");
    let read = summary(&fx);
    assert_eq!(read.branch.as_deref(), Some("topic"));
    assert_eq!(read.operation.as_deref(), Some("Rebase paused"));
    assert_eq!(read.stashes, Some(2));
    assert_eq!(
        std::fs::read_to_string(fx.path().join(".git/rebase-merge/head-name")).unwrap(),
        "refs/heads/topic\n"
    );
}
