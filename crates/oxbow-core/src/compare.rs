//! Comparing two branches, tags or commits: what each has that the other doesn't, where they
//! split, and the files that differ.
//!
//! Since Split (`git diff A...B`) shows what B changed after it left A; Tip to Tip (`git diff
//! A..B`) compares the two latest states directly, so A's newer work shows up undone.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::cli::GitCommand;
use crate::commit::FileChange;
use crate::error::{Error, Result};
use crate::repo::Repo;

/// At most this many commits of each side are listed.
const LIMIT: usize = 500;

/// Paths looked for in one `git log` past the newest [`LIMIT`] commits.
const TOUCHED_BATCH: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CompareMode {
    /// From the common ancestor to the compared side: `git diff A...B`.
    Split,
    /// From one tip to the other: `git diff A..B`.
    Tips,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareCommit {
    pub id: String,
    pub summary: String,
    pub author_name: String,
    pub time: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompareFile {
    #[serde(flatten)]
    pub file: FileChange,
    /// The newest commit of the compared side that touched the file.
    pub last: Option<String>,
    /// Tip to Tip only: the difference comes from the base side's newer commits alone.
    pub only_base: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub base_id: String,
    pub target_id: String,
    /// Where the two split; none for unrelated histories.
    pub merge_base: Option<CompareCommit>,
    /// Commits only the compared side has (`git log base..target`), newest first.
    pub ahead: Vec<CompareCommit>,
    /// Commits only the base has (`git log target..base`), newest first.
    pub behind: Vec<CompareCommit>,
    pub ahead_count: usize,
    pub behind_count: usize,
    /// The mode used: Since Split needs a common ancestor, so unrelated histories compare tips.
    pub mode: CompareMode,
    /// The commit the diff starts from: the split point, or the base's tip.
    pub from: String,
    pub files: Vec<CompareFile>,
    /// The diff as one would type it: `git diff main...feature`.
    pub command: String,
}

impl Repo {
    /// The comparison of `base` and `target`, its files without line counts: those come with
    /// the pages of the diff and from [`Repo::tree_files_with`].
    pub fn compare(&self, base: &str, target: &str, mode: CompareMode) -> Result<Comparison> {
        let base_id = self.commit_id(base)?;
        let target_id = self.commit_id(target)?;
        let merge_base = self
            .run(&GitCommand::new(["merge-base", &base_id, &target_id]))
            .ok()
            .map(|out| out.stdout.trim().to_owned())
            .filter(|id| !id.is_empty());
        let mode = if merge_base.is_none() { CompareMode::Tips } else { mode };

        let counts = self.run(&GitCommand::new([
            "rev-list",
            "--left-right",
            "--count",
            &format!("{base_id}...{target_id}"),
        ]))?;
        let mut numbers = counts
            .stdout
            .split_whitespace()
            .map(|n| n.parse::<usize>().unwrap_or(0));
        let behind_count = numbers.next().unwrap_or(0);
        let ahead_count = numbers.next().unwrap_or(0);

        let ahead = self.commits_between(&base_id, &target_id)?;
        let behind = self.commits_between(&target_id, &base_id)?;
        let merge_base = match &merge_base {
            Some(id) => self.commit_info(id)?,
            None => None,
        };

        let from = match (&mode, &merge_base) {
            (CompareMode::Split, Some(m)) => m.id.clone(),
            _ => base_id.clone(),
        };
        let changes = self.tree_files_with(&from, &target_id, None, &std::sync::atomic::AtomicBool::new(false))?;
        let paths: Vec<&str> = changes.iter().map(|f| f.path.as_str()).collect();
        let touched_ahead = self.touched(&base_id, &target_id, &paths)?;
        let touched_behind = match mode {
            CompareMode::Tips => {
                let left: Vec<&str> = paths
                    .iter()
                    .copied()
                    .filter(|p| !touched_ahead.contains_key(*p))
                    .collect();
                self.touched(&target_id, &base_id, &left)?
            }
            CompareMode::Split => HashMap::new(),
        };
        let files = changes
            .into_iter()
            .map(|file| {
                let last = touched_ahead.get(&file.path).cloned();
                let only_base = last.is_none() && touched_behind.contains_key(&file.path);
                CompareFile { file, last, only_base }
            })
            .collect();

        let dots = if mode == CompareMode::Split { "..." } else { ".." };
        let command = GitCommand::new(["diff".to_owned(), format!("{base}{dots}{target}")]).display();
        Ok(Comparison {
            base_id,
            target_id,
            merge_base,
            ahead,
            behind,
            ahead_count,
            behind_count,
            mode,
            from,
            files,
            command,
        })
    }

    /// The commit a branch, tag or SHA names.
    pub(crate) fn commit_id(&self, name: &str) -> Result<String> {
        let out = self
            .run(&GitCommand::new([
                "rev-parse",
                "--verify",
                "--quiet",
                "--end-of-options",
                &format!("{name}^{{commit}}"),
            ]))
            .map_err(|_| Error::UnknownCommit(name.to_owned()))?;
        Ok(out.stdout.trim().to_owned())
    }

    /// Commits `to` has that `from` doesn't, newest first.
    fn commits_between(&self, from: &str, to: &str) -> Result<Vec<CompareCommit>> {
        let out = self.run(&GitCommand::new([
            "log".to_owned(),
            format!("--max-count={LIMIT}"),
            "--format=%H%x00%s%x00%an%x00%at".to_owned(),
            format!("{from}..{to}"),
        ]))?;
        Ok(out.stdout.lines().filter_map(parse_commit).collect())
    }

    fn commit_info(&self, id: &str) -> Result<Option<CompareCommit>> {
        let out = self.run(&GitCommand::new(["log", "-1", "--format=%H%x00%s%x00%an%x00%at", id]))?;
        Ok(out.stdout.lines().next().and_then(parse_commit))
    }

    /// Which of `paths` the commits in `from..to` touched, each with the newest commit that did.
    /// The newest [`LIMIT`] commits are read whole; paths none of them touched are looked for
    /// further back, so a file changed only in an old commit still gets it.
    fn touched(&self, from: &str, to: &str, paths: &[&str]) -> Result<HashMap<String, String>> {
        if paths.is_empty() {
            return Ok(HashMap::new());
        }
        let (mut files, walked) = self.touched_by(from, to, Some(LIMIT), &[])?;
        if walked >= LIMIT {
            let older: Vec<&str> = paths.iter().copied().filter(|p| !files.contains_key(*p)).collect();
            // In batches, to keep each command line short.
            for batch in older.chunks(TOUCHED_BATCH) {
                for (path, id) in self.touched_by(from, to, None, batch)?.0 {
                    files.entry(path).or_insert(id);
                }
            }
        }
        Ok(files)
    }

    /// The files the commits in `from..to` touched, newest first, at most `max` commits of them,
    /// only `paths` when given; and how many commits were read.
    fn touched_by(
        &self,
        from: &str,
        to: &str,
        max: Option<usize>,
        paths: &[&str],
    ) -> Result<(HashMap<String, String>, usize)> {
        let mut args = vec!["log".to_owned()];
        if let Some(max) = max {
            args.push(format!("--max-count={max}"));
        }
        args.extend([
            // The names as they are, not quoted, as the diff has them.
            "-z".to_owned(),
            "--format=%x00%H".to_owned(),
            "--name-only".to_owned(),
            "--end-of-options".to_owned(),
            format!("{from}..{to}"),
            "--".to_owned(),
        ]);
        args.extend(paths.iter().map(|p| crate::cli::literal(p)));
        let out = self.run(&GitCommand::new(args))?;
        let hits = crate::search::parse_log(&out.stdout);
        let walked = hits.len();
        let mut files = HashMap::new();
        for hit in hits {
            for path in hit.files {
                files.entry(path).or_insert_with(|| hit.id.clone());
            }
        }
        Ok((files, walked))
    }
}

fn parse_commit(line: &str) -> Option<CompareCommit> {
    let mut parts = line.split('\0');
    let id = parts.next()?.trim().to_owned();
    if id.is_empty() {
        return None;
    }
    Some(CompareCommit {
        id,
        summary: parts.next().unwrap_or_default().to_owned(),
        author_name: parts.next().unwrap_or_default().to_owned(),
        time: parts.next().and_then(|t| t.trim().parse().ok()).unwrap_or(0),
    })
}
