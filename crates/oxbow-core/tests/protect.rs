mod support;

use oxbow_core::stack::{StackPlan, StackStep, StepAction};
use oxbow_core::{Action, BranchPush, Fix, HistoryOptions, MergeMethod, Repo, ResetMode};
use support::Fixture;

/// A bare repository standing in for GitHub.
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
    Remote { _dir: dir }
}

/// `main` with two commits on origin and one not pushed yet.
fn pushed_main() -> (Fixture, Remote, [String; 3]) {
    let mut fx = Fixture::new();
    let remote = with_remote(&mut fx);
    let root = fx.commit("a.txt", "a\n", "Root");
    let pushed = fx.commit("b.txt", "b\n", "Add b");
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    let local = fx.commit("c.txt", "c\n", "Add c");
    (fx, remote, [root, pushed, local])
}

fn force_push(branch: &str) -> Action {
    Action::Push {
        remote: "origin".into(),
        branch: branch.into(),
        upstream: branch.into(),
        set_upstream: false,
        force: true,
        lease: None,
        no_verify: false,
        submodules: false,
        merge_request: None,
    }
}

fn amend() -> Action {
    Action::Commit {
        message: "Changed".into(),
        amend: true,
        no_verify: false,
        allow_secrets: false,
        skip_checks: false,
        sign_off: false,
        changelist: None,
    }
}

fn refused(repo: &Repo, action: &Action) -> String {
    let err = repo.plan(action).expect_err("refused").to_string();
    assert!(err.contains("protected branch"), "{err}");
    let run = repo.perform(action).expect_err("doesn't run").to_string();
    assert_eq!(run, err);
    err
}

#[test]
fn main_master_and_the_production_branch_are_protected_until_settings_say_otherwise() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "Root");
    fx.git(&["branch", "stable"]);
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(repo.protected_branches(), ["main", "master"]);
    assert_eq!(repo.protected_setting(), None);

    fx.git(&["config", "oxbow.productionBranch", "refs/heads/stable"]);
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(repo.protected_branches(), ["main", "master", "stable"]);

    fx.git(&["config", "oxbow.protectedBranches", "main release/*"]);
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(repo.protected_branches(), ["main", "release/*"]);
    let history = repo.history(&HistoryOptions::default()).unwrap();
    assert_eq!(history.protected, ["main", "release/*"]);

    // Set and empty: none is protected.
    fx.git(&["config", "oxbow.protectedBranches", ""]);
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.protected_branches().is_empty());
}

#[test]
fn a_production_branch_on_a_remote_is_protected_by_its_name_there() {
    let (mut fx, _remote, _) = pushed_main();
    fx.git(&["push", "-q", "origin", "main:prod"]);
    fx.git(&["config", "oxbow.productionBranch", "refs/remotes/origin/prod"]);
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(repo.protected_branches(), ["main", "master", "prod"]);
}

#[test]
fn a_protected_branch_is_never_force_pushed() {
    let (mut fx, _remote, _) = pushed_main();
    fx.git(&["branch", "feature"]);
    let repo = Repo::open(fx.path()).unwrap();
    let err = refused(&repo, &force_push("main"));
    assert!(
        err.starts_with("main is a protected branch: Oxbow doesn’t force push it"),
        "{err}"
    );
    // Other branches still can be, and main without force.
    assert!(repo.plan(&force_push("feature")).is_ok());
    let mut plain = force_push("main");
    if let Action::Push { force, .. } = &mut plain {
        *force = false;
    }
    assert!(repo.plan(&plain).is_ok());

    let replay = Action::ReplayAndPush {
        remote: "origin".into(),
        branch: "main".into(),
        upstream: "main".into(),
        base: "HEAD".into(),
        lease: "HEAD".into(),
        no_verify: false,
    };
    refused(&repo, &replay);
    let several = Action::PushBranches {
        remote: "origin".into(),
        branches: vec![
            BranchPush {
                branch: "feature".into(),
                upstream: "feature".into(),
            },
            BranchPush {
                branch: "main".into(),
                upstream: "main".into(),
            },
        ],
    };
    refused(&repo, &several);
}

