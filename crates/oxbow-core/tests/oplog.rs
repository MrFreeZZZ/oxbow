mod support;

use oxbow_core::{Action, Repo, ResetMode};
use support::Fixture;

fn read(fx: &Fixture, file: &str) -> Option<String> {
    std::fs::read_to_string(fx.path().join(file)).ok()
}

fn undo_last(repo: &Repo) {
    let log = repo.operation_log().unwrap();
    repo.perform(&Action::Restore {
        id: log[0].id.clone(),
        after: false,
    })
    .unwrap();
}

#[test]
fn a_discard_is_undone_with_untracked_files_and_staged_changes() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "a staged\n");
    fx.git(&["add", "a.txt"]);
    fx.write("a.txt", "a staged\nand more\n");
    fx.write("notes/new.txt", "new\n");
    let repo = Repo::open(fx.path()).unwrap();

    repo.perform(&Action::Discard {
        paths: vec!["a.txt".into(), "notes/new.txt".into()],
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt").unwrap(), "a staged\n");
    assert!(read(&fx, "notes/new.txt").is_none());

    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, "Discard changes in 2 files");
    assert_eq!(log[0].kind, "discard");
    let plan = repo
        .plan(&Action::Restore {
            id: log[0].id.clone(),
            after: false,
        })
        .unwrap();
    let shown: Vec<_> = plan.commands.iter().map(|c| c.display()).collect();
    assert!(shown.iter().any(|c| c.starts_with("git read-tree ")), "{shown:?}");
    assert!(shown.last().unwrap().starts_with("git restore --source="), "{shown:?}");

    undo_last(&repo);
    assert_eq!(read(&fx, "a.txt").unwrap(), "a staged\nand more\n");
    assert_eq!(read(&fx, "notes/new.txt").unwrap(), "new\n");
    // The staged version is staged again.
    assert_eq!(fx.git(&["show", ":a.txt"]), "a staged");
    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, "Undo “Discard changes in 2 files”");
}

#[test]
fn a_commit_and_a_hard_reset_are_undone_and_redone() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    fx.write("a.txt", "b\n");
    fx.git(&["add", "a.txt"]);
    let repo = Repo::open(fx.path()).unwrap();

    repo.perform(&Action::Commit {
        message: "Change a".into(),
        amend: false,
        no_verify: false,
        allow_secrets: false,
    })
    .unwrap();
    let commit = fx.git(&["rev-parse", "HEAD"]);
    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, "Commit to main");
    assert_eq!(log[0].detail, "Change a");

    // Undo the commit: main goes back and the change is staged again.
    undo_last(&repo);
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), root);
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "a.txt");

    // Undo the undo: the commit is back.
    undo_last(&repo);
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), commit);
    assert_eq!(fx.git(&["status", "--porcelain"]), "");

    // A hard reset with uncommitted work, undone.
    fx.write("a.txt", "work in progress\n");
    repo.perform(&Action::Reset {
        commit: root.clone(),
        mode: ResetMode::Hard,
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt").unwrap(), "a\n");
    undo_last(&repo);
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), commit);
    assert_eq!(read(&fx, "a.txt").unwrap(), "work in progress\n");
}

#[test]
fn restore_to_here_goes_back_several_steps() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    let dir = fx.path().to_path_buf();
    let commit = |text: &str| {
        std::fs::write(dir.join("a.txt"), text).unwrap();
        repo.perform(&Action::Stage {
            paths: vec!["a.txt".into()],
        })
        .unwrap();
        repo.perform(&Action::Commit {
            message: text.trim().into(),
            amend: false,
            no_verify: false,
            allow_secrets: false,
        })
        .unwrap();
    };
    commit("one\n");
    let one = fx.git(&["rev-parse", "HEAD"]);
    commit("two\n");
    commit("three\n");
    let log = repo.operation_log().unwrap();
    assert_eq!(log.len(), 6);
    // Right after the commit of "one".
    let first_commit = &log[4];
    assert_eq!(first_commit.detail, "one");
    repo.perform(&Action::Restore {
        id: first_commit.id.clone(),
        after: true,
    })
    .unwrap();
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), one);
    assert_eq!(read(&fx, "a.txt").unwrap(), "one\n");
    assert_eq!(fx.git(&["status", "--porcelain"]), "");
}

#[test]
fn deleted_branches_tags_and_stashes_come_back() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.git(&["branch", "topic"]);
    fx.git(&["tag", "-a", "v1", "-m", "Version one"]);
    let tag = fx.git(&["rev-parse", "v1"]);
    fx.write("a.txt", "stashed\n");
    fx.git(&["stash", "push", "-m", "keep me"]);
    let repo = Repo::open(fx.path()).unwrap();

    repo.perform(&Action::DeleteBranch {
        name: "topic".into(),
        force: false,
        upstream: None,
    })
    .unwrap();
    repo.perform(&Action::DeleteTag {
        name: "v1".into(),
        remote: None,
    })
    .unwrap();
    let stash = repo.stashes().unwrap()[0].id.clone();
    repo.perform(&Action::StashDrop { index: 0, id: stash }).unwrap();
    assert!(repo.stashes().unwrap().is_empty());

    let log = repo.operation_log().unwrap();
    assert_eq!(log[2].title, "Delete branch topic");
    // Restoring to before the first step undoes all three.
    repo.perform(&Action::Restore {
        id: log[2].id.clone(),
        after: false,
    })
    .unwrap();
    assert_eq!(fx.git(&["branch", "--list", "topic"]), "topic");
    assert_eq!(fx.git(&["rev-parse", "v1"]), tag);
    assert_eq!(fx.git(&["stash", "list"]), "stash@{0}: On main: keep me");
}

