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
        skip_checks: false,
        sign_off: false,
        changelist: None,
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
            skip_checks: false,
            sign_off: false,
            changelist: None,
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
    let never = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let err = repo.perform_with(&discard, &mut |_| {}, &never).unwrap_err();
    assert!(matches!(err, oxbow_core::Error::NoUndo(_)));
    // Said in plain words, not as the system's «File exists (os error 17)».
    assert!(err.to_string().contains("a file named .git/oxbow"), "{err}");
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
        &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
        &std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
        fetch: None,
        stash: false,
    })
    .unwrap();
    let id = repo.operation_log().unwrap()[0].id.clone();
    let err = repo.plan_restore(&id, false).unwrap_err();
    assert!(err.to_string().contains("middle of a merge"), "{err}");
}

/// Set `cancel` from another thread once `ready` holds.
#[cfg(unix)]
fn stop_when(
    cancel: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ready: impl Fn() -> bool + Send + 'static,
) -> std::thread::JoinHandle<()> {
    let stop = cancel.clone();
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        while !ready() && start.elapsed() < std::time::Duration::from_secs(20) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    })
}

#[cfg(unix)]
#[test]
fn stop_ends_the_snapshot_before_an_action_and_nothing_runs() {
    let mut fx = Fixture::new();
    // A clean filter that takes long: saving the files for Undo runs it on the changed file.
    fx.commit(".gitattributes", "slow.txt filter=slow\n", "Attributes");
    fx.commit("slow.txt", "a\n", "Slow");
    fx.git(&["config", "filter.slow.clean", "touch filter-ran; sleep 20; cat"]);
    fx.write("slow.txt", "edited\n");
    fx.write("b.txt", "new\n");
    let _ = std::fs::remove_file(fx.path().join("filter-ran"));
    let repo = Repo::open(fx.path()).unwrap();
    let logged = repo.operation_log().unwrap().len();

    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let ran = fx.path().join("filter-ran");
    let stopper = stop_when(&cancel, move || ran.exists());
    let started = std::time::Instant::now();
    let err = repo
        .perform_with(
            &Action::Stage {
                paths: vec!["b.txt".into()],
            },
            &mut |_| {},
            &cancel,
        )
        .unwrap_err();
    stopper.join().unwrap();
    assert!(matches!(err, oxbow_core::Error::Cancelled), "{err}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "Stop waited for the snapshot: {:?}",
        started.elapsed()
    );
    // The action never ran, and there is no step to undo.
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(repo.operation_log().unwrap().len(), logged);
}

#[test]
fn stop_pressed_before_an_action_starts_runs_nothing() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("b.txt", "new\n");
    let repo = Repo::open(fx.path()).unwrap();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let err = repo
        .perform_with(
            &Action::Stage {
                paths: vec!["b.txt".into()],
            },
            &mut |_| {},
            &cancel,
        )
        .unwrap_err();
    assert!(matches!(err, oxbow_core::Error::Cancelled), "{err}");
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "");
    assert!(repo.operation_log().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn an_action_stopped_while_it_runs_is_still_written_down_for_undo() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "edited\n");
    fx.git(&["add", "a.txt"]);
    // The hook changes a file and then hangs: Stop ends it after it changed the file.
    let hook = fx.path().join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho made > hook-made.txt\nsleep 20\n").unwrap();
    let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&hook, permissions).unwrap();
    let repo = Repo::open(fx.path()).unwrap();

    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let made = fx.path().join("hook-made.txt");
    let stopper = stop_when(&cancel, move || made.exists());
    // Even with Stop set around it the way a read's is, the writing down isn't stopped.
    let err = oxbow_core::cli::stoppable(&cancel, || {
        repo.perform_with(
            &Action::Commit {
                message: "Second".into(),
                amend: false,
                allow_secrets: true,
                skip_checks: true,
                no_verify: false,
                sign_off: false,
                changelist: None,
            },
            &mut |_| {},
            &cancel,
        )
    })
    .unwrap_err();
    stopper.join().unwrap();
    assert!(matches!(err, oxbow_core::Error::Cancelled), "{err}");
    let log = repo.operation_log().unwrap();
    assert_eq!(log.len(), 1, "the stopped step isn't in the Operation Log");
    // Undo takes back the file the hook made.
    undo_last(&repo);
    assert_eq!(read(&fx, "hook-made.txt"), None);
}

