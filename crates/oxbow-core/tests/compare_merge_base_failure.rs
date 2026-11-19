#![cfg(unix)]

mod support;

use std::os::unix::fs::PermissionsExt;

use oxbow_core::{CompareMode, Error, Repo, config};
use support::Fixture;

// The git program setting is process-wide, so this test runs in its own integration binary.
#[test]
fn merge_base_failure_is_reported_for_valid_commits() {
    let mut fx = Fixture::new();
    let base = fx.commit("file.txt", "base\n", "Base");
    let target = fx.commit("file.txt", "target\n", "Target");
    let repo = Repo::open(fx.path()).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let git = dir.path().join("git-wrapper");
    std::fs::write(
        &git,
        "#!/bin/sh\nif [ \"$3\" = merge-base ]; then\n  echo 'forced merge-base failure' >&2\n  exit 128\nfi\nexec git \"$@\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700)).unwrap();
    config::set_git_program(Some(git));
    for mode in [CompareMode::Split, CompareMode::Tips] {
        let result = repo.compare(&base, &target, mode);
        assert!(
            matches!(&result, Err(Error::Command { command, code: Some(128), output })
                if command.starts_with("git merge-base ") && output.trim() == "forced merge-base failure"),
            "{result:?}"
        );
    }
}
