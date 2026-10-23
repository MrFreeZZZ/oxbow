mod support;

use oxbow_core::{Action, FailureKind, LineKind, Repo, Side};
use support::Fixture;

const BEFORE: &str = "one\ntwo\nthree\nfour\nfive\n";
/// `two` becomes `TWO`, and `2a` and `2b` are added after it: one hunk with 1 removed and 3
/// added lines.
const AFTER: &str = "one\nTWO\n2a\n2b\nthree\nfour\nfive\n";

/// The header and check of the only hunk of `path` on `side`, and indexes of its changed lines with their text.
fn changed(repo: &Repo, path: &str, side: Side) -> (String, String, Vec<(usize, LineKind, String)>) {
    let diff = repo.working_diff(path, side, false).unwrap();
    assert_eq!(diff.hunks.len(), 1);
    let hunk = &diff.hunks[0];
    let lines = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.kind != LineKind::Context)
        .map(|(i, l)| (i, l.kind, l.text.clone()))
        .collect();
    (hunk.header.clone(), hunk.check.clone(), lines)
}

fn index_of(lines: &[(usize, LineKind, String)], text: &str) -> usize {
    lines.iter().find(|(_, _, t)| t == text).unwrap().0
}

fn staged_text(fx: &mut Fixture, path: &str) -> String {
    fx.git(&["show", &format!(":{path}")]) + "\n"
}

#[test]
fn single_lines_are_staged_unstaged_and_discarded() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", BEFORE, "Root");
    fx.write("a.txt", AFTER);
    let repo = Repo::open(fx.path()).unwrap();

    // Stage the replacement of `two` and `2b`, but not `2a`.
    let (header, check, lines) = changed(&repo, "a.txt", Side::Unstaged);
    let picked = vec![index_of(&lines, "two"), index_of(&lines, "TWO"), index_of(&lines, "2b")];
    let stage = Action::StageHunk {
        path: "a.txt".into(),
        header,
        check,
        lines: Some(picked),
    };
    let plan = repo.plan(&stage).unwrap();
    let patch = plan.commands[0].input.clone().unwrap();
    assert!(
        patch.contains("@@ -1,5 +1,6 @@\n one\n-two\n+TWO\n+2b\n three\n"),
        "{patch}"
    );
    assert_eq!(plan.commands[0].display(), "git apply --cached -");
    repo.perform(&stage).unwrap();
    assert_eq!(staged_text(&mut fx, "a.txt"), "one\nTWO\n2b\nthree\nfour\nfive\n");
    // The file itself keeps every change; only `2a` is left unstaged.
    assert_eq!(std::fs::read_to_string(fx.path().join("a.txt")).unwrap(), AFTER);
    let (_, _, left) = changed(&repo, "a.txt", Side::Unstaged);
    assert_eq!(left.iter().map(|l| l.2.as_str()).collect::<Vec<_>>(), ["2a"]);

    // Unstage `2b` again, keeping the staged replacement.
    let (header, check, lines) = changed(&repo, "a.txt", Side::Staged);
    repo.perform(&Action::UnstageHunk {
        path: "a.txt".into(),
        header,
        check,
        lines: Some(vec![index_of(&lines, "2b")]),
    })
    .unwrap();
    assert_eq!(staged_text(&mut fx, "a.txt"), "one\nTWO\nthree\nfour\nfive\n");

    // Discard `2a` from the file; `2b` stays.
    let (header, check, lines) = changed(&repo, "a.txt", Side::Unstaged);
    repo.perform(&Action::DiscardHunk {
        path: "a.txt".into(),
        header,
        check,
        lines: Some(vec![index_of(&lines, "2a")]),
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        "one\nTWO\n2b\nthree\nfour\nfive\n"
    );

    // Nothing picked is refused.
    let (header, check, _) = changed(&repo, "a.txt", Side::Unstaged);
    let err = repo
        .perform(&Action::StageHunk {
            path: "a.txt".into(),
            header,
            check,
            lines: Some(Vec::new()),
        })
        .unwrap_err();
    assert!(err.to_string().contains("at least one"), "{err}");
}

#[test]
fn a_last_line_without_newline_can_be_staged_alone() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\ntwo", "Root");
    fx.write("a.txt", "one\ntwo\nthree\nfour");
    let repo = Repo::open(fx.path()).unwrap();
    let (header, check, lines) = changed(&repo, "a.txt", Side::Unstaged);
    // -two (no newline), +two, +three, +four (no newline): stage only the newline fix of `two`.
    repo.perform(&Action::StageHunk {
        path: "a.txt".into(),
        header,
        check,
        lines: Some(vec![lines[0].0, lines[1].0]),
    })
    .unwrap();
    let staged = std::process::Command::new("git")
        .args(["show", ":a.txt"])
        .current_dir(fx.path())
        .output()
        .unwrap();
    assert_eq!(String::from_utf8(staged.stdout).unwrap(), "one\ntwo\n");
}

