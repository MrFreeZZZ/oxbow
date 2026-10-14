mod support;

use oxbow_core::{Action, Error, FailureKind, Repo, Side};
use support::Fixture;

fn commit(allow_secrets: bool) -> Action {
    Action::Commit {
        message: "Add deploy settings".into(),
        amend: false,
        no_verify: false,
        allow_secrets,
    }
}

#[test]
fn a_commit_with_a_secret_stops_until_allowed() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "First");
    fx.write(
        "deploy.env",
        // A fake key, split so that secret scanners don't take this file for a leak.
        concat!("REGION=eu-central-1\nAWS_ACCESS_KEY_ID=AKIA", "Q7XKZ3M4P2LJ6WVB\n"),
    );
    fx.git(&["add", "deploy.env"]);
    let repo = Repo::open(fx.path()).unwrap();

    // The staged diff marks the line too.
    let diff = repo.working_diff("deploy.env", Side::Staged, false).unwrap();
    let marked: Vec<_> = diff.hunks[0].lines.iter().filter_map(|l| l.secret.clone()).collect();
    assert_eq!(marked, ["AWS access key"]);

    let err = repo.perform(&commit(false)).unwrap_err();
    let Error::Secrets(found) = &err else { panic!("{err}") };
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].path.as_str(), found[0].line), ("deploy.env", 2));
    assert!(!found[0].masked.contains("Q7XKZ3M4P2LJ6"), "{}", found[0].masked);
    let failure = repo.explain_failure(&commit(false), &err);
    assert_eq!(failure.kind, FailureKind::Secrets);
    assert_eq!(failure.secrets.as_deref().map(Vec::len), Some(1));
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "First", "nothing was committed");

    repo.perform(&commit(true)).unwrap();
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Add deploy settings");
}

#[test]
fn an_allowed_line_commits() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "First");
    fx.write("test.py", "API_KEY = \"a8f3k29dk3m1\"  # oxbow:allow\n");
    fx.git(&["add", "test.py"]);
    let repo = Repo::open(fx.path()).unwrap();
    repo.perform(&commit(false)).unwrap();
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]), "Add deploy settings");
}
