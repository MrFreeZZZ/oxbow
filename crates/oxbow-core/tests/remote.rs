mod support;

use std::sync::atomic::AtomicBool;

use oxbow_core::{Action, ActionEvent, FailureKind, Repo};
use support::Fixture;

/// A bare repository standing in for GitHub, and two clones of it: ours and a teammate's.
struct Team {
    _remote: tempfile::TempDir,
    me: Fixture,
    kirill: Fixture,
}

impl Team {
    fn new() -> Self {
        let remote = tempfile::tempdir().unwrap();
        let url = remote.path().join("acme.git");
        let status = std::process::Command::new("git")
            .args(["init", "-q", "--bare", "-b", "main"])
            .arg(&url)
            .status()
            .unwrap();
        assert!(status.success());
        let url = url.to_str().unwrap();

        let mut me = Fixture::new();
        me.commit("a.txt", "one\n", "Root");
        me.git(&["remote", "add", "origin", url]);
        me.git(&["push", "-q", "-u", "origin", "main"]);

        let mut kirill = Fixture::new();
        kirill.git(&["remote", "add", "origin", url]);
        kirill.git(&["fetch", "-q", "origin"]);
        kirill.git(&["checkout", "-q", "-B", "main", "origin/main"]);
        Team {
            _remote: remote,
            me,
            kirill,
        }
    }
}

fn push(set_upstream: bool) -> Action {
    Action::Push {
        remote: "origin".into(),
        branch: "main".into(),
        upstream: "main".into(),
        set_upstream,
        force: false,
        lease: None,
        no_verify: false,
    }
}

#[test]
fn tracking_counts_commits_both_ways() {
    let mut team = Team::new();
    let repo = Repo::open(team.me.path()).unwrap();
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.remote.as_str(), tracking.branch.as_str()), ("origin", "main"));
    assert_eq!((tracking.ahead, tracking.behind), (0, 0));

    team.me.commit("b.txt", "mine\n", "Mine");
    team.kirill.commit("c.txt", "his\n", "His");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    repo.perform(&Action::Fetch {
        remote: Some("origin".into()),
    })
    .unwrap();
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind), (1, 1));
    // A fetch only moves origin/main, which an undo can't take back: it stays out of the log.
    assert!(repo.operation_log().unwrap().is_empty());

    // The history carries the same counts for the toolbar.
    let history = repo.history(&Default::default()).unwrap();
    assert_eq!(history.tracking.unwrap().behind, 1);
    assert_eq!(history.default_remote.as_deref(), Some("origin"));
}

#[test]
fn an_edited_pushed_commit_is_pushed_with_a_lease() {
    let mut team = Team::new();
    team.me.commit("b.txt", "mine\n", "Yonder spider");
    team.me.git(&["push", "-q", "origin", "main"]);
    let pushed = team.me.git(&["rev-parse", "HEAD"]);
    let repo = Repo::open(team.me.path()).unwrap();
    assert_eq!(repo.tracking().unwrap().unwrap().rewritten, None);

    repo.perform(&Action::Reword {
        message: "Yandex spider".into(),
        no_verify: false,
    })
    .unwrap();
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind), (1, 1));
    assert_eq!(tracking.rewritten.as_deref(), Some(pushed.as_str()));

    let mut action = push(false);
    if let Action::Push { force, lease, .. } = &mut action {
        *force = true;
        *lease = tracking.rewritten.clone();
    }
    repo.perform(&action).unwrap();
    assert_eq!(
        team.me.git(&["log", "-1", "--format=%s", "origin/main"]),
        "Yandex spider"
    );
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind, tracking.rewritten), (0, 0, None));

    // A teammate's commit was never on this branch: that is something to pull, not to replace.
    team.kirill.git(&["pull", "-q", "--rebase", "origin", "main"]);
    team.kirill.commit("c.txt", "his\n", "His");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    team.me.commit("d.txt", "more\n", "More");
    repo.perform(&Action::Fetch {
        remote: Some("origin".into()),
    })
    .unwrap();
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind, tracking.rewritten), (1, 1, None));
}

#[test]
fn an_undone_pull_does_not_make_a_teammates_commit_look_rewritten() {
    let mut team = Team::new();
    team.kirill.commit("c.txt", "his\n", "His");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    let repo = Repo::open(team.me.path()).unwrap();
    team.me.git(&["pull", "-q", "--ff-only", "origin", "main"]);
    // Undo of the pull: the branch goes back, but its reflog still has Kirill's commit.
    team.me.git(&["reset", "-q", "--hard", "HEAD@{1}"]);
    team.me.commit("b.txt", "mine\n", "Mine");
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind), (1, 1));
    assert_eq!(tracking.rewritten, None);
}

