//! Lost work: where `HEAD` and the local branches have been, from git's own reflog.
//!
//! The Operation Log only knows what Oxbow did. A reset, a rebase or a deleted branch done in a
//! terminal or another app leaves its trace only in the reflog, and a commit no branch has any
//! more stays there until git expires the entry (`gc.reflogExpireUnreachable`, 30 days by
//! default). This lists those entries, newest first, and marks the commits no branch, tag or
//! remote branch has, so they can be looked at and kept on a new branch.

use std::collections::{BTreeSet, HashMap};

use serde::Serialize;

use crate::cli::GitCommand;
use crate::error::Result;
use crate::repo::Repo;

/// How many entries of `HEAD`'s reflog are read; it moves on every checkout.
const HEAD_ENTRIES: usize = 1000;
/// How many entries of each branch's reflog are read.
const BRANCH_ENTRIES: usize = 200;
/// How many commits the list holds at most, newest first.
const MAX_ROWS: usize = 300;

/// One commit a reflog points at, as of the newest entry that moved a ref to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReflogEntry {
    pub id: String,
    /// `HEAD` or the branch whose reflog has the entry.
    pub source: String,
    /// The entry the way git names it, e.g. `HEAD@{3}` or `main@{1}`.
    pub selector: String,
    /// When the ref moved there, in seconds since the Unix epoch.
    pub time: i64,
    /// The icon: commit, amend, reset, checkout, rebase, merge, pull, cherry, revert, branch or
    /// other.
    pub kind: String,
    /// What git wrote for the move, e.g. `reset: moving to HEAD~2`.
    pub message: String,
    pub summary: String,
    pub author_name: String,
    pub author_email: String,
    pub parents: Vec<String>,
    /// No branch, tag or remote branch has the commit, and `HEAD` is not on it.
    pub lost: bool,
    /// How many commits only this one keeps: itself and the lost commits before it.
    pub lost_count: usize,
    /// A newer lost commit in the list whose history has this one: keeping that one keeps this.
    pub within: Option<String>,
    /// For a lost commit, until when git keeps the entry, in seconds since the Unix epoch; then
    /// its next clean-up may delete the commit. `None` when git never expires it.
    pub expires: Option<i64>,
}

impl Repo {
    /// The commits `HEAD`'s and the local branches' reflogs point at, newest first, each once.
    pub fn reflog_entries(&self) -> Result<Vec<ReflogEntry>> {
        let mut sources = vec![("HEAD".to_owned(), HEAD_ENTRIES)];
        let branches = self.run(&GitCommand::new([
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
        ]))?;
        sources.extend(branches.stdout.lines().map(|name| (name.to_owned(), BRANCH_ENTRIES)));

        let mut newest: HashMap<String, ReflogEntry> = HashMap::new();
        for (source, max) in sources {
            for entry in self.read_reflog(&source, max) {
                // A branch's entry beats `HEAD`'s for the same move: it says where the work was.
                let better = newest.get(&entry.id).is_none_or(|seen| {
                    entry.time > seen.time || (entry.time == seen.time && seen.source == "HEAD" && source != "HEAD")
                });
                if better {
                    newest.insert(entry.id.clone(), entry);
                }
            }
        }
        let mut entries: Vec<ReflogEntry> = newest.into_values().collect();
        entries.sort_by(|a, b| b.time.cmp(&a.time).then_with(|| a.selector.cmp(&b.selector)));
        entries.truncate(MAX_ROWS);
        self.mark_lost(&mut entries)?;
        Ok(entries)
    }

    /// The entries of one reflog, newest first; empty when the ref has none.
    fn read_reflog(&self, source: &str, max: usize) -> Vec<ReflogEntry> {
        let reference = if source == "HEAD" {
            "HEAD".to_owned()
        } else {
            format!("refs/heads/{source}")
        };
        let Ok(out) = self.run(&GitCommand::new([
            "reflog".to_owned(),
            "show".to_owned(),
            "--date=unix".to_owned(),
            "--format=%H%x1f%gd%x1f%gs%x1f%s%x1f%an%x1f%ae%x1f%P".to_owned(),
            format!("--max-count={max}"),
            reference,
            "--".to_owned(),
        ])) else {
            return Vec::new();
        };
        out.stdout
            .lines()
            .enumerate()
            .filter_map(|(n, line)| {
                let mut parts = line.split('\x1f');
                let id = parts.next()?.to_owned();
                let time = parts
                    .next()?
                    .rsplit_once("@{")
                    .and_then(|(_, t)| t.trim_end_matches('}').parse().ok())?;
                let message = parts.next()?.to_owned();
                let summary = parts.next()?.to_owned();
                let author_name = parts.next()?.to_owned();
                let author_email = parts.next()?.to_owned();
                let parents = parts
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect();
                Some(ReflogEntry {
                    kind: kind_of(&message).to_owned(),
                    selector: format!("{source}@{{{n}}}"),
                    source: source.to_owned(),
                    id,
                    time,
                    message,
                    summary,
                    author_name,
                    author_email,
                    parents,
                    lost: false,
                    lost_count: 0,
                    within: None,
                    expires: None,
                })
            })
            .collect()
    }

