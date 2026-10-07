mod support;

use oxbow_core::{Action, ConflictSide, OperationKind, Repo};
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
