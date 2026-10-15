//! What Undo can't bring back: the parts of a step that stay out of the Operation Log's
//! snapshots, so the confirmation sheet says so before the step runs instead of promising more
//! than the log keeps.

use std::collections::BTreeSet;
use std::path::Path;

use serde::Serialize;

use crate::cli::GitCommand;
use crate::edit::ResetMode;
use crate::error::Result;
use crate::oplog::{MAX_UNTRACKED, short_ref};
use crate::repo::Repo;
use crate::worktree::Action;
use crate::worktrees::same_path;

/// At most this many files are named; the rest are counted.
const NAMED: usize = 5;
/// A folder of ignored files is measured up to this many entries, so a huge `node_modules`
/// doesn't hold the sheet up; past it the size is at least what was counted.
const MEASURED: usize = 20_000;
/// At most this many commits of a branch are looked at for being on its remote.
const LOOKED_AT: usize = 500;

/// A file and its size in bytes; a folder's size is everything in it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SizedFile {
    /// Relative to the folder it is in; a folder ends with `/`.
    pub path: String,
    pub size: u64,
}

/// A commit of a branch that is already on its remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushedCommit {
    pub id: String,
    pub summary: String,
}

/// Something an action changes that the Operation Log can't bring back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum NotKept {
    /// Untracked files too big for a snapshot that the action deletes or writes over.
    BigFiles {
        files: Vec<SizedFile>,
        more: usize,
        limit: u64,
    },
    /// Files git ignores, e.g. build output or `.env`, in a folder the action deletes.
    Ignored {
        folder: String,
        files: Vec<SizedFile>,
        more: usize,
        bytes: u64,
    },
    /// Another worktree's files: a removal throws its uncommitted changes away (`removed`), or
    /// an undo moves its checked-out branch while its files stay as they are.
    Worktree {
        folder: String,
        branch: Option<String>,
        changed: usize,
        removed: bool,
    },
    /// The step itself stays out of the log, e.g. removing a worktree.
    NotLogged { what: String },
    /// Commits the undo takes off a branch that its remote already has: only this copy changes.
    /// `revert` is the commit to revert instead, when there is just one on the checked-out branch.
    Pushed {
        branch: String,
        remote: String,
        commits: Vec<PushedCommit>,
        more: usize,
        revert: Option<String>,
    },
}

impl Repo {
    /// What running `action` changes that the Operation Log can't bring back, for the sheet.
    /// Empty for most actions: their every change is in the log.
    pub fn not_kept(&self, action: &Action) -> Result<Vec<NotKept>> {
        let mut out = Vec::new();
        match action {
            Action::Discard { paths } => {
                let untracked = self.untracked()?;
                let big: Vec<&String> = paths.iter().filter(|p| untracked.contains(*p)).collect();
                out.extend(self.big_files(big));
            }
            Action::Reset {
                commit,
                mode: ResetMode::Hard,
            } => {
                // An untracked file at a path the commit has is written over.
                let paths = self.tree_paths(commit)?;
                out.extend(self.big_files(self.untracked()?.iter().filter(|p| paths.contains(*p))));
            }
            Action::Restore { id, after } => {
                let point = self.restore_point(id, *after)?;
                let paths = self.tree_paths(&point.trees.worktree)?;
                out.extend(self.big_files(self.untracked()?.iter().filter(|p| paths.contains(*p))));
                let now = self.all_refs()?;
                let moving: Vec<(&String, &String)> = point
                    .refs
                    .iter()
                    .filter_map(|(name, then)| {
                        let now = now.get(name)?;
                        (then.as_ref() != Some(now)).then_some((name, now))
                    })
                    .collect();
                for worktree in self.worktrees()?.into_iter().filter(|w| !w.current && !w.missing) {
                    let Some(branch) = &worktree.branch else { continue };
                    if moving.iter().any(|(name, _)| **name == format!("refs/heads/{branch}")) {
                        out.push(NotKept::Worktree {
                            folder: self.shown_path(&worktree.path),
                            branch: Some(branch.clone()),
                            changed: worktree.changed,
                            removed: false,
                        });
                    }
                }
                let head = self.head_state()?.branch;
                for (name, then) in &point.refs {
                    let (Some(then), Some(now)) = (then, now.get(name)) else {
                        continue;
                    };
                    if then == now {
                        continue;
                    }
                    if let Some(pushed) = self.pushed(name, then, now, head.as_deref())? {
                        out.push(pushed);
                    }
                }
            }
            Action::WorktreeRemove { path, force, stash } => {
                let folder = self.shown_path(path);
                let worktree = self.worktrees()?.into_iter().find(|w| same_path(&w.path, path));
                if *force && !*stash {
                    out.push(NotKept::Worktree {
                        folder: folder.clone(),
                        branch: worktree.as_ref().and_then(|w| w.branch.clone()),
                        changed: worktree.as_ref().map_or(0, |w| w.changed),
                        removed: true,
                    });
                }
                if let Some(ignored) = self.ignored_in(path, folder.clone())? {
                    out.push(ignored);
                }
                out.push(NotKept::NotLogged {
                    what: format!("removing {folder}"),
                });
            }
            _ => {}
        }
        Ok(out)
    }

