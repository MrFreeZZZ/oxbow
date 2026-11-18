#![cfg(unix)]

mod support;

use std::os::unix::fs::PermissionsExt;

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
        "#!/bin/sh\nif [ \"$3\" = merge-base ] && [ \"$4\" = --is-ancestor ]; then\n  echo 'forced ancestry failure' >&2\n  exit 128\nfi\nexec git \"$@\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700)).unwrap();
    config::set_git_program(Some(git));

    let result = repo.stack(None);
    assert!(
        matches!(&result, Err(Error::Command { command, code: Some(128), output })
            if command.starts_with("git merge-base --is-ancestor ") && output.trim() == "forced ancestry failure"),
        "{result:?}"
    );
}
