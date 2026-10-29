mod support;

use oxbow_core::{Action, Error, FailureKind, Repo, Side};
use support::Fixture;

fn commit(allow_secrets: bool) -> Action {
    Action::Commit {
        message: "Add deploy settings".into(),
        amend: false,
        no_verify: false,
        allow_secrets,
        skip_checks: false,
        sign_off: false,
        changelist: None,
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

#[test]
fn the_console_keeps_no_secret() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "First");
    // Fake keys, split so that secret scanners don't take this file for a leak.
    let secrets = [
        concat!("sk-proj-", "Q7XKZ3M4P2LJ6WVBq7xkz3m4p2lj6wvbQ7XK"),
        concat!("AKIA", "Q7XKZ3M4P2LJ6WVB"),
        concat!("sk_", "live_4eC39HqLyjWDarjtT1zdp7dc"),
        "audit-password-123",
        "a8f3k29dk3m1xq",
        "MIIEowIBAAKCAQEAu1SU1LfVLPHCozMxH2Mo4lgOEePzNm0tRgeLezV6ffAt0gun",
        "acme_0badf00d",
    ];
    oxbow_core::set_secrets_check(
        true,
        &[oxbow_core::SecretPattern {
            kind: oxbow_core::PatternKind::Text,
            pattern: r"acme_[0-9a-f]{8}".into(),
        }],
    );
    fx.write(
        "deploy.env",
        &format!(
            "OPENAI = {}\nAWS_ACCESS_KEY_ID={}\nstripe: '{}'\nPASSWORD = \"{}\"\nAPI_KEY=\"{}\"\n\
             -----BEGIN RSA PRIVATE KEY-----\n{}\n-----END RSA PRIVATE KEY-----\nid = {}\n",
            secrets[0], secrets[1], secrets[2], secrets[3], secrets[4], secrets[5], secrets[6]
        ),
    );
    // What the check before a commit lets through is still masked in the log.
    fx.write(
        "notes.txt",
        &format!(
            "{aws} # oxbow:allow\n{aws} {long}\n{nine}\n",
            aws = secrets[1],
            long = "x".repeat(4100),
            nine = [secrets[1]; 9].join(" ")
        ),
    );
    fx.git(&["add", "deploy.env", "notes.txt"]);
    let repo = Repo::open(fx.path()).unwrap();
    let stopped: Result<String, Error> = oxbow_core::console::scope(
        Some(fx.path()),
        oxbow_core::console::EntryKind::Action,
        "Commit",
        || repo.perform(&commit(false)),
    );
    assert!(matches!(stopped, Err(Error::Secrets(_))));
    // Other commands that print the file are masked line by line.
    repo.run(&oxbow_core::GitCommand::new(["diff", "--cached"])).unwrap();
    repo.run(&oxbow_core::GitCommand::new(["show", ":deploy.env"])).unwrap();
    repo.run(&oxbow_core::GitCommand::new(["show", ":notes.txt"])).unwrap();

    let log = std::fs::read_to_string(fx.path().join(".git/oxbow/console.jsonl")).unwrap();
    assert!(log.contains("secrets check"), "{log}");
    assert!(log.contains("not kept"), "{log}");
    for secret in secrets {
        assert!(!log.contains(secret), "{secret} is in the log:\n{log}");
    }
    oxbow_core::set_secrets_check(true, &[]);
}

#[test]
fn every_staged_file_is_checked_whatever_its_name() {
    // Git quotes names with a tab, a line break, a quote or letters outside ASCII, and puts a
    // tab after one with a space.
    let names = [
        "tab\there.env",
        "line\nbreak.env",
        "quote\"d.env",
        "back\\slash.env",
        "ключи.env",
        "my keys.env",
    ];
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "First");
    // An added line `++ x` must not end the file early, and diff.noprefix must not hide paths.
    fx.git(&["config", "diff.noprefix", "true"]);
    for name in names {
        fx.write(name, concat!("++ notes\nAWS_ACCESS_KEY_ID=AKIA", "Q7XKZ3M4P2LJ6WVB\n"));
    }
    fx.git(&["add", "."]);
    let repo = Repo::open(fx.path()).unwrap();

    let found = repo.staged_secrets().unwrap();
    let mut paths: Vec<_> = found.iter().map(|f| (f.path.as_str(), f.line)).collect();
    paths.sort();
    let mut expected: Vec<_> = names.iter().map(|n| (*n, 2)).collect();
    expected.sort();
    assert_eq!(paths, expected);
}
