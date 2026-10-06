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
        format!("git cherry-pick {}", &add_b[..7])
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
        format!("git revert --no-edit {}", &change[..7])
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
        format!("git revert --no-edit -m 1 {}", &merge[..7])
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
