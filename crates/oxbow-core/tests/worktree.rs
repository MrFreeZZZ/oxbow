mod support;

use oxbow_core::{Action, FileStatus, LineKind, Repo, Side};
use support::Fixture;

fn lines(n: usize, changed: &[usize]) -> String {
    (1..=n)
        .map(|i| {
            if changed.contains(&i) {
                format!("line {i} changed\n")
            } else {
                format!("line {i}\n")
            }
        })
        .collect()
}

#[test]
fn working_tree_groups_staged_unstaged_and_untracked() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", &lines(5, &[]), "Root");
    fx.commit("b.txt", "b\n", "Second");
    fx.write("a.txt", &lines(5, &[2]));
    fx.git(&["add", "a.txt"]);
    fx.write("a.txt", &lines(5, &[2, 4]));
    fx.write("b.txt", "b\nmore\n");
    fx.write("new dir/notes.md", "one\ntwo\n");

    let tree = Repo::open(fx.path()).unwrap().working_tree().unwrap();
    let staged: Vec<_> = tree
        .staged
        .iter()
        .map(|f| (f.path.as_str(), f.additions, f.deletions))
        .collect();
    assert_eq!(staged, [("a.txt", 1, 1)]);
    let unstaged: Vec<_> = tree
        .unstaged
        .iter()
        .map(|f| (f.path.as_str(), f.status, f.additions))
        .collect();
    assert_eq!(
        unstaged,
        [
            ("a.txt", FileStatus::Modified, 1),
            ("b.txt", FileStatus::Modified, 1),
            ("new dir/notes.md", FileStatus::Untracked, 2)
        ]
    );
    assert_eq!(tree.file_count(), 3);
}

#[test]
fn hunks_can_be_staged_unstaged_and_discarded_one_by_one() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", &lines(30, &[]), "Root");
    fx.write("a.txt", &lines(30, &[3, 25]));
    let repo = Repo::open(fx.path()).unwrap();

    let diff = repo.working_diff("a.txt", Side::Unstaged, false).unwrap();
    assert_eq!(diff.hunks.len(), 2);
    // The front end sends back the header it was given.
    let header = |h: &oxbow_core::Hunk| h.header.clone();
    let second = header(&diff.hunks[1]);

    // Stage only the second change.
    repo.perform(&Action::StageHunk {
        path: "a.txt".into(),
        header: second.clone(),
        check: diff.hunks[1].check.clone(),
        lines: None,
    })
    .unwrap();
    let staged = repo.working_diff("a.txt", Side::Staged, false).unwrap();
    assert_eq!(staged.hunks.len(), 1);
    assert!(
        staged.hunks[0]
            .lines
            .iter()
            .any(|l| l.kind == LineKind::Added && l.text == "line 25 changed")
    );
    let unstaged = repo.working_diff("a.txt", Side::Unstaged, false).unwrap();
    assert_eq!(unstaged.hunks.len(), 1);

    // Discard the first change from the working copy.
    let first = header(&unstaged.hunks[0]);
    repo.perform(&Action::DiscardHunk {
        path: "a.txt".into(),
        header: first,
        check: unstaged.hunks[0].check.clone(),
        lines: None,
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        lines(30, &[25])
    );

    // A stale header is refused instead of patching the wrong place.
    let err = repo
        .perform(&Action::UnstageHunk {
            path: "a.txt".into(),
            header: "@@ -1,2 +1,2 @@".into(),
            check: String::new(),
            lines: None,
        })
        .unwrap_err();
    assert!(err.to_string().contains("changed since"), "{err}");

    // Unstage it again: the working copy keeps the change.
    let staged = repo.working_diff("a.txt", Side::Staged, false).unwrap();
    repo.perform(&Action::UnstageHunk {
        path: "a.txt".into(),
        header: header(&staged.hunks[0]),
        check: staged.hunks[0].check.clone(),
        lines: None,
    })
    .unwrap();
    let tree = repo.working_tree().unwrap();
    assert!(tree.staged.is_empty());
    assert_eq!(tree.unstaged.len(), 1);
}

#[test]
fn files_are_staged_discarded_and_committed() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "a\nb\n");
    fx.write("junk.log", "x\n");
    fx.write("new.txt", "new\n");
    let repo = Repo::open(fx.path()).unwrap();

    let plan = repo
        .plan(&Action::Discard {
            paths: vec!["a.txt".into(), "junk.log".into()],
        })
        .unwrap();
    let shown: Vec<_> = plan.commands.iter().map(|c| c.display()).collect();
    assert_eq!(shown, ["git restore --worktree -- a.txt", "git clean -f -- junk.log"]);
    repo.perform(&Action::Discard {
        paths: vec!["a.txt".into(), "junk.log".into()],
    })
    .unwrap();
    assert!(!fx.path().join("junk.log").exists());
    assert_eq!(std::fs::read_to_string(fx.path().join("a.txt")).unwrap(), "a\n");

    repo.perform(&Action::Stage {
        paths: vec!["new.txt".into()],
    })
    .unwrap();
    let commit = Action::Commit {
        message: "Add new.txt\n\nWith a body.".into(),
        amend: false,
        no_verify: false,
    };
    assert_eq!(
        repo.plan(&commit).unwrap().commands[0].display(),
        "git commit -m 'Add new.txt' -m 'With a body.'"
    );
    repo.perform(&commit).unwrap();
    assert!(repo.working_tree().unwrap().is_empty());
    assert_eq!(
        fx.git(&["log", "-1", "--format=%B"]).trim_end(),
        "Add new.txt\n\nWith a body."
    );

    // Unstage works before the first commit too.
    let mut empty = Fixture::new();
    empty.write("x.txt", "x\n");
    empty.git(&["add", "x.txt"]);
    let repo = Repo::open(empty.path()).unwrap();
    assert_eq!(repo.working_tree().unwrap().staged[0].additions, 1);
    repo.perform(&Action::Unstage {
        paths: vec!["x.txt".into()],
    })
    .unwrap();
    let tree = repo.working_tree().unwrap();
    assert!(tree.staged.is_empty());
    assert_eq!(tree.unstaged[0].status, FileStatus::Untracked);
}

