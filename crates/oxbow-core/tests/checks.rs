mod support;

use oxbow_core::{Action, ActionEvent, CommitCheck, Error, FailureKind, Fix, Repo, TrustKind, trust};
use std::sync::atomic::AtomicBool;
use support::Fixture;

fn commit(skip_checks: bool) -> Action {
    Action::Commit {
        message: "Second".into(),
        amend: false,
        no_verify: false,
        allow_secrets: false,
        skip_checks,
        sign_off: false,
    }
}

fn check(command: &str, on: bool) -> CommitCheck {
    CommitCheck {
        command: command.into(),
        on,
    }
}

/// A repository with one commit and a staged file to commit next.
fn staged() -> Fixture {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "First");
    fx.write("b.txt", "b\n");
    fx.git(&["add", "b.txt"]);
    fx
}

#[test]
fn checks_are_kept_in_the_repository_config_in_order() {
    let mut fx = staged();
    let repo = Repo::open(fx.path()).unwrap();
    assert_eq!(repo.commit_checks().unwrap(), []);

    let checks = [check("ruff check .", true), check("npm test", false), check("  ", true)];
    repo.set_commit_checks(&checks).unwrap();
    assert_eq!(
        fx.git(&["config", "--local", "--get-all", "oxbow.check"]),
        "ruff check .\n# npm test"
    );
    assert_eq!(repo.commit_checks().unwrap(), checks[..2]);

    // Written again whole: the old values go.
    repo.set_commit_checks(&[check("npm test", true)]).unwrap();
    assert_eq!(repo.commit_checks().unwrap(), [check("npm test", true)]);
    repo.set_commit_checks(&[]).unwrap();
    assert_eq!(repo.commit_checks().unwrap(), []);
}

#[cfg(unix)]
#[test]
fn a_failing_check_stops_the_commit_until_skipped() {
    let mut fx = staged();
    let repo = Repo::open(fx.path()).unwrap();
    repo.set_commit_checks(&[
        check("echo lint ok", true),
        check("touch off-ran", false),
        check(
            "echo 'b.txt:1: missing docstring'; echo 'found 1 error' >&2; exit 3",
            true,
        ),
        check("touch after-ran", true),
    ])
    .unwrap();

    let mut shown = Vec::new();
    let err = repo
        .perform_with(
            &commit(false),
            &mut |event| {
                if let ActionEvent::Command { display } = event {
                    shown.push(display);
                }
            },
            &AtomicBool::new(false),
        )
        .unwrap_err();
    let Error::Check(failed) = &err else { panic!("{err}") };
    assert!(failed.command.ends_with("exit 3"), "{}", failed.command);
    assert_eq!(failed.code, Some(3));
    // What it printed on both streams; two pipes, so their lines may come in either order.
    let mut lines: Vec<&str> = failed.output.lines().collect();
    lines.sort();
    assert_eq!(lines, ["b.txt:1: missing docstring", "found 1 error"]);
    assert_eq!(shown.len(), 2, "{shown:?}");
    assert!(shown[0].ends_with("'echo lint ok'"), "{shown:?}");
    assert!(!fx.path().join("off-ran").exists(), "a check turned off runs not");
    assert!(
        !fx.path().join("after-ran").exists(),
        "the first failure stops the rest"
    );
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "First", "nothing was committed");
    assert_eq!(fx.git(&["diff", "--cached", "--name-only"]), "b.txt", "still staged");

    let failure = repo.explain_failure(&commit(false), &err);
    assert_eq!(failure.kind, FailureKind::Check);
    assert_eq!(failure.check.as_deref(), Some(&failed.command));
    assert_eq!(failure.output, failed.output);

    repo.perform(&commit(true)).unwrap();
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Second");
    assert!(!fx.path().join("after-ran").exists());
}

