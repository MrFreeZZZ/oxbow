mod support;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use oxbow_core::{Action, Error, FolderCache, FolderState, HistoryOptions, Repo, Worktree};
use support::Fixture;

/// `main` with two commits and a branch `fix/login` off the first.
fn repo_with_branch() -> Fixture {
    let mut fx = Fixture::new();
    fx.commit("app.rs", "one\n", "First");
    fx.git(&["branch", "fix/login"]);
    fx.commit("app.rs", "two\n", "Second");
    fx
}

/// A folder next to the repository's, like the sheet suggests.
fn sibling(fx: &Fixture, name: &str) -> String {
    fx.path().parent().unwrap().join(name).display().to_string()
}

fn shown(repo: &Repo, action: &Action) -> Vec<String> {
    repo.plan(action)
        .unwrap()
        .commands
        .iter()
        .map(|c| c.display())
        .collect()
}

#[test]
fn adding_a_worktree_checks_the_branch_out_in_a_new_folder() {
    let fx = repo_with_branch();
    let repo = Repo::open(fx.path()).unwrap();
    let path = sibling(&fx, "repo-fix-login");
    let add = Action::WorktreeAdd {
        path: path.clone(),
        branch: "fix/login".into(),
        create: false,
        start: None,
    };
    assert_eq!(shown(&repo, &add), ["git worktree add ../repo-fix-login fix/login"]);
    repo.perform(&add).unwrap();

    let list = repo.worktrees().unwrap();
    assert_eq!(list.len(), 2);
    assert!(list[0].main && list[0].current);
    assert_eq!(list[0].branch.as_deref(), Some("main"));
    assert_eq!(list[1].name, "repo-fix-login");
    assert_eq!(list[1].branch.as_deref(), Some("fix/login"));
    assert!(!list[1].main && !list[1].current && !list[1].missing);
    assert!(std::path::Path::new(&path).join("app.rs").is_file());

    // The history knows them too, and they are not in the Operation Log.
    let history = repo.history(&HistoryOptions::default()).unwrap();
    assert_eq!(history.worktrees.len(), 2);
    assert!(repo.operation_log().unwrap().is_empty());
}

#[test]
fn a_new_branch_is_made_where_asked() {
    let mut fx = repo_with_branch();
    let first = fx.git(&["rev-parse", "fix/login"]);
    let repo = Repo::open(fx.path()).unwrap();
    let add = Action::WorktreeAdd {
        path: sibling(&fx, "repo-spike"),
        branch: "spike/cache".into(),
        create: true,
        start: Some("fix/login".into()),
    };
    assert_eq!(
        shown(&repo, &add),
        ["git worktree add -b spike/cache ../repo-spike fix/login"]
    );
    repo.perform(&add).unwrap();
    assert_eq!(fx.git(&["rev-parse", "spike/cache"]), first);
    let list = repo.worktrees().unwrap();
    assert_eq!(list[1].branch.as_deref(), Some("spike/cache"));
    assert_eq!(list[1].head.as_deref(), Some(first.as_str()));
}

#[test]
fn the_other_folder_opens_as_a_repository_of_its_own() {
    let fx = repo_with_branch();
    let path = sibling(&fx, "repo-fix-login");
    let main = Repo::open(fx.path()).unwrap();
    main.perform(&Action::WorktreeAdd {
        path: path.clone(),
        branch: "fix/login".into(),
        create: false,
        start: None,
    })
    .unwrap();

    let linked = Repo::open(&path).unwrap();
    let history = linked.history(&HistoryOptions::default()).unwrap();
    assert_eq!(history.head.branch.as_deref(), Some("fix/login"));
    let here: Vec<_> = history.worktrees.iter().filter(|t| t.current).collect();
    assert_eq!(here.len(), 1);
    assert_eq!(here[0].name, "repo-fix-login");
    assert!(history.worktrees[0].main && !history.worktrees[0].current);
}

#[test]
fn a_status_that_fails_is_not_counted_as_no_changes() {
    let fx = repo_with_branch();
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&Action::WorktreeAdd {
        path: sibling(&fx, "repo-broken"),
        branch: "broken".into(),
        create: true,
        start: None,
    })
    .unwrap();
    // A damaged index: the folder is there, but `git status` can't read it.
    let index = fx.path().join(".git/worktrees/repo-broken/index");
    std::fs::write(&index, "not an index").unwrap();
    let list = repo.worktrees().unwrap();
    let broken = list.iter().find(|w| w.name == "repo-broken").unwrap();
    assert!(!broken.missing);
    assert_eq!(broken.changed, None);
    assert!(broken.status_error.as_deref().is_some_and(|e| e.contains("index")));
    let main = list.iter().find(|w| w.current).unwrap();
    assert_eq!(main.changed, Some(0));
    assert_eq!(main.status_error, None);
}

