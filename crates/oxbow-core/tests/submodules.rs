mod support;

use std::sync::atomic::AtomicBool;

use oxbow_core::{
    Action, ActionEvent, DiffContext, Error, FailureKind, FolderCache, HistoryOptions, Moved, NotKept, PartState, Repo,
    Side, classify_failure,
};
use support::Fixture;

/// Let git clone from local paths, as `submodule update` refuses by default; the tests' clones
/// are all on disk.
fn allow_file_urls() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // SAFETY: set once, before any test of this file runs git through `Repo`.
        unsafe {
            std::env::set_var("GIT_CONFIG_COUNT", "1");
            std::env::set_var("GIT_CONFIG_KEY_0", "protocol.file.allow");
            std::env::set_var("GIT_CONFIG_VALUE_0", "always");
        }
    });
}

/// A library with three commits, and an app that has it as the submodule `libs/lint` at the
/// third. Returns both and the library's commits, oldest first.
fn app_with_submodule() -> (Fixture, Fixture, Vec<String>) {
    allow_file_urls();
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
    assert_eq!((lint.changed, lint.ahead, lint.behind), (Some(0), 0, 0));

    // The history lists it without looking into its folder, which is read after it.
    let history = repo.history(&HistoryOptions::default()).unwrap();
    assert!(history.submodules[0].pending && history.submodules[0].current.is_none());
    let mut read = history.submodules.clone();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let cache = FolderCache::default();
    let mut states = Vec::new();
    repo.folder_states(&[], &read, &cache, None, 4, &cancel, &mut |s| states.push(s))
        .unwrap();
    assert_eq!(states.len(), 1);
    read[0].take_state(&states[0]);
    assert_eq!(read, list);
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
    let summary = |c: Option<oxbow_core::CommitBrief>| c.map(|c| c.summary);
    assert_eq!(summary(moved.old_commit).as_deref(), Some("Third rule"));
    assert_eq!(summary(moved.new_commit).as_deref(), Some("First rule"));
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
    assert_eq!(repo.submodules()[0].changed, Some(2));

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
    assert_eq!(moved.changed, Some(2));
}

