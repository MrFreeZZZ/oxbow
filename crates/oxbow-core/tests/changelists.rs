mod support;

use oxbow_core::changelists::{ChangelistEdit, Changelists, Moving};
use oxbow_core::{Action, Repo, Side};
use support::Fixture;

/// Twenty lines, so a change near the top and one near the bottom are hunks of their own.
fn lines() -> Vec<String> {
    (1..=20).map(|n| format!("line {n}")).collect()
}

fn text(lines: &[String]) -> String {
    lines.iter().map(|l| format!("{l}\n")).collect()
}

/// `lines()` with line 2 and line 18 changed: two runs, in two hunks.
fn both_changed() -> String {
    let mut l = lines();
    l[1] = "line 2 for login".into();
    l[17] = "line 18 for logging".into();
    text(&l)
}

fn setup() -> (Fixture, Repo) {
    let mut fx = Fixture::new();
    fx.write("a.txt", &text(&lines()));
    fx.write("b.txt", "b\n");
    fx.write("c.txt", "c\n");
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "Root"]);
    let repo = Repo::open(fx.path()).unwrap();
    (fx, repo)
}

fn edit(repo: &Repo, edit: ChangelistEdit) -> Changelists {
    repo.edit_changelists(&edit).unwrap()
}

fn create(repo: &Repo, name: &str, paths: &[&str]) -> String {
    let view = edit(
        repo,
        ChangelistEdit::Create {
            name: name.into(),
            moving: Some(Moving::Files {
                paths: paths.iter().map(|p| p.to_string()).collect(),
            }),
        },
    );
    view.lists.iter().find(|l| l.name == name.trim()).unwrap().id.clone()
}

fn read(repo: &Repo) -> Changelists {
    repo.changelists(&repo.working_tree().unwrap()).unwrap()
}

fn paths_of(view: &Changelists, id: &str) -> Vec<String> {
    view.lists.iter().find(|l| l.id == id).unwrap().paths.clone()
}

fn commit_list(id: &str, message: &str) -> Action {
    Action::Commit {
        message: message.into(),
        amend: false,
        no_verify: false,
        allow_secrets: false,
        skip_checks: false,
        sign_off: false,
        changelist: Some(id.into()),
    }
}

/// The keys of the runs of `path`'s unstaged changes, in order.
fn run_keys(repo: &Repo, path: &str) -> Vec<String> {
    let diff = repo.working_diff(path, Side::Unstaged, false).unwrap();
    diff.hunks.iter().flat_map(|h| h.runs.clone()).collect()
}

#[test]
fn no_lists_no_file() {
    let (fx, repo) = setup();
    fx.write("b.txt", "b2\n");
    assert_eq!(read(&repo), Changelists::default());
    assert!(!repo.has_changelists());
    let id = create(&repo, "Login fix", &["b.txt"]);
    assert!(fx.path().join(".git/oxbow/changelists.json").exists());
    assert!(repo.has_changelists());
    edit(&repo, ChangelistEdit::Delete { id });
    assert!(!fx.path().join(".git/oxbow/changelists.json").exists());
    assert!(!repo.has_changelists());
    assert_eq!(read(&repo), Changelists::default());
}

#[test]
fn lists_are_named_renamed_and_deleted() {
    let (fx, repo) = setup();
    fx.write("b.txt", "b2\n");
    fx.write("c.txt", "c2\n");
    let id = create(&repo, "  Login fix ", &["b.txt"]);
    let view = read(&repo);
    assert_eq!(view.lists[0].name, "Login fix");
    assert_eq!(paths_of(&view, &id), ["b.txt"]);

    // A name taken, by another list or by the panel's own lists, is refused.
    for name in ["login FIX", "Staged", "untracked", "  "] {
        let err = repo
            .edit_changelists(&ChangelistEdit::Create {
                name: name.into(),
                moving: None,
            })
            .unwrap_err();
        assert!(!err.to_string().is_empty());
    }
    edit(
        &repo,
        ChangelistEdit::Rename {
            id: id.clone(),
            name: "Login".into(),
        },
    );
    // Its own name again is fine.
    edit(
        &repo,
        ChangelistEdit::Rename {
            id: id.clone(),
            name: "login".into(),
        },
    );
    assert_eq!(read(&repo).lists[0].name, "login");

    // Deleted, its files go back to Unstaged.
    let view = edit(&repo, ChangelistEdit::Delete { id: id.clone() });
    assert!(view.lists.is_empty());
    let err = repo.edit_changelists(&ChangelistEdit::Delete { id }).unwrap_err();
    assert!(err.to_string().contains("gone"), "{err}");
}

