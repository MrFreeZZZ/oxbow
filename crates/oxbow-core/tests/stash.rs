mod support;

use oxbow_core::{Action, ActionEvent, ConflictSide, DiffContext, OperationKind, Repo};
use support::Fixture;

fn read(fx: &Fixture, file: &str) -> String {
    std::fs::read_to_string(fx.path().join(file)).unwrap_or_default()
}

/// A stash on `main` that changes `a.txt` and adds the untracked `new.txt`.
fn stashed() -> (Fixture, Repo) {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\ntwo\nthree\n", "Root");
    fx.write("a.txt", "one\nTWO stashed\nthree\n");
    fx.write("new.txt", "new\n");
    let repo = Repo::open(fx.path()).unwrap();
    let push = Action::StashPush {
        message: Some("Shout two".into()),
        untracked: true,
        paths: Vec::new(),
        staged: false,
    };
    assert_eq!(
        repo.plan(&push).unwrap().commands[0].display(),
        "git stash push --include-untracked -m 'Shout two'"
    );
    repo.perform(&push).unwrap();
    (fx, repo)
}

fn apply(repo: &Repo, pop: bool) -> Action {
    let stash = &repo.stashes().unwrap()[0];
    Action::StashApply {
        index: 0,
        id: stash.id.clone(),
        pop,
        keep_index: false,
    }
}

#[test]
fn a_stash_knows_its_branch_base_and_untracked_files() {
    let (mut fx, repo) = stashed();
    let stash = &repo.stashes().unwrap()[0];
    assert_eq!(stash.branch.as_deref(), Some("main"));
    assert_eq!(stash.title, "Shout two");
    assert_eq!(stash.base, fx.git(&["rev-parse", "HEAD"]));
    assert!(stash.untracked.is_some());
    assert_eq!(fx.git(&["status", "--porcelain"]), "");
}

#[test]
fn a_stash_commit_lists_its_untracked_files_too() {
    let (_fx, repo) = stashed();
    let id = repo.stashes().unwrap()[0].id.clone();
    let files: Vec<String> = repo
        .commit_detail(&id)
        .unwrap()
        .files
        .into_iter()
        .map(|f| f.path)
        .collect();
    assert_eq!(files, ["a.txt", "new.txt"]);
    let diff = repo.commit_diff(&id, Some("new.txt"), DiffContext::Compact).unwrap();
    assert_eq!(diff.len(), 1);
    assert_eq!(diff[0].file.additions, 1);
}

#[test]
fn apply_keeps_the_stash_and_pop_deletes_it() {
    let (fx, repo) = stashed();
    repo.perform(&apply(&repo, false)).unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\nTWO stashed\nthree\n");
    assert_eq!(read(&fx, "new.txt"), "new\n");
    assert_eq!(repo.stashes().unwrap().len(), 1);

    let (fx, repo) = stashed();
    repo.perform(&apply(&repo, true)).unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\nTWO stashed\nthree\n");
    assert!(repo.stashes().unwrap().is_empty());
}

#[test]
fn a_dropped_stash_can_be_put_back() {
    let (_fx, repo) = stashed();
    let stash = repo.stashes().unwrap()[0].clone();
    repo.perform(&Action::StashDrop {
        index: 0,
        id: stash.id.clone(),
    })
    .unwrap();
    assert!(repo.stashes().unwrap().is_empty());
    repo.perform(&Action::StashStore {
        id: stash.id.clone(),
        message: stash.message.clone(),
    })
    .unwrap();
    let back = &repo.stashes().unwrap()[0];
    assert_eq!(
        (back.id.as_str(), back.title.as_str()),
        (stash.id.as_str(), "Shout two")
    );
}

#[test]
fn an_action_on_a_stash_that_moved_is_refused() {
    let (fx, repo) = stashed();
    let old = repo.stashes().unwrap()[0].id.clone();
    fx.write("a.txt", "another\n");
    repo.perform(&Action::StashPush {
        message: None,
        untracked: false,
        paths: Vec::new(),
        staged: false,
    })
    .unwrap();
    // stash@{0} is the new one now.
    let drop = Action::StashDrop { index: 0, id: old };
    assert!(repo.perform(&drop).is_err());
    assert_eq!(repo.stashes().unwrap().len(), 2);
}

