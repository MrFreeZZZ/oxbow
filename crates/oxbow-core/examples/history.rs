//! Load a repository's history and print how long it took:
//! `cargo run --release -p oxbow-core --example history -- <path>`

use std::time::Instant;

use oxbow_core::{HistoryOptions, Repo};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let start = Instant::now();
    let repo = Repo::open(&path)?;
    let history = repo.history(&HistoryOptions::default())?;
    let elapsed = start.elapsed();
    let lanes = history.rows.iter().map(|r| r.graph.width).max().unwrap_or(0) + 1;
    println!(
        "{}: {} commits{} in {:.0?}, {} refs, up to {} lanes",
        repo.name(),
        history.rows.len(),
        if history.truncated { " (truncated)" } else { "" },
        elapsed,
        history.refs.len(),
        lanes,
    );
    for row in history.rows.iter().take(15) {
        let mut graph = vec![' '; usize::from(row.graph.width) + 1];
        for seg in &row.graph.segments {
            graph[usize::from(seg.from)] = '│';
        }
        graph[usize::from(row.graph.column)] = if row.parents.len() > 1 { '◉' } else { '●' };
        let graph: String = graph.into_iter().collect();
        println!("{graph}  {} {}", &row.id[..7], row.summary);
    }
    Ok(())
}
