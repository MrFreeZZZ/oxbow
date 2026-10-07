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
        let touched_ahead = self.touched(&base_id, &target_id)?;
        let touched_behind = match mode {
            CompareMode::Tips => self.touched(&target_id, &base_id)?,
            CompareMode::Split => HashMap::new(),
        };
        let files = self
            .tree_files(&from, &target_id)?
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
            "--format=%H%x1f%s%x1f%an%x1f%at".to_owned(),
            format!("{from}..{to}"),
        ]))?;
        Ok(out.stdout.lines().filter_map(parse_commit).collect())
    }

    fn commit_info(&self, id: &str) -> Result<Option<CompareCommit>> {
        let out = self.run(&GitCommand::new(["log", "-1", "--format=%H%x1f%s%x1f%an%x1f%at", id]))?;
        Ok(out.stdout.lines().next().and_then(parse_commit))
    }

    /// Files the commits in `from..to` touched, each with the newest commit that did.
    fn touched(&self, from: &str, to: &str) -> Result<HashMap<String, String>> {
        let out = self.run(&GitCommand::new([
            "log".to_owned(),
            format!("--max-count={LIMIT}"),
            "--format=%x01%H".to_owned(),
            "--name-only".to_owned(),
            format!("{from}..{to}"),
        ]))?;
        let mut files = HashMap::new();
        for chunk in out.stdout.split('\u{1}') {
            let mut lines = chunk.lines().map(str::trim).filter(|l| !l.is_empty());
            let Some(id) = lines.next() else { continue };
            for path in lines {
                files.entry(path.to_owned()).or_insert_with(|| id.to_owned());
            }
        }
        Ok(files)
    }
}

fn parse_commit(line: &str) -> Option<CompareCommit> {
    let mut parts = line.split('\u{1f}');
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
