//! Measure two things the audit of 2026-10-28 found slow, on a repository this makes in a
//! temporary folder: staging one file with Undo kept, beside a bare `git add`, and reading a
//! page of lines of a 15 MB file, as a diff asks for those between hunks.
//! `cargo run --release -p oxbow-core --example steps` (`FILES=5000` for a bigger repository).
//!
//! Stage with Undo is also taken apart by the git processes it starts, as the Console lists
//! them: how many of each kind and how long they ran. The snapshot before the action reads the
//! files on one thread and the rest beside it, so the parts add up to more than the whole.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use oxbow_core::console::{self, Entry, EntryKind};
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

/// The part of Stage with Undo a git process belongs to, from what it runs.
fn part(display: &str, role: Option<&str>) -> &'static str {
    let words: Vec<&str> = display.split(' ').skip(1).collect();
    match words.as_slice() {
        _ if role.is_none() => "the action (git add)",
        ["rev-parse", "--git-path", ..] => "git paths",
        ["add", ..] | ["ls-files", ..] | ["write-tree"] => "index and files",
        ["symbolic-ref", ..] | ["rev-parse", ..] | ["for-each-ref", ..] | ["log", ..] => "HEAD, refs, stash",
        _ => "writing the log",
    }
}

/// Stage one changed file `RUNS` times with Undo kept, each in a Console entry, the repository
/// opened again before each one when `reopen` (as each read of the history does). The median
/// time, and the processes by part: the median of how many a run had, and of their time.
fn stage(
    repo: &Repo,
    path: &Path,
    entries: &Mutex<Vec<Entry>>,
    dir: usize,
    reopen: bool,
) -> (f64, BTreeMap<&'static str, (usize, f64)>) {
    let mut times = Vec::new();
    let mut parts: BTreeMap<&'static str, Vec<(usize, Duration)>> = BTreeMap::new();
    for run in 0..RUNS {
        let repo = if reopen { repo.reopen().unwrap() } else { repo.clone() };
        let file = format!("src/{dir}/file{}.rs", dir + run * 40);
        std::fs::write(path.join(&file), format!("// run {run}\n")).unwrap();
        entries.lock().unwrap().clear();
        let mut took = Duration::ZERO;
        console::scope(Some(path), EntryKind::Action, "Stage", || {
            let started = Instant::now();
            let done = repo.perform(&Action::Stage { paths: vec![file] });
            took = started.elapsed();
            done
        })
        .unwrap();
        times.push(took);
        let mut this: BTreeMap<&'static str, (usize, Duration)> = BTreeMap::new();
        for entry in entries.lock().unwrap().iter() {
            for ran in &entry.commands {
                let slot = this.entry(part(&ran.display, ran.role.as_deref())).or_default();
                slot.0 += 1;
                slot.1 += Duration::from_millis(ran.millis);
            }
        }
        for (name, value) in this {
            parts.entry(name).or_default().push(value);
        }
    }
    let parts = parts
        .into_iter()
        .map(|(name, runs)| {
            // A part a run didn't have counts as none in it.
            let missing = std::iter::repeat_n((0, Duration::ZERO), RUNS - runs.len());
            let (mut counts, times): (Vec<usize>, Vec<Duration>) = runs.into_iter().chain(missing).unzip();
            counts.sort();
            (name, (counts[RUNS / 2], median(times)))
        })
        .collect();
    (median(times), parts)
}

fn print_parts(parts: &BTreeMap<&'static str, (usize, f64)>) {
    for (name, (count, ms)) in parts {
        println!("    {name:22} {count:2} git  {ms:6.1} ms");
    }
    let total: usize = parts.values().map(|p| p.0).sum();
    println!("    {:22} {total:2} git", "all");
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
    std::fs::write(path.join("big.txt"), &big).unwrap();
    git(path, &["add", "-A"]);
    git(path, &["commit", "-q", "-m", "Files"]);
    let repo = Repo::open(path).unwrap();

    let entries = Arc::new(Mutex::new(Vec::new()));
    let kept = entries.clone();
    console::set_listener(move |_, entry| {
        if !entry.running {
            kept.lock().unwrap().push(entry.clone());
        }
    });

    let (logged, parts) = stage(&repo, path, &entries, 0, false);
    let (reopened, reopened_parts) = stage(&repo, path, &entries, 2, true);
    let mut plain = Vec::new();
    for run in 0..RUNS {
        let other = format!("src/1/file{}.rs", 1 + run * 40);
        std::fs::write(path.join(&other), format!("// run {run}\n")).unwrap();
        let started = Instant::now();
        git(path, &["add", "--", &other]);
        plain.push(started.elapsed());
    }
    println!("{files} files, median of {RUNS}");
    println!("  Stage with Undo:  {logged:7.1} ms");
    print_parts(&parts);
    println!("  the same, opened again first (as after a refresh): {reopened:5.1} ms");
    print_parts(&reopened_parts);
    println!("  git add:          {:7.1} ms", median(plain));

    let blob = Source::Blob {
        id: git(path, &["rev-parse", "HEAD:big.txt"]),
    };
    // The file on disk differs from the commit, as a file shown in Changes does, saved a minute
    // ago: lines of a file saved just now aren't kept yet.
    std::fs::write(path.join("big.txt"), format!("changed\n{big}")).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path.join("big.txt"))
        .and_then(|f| f.set_modified(std::time::SystemTime::now() - Duration::from_secs(60)))
        .unwrap();
    let file = Source::Worktree { path: "big.txt".into() };
    for (name, source) in [("committed", &blob), ("on disk", &file)] {
        let started = Instant::now();
        repo.read_lines(source, 1, 20).unwrap();
        println!(
            "  15 MB file {name}, first 20 lines: {:5.1} ms",
            started.elapsed().as_secs_f64() * 1000.0
        );
        for count in [0, 20, 200] {
            let times = (0..RUNS as u32)
                .map(|i| {
                    let started = Instant::now();
                    repo.read_lines(source, 1 + i * 1000, count).unwrap();
                    started.elapsed()
                })
                .collect();
            println!("    then {count:3} lines:        {:6.3} ms", median(times));
        }
    }
}
