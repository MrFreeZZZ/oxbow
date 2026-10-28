mod support;

use oxbow_core::{Action, ConflictSide, MergeMethod, OperationKind, Repo, ResetMode};
use support::Fixture;

fn read(fx: &Fixture, file: &str) -> String {
    std::fs::read_to_string(fx.path().join(file)).unwrap()
}

/// `feature` adds `b.txt` and changes `a.txt`; `main` changes `a.txt` the other way.
fn diverged() -> (Fixture, String, String) {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\ntwo\nthree\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    let add_b = fx.commit("b.txt", "b\n", "Add b");
    let shout = fx.commit("a.txt", "one\nTWO on feature\nthree\n", "Shout two");
    fx.git(&["switch", "-q", "main"]);
    fx.commit("a.txt", "one\n2 on main\nthree\n", "Number two");
    (fx, add_b, shout)
}

#[test]
fn cherry_pick_copies_a_commit_onto_the_branch() {
    let (mut fx, add_b, _) = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    let action = Action::CherryPick { commit: add_b.clone() };
    assert_eq!(
        repo.plan(&action).unwrap().commands[0].display(),
        format!("git cherry-pick {}", add_b)
    );
    repo.perform(&action).unwrap();
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Add b");
    assert_ne!(fx.git(&["rev-parse", "HEAD"]), add_b);
    assert_eq!(read(&fx, "b.txt"), "b\n");
}

#[test]
fn a_cherry_pick_that_conflicts_is_resolved_and_continued() {
    let (mut fx, _, shout) = diverged();
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::CherryPick { commit: shout }).unwrap_err();

    let op = repo.operation().unwrap().expect("a cherry-pick in progress");
    assert_eq!(op.kind, OperationKind::CherryPick);
    assert_eq!(op.conflicted, 1);
    assert_eq!(op.commit.unwrap().summary, "Shout two");

    repo.perform(&Action::TakeFile {
        path: "a.txt".into(),
        side: ConflictSide::Theirs,
    })
    .unwrap();
    repo.perform(&Action::Continue { message: None }).unwrap();
    assert!(repo.operation().unwrap().is_none());
    assert_eq!(read(&fx, "a.txt"), "one\nTWO on feature\nthree\n");
    // git's "# Conflicts:" notes stay out of the message.
    assert_eq!(fx.git(&["log", "-1", "--format=%B"]).trim(), "Shout two");
}

#[test]
fn revert_adds_a_commit_with_the_opposite_changes() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    let change = fx.commit("a.txt", "two\n", "Say two");
    let repo = Repo::open(fx.path()).unwrap();
    let action = Action::Revert { commit: change.clone() };
    assert_eq!(
        repo.plan(&action).unwrap().commands[0].display(),
        format!("git revert --no-edit {}", change)
    );
    repo.perform(&action).unwrap();
    assert_eq!(read(&fx, "a.txt"), "one\n");
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Revert \"Say two\"");
    assert_eq!(fx.git(&["rev-list", "--count", "HEAD"]), "3");
}

#[test]
fn a_merge_commit_is_reverted_against_its_first_parent() {
    let (mut fx, _, _) = diverged();
    fx.git(&["switch", "-q", "-c", "side", "main~1"]);
    fx.commit("c.txt", "c\n", "Add c");
    fx.git(&["switch", "-q", "main"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::Merge {
        branch: "side".into(),
        method: MergeMethod::Merge,
        message: None,
    })
    .unwrap();
    let merge = fx.git(&["rev-parse", "HEAD"]);

    let action = Action::Revert { commit: merge.clone() };
    assert_eq!(
        repo.plan(&action).unwrap().commands[0].display(),
        format!("git revert --no-edit -m 1 {}", merge)
    );
    repo.perform(&action).unwrap();
    assert!(!fx.path().join("c.txt").exists());
}

#[test]
fn undo_commit_keeps_its_changes_staged() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    fx.commit("a.txt", "two\n", "Say two");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::Reset {
        commit: "HEAD~1".into(),
        mode: ResetMode::Soft,
    })
    .unwrap();
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), root);
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "a.txt");
    assert_eq!(read(&fx, "a.txt"), "two\n");
}

#[test]
fn a_hard_reset_throws_away_commits_and_edits() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "one\n", "Root");
    fx.commit("a.txt", "two\n", "Say two");
    fx.write("a.txt", "three\n");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::Reset {
        commit: root.clone(),
        mode: ResetMode::Hard,
    })
    .unwrap();
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), root);
    assert_eq!(read(&fx, "a.txt"), "one\n");
    assert_eq!(fx.git(&["status", "--porcelain"]), "");
}