#[test]
fn a_folder_git_cant_read_is_not_counted_as_clean() {
    let (fx, _lib, _ids) = app_with_submodule();
    // The submodule's own index, damaged: its `git status` fails.
    std::fs::write(fx.path().join(".git/modules/libs/lint/index"), "not an index").unwrap();
    let repo = Repo::open(fx.path()).unwrap();
    let sub = &repo.read_submodules().unwrap()[0];
    assert!(sub.cloned);
    assert_eq!(sub.changed, None);
    assert!(sub.status_error.is_some());
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

fn shown(repo: &Repo, action: &Action) -> Vec<String> {
    repo.plan(action)
        .unwrap()
        .commands
        .iter()
        .map(|c| c.display())
        .collect()
}

#[test]
fn update_brings_the_folder_back_to_the_recorded_commit() {
    let (mut fx, _lib, ids) = app_with_submodule();
    sub(&mut fx, &["checkout", "-q", &ids[0]]);
    let repo = Repo::open(fx.path()).unwrap();
    let update = Action::SubmoduleUpdate {
        paths: vec!["libs/lint".into()],
        latest: false,
    };
    assert_eq!(
        shown(&repo, &update),
        ["git submodule update --init --recursive -- libs/lint"]
    );
    repo.perform(&update).unwrap();
    let lint = &repo.submodules()[0];
    assert!(!lint.out_of_sync());
    assert_eq!(lint.current.as_deref(), Some(ids[2].as_str()));
    // A submodule's folder is in no snapshot, so the step isn't in the Operation Log.
    assert!(repo.operation_log().unwrap().is_empty());
}

#[test]
fn update_clones_the_ones_that_are_not_cloned() {
    let (fx, _lib, ids) = app_with_submodule();
    let mut copy = Fixture::new();
    let target = copy.path().parent().unwrap().join("copy");
    copy.git(&[
        "clone",
        "-q",
        &fx.path().display().to_string(),
        &target.display().to_string(),
    ]);
    let repo = Repo::open(&target).unwrap();
    repo.perform(&Action::SubmoduleUpdate {
        paths: Vec::new(),
        latest: false,
    })
    .unwrap();
    let lint = &repo.submodules()[0];
    assert!(lint.cloned);
    assert_eq!(lint.current.as_deref(), Some(ids[2].as_str()));
}

#[test]
fn pull_latest_takes_the_newest_commit_of_its_branch() {
    let (fx, mut lib, ids) = app_with_submodule();
    let fourth = lib.commit("rules.txt", "four\n", "Fourth rule");
    let repo = Repo::open(fx.path()).unwrap();
    let latest = Action::SubmoduleUpdate {
        paths: vec!["libs/lint".into()],
        latest: true,
    };
    assert_eq!(
        shown(&repo, &latest),
        ["git submodule update --remote --recursive -- libs/lint"]
    );
    repo.perform(&latest).unwrap();
    let lint = &repo.submodules()[0];
    assert_eq!(lint.current.as_deref(), Some(fourth.as_str()));
    assert_eq!(lint.recorded.as_deref(), Some(ids[2].as_str()));
    assert_eq!((lint.ahead, lint.behind), (1, 0));
}

#[test]
fn a_submodule_is_added_and_removed() {
    let (fx, _lib, _ids) = app_with_submodule();
    let mut other = Fixture::new();
    other.commit("README", "other\n", "Other");
    let repo = Repo::open(fx.path()).unwrap();
    let url = other.path().display().to_string();
    let add = Action::SubmoduleAdd {
        url: url.clone(),
        path: "libs/other".into(),
        branch: Some("main".into()),
    };
    assert_eq!(
        shown(&repo, &add),
        [format!("git submodule add -b main -- {url} libs/other")]
    );
    repo.perform(&add).unwrap();
    let list = repo.submodules();
    assert_eq!(
        list.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(),
        ["libs/lint", "libs/other"]
    );
    assert_eq!(list[1].follows.as_deref(), Some("main"));
    assert!(list[1].cloned && list[1].committed.is_none());

    let remove = Action::SubmoduleRemove {
        path: "libs/lint".into(),
    };
    assert_eq!(
        shown(&repo, &remove),
        ["git submodule deinit -f -- libs/lint", "git rm -f -- libs/lint"]
    );
    repo.perform(&remove).unwrap();
    let list = repo.submodules();
    assert_eq!(list.iter().map(|s| s.path.as_str()).collect::<Vec<_>>(), ["libs/other"]);
    assert!(!fx.path().join("libs/lint").exists());
    let gitmodules = std::fs::read_to_string(fx.path().join(".gitmodules")).unwrap();
    assert!(!gitmodules.contains("libs/lint"));
}

#[test]
fn a_commit_only_this_clone_has_is_found_before_push_and_pushed_first() {
    allow_file_urls();
    // The library on a bare server, so its clone can push to it.
    let mut lib = Fixture::new();
    lib.commit("rules.txt", "one\n", "First rule");
    let server = lib.path().parent().unwrap().join("lint.git");
    lib.git(&["clone", "-q", "--bare", ".", &server.display().to_string()]);
    let mut app = Fixture::new();
    app.commit("app.rs", "fn main() {}\n", "Start");
    let app_server = app.path().parent().unwrap().join("app.git");
    app.git(&["init", "-q", "--bare", &app_server.display().to_string()]);
    app.git(&["remote", "add", "origin", &app_server.display().to_string()]);
    app.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        &server.display().to_string(),
        "libs/lint",
    ]);
    app.git(&["commit", "-q", "-m", "Add lint"]);
    app.git(&["push", "-q", "-u", "origin", "main"]);

    let repo = Repo::open(app.path()).unwrap();
    assert!(repo.unpushed_submodules().is_empty());

    // A new commit in the submodule, recorded here but not pushed there.
    app.write("libs/lint/rules.txt", "two\n");
    sub(&mut app, &["commit", "-q", "-am", "Second rule"]);
    let second = sub(&mut app, &["rev-parse", "HEAD"]);
    app.git(&["commit", "-q", "-am", "Update lint"]);
    let unpushed = repo.unpushed_submodules();
    let head = app.git(&["rev-parse", "HEAD"]);
    let moved = repo
        .commit_diff(&head, Some("libs/lint"), DiffContext::Compact)
        .unwrap()
        .remove(0);
    assert!(moved.submodule.unwrap().unpushed);
    assert_eq!(unpushed.len(), 1);
    assert_eq!(
        (unpushed[0].path.as_str(), unpushed[0].commit.as_str()),
        ("libs/lint", second.as_str())
    );

    let push = Action::Push {
        remote: "origin".into(),
        branch: "main".into(),
        upstream: "main".into(),
        set_upstream: false,
        force: false,
        lease: None,
        no_verify: false,
        submodules: true,
        merge_request: None,
    };
    assert_eq!(
        shown(&repo, &push),
        ["git push --recurse-submodules=on-demand origin main"]
    );
    repo.perform(&push).unwrap();
    assert!(repo.unpushed_submodules().is_empty());
    let on_server = std::process::Command::new("git")
        .args(["--git-dir", &server.display().to_string(), "rev-parse", "main"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&on_server.stdout).trim(), second);
}

