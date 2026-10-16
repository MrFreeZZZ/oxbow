mod support;

use oxbow_core::{DiffContext, HistoryOptions, Moved, Repo, Side};
use support::Fixture;

/// A library with three commits, and an app that has it as the submodule `libs/lint` at the
/// third. Returns both and the library's commits, oldest first.
fn app_with_submodule() -> (Fixture, Fixture, Vec<String>) {
    let mut lib = Fixture::new();
    let ids = vec![
        lib.commit("rules.txt", "one\n", "First rule"),
        lib.commit("rules.txt", "two\n", "Second rule"),
        lib.commit("rules.txt", "three\n", "Third rule"),
    ];
    let mut app = Fixture::new();
    app.commit("app.rs", "fn main() {}\n", "Start");
    let url = lib.path().display().to_string();
    app.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        &url,
        "libs/lint",
    ]);
    app.git(&["commit", "-q", "-m", "Add lint"]);
    (app, lib, ids)
}

fn sub(fx: &mut Fixture, args: &[&str]) -> String {
    let mut all = vec!["-C", "libs/lint"];
    all.extend_from_slice(args);
    fx.git(&all)
}

#[test]
fn the_list_has_what_is_recorded_and_what_is_checked_out() {
    let (fx, lib, ids) = app_with_submodule();
    let repo = Repo::open(fx.path()).unwrap();
    let list = repo.submodules();
    assert_eq!(list.len(), 1);
    let lint = &list[0];
    assert_eq!(lint.path, "libs/lint");
    assert_eq!(lint.title, "lint");
    assert_eq!(lint.url.as_deref(), Some(lib.path().display().to_string().as_str()));
    assert_eq!(lint.recorded.as_deref(), Some(ids[2].as_str()));
    assert_eq!(lint.committed.as_deref(), Some(ids[2].as_str()));
    assert_eq!(lint.current.as_deref(), Some(ids[2].as_str()));
    assert_eq!(lint.branch.as_deref(), Some("main"));
    assert!(lint.cloned && !lint.out_of_sync());
    assert_eq!((lint.changed, lint.ahead, lint.behind), (0, 0, 0));

    let history = repo.history(&HistoryOptions::default()).unwrap();
    assert_eq!(history.submodules, list);
}

#[test]
fn a_folder_at_an_older_commit_is_out_of_sync() {
    let (mut fx, _lib, ids) = app_with_submodule();
    sub(&mut fx, &["checkout", "-q", &ids[0]]);
    let repo = Repo::open(fx.path()).unwrap();
    let lint = &repo.submodules()[0];
    assert!(lint.out_of_sync());
    assert_eq!(lint.current.as_deref(), Some(ids[0].as_str()));
    assert_eq!(lint.branch, None);
    assert_eq!((lint.ahead, lint.behind), (0, 2));

    // In Changes it is an unstaged move back over two commits.
    let tree = repo.working_tree().unwrap();
    let file = tree.unstaged.iter().find(|f| f.path == "libs/lint").unwrap();
    let flags = file.submodule.unwrap();
    assert!(flags.moved && !flags.changed);
    let diff = repo.working_diff("libs/lint", Side::Unstaged, false).unwrap();
    let moved = diff.submodule.unwrap();
    assert_eq!(moved.moved, Moved::Back);
    assert_eq!(moved.old.as_deref(), Some(ids[2].as_str()));
    assert_eq!(moved.new.as_deref(), Some(ids[0].as_str()));
    assert_eq!(moved.count, 2);
    let summaries: Vec<_> = moved.commits.iter().map(|c| c.summary.as_str()).collect();
    assert_eq!(summaries, ["Third rule", "Second rule"]);
}

#[test]
fn a_commit_that_moves_it_shows_the_commits_in_between() {
    let (mut fx, mut lib, ids) = app_with_submodule();
    let fourth = lib.commit("rules.txt", "four\n", "Fourth rule");
    sub(&mut fx, &["-c", "protocol.file.allow=always", "pull", "-q"]);
    fx.git(&["add", "libs/lint"]);

    // Staged: HEAD records the third, the index the fourth.
    let repo = Repo::open(fx.path()).unwrap();
    let staged = repo.working_diff("libs/lint", Side::Staged, false).unwrap();
    let moved = staged.submodule.unwrap();
    assert_eq!(moved.moved, Moved::Forward);
    assert_eq!(
        (moved.old.as_deref(), moved.new.as_deref()),
        (Some(ids[2].as_str()), Some(fourth.as_str()))
    );
    assert_eq!(moved.count, 1);

    fx.git(&["commit", "-q", "-m", "Update lint"]);
    let head = fx.git(&["rev-parse", "HEAD"]);
    let detail = repo.commit_detail(&head).unwrap();
    assert!(detail.files[0].submodule.is_some_and(|s| s.moved));
    let diffs = repo.commit_diff(&head, None, DiffContext::Compact).unwrap();
    let moved = diffs[0].submodule.as_ref().unwrap();
    assert_eq!(moved.moved, Moved::Forward);
    assert_eq!(moved.commits.len(), 1);
    assert_eq!(moved.commits[0].summary, "Fourth rule");
    assert!(moved.cloned);

    // The commit that added it.
    let added = fx.git(&["rev-parse", "HEAD~1"]);
    let diffs = repo
        .commit_diff(&added, Some("libs/lint"), DiffContext::Compact)
        .unwrap();
    assert_eq!(diffs[0].submodule.as_ref().unwrap().moved, Moved::Added);
}

#[test]
fn files_changed_inside_are_counted() {
    let (fx, _lib, _ids) = app_with_submodule();
    fx.write("libs/lint/rules.txt", "edited\n");
    fx.write("libs/lint/new.txt", "new\n");
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(repo.submodules()[0].changed, 2);

    let tree = repo.working_tree().unwrap();
    let file = tree.unstaged.iter().find(|f| f.path == "libs/lint").unwrap();
    let flags = file.submodule.unwrap();
    assert!(!flags.moved && flags.changed);
    let moved = repo
        .working_diff("libs/lint", Side::Unstaged, false)
        .unwrap()
        .submodule
        .unwrap();
    assert_eq!(moved.moved, Moved::Same);
    assert_eq!(moved.changed, 2);
}

#[test]
fn a_clone_without_submodules_has_them_not_cloned() {
    let (mut fx, _lib, ids) = app_with_submodule();
    let mut copy = Fixture::new();
    let target = copy.path().parent().unwrap().join("copy");
    copy.git(&[
        "clone",
        "-q",
        &fx.path().display().to_string(),
        &target.display().to_string(),
    ]);
    let repo = Repo::open(&target).unwrap();
    let list = repo.submodules();
    assert_eq!(list.len(), 1);
    assert!(!list[0].cloned && !list[0].out_of_sync());
    assert_eq!(list[0].current, None);
    assert_eq!(list[0].recorded.as_deref(), Some(ids[2].as_str()));

    // Without a clone its moves can't be read.
    let head = fx.git(&["rev-parse", "HEAD"]);
    let moved = repo
        .commit_diff(&head, Some("libs/lint"), DiffContext::Compact)
        .unwrap()
        .remove(0)
        .submodule
        .unwrap();
    assert_eq!(moved.moved, Moved::Added);
    assert!(!moved.cloned);
}
