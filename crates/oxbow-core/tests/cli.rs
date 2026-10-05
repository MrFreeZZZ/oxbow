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
