mod support;

use oxbow_core::{
    Action, Chunk, ConflictSide, FailureKind, MergeMethod, OperationKind, Pick, Renamed, Repo, Resolution,
};
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
        text: None,
        stages: None,
    };
    assert!(repo.perform(&wrong).is_err());

    let resolve = Action::Resolve {
        path: "a.txt".into(),
        picks: vec![Pick::TheirsThenOurs.into()],
        text: None,
        stages: None,
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
fn resolving_keeps_bytes_that_are_not_utf8() {
    // `café` in Latin-1 on a line both sides keep; the next line conflicts.
    let mut fx = Fixture::new();
    let write = |fx: &Fixture, bytes: &[u8]| std::fs::write(fx.path().join("a.txt"), bytes).unwrap();
    write(&fx, b"caf\xe9\nbase\n");
    fx.git(&["add", "a.txt"]);
    fx.git(&["commit", "-q", "-m", "Root"]);
    fx.git(&["switch", "-q", "-c", "feature"]);
    write(&fx, b"caf\xe9\ntheirs \xe9\n");
    fx.git(&["commit", "-q", "-am", "Theirs"]);
    fx.git(&["switch", "-q", "main"]);
    write(&fx, b"caf\xe9\nours\n");
    fx.git(&["commit", "-q", "-am", "Ours"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();

    let file = repo.conflict_file("a.txt").unwrap();
    assert_eq!(file.chunks.len(), 2);
    repo.perform(&Action::Resolve {
        path: "a.txt".into(),
        picks: vec![Pick::OursThenTheirs.into()],
        text: None,
        stages: None,
    })
    .unwrap();
    let bytes = std::fs::read(fx.path().join("a.txt")).unwrap();
    assert_eq!(bytes, b"caf\xe9\nours\ntheirs \xe9\n");
    // The index holds the same bytes.
    assert_eq!(fx.git(&["rev-parse", ":a.txt"]), fx.git(&["hash-object", "a.txt"]));
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
            picks: vec![Pick::Theirs.into()],
            text: None,
            stages: None,
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

#[test]
fn a_conflict_takes_lines_written_by_hand() {
    let fx = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();
    let file = repo.conflict_file("a.txt").unwrap();
    assert!(file.text && file.renamed.is_none());

    repo.perform(&Action::Resolve {
        path: "a.txt".into(),
        picks: vec![Resolution::Lines {
            lines: vec!["2, as both meant".into(), "".into()],
        }],
        text: None,
        stages: None,
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\n2, as both meant\n\nthree\n");
    assert_eq!(repo.operation().unwrap().unwrap().conflicted, 0);
}

/// Both sides change the last line of a CRLF file that ends without a line break.
fn crlf_conflict() -> Fixture {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\r\ntwo\r\nthree", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.commit("a.txt", "one\r\ntwo\r\nTHREE", "Shout three");
    fx.git(&["switch", "-q", "main"]);
    fx.commit("a.txt", "one\r\ntwo\r\n3", "Number three");
    fx
}

#[test]
fn typed_lines_keep_the_line_endings_of_the_file() {
    let fx = crlf_conflict();
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();
    repo.perform(&Action::Resolve {
        path: "a.txt".into(),
        picks: vec![Resolution::Lines {
            lines: vec!["Three".into(), "and four".into()],
        }],
        text: None,
        stages: None,
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\r\ntwo\r\nThree\r\nand four");

    let fx = crlf_conflict();
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();
    // Picks and text at once are refused.
    assert!(
        repo.perform(&Action::Resolve {
            path: "a.txt".into(),
            picks: vec![Pick::Ours.into()],
            text: Some("x".into()),
            stages: None,
        })
        .is_err()
    );
    repo.perform(&Action::Resolve {
        path: "a.txt".into(),
        picks: vec![],
        text: Some("one\nTwo\nThree".into()),
        stages: None,
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\r\nTwo\r\nThree");
    assert_eq!(repo.operation().unwrap().unwrap().conflicted, 0);
}

#[test]
fn typed_text_is_refused_for_a_file_that_is_not_utf8() {
    let mut fx = Fixture::new();
    let write = |fx: &Fixture, bytes: &[u8]| std::fs::write(fx.path().join("a.txt"), bytes).unwrap();
    write(&fx, b"caf\xe9\nbase\n");
    fx.git(&["add", "a.txt"]);
    fx.git(&["commit", "-q", "-m", "Root"]);
    fx.git(&["switch", "-q", "-c", "feature"]);
    write(&fx, b"caf\xe9\ntheirs\n");
    fx.git(&["commit", "-q", "-am", "Theirs"]);
    fx.git(&["switch", "-q", "main"]);
    write(&fx, b"caf\xe9\nours\n");
    fx.git(&["commit", "-q", "-am", "Ours"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();

    assert!(!repo.conflict_file("a.txt").unwrap().text);
    let err = repo
        .perform(&Action::Resolve {
            path: "a.txt".into(),
            picks: vec![Resolution::Lines {
                lines: vec!["x".into()],
            }],
            text: None,
            stages: None,
        })
        .unwrap_err();
    assert!(err.to_string().contains("UTF-8"), "{err}");
    // Nothing was written: the file still has git's markers.
    assert!(
        std::fs::read(fx.path().join("a.txt"))
            .unwrap()
            .starts_with(b"caf\xe9\n<<<<<<<")
    );
}

#[test]
fn a_file_renamed_on_one_side_says_where_it_came_from() {
    let mut fx = Fixture::new();
    fx.commit("old.txt", "one\ntwo\nthree\nfour\nfive\nsix\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.git(&["mv", "old.txt", "new.txt"]);
    fx.commit("new.txt", "one\nTWO\nthree\nfour\nfive\nsix\n", "Rename and shout");
    fx.git(&["switch", "-q", "main"]);
    fx.commit("old.txt", "one\n2\nthree\nfour\nfive\nsix\n", "Number two");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();

    let file = repo.conflict_file("new.txt").unwrap();
    assert_eq!(
        file.renamed,
        Some(Renamed {
            from: "old.txt".into(),
            side: ConflictSide::Theirs
        })
    );
    assert_eq!(file.chunks.len(), 3);

    // The other way round: main renamed it, feature changed the old name.
    let mut fx = Fixture::new();
    fx.commit("old.txt", "one\ntwo\nthree\nfour\nfive\nsix\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.commit("old.txt", "one\nTWO\nthree\nfour\nfive\nsix\n", "Shout");
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["mv", "old.txt", "new.txt"]);
    fx.commit("new.txt", "one\n2\nthree\nfour\nfive\nsix\n", "Rename and number");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();
    let renamed = repo.conflict_file("new.txt").unwrap().renamed.unwrap();
    assert_eq!((renamed.from.as_str(), renamed.side), ("old.txt", ConflictSide::Ours));
}

#[test]
fn the_merge_tool_comes_from_the_git_config() {
    let mut fx = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();

    fx.git(&["config", "merge.tool", "vimdiff"]);
    let tool = repo.merge_tool().unwrap();
    assert!(tool.terminal && !tool.gui);
    assert!(
        repo.open_merge_tool("a.txt")
            .unwrap_err()
            .to_string()
            .contains("terminal")
    );

    fx.git(&["config", "merge.guitool", "vscode"]);
    let tool = repo.merge_tool().unwrap();
    assert_eq!((tool.label.as_str(), tool.terminal, tool.gui), ("VS Code", false, true));
    assert_eq!(
        repo.merge_tool_command("a.txt").unwrap().display(),
        "git -c mergetool.keepBackup=false mergetool --no-prompt --gui -- a.txt"
    );

    // A custom tool that "merges" by taking feature's side: git stages the file once it quits.
    fx.git(&["config", "merge.guitool", "take"]);
    fx.git(&["config", "mergetool.take.cmd", "cp \"$REMOTE\" \"$MERGED\""]);
    fx.git(&["config", "mergetool.take.trustExitCode", "true"]);
    assert!(repo.open_merge_tool("a.txt").unwrap());
    assert_eq!(read(&fx, "a.txt"), "one\nTWO on feature\nthree\n");
    assert_eq!(repo.operation().unwrap().unwrap().conflicted, 0);
    assert!(!fx.path().join("a.txt.orig").exists());
}

#[test]
fn typed_text_is_written_exactly_with_its_last_line_breaks() {
    for (typed, written) in [
        ("one\nMERGED\nthree\n", "one\nMERGED\nthree\n"),
        ("one\nMERGED\nthree", "one\nMERGED\nthree"),
        ("one\nMERGED\nthree\n\n", "one\nMERGED\nthree\n\n"),
        ("one\r\nMERGED\r\nthree\r\n", "one\nMERGED\nthree\n"),
        ("", ""),
    ] {
        let fx = diverged();
        let repo = Repo::open(fx.path()).unwrap();
        repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();
        let file = repo.conflict_file("a.txt").unwrap();
        assert!(!file.open_end);
        repo.perform(&Action::Resolve {
            path: "a.txt".into(),
            picks: vec![],
            text: Some(typed.into()),
            stages: Some(file.stages),
        })
        .unwrap();
        assert_eq!(read(&fx, "a.txt"), written, "typed {typed:?}");
    }

    // A file without a line break at its end says so, and CRLF text keeps CRLF.
    let fx = crlf_conflict();
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();
    assert!(repo.conflict_file("a.txt").unwrap().open_end);
    repo.perform(&Action::Resolve {
        path: "a.txt".into(),
        picks: vec![],
        text: Some("one\ntwo\nTHREE\n".into()),
        stages: None,
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\r\ntwo\r\nTHREE\r\n");
}

#[test]
fn choices_made_for_other_versions_are_refused() {
    let mut fx = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();
    let before = repo.conflict_file("a.txt").unwrap().stages;
    assert_eq!(before.split(' ').count(), 3);

    // Abort, change main's side, and merge the same branch again: other versions.
    repo.perform(&Action::Abort).unwrap();
    fx.commit("a.txt", "one\nNEW main value\nthree\n", "New value");
    repo.perform(&merge("feature", MergeMethod::Merge, None)).unwrap_err();
    let after = repo.conflict_file("a.txt").unwrap().stages;
    assert_ne!(before, after);

    let stale = Action::Resolve {
        path: "a.txt".into(),
        picks: vec![],
        text: Some("old result\n".into()),
        stages: Some(before),
    };
    let err = repo.perform(&stale).unwrap_err();
    assert!(err.to_string().contains("changed since it was shown"), "{err}");
    assert_eq!(repo.operation().unwrap().unwrap().conflicted, 1);

    repo.perform(&Action::Resolve {
        path: "a.txt".into(),
        picks: vec![Pick::Ours.into()],
        text: None,
        stages: Some(after),
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\nNEW main value\nthree\n");
}