    /// Mark the commits no branch, tag or remote branch has, count what each keeps, and say
    /// until when git keeps them.
    fn mark_lost(&self, entries: &mut [ReflogEntry]) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }
        let mut args: Vec<String> = vec!["rev-list".into(), "--parents".into()];
        args.extend(entries.iter().map(|e| e.id.clone()));
        args.extend(["--not", "--branches", "--tags", "--remotes", "HEAD", "--"].map(String::from));
        let out = match self.run(&GitCommand::new(args)) {
            Ok(out) => out,
            // A new repository has no HEAD commit to leave out; try again without it.
            Err(_) => {
                let mut args: Vec<String> = vec!["rev-list".into(), "--parents".into()];
                args.extend(entries.iter().map(|e| e.id.clone()));
                args.extend(["--not", "--branches", "--tags", "--remotes", "--"].map(String::from));
                self.run(&GitCommand::new(args))?
            }
        };
        // Lost commits and their lost parents.
        let mut parents: HashMap<&str, Vec<&str>> = HashMap::new();
        for line in out.stdout.lines() {
            let mut ids = line.split(' ');
            if let Some(id) = ids.next() {
                parents.insert(id, ids.collect());
            }
        }
        let reach = |tip: &str| -> BTreeSet<String> {
            let mut seen = BTreeSet::new();
            let mut todo = vec![tip];
            while let Some(id) = todo.pop() {
                if let Some(up) = parents.get(id)
                    && seen.insert(id.to_owned())
                {
                    todo.extend(up.iter().copied().filter(|p| parents.contains_key(p)));
                }
            }
            seen
        };

        let days = self
            .config_value("gc.reflogExpireUnreachable")
            .map_or(Some(30), |value| crate::oplog::expiry_days(&value));
        let lost: Vec<(usize, BTreeSet<String>)> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| parents.contains_key(e.id.as_str()))
            .map(|(i, e)| (i, reach(&e.id)))
            .collect();
        // A lost commit inside another one's lost history is kept with it: the list can fold it.
        let within: Vec<(usize, String)> = lost
            .iter()
            .filter_map(|(i, _)| {
                let id = &entries[*i].id;
                lost.iter()
                    .find(|(j, history)| j != i && history.contains(id))
                    .map(|(j, _)| (*i, entries[*j].id.clone()))
            })
            .collect();
        for (i, history) in &lost {
            let entry = &mut entries[*i];
            entry.lost = true;
            entry.lost_count = history.len();
            entry.expires = days.map(|d| entry.time + i64::from(d) * 86_400);
        }
        for (i, tip) in within {
            entries[i].within = Some(tip);
        }
        Ok(())
    }
}

/// The icon of a reflog message, from git's prefix for each command.
fn kind_of(message: &str) -> &'static str {
    let head = message.split(':').next().unwrap_or_default();
    if head.starts_with("commit (amend)") {
        "amend"
    } else if head.starts_with("commit (merge)") || head.starts_with("merge") {
        "merge"
    } else if head.starts_with("commit") {
        "commit"
    } else if head.starts_with("reset") {
        "reset"
    } else if head.starts_with("checkout") || head.starts_with("switch") {
        "checkout"
    } else if head.starts_with("rebase") {
        "rebase"
    } else if head.starts_with("pull") {
        "pull"
    } else if head.starts_with("cherry-pick") {
        "cherry"
    } else if head.starts_with("revert") {
        "revert"
    } else if head.starts_with("branch") || head.starts_with("Branch") {
        "branch"
    } else {
        "other"
    }
}

#[cfg(test)]
mod tests {
    use super::kind_of;

    #[test]
    fn kinds_follow_git_prefixes() {
        assert_eq!(kind_of("commit (amend): Fix"), "amend");
        assert_eq!(kind_of("commit (initial): Root"), "commit");
        assert_eq!(kind_of("commit: Fix"), "commit");
        assert_eq!(kind_of("reset: moving to HEAD~2"), "reset");
        assert_eq!(kind_of("checkout: moving from main to topic"), "checkout");
        assert_eq!(kind_of("rebase (finish): returning to refs/heads/topic"), "rebase");
        assert_eq!(kind_of("rebase -i (pick): Fix"), "rebase");
        assert_eq!(kind_of("merge topic: Fast-forward"), "merge");
        assert_eq!(kind_of("pull: Fast-forward"), "pull");
        assert_eq!(kind_of("cherry-pick: Fix"), "cherry");
        assert_eq!(kind_of("branch: Created from HEAD"), "branch");
        assert_eq!(kind_of("update by push"), "other");
    }
}