#[test]
fn files_move_between_lists_and_leave_with_their_changes() {
    let (mut fx, repo) = setup();
    fx.write("b.txt", "b2\n");
    fx.write("c.txt", "c2\n");
    fx.write("new.txt", "new\n");
    let login = create(&repo, "Login", &["b.txt", "new.txt"]);
    let logging = create(&repo, "Logging", &[]);
    edit(
        &repo,
        ChangelistEdit::Move {
            moving: Moving::Part {
                paths: vec!["b.txt".into()],
                from: Some(login.clone()),
            },
            to: Some(logging.clone()),
        },
    );
    let view = read(&repo);
    assert_eq!(paths_of(&view, &login), ["new.txt"]);
    assert_eq!(paths_of(&view, &logging), ["b.txt"]);
    // A part a list doesn't hold doesn't move.
    edit(
        &repo,
        ChangelistEdit::Move {
            moving: Moving::Part {
                paths: vec!["c.txt".into()],
                from: Some(login.clone()),
            },
            to: Some(logging.clone()),
        },
    );
    assert_eq!(paths_of(&read(&repo), &logging), ["b.txt"]);

    // Staged whole, a file keeps its list; once it has no changes at all it leaves.
    fx.git(&["add", "b.txt"]);
    assert!(paths_of(&read(&repo), &logging).is_empty());
    fx.git(&["reset", "-q"]);
    assert_eq!(paths_of(&read(&repo), &logging), ["b.txt"]);
    fx.write("b.txt", "b\n");
    edit(&repo, ChangelistEdit::Activate { id: None });
    fx.write("b.txt", "b3\n");
    assert!(paths_of(&read(&repo), &logging).is_empty());
}

#[test]
fn runs_of_one_file_are_shared_between_lists() {
    let (fx, repo) = setup();
    fx.write("a.txt", &both_changed());
    let keys = run_keys(&repo, "a.txt");
    assert_eq!(keys.len(), 2);
    let login = create(&repo, "Login", &["a.txt"]);
    let view = edit(
        &repo,
        ChangelistEdit::Create {
            name: "Logging".into(),
            moving: Some(Moving::Runs {
                path: "a.txt".into(),
                keys: vec![keys[1].clone()],
            }),
        },
    );
    let logging = view.lists[1].id.clone();
    assert_eq!(paths_of(&view, &login), ["a.txt"]);
    assert_eq!(paths_of(&view, &logging), ["a.txt"]);
    assert_eq!(view.split.len(), 1);
    let parts = &view.split[0].parts;
    assert_eq!(parts[0].list.as_deref(), Some(login.as_str()));
    assert_eq!(parts[0].runs, [keys[0].clone()]);
    assert_eq!((parts[0].additions, parts[0].deletions), (1, 1));
    assert_eq!(parts[1].list.as_deref(), Some(logging.as_str()));

    // An edit elsewhere in the file leaves both runs where they are.
    let mut more: Vec<String> = both_changed().lines().map(str::to_owned).collect();
    more[9] = "line 10 edited".into();
    fx.write("a.txt", &text(&more));
    let view = read(&repo);
    assert_eq!(view.split[0].parts.len(), 2);
    assert_eq!(view.split[0].parts[1].runs, [keys[1].clone()]);
    // The new run is the file's own list's.
    assert_eq!(view.split[0].parts[0].runs.len(), 2);

    // Moved back, the file is in one list again, whole.
    let view = edit(
        &repo,
        ChangelistEdit::Move {
            moving: Moving::Part {
                paths: vec!["a.txt".into()],
                from: Some(logging.clone()),
            },
            to: Some(login.clone()),
        },
    );
    assert!(view.split.is_empty());
    assert!(paths_of(&view, &logging).is_empty());
    let stored = std::fs::read_to_string(fx.path().join(".git/oxbow/changelists.json")).unwrap();
    assert!(!stored.contains("\"runs\""), "{stored}");
}

