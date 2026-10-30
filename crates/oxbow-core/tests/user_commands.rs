#![cfg(unix)]
mod support;
use oxbow_core::{Error, Repo, console, trust, user_commands::UserCommand};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use support::Fixture;

fn command(script: &str) -> UserCommand {
    UserCommand {
        id: "test".into(),
        name: "Run tests".into(),
        script: script.into(),
        repository: String::new(),
    }
}

#[test]
fn context_is_data_and_the_original_repository_is_the_working_directory() {
    let mut fx = Fixture::new();
    let path = fx.path.with_file_name("repo with ' quotes $(touch injected)");
    std::fs::rename(fx.path(), &path).unwrap();
    fx.path = path;
    let repo = Repo::open(fx.path()).unwrap();
    let branch = "feature/'\"; touch injected; #";
    let commit = "$(touch injected) `touch injected`";
    repo.run_user_command(
        &command("printf '%s\\n' \"$OXBOW_REPOSITORY\" \"$OXBOW_BRANCH\" \"$OXBOW_COMMIT\" > context.txt"),
        branch,
        commit,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(fx.path().join("context.txt")).unwrap(),
        format!("{}\n{branch}\n{commit}\n", fx.path().display())
    );
    assert!(!fx.path().join("injected").exists());
    let other = Fixture::new();
    let mut only = command("touch wrong");
    only.repository = other.path().display().to_string();
    assert!(repo.run_user_command(&only, "", "", &AtomicBool::new(false)).is_err());
    assert!(!fx.path().join("wrong").exists());
}

#[test]
fn restricted_and_cancelled_commands_never_start() {
    let fx = Fixture::new();
    let repo = Repo::open(fx.path()).unwrap();
    let c = command("touch ran");
    trust::restrict(fx.path(), &[]).unwrap();
    let result = repo.run_user_command(&c, "", "", &AtomicBool::new(false));
    trust::lift(fx.path());
    assert!(result.is_err());
    assert!(matches!(
        repo.run_user_command(&c, "", "", &AtomicBool::new(true)),
        Err(Error::Cancelled)
    ));
    assert!(!fx.path().join("ran").exists());
}

#[test]
fn console_keeps_exit_code_and_masks_script_and_output() {
    let fx = Fixture::new();
    let repo = Repo::open(fx.path()).unwrap();
    let c = command("printf 'password=hunter2\\n'; exit 7");
    let result = console::scope(Some(fx.path()), console::EntryKind::Action, "Custom: test", || {
        repo.run_user_command(&c, "", "", &AtomicBool::new(false))
    });
    assert!(matches!(result, Err(Error::Command { code: Some(7), .. })));
    let entries = console::entries(Some(fx.path()));
    let entry = entries.last().unwrap();
    assert!(entry.failed);
    assert_eq!(entry.commands.last().unwrap().role, None);
    assert_eq!(entry.commands.last().unwrap().code, Some(7));
    assert!(!serde_json::to_string(entry).unwrap().contains("hunter2"));
}

#[test]
fn stop_ends_a_quiet_process_and_its_children() {
    let fx = Fixture::new();
    let repo = Repo::open(fx.path()).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        signal.store(true, Ordering::Relaxed);
    });
    let started = std::time::Instant::now();
    let result = repo.run_user_command(&command("sleep 30 & wait"), "", "", &cancel);
    thread.join().unwrap();
    assert!(matches!(result, Err(Error::Cancelled)));
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
}
