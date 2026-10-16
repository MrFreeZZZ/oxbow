//! Submodules: other repositories checked out in a folder here. This repository records which
//! commit of each one it builds with (a "gitlink" in its tree); the folder can be at that commit
//! or another one, have its own uncommitted changes, or not be cloned at all.
//!
//! The list comes from `.gitmodules` and the index (read with gitoxide); what is checked out in
//! each folder is asked of git there, like for worktrees.

use std::path::{Path, PathBuf};

use gix::bstr::ByteSlice;
use serde::Serialize;

use crate::cli::GitCommand;
use crate::remote::{CommitBrief, parse_briefs};
use crate::repo::Repo;

/// Commits listed between two commits of a submodule; the count says how many there are.
const LISTED: usize = 50;

/// One submodule of the repository.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Submodule {
    /// Its name in `.gitmodules`, usually its path.
    pub name: String,
    /// Where it is checked out, relative to the repository's folder.
    pub path: String,
    /// The folder's name, how Oxbow calls it.
    pub title: String,
    pub url: Option<String>,
    /// The branch `git submodule update --remote` takes new commits from (`submodule.<name>.branch`);
    /// `None` for the remote's default branch.
    pub follows: Option<String>,
    /// The commit this repository records for it: the one in the index, which the next commit
    /// records. `None` while it is being removed.
    pub recorded: Option<String>,
    /// The commit `HEAD` records; differs from `recorded` when a move is staged.
    pub committed: Option<String>,
    /// The commit checked out in its folder; `None` when it isn't cloned.
    pub current: Option<String>,
    /// The branch checked out in its folder; `None` when `HEAD` is detached there, as
    /// `git submodule update` leaves it.
    pub branch: Option<String>,
    /// Cloned and checked out in its folder.
    pub cloned: bool,
    /// Files with uncommitted changes in its folder, untracked ones included.
    pub changed: usize,
    /// Commits checked out in the folder that `recorded` doesn't have.
    pub ahead: usize,
    /// Commits of `recorded` the folder doesn't have checked out.
    pub behind: usize,
    /// The recorded commit isn't in its clone yet: it needs a fetch before the folder can have it.
    pub missing: bool,
}

impl Submodule {
    /// The folder has another commit than the one recorded.
    pub fn out_of_sync(&self) -> bool {
        self.cloned && self.current.is_some() && self.recorded.is_some() && self.current != self.recorded
    }
}

/// How a submodule's recorded commit moved, for the card shown in place of its diff.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Moved {
    /// To a newer commit: `commits` are the ones it gained.
    Forward,
    /// To an older commit: `commits` are the ones it dropped.
    Back,
    /// To a commit on another line of history: `commits` are the ones it gained, `dropped`
    /// counts the ones it lost.
    Sideways,
    Added,
    Removed,
    /// The same commit, as for a folder whose only changes are its own uncommitted files.
    Same,
    /// Its clone doesn't have both commits, so how they relate isn't known.
    Unknown,
}

/// A submodule's change in a commit or in the working copy.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SubmoduleMove {
    pub path: String,
    pub title: String,
    pub url: Option<String>,
    /// The branch it follows, as in [`Submodule::follows`].
    pub follows: Option<String>,
    pub old: Option<String>,
    pub new: Option<String>,
    pub moved: Moved,
    /// The commits between the two, newest first, at most [`LISTED`].
    pub commits: Vec<CommitBrief>,
    /// How many commits there are between the two in all.
    pub count: usize,
    /// For [`Moved::Sideways`]: commits of `old` that `new` doesn't have.
    pub dropped: usize,
    /// A clone exists to read its commits from.
    pub cloned: bool,
    /// For the working copy: files with uncommitted changes in its folder.
    pub changed: usize,
}