#[test]
fn a_repository_opened_again_still_knows_where_git_keeps_its_files() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    fx.write("a.txt", "two\n");
    repo.perform(&Action::Stage {
        paths: vec!["a.txt".into()],
    })
    .unwrap();

    // As each read of the history opens it again.
    let repo = repo.reopen().unwrap();
    fx.write("a.txt", "three\n");
    oxbow_core::console::scope(
        Some(fx.path()),
        oxbow_core::console::EntryKind::Action,
        "Unstage",
        || {
            repo.perform(&Action::Unstage {
                paths: vec!["a.txt".into()],
            })
        },
    )
    .unwrap();
    let entries = oxbow_core::console::entries(Some(fx.path()));
    let ran: Vec<&str> = entries
        .iter()
        .filter(|entry| entry.title == "Unstage")
        .flat_map(|entry| &entry.commands)
        .map(|ran| ran.display.as_str())
        .collect();
    assert!(ran.iter().any(|ran| ran.starts_with("git restore")), "{ran:?}");
    assert!(!ran.iter().any(|ran| ran.contains("--git-path")), "{ran:?}");
    assert_eq!(repo.operation_log().unwrap().len(), 2);
}

/// Run `action` in a Console entry named `title`.
fn perform_as(repo: &Repo, fx: &Fixture, title: &str, action: Action) {
    oxbow_core::console::scope(Some(fx.path()), oxbow_core::console::EntryKind::Action, title, || {
        repo.perform(&action)
    })
    .unwrap();
}

/// Wait until the watch's count has moved on from `from` and then stayed still for a moment.
fn settled(watch: &oxbow_core::Watch, from: u64) {
    let started = std::time::Instant::now();
    let mut last = (watch.seen().unwrap(), std::time::Instant::now());
    while started.elapsed() < std::time::Duration::from_secs(5) {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let now = watch.seen().unwrap();
        if now != last.0 {
            last = (now, std::time::Instant::now());
        } else if now != from && last.1.elapsed() > std::time::Duration::from_millis(300) {
            return;
        }
    }
    panic!("the watch saw no change");
}

fn staged(fx: &mut Fixture) -> String {
    fx.git(&["diff", "--cached", "--name-only"])
}

