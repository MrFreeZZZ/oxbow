mod support;

use oxbow_core::{Action, FailureKind, RefKind, RemoteBranch, Repo};
use support::Fixture;

/// A repository with `main` pushed to a bare remote standing in for GitHub.
fn with_remote() -> (tempfile::TempDir, Fixture) {
    let remote = tempfile::tempdir().unwrap();
    let url = remote.path().join("acme.git");
    let status = std::process::Command::new("git")
        .args(["init", "-q", "--bare", "-b", "main"])
        .arg(&url)
        .status()
        .unwrap();
    assert!(status.success());
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    fx.git(&["remote", "add", "origin", url.to_str().unwrap()]);
    fx.git(&["push", "-q", "-u", "origin", "main"]);
    (remote, fx)
}

fn switch(branch: &str, stash: bool) -> Action {
    Action::Switch {
        branch: branch.into(),
        stash,
        keep: None,
    }
}

fn head(fx: &mut Fixture) -> String {
    fx.git(&["branch", "--show-current"])
}

#[test]
fn switching_moves_head_and_carries_uncommitted_files() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    fx.git(&["branch", "feature/x"]);
    fx.write("b.txt", "draft\n");
    let repo = Repo::open(fx.path()).unwrap();

    let plan = repo.plan(&switch("feature/x", false)).unwrap();
    assert_eq!(plan.commands[0].display(), "git switch feature/x");
    repo.perform(&switch("feature/x", false)).unwrap();
    assert_eq!(head(&mut fx), "feature/x");
    assert_eq!(fx.git(&["status", "--porcelain"]), "?? b.txt");
}

#[test]
fn changes_in_the_way_are_recognized_and_can_be_stashed() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "Root");
    fx.git(&["switch", "-q", "-c", "other"]);
    fx.commit("a.txt", "two\n", "Change a");
    fx.git(&["switch", "-q", "main"]);
    fx.write("a.txt", "mine\n");
    let repo = Repo::open(fx.path()).unwrap();

    let err = repo.perform(&switch("other", false)).unwrap_err();
    assert_eq!(
        repo.explain_failure(&switch("other", false), &err).kind,
        FailureKind::LocalChanges
    );
    assert_eq!(head(&mut fx), "main");

    let plan = repo.plan(&switch("other", true)).unwrap();
    assert_eq!(
        plan.commands[0].display(),
        "git stash push --include-untracked -m 'Before switching to other'"
    );
    repo.perform(&switch("other", true)).unwrap();
    assert_eq!(head(&mut fx), "other");
    assert_eq!(
        fx.git(&["stash", "list"]),
        "stash@{0}: On main: Before switching to other"
    );
}

#[test]
fn new_branches_are_created_switched_to_and_published() {
    let (_remote, mut fx) = with_remote();
    let repo = Repo::open(fx.path()).unwrap();
    let create = Action::CreateBranch {
        name: "feature/csv".into(),
        start: Some("main".into()),
        switch: true,
        publish: Some("origin".into()),
    };
    let displays: Vec<String> = repo
        .plan(&create)
        .unwrap()
        .commands
        .iter()
        .map(|c| c.display())
        .collect();
    assert_eq!(
        displays,
        ["git switch -c feature/csv main", "git push -u origin feature/csv"]
    );
    repo.perform(&create).unwrap();
    assert_eq!(head(&mut fx), "feature/csv");
    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "feature/csv@{upstream}"]),
        "origin/feature/csv"
    );

    let stay = Action::CreateBranch {
        name: "fix/y".into(),
        start: None,
        switch: false,
        publish: None,
    };
    assert_eq!(repo.plan(&stay).unwrap().commands[0].display(), "git branch fix/y");
    repo.perform(&stay).unwrap();
    assert_eq!(head(&mut fx), "feature/csv");
    let history = repo.history(&Default::default()).unwrap();
    let local = |name: &str| {
        history
            .refs
            .iter()
            .find(|r| r.kind == RefKind::Local && r.name == name)
            .cloned()
    };
    assert_eq!(local("feature/csv").unwrap().tracking.unwrap().branch, "feature/csv");
    assert!(local("fix/y").unwrap().tracking.is_none());
}