#[test]
fn removing_a_worktree_with_changes_forces_or_stashes_them() {
    let mut fx = repo_with_branch();
    let repo = Repo::open(fx.path()).unwrap();
    for name in ["repo-a", "repo-b"] {
        repo.perform(&Action::WorktreeAdd {
            path: sibling(&fx, name),
            branch: name.into(),
            create: true,
            start: None,
        })
        .unwrap();
        let folder = std::path::Path::new(&sibling(&fx, name)).to_path_buf();
        std::fs::write(folder.join("app.rs"), "edited\n").unwrap();
        std::fs::write(folder.join("notes.txt"), "new\n").unwrap();
    }
    let list = repo.worktrees().unwrap();
    assert_eq!(list[1].changed, Some(2));
    assert_eq!(list[2].changed, Some(2));

    // git refuses a plain remove while there are changes.
    let a = sibling(&fx, "repo-a");
    let plain = Action::WorktreeRemove {
        path: a.clone(),
        force: false,
        stash: false,
    };
    assert!(repo.perform(&plain).is_err());
    let forced = Action::WorktreeRemove {
        path: a.clone(),
        force: true,
        stash: false,
    };
    assert_eq!(shown(&repo, &forced), ["git worktree remove --force ../repo-a"]);
    repo.perform(&forced).unwrap();
    assert!(!std::path::Path::new(&a).exists());
    // The branch stays.
    assert!(!fx.git(&["rev-parse", "repo-a"]).is_empty());

    let b = sibling(&fx, "repo-b");
    let stashed = Action::WorktreeRemove {
        path: b.clone(),
        force: true,
        stash: true,
    };
    assert_eq!(
        shown(&repo, &stashed),
        [
            "git -C ../repo-b stash push --include-untracked -m 'repo-b before removing it'",
            "git worktree remove ../repo-b",
        ]
    );
    repo.perform(&stashed).unwrap();
    assert!(!std::path::Path::new(&b).exists());
    assert_eq!(
        fx.git(&["stash", "list", "--format=%s"]),
        "On repo-b: repo-b before removing it"
    );
    assert_eq!(repo.worktrees().unwrap().len(), 1);
}

#[test]
fn locked_missing_and_moved_worktrees() {
    let fx = repo_with_branch();
    let repo = Repo::open(fx.path()).unwrap();
    let path = sibling(&fx, "repo-usb");
    repo.perform(&Action::WorktreeAdd {
        path: path.clone(),
        branch: "fix/login".into(),
        create: false,
        start: None,
    })
    .unwrap();

    let lock = Action::WorktreeLock {
        path: path.clone(),
        reason: Some("on the USB drive".into()),
    };
    assert_eq!(
        shown(&repo, &lock),
        ["git worktree lock --reason 'on the USB drive' ../repo-usb"]
    );
    repo.perform(&lock).unwrap();
    assert_eq!(repo.worktrees().unwrap()[1].locked.as_deref(), Some("on the USB drive"));

    // Removing a locked one unlocks it first.
    let remove = Action::WorktreeRemove {
        path: path.clone(),
        force: false,
        stash: false,
    };
    assert_eq!(
        shown(&repo, &remove),
        ["git worktree unlock ../repo-usb", "git worktree remove ../repo-usb"]
    );

    repo.perform(&Action::WorktreeUnlock { path: path.clone() }).unwrap();
    assert_eq!(repo.worktrees().unwrap()[1].locked, None);

    // Moved by hand: missing, until git is told where it went.
    let moved = sibling(&fx, "repo-moved");
    std::fs::rename(&path, &moved).unwrap();
    assert!(repo.worktrees().unwrap()[1].missing);
    let repair = Action::WorktreeRepair { path: moved.clone() };
    assert_eq!(shown(&repo, &repair), ["git worktree repair ../repo-moved"]);
    repo.perform(&repair).unwrap();
    let list = repo.worktrees().unwrap();
    assert!(!list[1].missing);
    assert_eq!(list[1].name, "repo-moved");

    // Deleted: prune forgets it, and its branch can be checked out again.
    std::fs::remove_dir_all(&moved).unwrap();
    assert!(repo.worktrees().unwrap()[1].missing);
    assert_eq!(shown(&repo, &Action::WorktreePrune), ["git worktree prune --verbose"]);
    repo.perform(&Action::WorktreePrune).unwrap();
    assert_eq!(repo.worktrees().unwrap().len(), 1);
}

