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
        allow_secrets: false,
        skip_checks: false,
        sign_off: false,
        changelist: None,
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

#[test]
fn new_files_are_counted_once_until_they_change() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("new.txt", "a\nb\nc\n");
    let old = |fx: &mut Fixture, when: &str| {
        let status = std::process::Command::new("touch")
            .args(["-d", when, "new.txt"])
            .current_dir(fx.path())
            .status()
            .unwrap();
        assert!(status.success());
    };
    old(&mut fx, "2020-01-01 00:00:00");
    let repo = Repo::open(fx.path()).unwrap();
    let count = |repo: &Repo| {
        let tree = repo.working_tree().unwrap();
        let file = tree.unstaged.iter().find(|f| f.path == "new.txt").unwrap().clone();
        (file.additions, file.uncounted)
    };
    assert_eq!(count(&repo), (3, false));
    // Same size and time of change: the count is not read again.
    fx.write("new.txt", "abcde\n");
    old(&mut fx, "2020-01-01 00:00:00");
    assert_eq!(count(&repo), (3, false));
    old(&mut fx, "2021-01-01 00:00:00");
    assert_eq!(count(&repo), (1, false));

    // Past the bytes one reading counts, the rest wait for the next ones.
    let big = "x\n".repeat(4 * 1024 * 1024);
    for i in 0..5 {
        fx.write(&format!("big{i}.txt"), &big);
    }
    let tree = repo.working_tree().unwrap();
    let waiting = tree.unstaged.iter().filter(|f| f.uncounted).count();
    assert!(waiting >= 1);
    assert!(
        tree.unstaged
            .iter()
            .filter(|f| f.path.starts_with("big"))
            .all(|f| f.uncounted || f.additions == 4 * 1024 * 1024)
    );

    // Each refresh of the app opens the repository afresh: the counts stay, so the files left
    // waiting get theirs in the next readings instead of the first ones being read again.
    for i in 0..5 {
        let status = std::process::Command::new("touch")
            .args(["-d", "2020-01-01 00:00:00", &format!("big{i}.txt")])
            .current_dir(fx.path())
            .status()
            .unwrap();
        assert!(status.success());
    }
    let mut repo = repo;
    let mut waiting = usize::MAX;
    for _ in 0..4 {
        repo = repo.reopen().unwrap();
        let now = repo
            .working_tree()
            .unwrap()
            .unstaged
            .iter()
            .filter(|f| f.uncounted)
            .count();
        assert!(now < waiting || now == 0, "{now} still waiting after {waiting}");
        waiting = now;
    }
    assert_eq!(waiting, 0);
    // A count kept from before the reopen: same size and time of change.
    fx.write("new.txt", "a\nb\nc\n");
    old(&mut fx, "2021-01-01 00:00:00");
    assert_eq!(count(&repo.reopen().unwrap()), (1, false));
}

#[test]
fn a_commit_with_sign_off_ends_with_the_committers_line() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "a changed\n");
    fx.git(&["add", "a.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    let action = Action::Commit {
        message: "Change a".into(),
        amend: false,
        no_verify: false,
        allow_secrets: false,
        skip_checks: false,
        sign_off: true,
        changelist: None,
    };
    let shown = repo.plan(&action).unwrap().commands[0].display();
    assert!(
        shown.starts_with("git commit") && shown.contains(" --signoff -m 'Change a'"),
        "{shown}"
    );
    repo.perform(&action).unwrap();
    assert_eq!(
        fx.git(&["log", "-1", "--format=%B"]),
        "Change a\n\nSigned-off-by: Alexander <alexander@example.com>"
    );
}

#[test]
fn recent_messages_are_yours_newest_first_and_each_once() {
    let mut fx = Fixture::new();
    assert!(Repo::open(fx.path()).unwrap().recent_messages(10).unwrap().is_empty());
    fx.commit("a.txt", "1\n", "Fix the build");
    fx.write("b.txt", "b\n");
    fx.git(&["add", "b.txt"]);
    fx.git(&[
        "commit",
        "-q",
        "-m",
        "Someone else's",
        "--author=Other <other@example.com>",
    ]);
    fx.commit("a.txt", "2\n", "Add a test\n\nWith a body.");
    fx.commit("a.txt", "3\n", "Fix the build");
    fx.git(&["switch", "-q", "-c", "side", "HEAD~1"]);
    fx.commit("c.txt", "c\n", "On the side");
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["merge", "-q", "--no-ff", "-m", "Merge side", "side"]);
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(
        repo.recent_messages(10).unwrap(),
        ["On the side", "Fix the build", "Add a test\n\nWith a body."]
    );
    assert_eq!(repo.recent_messages(1).unwrap(), ["On the side"]);
}