impl Repo {
    /// The repository's submodules in the order of `.gitmodules`; empty when it has none or they
    /// can't be read.
    pub fn submodules(&self) -> Vec<Submodule> {
        let repo = self.local();
        let Ok(Some(list)) = repo.submodules() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for module in list {
            let Ok(path) = module.path() else { continue };
            let path = path.to_str_lossy().into_owned();
            let recorded = module.index_id().ok().flatten().map(|id| id.to_string());
            let committed = module.head_id().ok().flatten().map(|id| id.to_string());
            if recorded.is_none() && committed.is_none() {
                // In .gitmodules only, without a commit recorded: not a submodule git would use.
                continue;
            }
            let state = module.state().ok();
            let cloned = state.as_ref().is_some_and(|s| s.worktree_checkout) && self.workdir().join(&path).is_dir();
            let mut sub = Submodule {
                name: module.name().to_str_lossy().into_owned(),
                title: title(&path),
                url: module.url().ok().map(|u| u.to_bstring().to_str_lossy().into_owned()),
                follows: module.branch().ok().flatten().map(|b| match b {
                    gix::submodule::config::Branch::Name(name) => name.to_str_lossy().into_owned(),
                    gix::submodule::config::Branch::CurrentInSuperproject => ".".to_owned(),
                }),
                path,
                recorded,
                committed,
                current: None,
                branch: None,
                cloned,
                changed: 0,
                ahead: 0,
                behind: 0,
                missing: false,
            };
            if cloned {
                let dir = self.workdir().join(&sub.path);
                sub.current = git(&dir, ["rev-parse", "--verify", "-q", "HEAD"]).map(|s| s.trim().to_owned());
                sub.branch = git(&dir, ["symbolic-ref", "-q", "--short", "HEAD"])
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty());
                sub.changed = crate::worktrees::changed_files(&dir);
                if let (Some(current), Some(recorded)) = (&sub.current, &sub.recorded)
                    && current != recorded
                {
                    sub.missing = !has_commit(&dir, recorded);
                    if !sub.missing
                        && let Some((ahead, behind)) = counts(&dir, recorded, current)
                    {
                        sub.ahead = ahead;
                        sub.behind = behind;
                    }
                }
            }
            out.push(sub);
        }
        out
    }

    /// Where git can read the submodule at `path`: its folder when checked out, else its clone in
    /// `.git/modules`; `None` when it was never cloned.
    pub(crate) fn submodule_dir(&self, path: &str) -> Option<PathBuf> {
        let folder = self.workdir().join(path);
        if folder.join(".git").exists() {
            return Some(folder);
        }
        let repo = self.local();
        let list = repo.submodules().ok().flatten()?;
        for module in list {
            if module
                .path()
                .ok()
                .is_some_and(|p| p.as_bstr() == path.as_bytes().as_bstr())
            {
                return module.git_dir().ok().filter(|dir| dir.is_dir());
            }
        }
        None
    }

    /// How the submodule at `path` moved from `old` to `new`, with the commits in between as far
    /// as its clone has them. `changed` counts its folder's uncommitted files, for the working copy.
    pub fn submodule_move(&self, path: &str, old: Option<&str>, new: Option<&str>, changed: usize) -> SubmoduleMove {
        let info = self.submodules().into_iter().find(|s| s.path == path);
        let dir = self.submodule_dir(path);
        let mut out = SubmoduleMove {
            path: path.to_owned(),
            title: title(path),
            url: info.as_ref().and_then(|s| s.url.clone()),
            follows: info.as_ref().and_then(|s| s.follows.clone()),
            old: old.map(str::to_owned),
            new: new.map(str::to_owned),
            moved: Moved::Unknown,
            commits: Vec::new(),
            count: 0,
            dropped: 0,
            cloned: dir.is_some(),
            changed,
        };
        out.moved = match (old, new) {
            (None, _) => Moved::Added,
            (_, None) => Moved::Removed,
            (Some(a), Some(b)) if a == b => Moved::Same,
            (Some(a), Some(b)) => {
                let Some(dir) = dir.as_deref().filter(|d| has_commit(d, a) && has_commit(d, b)) else {
                    return out;
                };
                let Some((gained, lost)) = counts(dir, a, b) else {
                    return out;
                };
                let (from, to, moved) = match (gained, lost) {
                    (_, 0) => (a, b, Moved::Forward),
                    (0, _) => (b, a, Moved::Back),
                    _ => (a, b, Moved::Sideways),
                };
                out.count = if moved == Moved::Back { lost } else { gained };
                out.dropped = if moved == Moved::Sideways { lost } else { 0 };
                out.commits = briefs(dir, from, to);
                moved
            }
        };
        out
    }
}

/// The last part of a path.
fn title(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// What git prints in `dir`, or `None` when it fails.
fn git<const N: usize>(dir: &Path, args: [&str; N]) -> Option<String> {
    crate::cli::run_in(dir, &GitCommand::new(args), None)
        .ok()
        .map(|out| out.stdout)
}

fn has_commit(dir: &Path, id: &str) -> bool {
    git(dir, ["cat-file", "-e", &format!("{id}^{{commit}}")]).is_some()
}

/// Commits of `to` that `from` doesn't have, and the other way round.
fn counts(dir: &Path, from: &str, to: &str) -> Option<(usize, usize)> {
    let out = git(dir, ["rev-list", "--count", "--left-right", &format!("{to}...{from}")])?;
    let mut parts = out.split_whitespace().map(|n| n.parse::<usize>().ok());
    Some((parts.next()??, parts.next()??))
}

/// Commits of `to` that `from` doesn't have, newest first, at most [`LISTED`].
fn briefs(dir: &Path, from: &str, to: &str) -> Vec<CommitBrief> {
    let limit = format!("-{LISTED}");
    git(
        dir,
        [
            "log",
            &limit,
            "--format=%H%x1f%s%x1f%an%x1f%ct",
            &format!("{from}..{to}"),
        ],
    )
    .map(|out| parse_briefs(&out))
    .unwrap_or_default()
}
