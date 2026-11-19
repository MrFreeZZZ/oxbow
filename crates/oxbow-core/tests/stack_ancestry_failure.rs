#![cfg(unix)]

mod support;

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use oxbow_core::{Error, Repo, config};
use support::Fixture;

// The git program setting is process-wide, so this test runs in its own integration binary.
#[test]
fn edit_stack_stops_when_git_cant_tell_whether_the_branch_is_on_the_trunk() {
    let mut fx = Fixture::new();
    fx.commit("readme.md", "hi\n", "Root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.commit("feature.txt", "work\n", "Feature");
    let repo = Repo::open(fx.path()).unwrap();
    let stack = repo.stack(None).unwrap();
    assert_eq!(stack.head.as_deref(), Some("feature"));
    assert_eq!(stack.trunk, "main");

    let dir = tempfile::tempdir().unwrap();
    let git = dir.path().join("git-wrapper");
    std::fs::write(
        &git,
        r#"#!/bin/sh
is_ancestry_check() {
  while [ "$#" -ge 2 ] && [ "$1" = -c ]; do
    shift 2
  done
  [ "$1" = merge-base ] && [ "$2" = --is-ancestor ]
}
if is_ancestry_check "$@"; then
  echo 'forced ancestry failure' >&2
  exit 128
fi
exec git "$@"
"#,
    )
    .unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700)).unwrap();

    let wrapper = || {
        let mut command = Command::new(&git);
        command
            .current_dir(fx.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null");
        command
    };
    let options = ["-c", "oxbow.test=first value", "-c", "oxbow.test=second value"];
    for prefix in [&options[..2], &options[..]] {
        let output = wrapper()
            .args(prefix)
            .args(["merge-base", "--is-ancestor", "main", "feature"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(128), "{prefix:?}: {output:?}");
        assert_eq!(output.stderr, b"forced ancestry failure\n");
    }
    let output = wrapper()
        .args(options)
        .args(["config", "--get-all", "oxbow.test"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"first value\nsecond value\n");

    config::set_git_program(Some(git));

    let result = repo.stack(None);
    assert!(
        matches!(&result, Err(Error::Command { command, code: Some(128), output })
            if command.starts_with("git merge-base --is-ancestor ") && output.trim() == "forced ancestry failure"),
        "{result:?}"
    );
}