#[test]
fn a_dropped_pushed_commit_is_pushed_away_with_a_lease() {
    let mut team = Team::new();
    team.me.commit("b.txt", "mine\n", "Oops");
    team.me.git(&["push", "-q", "origin", "main"]);
    let pushed = team.me.git(&["rev-parse", "HEAD"]);
    let repo = Repo::open(team.me.path()).unwrap();
    // Undo Commit: the branch goes back, nothing new on top.
    team.me.git(&["reset", "-q", "--soft", "HEAD~1"]);
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind), (0, 1));
    assert_eq!(tracking.rewritten.as_deref(), Some(pushed.as_str()));
    assert_eq!(tracking.replay, None);

    let mut action = push(false);
    if let Action::Push { force, lease, .. } = &mut action {
        *force = true;
        *lease = tracking.rewritten.clone();
    }
    repo.perform(&action).unwrap();
    assert_eq!(team.me.git(&["log", "-1", "--format=%s", "origin/main"]), "Root");
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind, tracking.rewritten), (0, 0, None));
}

/// Edit Message on a pushed commit, while Kirill pushes on top of its old version.
fn edited_under_a_teammate() -> (Team, Repo, String) {
    let mut team = Team::new();
    team.me.commit("b.txt", "mine\n", "Old message");
    team.me.git(&["push", "-q", "origin", "main"]);
    let old = team.me.git(&["rev-parse", "HEAD"]);
    let repo = Repo::open(team.me.path()).unwrap();
    repo.perform(&Action::Reword {
        message: "New message".into(),
        no_verify: false,
    })
    .unwrap();
    team.kirill.git(&["pull", "-q", "--rebase", "origin", "main"]);
    team.kirill.commit("c.txt", "his\n", "Peer change");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    (team, repo, old)
}

fn messages(team: &mut Team, rev: &str) -> String {
    team.me.git(&["log", "--format=%s", rev])
}

#[test]
fn commits_pushed_on_an_edited_one_are_copied_over_before_the_push() {
    let (mut team, repo, old) = edited_under_a_teammate();
    repo.perform(&Action::Fetch {
        remote: Some("origin".into()),
    })
    .unwrap();
    let tip = team.me.git(&["rev-parse", "origin/main"]);
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind), (1, 2));
    assert_eq!(tracking.rewritten.as_deref(), Some(tip.as_str()));
    let replay = tracking.replay.unwrap();
    assert_eq!(replay.base, old);
    assert_eq!(
        replay.commits.iter().map(|c| c.summary.as_str()).collect::<Vec<_>>(),
        ["Peer change"]
    );

    let action = Action::ReplayAndPush {
        remote: "origin".into(),
        branch: "main".into(),
        upstream: "main".into(),
        base: replay.base,
        lease: tip,
        no_verify: false,
    };
    let plan = repo.plan(&action).unwrap();
    assert_eq!(plan.commands.len(), 2);
    assert!(
        plan.commands[0].display().starts_with("git cherry-pick "),
        "{}",
        plan.commands[0].display()
    );
    repo.perform(&action).unwrap();
    assert_eq!(messages(&mut team, "origin/main"), "Peer change\nNew message\nRoot");
    assert!(team.me.path().join("c.txt").exists());
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind, tracking.rewritten), (0, 0, None));
}

#[test]
fn a_stale_lease_after_an_edit_says_what_to_copy_over() {
    let (mut team, repo, old) = edited_under_a_teammate();
    // The last fetch was before Kirill's push: the lease is the old version.
    let mut action = push(false);
    if let Action::Push { force, lease, .. } = &mut action {
        *force = true;
        *lease = Some(old.clone());
    }
    let err = repo.perform(&action).unwrap_err();
    let failure = repo.explain_failure(&action, &err);
    assert_eq!(failure.kind, FailureKind::StaleLease);
    let tip = team.me.git(&["rev-parse", "origin/main"]);
    assert_eq!(failure.remote_tip.as_deref(), Some(tip.as_str()));
    let tracking = failure.tracking.unwrap();
    assert_eq!(tracking.rewritten.as_deref(), Some(tip.as_str()));
    assert_eq!(tracking.replay.unwrap().base, old);
    // The edited message is still the branch's, not lost to a rebase.
    assert_eq!(messages(&mut team, "main"), "New message\nRoot");
}

