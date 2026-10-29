//! What Undo can't bring back: the parts of a step that stay out of the Operation Log's
//! snapshots, so the confirmation sheet says so before the step runs instead of promising more
//! than the log keeps.

use std::collections::BTreeSet;
use std::io::ErrorKind;
use std::path::Path;

use serde::Serialize;

use crate::action::Action;
use crate::cli::GitCommand;
use crate::edit::ResetMode;
use crate::error::Result;
use crate::oplog::{MAX_UNTRACKED, short_ref};
use crate::repo::Repo;
use crate::worktrees::same_path;

/// At most this many files are named; the rest are counted.
const NAMED: usize = 5;
/// A folder of ignored files is measured up to this many entries, so a huge `node_modules`
/// doesn't hold the sheet up; past it the size is at least what was counted. A folder in the way
/// of a reset is looked through up to as many, and the sheet names what was left unchecked.
const MEASURED: usize = 20_000;
/// At most this many submodule folders, nested ones included, are looked into before a removal.
const NESTED: usize = 50;
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
    /// Files git ignores, e.g. a local `.env`, that a reset or an undo writes over or deletes:
    /// the commit has a file where they are, or a folder where one of them is a file.
    IgnoredInTheWay {
        files: Vec<SizedFile>,
        more: usize,
        bytes: u64,
    },
    /// Folders a reset or an undo deletes or writes over that weren't looked through to the end:
    /// too many files, or one that can't be read. Files there too big for a snapshot, or ignored,
    /// would be lost, so the sheet says so instead of naming none.
    Unchecked {
        /// Each ends with `/`.
        folders: Vec<String>,
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
        /// `None` when `git status` failed there: the files may or may not have changes.
        changed: Option<usize>,
        removed: bool,
    },
    /// The step itself stays out of the log, e.g. removing a worktree.
    NotLogged { what: String },
    /// Removing a submodule stays out of the log: its folder is another repository.
    SubmoduleFolder { folder: String },
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
            // A file git doesn't track, ignored ones too, that the commit's or Local History's version
            // writes over.
            Action::TakeVersion { path, .. } | Action::RestoreLocalVersion { path, .. } => {
                let out_of_git = !self
                    .run(&GitCommand::new([
                        "ls-files",
                        "-z",
                        "-o",
                        "--",
                        &crate::cli::literal(path),
                    ]))?
                    .stdout
                    .is_empty();
                out.extend(self.big_files(out_of_git.then_some(path)));
            }
            Action::Reset {
                commit,
                mode: ResetMode::Hard,
            } => {
                out.extend(self.in_the_way(commit)?);
            }
            Action::Restore { id, after } => {
                let point = self.restore_point(id, *after)?;
                out.extend(self.in_the_way(&point.trees.worktree)?);
                let now = self.all_refs()?;
                let moving: Vec<(&String, &String)> = point
                    .refs
                    .iter()
                    .filter_map(|(name, then)| {
                        let now = now.get(name)?;
                        (then.as_ref() != Some(now)).then_some((name, now))
                    })
                    .collect();
                for worktree in self.worktree_list()?.into_iter().filter(|w| !w.current && !w.missing) {
                    let Some(branch) = &worktree.branch else { continue };
                    if moving.iter().any(|(name, _)| **name == format!("refs/heads/{branch}")) {
                        out.push(NotKept::Worktree {
                            folder: self.shown_path(&worktree.path),
                            branch: Some(branch.clone()),
                            changed: crate::worktrees::changed_files(Path::new(&worktree.path)).ok(),
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
                let worktree = self.worktree_list()?.into_iter().find(|w| same_path(&w.path, path));
                if *force && !*stash {
                    // Only its own folder is counted, not every worktree's.
                    let changed = match &worktree {
                        Some(w) if w.pending => crate::worktrees::changed_files(Path::new(&w.path)).ok(),
                        Some(w) => w.changed,
                        None => Some(0),
                    };
                    out.push(NotKept::Worktree {
                        folder: folder.clone(),
                        branch: worktree.as_ref().and_then(|w| w.branch.clone()),
                        changed,
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
            Action::SubmoduleRemove { path } => {
                // `git status` leaves ignored files out, so the folder can look clean with a
                // `.env` in it; submodules inside it go with it too.
                let mut folders = vec![path.clone()];
                let mut i = 0;
                while i < folders.len() && folders.len() < NESTED {
                    folders.extend(self.nested_submodules(&folders[i]));
                    i += 1;
                }
                for folder in folders {
                    let dir = self.workdir().join(&folder);
                    if let Some(ignored) = self.ignored_in(&dir.to_string_lossy(), folder)? {
                        out.push(ignored);
                    }
                }
                out.push(NotKept::SubmoduleFolder { folder: path.clone() });
            }
            _ => {}
        }
        Ok(out)
    }

    /// The submodules checked out inside the submodule at `folder`, relative to this repository.
    fn nested_submodules(&self, folder: &str) -> Vec<String> {
        let dir = self.workdir().join(folder);
        let Ok(out) = self.run(&GitCommand::new([
            "-C",
            &dir.to_string_lossy(),
            "ls-files",
            "-z",
            "--stage",
        ])) else {
            return Vec::new();
        };
        out.stdout
            .split('\0')
            .filter_map(|entry| entry.strip_prefix("160000 "))
            .filter_map(|rest| rest.split_once('\t').map(|(_, path)| format!("{folder}/{path}")))
            .filter(|path| self.workdir().join(path).join(".git").exists())
            .collect()
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

    /// What a reset or an undo to `tree` writes over or deletes that no snapshot has: untracked
    /// files git lists that are too big, and files it ignores. A file is in the way when the tree
    /// has a file at its path or in a folder of that name, or a folder where the file is.
    fn in_the_way(&self, tree: &str) -> Result<Vec<NotKept>> {
        let paths = self.tree_paths(tree)?;
        let tracked = self.index_paths()?;
        let untracked = self.untracked()?;
        let workdir = self.workdir();
        let mut found = BTreeSet::new();
        let mut unchecked = BTreeSet::new();
        let mut budget = MEASURED;
        // Paths the index has are tracked files, whose content every snapshot has.
        for path in paths.iter().filter(|p| !tracked.contains(*p)) {
            // What git lists is all there, however far the look through the folder gets.
            let inside = format!("{path}/");
            if untracked.contains(path) {
                found.insert(path.clone());
            }
            found.extend(
                untracked
                    .range(inside.clone()..)
                    .take_while(|u| u.starts_with(&inside))
                    .cloned(),
            );
            files_at(workdir, path, &tracked, &mut found, &mut unchecked, &mut budget);
            let mut end = 0;
            while let Some(slash) = path[end..].find('/') {
                end += slash;
                let folder = &path[..end];
                if !tracked.contains(folder) && workdir.join(folder).symlink_metadata().is_ok_and(|m| !m.is_dir()) {
                    found.insert(folder.to_owned());
                }
                end += 1;
            }
        }
        let (listed, ignored): (Vec<&String>, Vec<&String>) = found.iter().partition(|p| untracked.contains(*p));
        // An ignored file just as the tree has it loses nothing, such as one Add to .gitignore
        // stopped tracking and an undo tracks again.
        let same = self.same_as_in(tree, &ignored);
        let ignored: Vec<&String> = ignored.into_iter().filter(|p| !same.contains(*p)).collect();
        let mut out: Vec<NotKept> = self.big_files(listed).into_iter().collect();
        if !ignored.is_empty() {
            let mut files: Vec<SizedFile> = ignored
                .into_iter()
                .map(|p| SizedFile {
                    path: p.clone(),
                    size: workdir.join(p).symlink_metadata().map_or(0, |m| m.len()),
                })
                .collect();
            let bytes = files.iter().map(|f| f.size).sum();
            files.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
            let more = files.len().saturating_sub(NAMED);
            files.truncate(NAMED);
            out.push(NotKept::IgnoredInTheWay { files, more, bytes });
        }
        // A folder inside one already named adds nothing.
        let mut folders: Vec<String> = Vec::new();
        for folder in unchecked {
            if !folders.last().is_some_and(|f| folder.starts_with(f.as_str())) {
                folders.push(folder);
            }
        }
        if !folders.is_empty() {
            let more = folders.len().saturating_sub(NAMED);
            folders.truncate(NAMED);
            out.push(NotKept::Unchecked {
                folders,
                more,
                limit: MAX_UNTRACKED,
            });
        }
        Ok(out)
    }

    /// Every path in the index.
    fn index_paths(&self) -> Result<BTreeSet<String>> {
        Ok(self
            .run(&GitCommand::new(["ls-files", "-z"]))?
            .stdout
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect())
    }

    /// Every file path in a commit's or a tree's tree.
    /// Those of `paths` whose file on disk has the very content of the file at its path in `tree`.
    fn same_as_in(&self, tree: &str, paths: &[&String]) -> BTreeSet<String> {
        let files: Vec<&String> = paths
            .iter()
            .copied()
            .filter(|p| self.workdir().join(p).symlink_metadata().is_ok_and(|m| m.is_file()))
            .collect();
        if files.is_empty() {
            return BTreeSet::new();
        }
        let listed = |args: Vec<&str>| {
            self.run(&GitCommand::new(args))
                .map(|out| out.stdout)
                .unwrap_or_default()
        };
        let mut ls = vec!["ls-tree", "-r", "-z", tree, "--"];
        ls.extend(files.iter().map(|p| p.as_str()));
        let in_tree: std::collections::HashMap<&str, String> = listed(ls)
            .split('\0')
            .filter_map(|entry| {
                let (meta, path) = entry.split_once('\t')?;
                let id = meta.split(' ').nth(2)?;
                Some((files.iter().find(|p| p.as_str() == path)?.as_str(), id.to_owned()))
            })
            .collect();
        let mut hash = vec!["hash-object", "--"];
        hash.extend(files.iter().map(|p| p.as_str()));
        let ids = listed(hash);
        files
            .iter()
            .zip(ids.lines())
            .filter(|(p, id)| in_tree.get(p.as_str()).is_some_and(|t| t == id))
            .map(|(p, _)| (*p).clone())
            .collect()
    }

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

/// The untracked file at `path`, or every untracked file in the folder there, without following
/// symlinks; stops once `budget` entries were looked at. A folder not looked through to the end,
/// one it can't read, has a name that isn't UTF-8 in it or is past the budget, goes in
/// `unchecked`, ending with `/`.
fn files_at(
    workdir: &Path,
    path: &str,
    tracked: &BTreeSet<String>,
    found: &mut BTreeSet<String>,
    unchecked: &mut BTreeSet<String>,
    budget: &mut usize,
) {
    let meta = match workdir.join(path).symlink_metadata() {
        Ok(meta) => meta,
        // Nothing there, or a file where a folder of the path would be.
        Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => return,
        Err(_) => {
            unchecked.insert(format!("{path}/"));
            return;
        }
    };
    if !meta.is_dir() {
        if !tracked.contains(path) {
            found.insert(path.to_owned());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(workdir.join(path)) else {
        unchecked.insert(format!("{path}/"));
        return;
    };
    for entry in entries {
        if *budget == 0 {
            unchecked.insert(format!("{path}/"));
            return;
        }
        *budget -= 1;
        match entry.as_ref().map(|e| e.file_name().into_string()) {
            Ok(Ok(name)) => files_at(workdir, &format!("{path}/{name}"), tracked, found, unchecked, budget),
            _ => {
                unchecked.insert(format!("{path}/"));
            }
        }
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
