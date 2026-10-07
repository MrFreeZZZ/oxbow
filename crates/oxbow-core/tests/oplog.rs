mod support;

use oxbow_core::{Action, Repo, ResetMode};
use support::Fixture;

fn read(fx: &Fixture, file: &str) -> Option<String> {
    std::fs::read_to_string(fx.path().join(file)).ok()
}

fn undo_last(repo: &Repo) {
    let log = repo.operation_log().unwrap();
    repo.perform(&Action::Restore {
        id: log[0].id.clone(),
        after: false,
    })
    .unwrap();
}

#[test]
fn a_discard_is_undone_with_untracked_files_and_staged_changes() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "a staged\n");
    fx.git(&["add", "a.txt"]);
    fx.write("a.txt", "a staged\nand more\n");
    fx.write("notes/new.txt", "new\n");
    let repo = Repo::open(fx.path()).unwrap();

    repo.perform(&Action::Discard {
        paths: vec!["a.txt".into(), "notes/new.txt".into()],
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt").unwrap(), "a staged\n");
    assert!(read(&fx, "notes/new.txt").is_none());

    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, "Discard changes in 2 files");
    assert_eq!(log[0].kind, "discard");
    let plan = repo
        .plan(&Action::Restore {
            id: log[0].id.clone(),
            after: false,
        })
        .unwrap();
    let shown: Vec<_> = plan.commands.iter().map(|c| c.display()).collect();
    assert!(shown.iter().any(|c| c.starts_with("git read-tree ")), "{shown:?}");
    assert!(shown.last().unwrap().starts_with("git restore --source="), "{shown:?}");

    undo_last(&repo);
    assert_eq!(read(&fx, "a.txt").unwrap(), "a staged\nand more\n");
    assert_eq!(read(&fx, "notes/new.txt").unwrap(), "new\n");
    // The staged version is staged again.
    assert_eq!(fx.git(&["show", ":a.txt"]), "a staged");
    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, "Undo “Discard changes in 2 files”");
}

#[test]
fn a_commit_and_a_hard_reset_are_undone_and_redone() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let root = fx.git(&["rev-parse", "HEAD"]);
    fx.write("a.txt", "b\n");
    fx.git(&["add", "a.txt"]);
    let repo = Repo::open(fx.path()).unwrap();

    repo.perform(&Action::Commit {
        message: "Change a".into(),
        amend: false,
        no_verify: false,
    })
    .unwrap();
    let commit = fx.git(&["rev-parse", "HEAD"]);
    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, "Commit to main");
    assert_eq!(log[0].detail, "Change a");

    // Undo the commit: main goes back and the change is staged again.
    undo_last(&repo);
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), root);
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "a.txt");

    // Undo the undo: the commit is back.
    undo_last(&repo);
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), commit);
    assert_eq!(fx.git(&["status", "--porcelain"]), "");

    // A hard reset with uncommitted work, undone.
    fx.write("a.txt", "work in progress\n");
    repo.perform(&Action::Reset {
        commit: root.clone(),
        mode: ResetMode::Hard,
    })
    .unwrap();
    assert_eq!(read(&fx, "a.txt").unwrap(), "a\n");
    undo_last(&repo);
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), commit);
    assert_eq!(read(&fx, "a.txt").unwrap(), "work in progress\n");
}

#[test]
fn restore_to_here_goes_back_several_steps() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    let dir = fx.path().to_path_buf();
    let commit = |text: &str| {
        std::fs::write(dir.join("a.txt"), text).unwrap();
        repo.perform(&Action::Stage {
            paths: vec!["a.txt".into()],
        })
        .unwrap();
        repo.perform(&Action::Commit {
            message: text.trim().into(),
            amend: false,
            no_verify: false,
        })
        .unwrap();
    };
    commit("one\n");
    let one = fx.git(&["rev-parse", "HEAD"]);
    commit("two\n");
    commit("three\n");
    let log = repo.operation_log().unwrap();
    assert_eq!(log.len(), 6);
    // Right after the commit of "one".
    let first_commit = &log[4];
    assert_eq!(first_commit.detail, "one");
    repo.perform(&Action::Restore {
        id: first_commit.id.clone(),
        after: true,
    })
    .unwrap();
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), one);
    assert_eq!(read(&fx, "a.txt").unwrap(), "one\n");
    assert_eq!(fx.git(&["status", "--porcelain"]), "");
}

#[test]
fn deleted_branches_tags_and_stashes_come_back() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.git(&["branch", "topic"]);
    fx.git(&["tag", "-a", "v1", "-m", "Version one"]);
    let tag = fx.git(&["rev-parse", "v1"]);
    fx.write("a.txt", "stashed\n");
    fx.git(&["stash", "push", "-m", "keep me"]);
    let repo = Repo::open(fx.path()).unwrap();

    repo.perform(&Action::DeleteBranch {
        name: "topic".into(),
        force: false,
        upstream: None,
    })
    .unwrap();
    repo.perform(&Action::DeleteTag {
        name: "v1".into(),
        remote: None,
    })
    .unwrap();
    let stash = repo.stashes().unwrap()[0].id.clone();
    repo.perform(&Action::StashDrop { index: 0, id: stash }).unwrap();
    assert!(repo.stashes().unwrap().is_empty());

    let log = repo.operation_log().unwrap();
    assert_eq!(log[2].title, "Delete branch topic");
    // Restoring to before the first step undoes all three.
    repo.perform(&Action::Restore {
        id: log[2].id.clone(),
        after: false,
    })
    .unwrap();
    assert_eq!(fx.git(&["branch", "--list", "topic"]), "topic");
    assert_eq!(fx.git(&["rev-parse", "v1"]), tag);
    assert_eq!(fx.git(&["stash", "list"]), "stash@{0}: On main: keep me");
}

#[test]
fn the_log_adds_nothing_to_the_history_and_skips_steps_that_changed_nothing() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.write("a.txt", "b\n");
    fx.git(&["add", "a.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    // Staging what is already staged changes nothing.
    repo.perform(&Action::Stage {
        paths: vec!["a.txt".into()],
    })
    .unwrap();
    assert!(repo.operation_log().unwrap().is_empty());

    repo.perform(&Action::Discard {
        paths: vec!["a.txt".into()],
    })
    .ok();
    repo.perform(&Action::Unstage {
        paths: vec!["a.txt".into()],
    })
    .unwrap();
    assert_eq!(repo.operation_log().unwrap().len(), 1);
    assert_eq!(fx.git(&["log", "--all", "--format=%s"]), "Root");
    assert_eq!(
        fx.git(&["rev-parse", "refs/oxbow/oplog"]),
        fx.git(&["rev-parse", "HEAD"])
    );

    repo.perform(&Action::ClearOperationLog).unwrap();
    assert!(repo.operation_log().unwrap().is_empty());
}