#[test]
fn files_listed_before_their_lines_are_counted_show_the_same_diffs() {
    use std::sync::atomic::AtomicBool;
    let mut fx = Fixture::new();
    fx.commit("a.txt", &lines(5, &[]), "Root");
    fx.commit("long.txt", &lines(60, &[]), "Long");
    fx.commit("blob.bin", "\0old", "Binary");
    fx.write("a.txt", &lines(5, &[2]));
    fx.git(&["add", "a.txt"]);
    fx.write("a.txt", &lines(5, &[2, 4]));
    // Past the limit of 10 changed lines: its patch is never read, counted or not.
    fx.write("long.txt", &lines(60, &(1..=30).collect::<Vec<_>>()));
    std::fs::write(fx.path().join("blob.bin"), b"\0new").unwrap();
    fx.write("new.md", "one\ntwo\n");
    let mut repo = Repo::open(fx.path()).unwrap();
    repo.set_diff_options(oxbow_core::DiffOptions {
        max_lines: 10,
        ..Default::default()
    });

    let listed = repo.working_files().unwrap();
    assert!(!listed.counted);
    assert!(listed.unstaged.iter().all(|f| f.additions == 0 && !f.binary));
    let mut counted = listed.clone();
    repo.count_lines(&mut counted).unwrap();
    let whole = repo.working_tree().unwrap();
    assert!(counted.counted && whole.counted);
    assert_eq!(
        serde_json::to_value(&counted).unwrap(),
        serde_json::to_value(&whole).unwrap()
    );

    let never = AtomicBool::new(false);
    for side in [Side::Unstaged, Side::Staged] {
        let list = if side == Side::Staged {
            &whole.staged
        } else {
            &whole.unstaged
        };
        let paths: Vec<String> = list.iter().map(|f| f.path.clone()).collect();
        let page = |tree| {
            let page = repo
                .working_diff_page(tree, &paths, side, false, 0, usize::MAX, &never)
                .unwrap();
            serde_json::to_value(&page).unwrap()
        };
        assert_eq!(page(&listed), page(&whole), "{side:?}");
    }
    let long = repo.working_diff_page(
        &listed,
        &["long.txt".into()],
        Side::Unstaged,
        false,
        0,
        usize::MAX,
        &never,
    );
    assert!(long.unwrap().diffs[0].limited);
}

#[test]
fn picked_lines_preserve_exact_eof_bytes_in_both_directions() {
    // Exercise unchanged EOF context, an unpicked EOF replacement, and a picked EOF
    // replacement, including adding/removing the final newline and CRLF.
    for (base, edited, pick, staged) in [
        ("a\nb\nc", "A\nb\nc", "a", "A\nb\nc"),
        ("a\nb", "A\nB", "a", "A\nb"),
        ("a\nb", "A\nB", "b", "a\nB"),
        ("a\nb", "A\nB\n", "a", "A\nb"),
        ("a\nb\n", "A\nB", "a", "A\nb\n"),
        ("a\r\nb", "A\r\nB", "a", "A\r\nb"),
    ] {
        let mut fx = Fixture::new();
        fx.commit("eof.txt", base, "Root");
        fx.write("eof.txt", edited);
        let repo = Repo::open(fx.path()).unwrap();
        let hunk = repo
            .working_diff("eof.txt", Side::Unstaged, false)
            .unwrap()
            .hunks
            .remove(0);
        let selected = |hunk: &oxbow_core::Hunk| {
            hunk.lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.kind != LineKind::Context && line.text.eq_ignore_ascii_case(pick))
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        };
        let picked = selected(&hunk);
        assert_eq!(picked.len(), 2);
        repo.perform(&Action::StageHunk {
            path: "eof.txt".into(),
            header: hunk.header,
            check: hunk.check,
            lines: Some(picked),
        })
        .unwrap();
        let index = || {
            let output = std::process::Command::new("git")
                .args(["show", ":eof.txt"])
                .current_dir(fx.path())
                .output()
                .unwrap();
            assert!(output.status.success());
            output.stdout
        };
        assert_eq!(index(), staged.as_bytes(), "{base:?} -> {edited:?}, pick {pick}");
        assert_eq!(std::fs::read(fx.path().join("eof.txt")).unwrap(), edited.as_bytes());
        let hunk = repo
            .working_diff("eof.txt", Side::Staged, false)
            .unwrap()
            .hunks
            .remove(0);
        let picked = selected(&hunk);
        repo.perform(&Action::UnstageHunk {
            path: "eof.txt".into(),
            header: hunk.header,
            check: hunk.check,
            lines: Some(picked),
        })
        .unwrap();
        assert_eq!(index(), base.as_bytes());
        assert_eq!(std::fs::read(fx.path().join("eof.txt")).unwrap(), edited.as_bytes());
    }
}

#[test]
fn adding_selected_lines_after_an_unterminated_line_keeps_them_separate() {
    for (base, edited, side, kind, pick, expected) in [
        (
            "one\ntwo",
            "one\nTWO\nextra",
            Side::Unstaged,
            LineKind::Added,
            "TWO",
            "one\ntwo\nTWO",
        ),
        (
            "one\ntwo",
            "one\nTWO\nextra",
            Side::Unstaged,
            LineKind::Added,
            "extra",
            "one\ntwo\nextra",
        ),
        (
            "one\nTWO\nextra",
            "one\ntwo",
            Side::Staged,
            LineKind::Removed,
            "extra",
            "one\ntwo\nextra",
        ),
    ] {
        let mut fx = Fixture::new();
        fx.commit("eof.txt", base, "Root");
        fx.write("eof.txt", edited);
        if side == Side::Staged {
            fx.git(&["add", "eof.txt"]);
        }
        let repo = Repo::open(fx.path()).unwrap();
        let hunk = repo.working_diff("eof.txt", side, false).unwrap().hunks.remove(0);
        let picked = hunk
            .lines
            .iter()
            .enumerate()
            .filter(|(_, line)| line.kind == kind && line.text == pick)
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        assert_eq!(picked.len(), 1);
        let action = if side == Side::Staged {
            Action::UnstageHunk {
                path: "eof.txt".into(),
                header: hunk.header,
                check: hunk.check,
                lines: Some(picked),
            }
        } else {
            Action::StageHunk {
                path: "eof.txt".into(),
                header: hunk.header,
                check: hunk.check,
                lines: Some(picked),
            }
        };
        repo.perform(&action).unwrap();
        let output = std::process::Command::new("git")
            .args(["show", ":eof.txt"])
            .current_dir(fx.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, expected.as_bytes(), "{side:?}: {pick}");
        assert_eq!(std::fs::read(fx.path().join("eof.txt")).unwrap(), edited.as_bytes());
    }
}