#[test]
fn remote_branches_are_checked_out_as_tracking_branches() {
    let (_remote, mut fx) = with_remote();
    fx.git(&["switch", "-q", "-c", "theirs"]);
    fx.commit("t.txt", "t\n", "Their work");
    fx.git(&["push", "-q", "origin", "theirs"]);
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["branch", "-q", "-D", "theirs"]);
    let repo = Repo::open(fx.path()).unwrap();

    let track = Action::Track {
        remote: "origin".into(),
        branch: "theirs".into(),
        stash: false,
    };
    assert_eq!(
        repo.plan(&track).unwrap().commands[0].display(),
        "git switch --track origin/theirs"
    );
    repo.perform(&track).unwrap();
    assert_eq!(head(&mut fx), "theirs");
    assert_eq!(fx.git(&["rev-parse", "--abbrev-ref", "@{upstream}"]), "origin/theirs");
}

#[test]
fn renaming_on_the_remote_publishes_the_new_name_and_drops_the_old() {
    let (_remote, mut fx) = with_remote();
    fx.git(&["switch", "-q", "-c", "old-name"]);
    fx.commit("o.txt", "o\n", "Work");
    fx.git(&["push", "-q", "-u", "origin", "old-name"]);
    let repo = Repo::open(fx.path()).unwrap();

    repo.perform(&Action::RenameBranch {
        from: "old-name".into(),
        to: "new-name".into(),
        upstream: Some(RemoteBranch {
            remote: "origin".into(),
            branch: "old-name".into(),
        }),
    })
    .unwrap();
    assert_eq!(head(&mut fx), "new-name");
    assert_eq!(fx.git(&["rev-parse", "--abbrev-ref", "@{upstream}"]), "origin/new-name");
    assert_eq!(fx.git(&["ls-remote", "--heads", "origin", "old-name"]), "");
}

#[test]
fn deleting_knows_which_commits_would_be_lost() {
    let (_remote, mut fx) = with_remote();
    // Merged into main: nothing to lose.
    fx.git(&["branch", "merged"]);
    // Pushed: safe on origin, lost only if origin's copy goes too.
    fx.git(&["switch", "-q", "-c", "pushed"]);
    let pushed = fx.commit("p.txt", "p\n", "Pushed work");
    fx.git(&["push", "-q", "-u", "origin", "pushed"]);
    // Never pushed and on no other branch.
    fx.git(&["switch", "-q", "-c", "local", "main"]);
    let local = fx.commit("l.txt", "l\n", "Local work");
    fx.git(&["switch", "-q", "main"]);
    let repo = Repo::open(fx.path()).unwrap();

    let merged = repo.deletion_check("merged").unwrap();
    assert!(merged.lost.is_empty() && merged.lost_with_upstream.is_empty());
    let check = repo.deletion_check("pushed").unwrap();
    assert!(check.lost.is_empty());
    assert_eq!(
        check
            .lost_with_upstream
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        [pushed]
    );
    let check = repo.deletion_check("local").unwrap();
    assert_eq!(check.lost[0].id, local);
    assert_eq!(check.lost[0].summary, "Local work");

    let delete = |name: &str, force: bool| Action::DeleteBranch {
        name: name.into(),
        force,
        upstream: None,
    };
    let err = repo.perform(&delete("local", false)).unwrap_err();
    assert_eq!(
        repo.explain_failure(&delete("local", false), &err).kind,
        FailureKind::NotMerged
    );
    repo.perform(&delete("local", true)).unwrap();
    repo.perform(&Action::DeleteBranch {
        name: "pushed".into(),
        force: true,
        upstream: Some(RemoteBranch {
            remote: "origin".into(),
            branch: "pushed".into(),
        }),
    })
    .unwrap();
    assert_eq!(fx.git(&["branch", "--list", "local", "pushed"]), "");
    assert_eq!(fx.git(&["ls-remote", "--heads", "origin", "pushed"]), "");
}