#[test]
fn the_check_predicts_conflicts_and_files_in_the_way() {
    let (mut fx, repo) = stashed();
    let stash = repo.stashes().unwrap()[0].clone();
    let check = repo.stash_check(0, &stash.id).unwrap();
    assert_eq!(check.conflicts, Some(vec![]));
    assert!(check.in_the_way.is_empty());

    fx.commit("a.txt", "one\n2 committed\nthree\n", "Number two");
    fx.write("new.txt", "mine\n");
    let check = repo.stash_check(0, &stash.id).unwrap();
    assert_eq!(check.conflicts, Some(vec!["a.txt".to_owned()]));
    assert_eq!(check.in_the_way, ["new.txt"]);
}

#[test]
fn a_pop_that_conflicts_is_resolved_and_finished() {
    let (mut fx, repo) = stashed();
    fx.commit("a.txt", "one\n2 committed\nthree\n", "Number two");
    // Staged before the pop: finishing must leave it staged.
    fx.write("mine.txt", "my own work\n");
    fx.git(&["add", "mine.txt"]);
    repo.perform(&apply(&repo, true)).unwrap_err();

    let op = repo.operation().unwrap().expect("an apply in progress");
    assert_eq!(op.kind, OperationKind::StashApply);
    assert_eq!(op.conflicted, 1);
    assert_eq!(
        (op.ours_label.as_str(), op.theirs_label.as_str()),
        ("main", "stash@{0}")
    );
    assert_eq!(op.message.as_deref(), Some("Shout two"));

    repo.perform(&Action::TakeFile {
        path: "a.txt".into(),
        side: ConflictSide::Theirs,
    })
    .unwrap();
    let finish = repo.plan(&Action::Continue { message: None }).unwrap();
    let shown: Vec<String> = finish.commands.iter().map(|c| c.display()).collect();
    assert_eq!(
        shown,
        ["git reset --quiet -- a.txt", "git stash drop --quiet stash@{0}"]
    );
    repo.perform(&Action::Continue { message: None }).unwrap();

    assert!(repo.operation().unwrap().is_none());
    assert!(repo.stashes().unwrap().is_empty());
    assert_eq!(read(&fx, "a.txt"), "one\nTWO stashed\nthree\n");
    assert_eq!(read(&fx, "new.txt"), "new\n");
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "mine.txt");
}

#[test]
fn undo_apply_takes_back_only_what_the_stash_brought() {
    let (mut fx, repo) = stashed();
    fx.commit("a.txt", "one\n2 committed\nthree\n", "Number two");
    fx.write("mine.txt", "my own work\n");
    fx.git(&["add", "mine.txt"]);
    repo.perform(&apply(&repo, false)).unwrap_err();
    assert_eq!(repo.operation().unwrap().unwrap().kind, OperationKind::StashApply);

    repo.perform(&Action::Abort).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(read(&fx, "a.txt"), "one\n2 committed\nthree\n");
    assert!(!fx.path().join("new.txt").exists());
    assert_eq!(read(&fx, "mine.txt"), "my own work\n");
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "mine.txt");
    assert_eq!(repo.stashes().unwrap().len(), 1);
}

#[test]
fn a_stash_becomes_a_branch() {
    let (mut fx, repo) = stashed();
    fx.commit("a.txt", "one\n2 committed\nthree\n", "Number two");
    let stash = repo.stashes().unwrap()[0].clone();
    repo.perform(&Action::StashBranch {
        index: 0,
        id: stash.id,
        name: "shout".into(),
    })
    .unwrap();
    assert_eq!(fx.git(&["branch", "--show-current"]), "shout");
    assert_eq!(read(&fx, "a.txt"), "one\nTWO stashed\nthree\n");
    assert!(repo.stashes().unwrap().is_empty());
}

#[test]
fn undo_apply_keeps_an_untracked_file_that_was_there_before() {
    let (mut fx, repo) = stashed();
    fx.commit("a.txt", "one\n2 committed\nthree\n", "Number two");
    // Made after the check before the apply, so git does not restore over it.
    fx.write("new.txt", "my own new.txt\n");
    let err = repo.perform(&apply(&repo, false)).unwrap_err();
    assert!(err.to_string().contains("already exists"), "{err}");
    assert_eq!(repo.operation().unwrap().unwrap().kind, OperationKind::StashApply);

    let undo = repo.plan(&Action::Abort).unwrap();
    assert!(undo.commands.iter().all(|c| !c.display().contains("clean")), "{undo:?}");
    repo.perform(&Action::Abort).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(read(&fx, "a.txt"), "one\n2 committed\nthree\n");
    assert_eq!(read(&fx, "new.txt"), "my own new.txt\n");
}