#[test]
fn reword_changes_only_the_message() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    fx.commit("a.txt", "two\n", "Sya two");
    fx.write("b.txt", "b\n");
    fx.git(&["add", "b.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    let action = Action::Reword {
        message: "Say two\n\nWith a body.".into(),
        no_verify: false,
    };
    assert_eq!(
        repo.plan(&action).unwrap().commands[0].display(),
        "git commit --amend --only -m 'Say two' -m 'With a body.'"
    );
    repo.perform(&action).unwrap();
    assert_eq!(fx.git(&["log", "-1", "--format=%B"]).trim(), "Say two\n\nWith a body.");
    assert_eq!(fx.git(&["show", "--name-only", "--format=", "HEAD"]), "a.txt");
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "b.txt");
}

fn undo_last(repo: &Repo) {
    let log = repo.operation_log().unwrap();
    repo.perform(&Action::Restore {
        id: log[0].id.clone(),
        after: false,
    })
    .unwrap();
}

const TEN: &str = "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n";

/// A commit that changes `a.txt` and `c.txt`, on top of a root with both.
fn two_files() -> (Fixture, String) {
    let mut fx = Fixture::new();
    fx.write("c.txt", "c\n");
    fx.commit("a.txt", TEN, "Root");
    fx.write("c.txt", "c changed\n");
    let both = fx.commit("a.txt", &TEN.replace("2\n", "two\n"), "Change both");
    (fx, both)
}

#[test]
fn revert_this_file_undoes_one_file_of_a_commit_in_the_files_on_disk() {
    let (mut fx, both) = two_files();
    let head = fx.git(&["rev-parse", "HEAD"]);
    let repo = Repo::open(fx.path()).unwrap();
    let action = Action::RevertFile {
        commit: both.clone(),
        path: "a.txt".into(),
        old_path: None,
    };
    let plan = repo.plan(&action).unwrap();
    assert_eq!(plan.commands[0].display(), "git apply --reverse -");
    assert!(plan.commands[0].input.as_deref().unwrap().contains("+two"));

    repo.perform(&action).unwrap();
    assert_eq!(read(&fx, "a.txt"), TEN);
    assert_eq!(read(&fx, "c.txt"), "c changed\n");
    // Nothing is committed or staged.
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(fx.git(&["diff", "--name-only"]), "a.txt");

    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, format!("Revert a.txt of {}", &both[..7]));
    undo_last(&repo);
    assert_eq!(read(&fx, "a.txt"), TEN.replace("2\n", "two\n"));
}

#[test]
fn a_file_changed_since_gets_the_revert_merged_in_and_staged() {
    let (mut fx, both) = two_files();
    // Line 5 is in the patch's context, so it no longer fits as it is.
    fx.commit(
        "a.txt",
        &TEN.replace("2\n", "two\n").replace("5\n", "five\n"),
        "Spell five",
    );
    let repo = Repo::open(fx.path()).unwrap();
    let action = Action::RevertFile {
        commit: both,
        path: "a.txt".into(),
        old_path: None,
    };
    assert_eq!(
        repo.plan(&action).unwrap().commands[0].display(),
        "git apply --reverse --3way -"
    );
    repo.perform(&action).unwrap();
    assert_eq!(read(&fx, "a.txt"), TEN.replace("5\n", "five\n"));
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "a.txt");
}

#[test]
fn a_file_revert_that_would_conflict_is_refused_before_anything_runs() {
    let (mut fx, both) = two_files();
    fx.commit("a.txt", &TEN.replace("2\n", "TWO\n"), "Shout two");
    let repo = Repo::open(fx.path()).unwrap();
    let action = Action::RevertFile {
        commit: both,
        path: "a.txt".into(),
        old_path: None,
    };
    let err = repo.plan(&action).unwrap_err().to_string();
    assert!(err.contains("conflicts with the file as it is now"), "{err}");
    assert!(err.contains("Revert the whole commit"), "{err}");
    let steps = repo.operation_log().unwrap().len();
    assert!(repo.perform(&action).is_err());
    assert_eq!(repo.operation_log().unwrap().len(), steps);
    assert_eq!(read(&fx, "a.txt"), TEN.replace("2\n", "TWO\n"));
    assert_eq!(fx.git(&["status", "--porcelain"]), "");
}