#[test]
fn deleting_a_branch_with_its_upstream_counts_commits_only_the_remote_has() {
    let (_remote, mut fx) = with_remote();
    fx.git(&["switch", "-q", "-c", "shared"]);
    fx.git(&["push", "-q", "-u", "origin", "shared"]);
    // A teammate's commit on origin/shared that the local branch never pulled.
    fx.git(&["switch", "-q", "--detach"]);
    let theirs = fx.commit("t.txt", "t\n", "Their commit");
    fx.git(&["push", "-q", "origin", "HEAD:shared"]);
    fx.git(&["switch", "-q", "main"]);
    fx.git(&["fetch", "-q", "origin"]);
    let repo = Repo::open(fx.path()).unwrap();

    let check = repo.deletion_check("shared").unwrap();
    assert!(check.lost.is_empty());
    assert_eq!(check.lost_with_upstream[0].id, theirs);
    assert_eq!(repo.remote_deletion_check("origin/shared").unwrap()[0].id, theirs);
    assert!(repo.remote_deletion_check("origin/main").unwrap().is_empty());
}

#[test]
fn remote_branches_are_deleted_on_the_remote() {
    let (_remote, mut fx) = with_remote();
    fx.git(&["push", "-q", "origin", "main:gone-soon"]);
    let repo = Repo::open(fx.path()).unwrap();
    let delete = Action::DeleteRemoteBranch {
        remote: "origin".into(),
        branch: "gone-soon".into(),
        expect: None,
    };
    assert_eq!(
        repo.plan(&delete).unwrap().commands[0].display(),
        "git push origin --delete gone-soon"
    );
    repo.perform(&delete).unwrap();
    assert_eq!(fx.git(&["ls-remote", "--heads", "origin", "gone-soon"]), "");
}

#[test]
fn detaching_and_keeping_commits_made_without_a_branch() {
    let mut fx = Fixture::new();
    let root = fx.commit("a.txt", "one\n", "Root");
    fx.commit("a.txt", "two\n", "Second");
    let repo = Repo::open(fx.path()).unwrap();

    let detach = Action::Detach {
        commit: root.clone(),
        stash: false,
    };
    assert_eq!(
        repo.plan(&detach).unwrap().commands[0].display(),
        format!("git switch --detach {}", &root[..7])
    );
    repo.perform(&detach).unwrap();
    assert_eq!(head(&mut fx), "");
    let experiment = fx.commit("b.txt", "b\n", "Experiment");

    let history = repo.history(&Default::default()).unwrap();
    assert_eq!(history.previous_branch.as_deref(), Some("main"));
    let top = &history.rows[0];
    assert_eq!(top.id, experiment);
    assert!(top.no_branch);
    assert_eq!(top.graph.color, oxbow_core::graph::NO_BRANCH_COLOR);
    assert_eq!(
        (top.labels[0].name.as_str(), top.labels[0].kind),
        ("HEAD", RefKind::Head)
    );
    // The detached line takes column 0; main steps aside until the commit it started from.
    assert_eq!(top.graph.column, 0);
    let root_row = history.rows.iter().find(|r| r.id == root).unwrap();
    assert!(!root_row.no_branch);
    assert_eq!(root_row.graph.fork_colors, [oxbow_core::graph::NO_BRANCH_COLOR]);

    let keep = Action::Switch {
        branch: "main".into(),
        stash: false,
        keep: Some("experiment".into()),
    };
    let displays: Vec<String> = repo.plan(&keep).unwrap().commands.iter().map(|c| c.display()).collect();
    assert_eq!(displays, ["git branch experiment", "git switch main"]);
    repo.perform(&keep).unwrap();
    assert_eq!(head(&mut fx), "main");
    assert_eq!(fx.git(&["rev-parse", "experiment"]), experiment);
}
