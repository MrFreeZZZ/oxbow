mod support;

use oxbow_core::{Error, GitCommand, Repo};
use support::Fixture;

#[test]
fn runs_git_in_the_working_directory() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    fx.write("b.txt", "new\n");
    let repo = Repo::open(fx.path()).unwrap();

    let out = repo.run(&GitCommand::new(["status", "--porcelain"])).unwrap();
    assert_eq!(out.stdout, "?? b.txt\n");
}

#[test]
fn failures_carry_the_command_and_its_message() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "1\n", "Root");
    let repo = Repo::open(fx.path()).unwrap();

    let err = repo.run(&GitCommand::new(["switch", "no-such-branch"])).unwrap_err();
    match err {
        Error::Command { command, code, output } => {
            assert_eq!(command, "git switch no-such-branch");
            assert_eq!(code, Some(128));
            assert!(output.contains("invalid reference"), "{output}");
        }
        other => panic!("unexpected error {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn temp_folders_and_their_files_are_closed_to_other_users_from_the_start() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;

    let shared = oxbow_core::cli::private_temp_dir().unwrap();
    assert_eq!(mode(&shared), 0o700);
    let versions = shared.join("versions-test");
    oxbow_core::cli::create_private_dir(&versions).unwrap();
    assert_eq!(mode(&versions), 0o700);
    let file = versions.join("logo (old a3f9c21).png");
    oxbow_core::cli::write_private(&file, b"data").unwrap();
    assert_eq!(mode(&file), 0o600);
    std::fs::remove_dir_all(&versions).unwrap();

    let merge = oxbow_core::cli::private_temp_folder("oxbow-merge-").unwrap();
    assert_eq!(mode(merge.path()), 0o700);
}
