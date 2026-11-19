#![cfg(unix)]

mod support;

use std::os::unix::fs::PermissionsExt;

use oxbow_core::{DiffContext, Error, Repo, config};
use support::Fixture;

// The git program setting is process-wide, so this test runs in its own integration binary.
#[test]
fn an_incomplete_binary_sample_is_an_error_for_blame() {
    let mut fx = Fixture::new();
    fx.commit("large.txt", "binary\0", "Small binary");
    let commit = fx.commit("large.txt", &"x".repeat(17 * 1024 * 1024), "Large text");
    let repo = Repo::open(fx.path()).unwrap();
    let diff = || {
        repo.commit_diff(&commit, Some("large.txt"), DiffContext::Compact)
            .unwrap()
    };
    assert!(diff()[0].file.binary);

    let dir = tempfile::tempdir().unwrap();
    config::set_git_program(Some(dir.path().join("missing-git")));
    assert!(matches!(
        repo.file_is_binary(&commit, "large.txt"),
        Err(Error::GitNotFound(_))
    ));

    for (name, response) in [("failed-git", "exit 128"), ("short-git", "printf 'short\\000'")] {
        let git = dir.path().join(name);
        std::fs::write(
            &git,
            format!(
                r#"#!/bin/sh
is_cat_file_blob() {{
  while [ "$#" -ge 2 ]; do
    case "$1" in
      -c|--git-dir) shift 2 ;;
      *) break ;;
    esac
  done
  [ "$1" = cat-file ] && [ "$2" = blob ]
}}
if is_cat_file_blob "$@"; then
  {response}
  exit
fi
exec git "$@"
"#
            ),
        )
        .unwrap();
        std::fs::set_permissions(&git, std::fs::Permissions::from_mode(0o700)).unwrap();
        config::set_git_program(Some(git));

        let result = repo.file_is_binary(&commit, "large.txt");
        assert!(
            matches!(&result, Err(Error::Command { command, output, .. })
                if command.starts_with("git cat-file blob ") && output.contains("first 8000 bytes")),
            "{name}: {result:?}"
        );
        let files = diff();
        assert!(files[0].too_large && !files[0].file.binary, "{name}");
    }
}
