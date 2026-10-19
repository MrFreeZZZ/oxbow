mod support;

use oxbow_core::{Action, Error, Fix, FixStep, Repo, ResetMode, StepKind};
use support::Fixture;

fn read(fx: &Fixture, file: &str) -> Option<String> {
    std::fs::read_to_string(fx.path().join(file)).ok()
}

/// The action a sheet opened now would run.
fn fix(repo: &Repo, fix: Fix) -> Action {
    Action::Fix {
        fix,
        expect: repo.fix_state().unwrap().expect,
    }
}

fn kinds(steps: &[FixStep]) -> Vec<&'static str> {
    steps
        .iter()
        .map(|s| match s.what {
            StepKind::Stash { .. } => "stash",
            StepKind::CreateBranch { .. } => "create",
            StepKind::MoveBranch { .. } => "move",
            StepKind::Switch { .. } => "switch",
            StepKind::Copy { .. } => "copy",
            StepKind::Revert { .. } => "revert",
            StepKind::Stage { .. } => "stage",
            StepKind::Amend => "amend",
            StepKind::Reset { .. } => "reset",
            StepKind::Push { .. } => "push",
            StepKind::Pop => "pop",
        })
        .collect()
}

fn displays(repo: &Repo, action: &Action) -> Vec<String> {
    repo.plan(action)
        .unwrap()
        .commands
        .iter()
        .map(|c| c.display())
        .collect()
}

/// A bare repository standing in for GitHub, with `main` pushed there.
struct Remote {
    _dir: tempfile::TempDir,
}

fn with_remote(fx: &mut Fixture) -> Remote {
    let dir = tempfile::tempdir().unwrap();
    let url = dir.path().join("origin.git");
    let status = std::process::Command::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(&url)
        .status()
        .unwrap();
    assert!(status.success());
    fx.git(&["remote", "add", "origin", url.to_str().unwrap()]);
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    Remote { _dir: dir }
}

#[test]
fn unpushed_commits_move_to_a_new_branch_without_touching_the_files() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "a\n", "Root");
    let one = fx.commit("b.txt", "b\n", "Add b");
    let two = fx.commit("b.txt", "b2\n", "Change b");
    fx.write("b.txt", "b3 not committed\n");
    let repo = Repo::open(fx.path()).unwrap();

    let action = fix(
        &repo,
        Fix::MoveCommits {
            count: 2,
            to: "feature/b".into(),
            create: true,
        },
    );
    let plan = repo.plan(&action).unwrap();
    assert_eq!(kinds(&plan.steps), ["create", "move"]);
    assert_eq!(
        displays(&repo, &action),
        [
            "git switch -c feature/b".to_owned(),
            format!("git branch -f main {}", &root[..7])
        ]
    );
    repo.perform(&action).unwrap();

    assert_eq!(fx.git(&["branch", "--show-current"]), "feature/b");
    assert_eq!(fx.git(&["rev-parse", "feature/b"]), two);
    assert_eq!(fx.git(&["rev-parse", "main"]), root);
    assert_eq!(fx.git(&["rev-parse", "feature/b~1"]), one);
    assert_eq!(read(&fx, "b.txt").unwrap(), "b3 not committed\n");

    // One step in the Operation Log, so one Undo takes it all back.
    let log = repo.operation_log().unwrap();
    assert_eq!(log[0].title, "Move 2 commits from main to feature/b");
    repo.perform(&Action::Restore {
        id: log[0].id.clone(),
        after: false,
    })
    .unwrap();
    assert_eq!(fx.git(&["branch", "--show-current"]), "main");
    assert_eq!(fx.git(&["rev-parse", "main"]), two);
}