/// A repository with `a.txt` and `b.txt` committed, watched as the window watches it, with
/// `a.txt` saved and staged once and the watch caught up: the next save is the one that must stay.
fn watched_after_a_stage() -> (Fixture, Repo, oxbow_core::Watch) {
    let mut fx = Fixture::new();
    fx.write("a.txt", "a1\n");
    fx.commit("b.txt", "b1\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    let watch = repo.watch(|_| {}).unwrap();
    assert!(watch.files);
    let from = watch.seen().unwrap();
    fx.write("a.txt", "first edit\n");
    settled(&watch, from);
    repo.perform(&stage("a.txt")).unwrap();
    (fx, repo, watch)
}

fn stage(path: &str) -> Action {
    Action::Stage {
        paths: vec![path.into()],
    }
}

// The watch hears of a save only a moment later: none of these waits for it before the step.

#[test]
fn undo_of_a_stage_keeps_the_file_as_saved_just_before_it() {
    let (mut fx, repo, _watch) = watched_after_a_stage();
    fx.write("a.txt", "second edit\n");
    repo.perform(&stage("a.txt")).unwrap();
    undo_last(&repo);
    assert_eq!(read(&fx, "a.txt").as_deref(), Some("second edit\n"));
    assert_eq!(fx.git(&["show", ":a.txt"]), "first edit");
}

#[test]
fn undo_of_a_stage_keeps_another_file_saved_just_before_it() {
    let (fx, repo, _watch) = watched_after_a_stage();
    fx.write("b.txt", "b2\n");
    fx.write("a.txt", "second edit\n");
    repo.perform(&stage("a.txt")).unwrap();
    undo_last(&repo);
    assert_eq!(read(&fx, "b.txt").as_deref(), Some("b2\n"));
    assert_eq!(read(&fx, "a.txt").as_deref(), Some("second edit\n"));
}

#[test]
fn undo_of_a_stage_keeps_a_new_file_made_just_before_it() {
    let (mut fx, repo, _watch) = watched_after_a_stage();
    fx.write("new.txt", "new\n");
    repo.perform(&stage("new.txt")).unwrap();
    undo_last(&repo);
    assert_eq!(read(&fx, "new.txt").as_deref(), Some("new\n"));
    assert_eq!(fx.git(&["status", "--porcelain", "--", "new.txt"]), "?? new.txt");
}

#[test]
fn saving_staging_and_undoing_in_quick_turns_loses_no_save() {
    let (mut fx, repo, _watch) = watched_after_a_stage();
    for turn in 0..5 {
        let text = format!("turn {turn}\n");
        fx.write("a.txt", &text);
        repo.perform(&stage("a.txt")).unwrap();
        undo_last(&repo);
        assert_eq!(read(&fx, "a.txt").as_deref(), Some(text.as_str()), "turn {turn}");
        // Unstaged again: the index is as before this turn's step.
        assert_eq!(fx.git(&["show", ":a.txt"]), "first edit", "turn {turn}");
    }
}

#[test]
fn what_is_staged_outside_between_two_steps_is_kept_by_undo() {
    let mut fx = Fixture::new();
    fx.write("a.txt", "a1\n");
    fx.commit("b.txt", "b1\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    let watch = repo.watch(|_| {}).unwrap();
    let from = watch.seen().unwrap();
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b2\n");
    settled(&watch, from);
    perform_as(
        &repo,
        &fx,
        "Stage",
        Action::Stage {
            paths: vec!["a.txt".into()],
        },
    );
    // Staged in a terminal: the watch counts no change of files for it, the index stages more.
    fx.git(&["add", "b.txt"]);
    perform_as(
        &repo,
        &fx,
        "Unstage",
        Action::Unstage {
            paths: vec!["a.txt".into()],
        },
    );
    assert_eq!(staged(&mut fx), "b.txt");
    undo_last(&repo);
    assert_eq!(staged(&mut fx), "a.txt\nb.txt");
}

#[test]
fn a_longer_expiry_of_the_logs_own_ref_counts() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.git(&["config", "gc.reflogExpireUnreachable", "40.days"]);
    fx.git(&["config", "gc.refs/oxbow/oplog.reflogExpireUnreachable", "5.days"]);
    fx.write("a.txt", "b\n");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::Stage {
        paths: vec!["a.txt".into()],
    })
    .unwrap();
    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].expires, log[0].time + 5 * 86_400);
}

#[test]
fn undo_switch_refuses_a_branch_taken_by_another_worktree() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "root\n", "Root");
    fx.git(&["switch", "-qc", "topic"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::Switch {
        branch: "main".into(),
        stash: false,
        keep: None,
    })
    .unwrap();
    let action = Action::Restore {
        id: repo.operation_log().unwrap()[0].id.clone(),
        after: false,
    };
    // Planning before the other checkout succeeds; execution must check again.
    repo.plan(&action).unwrap();
    let other = fx.path().parent().unwrap().join("other");
    fx.git(&["worktree", "add", other.to_str().unwrap(), "topic"]);
    let before = fx.git(&["status", "--porcelain=v2", "--branch"]);
    assert!(repo.plan(&action).unwrap_err().to_string().contains("checked out"));
    assert!(repo.perform(&action).unwrap_err().to_string().contains("checked out"));
    assert_eq!(fx.git(&["status", "--porcelain=v2", "--branch"]), before);
    assert_eq!(repo.head().unwrap().branch.as_deref(), Some("main"));
    fx.git(&["-C", other.to_str().unwrap(), "switch", "--detach"]);
    repo.perform(&action).unwrap();
    assert_eq!(repo.head().unwrap().branch.as_deref(), Some("topic"));
}
