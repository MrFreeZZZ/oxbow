//! Measure what the next page of older commits costs as History is scrolled down, the way the
//! window asks for it (2,000 commits, then twice as many, at most 50,000 more at a time), and a
//! jump straight to an old commit: `cargo run --release -p oxbow-core --example pages -- <path>`.
//! On `history-100k` of `scripts/make-perf-fixtures.sh` (docs/measuring-reads.md). `RUNS=5`
//! repeats the whole scroll (default 3); the median of each page is printed, and how many of the
//! rows already shown came back the same: a row whose graph moved would come anew.

use std::time::{Duration, Instant};

use oxbow_core::{HistoryCache, HistoryOptions, PAGE, Repo};

const MAX_STEP: usize = 50_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: pages <repository>")?;
    let runs: usize = std::env::var("RUNS").ok().and_then(|v| v.parse().ok()).unwrap_or(3);
    let repo = Repo::open(&path)?;
    // Each page as the window asks for it: the commits it has, and how long the page took.
    let mut pages: Vec<(usize, Vec<Duration>, usize)> = Vec::new();
    let mut oldest = None;
    for _ in 0..runs {
        let mut cache = HistoryCache::default();
        let mut limit = PAGE;
        let mut known: Option<String> = None;
        let mut at = 0;
        loop {
            let start = Instant::now();
            let history = repo.history_with(
                &HistoryOptions {
                    limit,
                    ..Default::default()
                },
                &mut cache,
                known.as_deref(),
            )?;
            let took = start.elapsed();
            let kept = history.pieces.iter().filter(|p| p.known.is_some()).map(|p| p.len).sum();
            if pages.len() == at {
                pages.push((history.loaded, Vec::new(), kept));
            }
            pages[at].1.push(took);
            at += 1;
            known = Some(history.key.clone());
            if !history.truncated {
                oldest = history.rows.last().map(|r| r.id.clone());
                break;
            }
            limit = history.loaded + history.loaded.min(MAX_STEP);
        }
    }
    println!("{} · {runs} runs · median", repo.name());
    let mut before = 0;
    for (loaded, times, kept) in &mut pages {
        times.sort();
        println!(
            "  {:<34} {:>8.1?}   {} of {} rows shown kept",
            format!("Page {}–{}", before + 1, loaded),
            times[times.len() / 2],
            kept,
            before
        );
        before = *loaded;
    }
    // Opened afresh, then straight to the oldest commit, as a search result or a link does.
    if let Some(id) = oldest {
        let mut times = Vec::new();
        for _ in 0..runs {
            let mut cache = HistoryCache::default();
            let first = repo.history_with(&HistoryOptions::default(), &mut cache, None)?;
            let start = Instant::now();
            let options = HistoryOptions {
                until: Some(id.clone()),
                ..Default::default()
            };
            let to = repo.history_with(&options, &mut cache, Some(&first.key))?;
            assert!(to.rows.iter().any(|r| r.id == id), "the oldest commit is loaded");
            times.push(start.elapsed());
        }
        times.sort();
        println!("  {:<34} {:>8.1?}", "Jump to the oldest commit", times[times.len() / 2]);
    }
    Ok(())
}
