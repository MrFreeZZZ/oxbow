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

    // The history carries the same counts for the toolbar.
    let history = repo.history(&Default::default()).unwrap();
    assert_eq!(history.tracking.unwrap().behind, 1);
    assert_eq!(history.default_remote.as_deref(), Some("origin"));
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
fn pull_conflict_is_recognized_and_can_be_aborted() {
    let mut team = Team::new();
    team.me.commit("a.txt", "mine\n", "Mine");
    team.kirill.commit("a.txt", "his\n", "His");
    team.kirill.git(&["push", "-q", "origin", "main"]);
    let repo = Repo::open(team.me.path()).unwrap();
    let pull = Action::Pull {
        remote: "origin".into(),
        branch: "main".into(),
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