#[test]
fn a_run_can_go_to_unstaged_while_its_file_is_in_a_list() {
    let (fx, repo) = setup();
    fx.write("a.txt", &both_changed());
    let keys = run_keys(&repo, "a.txt");
    let login = create(&repo, "Login", &["a.txt"]);
    let view = edit(
        &repo,
        ChangelistEdit::Move {
            moving: Moving::Runs {
                path: "a.txt".into(),
                keys: vec![keys[0].clone()],
            },
            to: None,
        },
    );
    let parts = &view.split[0].parts;
    assert_eq!(parts[0].list, None);
    assert_eq!(parts[1].list.as_deref(), Some(login.as_str()));
    // The Unstaged part moves into a list like any other.
    let view = edit(
        &repo,
        ChangelistEdit::Move {
            moving: Moving::Part {
                paths: vec!["a.txt".into()],
                from: None,
            },
            to: Some(login.clone()),
        },
    );
    assert!(view.split.is_empty());
    assert_eq!(paths_of(&view, &login), ["a.txt"]);
}

#[test]
fn the_same_text_twice_has_two_keys() {
    let (fx, repo) = setup();
    // The same line added in two places.
    let mut l = lines();
    l.insert(17, "same".into());
    l.insert(2, "same".into());
    fx.write("a.txt", &text(&l));
    let keys = run_keys(&repo, "a.txt");
    assert_eq!(keys.len(), 2);
    assert_ne!(keys[0], keys[1]);
    assert!(keys[1].ends_with("#2"), "{keys:?}");
}

#[test]
fn a_list_is_committed_alone() {
    let (mut fx, repo) = setup();
    fx.write("a.txt", &both_changed());
    fx.write("b.txt", "b2\n");
    fx.write("c.txt", "c2\n");
    fx.write("new.txt", "new\n");
    // c.txt is staged, for the next commit of the staged changes: it stays staged.
    fx.git(&["add", "c.txt"]);
    let keys = run_keys(&repo, "a.txt");
    let login = create(&repo, "Login", &["a.txt", "b.txt", "new.txt"]);
    edit(
        &repo,
        ChangelistEdit::Create {
            name: "Logging".into(),
            moving: Some(Moving::Runs {
                path: "a.txt".into(),
                keys: vec![keys[1].clone()],
            }),
        },
    );
    let action = commit_list(&login, "Fix the login");
    let plan = repo.plan(&action).unwrap();
    let shown: Vec<String> = plan.commands.iter().map(|c| c.display()).collect();
    assert_eq!(
        shown,
        [
            "git read-tree HEAD",
            "git add -A -- b.txt new.txt",
            "git apply --cached -",
            "git commit -m 'Fix the login'",
            "git reset -q -- b.txt new.txt a.txt",
        ]
    );
    assert!(plan.commands[0].comment.as_deref().unwrap().contains("GIT_INDEX_FILE"));
    repo.perform(&action).unwrap();

    let mut committed = lines();
    committed[1] = "line 2 for login".into();
    assert_eq!(fx.git(&["show", "HEAD:a.txt"]) + "\n", text(&committed));
    assert_eq!(fx.git(&["show", "HEAD:b.txt"]), "b2");
    assert_eq!(fx.git(&["show", "HEAD:new.txt"]), "new");
    assert_eq!(fx.git(&["show", "HEAD:c.txt"]), "c");
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Fix the login");
    // The staged file is still staged; the list's files are clean, but for Logging's change.
    let tree = repo.working_tree().unwrap();
    let staged: Vec<&str> = tree.staged.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(staged, ["c.txt"]);
    let unstaged: Vec<&str> = tree.unstaged.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(unstaged, ["a.txt"]);
    assert_eq!(
        std::fs::read_to_string(fx.path().join("a.txt")).unwrap(),
        both_changed()
    );
    // Its staging area is gone, and so is the list, now empty; Logging keeps its change.
    assert!(!fx.path().join(".git/oxbow/changelist-index").exists());
    let view = read(&repo);
    assert_eq!(view.lists.len(), 1);
    assert_eq!(view.lists[0].name, "Logging");
    assert_eq!(view.lists[0].paths, ["a.txt"]);
    assert!(view.split.is_empty());
    // The Operation Log names it.
    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, "Commit “Login” to main");
}