#[test]
fn a_copied_commit_resolved_by_hand_is_not_copied_again() {
    let mut team = Team::new();
    team.me.commit("b.txt", "mine\n", "Mine");
    team.me.git(&["push", "-q", "origin", "main"]);
    let old = team.me.git(&["rev-parse", "HEAD"]);
    team.kirill.git(&["pull", "-q", "--rebase", "origin", "main"]);
    team.kirill.commit("b.txt", "his\n", "His");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    // Amend with other contents: Kirill's change no longer applies cleanly.
    team.me.write("b.txt", "mine, better\n");
    team.me.git(&["commit", "-q", "-a", "--amend", "--no-edit"]);
    team.me.git(&["fetch", "-q", "origin"]);
    let tip = team.me.git(&["rev-parse", "origin/main"]);
    let repo = Repo::open(team.me.path()).unwrap();
    assert_eq!(repo.tracking().unwrap().unwrap().replay.unwrap().base, old);

    let action = Action::ReplayAndPush {
        remote: "origin".into(),
        branch: "main".into(),
        upstream: "main".into(),
        base: old,
        lease: tip.clone(),
        no_verify: false,
    };
    let err = repo.perform(&action).unwrap_err();
    assert_eq!(repo.explain_failure(&action, &err).kind, FailureKind::Conflict);
    team.me.write("b.txt", "his, better\n");
    team.me.git(&["add", "b.txt"]);
    team.me.git(&["-c", "core.editor=true", "cherry-pick", "--continue"]);

    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!(tracking.rewritten.as_deref(), Some(tip.as_str()));
    assert_eq!(tracking.replay, None);
}

#[test]
fn a_teammates_push_on_unchanged_commits_is_still_a_plain_pull() {
    let mut team = Team::new();
    team.me.commit("b.txt", "mine\n", "Mine");
    team.me.git(&["push", "-q", "origin", "main"]);
    team.kirill.git(&["pull", "-q", "--rebase", "origin", "main"]);
    team.kirill.commit("c.txt", "his\n", "His");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    let repo = Repo::open(team.me.path()).unwrap();
    team.me.git(&["fetch", "-q", "origin"]);
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind, tracking.rewritten), (0, 1, None));
}

#[test]
fn push_streams_its_output() {
    let mut team = Team::new();
    team.me.commit("b.txt", "mine\n", "Mine");
    let repo = Repo::open(team.me.path()).unwrap();
    let mut events = Vec::new();
    repo.perform_with(&push(false), &mut |e| events.push(e), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(
        events[0],
        ActionEvent::Command {
            display: "git push origin main".into()
        }
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, ActionEvent::Line(l) if l.text.contains("main -> main")))
    );
    assert_eq!(repo.tracking().unwrap().unwrap().ahead, 0);
}

#[test]
fn rejected_push_names_the_commits_in_the_way_and_pull_and_push_fixes_it() {
    let mut team = Team::new();
    team.me.commit("b.txt", "mine\n", "Mine");
    let his = team.kirill.commit("c.txt", "his\n", "His change");
    team.kirill.git(&["push", "-q", "origin", "main"]);

    let repo = Repo::open(team.me.path()).unwrap();
    let error = repo.perform(&push(false)).unwrap_err();
    let failure = repo.explain_failure(&push(false), &error);
    assert_eq!(failure.kind, FailureKind::Rejected);
    assert_eq!(failure.incoming.len(), 1);
    assert_eq!(failure.incoming[0].summary, "His change");
    assert_eq!(failure.remote_tip.as_deref(), Some(his.as_str()));

    let both = Action::PullAndPush {
        remote: "origin".into(),
        branch: "main".into(),
        upstream: "main".into(),
        no_verify: false,
    };
    let commands: Vec<String> = repo.plan(&both).unwrap().commands.iter().map(|c| c.display()).collect();
    assert_eq!(
        commands,
        ["git pull --rebase --autostash origin main", "git push origin main"]
    );
    repo.perform(&both).unwrap();
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!((tracking.ahead, tracking.behind), (0, 0));
    assert_eq!(team.me.git(&["log", "-1", "--format=%s", "HEAD~1"]), "His change");
}

#[test]
fn force_push_with_lease_refuses_when_the_remote_moved_again() {
    let mut team = Team::new();
    team.me.commit("b.txt", "mine\n", "Mine");
    let his = team.kirill.commit("c.txt", "his\n", "His change");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    let repo = Repo::open(team.me.path()).unwrap();
    let rejected = repo.perform(&push(false)).unwrap_err();
    let tip = repo.explain_failure(&push(false), &rejected).remote_tip.unwrap();
    assert_eq!(tip, his);

    // Kirill pushes once more before we force push: the lease must protect his new commit.
    team.kirill.commit("d.txt", "again\n", "His second change");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    let force = Action::Push {
        remote: "origin".into(),
        branch: "main".into(),
        upstream: "main".into(),
        set_upstream: false,
        force: true,
        lease: Some(tip.clone()),
        no_verify: false,
    };
    assert_eq!(
        repo.plan(&force).unwrap().commands[0].display(),
        format!("git push --force-with-lease=main:{tip} origin main")
    );
    let error = repo.perform(&force).unwrap_err();
    assert_eq!(repo.explain_failure(&force, &error).kind, FailureKind::StaleLease);

    // With the lease at the remote's real tip the push goes through and replaces his commits.
    let tip = repo.remote_tip("origin", "main").unwrap();
    let force = Action::Push {
        remote: "origin".into(),
        branch: "main".into(),
        upstream: "main".into(),
        set_upstream: false,
        force: true,
        lease: Some(tip),
        no_verify: false,
    };
    repo.perform(&force).unwrap();
    assert_eq!(
        team.kirill.git(&["ls-remote", "origin", "main"]).split('\t').next(),
        Some(team.me.git(&["rev-parse", "HEAD"]).as_str())
    );
}

