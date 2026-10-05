#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command;

/// A throwaway repository driven through the `git` command line.
pub struct Fixture {
    _dir: tempfile::TempDir,
    pub path: PathBuf,
    clock: u32,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("repo");
        std::fs::create_dir(&path).unwrap();
        let mut fixture = Fixture {
            _dir: dir,
            path,
            clock: 0,
        };
        fixture.git(&["init", "-q", "-b", "main"]);
        // Commits made through `Repo` use the repository's own identity, like a configured machine.
        fixture.git(&["config", "user.name", "Alexander"]);
        fixture.git(&["config", "user.email", "alexander@example.com"]);
        fixture
    }

    /// Run git in the repository with a fixed identity and a clock that moves forward per call.
    pub fn git(&mut self, args: &[&str]) -> String {
        self.clock += 60;
        let date = format!("{} +0000", 1_700_000_000 + self.clock);
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.path)
            .env("GIT_AUTHOR_NAME", "Alexander")
            .env("GIT_AUTHOR_EMAIL", "alexander@example.com")
            .env("GIT_COMMITTER_NAME", "Alexander")
            .env("GIT_COMMITTER_EMAIL", "alexander@example.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    pub fn write(&self, file: &str, content: &str) {
        let path = self.path.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    /// Write a file and commit it with `message`. Returns the new commit id.
    pub fn commit(&mut self, file: &str, content: &str, message: &str) -> String {
        self.write(file, content);
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
        self.git(&["rev-parse", "HEAD"])
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