#[cfg(unix)]
#[test]
fn checks_that_pass_let_the_commit_through_from_the_top_folder() {
    let mut fx = staged();
    let repo = Repo::open(fx.path()).unwrap();
    repo.set_commit_checks(&[check("pwd > where.txt", true), check("test -f b.txt", true)])
        .unwrap();
    // The sheet shows them before the commit, unless they are skipped.
    let shown = |action: &Action| repo.plan(action).unwrap().commands[0].before.clone();
    assert_eq!(
        shown(&commit(false)).as_deref(),
        Some("pwd > where.txt && test -f b.txt")
    );
    assert_eq!(shown(&commit(true)), None);
    repo.perform(&commit(false)).unwrap();
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Second");
    let where_ = std::fs::read_to_string(fx.path().join("where.txt")).unwrap();
    assert_eq!(
        std::path::Path::new(where_.trim()).canonicalize().unwrap(),
        fx.path().canonicalize().unwrap()
    );
}

#[cfg(unix)]
#[test]
fn stop_ends_a_running_check() {
    let fx = staged();
    let repo = Repo::open(fx.path()).unwrap();
    repo.set_commit_checks(&[check("sleep 30", true)]).unwrap();
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let stop = cancel.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    let started = std::time::Instant::now();
    let err = repo.perform_with(&commit(false), &mut |_| {}, &cancel).unwrap_err();
    stopper.join().unwrap();
    assert!(matches!(err, Error::Cancelled), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}

#[cfg(unix)]
#[test]
fn a_restricted_repository_runs_no_checks() {
    let mut fx = staged();
    let repo = Repo::open(fx.path()).unwrap();
    repo.set_commit_checks(&[check("touch ran; exit 1", true), check("touch off", false)])
        .unwrap();

    // A check that runs is a setting that runs a program; one turned off is not.
    let found = trust::scan(fx.path()).unwrap();
    let checks: Vec<_> = found.iter().filter(|f| f.kind == TrustKind::Check).collect();
    assert_eq!(checks.len(), 1, "{found:?}");
    assert_eq!(
        (checks[0].key.as_str(), checks[0].value.as_str()),
        ("oxbow.check", "touch ran; exit 1")
    );

    trust::restrict(fx.path(), &found).unwrap();
    let shown = repo.plan(&commit(false)).map(|p| p.commands[0].before.clone());
    let done = repo.perform(&commit(false));
    trust::lift(fx.path());
    done.unwrap();
    assert_eq!(shown.unwrap(), None, "none is shown either");
    assert!(!fx.path().join("ran").exists());
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Second");
}

#[cfg(unix)]
#[test]
fn adding_to_an_older_commit_and_to_the_last_one_runs_the_checks_too() {
    let mut fx = Fixture::new();
    fx.commit("r.txt", "r\n", "Root");
    fx.commit("a.txt", "a\n", "First");
    let first = fx.git(&["rev-parse", "HEAD"]);
    fx.commit("b.txt", "b\n", "Second");
    let top = fx.git(&["rev-parse", "HEAD"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.set_commit_checks(&[check("exit 1", true)]).unwrap();

    fx.write("c.txt", "c\n");
    fx.git(&["add", "c.txt"]);
    let add = |skip_checks| Action::AddToCommit {
        commit: first.clone(),
        allow_secrets: false,
        skip_checks,
    };
    assert_eq!(
        repo.plan(&add(false)).unwrap().commands[0].before.as_deref(),
        Some("exit 1")
    );
    let err = repo.perform(&add(false)).unwrap_err();
    assert!(matches!(err, Error::Check(_)), "{err}");
    assert_eq!(fx.git(&["rev-parse", "HEAD"]), top, "nothing was rewritten");
    repo.perform(&add(true)).unwrap();
    assert_eq!(fx.git(&["show", "--format=", "--name-only", "HEAD~1"]), "a.txt\nc.txt");

    fx.write("d.txt", "d\n");
    let last = |skip_checks| Action::Fix {
        fix: Fix::AddToLast {
            paths: vec!["d.txt".into()],
            allow_secrets: false,
            skip_checks,
        },
        expect: repo.fix_state().unwrap().expect,
    };
    let err = repo.perform(&last(false)).unwrap_err();
    assert!(matches!(err, Error::Check(_)), "{err}");
    assert_eq!(fx.git(&["show", "--format=", "--name-only", "HEAD"]), "b.txt");
    repo.perform(&last(true)).unwrap();
    assert_eq!(fx.git(&["show", "--format=", "--name-only", "HEAD"]), "b.txt\nd.txt");
}