#[test]
fn the_active_list_stays_after_its_commit_and_takes_new_files() {
    let (fx, repo) = setup();
    fx.write("b.txt", "b2\n");
    let id = create(&repo, "Login", &[]);
    // What has changes already stays where it is.
    edit(&repo, ChangelistEdit::Activate { id: Some(id.clone()) });
    let view = read(&repo);
    assert_eq!(view.active.as_deref(), Some(id.as_str()));
    assert!(paths_of(&view, &id).is_empty());
    // A file that starts changing goes to it.
    fx.write("c.txt", "c2\n");
    fx.write("fresh.txt", "fresh\n");
    assert_eq!(paths_of(&read(&repo), &id), ["c.txt", "fresh.txt"]);
    // Moved out, it stays out.
    edit(
        &repo,
        ChangelistEdit::Move {
            moving: Moving::Files {
                paths: vec!["c.txt".into()],
            },
            to: None,
        },
    );
    assert_eq!(paths_of(&read(&repo), &id), ["fresh.txt"]);
    repo.perform(&commit_list(&id, "Fresh")).unwrap();
    let view = read(&repo);
    assert_eq!(view.lists.len(), 1);
    assert!(paths_of(&view, &id).is_empty());
    // Off again, new files stay in Unstaged.
    edit(&repo, ChangelistEdit::Activate { id: None });
    fx.write("fresh.txt", "fresher\n");
    assert!(paths_of(&read(&repo), &id).is_empty());
}

#[test]
fn a_list_stages_and_discards_only_its_part() {
    let (mut fx, repo) = setup();
    fx.write("a.txt", &both_changed());
    fx.write("b.txt", "b2\n");
    let keys = run_keys(&repo, "a.txt");
    let login = create(&repo, "Login", &["a.txt", "b.txt"]);
    edit(
        &repo,
        ChangelistEdit::Move {
            moving: Moving::Runs {
                path: "a.txt".into(),
                keys: vec![keys[1].clone()],
            },
            to: None,
        },
    );
    let stage = Action::StageChangelist {
        list: Some(login.clone()),
        paths: vec!["a.txt".into(), "b.txt".into()],
    };
    repo.perform(&stage).unwrap();
    let mut staged = lines();
    staged[1] = "line 2 for login".into();
    assert_eq!(fx.git(&["show", ":a.txt"]) + "\n", text(&staged));
    assert_eq!(fx.git(&["show", ":b.txt"]), "b2");
    fx.git(&["reset", "-q"]);

    // Discarding Unstaged's part of it leaves the list's.
    let discard = Action::DiscardChangelist {
        list: None,
        paths: vec!["a.txt".into()],
    };
    repo.perform(&discard).unwrap();
    assert_eq!(std::fs::read_to_string(fx.path().join("a.txt")).unwrap(), text(&staged));
    assert_eq!(std::fs::read_to_string(fx.path().join("b.txt")).unwrap(), "b2\n");
    // Nothing of it is left there now.
    let err = repo.plan(&discard).unwrap_err();
    assert!(err.to_string().contains("holds nothing"), "{err}");
}