#[test]
fn moving_to_an_existing_branch_copies_and_puts_files_in_the_way_aside() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "a\n", "Root");
    fx.git(&["branch", "feature"]);
    fx.git(&["switch", "-q", "feature"]);
    fx.commit("f.txt", "feature\n", "Feature work");
    fx.git(&["switch", "-q", "main"]);
    fx.commit("a.txt", "a\nmore\n", "Meant for feature");
    // An edit to a file the moved commit changes is in the way of the cherry-pick.
    fx.write("a.txt", "a\nmore\nand edits\n");
    fx.write("notes.txt", "mine\n");
    let repo = Repo::open(fx.path()).unwrap();

    let action = fix(
        &repo,
        Fix::MoveCommits {
            count: 1,
            to: "feature".into(),
            create: false,
        },
    );
    let plan = repo.plan(&action).unwrap();
    assert_eq!(kinds(&plan.steps), ["stash", "switch", "copy", "move", "pop"]);
    repo.perform(&action).unwrap();

    assert_eq!(fx.git(&["branch", "--show-current"]), "feature");
    assert_eq!(fx.git(&["log", "-2", "--format=%s"]), "Meant for feature\nFeature work");
    assert_eq!(fx.git(&["rev-parse", "main"]), root);
    assert_eq!(read(&fx, "a.txt").unwrap(), "a\nmore\nand edits\n");
    assert_eq!(read(&fx, "notes.txt").unwrap(), "mine\n");
    assert_eq!(fx.git(&["stash", "list"]), "");
}

#[test]
fn files_out_of_the_way_are_not_stashed() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.git(&["branch", "feature"]);
    fx.commit("b.txt", "b\n", "Add b");
    fx.write("notes.txt", "mine\n");
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(
        &repo,
        Fix::MoveCommits {
            count: 1,
            to: "feature".into(),
            create: false,
        },
    );
    assert_eq!(kinds(&repo.plan(&action).unwrap().steps), ["switch", "copy", "move"]);
}

#[test]
fn pushed_commits_are_reverted_on_the_branch_and_copied_to_the_new_one() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.commit("b.txt", "b\n", "Add b");
    let _remote = with_remote(&mut fx);
    let pushed = fx.git(&["rev-parse", "HEAD"]);
    let repo = Repo::open(fx.path()).unwrap();

    let action = fix(
        &repo,
        Fix::MoveCommits {
            count: 1,
            to: "feature/b".into(),
            create: true,
        },
    );
    let plan = repo.plan(&action).unwrap();
    assert_eq!(kinds(&plan.steps), ["revert", "create", "copy"]);
    repo.perform(&action).unwrap();

    // main keeps its pushed history and gets a revert on top: a plain push, no force.
    assert_eq!(fx.git(&["rev-parse", "main~1"]), pushed);
    assert_eq!(fx.git(&["log", "-1", "--format=%s", "main"]), "Revert \"Add b\"");
    assert_eq!(fx.git(&["branch", "--show-current"]), "feature/b");
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Add b");
    assert_eq!(read(&fx, "b.txt").unwrap(), "b\n");
}

#[test]
fn a_forgotten_file_goes_into_the_last_commit_with_its_message() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.commit("a.txt", "a2\n", "Change a");
    fx.write("forgotten.txt", "f\n");
    fx.write("other.txt", "o\n");
    let repo = Repo::open(fx.path()).unwrap();

    let state = repo.fix_state().unwrap();
    assert_eq!(
        state.changed.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["forgotten.txt", "other.txt"]
    );
    let action = fix(
        &repo,
        Fix::AddToLast {
            paths: vec!["forgotten.txt".into()],
            allow_secrets: false,
        },
    );
    assert_eq!(
        displays(&repo, &action),
        [
            "git add -- forgotten.txt",
            "git commit --amend --no-edit --only -- forgotten.txt"
        ]
    );
    repo.perform(&action).unwrap();

    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Change a");
    assert_eq!(fx.git(&["rev-list", "--count", "HEAD"]), "2");
    assert_eq!(
        fx.git(&["show", "--format=", "--name-only", "HEAD"]),
        "a.txt\nforgotten.txt"
    );
    assert_eq!(fx.git(&["status", "--porcelain"]), "?? other.txt");
}