#[test]
fn a_file_is_ignored_with_a_staged_rule() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("debug.log", "noise\n");
    fx.write("logs/other.log", "noise\n");
    fx.write(".env.local", "SECRET=1\n");
    let repo = Repo::open(fx.path()).unwrap();

    let ignore = Action::Ignore {
        pattern: "*.log".into(),
    };
    let plan = repo.plan(&ignore).unwrap();
    assert_eq!(plan.commands[0].before.as_deref(), Some("echo '*.log' >> .gitignore"));
    assert_eq!(plan.commands[0].display(), "git add -- .gitignore");
    repo.perform(&ignore).unwrap();
    // Twice is the same as once.
    repo.perform(&ignore).unwrap();
    repo.perform(&Action::Ignore {
        pattern: "/.env.local".into(),
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.path().join(".gitignore")).unwrap(),
        "*.log\n/.env.local\n"
    );
    let tree = repo.working_tree().unwrap();
    let staged: Vec<_> = tree.staged.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(staged, [".gitignore"]);
    assert!(tree.unstaged.is_empty(), "{:?}", tree.unstaged);
}

#[test]
fn ignoring_keeps_a_gitignore_that_is_not_utf8() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    std::fs::write(fx.path().join(".gitignore"), b"# caf\xe9\nimportant/\n").unwrap();
    fx.write("new-file.log", "noise\n");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::Ignore {
        pattern: "new-file.log".into(),
    })
    .unwrap();
    assert_eq!(
        std::fs::read(fx.path().join(".gitignore")).unwrap(),
        b"# caf\xe9\nimportant/\nnew-file.log\n"
    );
}

#[cfg(unix)]
#[test]
fn ignoring_never_writes_through_a_gitignore_symlink() {
    // A repository commits `.gitignore` as a link to a file outside it.
    let mut fx = Fixture::new();
    let outside = fx
        .path()
        .parent()
        .unwrap()
        .join(format!("outside-{}", std::process::id()));
    std::fs::write(&outside, "mine\n").unwrap();
    std::os::unix::fs::symlink(&outside, fx.path().join(".gitignore")).unwrap();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("debug.log", "noise\n");
    let repo = Repo::open(fx.path()).unwrap();

    let err = repo
        .perform(&Action::Ignore {
            pattern: "*.log".into(),
        })
        .unwrap_err();
    assert!(err.to_string().contains("not a plain file"), "{err}");
    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "mine\n");
    assert!(fx.path().join(".gitignore").symlink_metadata().unwrap().is_symlink());
    std::fs::remove_file(outside).unwrap();
}

#[test]
fn chosen_files_are_stashed_and_the_rest_stays() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.commit("b.txt", "b\n", "Second");
    fx.write("a.txt", "a changed\n");
    fx.write("b.txt", "b changed\n");
    fx.write("new.txt", "new\n");
    let repo = Repo::open(fx.path()).unwrap();
    let push = Action::StashPush {
        message: Some("WIP: a".into()),
        untracked: true,
        paths: vec!["a.txt".into(), "new.txt".into()],
    };
    assert_eq!(
        repo.plan(&push).unwrap().commands[0].display(),
        "git stash push --include-untracked -m 'WIP: a' -- a.txt new.txt"
    );
    repo.perform(&push).unwrap();
    let tree = repo.working_tree().unwrap();
    let left: Vec<_> = tree.unstaged.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(left, ["b.txt"]);
    assert!(!fx.path().join("new.txt").exists());
    assert!(repo.stashes().unwrap()[0].message.ends_with("WIP: a"));
}

#[test]
fn a_failing_pre_commit_hook_is_told_apart_and_can_be_skipped() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let hook = fx.path().join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho 'prettier: a.txt is not formatted' >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    fx.write("a.txt", "a b\n");
    fx.git(&["add", "a.txt"]);
    let repo = Repo::open(fx.path()).unwrap();

    let commit = Action::Commit {
        message: "Change a".into(),
        amend: false,
        no_verify: false,
        allow_secrets: false,
    };
    let err = repo.perform(&commit).unwrap_err();
    let failure = repo.explain_failure(&commit, &err);
    assert_eq!(failure.kind, FailureKind::Hook);
    assert_eq!(failure.hook.as_deref(), Some("pre-commit"));
    assert!(failure.output.contains("not formatted"), "{}", failure.output);

    let skip = Action::Commit {
        message: "Change a".into(),
        amend: false,
        no_verify: true,
        allow_secrets: false,
    };
    assert_eq!(
        repo.plan(&skip).unwrap().commands[0].display(),
        "git commit --no-verify -m 'Change a'"
    );
    repo.perform(&skip).unwrap();
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Change a");
}