#[test]
fn the_staged_lines_of_a_shared_file_go_with_it() {
    let (mut fx, repo) = setup();
    let mut l = lines();
    l[1] = "line 2 for login".into();
    l[9] = "line 10 staged".into();
    fx.write("a.txt", &text(&l));
    fx.git(&["add", "a.txt"]);
    let staged = l.clone();
    l[17] = "line 18 for logging".into();
    l[4] = "line 5 for login".into();
    fx.write("a.txt", &text(&l));
    let keys = run_keys(&repo, "a.txt");
    assert_eq!(keys.len(), 2);
    let login = create(&repo, "Login", &["a.txt"]);
    edit(
        &repo,
        ChangelistEdit::Move {
            moving: Moving::Runs {
                path: "a.txt".into(),
                keys: vec![keys[1].clone()],
            },
            to: None,
        },
    );
    let action = commit_list(&login, "Login");
    let plan = repo.plan(&action).unwrap();
    assert_eq!(plan.commands[1].display(), "git update-index -z --index-info");
    repo.perform(&action).unwrap();
    let mut committed = staged;
    committed[4] = "line 5 for login".into();
    assert_eq!(fx.git(&["show", "HEAD:a.txt"]) + "\n", text(&committed));
    // Logging's line is all that is left, unstaged.
    let tree = repo.working_tree().unwrap();
    assert!(tree.staged.is_empty());
    assert_eq!(tree.unstaged.len(), 1);
}

#[test]
fn a_list_commits_in_a_repository_without_commits() {
    let mut fx = Fixture::new();
    fx.write("first.txt", "first\n");
    fx.write("other.txt", "other\n");
    let repo = Repo::open(fx.path()).unwrap();
    let id = create(&repo, "First", &["first.txt"]);
    let plan = repo.plan(&commit_list(&id, "First")).unwrap();
    assert_eq!(plan.commands[0].display(), "git read-tree --empty");
    // As an assertion, so a failure shows git's own words.
    assert_eq!(
        repo.perform(&commit_list(&id, "First")).err().map(|e| e.to_string()),
        None
    );
    assert_eq!(fx.git(&["ls-tree", "--name-only", "HEAD"]), "first.txt");
    let tree = repo.working_tree().unwrap();
    assert!(tree.staged.is_empty());
    assert_eq!(tree.unstaged[0].path, "other.txt");
}

#[test]
fn an_empty_or_deleted_list_is_not_committed() {
    let (fx, repo) = setup();
    fx.write("b.txt", "b2\n");
    let id = create(&repo, "Login", &[]);
    let err = repo.plan(&commit_list(&id, "Nothing")).unwrap_err();
    assert!(err.to_string().contains("no changes"), "{err}");
    edit(&repo, ChangelistEdit::Delete { id: id.clone() });
    let err = repo.plan(&commit_list(&id, "Nothing")).unwrap_err();
    assert!(err.to_string().contains("gone"), "{err}");
}

#[test]
fn the_secrets_check_reads_the_list_not_the_staged_files() {
    let (mut fx, repo) = setup();
    let key = concat!("AKIA", "Q7XKZ3M4P2LJ6WVB");
    // A secret staged, outside the list, doesn't stop the list's commit.
    fx.write("c.txt", &format!("aws = \"{key}\"\n"));
    fx.git(&["add", "c.txt"]);
    fx.write("b.txt", "b2\n");
    let id = create(&repo, "Login", &["b.txt"]);
    repo.perform(&commit_list(&id, "B")).unwrap();
    // One in the list does, before anything runs.
    fx.write("b.txt", &format!("aws = \"{key}\"\n"));
    let id = create(&repo, "Keys", &["b.txt"]);
    let head = fx.git(&["rev-parse", "HEAD"]);
    let err = repo.perform(&commit_list(&id, "Keys")).unwrap_err();
    assert!(matches!(err, oxbow_core::error::Error::Secrets(_)), "{err}");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), head);
    assert!(!fx.path().join(".git/oxbow/changelist-check-index").exists());
    assert_eq!(read(&repo).lists.iter().filter(|l| l.id == id).count(), 1);
}

