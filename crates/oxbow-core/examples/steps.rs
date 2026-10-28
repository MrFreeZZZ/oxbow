//! Measure two things the audit of 2026-10-28 found slow, on a repository this makes in a
//! temporary folder: staging one file with Undo kept, beside a bare `git add`, and reading a
//! page of lines of a 15 MB file, as a diff asks for those between hunks.
//! `cargo run --release -p oxbow-core --example steps` (`FILES=5000` for a bigger repository).

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use oxbow_core::{Action, Repo, Source};

const RUNS: usize = 15;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-c", "user.name=Oxbow", "-c", "user.email=oxbow@example.com"])
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn median(mut times: Vec<Duration>) -> f64 {
    times.sort();
    times[times.len() / 2].as_secs_f64() * 1000.0
}

fn main() {
    let files: usize = std::env::var("FILES").ok().and_then(|n| n.parse().ok()).unwrap_or(2000);
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path();
    git(path, &["init", "-q", "-b", "main"]);
    for i in 0..files {
        let sub = path.join(format!("src/{}", i % 40));
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join(format!("file{i}.rs")), format!("fn f{i}() {{}}\n").repeat(20)).unwrap();
    }
    let big: String = (0..300_000).map(|i| format!("line {i} {}\n", "x".repeat(40))).collect();
    std::fs::write(path.join("big.txt"), big).unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-q", "-m", "Files"]);
    let repo = Repo::open(path).unwrap();

    let mut logged = Vec::new();
    let mut plain = Vec::new();
    for run in 0..RUNS {
        let file = format!("src/0/file{}.rs", run * 40);
        std::fs::write(path.join(&file), format!("// run {run}\n")).unwrap();
        let started = Instant::now();
        repo.perform(&Action::Stage { paths: vec![file] }).unwrap();
        logged.push(started.elapsed());

        let other = format!("src/1/file{}.rs", 1 + run * 40);
        std::fs::write(path.join(&other), format!("// run {run}\n")).unwrap();
        let started = Instant::now();
        git(path, &["add", "--", &other]);
        plain.push(started.elapsed());
    }
    println!("{files} files, median of {RUNS}");
    println!("  Stage with Undo:  {:7.1} ms", median(logged));
    println!("  git add:          {:7.1} ms", median(plain));

    let source = Source::Blob {
        id: git(path, &["rev-parse", "HEAD:big.txt"]),
    };
    let started = Instant::now();
    repo.read_lines(&source, 1, 20).unwrap();
    println!(
        "  first 20 lines of 15 MB: {:5.1} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    for count in [0, 20, 200] {
        let times = (0..RUNS as u32)
            .map(|i| {
                let started = Instant::now();
                repo.read_lines(&source, 1 + i * 1000, count).unwrap();
                started.elapsed()
            })
            .collect();
        println!("  then {count:3} lines:        {:5.1} ms", median(times));
    }
}