/// The states `folder_states` reads of `trees`, in the order they came.
fn states(repo: &Repo, trees: &[Worktree], cache: &FolderCache, since: Option<SystemTime>) -> Vec<FolderState> {
    let cancel = Arc::new(AtomicBool::new(false));
    let mut out = Vec::new();
    repo.folder_states(trees, &[], cache, since, 4, &cancel, &mut |s| out.push(s))
        .unwrap();
    out
}

/// A repository with a second worktree on `fix/login` at `../repo-fix-login`.
fn with_worktree() -> (Fixture, Repo, String) {
    let fx = repo_with_branch();
    let repo = Repo::open(fx.path()).unwrap();
    let path = sibling(&fx, "repo-fix-login");
    repo.perform(&Action::WorktreeAdd {
        path: path.clone(),
        branch: "fix/login".into(),
        create: false,
        start: None,
    })
    .unwrap();
    (fx, repo, path)
}

#[test]
fn the_history_lists_the_worktrees_and_their_changes_come_after() {
    let (_fx, repo, path) = with_worktree();
    std::fs::write(std::path::Path::new(&path).join("notes.txt"), "draft\n").unwrap();
    let history = repo.history(&HistoryOptions::default()).unwrap();
    let other = &history.worktrees[1];
    // Not counted yet, which is not the same as no changes.
    assert!(other.pending && other.changed.is_none() && other.status_error.is_none());
    assert!(!history.worktrees[0].pending);

    let read = states(&repo, &history.worktrees, &FolderCache::default(), None);
    assert_eq!(read.len(), 1);
    assert_eq!((read[0].path.as_str(), read[0].changed), (path.as_str(), Some(1)));
    let mut trees = history.worktrees.clone();
    trees[1].take_state(&read[0]);
    assert_eq!(trees, repo.worktrees().unwrap());
}

#[test]
fn a_folder_that_shows_no_change_is_not_read_again_unless_asked_afresh() {
    let (mut fx, repo, path) = with_worktree();
    let folder = std::path::Path::new(&path);
    std::fs::write(folder.join("one.txt"), "1\n").unwrap();
    let trees = repo.worktree_list().unwrap();
    let cache = FolderCache::default();
    assert_eq!(states(&repo, &trees, &cache, None)[0].changed, Some(1));

    // A new file moves neither HEAD nor the index: the kept state answers.
    std::fs::write(folder.join("two.txt"), "2\n").unwrap();
    assert_eq!(states(&repo, &trees, &cache, None)[0].changed, Some(1));
    // Asked afresh, as when the window comes back to the front: read again.
    assert_eq!(
        states(&repo, &trees, &cache, Some(SystemTime::now()))[0].changed,
        Some(2)
    );

    // Staging there changes its index, which is read again by itself.
    std::fs::write(folder.join("three.txt"), "3\n").unwrap();
    fx.git(&["-C", &path, "add", "three.txt"]);
    assert_eq!(states(&repo, &trees, &cache, None)[0].changed, Some(3));
}

#[test]
fn a_stopped_read_of_the_folders_answers_nothing() {
    let (_fx, repo, _path) = with_worktree();
    let trees = repo.worktree_list().unwrap();
    let cancel = Arc::new(AtomicBool::new(true));
    let mut got = Vec::new();
    let result = repo.folder_states(&trees, &[], &FolderCache::default(), None, 4, &cancel, &mut |s| {
        got.push(s)
    });
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(got.is_empty());
}

#[test]
fn a_stopped_read_runs_no_more_git() {
    let (_fx, repo, _path) = with_worktree();
    let stop = Arc::new(AtomicBool::new(true));
    let result = oxbow_core::cli::stoppable(&stop, || repo.run(&oxbow_core::GitCommand::new(["status"])));
    assert!(matches!(result, Err(Error::Cancelled)));
    let history = oxbow_core::cli::stoppable(&stop, || repo.history(&HistoryOptions::default()));
    assert!(matches!(history, Err(Error::Cancelled)));
}