#[test]
fn pull_can_bring_the_submodules_along() {
    let (fx, _lib, _ids) = app_with_submodule();
    let repo = Repo::open(fx.path()).unwrap();
    let pull = |submodules| Action::Pull {
        remote: "origin".into(),
        branch: "main".into(),
        fetch_first: false,
        submodules,
        mode: None,
    };
    let with = shown(&repo, &pull(true));
    assert_eq!(with.last().unwrap(), "git submodule update --init --recursive");
    assert_eq!(shown(&repo, &pull(false)).len(), with.len() - 1);
}

/// A server with the app before it had submodules, a clone of it, and a teammate's clone that
/// adds `libs/lint` and pushes.
fn first_submodule_on_the_server() -> (tempfile::TempDir, Fixture, Fixture, Fixture) {
    allow_file_urls();
    let server = tempfile::tempdir().unwrap();
    let url = server.path().join("app.git");
    let url = url.to_str().unwrap().to_owned();
    let mut lib = Fixture::new();
    lib.commit("rules.txt", "one\n", "First rule");
    let mut me = Fixture::new();
    me.commit("app.rs", "fn main() {}\n", "Start");
    me.git(&["init", "-q", "--bare", "-b", "main", &url]);
    me.git(&["remote", "add", "origin", &url]);
    me.git(&["push", "-q", "-u", "origin", "main"]);
    let mut mate = Fixture::new();
    mate.git(&["remote", "add", "origin", &url]);
    mate.git(&["fetch", "-q", "origin"]);
    mate.git(&["checkout", "-q", "-B", "main", "origin/main"]);
    let lib_url = lib.path().display().to_string();
    mate.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        &lib_url,
        "libs/lint",
    ]);
    mate.git(&["commit", "-q", "-m", "Add lint"]);
    mate.git(&["push", "-q", "origin", "main"]);
    (server, me, mate, lib)
}

#[test]
fn pull_that_brings_the_first_submodule_clones_it() {
    let pull = Action::Pull {
        remote: "origin".into(),
        branch: "main".into(),
        fetch_first: false,
        submodules: true,
        mode: None,
    };

    // Fetched before: the sheet already shows the update.
    let (_server, mut me, _mate, _lib) = first_submodule_on_the_server();
    me.git(&["fetch", "-q", "origin"]);
    let repo = Repo::open(me.path()).unwrap();
    assert_eq!(
        shown(&repo, &pull).last().unwrap(),
        "git submodule update --init --recursive"
    );
    repo.perform(&pull).unwrap();
    assert!(me.path().join("libs/lint/.git").exists());

    // Not fetched yet: the pull finds it and the update still runs.
    let (_server, me, _mate, _lib) = first_submodule_on_the_server();
    let repo = Repo::open(me.path()).unwrap();
    assert_eq!(shown(&repo, &pull).len(), 1);
    repo.perform(&pull).unwrap();
    assert!(me.path().join("libs/lint/.git").exists());
    assert!(repo.submodules()[0].cloned);
}

#[test]
fn removing_one_names_the_ignored_files_lost_with_its_folder() {
    let (mut fx, _lib, _ids) = app_with_submodule();
    let exclude = sub(
        &mut fx,
        &["rev-parse", "--path-format=absolute", "--git-path", "info/exclude"],
    );
    std::fs::create_dir_all(std::path::Path::new(&exclude).parent().unwrap()).unwrap();
    std::fs::write(&exclude, ".env\n").unwrap();
    std::fs::write(fx.path().join("libs/lint/.env"), "TOKEN=1\n").unwrap();
    // git status in the folder says nothing about it.
    assert_eq!(sub(&mut fx, &["status", "--porcelain"]), "");
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(repo.submodules()[0].changed, Some(0));

    let gaps = repo
        .not_kept(&Action::SubmoduleRemove {
            path: "libs/lint".into(),
        })
        .unwrap();
    let NotKept::Ignored { folder, files, .. } = &gaps[0] else {
        panic!("{gaps:?}")
    };
    assert_eq!(folder, "libs/lint");
    assert_eq!(files[0].path, ".env");
    assert_eq!(
        gaps[1],
        NotKept::SubmoduleFolder {
            folder: "libs/lint".into()
        }
    );
}

/// A library at `path` of `app`, with one commit, and one more commit made in its folder there
/// and recorded by `app`, which the library itself never got: a clone of `app` can't check it out.
fn add_unfetchable(app: &mut Fixture, path: &str) -> Fixture {
    let mut lib = Fixture::new();
    lib.commit("icons.svg", "<svg/>\n", "First icon");
    let url = lib.path().display().to_string();
    app.git(&["-c", "protocol.file.allow=always", "submodule", "add", &url, path]);
    app.write(&format!("{path}/icons.svg"), "<svg></svg>\n");
    app.git(&["-C", path, "commit", "-q", "-am", "Only here"]);
    app.git(&["add", path]);
    app.git(&["commit", "-q", "-m", &format!("Add {path}")]);
    lib
}

