//! Measure what opening and refreshing a repository costs, as a window does it: the history,
//! then what the other worktrees and the submodules' folders have. On the fixtures of
//! `scripts/make-perf-fixtures.sh` (see docs/measuring-reads.md):
//! `cargo run --release -p oxbow-core --example reads -- <path>...`
//! `RUNS=30` repeats each step (default 15); `SLOW=2` makes `git status` in every other folder
//! take 2 seconds longer, to see whether History waits for it; `AT_ONCE=1,2,4,8` compares how
//! many folders are read side by side (default: Oxbow's own).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use oxbow_core::folders::AT_ONCE;
use oxbow_core::{FolderCache, History, HistoryCache, HistoryOptions, Repo};

/// What one load cost.
struct Load {
    history: History,
    /// Until the history could show.
    shown: Duration,
    /// Until every folder's state was in.
    all: Duration,
}

/// Load the history with `cache`, then read the folders it left pending with `folders`, the way
/// the app does after a reload or a refresh.
fn load(
    repo: &Repo,
    cache: &mut HistoryCache,
    folders: &FolderCache,
    known: Option<&str>,
    since: Option<SystemTime>,
    at_once: usize,
) -> Result<Load, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let history = repo.history_with(&HistoryOptions::default(), cache, known)?;
    let shown = start.elapsed();
    let cancel = Arc::new(AtomicBool::new(false));
    repo.folder_states(
        &history.worktrees,
        &history.submodules,
        folders,
        since,
        at_once,
        &cancel,
        &mut |_| {},
    )?;
    Ok(Load {
        history,
        shown,
        all: start.elapsed(),
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runs: usize = std::env::var("RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(15);
    let slow: Option<u64> = std::env::var("SLOW").ok().and_then(|v| v.parse().ok());
    let at_once: Vec<usize> = std::env::var("AT_ONCE")
        .ok()
        .map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .filter(|v: &Vec<usize>| !v.is_empty())
        .unwrap_or_else(|| vec![AT_ONCE]);
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        return Err("usage: reads <repository>...".into());
    }
    let git = real_git()?;
    let scratch = std::env::temp_dir().join(format!("oxbow-reads-{}", std::process::id()));
    std::fs::create_dir_all(&scratch)?;
    println!("git {} · {runs} runs · median / p95", git_version(&git));
    for path in &paths {
        let repo = Repo::open(path)?;
        println!("\n{} ({path})", repo.name());

        // Times, with git as it is.
        oxbow_core::config::set_git_program(None);
        for &n in &at_once {
            let (mut shown, mut all, mut again, mut afresh) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
            let mut size = 0;
            for _ in 0..runs {
                let repo = Repo::open(path)?;
                let mut cache = HistoryCache::default();
                let folders = FolderCache::default();
                let first = load(&repo, &mut cache, &folders, None, None, n)?;
                shown.push(first.shown);
                all.push(first.all);
                size = serde_json::to_vec(&first.history)?.len();
                let key = first.history.key.clone();
                again.push(load(&repo, &mut cache, &folders, Some(&key), None, n)?.all);
                // The window came back to the front: every folder is read again.
                afresh.push(load(&repo, &mut cache, &folders, Some(&key), Some(SystemTime::now()), n)?.all);
            }
            let at = if at_once.len() > 1 {
                format!(" ({n} at once)")
            } else {
                String::new()
            };
            line(&format!("History shown{at}"), &shown);
            line(&format!("All statuses in{at}"), &all);
            line(&format!("Refresh, nothing changed{at}"), &again);
            line(&format!("Refresh, back to front{at}"), &afresh);
            if n == at_once[0] {
                println!("  {:<34} {}", "History answer (JSON)", bytes(size));
            }
        }

        // Processes, with git wrapped to log when each one starts and ends.
        let log = scratch.join("processes.log");
        let wrapped = wrapper(&scratch, &git, &log, repo.workdir(), None)?;
        oxbow_core::config::set_git_program(Some(wrapped));
        let mut cache = HistoryCache::default();
        let folders = FolderCache::default();
        let _ = std::fs::remove_file(&log);
        let first = load(&repo, &mut cache, &folders, None, None, at_once[0])?;
        let (count, most) = processes(&log)?;
        println!("  {:<34} {count} git processes, at most {most} at once", "Open");
        let _ = std::fs::remove_file(&log);
        load(&repo, &mut cache, &folders, Some(&first.history.key), None, at_once[0])?;
        let (count, most) = processes(&log)?;
        println!(
            "  {:<34} {count} git processes, at most {most} at once",
            "Refresh, nothing changed"
        );

        if let Some(seconds) = slow {
            let wrapped = wrapper(&scratch, &git, &log, repo.workdir(), Some(seconds))?;
            oxbow_core::config::set_git_program(Some(wrapped));
            let first = load(
                &repo,
                &mut HistoryCache::default(),
                &FolderCache::default(),
                None,
                None,
                at_once[0],
            )?;
            println!(
                "  {:<34} {:.0?} (all statuses {:.0?})",
                format!("History shown, {seconds} s status"),
                first.shown,
                first.all
            );
            // Another repository opened while the folders are read: how soon the reading stops.
            let cancel = Arc::new(AtomicBool::new(false));
            let stop = cancel.clone();
            let stopper = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                stop.store(true, Ordering::Relaxed);
                Instant::now()
            });
            let _ = repo.folder_states(
                &first.history.worktrees,
                &first.history.submodules,
                &FolderCache::default(),
                None,
                at_once[0],
                &cancel,
                &mut |_| {},
            );
            let ended = Instant::now();
            let asked = stopper.join().expect("stopper thread");
            println!(
                "  {:<34} {:.0?}",
                "Stopped reading the folders in",
                ended.saturating_duration_since(asked)
            );
        }
    }
    println!("\nPeak memory {}", peak_memory());
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}

