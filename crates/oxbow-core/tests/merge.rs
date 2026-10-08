mod support;

use oxbow_core::{Action, Chunk, ConflictSide, FailureKind, MergeMethod, OperationKind, Pick, Repo};
use support::Fixture;

/// `main` and `feature` both change the second line of `a.txt`; `feature` also adds `b.txt`.
fn diverged() -> Fixture {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\ntwo\nthree\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.commit("a.txt", "one\nTWO on feature\nthree\n", "Shout two");
    fx.commit("b.txt", "new\n", "Add b");
    fx.git(&["switch", "-q", "main"]);
    fx.commit("a.txt", "one\n2 on main\nthree\n", "Number two");
    fx
}

fn merge(branch: &str, method: MergeMethod, message: Option<&str>) -> Action {
    Action::Merge {
        branch: branch.into(),
        method,
        message: message.map(Into::into),
    }
}

fn read(fx: &Fixture, file: &str) -> String {
    std::fs::read_to_string(fx.path().join(file)).unwrap()
}

#[test]
fn the_preview_lists_both_sides_and_predicts_the_conflict() {
    let fx = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    let preview = repo.merge_preview("feature").unwrap();
    assert_eq!(preview.incoming_count, 2);
    assert_eq!(preview.incoming[0].summary, "Add b");
    assert_eq!(preview.ours_count, 1);
    assert_eq!(preview.base.unwrap().summary, "Root");
    assert_eq!(preview.conflicts.unwrap(), ["a.txt"]);
    assert_eq!((preview.files, preview.touched_here), (2, 1));
}

#[test]
fn a_clean_merge_makes_a_merge_commit() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.commit("b.txt", "b\n", "Add b");
    fx.git(&["switch", "-q", "main"]);
    let repo = Repo::open(fx.path()).unwrap();

    let preview = repo.merge_preview("feature").unwrap();
    assert_eq!(preview.conflicts.unwrap().len(), 0);
    assert_eq!(preview.ours_count, 0);

    let action = merge("feature", MergeMethod::Merge, Some("Merge branch 'feature' into main"));
    assert_eq!(
        repo.plan(&action).unwrap().commands[0].display(),
        "git merge --no-ff -m 'Merge branch '\\''feature'\\'' into main' feature"
    );
    repo.perform(&action).unwrap();
    // A merge commit, even though a fast-forward was possible.
    assert_eq!(fx.git(&["log", "-1", "--format=%P"]).split(' ').count(), 2);
    assert_eq!(
        fx.git(&["log", "-1", "--format=%s"]),
        "Merge branch 'feature' into main"
    );
}

#[test]
fn fast_forward_moves_the_branch_without_a_commit() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    let tip = fx.commit("b.txt", "b\n", "Add b");
    fx.git(&["switch", "-q", "main"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::FastForward, None)).unwrap();
    assert_eq!(fx.git(&["rev-parse", "main"]), tip);
}