#[test]
fn undo_last_keeps_the_changes_staged() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "a\n", "Root");
    fx.commit("a.txt", "a2\n", "Change a");
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(&repo, Fix::UndoLast { mode: ResetMode::Soft });
    assert_eq!(displays(&repo, &action), ["git reset --soft HEAD~1"]);
    repo.perform(&action).unwrap();
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), root);
    assert_eq!(fx.git(&["status", "--porcelain"]), "M  a.txt");

    // The first commit has nothing before it.
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.plan(&fix(&repo, Fix::UndoLast { mode: ResetMode::Soft })).is_err());
}

#[test]
fn a_pushed_commit_is_reverted_and_pushed() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let bad = fx.commit("a.txt", "bad\n", "Break a");
    let _remote = with_remote(&mut fx);
    let repo = Repo::open(fx.path()).unwrap();

    let action = fix(
        &repo,
        Fix::RevertPushed {
            commit: bad.clone(),
            push: true,
        },
    );
    let plan = repo.plan(&action).unwrap();
    assert_eq!(kinds(&plan.steps), ["revert", "push"]);
    repo.perform(&action).unwrap();
    assert_eq!(read(&fx, "a.txt").unwrap(), "a\n");
    assert_eq!(fx.git(&["rev-parse", "origin/main"]), fx.git(&["rev-parse", "HEAD"]));
}

#[test]
fn a_revert_puts_edits_to_the_same_file_aside_and_back() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\ntwo\nthree\nfour\nfive\n", "Root");
    let bad = fx.commit("a.txt", "one\nTWO\nthree\nfour\nfive\n", "Shout two");
    fx.write("a.txt", "one\nTWO\nthree\nfour\nFIVE\n");
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(
        &repo,
        Fix::RevertPushed {
            commit: bad,
            push: false,
        },
    );
    assert_eq!(kinds(&repo.plan(&action).unwrap().steps), ["stash", "revert", "pop"]);
    repo.perform(&action).unwrap();
    assert_eq!(read(&fx, "a.txt").unwrap(), "one\ntwo\nthree\nfour\nFIVE\n");
}

#[test]
fn work_on_a_detached_head_is_kept_and_a_lost_commit_found_again() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.git(&["switch", "-q", "--detach"]);
    let lost = fx.commit("b.txt", "b\n", "Work without a branch");
    let repo = Repo::open(fx.path()).unwrap();

    let keep = fix(
        &repo,
        Fix::SaveDetached {
            name: "rescue".into(),
            commit: None,
            switch: true,
        },
    );
    assert_eq!(displays(&repo, &keep), ["git switch -c rescue"]);

    // Leave it behind: only the reflog has it now.
    fx.git(&["switch", "-q", "main"]);
    let state = repo.fix_state().unwrap();
    assert_eq!(state.lost.len(), 1);
    assert_eq!(state.lost[0].id, lost);
    let action = fix(
        &repo,
        Fix::SaveDetached {
            name: "rescue".into(),
            commit: Some(lost.clone()),
            switch: false,
        },
    );
    repo.perform(&action).unwrap();
    assert_eq!(fx.git(&["rev-parse", "rescue"]), lost);
    assert_eq!(fx.git(&["branch", "--show-current"]), "main");
    assert!(repo.fix_state().unwrap().lost.is_empty());
}

#[test]
fn nothing_runs_when_the_repository_changed_since_the_plan() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    let tip = fx.commit("b.txt", "b\n", "Add b");
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(&repo, Fix::UndoLast { mode: ResetMode::Soft });

    fx.write("new.txt", "appeared\n");
    let err = repo.perform(&action).unwrap_err();
    assert!(matches!(err, Error::Changed(_)), "{err:?}");
    assert_eq!(
        repo.explain_failure(&action, &err).kind,
        oxbow_core::FailureKind::Changed
    );
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), tip);

    std::fs::remove_file(fx.path().join("new.txt")).unwrap();
    fx.commit("c.txt", "c\n", "Add c");
    assert!(matches!(repo.perform(&action).unwrap_err(), Error::Changed(_)));
}