#[test]
fn a_failing_commit_msg_hook_stops_an_edited_message_and_can_be_skipped() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let hook = fx.path().join(".git/hooks/commit-msg");
    std::fs::write(
        &hook,
        "#!/bin/sh\necho 'commit-msg: needs a ticket number' >&2\nexit 1\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let repo = Repo::open(fx.path()).unwrap();

    let reword = Action::Reword {
        message: "Root commit".into(),
        no_verify: false,
    };
    let err = repo.perform(&reword).unwrap_err();
    let failure = repo.explain_failure(&reword, &err);
    assert_eq!(failure.kind, FailureKind::Hook);
    assert_eq!(failure.hook.as_deref(), Some("commit-msg"));
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Root");

    let skip = Action::Reword {
        message: "Root commit".into(),
        no_verify: true,
    };
    assert_eq!(
        repo.plan(&skip).unwrap().commands[0].display(),
        "git commit --amend --only --no-verify -m 'Root commit'"
    );
    repo.perform(&skip).unwrap();
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Root commit");
}

#[test]
fn working_diffs_follow_the_paths_on_one_side() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", BEFORE, "Root");
    fx.commit("b.txt", BEFORE, "Second");
    fx.write("a.txt", AFTER);
    fx.write("b.txt", AFTER);
    fx.git(&["add", "b.txt"]);
    fx.write("new.txt", "fresh\n");
    let repo = Repo::open(fx.path()).unwrap();

    let paths = |list: Vec<oxbow_core::FileDiff>| list.into_iter().map(|d| d.file.path).collect::<Vec<_>>();
    let wanted = ["new.txt", "b.txt", "a.txt"].map(String::from);
    // b.txt is staged only, so the unstaged side leaves it out; the order is the one asked for.
    let unstaged = repo.working_diffs(&wanted, Side::Unstaged, false).unwrap();
    assert_eq!(paths(unstaged), ["new.txt", "a.txt"]);
    let staged = repo.working_diffs(&wanted, Side::Staged, false).unwrap();
    assert_eq!(staged[0].hunks.len(), 1);
    assert_eq!(paths(staged), ["b.txt"]);
}

