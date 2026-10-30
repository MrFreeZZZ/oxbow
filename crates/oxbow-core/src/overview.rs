//! Passive local summaries. Unlike a full working tree, these never count lines or run filters.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;

use crate::Repo;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepoOverview {
    pub branch: Option<String>,
    pub commit: Option<String>,
    pub color: u8,
    pub stashes: Option<usize>,
    pub operation: Option<String>,
    pub changed: usize,
    pub conflicts: usize,
    pub upstream: Option<String>,
    /// None means unavailable, never an assumed zero (deleted upstream or shallow history).
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    /// Time of FETCH_HEAD, if present. This doesn't promise every remote was fetched then.
    pub fetched_at: Option<u64>,
    /// Filters are disabled; changes may differ from a normal status that applies them.
    pub filters_disabled: bool,
}

pub fn read(path: &Path, cancel: &AtomicBool) -> Result<RepoOverview> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Cancelled);
    }
    let repo = Repo::open(path)?;
    // A folder replaced with an ordinary directory must not silently summarize its parent repo.
    if path.canonicalize().ok() != repo.workdir().canonicalize().ok() {
        return Err(Error::NotARepository {
            path: path.display().to_string(),
        });
    }
    let head = repo.head()?;
    let (command, filters_disabled) = crate::trust::passive_status(repo.workdir())?;
    let out = repo.run_cancellable(&command, cancel)?;
    if repo.head()? != head {
        return Err(Error::Changed(
            "HEAD changed during the read. Refresh to try again.".into(),
        ));
    }
    let mut result = parse(&out.stdout);
    result.branch = head.branch;
    result.commit = head.commit;
    result.filters_disabled = filters_disabled;
    let local = repo.local();
    result.operation = operation(&repo, result.conflicts);
    if result.branch.is_none() {
        for state in ["rebase-merge", "rebase-apply"] {
            if let Ok(name) = std::fs::read_to_string(local.git_dir().join(state).join("head-name")) {
                result.branch = name.trim().strip_prefix("refs/heads/").map(str::to_owned);
                break;
            }
        }
    }
    let refs = repo.refs().unwrap_or_default();
    let production = repo.production_setting();
    let trunk = crate::history::pick_trunk(&refs, production.as_deref());
    result.color = match result.branch.as_deref() {
        None => crate::graph::NO_BRANCH_COLOR,
        Some(name) if trunk.is_some_and(|r| r.kind == crate::RefKind::Local && r.name == name) => {
            crate::graph::TRUNK_COLOR
        }
        Some(name) => crate::graph::color_for_name(name),
    };
    result.stashes = stash_count(&local, cancel)?;
    // A shallow boundary can make a comparison appear complete even though ancestry is missing.
    if repo.local().is_shallow() {
        result.ahead = None;
        result.behind = None;
    }
    result.fetched_at = std::fs::metadata(repo.local().git_dir().join("FETCH_HEAD"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());
    Ok(result)
}

fn parse(text: &str) -> RepoOverview {
    let mut result = RepoOverview {
        branch: None,
        commit: None,
        color: 0,
        stashes: None,
        operation: None,
        changed: 0,
        conflicts: 0,
        upstream: None,
        ahead: None,
        behind: None,
        fetched_at: None,
        filters_disabled: false,
    };
    let mut records = text.split('\0');
    while let Some(record) = records.next() {
        if let Some(value) = record.strip_prefix("# branch.upstream ") {
            result.upstream = Some(value.to_owned());
        } else if let Some(value) = record.strip_prefix("# branch.ab ") {
            if let Some((a, b)) = value.split_once(' ') {
                result.ahead = a.strip_prefix('+').and_then(|n| n.parse().ok());
                result.behind = b.strip_prefix('-').and_then(|n| n.parse().ok());
            }
        } else if record.starts_with("1 ") || record.starts_with("? ") {
            result.changed += 1;
        } else if record.starts_with("2 ") {
            result.changed += 1;
            records.next(); // A rename's original name is a separate NUL record.
        } else if record.starts_with("u ") {
            result.changed += 1;
            result.conflicts += 1;
        }
    }
    result
}

// Metadata alone: the full operation reader loads commit details and may clear old apply state.
fn operation(repo: &Repo, conflicts: usize) -> Option<String> {
    use gix::state::InProgress as State;
    let local = repo.local();
    let label = match local.state() {
        Some(State::ApplyMailbox) => "Applying patches paused",
        Some(State::ApplyMailboxRebase | State::Rebase | State::RebaseInteractive) => "Rebase paused",
        Some(State::Merge) => "Merge paused",
        Some(State::CherryPick | State::CherryPickSequence) => "Cherry-pick paused",
        Some(State::Revert | State::RevertSequence) => "Revert paused",
        Some(State::Bisect) => "Bisect in progress",
        None if local.git_dir().join("SQUASH_MSG").is_file() => "Squash waiting to be committed",
        None if conflicts > 0 && repo.pending_apply().is_some() => "Stash apply paused",
        None => return None,
    };
    Some(label.to_owned())
}

fn stash_count(local: &gix::Repository, cancel: &AtomicBool) -> Result<Option<usize>> {
    let Ok(reference) = local.try_find_reference("refs/stash") else {
        return Ok(None);
    };
    let Some(reference) = reference else {
        return Ok(Some(0));
    };
    let mut log = reference.log_iter();
    let Ok(Some(lines)) = log.rev() else {
        return Ok(None);
    };
    let mut count = 0;
    for line in lines {
        if cancel.load(Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        if line.is_err() {
            return Ok(None);
        }
        count += 1;
    }
    Ok(Some(count))
}
