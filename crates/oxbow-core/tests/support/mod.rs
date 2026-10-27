#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

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

/// A stand-in for GitHub's API answering each request by its path: the first route whose
/// prefix matches, else 404. A route `"/graphql markFileAsViewed"` also needs its words in the body.
pub fn server(routes: Vec<(&'static str, &'static str)>) -> String {
    stand_in(routes).0
}

/// Each request a stand-in got: its path and body.
pub type Seen = Arc<Mutex<Vec<(String, String)>>>;

/// [`server`], also keeping each request's path and body as they came.
pub fn stand_in(routes: Vec<(&'static str, &'static str)>) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let kept = seen.clone();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { break };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let mut length = 0;
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header.trim().is_empty() {
                    break;
                }
                if let Some(value) = header.to_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            let body = String::from_utf8_lossy(&body).into_owned();
            let path = line.split_whitespace().nth(1).unwrap_or("").to_owned();
            kept.lock().unwrap().push((path.clone(), body.clone()));
            let (status, answer) = routes
                .iter()
                .find(|(route, _)| {
                    let (prefix, words) = route.split_once(' ').unwrap_or((route, ""));
                    path.starts_with(prefix) && body.contains(words)
                })
                .map_or((404, r#"{"message":"Not Found"}"#), |(_, answer)| (200, answer));
            let mut stream = stream;
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            );
        }
    });
    (address, seen)
}
