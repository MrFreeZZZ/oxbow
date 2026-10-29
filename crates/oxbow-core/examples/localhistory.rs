//! Measure what Local History costs on a real repository: a day of saves to 20 files, a formatter
//! run over every file, and a branch switch writing every file back, in disk and in time; then
//! letting the old versions go. It edits the working copy, so it asks for one with no changes
//! and puts every file back after (`git checkout -- .`), and deletes the versions it kept.
//! `cargo run --release -p oxbow-core --example localhistory -- <repository>`

use std::path::Path;
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use oxbow_core::localhistory::{GROUP, MAX_FILE};
use oxbow_core::{LocalHistory, Repo};

/// Saves of each of the 20 files in the day.
const SAVES: usize = 50;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git runs");
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn size_of(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| match e.metadata() {
            Ok(m) if m.is_dir() => size_of(&e.path()),
            Ok(m) => m.len(),
            Err(_) => 0,
        })
        .sum()
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1024.0 / 1024.0)
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: localhistory <repository>");
    let path = Path::new(&path);
    assert!(
        git(path, &["status", "--porcelain", "--untracked-files=no"]).is_empty(),
        "the repository has changes: run it on a clean clone"
    );
    let repo = Repo::open(path).unwrap();
    let store = path.join(".git/oxbow/local-history");
    let _ = std::fs::remove_dir_all(&store);
    let mut history = LocalHistory::open(&repo);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;

    let files: Vec<String> = git(path, &["ls-files", "-z"])
        .split('\0')
        .filter(|f| !f.is_empty())
        .map(str::to_owned)
        .filter(|f| std::fs::metadata(path.join(f)).is_ok_and(|m| m.is_file() && m.len() <= MAX_FILE))
        .collect();
    // Source files of a usual size, 5 to 100 KB.
    let edited: Vec<&String> = files
        .iter()
        .filter(|f| std::fs::metadata(path.join(f)).is_ok_and(|m| (5 * 1024..=100 * 1024).contains(&m.len())))
        .step_by(97)
        .take(20)
        .collect();
    let start = Instant::now();
    for save in 0..SAVES {
        for file in &edited {
            let full = path.join(file);
            let mut text = std::fs::read(&full).unwrap();
            text.extend_from_slice(format!("\n// edit {save}").as_bytes());
            std::fs::write(&full, text).unwrap();
        }
        let told: Vec<String> = edited.iter().map(|f| f.to_string()).collect();
        history.keep(&repo, &told, now - 3 * 60 * 60 * 1000 + save as u64 * GROUP * 2);
    }
    let day = size_of(&store);
    println!(
        "a day of {SAVES} saves to {} files: {} on disk, {:.1} ms a save of all of them",
        edited.len(),
        mb(day),
        start.elapsed().as_secs_f64() * 1000.0 / SAVES as f64
    );

    let start = Instant::now();
    for file in &files {
        let full = path.join(file);
        let mut text = std::fs::read(&full).unwrap();
        text.extend_from_slice(b"\n");
        std::fs::write(&full, text).unwrap();
    }
    let wrote = start.elapsed();
    let start = Instant::now();
    let kept = history.keep(&repo, &files, now - 60 * 60 * 1000);
    println!(
        "a formatter over {} files: {kept} versions, {} more on disk, kept in {:.1} s (writing the files took {:.1} s)",
        files.len(),
        mb(size_of(&store) - day),
        start.elapsed().as_secs_f64(),
        wrote.as_secs_f64()
    );

    let before = size_of(&store);
    git(path, &["checkout", "--", "."]);
    let start = Instant::now();
    let kept = history.keep(&repo, &files, now);
    println!(
        "a branch switch writing every file back: {kept} versions, {} more on disk, {:.1} s",
        mb(size_of(&store) - before),
        start.elapsed().as_secs_f64()
    );

    let start = Instant::now();
    let total = history.prune(now).unwrap();
    println!(
        "letting old versions go: {:.0} ms, blobs {}, the lists {}",
        start.elapsed().as_secs_f64() * 1000.0,
        mb(total),
        mb(size_of(&store) - size_of(&store.join("objects")))
    );
    let start = Instant::now();
    let all = repo.local_history_all().unwrap();
    let one = repo.local_history(edited[0]).unwrap();
    println!(
        "reading the lists: every file {} versions, one file {} versions, {:.0} ms",
        all.len(),
        one.len(),
        start.elapsed().as_secs_f64() * 1000.0
    );
    let _ = std::fs::remove_dir_all(&store);
}