    /// Untracked files that are not ignored, as `git status` lists them.
    fn untracked(&self) -> Result<BTreeSet<String>> {
        Ok(self
            .run(&GitCommand::new(["ls-files", "-z", "-o", "--exclude-standard"]))?
            .stdout
            .split('\0')
            .filter(|p| !p.is_empty() && !p.ends_with('/'))
            .map(str::to_owned)
            .collect())
    }

    /// Every file path in a commit's or a tree's tree.
    fn tree_paths(&self, tree: &str) -> Result<BTreeSet<String>> {
        Ok(self
            .run(&GitCommand::new(["ls-tree", "-r", "-z", "--name-only", tree]))?
            .stdout
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// The files of `paths` too big for a snapshot, biggest first.
    fn big_files<'a>(&self, paths: impl IntoIterator<Item = &'a String>) -> Option<NotKept> {
        let mut files: Vec<SizedFile> = paths
            .into_iter()
            .filter(|p| !self.small_file(p))
            .filter_map(|p| {
                let meta = self.workdir().join(p).symlink_metadata().ok()?;
                meta.is_file().then(|| SizedFile {
                    path: p.clone(),
                    size: meta.len(),
                })
            })
            .collect();
        if files.is_empty() {
            return None;
        }
        files.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
        let more = files.len().saturating_sub(NAMED);
        files.truncate(NAMED);
        Some(NotKept::BigFiles {
            files,
            more,
            limit: MAX_UNTRACKED,
        })
    }

    /// The files git ignores in the worktree at `path`; a folder of them counts as one.
    fn ignored_in(&self, path: &str, folder: String) -> Result<Option<NotKept>> {
        if !Path::new(path).is_dir() {
            return Ok(None);
        }
        let Ok(out) = self.run(&GitCommand::new([
            "-C",
            path,
            "ls-files",
            "-z",
            "-o",
            "-i",
            "--exclude-standard",
            "--directory",
        ])) else {
            return Ok(None);
        };
        let mut budget = MEASURED;
        let mut files: Vec<SizedFile> = out
            .stdout
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(|p| SizedFile {
                path: p.to_owned(),
                size: size_of(&Path::new(path).join(p), &mut budget),
            })
            .collect();
        if files.is_empty() {
            return Ok(None);
        }
        let bytes = files.iter().map(|f| f.size).sum();
        files.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
        let more = files.len().saturating_sub(NAMED);
        files.truncate(NAMED);
        Ok(Some(NotKept::Ignored {
            folder,
            files,
            more,
            bytes,
        }))
    }

    /// The commits an undo takes off branch `name` (from `now` back to `then`) that its
    /// upstream already has, newest first.
    fn pushed(&self, name: &str, then: &str, now: &str, head: Option<&str>) -> Result<Option<NotKept>> {
        let Some(branch) = name.strip_prefix("refs/heads/") else {
            return Ok(None);
        };
        let upstream = self
            .run(&GitCommand::new(["for-each-ref", "--format=%(upstream)", name]))?
            .stdout
            .trim()
            .to_owned();
        if upstream.is_empty()
            || self
                .run(&GitCommand::new(["rev-parse", "-q", "--verify", &upstream]))
                .is_err()
        {
            return Ok(None);
        }
        let list = |args: &[&str]| -> Result<Vec<String>> {
            let max = format!("--max-count={LOOKED_AT}");
            let mut all = vec!["rev-list", max.as_str()];
            all.extend_from_slice(args);
            Ok(self
                .run(&GitCommand::new(all))?
                .stdout
                .lines()
                .map(str::to_owned)
                .collect())
        };
        let leaving = list(&[now, &format!("^{then}")])?;
        let unpushed: BTreeSet<String> = list(&[now, &format!("^{then}"), &format!("^{upstream}")])?
            .into_iter()
            .collect();
        let pushed: Vec<String> = leaving.into_iter().filter(|id| !unpushed.contains(id)).collect();
        if pushed.is_empty() {
            return Ok(None);
        }
        let revert = match pushed.as_slice() {
            [one] if head == Some(branch) && self.parent_count(one) == 1 => Some(one.clone()),
            _ => None,
        };
        let more = pushed.len().saturating_sub(NAMED);
        let commits = pushed
            .iter()
            .take(NAMED)
            .map(|id| PushedCommit {
                id: id.clone(),
                summary: self
                    .run(&GitCommand::new(["log", "-1", "--format=%s", id]))
                    .map(|o| o.stdout.trim().to_owned())
                    .unwrap_or_default(),
            })
            .collect();
        Ok(Some(NotKept::Pushed {
            branch: branch.to_owned(),
            remote: short_ref(&upstream),
            commits,
            more,
            revert,
        }))
    }

    fn parent_count(&self, id: &str) -> usize {
        self.run(&GitCommand::new(["rev-list", "--parents", "-n", "1", id]))
            .map(|o| o.stdout.split_whitespace().count().saturating_sub(1))
            .unwrap_or(0)
    }
}

/// The size of a file, or of everything in a folder, without following symlinks; stops counting
/// once `budget` entries were looked at.
fn size_of(path: &Path, budget: &mut usize) -> u64 {
    let Ok(meta) = path.symlink_metadata() else { return 0 };
    if !meta.is_dir() {
        return meta.len();
    }
    let Ok(entries) = std::fs::read_dir(path) else { return 0 };
    let mut total = 0;
    for entry in entries.flatten() {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        total += size_of(&entry.path(), budget);
    }
    total
}