#[test]
fn a_merge_that_conflicts_is_resolved_pick_by_pick_and_committed() {
    let mut fx = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.operation().unwrap().is_none());

    let err = repo
        .perform(&merge("feature", MergeMethod::Merge, Some("Bring in feature")))
        .unwrap_err();
    assert_eq!(
        repo.explain_failure(&merge("feature", MergeMethod::Merge, None), &err)
            .kind,
        FailureKind::Conflict
    );

    let op = repo.operation().unwrap().expect("a merge in progress");
    assert_eq!(op.kind, OperationKind::Merge);
    assert_eq!(op.branch.as_deref(), Some("main"));
    assert_eq!(op.incoming.as_deref(), Some("feature"));
    assert_eq!(op.incoming_count, 2);
    assert_eq!(op.conflicted, 1);
    assert_eq!((op.ours_label.as_str(), op.theirs_label.as_str()), ("main", "feature"));
    assert_eq!(op.yours, ConflictSide::Ours);
    assert_eq!(op.message.as_deref(), Some("Bring in feature"));

    let file = repo.conflict_file("a.txt").unwrap();
    assert!(file.base && file.ours && file.theirs && !file.binary);
    assert_eq!(
        file.chunks,
        [
            Chunk::Same {
                lines: vec!["one".into()]
            },
            Chunk::Conflict {
                ours: vec!["2 on main".into()],
                base: vec!["two".into()],
                theirs: vec!["TWO on feature".into()],
            },
            Chunk::Same {
                lines: vec!["three".into()]
            },
        ]
    );

    // A pick count that doesn't match the file is refused, and nothing is written.
    let wrong = Action::Resolve {
        path: "a.txt".into(),
        picks: vec![],
    };
    assert!(repo.perform(&wrong).is_err());

    let resolve = Action::Resolve {
        path: "a.txt".into(),
        picks: vec![Pick::TheirsThenOurs],
    };
    assert_eq!(repo.plan(&resolve).unwrap().commands[0].display(), "git add -- a.txt");
    repo.perform(&resolve).unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\nTWO on feature\n2 on main\nthree\n");
    assert_eq!(repo.operation().unwrap().unwrap().conflicted, 0);

    let finish = Action::Continue { message: None };
    assert_eq!(
        repo.plan(&finish).unwrap().commands[0].display(),
        "git commit -m 'Bring in feature'"
    );
    repo.perform(&finish).unwrap();
    assert!(repo.operation().unwrap().is_none());
    // Without git's "# Conflicts:" comment lines.
    assert_eq!(fx.git(&["log", "-1", "--format=%B"]), "Bring in feature");
    assert_eq!(fx.git(&["log", "-1", "--format=%P"]).split(' ').count(), 2);
}

#[test]
fn abort_puts_everything_back() {
    let mut fx = diverged();
    let before = fx.git(&["rev-parse", "HEAD"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();
    assert_eq!(
        repo.plan(&Action::Abort).unwrap().commands[0].display(),
        "git merge --abort"
    );
    repo.perform(&Action::Abort).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), before);
    assert_eq!(fx.git(&["status", "--porcelain"]), "");
}