#[test]
fn a_merge_among_commits_to_copy_is_refused() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.git(&["branch", "feature"]);
    fx.git(&["switch", "-q", "-c", "side"]);
    fx.commit("s.txt", "s\n", "Side");
    fx.git(&["switch", "-q", "main"]);
    fx.commit("m.txt", "m\n", "Main");
    fx.git(&["merge", "-q", "--no-ff", "-m", "Merge side", "side"]);
    let repo = Repo::open(fx.path()).unwrap();
    let to_feature = fix(
        &repo,
        Fix::MoveCommits {
            count: 2,
            to: "feature".into(),
            create: false,
        },
    );
    assert!(repo.plan(&to_feature).is_err());
    // A new branch only moves refs, so a merge is no problem there.
    let to_new = fix(
        &repo,
        Fix::MoveCommits {
            count: 2,
            to: "new".into(),
            create: true,
        },
    );
    assert!(repo.plan(&to_new).is_ok());
}

#[test]
fn a_forgotten_file_with_a_secret_stops_until_allowed() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.commit("a.txt", "a2\n", "Change a");
    // A fake key, split so that secret scanners don't take this file for a leak.
    fx.write("deploy.env", concat!("AWS_ACCESS_KEY_ID=AKIA", "Q7XKZ3M4P2LJ6WVB\n"));
    let repo = Repo::open(fx.path()).unwrap();
    let add = |allow_secrets| {
        fix(
            &repo,
            Fix::AddToLast {
                paths: vec!["deploy.env".into()],
                allow_secrets,
            },
        )
    };
    let err = repo.perform(&add(false)).unwrap_err();
    assert!(matches!(err, Error::Secrets(_)), "{err:?}");
    assert_eq!(fx.git(&["rev-list", "--count", "HEAD"]), "2");
    assert_eq!(fx.git(&["show", "--format=", "--name-only", "HEAD"]), "a.txt");

    // Staged by the stopped run, the same files changed: Add Anyway still runs.
    repo.perform(&add(true)).unwrap();
    assert_eq!(
        fx.git(&["show", "--format=", "--name-only", "HEAD"]),
        "a.txt\ndeploy.env"
    );
}

#[test]
fn files_left_unpicked_stay_out_of_the_last_commit_even_when_staged() {
    let mut fx = Fixture::new();
    fx.commit("b.txt", "one\ntwo\n", "Root");
    fx.commit("a.txt", "a\n", "Last commit");
    // b.txt has a staged edit and an unstaged one on top.
    fx.write("b.txt", "ONE\ntwo\n");
    fx.git(&["add", "b.txt"]);
    fx.write("b.txt", "ONE\nTWO\n");
    fx.write("forgotten.txt", "f\n");
    let repo = Repo::open(fx.path()).unwrap();

    let action = fix(
        &repo,
        Fix::AddToLast {
            paths: vec!["forgotten.txt".into()],
            allow_secrets: false,
        },
    );
    repo.perform(&action).unwrap();

    assert_eq!(
        fx.git(&["show", "--format=", "--name-only", "HEAD"]),
        "a.txt\nforgotten.txt"
    );
    assert_eq!(fx.git(&["show", "HEAD:b.txt"]), "one\ntwo");
    // Its staged edit is still staged, the other one still not.
    assert_eq!(fx.git(&["status", "--porcelain"]), "MM b.txt");
    assert_eq!(fx.git(&["show", ":b.txt"]), "ONE\ntwo");
}