#[test]
fn a_hook_that_stops_the_commit_leaves_everything_as_it_was() {
    let (mut fx, repo) = setup();
    fx.write("b.txt", "b2\n");
    fx.write("new.txt", "new\n");
    let id = create(&repo, "Login", &["b.txt", "new.txt"]);
    let hook = fx.path().join(".git/hooks/pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho no >&2\nexit 1\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let head = fx.git(&["rev-parse", "HEAD"]);
    assert!(repo.perform(&commit_list(&id, "Login")).is_err());
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), head);
    let tree = repo.working_tree().unwrap();
    assert!(tree.staged.is_empty());
    assert!(!fx.path().join(".git/oxbow/changelist-index").exists());
    assert_eq!(paths_of(&read(&repo), &id), ["b.txt", "new.txt"]);
}

#[test]
fn amend_adds_the_list_to_the_last_commit() {
    let (mut fx, repo) = setup();
    fx.write("b.txt", "b2\n");
    fx.write("c.txt", "c2\n");
    let id = create(&repo, "Login", &["b.txt"]);
    let mut action = commit_list(&id, "Root again");
    if let Action::Commit { amend, .. } = &mut action {
        *amend = true;
    }
    repo.perform(&action).unwrap();
    assert_eq!(fx.git(&["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(fx.git(&["show", "HEAD:b.txt"]), "b2");
    assert_eq!(fx.git(&["show", "HEAD:c.txt"]), "c");
    let tree = repo.working_tree().unwrap();
    assert!(tree.staged.is_empty());
    assert_eq!(tree.unstaged[0].path, "c.txt");
}

#[test]
fn unstaged_hunks_name_their_runs() {
    let (fx, repo) = setup();
    fx.write("a.txt", &both_changed());
    let diff = repo.working_diff("a.txt", Side::Unstaged, false).unwrap();
    assert_eq!(diff.hunks.len(), 2);
    assert!(diff.hunks.iter().all(|h| h.runs.len() == 1));
    // The keys don't depend on how much context the diff shows.
    let whole = repo.working_diff("a.txt", Side::Unstaged, true).unwrap();
    let keys: Vec<String> = whole.hunks.iter().flat_map(|h| h.runs.clone()).collect();
    assert_eq!(keys, run_keys(&repo, "a.txt"));
}

#[test]
fn runs_of_one_hunk_go_to_different_lists() {
    let (mut fx, repo) = setup();
    let mut l = lines();
    l[1] = "line 2 for login".into();
    l[5] = "line 6 for logging".into();
    fx.write("a.txt", &text(&l));
    let diff = repo.working_diff("a.txt", Side::Unstaged, false).unwrap();
    assert_eq!(diff.hunks.len(), 1);
    let keys = diff.hunks[0].runs.clone();
    assert_eq!(keys.len(), 2);
    let login = create(&repo, "Login", &["a.txt"]);
    edit(
        &repo,
        ChangelistEdit::Create {
            name: "Logging".into(),
            moving: Some(Moving::Runs {
                path: "a.txt".into(),
                keys: vec![keys[1].clone()],
            }),
        },
    );
    repo.perform(&commit_list(&login, "Login")).unwrap();
    let mut committed = lines();
    committed[1] = "line 2 for login".into();
    assert_eq!(fx.git(&["show", "HEAD:a.txt"]) + "\n", text(&committed));
    assert_eq!(std::fs::read_to_string(fx.path().join("a.txt")).unwrap(), text(&l));
    let view = read(&repo);
    assert_eq!(view.lists[0].name, "Logging");
    assert_eq!(view.lists[0].paths, ["a.txt"]);
}