#[test]
fn a_merge_needs_the_file_on_disk_as_it_is_staged() {
    let (mut fx, both) = two_files();
    fx.commit(
        "a.txt",
        &TEN.replace("2\n", "two\n").replace("5\n", "five\n"),
        "Spell five",
    );
    fx.write(
        "a.txt",
        &TEN.replace("2\n", "two\n")
            .replace("5\n", "five\n")
            .replace("9\n", "nine\n"),
    );
    let repo = Repo::open(fx.path()).unwrap();
    let err = repo
        .plan(&Action::RevertFile {
            commit: both,
            path: "a.txt".into(),
            old_path: None,
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("stage or stash them first"), "{err}");
}

#[test]
fn cherry_pick_this_file_brings_one_file_of_another_branch_uncommitted() {
    let (mut fx, add_b, shout) = diverged();
    let head = fx.git(&["rev-parse", "HEAD"]);
    let repo = Repo::open(fx.path()).unwrap();
    let action = Action::CherryPickFile {
        commit: add_b.clone(),
        path: "b.txt".into(),
        old_path: None,
    };
    assert_eq!(repo.plan(&action).unwrap().commands[0].display(), "git apply -");
    repo.perform(&action).unwrap();
    assert_eq!(read(&fx, "b.txt"), "b\n");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), head);
    assert_eq!(fx.git(&["status", "--porcelain"]), "?? b.txt");
    assert_eq!(
        repo.operation_log().unwrap()[0].title,
        format!("Cherry-pick b.txt from {}", &add_b[..7])
    );

    // main changed the same line of a.txt: no conflicts are left behind.
    let err = repo
        .plan(&Action::CherryPickFile {
            commit: shout,
            path: "a.txt".into(),
            old_path: None,
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("Cherry-pick the whole commit"), "{err}");
}

#[test]
fn a_renamed_file_is_reverted_with_its_old_name() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", TEN, "Root");
    fx.git(&["mv", "a.txt", "b.txt"]);
    let moved = fx.commit("b.txt", &TEN.replace("10\n", "ten\n"), "Rename a to b");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::RevertFile {
        commit: moved,
        path: "b.txt".into(),
        old_path: Some("a.txt".into()),
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt"), TEN);
    assert!(!fx.path().join("b.txt").exists());
}

#[test]
fn a_file_of_the_root_commit_is_reverted_from_nothing() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "a\n", "Root");
    fx.commit("b.txt", "b\n", "Add b");
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::RevertFile {
        commit: root,
        path: "a.txt".into(),
        old_path: None,
    })
    .unwrap();
    assert!(!fx.path().join("a.txt").exists());
    assert_eq!(fx.git(&["status", "--porcelain"]), "D a.txt");
}

#[test]
fn take_this_version_puts_the_commits_file_on_disk_and_stages_nothing() {
    let (mut fx, _) = two_files();
    let root = fx.git(&["rev-parse", "HEAD~1"]);
    fx.write("a.txt", "my edit\n");
    let repo = Repo::open(fx.path()).unwrap();
    let action = Action::TakeVersion {
        commit: root.clone(),
        path: "a.txt".into(),
    };
    let shown = repo.plan(&action).unwrap().commands[0].display();
    assert!(
        shown.starts_with(&format!("git restore --source={root} --worktree -- ")),
        "{shown}"
    );
    repo.perform(&action).unwrap();
    assert_eq!(read(&fx, "a.txt"), TEN);
    assert_eq!(read(&fx, "c.txt"), "c changed\n");
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "");
    assert_eq!(
        repo.operation_log().unwrap()[0].title,
        format!("Take a.txt from {}", &root[..7])
    );
    // The edit it wrote over comes back with Undo.
    undo_last(&repo);
    assert_eq!(read(&fx, "a.txt"), "my edit\n");
}

#[test]
fn compare_with_working_copy_has_just_that_file() {
    let (fx, both) = two_files();
    fx.write("a.txt", &TEN.replace("2\n", "two\n").replace("7\n", "seven\n"));
    fx.write("c.txt", "c edited too\n");
    let repo = Repo::open(fx.path()).unwrap();
    let c = repo.compare_with_working_copy(&both, "a.txt").unwrap();
    assert_eq!(c.files.len(), 1);
    assert_eq!(c.files[0].file.path, "a.txt");
    assert_eq!(c.from, both);
    assert_eq!(c.command, format!("git diff {} -- a.txt", &both[..7]));
    let diff = repo
        .tree_diff_with(
            &c.from,
            &c.target_id,
            Some("a.txt"),
            oxbow_core::DiffContext::Compact,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    let added: Vec<&str> = diff[0].hunks[0]
        .lines
        .iter()
        .filter(|l| l.kind == oxbow_core::LineKind::Added)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(added, ["seven"]);
    // The index and the files are as they were.
    assert_eq!(read(&fx, "c.txt"), "c edited too\n");

    // A file deleted since leaves the other side.
    std::fs::remove_file(fx.path().join("a.txt")).unwrap();
    let c = repo.compare_with_working_copy(&both, "a.txt").unwrap();
    assert_eq!(c.files[0].file.status, oxbow_core::FileStatus::Deleted);
}