#[test]
fn a_staged_file_can_be_added_to_the_last_commit() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.commit("a.txt", "a2\n", "Change a");
    fx.write("forgotten.txt", "f\n");
    fx.git(&["add", "forgotten.txt"]);
    fx.git(&["mv", "a.txt", "b.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    let state = repo.fix_state().unwrap();
    assert!(state.changed.iter().all(|f| f.staged && !f.unstaged));

    let action = fix(
        &repo,
        Fix::AddToLast {
            paths: vec!["b.txt".into(), "forgotten.txt".into()],
            allow_secrets: false,
        },
    );
    repo.perform(&action).unwrap();
    // A rename goes in whole: the old path is gone from the commit too.
    assert_eq!(
        fx.git(&["show", "--format=", "--name-status", "--no-renames", "HEAD"]),
        "D\ta.txt\nA\tb.txt\nA\tforgotten.txt"
    );
    assert_eq!(fx.git(&["status", "--porcelain"]), "");
}

#[test]
fn a_push_after_the_plan_was_made_stops_the_fix() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "a\n", "Root");
    let tip = fx.commit("b.txt", "b\n", "Move me");
    let remote = tempfile::tempdir().unwrap();
    let url = remote.path().join("origin.git");
    fx.git(&["init", "-q", "--bare", "-b", "main", url.to_str().unwrap()]);
    fx.git(&["remote", "add", "origin", url.to_str().unwrap()]);
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(
        &repo,
        Fix::MoveCommits {
            count: 1,
            to: "feature/rescue".into(),
            create: true,
        },
    );
    assert_eq!(kinds(&repo.plan(&action).unwrap().steps), ["create", "move"]);

    // Pushed meanwhile, without -u: HEAD, the branch and its upstream are as they were.
    fx.git(&["push", "-q", "origin", "main"]);
    let err = repo.perform(&action).unwrap_err();
    assert!(matches!(err, Error::Changed(_)), "{err:?}");
    assert_eq!(fx.git(&["rev-parse", "main"]), tip);
    assert_eq!(fx.git(&["branch", "--list", "feature/rescue"]), "");
    assert_ne!(fx.git(&["rev-parse", "main~1"]), "");
    assert_eq!(fx.git(&["rev-parse", "main~1"]), root);

    // Planned again, the commit is pushed: it is reverted, not taken off.
    let action = fix(
        &repo,
        Fix::MoveCommits {
            count: 1,
            to: "feature/rescue".into(),
            create: true,
        },
    );
    assert_eq!(kinds(&repo.plan(&action).unwrap().steps), ["revert", "create", "copy"]);
}

#[test]
fn staging_something_else_since_the_plan_stops_the_fix() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.commit("b.txt", "b\n", "Add b");
    fx.write("a.txt", "edit\n");
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(&repo, Fix::UndoLast { mode: ResetMode::Soft });
    fx.git(&["add", "a.txt"]);
    assert!(matches!(repo.perform(&action).unwrap_err(), Error::Changed(_)));
}

/// main with a commit meant for `feature`, which changes the same line of a.txt.
fn conflicting_move(fx: &mut Fixture) -> (String, String) {
    let base = fx.commit("a.txt", "a\n", "Root");
    fx.git(&["branch", "feature"]);
    fx.git(&["switch", "-q", "feature"]);
    fx.commit("a.txt", "feature\n", "Feature work");
    fx.git(&["switch", "-q", "main"]);
    let tip = fx.commit("a.txt", "main\n", "Move me");
    (base, tip)
}