#[test]
fn a_file_name_with_pattern_characters_means_just_that_file() {
    let mut fx = Fixture::new();
    fx.commit("a*.txt", "star\n", "Root");
    fx.commit("another.txt", "other\n", "Second");
    fx.write("a*.txt", "star changed\n");
    fx.write("another.txt", "other changed\n");
    let repo = Repo::open(fx.path()).unwrap();

    let only = vec!["a*.txt".to_owned()];
    let stage = Action::Stage { paths: only.clone() };
    assert_eq!(
        repo.plan(&stage).unwrap().commands[0].display(),
        "git add -- ':(literal)a*.txt'"
    );
    repo.perform(&stage).unwrap();
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "a*.txt");
    repo.perform(&Action::Unstage { paths: only.clone() }).unwrap();
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "");

    repo.perform(&Action::Discard { paths: only }).unwrap();
    assert_eq!(std::fs::read_to_string(fx.path().join("a*.txt")).unwrap(), "star\n");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("another.txt")).unwrap(),
        "other changed\n"
    );
}

#[test]
fn unstaging_a_rename_unstages_both_paths() {
    let mut fx = Fixture::new();
    fx.commit("old.txt", "keep me\n", "Root");
    fx.git(&["mv", "old.txt", "new.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    let renamed = &repo.working_tree().unwrap().staged[0];
    assert_eq!(
        (renamed.path.as_str(), renamed.old_path.as_deref()),
        ("new.txt", Some("old.txt"))
    );

    repo.perform(&Action::Unstage {
        paths: vec!["new.txt".into()],
    })
    .unwrap();
    // Nothing is left for the next commit, and the file on disk keeps its new name.
    assert_eq!(fx.git(&["diff", "--cached", "--name-status"]), "");
    assert!(fx.path().join("new.txt").exists());
}

#[test]
fn a_hunk_edited_after_it_was_shown_is_refused() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\ntwo\nthree\n", "Root");
    fx.write("a.txt", "one\nfirst edit\nthree\n");
    let repo = Repo::open(fx.path()).unwrap();
    let seen = repo.working_diff("a.txt", Side::Unstaged, false).unwrap().hunks[0].clone();

    // Another editor saves a new line in the same place: same `@@` header, other text.
    fx.write("a.txt", "one\nnew important edit\nthree\n");
    let fresh = repo.working_diff("a.txt", Side::Unstaged, false).unwrap().hunks[0].clone();
    assert_eq!(fresh.header, seen.header);

    let err = repo
        .perform(&Action::DiscardHunk {
            path: "a.txt".into(),
            header: seen.header.clone(),
            check: seen.check.clone(),
            lines: None,
        })
        .unwrap_err();
    assert!(err.to_string().contains("changed since"), "{err}");
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "one\nnew important edit\nthree\n"
    );
}

#[test]
fn a_hunk_whose_line_endings_changed_after_it_was_shown_is_refused() {
    // The text on screen stays the same; only the bytes git would apply differ.
    for (shown, saved) in [("new\n", "new"), ("new\n", "new\r\n")] {
        let mut fx = Fixture::new();
        fx.commit("a.txt", "old\n", "Root");
        fx.write("a.txt", shown);
        let repo = Repo::open(fx.path()).unwrap();
        let seen = repo.working_diff("a.txt", Side::Unstaged, false).unwrap().hunks[0].clone();
        fx.write("a.txt", saved);
        let fresh = repo.working_diff("a.txt", Side::Unstaged, false).unwrap().hunks[0].clone();
        assert_eq!(
            (fresh.header.as_str(), fresh.lines.len()),
            (seen.header.as_str(), seen.lines.len())
        );

        for action in [
            Action::DiscardHunk {
                path: "a.txt".into(),
                header: seen.header.clone(),
                check: seen.check.clone(),
                lines: None,
            },
            Action::StageHunk {
                path: "a.txt".into(),
                header: seen.header.clone(),
                check: seen.check.clone(),
                lines: None,
            },
        ] {
            let err = repo.perform(&action).unwrap_err();
            assert!(err.to_string().contains("changed since"), "{saved:?}: {err}");
        }
        assert_eq!(std::fs::read(fx.path().join("a.txt")).unwrap(), saved.as_bytes());
        assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "");
    }
}
