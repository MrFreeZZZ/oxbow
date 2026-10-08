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
    };
    assert_eq!(
        repo.plan(&skip).unwrap().commands[0].display(),
        "git commit --no-verify -m 'Change a'"
    );
    repo.perform(&skip).unwrap();
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Change a");
}