#[test]
fn a_page_of_working_diffs_matches_each_file_read_alone() {
    use std::sync::atomic::AtomicBool;
    let mut fx = Fixture::new();
    fx.commit("a.txt", BEFORE, "Root");
    fx.commit("dir with space/b.txt", BEFORE, "Second");
    fx.commit("ünï \"q\".txt", BEFORE, "Third");
    fx.commit("gone.txt", BEFORE, "Fourth");
    fx.commit("x b/x", BEFORE, "Fifth");
    fx.write("a.txt", AFTER);
    fx.write("dir with space/b.txt", AFTER);
    fx.git(&["add", "dir with space/b.txt"]);
    fx.write("dir with space/b.txt", "zero\none\nTWO\n2a\n2b\nthree\nfour\nfive\n");
    fx.write("ünï \"q\".txt", "one\ntwo\n");
    fx.write("x b/x", AFTER);
    std::fs::remove_file(fx.path().join("gone.txt")).unwrap();
    // New files of every kind the page reads itself, or leaves to git.
    fx.write("new/plain.txt", "fresh\nlines\n");
    fx.write("new/one.txt", "single");
    fx.write("new/crlf.txt", "a\r\nb\r\n");
    fx.write("new/empty.txt", "");
    fx.write("new/ünï.txt", "ü\n");
    std::fs::write(fx.path().join("new/blob.bin"), b"\0\x01\x02").unwrap();
    fx.write("new/nodiff.dat", "text\n");
    fx.write(".gitattributes", "*.dat -diff\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fx.write("new/run.sh", "#!/bin/sh\necho hi\n");
        std::fs::set_permissions(fx.path().join("new/run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("plain.txt", fx.path().join("new/link")).unwrap();
    }
    let repo = Repo::open(fx.path()).unwrap();
    let tree = repo.working_tree().unwrap();

    for side in [Side::Unstaged, Side::Staged] {
        let list = if side == Side::Staged {
            &tree.staged
        } else {
            &tree.unstaged
        };
        let paths: Vec<String> = list.iter().map(|f| f.path.clone()).collect();
        for whole in [false, true] {
            let never = AtomicBool::new(false);
            let page = repo
                .working_diff_page(&tree, &paths, side, whole, 0, usize::MAX, &never)
                .unwrap();
            assert_eq!(page.next, None);
            assert_eq!(page.diffs.len(), paths.len());
            for (diff, path) in page.diffs.iter().zip(&paths) {
                let alone = repo.working_diff(path, side, whole).unwrap();
                assert_eq!(
                    serde_json::to_value(diff).unwrap(),
                    serde_json::to_value(&alone).unwrap(),
                    "{path} on {side:?}, whole file: {whole}"
                );
            }
        }
    }

    // Pages follow the budget, each with at least one file, and a set Stop stops them.
    let paths: Vec<String> = tree.unstaged.iter().map(|f| f.path.clone()).collect();
    let never = AtomicBool::new(false);
    let first = repo
        .working_diff_page(&tree, &paths, Side::Unstaged, false, 0, 1, &never)
        .unwrap();
    assert_eq!(first.diffs.len(), 1);
    assert_eq!(first.next, Some(1));
    assert_eq!(first.files, paths.len());
    let stop = AtomicBool::new(true);
    assert!(matches!(
        repo.working_diff_page(&tree, &paths, Side::Unstaged, false, 0, 1, &stop),
        Err(oxbow_core::Error::Cancelled)
    ));
}

/// `count` committed files of `lines` lines each, with only their last line changed since.
fn long_files_with_a_last_line_changed(count: usize, lines: usize) -> (Fixture, Vec<String>) {
    let mut fx = Fixture::new();
    let text: String = (0..lines).map(|i| format!("line {i}\n")).collect();
    let paths: Vec<String> = (0..count).map(|i| format!("f{i:03}.txt")).collect();
    for path in &paths {
        fx.write(path, &text);
    }
    fx.git(&["add", "."]);
    fx.git(&["commit", "-q", "-m", "Long files"]);
    for path in &paths {
        fx.write(path, &format!("{text}changed\n"));
    }
    (fx, paths)
}

#[test]
fn a_whole_file_page_keeps_to_its_budget_in_lines_read() {
    use std::sync::atomic::AtomicBool;
    // Each file changes by one line but shows 10,001 as a whole file: the counts alone would put
    // all 100 in the first page of 4,000 lines.
    let (fx, paths) = long_files_with_a_last_line_changed(100, 10_000);
    let repo = Repo::open(fx.path()).unwrap();
    let tree = repo.working_tree().unwrap();
    let never = AtomicBool::new(false);
    let lines = |page: &oxbow_core::DiffPage| -> usize {
        page.diffs.iter().flat_map(|d| &d.hunks).map(|h| h.lines.len()).sum()
    };
    let mut from = Some(0);
    let mut seen = Vec::new();
    while let Some(at) = from {
        let page = repo
            .working_diff_page(&tree, &paths, Side::Unstaged, true, at, 4000, &never)
            .unwrap();
        // One file over the budget is a page of its own.
        assert_eq!(page.diffs.len(), 1, "page from {at}");
        assert_eq!(lines(&page), 10_001);
        seen.extend(page.diffs.iter().map(|d| d.file.path.clone()));
        from = page.next;
    }
    assert_eq!(seen, paths);
    // Compact, they still fit in one page.
    let page = repo
        .working_diff_page(&tree, &paths, Side::Unstaged, false, 0, 4000, &never)
        .unwrap();
    assert_eq!(page.diffs.len(), 100);
    assert!(lines(&page) < 4000);
}

#[test]
fn a_page_stops_while_it_is_read() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    let (fx, paths) = long_files_with_a_last_line_changed(60, 20_000);
    let repo = Repo::open(fx.path()).unwrap();
    let tree = repo.working_tree().unwrap();
    // Read whole, all of it takes a while: long enough to be stopped on the way.
    let never = AtomicBool::new(false);
    let started = Instant::now();
    let all = repo
        .working_diff_page(&tree, &paths, Side::Unstaged, true, 0, usize::MAX, &never)
        .unwrap();
    let whole = started.elapsed();
    assert_eq!(all.diffs.len(), 60);
    drop(all);

    let cancel = Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    let wait = whole / 10;
    let later = std::thread::spawn(move || {
        std::thread::sleep(wait);
        stop.store(true, Ordering::Relaxed);
        Instant::now()
    });
    let out = repo.working_diff_page(&tree, &paths, Side::Unstaged, true, 0, usize::MAX, &cancel);
    let ended = Instant::now();
    // Timed from when Stop was set, not from the start: on a busy machine the sleep before it
    // can last much longer than asked.
    let stopped = later.join().unwrap();
    assert!(matches!(out, Err(oxbow_core::Error::Cancelled)));
    let took = ended.saturating_duration_since(stopped);
    assert!(
        took < whole / 2 + Duration::from_millis(50),
        "stopped {took:?} after Stop, the whole read took {whole:?}"
    );
}