#[test]
fn the_log_adds_nothing_to_the_history_and_skips_steps_that_changed_nothing() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "b\n");
    fx.git(&["add", "a.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    // Staging what is already staged changes nothing.
    repo.perform(&Action::Stage {
        paths: vec!["a.txt".into()],
    })
    .unwrap();
    assert!(repo.operation_log().unwrap().is_empty());

    repo.perform(&Action::Discard {
        paths: vec!["a.txt".into()],
    })
    .ok();
    repo.perform(&Action::Unstage {
        paths: vec!["a.txt".into()],
    })
    .unwrap();
    assert_eq!(repo.operation_log().unwrap().len(), 1);
    assert_eq!(fx.git(&["log", "--all", "--format=%s"]), "Root");
    assert_eq!(
        fx.git(&["rev-parse", "refs/oxbow/oplog"]),
        fx.git(&["rev-parse", "HEAD"])
    );

    repo.perform(&Action::ClearOperationLog).unwrap();
    assert!(repo.operation_log().unwrap().is_empty());
}

#[test]
fn nothing_runs_when_the_state_before_cant_be_saved_unless_chosen() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "edited\n");
    let repo = Repo::open(fx.path()).unwrap();
    // Where the snapshot's scratch index goes is a file: no snapshot can be taken.
    std::fs::write(fx.path().join(".git/oxbow"), "in the way").unwrap();
    let discard = Action::Discard {
        paths: vec!["a.txt".into()],
    };
    let never = std::sync::atomic::AtomicBool::new(false);
    let err = repo.perform_with(&discard, &mut |_| {}, &never).unwrap_err();
    assert!(matches!(err, oxbow_core::Error::NoUndo(_)));
    assert_eq!(
        oxbow_core::classify_failure(&err, false),
        oxbow_core::FailureKind::NoUndo
    );
    assert_eq!(read(&fx, "a.txt").unwrap(), "edited\n");

    // Chosen: it runs, and says it isn't in the log.
    let mut events = Vec::new();
    repo.perform_without_undo(&discard, &mut |e| events.push(e), &never)
        .unwrap();
    assert_eq!(read(&fx, "a.txt").unwrap(), "a\n");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, oxbow_core::ActionEvent::NotLogged { .. }))
    );
}

#[test]
fn a_step_that_cant_be_written_is_reported_apart_from_the_action() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "edited\n");
    let repo = Repo::open(fx.path()).unwrap();
    // The log's file is a folder: the step can't be written down.
    std::fs::create_dir_all(fx.path().join(".git/oxbow/oplog.jsonl")).unwrap();
    let mut events = Vec::new();
    repo.perform_with(
        &Action::Discard {
            paths: vec!["a.txt".into()],
        },
        &mut |e| events.push(e),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(read(&fx, "a.txt").unwrap(), "a\n");
    let reasons: Vec<&String> = events
        .iter()
        .filter_map(|e| match e {
            oxbow_core::ActionEvent::NotLogged { reason } => Some(reason),
            _ => None,
        })
        .collect();
    assert_eq!(reasons.len(), 1);
}

#[test]
fn a_step_written_down_reports_nothing() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "edited\n");
    let repo = Repo::open(fx.path()).unwrap();
    let mut events = Vec::new();
    repo.perform_with(
        &Action::Discard {
            paths: vec!["a.txt".into()],
        },
        &mut |e| events.push(e),
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, oxbow_core::ActionEvent::NotLogged { .. }))
    );
    assert_eq!(repo.operation_log().unwrap().len(), 1);
}

#[test]
fn a_log_that_cant_be_read_is_an_error_not_an_empty_log() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.operation_log().unwrap().is_empty());
    std::fs::create_dir_all(fx.path().join(".git/oxbow/oplog.jsonl")).unwrap();
    let err = repo.operation_log().unwrap_err().to_string();
    assert!(err.starts_with("oplog.jsonl: "), "{err}");
}

#[test]
fn a_snapshot_is_whole_while_something_else_holds_the_index() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "edited\n");
    fx.write("new.txt", "new\n");
    let repo = Repo::open(fx.path()).unwrap();
    let before = repo.snapshot().unwrap();

    // A `git status` or a commit in a terminal holds index.lock for a moment.
    let lock = fx.path().join(".git/index.lock");
    std::fs::write(&lock, "").unwrap();
    assert_eq!(repo.snapshot().unwrap(), before);
    std::fs::remove_file(&lock).unwrap();

    // Snapshots taken side by side, as a compare next to a step takes them.
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                for _ in 0..4 {
                    assert_eq!(repo.snapshot().unwrap(), before);
                }
            });
        }
    });
}

#[test]
fn a_step_taken_during_a_merge_with_conflicts_cant_be_gone_back_to() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.git(&["checkout", "-q", "-b", "topic"]);
    fx.commit("a.txt", "topic\n", "Topic");
    fx.git(&["checkout", "-q", "main"]);
    fx.commit("a.txt", "main\n", "Main");
    let _ = std::process::Command::new("git")
        .args(["merge", "-q", "topic"])
        .current_dir(fx.path())
        .output()
        .unwrap();
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::CreateBranch {
        name: "fix".into(),
        start: None,
        switch: false,
        publish: None,
    })
    .unwrap();
    let id = repo.operation_log().unwrap()[0].id.clone();
    let err = repo.plan_restore(&id, false).unwrap_err();
    assert!(err.to_string().contains("middle of a merge"), "{err}");
}