#[test]
fn publishing_a_branch_sets_its_upstream() {
    let mut team = Team::new();
    team.me.git(&["switch", "-q", "-c", "feature/csv"]);
    team.me.commit("csv.rs", "fn csv() {}\n", "CSV export");
    let repo = Repo::open(team.me.path()).unwrap();
    assert!(repo.tracking().unwrap().is_none());
    let publish = Action::Push {
        remote: "origin".into(),
        branch: "feature/csv".into(),
        upstream: "feature/csv".into(),
        set_upstream: true,
        force: false,
        lease: None,
        no_verify: false,
    };
    assert_eq!(
        repo.plan(&publish).unwrap().commands[0].display(),
        "git push -u origin feature/csv"
    );
    repo.perform(&publish).unwrap();
    let tracking = repo.tracking().unwrap().unwrap();
    assert_eq!(tracking.branch, "feature/csv");
}

#[test]
fn pull_fetches_every_branch_first_when_asked() {
    let mut team = Team::new();
    team.kirill.git(&["checkout", "-q", "-b", "feature/csv"]);
    team.kirill.commit("csv.txt", "csv\n", "CSV");
    team.kirill.git(&["push", "-q", "origin", "feature/csv"]);
    team.kirill.git(&["checkout", "-q", "main"]);
    team.kirill.commit("c.txt", "his\n", "His");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    let repo = Repo::open(team.me.path()).unwrap();
    let pull = |fetch_first| Action::Pull {
        remote: "origin".into(),
        branch: "main".into(),
        fetch_first,
    };

    // git pull alone only brings origin/main up to date.
    let plain = repo.plan(&pull(false)).unwrap().commands;
    assert_eq!(plain.len(), 1);
    repo.perform(&pull(false)).unwrap();
    assert_eq!(team.me.git(&["log", "-1", "--format=%s"]), "His");
    assert!(
        team.me
            .git(&["branch", "-r"])
            .lines()
            .all(|b| b.trim() != "origin/feature/csv")
    );

    let first = repo.plan(&pull(true)).unwrap().commands;
    assert_eq!(first[0].args, ["fetch", "--prune", "origin"]);
    assert_eq!(first[1].args[0], "pull");
    repo.perform(&pull(true)).unwrap();
    assert!(
        team.me
            .git(&["branch", "-r"])
            .lines()
            .any(|b| b.trim() == "origin/feature/csv")
    );
}

#[test]
fn pull_conflict_is_recognized_and_can_be_aborted() {
    let mut team = Team::new();
    team.me.commit("a.txt", "mine\n", "Mine");
    team.kirill.commit("a.txt", "his\n", "His");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    let repo = Repo::open(team.me.path()).unwrap();
    let pull = Action::Pull {
        remote: "origin".into(),
        branch: "main".into(),
        fetch_first: false,
    };
    let error = repo.perform(&pull).unwrap_err();
    assert_eq!(repo.explain_failure(&pull, &error).kind, FailureKind::Conflict);
    repo.perform(&Action::AbortRebase).unwrap();
    assert_eq!(team.me.git(&["log", "-1", "--format=%s"]), "Mine");
}

#[cfg(unix)]
#[test]
fn a_failing_pre_push_hook_is_recognized_and_can_be_skipped() {
    use std::os::unix::fs::PermissionsExt;
    let mut team = Team::new();
    team.me.commit("b.txt", "mine\n", "Mine");
    let hook = team.me.path().join(".git/hooks/pre-push");
    std::fs::write(&hook, "#!/bin/sh\necho 'tests failed' >&2\nexit 1\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let repo = Repo::open(team.me.path()).unwrap();
    let error = repo.perform(&push(false)).unwrap_err();
    let failure = repo.explain_failure(&push(false), &error);
    assert_eq!(failure.kind, FailureKind::Hook);
    assert!(failure.output.contains("tests failed"));

    let skip = Action::Push {
        remote: "origin".into(),
        branch: "main".into(),
        upstream: "main".into(),
        set_upstream: false,
        force: false,
        lease: None,
        no_verify: true,
    };
    repo.perform(&skip).unwrap();
    assert_eq!(repo.tracking().unwrap().unwrap().ahead, 0);
}