#[test]
fn undo_apply_keeps_an_untracked_file_made_just_before_git_ran() {
    let (mut fx, repo) = stashed();
    fx.commit("a.txt", "one\n2 committed\nthree\n", "Number two");
    // An editor saves new.txt after Oxbow looked and before git restores the stash's files.
    let path = fx.path().join("new.txt");
    let mut on_event = |event: ActionEvent| {
        if matches!(event, ActionEvent::Command { .. }) && !path.exists() {
            std::fs::write(&path, "my own new.txt\n").unwrap();
        }
    };
    let err = repo
        .perform_with(
            &apply(&repo, false),
            &mut on_event,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap_err();
    assert!(err.to_string().contains("already exists"), "{err}");

    repo.perform(&Action::Abort).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(read(&fx, "a.txt"), "one\n2 committed\nthree\n");
    assert_eq!(read(&fx, "new.txt"), "my own new.txt\n");
}

#[test]
fn undo_apply_works_for_a_stash_without_untracked_files() {
    for pop in [false, true] {
        let mut fx = Fixture::new();
        fx.commit("a.txt", "one\ntwo\nthree\n", "Root");
        fx.write("a.txt", "one\nTWO stashed\nthree\n");
        let repo = Repo::open(fx.path()).unwrap();
        repo.perform(&Action::StashPush {
            message: None,
            untracked: false,
            paths: Vec::new(),
            staged: false,
        })
        .unwrap();
        fx.commit("a.txt", "one\n2 committed\nthree\n", "Number two");
        repo.perform(&apply(&repo, pop)).unwrap_err();

        repo.perform(&Action::Abort).unwrap();
        assert!(repo.operation().unwrap().is_none());
        assert_eq!(read(&fx, "a.txt"), "one\n2 committed\nthree\n");
        assert_eq!(repo.stashes().unwrap().len(), 1);
    }
}

#[test]
fn undo_apply_keeps_a_restored_file_edited_after_the_plan_was_made() {
    let (mut fx, repo) = stashed();
    fx.commit("a.txt", "one\n2 committed\nthree\n", "Number two");
    repo.perform(&apply(&repo, false)).unwrap_err();
    assert_eq!(read(&fx, "new.txt"), "new\n");

    // The sheet lists new.txt for removal; then an editor saves it before Undo is pressed.
    let plan = repo.plan(&Action::Abort).unwrap();
    assert!(
        plan.commands[0]
            .before
            .as_deref()
            .unwrap_or_default()
            .contains("new.txt"),
        "{plan:?}"
    );
    fx.write("new.txt", "edit saved after the plan\n");
    // And another edit lands while git runs.
    let path = fx.path().join("other.txt");
    let mut on_event = |event: ActionEvent| {
        if matches!(event, ActionEvent::Command { .. }) && !path.exists() {
            std::fs::write(&path, "x\n").unwrap();
        }
    };
    repo.perform_with(
        &Action::Abort,
        &mut on_event,
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(read(&fx, "new.txt"), "edit saved after the plan\n");
    assert_eq!(read(&fx, "a.txt"), "one\n2 committed\nthree\n");
}

/// Every file under `.git/oxbow/removed`, with its content.
fn removed(fx: &Fixture) -> Vec<String> {
    fn walk(dir: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if entry.path().is_dir() {
                walk(&entry.path(), out);
            } else {
                out.push(std::fs::read_to_string(entry.path()).unwrap());
            }
        }
    }
    let mut out = Vec::new();
    walk(&fx.path().join(".git/oxbow/removed"), &mut out);
    out.sort();
    out
}

#[test]
fn undo_apply_moves_restored_files_aside_and_keeps_writes_through_an_open_file() {
    use std::io::Write;
    let (mut fx, repo) = stashed();
    fx.commit("a.txt", "one\n2 committed\nthree\n", "Number two");
    repo.perform(&apply(&repo, false)).unwrap_err();
    // An editor has new.txt open and writes to it after the undo moved it.
    let mut open = std::fs::OpenOptions::new()
        .append(true)
        .open(fx.path().join("new.txt"))
        .unwrap();

    repo.perform(&Action::Abort).unwrap();
    assert!(!fx.path().join("new.txt").exists());
    open.write_all(b"edit through the open file\n").unwrap();
    drop(open);
    assert_eq!(removed(&fx), ["new\nedit through the open file\n"]);
}

#[test]
fn each_undo_keeps_its_own_copies() {
    let (mut fx, repo) = stashed();
    fx.commit("a.txt", "one\n2 committed\nthree\n", "Number two");
    for _ in 0..2 {
        repo.perform(&apply(&repo, false)).unwrap_err();
        repo.perform(&Action::Abort).unwrap();
    }
    assert_eq!(removed(&fx), ["new\n", "new\n"]);
    assert_eq!(repo.stashes().unwrap().len(), 1);
}

/// Ten lines, `n1` … `n10`, so changes at the top and the bottom are two hunks.
const TEN: &str = "n1\nn2\nn3\nn4\nn5\nn6\nn7\nn8\nn9\nn10\n";

#[test]
fn stash_staged_keeps_what_is_not_staged() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", TEN, "Root");
    fx.commit("c.txt", "c\n", "Second");
    fx.write("a.txt", &TEN.replace("n1\n", "N1 staged\n"));
    fx.write("b.txt", "new and staged\n");
    fx.git(&["add", "a.txt", "b.txt"]);
    fx.write(
        "a.txt",
        &TEN.replace("n1\n", "N1 staged\n").replace("n10\n", "N10 not staged\n"),
    );
    fx.write("c.txt", "c not staged\n");
    let repo = Repo::open(fx.path()).unwrap();

    let push = Action::StashPush {
        message: Some("Staged bits".into()),
        untracked: true,
        paths: Vec::new(),
        staged: true,
    };
    let plan = repo.plan(&push).unwrap();
    assert_eq!(plan.commands[0].display(), "git stash push --staged -m 'Staged bits'");
    repo.perform(&push).unwrap();

    assert_eq!(repo.stashes().unwrap()[0].title, "Staged bits");
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "", "nothing is staged");
    assert_eq!(read(&fx, "a.txt"), TEN.replace("n10\n", "N10 not staged\n"));
    assert_eq!(read(&fx, "c.txt"), "c not staged\n");
    assert!(!fx.path().join("b.txt").exists());
    let stashed = fx.git(&["stash", "show", "--name-only", "stash@{0}"]);
    assert_eq!(stashed, "a.txt\nb.txt");
    assert!(!fx.git(&["stash", "show", "-p", "stash@{0}"]).contains("N10"));
}