#[test]
fn pushed_commits_of_a_protected_branch_are_not_rewritten_and_unpushed_ones_are() {
    let (mut fx, _remote, [root, pushed, local]) = pushed_main();
    let repo = Repo::open(fx.path()).unwrap();
    // The last commit isn't pushed: Amend and Edit Message work as anywhere.
    assert!(repo.plan(&amend()).is_ok());
    let reword = Action::Reword {
        message: "Add c, reworded".into(),
        no_verify: false,
    };
    repo.perform(&reword).unwrap();
    // Reset back to what origin has drops only the unpushed commit.
    let back = Action::Reset {
        commit: pushed.clone(),
        mode: ResetMode::Hard,
    };
    assert!(repo.plan(&back).is_ok());
    // Past it, the pushed commit would go.
    let past = Action::Reset {
        commit: root.clone(),
        mode: ResetMode::Soft,
    };
    let err = refused(&repo, &past);
    assert!(err.contains("Revert them instead"), "{err}");

    fx.git(&["reset", "-q", "--hard", &pushed]);
    let repo = Repo::open(fx.path()).unwrap();
    refused(&repo, &amend());
    refused(&repo, &reword);
    refused(
        &repo,
        &Action::Fix {
            fix: Fix::UndoLast { mode: ResetMode::Soft },
            expect: repo.fix_state().unwrap().expect,
        },
    );
    fx.write("a.txt", "a2\n");
    fx.git(&["add", "a.txt"]);
    refused(
        &repo,
        &Action::AddToCommit {
            commit: pushed.clone(),
            allow_secrets: false,
            skip_checks: false,
        },
    );
    // Revert undoes it the way that stays: a new commit.
    fx.git(&["reset", "-q", "--hard"]);
    assert!(repo.perform(&Action::Revert { commit: pushed.clone() }).is_ok());
    assert_ne!(fx.git(&["rev-parse", "HEAD"]), local);
}

#[test]
fn a_rebase_of_a_protected_branch_stops_only_when_it_would_rewrite_pushed_commits() {
    let (mut fx, _remote, [root, ..]) = pushed_main();
    fx.git(&["branch", "other", &root]);
    fx.git(&["checkout", "-q", "other"]);
    fx.commit("d.txt", "d\n", "Add d");
    fx.git(&["checkout", "-q", "main"]);
    let repo = Repo::open(fx.path()).unwrap();
    let rebase = Action::Merge {
        branch: "other".into(),
        method: MergeMethod::Rebase,
        message: None,
    };
    refused(&repo, &rebase);
    // A merge doesn't rewrite anything.
    let merge = Action::Merge {
        branch: "other".into(),
        method: MergeMethod::Merge,
        message: None,
    };
    assert!(repo.plan(&merge).is_ok());
    // Onto origin/main itself only the unpushed commit moves.
    let onto_pushed = Action::Merge {
        branch: "origin/main".into(),
        method: MergeMethod::Rebase,
        message: None,
    };
    assert!(repo.plan(&onto_pushed).is_ok());
}

#[test]
fn nothing_is_protected_once_the_list_is_empty_or_without_a_remote() {
    let (mut fx, _remote, [root, ..]) = pushed_main();
    fx.git(&["config", "oxbow.protectedBranches", ""]);
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.plan(&force_push("main")).is_ok());
    let past = Action::Reset {
        commit: root.clone(),
        mode: ResetMode::Soft,
    };
    assert!(repo.plan(&past).is_ok());

    // A repository without remotes has nothing pushed to keep.
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "a\n", "Root");
    fx.commit("b.txt", "b\n", "Add b");
    let repo = Repo::open(fx.path()).unwrap();
    assert!(repo.plan(&amend()).is_ok());
    assert!(
        repo.plan(&Action::Reset {
            commit: root,
            mode: ResetMode::Hard
        })
        .is_ok()
    );
}

#[test]
fn edit_stack_leaves_a_protected_branch_with_pushed_commits_alone() {
    let (mut fx, _remote, _) = pushed_main();
    fx.git(&["checkout", "-q", "-b", "release/2"]);
    let one = fx.commit("r.txt", "1\n", "Release fix 1");
    fx.commit("r.txt", "2\n", "Release fix 2");
    fx.git(&["push", "-q", "-u", "origin", "release/2"]);
    fx.git(&["config", "oxbow.protectedBranches", "main release/*"]);
    let repo = Repo::open(fx.path()).unwrap();
    let err = repo.stack(None).expect_err("not editable").to_string();
    assert!(
        err.starts_with("release/2 is a protected branch, and its commits are already pushed"),
        "{err}"
    );
    // A plan made anyway, e.g. before the setting changed, doesn't run either.
    let base = fx.git(&["merge-base", "main", "release/2"]);
    let plan = StackPlan {
        top: "release/2".into(),
        base: base.clone(),
        onto: base,
        onto_name: None,
        head: None,
        steps: vec![StackStep::Commit {
            id: one,
            action: StepAction::Drop,
            message: None,
        }],
        splits: Vec::new(),
    };
    refused(&repo, &Action::EditStack { plan });

    // Not protected, the same stack loads.
    fx.git(&["config", "oxbow.protectedBranches", "main"]);
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(repo.stack(None).unwrap().commits.len(), 2);
}