/// A fresh clone of `fx` beside it, without its submodules.
fn clone_of(fx: &Fixture, name: &str) -> std::path::PathBuf {
    let target = fx.path().parent().unwrap().join(name);
    let out = std::process::Command::new("git")
        .args([
            "clone",
            "-q",
            &fx.path().display().to_string(),
            &target.display().to_string(),
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    target
}

#[test]
fn update_all_goes_one_at_a_time_and_says_how_each_ended() {
    let (mut fx, _lib, ids) = app_with_submodule();
    let _icons = add_unfetchable(&mut fx, "libs/icons");
    let target = clone_of(&fx, "copy");
    let repo = Repo::open(&target).unwrap();
    let all = Action::SubmoduleUpdate {
        paths: Vec::new(),
        latest: false,
    };
    assert_eq!(
        shown(&repo, &all),
        [
            "git submodule update --init --recursive -- libs/lint",
            "git submodule update --init --recursive -- libs/icons",
        ]
    );

    let mut events = Vec::new();
    let err = repo
        .perform_with(&all, &mut |e| events.push(e), &AtomicBool::new(false))
        .unwrap_err();
    // The one that failed doesn't stop the other, and the answer says which one it was.
    let Error::PartlyDone { of, failed } = &err else {
        panic!("{err}")
    };
    assert_eq!(*of, 2);
    assert_eq!(failed.len(), 1);
    assert_eq!(
        (failed[0].name.as_str(), failed[0].inner.as_deref()),
        ("libs/icons", None)
    );
    assert_eq!(classify_failure(&err, false), FailureKind::PartlyDone);
    let parts: Vec<(String, PartState)> = events
        .iter()
        .filter_map(|e| match e {
            ActionEvent::Part { name, state, .. } => Some((name.clone(), *state)),
            _ => None,
        })
        .collect();
    assert_eq!(
        parts,
        [
            ("libs/lint".to_owned(), PartState::Started),
            ("libs/lint".to_owned(), PartState::Done),
            ("libs/icons".to_owned(), PartState::Started),
            ("libs/icons".to_owned(), PartState::Failed),
        ]
    );
    assert_eq!(
        events.first(),
        Some(&ActionEvent::Parts {
            names: vec!["libs/lint".into(), "libs/icons".into()]
        })
    );
    let list = repo.submodules();
    assert_eq!(list[0].current.as_deref(), Some(ids[2].as_str()));
}

#[test]
fn a_nested_submodule_that_fails_is_named_by_its_whole_path() {
    allow_file_urls();
    let mut lint = Fixture::new();
    lint.commit("rules.txt", "one\n", "First rule");
    let _deep = add_unfetchable(&mut lint, "vendor/deep");
    let mut app = Fixture::new();
    app.commit("app.rs", "fn main() {}\n", "Start");
    let url = lint.path().display().to_string();
    app.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "add",
        &url,
        "libs/lint",
    ]);
    app.git(&["commit", "-q", "-m", "Add lint"]);
    let target = clone_of(&app, "nested");
    let repo = Repo::open(&target).unwrap();
    let err = repo
        .perform(&Action::SubmoduleUpdate {
            paths: Vec::new(),
            latest: false,
        })
        .unwrap_err();
    let Error::PartlyDone { failed, .. } = &err else {
        panic!("{err}")
    };
    assert_eq!(failed[0].name, "libs/lint");
    assert_eq!(failed[0].inner.as_deref(), Some("libs/lint/vendor/deep"));
}

#[test]
fn commits_made_on_no_branch_in_the_folder_are_counted() {
    let (mut fx, _lib, ids) = app_with_submodule();
    sub(&mut fx, &["checkout", "-q", "--detach"]);
    fx.write("libs/lint/rules.txt", "four\n");
    sub(&mut fx, &["commit", "-q", "-am", "Fourth rule"]);
    let repo = Repo::open(fx.path()).unwrap();
    let lint = &repo.submodules()[0];
    assert_eq!(lint.branch, None);
    assert_eq!((lint.ahead, lint.on_no_branch), (1, 1));
    assert_eq!(lint.recorded.as_deref(), Some(ids[2].as_str()));

    // On a branch, the branch keeps it.
    sub(&mut fx, &["branch", "keep"]);
    assert_eq!(repo.submodules()[0].on_no_branch, 0);
}