#[test]
fn a_hunk_goes_into_a_stash_and_out_of_the_file() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", TEN, "Root");
    let edited = TEN.replace("n1\n", "N1 top\n").replace("n10\n", "N10 bottom\n");
    fx.write("a.txt", &edited);
    let repo = Repo::open(fx.path()).unwrap();
    let diff = repo.working_diff("a.txt", oxbow_core::Side::Unstaged, false).unwrap();
    assert_eq!(diff.hunks.len(), 2);
    let top = &diff.hunks[0];
    let action = Action::StashHunk {
        path: "a.txt".into(),
        header: top.header.clone(),
        check: top.check.clone(),
        lines: None,
        message: None,
    };
    let shown: Vec<String> = repo
        .plan(&action)
        .unwrap()
        .commands
        .iter()
        .map(|c| c.display())
        .collect();
    assert_eq!(shown.len(), 2, "{shown:?}");
    assert!(shown[0].starts_with("git stash store -m 'WIP on main: "), "{shown:?}");
    assert!(
        shown[0].ends_with(" Root'") || shown[0].contains(" Root' "),
        "{shown:?}"
    );
    assert_eq!(shown[1], "git apply --reverse -");
    repo.perform(&action).unwrap();

    assert_eq!(read(&fx, "a.txt"), TEN.replace("n10\n", "N10 bottom\n"));
    let stash = &repo.stashes().unwrap()[0];
    assert_eq!(stash.branch.as_deref(), Some("main"));
    assert_eq!(stash.base, fx.git(&["rev-parse", "HEAD"]));
    let patch = fx.git(&["stash", "show", "-p", "stash@{0}"]);
    assert!(patch.contains("+N1 top") && !patch.contains("N10"), "{patch}");

    // It comes back like any stash; git wants the file it changes without other edits.
    fx.git(&["checkout", "--", "a.txt"]);
    repo.perform(&apply(&repo, true)).unwrap();
    assert_eq!(read(&fx, "a.txt"), TEN.replace("n1\n", "N1 top\n"));
    assert!(repo.stashes().unwrap().is_empty());
}

