mod support;

use std::sync::atomic::AtomicBool;

use oxbow_core::setup::{self, CloneOptions, NewRepoOptions};
use oxbow_core::{OperationKind, Repo};
use support::Fixture;

fn identity() {
    // A machine without ~/.gitconfig can still make the first commit in these tests.
    // SAFETY: set before any thread of this test binary runs git.
    unsafe {
        for (key, value) in [
            ("GIT_AUTHOR_NAME", "Alexander"),
            ("GIT_AUTHOR_EMAIL", "alexander@example.com"),
            ("GIT_COMMITTER_NAME", "Alexander"),
            ("GIT_COMMITTER_EMAIL", "alexander@example.com"),
        ] {
            std::env::set_var(key, value);
        }
    }
}

#[test]
fn new_repository_in_a_new_folder_gets_starter_files_and_a_first_commit() {
    identity();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Developer").join("inventory-sync");
    let options = NewRepoOptions {
        path: path.clone(),
        branch: "main".into(),
        readme: true,
        gitignore: Some("rust".into()),
        license: Some("mit".into()),
        commit: true,
    };
    let plan = setup::plan_new_repo(&options).unwrap();
    assert!(!plan.exists);
    assert_eq!(plan.writes, ["README.md", ".gitignore", "LICENSE"]);
    assert_eq!(
        plan.commands.iter().map(|c| c.display()).collect::<Vec<_>>(),
        [
            format!("git init -b main {}", path.display()),
            "git add --all".into(),
            "git commit -m 'Initial commit'".into(),
        ]
    );

    setup::create_repo(&options).unwrap();
    let repo = Repo::open(&path).unwrap();
    assert_eq!(repo.head().unwrap().branch.as_deref(), Some("main"));
    assert_eq!(
        std::fs::read_to_string(path.join("README.md")).unwrap(),
        "# inventory-sync\n"
    );
    let ignore = std::fs::read_to_string(path.join(".gitignore")).unwrap();
    assert!(ignore.starts_with("# Rust\n") && ignore.contains("target") && ignore.contains(".DS_Store"));
    let license = std::fs::read_to_string(path.join("LICENSE")).unwrap();
    assert!(license.starts_with("MIT License") && !license.contains("[year]"));
    let glance = setup::glance(&path).unwrap();
    assert_eq!(
        (glance.branch.as_deref(), glance.changed, glance.upstream),
        (Some("main"), 0, false)
    );

    // Now it is a repository: the sheet offers to open it instead.
    let again = setup::plan_new_repo(&options).unwrap();
    assert_eq!(again.repository.as_deref(), Some(path.to_str().unwrap()));
    assert!(setup::create_repo(&options).is_err());
}

#[test]
fn new_repository_in_a_folder_with_files_keeps_them() {
    identity();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes");
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join(".gitignore"), "*.tmp\n").unwrap();
    std::fs::write(path.join("todo.md"), "- ship\n").unwrap();
    let options = NewRepoOptions {
        path: path.clone(),
        branch: "trunk".into(),
        readme: true,
        gitignore: Some("node".into()),
        license: None,
        commit: false,
    };
    let plan = setup::plan_new_repo(&options).unwrap();
    assert_eq!((plan.exists, plan.entries), (true, 2));
    // No README in a folder that has files; its own .gitignore stays.
    assert!(plan.writes.is_empty());
    assert_eq!(plan.kept, [".gitignore"]);
    assert_eq!(plan.commands[0].display(), "git init -b trunk");

    setup::create_repo(&options).unwrap();
    assert_eq!(std::fs::read_to_string(path.join(".gitignore")).unwrap(), "*.tmp\n");
    let glance = setup::glance(&path).unwrap();
    assert_eq!((glance.branch.as_deref(), glance.changed), (Some("trunk"), 2));
}

#[test]
fn bad_branch_names_are_refused() {
    let options = |branch: &str| NewRepoOptions {
        path: std::env::temp_dir().join("never-made"),
        branch: branch.into(),
        readme: false,
        gitignore: None,
        license: None,
        commit: false,
    };
    for bad in ["", "-x", "a b", "a..b", "main.lock", "x/"] {
        assert!(setup::plan_new_repo(&options(bad)).is_err(), "{bad:?}");
    }
    assert!(setup::plan_new_repo(&options("feature/x")).is_ok());
}

#[test]
fn clone_probe_and_glance() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "First");
    fx.git(&["switch", "-q", "-c", "topic"]);
    fx.commit("b.txt", "two\n", "Second");
    fx.git(&["switch", "-q", "main"]);
    let url = fx.path().display().to_string();

    let probe = setup::probe_remote(&url).unwrap();
    assert_eq!(probe.transport, "local");
    assert_eq!(probe.default_branch.as_deref(), Some("main"));
    assert_eq!(probe.branches, 2);
    assert!(setup::probe_remote(&format!("{url}-missing")).is_err());

    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("copy");
    let options = CloneOptions {
        url: url.clone(),
        path: target.clone(),
        submodules: true,
        shallow: false,
    };
    assert_eq!(
        setup::clone_command(&options).unwrap().display(),
        format!("git clone --recurse-submodules {url} {}", target.display())
    );
    let mut lines = Vec::new();
    setup::clone(&options, &mut |line| lines.push(line.text), &AtomicBool::new(false)).unwrap();
    assert!(lines.iter().any(|l| l.starts_with("Cloning into")));
    let glance = setup::glance(&target).unwrap();
    assert_eq!(glance.branch.as_deref(), Some("main"));
    assert!(glance.upstream && glance.ahead == 0 && glance.behind == 0);

    // The folder is taken now; a failed clone leaves nothing behind.
    assert!(setup::clone(&options, &mut |_| {}, &AtomicBool::new(false)).is_err());
    let failed = CloneOptions {
        url: format!("{url}-missing"),
        path: dir.path().join("nothing"),
        submodules: false,
        shallow: true,
    };
    assert!(setup::clone(&failed, &mut |_| {}, &AtomicBool::new(false)).is_err());
    assert!(!dir.path().join("nothing").exists());
}

#[test]
fn glance_sees_a_merge_that_stopped() {
    let mut fx = Fixture::new();
    fx.commit("a.txt", "one\n", "First");
    fx.git(&["switch", "-q", "-c", "topic"]);
    fx.commit("a.txt", "topic\n", "Topic");
    fx.git(&["switch", "-q", "main"]);
    fx.commit("a.txt", "main\n", "Main");
    let _ = std::process::Command::new("git")
        .args(["merge", "topic"])
        .current_dir(fx.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .output();
    let glance = setup::glance(fx.path()).unwrap();
    assert_eq!(glance.operation, Some(OperationKind::Merge));
    assert_eq!(glance.conflicts, 1);
}
