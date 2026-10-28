mod support;

use oxbow_core::{Action, GitCommand, Repo, TrustKind, trust};
use support::Fixture;

/// A repository set up the way a prepared archive would be: a program for fsmonitor, a filter,
/// an ssh command and a hook, each leaving a file behind in `marks` when it runs.
#[cfg(unix)]
fn prepared(fx: &mut Fixture, marks: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let script = |name: &str| format!("touch '{}'", marks.join(name).display());
    let watch = fx.path().join(".git/watch.sh");
    std::fs::write(&watch, format!("#!/bin/sh\n{}\n", script("fsmonitor"))).unwrap();
    std::fs::set_permissions(&watch, std::fs::Permissions::from_mode(0o755)).unwrap();
    let hook = fx.path().join(".git/hooks/pre-commit");
    std::fs::write(&hook, format!("#!/bin/sh\n{}\n", script("hook"))).unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A sample hook, as `git init` leaves them, runs nothing.
    std::fs::write(fx.path().join(".git/hooks/pre-push.sample"), "#!/bin/sh\n").unwrap();
    fx.git(&["config", "core.fsmonitor", &watch.display().to_string()]);
    fx.git(&["config", "filter.crypt.clean", &format!("{}; cat", script("filter"))]);
    fx.git(&["config", "core.sshCommand", &script("ssh")]);
    fx.write(".gitattributes", "*.secret filter=crypt\n");
}

#[cfg(unix)]
#[test]
fn a_restricted_repository_runs_none_of_its_own_programs() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "First");
    let marks = tempfile::tempdir().unwrap();
    prepared(&mut fx, marks.path());

    let found = trust::scan(fx.path()).unwrap();
    let keys: Vec<(&str, TrustKind)> = found.iter().map(|f| (f.key.as_str(), f.kind)).collect();
    assert!(keys.contains(&("core.fsmonitor", TrustKind::Fsmonitor)), "{keys:?}");
    assert!(keys.contains(&("filter.crypt.clean", TrustKind::Filter)), "{keys:?}");
    assert!(keys.contains(&("core.sshCommand", TrustKind::Ssh)), "{keys:?}");
    assert!(keys.contains(&("hooks/pre-commit", TrustKind::Hook)), "{keys:?}");
    assert!(!keys.iter().any(|(k, _)| k.contains("sample")), "{keys:?}");
    let filter = found.iter().find(|f| f.kind == TrustKind::Filter).unwrap();
    assert_eq!(filter.origin, ".git/config");
    // Reading them ran nothing.
    assert_eq!(std::fs::read_dir(marks.path()).unwrap().count(), 0);

    trust::restrict(fx.path(), &found).unwrap();
    assert!(trust::is_restricted(&fx.path().join("sub/folder")));
    // Looked at again while restricted, it has the same settings and hooks.
    assert_eq!(trust::scan(fx.path()).unwrap(), found);
    let repo = Repo::open(fx.path()).unwrap();
    fx.write("b.secret", "b\n");
    repo.working_tree().unwrap();
    repo.perform(&Action::Stage {
        paths: vec!["b.secret".into()],
    })
    .unwrap();
    repo.perform(&Action::Commit {
        message: "Second".into(),
        amend: false,
        no_verify: false,
        allow_secrets: true,
        skip_checks: false,
        sign_off: false,
    })
    .unwrap();
    let ran: Vec<String> = std::fs::read_dir(marks.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(ran.is_empty(), "ran {ran:?}");
    // The filter was off, not broken: the file went in as it was.
    assert_eq!(fx.git(&["show", "HEAD:b.secret"]), "b");
    // ssh is the user's: it fails to reach a host that isn't there, without the repository's.
    let _ = oxbow_core::cli::run_in(
        fx.path(),
        &GitCommand::new(["ls-remote", "ssh://git@example.invalid/r.git"]),
        None,
    );
    assert!(!marks.path().join("ssh").exists());

    // Trusted, the same repository runs them, as git in a terminal would.
    trust::lift(fx.path());
    assert!(!trust::is_restricted(fx.path()));
    fx.write("c.secret", "c\n");
    repo.working_tree().unwrap();
    repo.perform(&Action::Stage {
        paths: vec!["c.secret".into()],
    })
    .unwrap();
    for mark in ["fsmonitor", "filter"] {
        assert!(marks.path().join(mark).exists(), "{mark} did not run once trusted");
    }
}

#[test]
fn a_repository_without_such_settings_finds_nothing() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "a\n", "First");
    fx.git(&["config", "core.fsmonitor", "false"]);
    fx.git(&["config", "remote.origin.url", "https://example.com/a.git"]);
    assert_eq!(trust::scan(fx.path()).unwrap(), Vec::new());
}

#[test]
fn settings_from_an_included_file_are_found_with_where_they_are() {
    let mut fx = Fixture::new();
    let other = tempfile::tempdir().unwrap();
    let included = other.path().join("team.cfg");
    std::fs::write(&included, "[merge \"ours\"]\n\tdriver = sh -c 'echo hi'\n").unwrap();
    fx.git(&["config", "include.path", &included.display().to_string()]);
    let found = trust::scan(fx.path()).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].kind, TrustKind::MergeDriver);
    assert_eq!(found[0].key, "merge.ours.driver");
    assert_eq!(found[0].origin, included.display().to_string());
}
