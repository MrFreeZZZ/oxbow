//! GitLab's REST API (`/api/v4`), signed in with a personal access token: gitlab.com or a
//! server of your own. Without a sign-in nothing here is used, and Create Merge Request works
//! through the push (see `gitlab.rs`). Each call has a [`Request::display`] form, the same
//! request as `curl`, for the sheets that show what Oxbow will do.

mod client;
mod types;

pub use client::{Client, Request, draft_title, line_code};
pub use types::*;

use crate::pulls::{Entry, HunkLine, PullRequest};
use crate::repo::Repo;

impl Repo {
    /// What of merge request `mr` this repository can tell when its commits are here: the lines
    /// each commit and the whole merge request add and delete, and the code under comments on
    /// lines.
    pub fn merge_request_here(&self, mr: &mut PullRequest) {
        let shas: Vec<String> = mr.commits.iter().map(|c| c.sha.clone()).collect();
        let stats = self.commit_stats(&shas);
        for commit in &mut mr.commits {
            if let Some((add, del)) = stats.get(&commit.sha) {
                commit.additions = Some(*add);
                commit.deletions = Some(*del);
            }
        }
        let Some(facts) = mr.gitlab.as_mut() else {
            return;
        };
        let Some(diff) = &facts.diff else {
            return;
        };
        if let Some((add, del)) = self.diff_stats(&diff.start_sha, &diff.head_sha) {
            mr.additions = add;
            mr.deletions = del;
            facts.line_counts = true;
        }
        let local = self.local();
        for entry in &mut mr.timeline {
            if let Entry::Thread {
                path,
                line: Some(line),
                start_line,
                side,
                hunk,
                commit,
                ..
            } = entry
                && hunk.is_empty()
                && !path.is_empty()
            {
                // New lines as the comment saw them: its own version when it is here.
                let mine = commit.as_deref().filter(|c| local.rev_parse_single(*c).is_ok());
                let at = if side == "LEFT" {
                    diff.base_sha.as_str()
                } else {
                    mine.unwrap_or(&diff.head_sha)
                };
                // A comment on several new lines shows them all, for a suggestion on them.
                let from = start_line.filter(|s| side != "LEFT" && *s <= *line).unwrap_or(*line);
                *hunk = lines_above(&local, at, path, *line, (*line - from + 1).max(4), side != "LEFT");
                for l in hunk.iter_mut() {
                    l.mark = l.number.is_some_and(|n| n >= from);
                }
            }
        }
    }

    /// Lines `head` adds and deletes against `base`, when both are here.
    fn diff_stats(&self, base: &str, head: &str) -> Option<(u32, u32)> {
        if base.is_empty() || head.is_empty() {
            return None;
        }
        let out = self
            .run(&crate::cli::GitCommand::new([
                "diff",
                "--numstat",
                "--no-renames",
                "--end-of-options",
                base,
                head,
                "--",
            ]))
            .ok()?;
        let (mut add, mut del) = (0, 0);
        for line in out.stdout.lines() {
            let mut cols = line.split('\t');
            add += cols.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
            del += cols.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
        }
        Some((add, del))
    }
}

/// The `count` lines of `path` at commit `at` that end with `line`, the last one marked.
fn lines_above(repo: &gix::Repository, at: &str, path: &str, line: u32, count: u32, numbered: bool) -> Vec<HunkLine> {
    let Ok(id) = repo.rev_parse_single(format!("{at}:{path}").as_str()) else {
        return Vec::new();
    };
    let Ok(blob) = repo.find_object(id) else {
        return Vec::new();
    };
    if blob.kind != gix::object::Kind::Blob {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&blob.data);
    let first = line.saturating_sub(count - 1).max(1);
    text.lines()
        .enumerate()
        .map(|(i, text)| (i as u32 + 1, text))
        .filter(|(n, _)| (first..=line).contains(n))
        .map(|(n, text)| HunkLine {
            number: numbered.then_some(n),
            text: text.to_owned(),
            mark: n == line,
        })
        .collect()
}