#[test]
fn continue_after_a_conflict_finishes_the_move() {
    let mut fx = Fixture::new();
    let (base, _) = conflicting_move(&mut fx);
    fx.write("notes.txt", "staged\n");
    fx.git(&["add", "notes.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(
        &repo,
        Fix::MoveCommits {
            count: 1,
            to: "feature".into(),
            create: false,
        },
    );
    assert_eq!(
        kinds(&repo.plan(&action).unwrap().steps),
        ["stash", "switch", "copy", "move", "pop"]
    );
    assert!(repo.perform(&action).is_err());
    assert_eq!(fx.git(&["branch", "--show-current"]), "feature");

    // Continue shows the steps left after its own.
    let cont = Action::Continue { message: None };
    let shown = displays(&repo, &cont);
    assert_eq!(shown[0], "git cherry-pick --continue");
    assert_eq!(shown[1], format!("git branch -f main {}", &base[..7]));
    assert_eq!(shown[2], "git stash pop --index");

    fx.write("a.txt", "both\n");
    fx.git(&["add", "a.txt"]);
    repo.perform(&cont).unwrap();
    assert_eq!(fx.git(&["rev-parse", "main"]), base);
    assert_eq!(fx.git(&["log", "-1", "--format=%s", "feature"]), "Move me");
    assert_eq!(fx.git(&["status", "--porcelain"]), "A  notes.txt");
    assert_eq!(fx.git(&["stash", "list"]), "");
    // Nothing is left for a later Continue.
    assert!(!fx.path().join(".git/oxbow/fix-rest.json").exists());
}

#[test]
fn abort_after_a_conflict_goes_back_to_where_the_fix_started() {
    let mut fx = Fixture::new();
    let (_, tip) = conflicting_move(&mut fx);
    fx.write("notes.txt", "staged\n");
    fx.git(&["add", "notes.txt"]);
    fx.write("loose.txt", "untracked\n");
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(
        &repo,
        Fix::MoveCommits {
            count: 1,
            to: "feature".into(),
            create: false,
        },
    );
    assert!(repo.perform(&action).is_err());
    assert_eq!(
        displays(&repo, &Action::Abort),
        ["git cherry-pick --abort", "git switch main", "git stash pop --index"]
    );
    repo.perform(&Action::Abort).unwrap();
    assert_eq!(fx.git(&["branch", "--show-current"]), "main");
    assert_eq!(fx.git(&["rev-parse", "main"]), tip);
    assert_eq!(fx.git(&["status", "--porcelain"]), "A  notes.txt\n?? loose.txt");
    assert_eq!(fx.git(&["stash", "list"]), "");
}

#[test]
fn a_staged_file_out_of_the_way_is_put_aside_and_comes_back_staged() {
    // Moving to an existing branch: git picks only with nothing staged.
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "a\n", "Root");
    fx.git(&["branch", "feature"]);
    fx.commit("b.txt", "b\n", "Add b");
    fx.write("notes.txt", "staged\n");
    fx.git(&["add", "notes.txt"]);
    fx.write("notes.txt", "staged\nand not\n");
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(
        &repo,
        Fix::MoveCommits {
            count: 1,
            to: "feature".into(),
            create: false,
        },
    );
    assert_eq!(
        kinds(&repo.plan(&action).unwrap().steps),
        ["stash", "switch", "copy", "move", "pop"]
    );
    repo.perform(&action).unwrap();
    assert_eq!(fx.git(&["rev-parse", "main"]), root);
    assert_eq!(fx.git(&["log", "-1", "--format=%s", "feature"]), "Add b");
    assert_eq!(fx.git(&["status", "--porcelain"]), "AM notes.txt");
    assert_eq!(fx.git(&["show", ":notes.txt"]), "staged");

    // Reverting, the same.
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.commit("notes.txt", "n\n", "Notes");
    let bad = fx.commit("a.txt", "bad\n", "Break a");
    fx.write("notes.txt", "n2\n");
    fx.git(&["add", "notes.txt"]);
    fx.write("a.txt", "bad\n");
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(
        &repo,
        Fix::RevertPushed {
            commit: bad,
            push: false,
        },
    );
    assert_eq!(kinds(&repo.plan(&action).unwrap().steps), ["stash", "revert", "pop"]);
    repo.perform(&action).unwrap();
    assert_eq!(read(&fx, "a.txt").unwrap(), "a\n");
    assert_eq!(fx.git(&["status", "--porcelain"]), "M  notes.txt");
}

#[test]
fn staged_changes_the_revert_moved_from_under_come_back_unstaged() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n2\n3\n4\n5\n6\n7\n8\n", "Root");
    let bad = fx.commit("a.txt", "1\nX\n3\n4\n5\n6\n7\n8\n", "Break 2");
    // A staged edit three lines below: its patch has the reverted line as context.
    fx.write("a.txt", "1\nX\n3\n4\nY\n6\n7\n8\n");
    fx.git(&["add", "a.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    let action = fix(
        &repo,
        Fix::RevertPushed {
            commit: bad,
            push: false,
        },
    );
    repo.perform(&action).unwrap();
    assert_eq!(read(&fx, "a.txt").unwrap(), "1\n2\n3\n4\nY\n6\n7\n8\n");
    assert_eq!(fx.git(&["stash", "list"]), "");
}