#[test]
fn picked_lines_go_into_a_stash_and_staged_changes_stay() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", TEN, "Root");
    // n1 is staged; n9 and n10 changed in the file, one hunk, of which only n10 is picked.
    fx.write("a.txt", &TEN.replace("n1\n", "N1 staged\n"));
    fx.git(&["add", "a.txt"]);
    let edited = TEN
        .replace("n1\n", "N1 staged\n")
        .replace("n9\n", "N9 stays\n")
        .replace("n10\n", "N10 picked\n");
    fx.write("a.txt", &edited);
    let repo = Repo::open(fx.path()).unwrap();
    let diff = repo.working_diff("a.txt", oxbow_core::Side::Unstaged, false).unwrap();
    assert_eq!(diff.hunks.len(), 1);
    let hunk = &diff.hunks[0];
    let picked: Vec<usize> = hunk
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.text.contains("10") && l.kind != oxbow_core::LineKind::Context)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(picked.len(), 2, "n10 and N10 picked");
    repo.perform(&Action::StashHunk {
        path: "a.txt".into(),
        header: hunk.header.clone(),
        check: hunk.check.clone(),
        lines: Some(picked),
        message: Some("Bottom line".into()),
    })
    .unwrap();

    assert_eq!(read(&fx, "a.txt"), edited.replace("N10 picked\n", "n10\n"));
    assert_eq!(
        fx.git(&["show", ":a.txt"]) + "\n",
        TEN.replace("n1\n", "N1 staged\n"),
        "the staged change stays staged"
    );
    let stash = &repo.stashes().unwrap()[0];
    assert_eq!(stash.title, "Bottom line");
    let patch = fx.git(&["stash", "show", "-p", "stash@{0}"]);
    assert!(patch.contains("+N10 picked"), "{patch}");
    assert!(!patch.contains("N9") && !patch.contains("N1 staged"), "{patch}");
    // Nothing of it is staged in the stash: --index brings back no staged lines.
    assert_eq!(fx.git(&["diff", "stash@{0}^1", "stash@{0}^2"]), "");
}

#[test]
fn stashing_lines_is_undone_whole() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", TEN, "Root");
    let edited = TEN.replace("n5\n", "N5\n");
    fx.write("a.txt", &edited);
    let repo = Repo::open(fx.path()).unwrap();
    let diff = repo.working_diff("a.txt", oxbow_core::Side::Unstaged, false).unwrap();
    let hunk = &diff.hunks[0];
    repo.perform(&Action::StashHunk {
        path: "a.txt".into(),
        header: hunk.header.clone(),
        check: hunk.check.clone(),
        lines: None,
        message: Some("Five".into()),
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt"), TEN);
    assert_eq!(repo.stashes().unwrap().len(), 1);

    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].kind, "stash");
    repo.perform(&Action::Restore {
        id: log[0].id.clone(),
        after: false,
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt"), edited);
    assert!(repo.stashes().unwrap().is_empty());
}

#[test]
fn a_hunk_that_changed_since_it_was_shown_is_refused() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", TEN, "Root");
    fx.write("a.txt", &TEN.replace("n5\n", "N5\n"));
    let repo = Repo::open(fx.path()).unwrap();
    let diff = repo.working_diff("a.txt", oxbow_core::Side::Unstaged, false).unwrap();
    let hunk = diff.hunks[0].clone();
    fx.write("a.txt", &TEN.replace("n5\n", "N5 again\n"));
    let err = repo
        .perform(&Action::StashHunk {
            path: "a.txt".into(),
            header: hunk.header,
            check: hunk.check,
            lines: None,
            message: None,
        })
        .unwrap_err();
    assert!(err.to_string().contains("changed since it was shown"), "{err}");
    assert!(repo.stashes().unwrap().is_empty());
    assert_eq!(read(&fx, "a.txt"), TEN.replace("n5\n", "N5 again\n"));
}

#[test]
fn lines_next_to_staged_changes_are_refused_with_the_way_out() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", TEN, "Root");
    fx.write("a.txt", &TEN.replace("n5\n", "N5 staged\n"));
    fx.git(&["add", "a.txt"]);
    let edited = TEN.replace("n5\n", "N5 staged\n").replace("n6\n", "N6\n");
    fx.write("a.txt", &edited);
    let repo = Repo::open(fx.path()).unwrap();
    let diff = repo.working_diff("a.txt", oxbow_core::Side::Unstaged, false).unwrap();
    let hunk = &diff.hunks[0];
    let err = repo
        .perform(&Action::StashHunk {
            path: "a.txt".into(),
            header: hunk.header.clone(),
            check: hunk.check.clone(),
            lines: None,
            message: None,
        })
        .unwrap_err();
    assert!(err.to_string().contains("next to staged changes"), "{err}");
    assert!(repo.stashes().unwrap().is_empty());
    assert_eq!(read(&fx, "a.txt"), edited);
    assert!(
        repo.operation_log().unwrap().is_empty(),
        "no failed step in the Operation Log"
    );
}