#[test]
fn a_rebase_that_conflicts_names_the_sides_and_continues() {
    let mut fx = diverged();
    fx.git(&["switch", "-q", "feature"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("main", MergeMethod::Rebase, None)).unwrap_err();

    let op = repo.operation().unwrap().expect("a rebase in progress");
    assert_eq!(op.kind, OperationKind::Rebase);
    assert_eq!(op.branch.as_deref(), Some("feature"));
    assert_eq!(op.incoming.as_deref(), Some("main"));
    assert_eq!(op.step, Some((1, 2)));
    assert_eq!(op.commit.as_ref().unwrap().summary, "Shout two");
    // In a rebase, "ours" is the new base and the user's commit is "theirs".
    assert_eq!((op.ours_label.as_str(), op.theirs_label.as_str()), ("main", "feature"));
    assert_eq!(op.yours, ConflictSide::Theirs);

    // The rebase is not a detached HEAD with lost commits.
    let history = repo.history(&Default::default()).unwrap();
    assert!(history.operation.is_some());
    assert!(history.rows.iter().all(|r| !r.no_branch));
    assert_eq!(history.rows[0].summary, "Rebase in progress");

    repo.perform(&Action::TakeFile {
        path: "a.txt".into(),
        side: ConflictSide::Theirs,
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\nTWO on feature\nthree\n");
    assert_eq!(
        repo.plan(&Action::Continue { message: None }).unwrap().commands[0].display(),
        "git rebase --continue"
    );
    // No editor opens: the commit keeps its message.
    repo.perform(&Action::Continue { message: None }).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(fx.git(&["branch", "--show-current"]), "feature");
    assert_eq!(fx.git(&["log", "--format=%s", "-3"]), "Add b\nShout two\nNumber two");
}

#[test]
fn a_squash_that_conflicts_keeps_the_chosen_message() {
    let mut fx = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    let action = merge("feature", MergeMethod::Squash, Some("Feature in one commit"));
    let plan = repo.plan(&action).unwrap();
    assert_eq!(plan.commands[0].display(), "git merge --squash feature");
    assert_eq!(plan.commands[1].display(), "git commit -m 'Feature in one commit'");
    repo.perform(&action).unwrap_err();

    let op = repo.operation().unwrap().expect("a squash in progress");
    assert_eq!(op.kind, OperationKind::Squash);
    assert_eq!(op.message.as_deref(), Some("Feature in one commit"));
    // SQUASH_MSG does not say which branch it was, so Oxbow remembers.
    assert_eq!(op.theirs_label, "feature");
    repo.perform(&Action::TakeFile {
        path: "a.txt".into(),
        side: ConflictSide::Ours,
    })
    .unwrap();
    let message = op.message.clone();
    repo.perform(&Action::Continue { message }).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(fx.git(&["log", "-1", "--format=%s %P"]).split(' ').count(), 5);
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Feature in one commit");
    assert_eq!(read(&fx, "b.txt"), "new\n");
}

#[test]
fn a_file_deleted_on_one_side_is_kept_or_deleted_whole() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.git(&["rm", "-q", "a.txt"]);
    fx.git(&["commit", "-q", "-m", "Remove a"]);
    fx.git(&["switch", "-q", "main"]);
    fx.commit("a.txt", "one more\n", "Change a");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();

    let file = repo.conflict_file("a.txt").unwrap();
    assert!(file.ours && !file.theirs && file.chunks.is_empty());
    let take = Action::TakeFile {
        path: "a.txt".into(),
        side: ConflictSide::Theirs,
    };
    assert_eq!(
        repo.plan(&take).unwrap().commands[0].display(),
        "git rm --quiet -- a.txt"
    );
    repo.perform(&take).unwrap();
    assert!(!fx.path().join("a.txt").exists());
    assert_eq!(repo.operation().unwrap().unwrap().conflicted, 0);
}

#[test]
fn a_squash_message_from_git_lists_the_commits() {
    let mut fx = diverged();
    fx.git(&["switch", "-q", "-c", "clean", "feature"]);
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["reset", "-q", "--hard", "HEAD~1"]);
    // Started from the command line: git writes its own SQUASH_MSG.
    fx.git(&["merge", "-q", "--squash", "clean"]);
    let repo = Repo::open(fx.path()).unwrap();
    let op = repo.operation().unwrap().expect("a squash waiting for its commit");
    assert_eq!(op.message.as_deref(), Some("Squash 2 commits\n\n- Shout two\n- Add b"));
}

#[cfg(unix)]
#[test]
fn a_conflicted_symlink_is_taken_whole_and_its_target_left_alone() {
    use std::os::unix::fs::symlink;
    let mut fx = Fixture::new();
    for name in ["base.txt", "ours.txt", "theirs.txt"] {
        fx.write(name, &format!("{name} content\n"));
    }
    symlink("base.txt", fx.path().join("link")).unwrap();
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "Root"]);
    fx.git(&["switch", "-q", "-c", "feature"]);
    std::fs::remove_file(fx.path().join("link")).unwrap();
    symlink("theirs.txt", fx.path().join("link")).unwrap();
    fx.git(&["commit", "-q", "-am", "Point at theirs"]);
    fx.git(&["switch", "-q", "main"]);
    std::fs::remove_file(fx.path().join("link")).unwrap();
    symlink("ours.txt", fx.path().join("link")).unwrap();
    fx.git(&["commit", "-q", "-am", "Point at ours"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();

    let file = repo.conflict_file("link").unwrap();
    assert!(file.link && file.chunks.is_empty());
    // Line by line would write through the link into ours.txt: refused.
    let err = repo
        .perform(&Action::Resolve {
            path: "link".into(),
            picks: vec![Pick::Theirs],
        })
        .unwrap_err();
    assert!(err.to_string().contains("whole"), "{err}");
    assert_eq!(read(&fx, "ours.txt"), "ours.txt content\n");

    repo.perform(&Action::TakeFile {
        path: "link".into(),
        side: ConflictSide::Theirs,
    })
    .unwrap();
    let target = std::fs::read_link(fx.path().join("link")).unwrap();
    assert_eq!(target.to_str(), Some("theirs.txt"));
    assert_eq!(read(&fx, "ours.txt"), "ours.txt content\n");
}
