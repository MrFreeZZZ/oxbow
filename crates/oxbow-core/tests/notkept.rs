mod support;

use oxbow_core::{Action, NotKept, Repo, ResetMode};
use support::Fixture;

/// A sparse file just over the snapshot limit: quick to make, never read.
fn big(fx: &Fixture, file: &str) {
    let path = fx.path().join(file);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::File::create(path).unwrap().set_len(51 * 1024 * 1024).unwrap();
}

fn sibling(fx: &Fixture, name: &str) -> String {
    fx.path().parent().unwrap().join(name).display().to_string()
}

fn undo_last(repo: &Repo) -> Action {
    let log = repo.operation_log().unwrap();
    Action::Restore {
        id: log[0].id.clone(),
        after: false,
    }
}

#[test]
fn discarding_a_file_too_big_for_a_snapshot_names_it() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("small.txt", "small\n");
    big(&fx, "video.mov");
    let repo = Repo::open(fx.path()).unwrap();

    let small = repo
        .not_kept(&Action::Discard {
            paths: vec!["small.txt".into()],
        })
        .unwrap();
    assert!(small.is_empty(), "{small:?}");

    let both = repo
        .not_kept(&Action::Discard {
            paths: vec!["small.txt".into(), "video.mov".into()],
        })
        .unwrap();
    match both.as_slice() {
        [NotKept::BigFiles { files, more: 0, .. }] => {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, "video.mov");
            assert_eq!(files[0].size, 51 * 1024 * 1024);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_hard_reset_names_big_untracked_files_it_writes_over() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    fx.commit("data.bin", "old data\n", "Add data");
    fx.git(&["rm", "-q", "data.bin"]);
    fx.git(&["commit", "-q", "-m", "Remove data"]);
    big(&fx, "data.bin");
    big(&fx, "other.bin");
    let repo = Repo::open(fx.path()).unwrap();

    // The root doesn't have data.bin: the reset leaves both files alone.
    let reset = |commit: String| Action::Reset {
        commit,
        mode: ResetMode::Hard,
    };
    assert!(repo.not_kept(&reset(root)).unwrap().is_empty());
    let back = fx.git(&["rev-parse", "HEAD~1"]);
    match repo.not_kept(&reset(back)).unwrap().as_slice() {
        [NotKept::BigFiles { files, .. }] => assert_eq!(files[0].path, "data.bin"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_hard_reset_names_ignored_files_it_writes_over() {
    // The older commit tracks .env; the newer one stops tracking and ignores it.
    let mut fx = Fixture::new();
    fx.commit(".env", "TOKEN=example\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    fx.git(&["rm", "-q", "--cached", ".env"]);
    fx.commit(".gitignore", ".env\n", "Ignore .env");
    fx.write(".env", "TOKEN=my own\n");
    let repo = Repo::open(fx.path()).unwrap();

    let reset = Action::Reset {
        commit: root,
        mode: ResetMode::Hard,
    };
    match repo.not_kept(&reset).unwrap().as_slice() {
        [NotKept::IgnoredInTheWay { files, more: 0, bytes }] => {
            assert_eq!(files[0].path, ".env");
            assert_eq!(*bytes, 13);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_ignored_file_just_as_the_commit_has_it_is_not_named() {
    let mut fx = Fixture::new();
    fx.commit(".env", "TOKEN=example\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    fx.git(&["rm", "-q", "--cached", ".env"]);
    fx.commit(".gitignore", ".env\n", "Ignore .env");
    let repo = Repo::open(fx.path()).unwrap();

    let reset = Action::Reset {
        commit: root,
        mode: ResetMode::Hard,
    };
    assert!(repo.not_kept(&reset).unwrap().is_empty());
}

#[test]
fn a_hard_reset_names_files_in_a_folder_where_the_commit_has_a_file() {
    // The older commit has a file `dir`; now `dir` is a folder of untracked files.
    let mut fx = Fixture::new();
    fx.commit("dir", "a file\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    fx.git(&["rm", "-q", "dir"]);
    fx.commit(".gitignore", "*.log\n", "Remove dir");
    big(&fx, "dir/large.bin");
    fx.write("dir/notes.txt", "small, in the snapshot\n");
    fx.write("dir/run.log", "ignored\n");
    let repo = Repo::open(fx.path()).unwrap();

    let reset = Action::Reset {
        commit: root,
        mode: ResetMode::Hard,
    };
    match repo.not_kept(&reset).unwrap().as_slice() {
        [
            NotKept::BigFiles { files: big, .. },
            NotKept::IgnoredInTheWay { files: ignored, .. },
        ] => {
            assert_eq!(big.len(), 1);
            assert_eq!(big[0].path, "dir/large.bin");
            assert_eq!(ignored.len(), 1);
            assert_eq!(ignored[0].path, "dir/run.log");
        }
        other => panic!("{other:?}"),
    }
}

/// `dir` full of more untracked files than the sheet looks through, and one too big for a
/// snapshot among them.
fn crowded(fx: &Fixture) {
    std::fs::create_dir_all(fx.path().join("dir")).unwrap();
    for n in 0..20_001 {
        std::fs::write(fx.path().join(format!("dir/{n}.txt")), "").unwrap();
    }
    big(fx, "dir/zz-large.bin");
}

fn names_the_big_file_and_the_unchecked_folder(notes: &[NotKept]) {
    match notes {
        [
            NotKept::BigFiles { files, .. },
            NotKept::Unchecked { folders, more: 0, .. },
        ] => {
            assert_eq!(files.len(), 1);
            assert_eq!(files[0].path, "dir/zz-large.bin");
            assert_eq!(folders, &["dir/".to_owned()]);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_hard_reset_over_a_folder_too_full_to_look_through_still_warns() {
    let mut fx = Fixture::new();
    fx.commit("dir", "a file\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    fx.git(&["rm", "-q", "dir"]);
    fx.commit("a.txt", "a\n", "Remove dir");
    crowded(&fx);
    let repo = Repo::open(fx.path()).unwrap();

    let reset = Action::Reset {
        commit: root,
        mode: ResetMode::Hard,
    };
    names_the_big_file_and_the_unchecked_folder(&repo.not_kept(&reset).unwrap());
}

#[test]
fn an_undo_over_a_folder_too_full_to_look_through_still_warns() {
    let mut fx = Fixture::new();
    fx.commit("dir", "a file\n", "Root");
    fx.git(&["rm", "-q", "dir"]);
    let removed = fx.commit("a.txt", "a\n", "Remove dir");
    fx.git(&["reset", "-q", "--hard", "HEAD~1"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::Reset {
        commit: removed,
        mode: ResetMode::Hard,
    })
    .unwrap();
    crowded(&fx);

    names_the_big_file_and_the_unchecked_folder(&repo.not_kept(&undo_last(&repo)).unwrap());
}

#[test]
fn a_hard_reset_names_an_ignored_file_where_the_commit_has_a_folder() {
    let mut fx = Fixture::new();
    fx.commit("build/out.txt", "out\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    fx.git(&["rm", "-q", "-r", "build"]);
    fx.commit(".gitignore", "build\n", "Remove build");
    fx.write("build", "a file now\n");
    let repo = Repo::open(fx.path()).unwrap();

    let reset = Action::Reset {
        commit: root,
        mode: ResetMode::Hard,
    };
    match repo.not_kept(&reset).unwrap().as_slice() {
        [NotKept::IgnoredInTheWay { files, .. }] => assert_eq!(files[0].path, "build"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn undoing_a_pushed_commit_says_only_this_copy_changes_and_offers_revert() {
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
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    fx.git(&["remote", "add", "origin", url.to_str().unwrap()]);
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    fx.write("a.txt", "two\n");
    fx.git(&["add", "a.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::Commit {
        message: "Second".into(),
        amend: false,
        no_verify: false,
        allow_secrets: false,
        skip_checks: false,
        sign_off: false,
    })
    .unwrap();
    let second = fx.git(&["rev-parse", "HEAD"]);

    // Not pushed yet: the undo is whole.
    assert!(repo.not_kept(&undo_last(&repo)).unwrap().is_empty());

    fx.git(&["push", "-q", "origin", "main"]);
    match repo.not_kept(&undo_last(&repo)).unwrap().as_slice() {
        [
            NotKept::Pushed {
                branch,
                remote,
                commits,
                more: 0,
                revert,
            },
        ] => {
            assert_eq!(branch, "main");
            assert_eq!(remote, "origin/main");
            assert_eq!(commits.len(), 1);
            assert_eq!(commits[0].summary, "Second");
            assert_eq!(revert.as_deref(), Some(second.as_str()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn removing_a_worktree_names_its_ignored_files_and_changes() {
    let mut fx = Fixture::new();
    fx.commit(".gitignore", ".env\nbuild/\n", "Root");
    fx.git(&["branch", "fix"]);
    let path = sibling(&fx, "repo-fix");
    fx.git(&["worktree", "add", "-q", &path, "fix"]);
    std::fs::write(format!("{path}/.env"), "TOKEN=1\n").unwrap();
    std::fs::create_dir_all(format!("{path}/build")).unwrap();
    std::fs::write(format!("{path}/build/app"), vec![0u8; 4096]).unwrap();
    std::fs::write(format!("{path}/notes.txt"), "draft\n").unwrap();
    let repo = Repo::open(fx.path()).unwrap();

    let stashed = repo
        .not_kept(&Action::WorktreeRemove {
            path: path.clone(),
            force: true,
            stash: true,
        })
        .unwrap();
    assert!(matches!(stashed.last(), Some(NotKept::NotLogged { .. })), "{stashed:?}");
    match &stashed[..stashed.len() - 1] {
        [
            NotKept::Ignored {
                files, bytes, more: 0, ..
            },
        ] => {
            let names: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
            assert_eq!(names, ["build/", ".env"]);
            assert_eq!(*bytes, 4096 + 8);
        }
        other => panic!("{other:?}"),
    }

    let forced = repo
        .not_kept(&Action::WorktreeRemove {
            path,
            force: true,
            stash: false,
        })
        .unwrap();
    assert!(
        forced.iter().any(|n| matches!(
            n,
            NotKept::Worktree {
                removed: true,
                changed: Some(1),
                ..
            }
        )),
        "{forced:?}"
    );
}

#[test]
fn an_undo_that_moves_a_branch_checked_out_elsewhere_says_its_files_stay() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::CreateBranch {
        name: "fix".into(),
        start: None,
        switch: false,
        publish: None,
    })
    .unwrap();
    let path = sibling(&fx, "repo-fix");
    fx.git(&["worktree", "add", "-q", &path, "fix"]);
    fx.git(&["-C", &path, "commit", "-q", "--allow-empty", "-m", "Work"]);

    let notes = repo.not_kept(&undo_last(&repo)).unwrap();
    assert!(
        notes.iter().any(|n| matches!(
            n,
            NotKept::Worktree { branch: Some(b), removed: false, .. } if b == "fix"
        )),
        "{notes:?}"
    );
}

#[test]
fn a_step_shows_until_when_it_is_kept_and_compares_with_now() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.git(&["config", "gc.reflogExpireUnreachable", "10.days"]);
    fx.write("a.txt", "b\n");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::Discard {
        paths: vec!["a.txt".into()],
    })
    .unwrap();
    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].expires, log[0].time + 10 * 86_400);

    fx.write("a.txt", "c\n");
    fx.write("new.txt", "new\n");
    let before = repo.compare_snapshot(&log[0].id, false).unwrap();
    let mut files: Vec<&str> = before.comparison.files.iter().map(|f| f.file.path.as_str()).collect();
    files.sort();
    assert_eq!(files, ["a.txt", "new.txt"]);
    assert!(before.refs.is_empty());
    let diff = repo
        .tree_diff(
            &before.comparison.from,
            &before.comparison.target_id,
            Some("a.txt"),
            oxbow_core::DiffContext::Compact,
        )
        .unwrap();
    assert_eq!(diff[0].file.additions, 1);
    assert_eq!(diff[0].file.deletions, 1);

    // Right after the discard, a.txt was as committed; now it is "c".
    let after = repo.compare_snapshot(&log[0].id, true).unwrap();
    assert_eq!(after.comparison.files.len(), 2);
}

#[test]
fn taking_a_version_over_a_big_file_git_does_not_track_names_it() {
    let mut fx = Fixture::new();
    let added = fx.commit("video.mov", "small then\n", "Add the video");
    fx.git(&["rm", "-q", "--cached", "video.mov"]);
    fx.git(&["commit", "-q", "-m", "Stop tracking the video"]);
    big(&fx, "video.mov");
    let repo = Repo::open(fx.path()).unwrap();
    let take = |path: &str| Action::TakeVersion {
        commit: added.clone(),
        path: path.into(),
    };
    match repo.not_kept(&take("video.mov")).unwrap().as_slice() {
        [NotKept::BigFiles { files, .. }] => assert_eq!(files[0].path, "video.mov"),
        other => panic!("{other:?}"),
    }
    std::fs::remove_file(fx.path().join("video.mov")).unwrap();
    assert!(repo.not_kept(&take("video.mov")).unwrap().is_empty());
}