fn line(what: &str, times: &[Duration]) {
    let mut sorted = times.to_vec();
    sorted.sort();
    let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];
    println!("  {what:<34} {:>8.1?} / {:>8.1?}", at(0.5), at(0.95));
}

fn bytes(n: usize) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MiB", n as f64 / (1 << 20) as f64)
    } else {
        format!("{:.1} KiB", n as f64 / 1024.0)
    }
}

/// The git found on PATH, which the wrapper runs.
fn real_git() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let out = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()?;
    let path = String::from_utf8(out.stdout)?.trim().to_owned();
    if path.is_empty() {
        return Err("git isn't on PATH".into());
    }
    Ok(PathBuf::from(path))
}

fn git_version(git: &Path) -> String {
    std::process::Command::new(git)
        .arg("--version")
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .trim_start_matches("git version ")
                .to_owned()
        })
        .unwrap_or_default()
}

/// A script that runs `git`, writing a line to `log` as each process starts and ends. With
/// `slow`, `git status` outside `main` takes that many seconds longer.
fn wrapper(dir: &Path, git: &Path, log: &Path, main: &Path, slow: Option<u64>) -> std::io::Result<PathBuf> {
    let file = dir.join(if slow.is_some() { "git-slow" } else { "git" });
    let main = main.canonicalize()?;
    let slow = slow.map_or_else(String::new, |s| {
        format!(
            "case \" $* \" in *\" status \"*) [ \"$(pwd -P)\" = \"{}\" ] || sleep {s} ;; esac\n",
            main.display()
        )
    });
    let script = format!(
        "#!/bin/sh\necho + >> '{log}'\n{slow}'{git}' \"$@\"\ncode=$?\necho - >> '{log}'\nexit $code\n",
        log = log.display(),
        git = git.display(),
    );
    std::fs::write(&file, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(file)
}

/// How many processes the log saw, and how many ran at once at most.
fn processes(log: &Path) -> std::io::Result<(usize, usize)> {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let (mut count, mut now, mut most) = (0, 0usize, 0);
    for line in text.lines() {
        if line == "+" {
            count += 1;
            now += 1;
            most = most.max(now);
        } else {
            now = now.saturating_sub(1);
        }
    }
    Ok((count, most))
}

/// The most memory this process held, as Linux counts it; macOS: see /usr/bin/time -l.
fn peak_memory() -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .map(|l| l[6..].trim().to_owned())
        })
        .unwrap_or_else(|| "not known here".into())
}
