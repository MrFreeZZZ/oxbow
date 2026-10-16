//! `git worktree`: more working folders for one repository, each with its own branch checked
//! out, so a hotfix or a review doesn't need stashing and switching. Commits, branches and
//! stashes are shared; `HEAD`, the staging area and the files are each folder's own.
//!
//! The list is git's own (`git worktree list --porcelain`), so worktrees made in a terminal show
//! too. Adding, removing, locking, pruning and repairing are [`crate::worktree::Action`]s like
//! every other change.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::cli::GitCommand;
use crate::error::Result;
use crate::repo::Repo;

/// One working folder of the repository.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// Where it is, as git has it.
    pub path: String,
    /// The folder's name, how Oxbow calls it.
    pub name: String,
    /// The checked-out branch; `None` when `HEAD` is detached there.
    pub branch: Option<String>,
    /// The commit checked out there; `None` before the first commit.
    pub head: Option<String>,
    /// The main working folder, the one that holds the repository (`.git`); it can't be removed.
    pub main: bool,
    /// The folder open here.
    pub current: bool,
    /// Locked, so git doesn't prune or remove it: the reason given, empty when none was.
    pub locked: Option<String>,
    /// The folder is gone: moved, deleted, or on a drive that isn't connected.
    pub missing: bool,
    /// Files with uncommitted changes there, untracked ones included. Not counted for the folder
    /// open here (History has its own row for those) or a missing one.
    pub changed: usize,
}

impl Repo {
    /// Every working folder of the repository, the main one first, as `git worktree list` has
    /// them. A bare main repository is left out: it has no folder to work in.
    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        let out = self.run(&GitCommand::new(["worktree", "list", "--porcelain", "-z"]))?;
        let here = canonical(self.workdir());
        let mut list = parse_list(&out.stdout);
        for tree in list.iter_mut() {
            let path = Path::new(&tree.path);
            tree.missing = tree.missing || !path.is_dir();
            tree.current = !tree.missing && canonical(path) == here;
            if !tree.current && !tree.missing {
                tree.changed = changed_files(path);
            }
        }
        Ok(list)
    }

    /// `path` as a command line here would take it: `../name` next to this folder, relative
    /// inside it, else in full.
    pub(crate) fn shown_path(&self, path: &str) -> String {
        let target = Path::new(path);
        let here = self.workdir();
        if let (Some(parent), Some(name)) = (target.parent(), target.file_name())
            && here.parent().is_some_and(|p| same(p, parent))
        {
            return format!("../{}", name.to_string_lossy());
        }
        if let Ok(inside) = target.strip_prefix(here)
            && !inside.as_os_str().is_empty()
        {
            return inside.display().to_string();
        }
        path.to_owned()
    }

    /// The commands of a new worktree at `path` with `branch` checked out; with `create`, a new
    /// branch of that name made at `start` (`HEAD` when missing).
    pub(crate) fn plan_worktree_add(
        &self,
        path: &str,
        branch: &str,
        create: bool,
        start: Option<&str>,
    ) -> Vec<GitCommand> {
        let shown = self.shown_path(path);
        let name = folder_name(path);
        if create {
            let mut args = vec![
                "worktree".to_owned(),
                "add".to_owned(),
                "-b".to_owned(),
                branch.to_owned(),
                shown,
            ];
            args.extend(start.map(str::to_owned));
            let at = start.unwrap_or("the current commit");
            vec![GitCommand::new(args).comment(format!(
                "-b: create {branch} at {at} and check it out in the new folder {name}"
            ))]
        } else {
            vec![
                GitCommand::new(["worktree", "add", &shown, branch])
                    .comment(format!("a second working folder with {branch} checked out")),
            ]
        }
    }

    /// The commands that delete the worktree at `path`. `force` throws its uncommitted changes
    /// away; `stash` puts them in the stash first instead. A locked one is unlocked first.
    pub(crate) fn plan_worktree_remove(&self, path: &str, force: bool, stash: bool) -> Result<Vec<GitCommand>> {
        let shown = self.shown_path(path);
        let name = folder_name(path);
        let locked = self
            .worktrees()?
            .iter()
            .any(|t| same_path(&t.path, path) && t.locked.is_some());
        let mut commands = Vec::new();
        if locked {
            commands
                .push(GitCommand::new(["worktree", "unlock", &shown]).comment("git doesn't remove a locked worktree"));
        }
        if stash {
            commands.push(
                GitCommand::new([
                    "-C",
                    &shown,
                    "stash",
                    "push",
                    "--include-untracked",
                    "-m",
                    &format!("{name} before removing it"),
                ])
                .comment("-C: run in that folder; its changes go to the stash, which every worktree shares"),
            );
        }
        commands.push(if force && !stash {
            GitCommand::new(["worktree", "remove", "--force", &shown])
                .comment("--force: also throws away the uncommitted changes in it")
        } else {
            GitCommand::new(["worktree", "remove", &shown])
                .comment("deletes the folder; the branch and its commits stay")
        });
        Ok(commands)
    }

    pub(crate) fn plan_worktree_lock(&self, path: &str, reason: Option<&str>) -> Vec<GitCommand> {
        let shown = self.shown_path(path);
        let mut args = vec!["worktree".to_owned(), "lock".to_owned()];
        if let Some(reason) = reason.map(str::trim).filter(|r| !r.is_empty()) {
            args.extend(["--reason".to_owned(), reason.to_owned()]);
        }
        args.push(shown);
        vec![GitCommand::new(args).comment("git won't prune or remove it, even while its folder can't be found")]
    }

    pub(crate) fn plan_worktree_unlock(&self, path: &str) -> Vec<GitCommand> {
        vec![
            GitCommand::new(["worktree", "unlock", &self.shown_path(path)])
                .comment("prune and remove work on it again"),
        ]
    }

    pub(crate) fn plan_worktree_prune(&self) -> Vec<GitCommand> {
        vec![
            GitCommand::new(["worktree", "prune", "--verbose"])
                .comment("forgets every worktree whose folder is gone, unless it is locked; branches stay"),
        ]
    }

    /// A worktree folder moved by hand, now at `path`: point git at it again.
    pub(crate) fn plan_worktree_repair(&self, path: &str) -> Vec<GitCommand> {
        vec![
            GitCommand::new(["worktree", "repair", &self.shown_path(path)])
                .comment("reconnects the moved folder and the repository, both ways"),
        ]
    }
}

/// The records of `git worktree list --porcelain -z`: attribute lines ending in NUL, an empty
/// one after each worktree. The first is always the main one.
fn parse_list(text: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    let mut tree: Option<Worktree> = None;
    let mut bare = false;
    let mut first = true;
    for line in text.split('\0') {
        if line.is_empty() {
            if let Some(done) = tree.take()
                && !bare
            {
                list.push(done);
            }
            bare = false;
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => {
                tree = Some(Worktree {
                    path: value.to_owned(),
                    name: folder_name(value),
                    branch: None,
                    head: None,
                    main: std::mem::take(&mut first),
                    current: false,
                    locked: None,
                    missing: false,
                    changed: 0,
                })
            }
            _ => {
                let Some(tree) = tree.as_mut() else { continue };
                match key {
                    "HEAD" => tree.head = Some(value.to_owned()).filter(|id| id.bytes().any(|b| b != b'0')),
                    "branch" => tree.branch = Some(value.strip_prefix("refs/heads/").unwrap_or(value).to_owned()),
                    "locked" => tree.locked = Some(value.to_owned()),
                    "prunable" => tree.missing = true,
                    "bare" => bare = true,
                    _ => {}
                }
            }
        }
    }
    if let Some(done) = tree
        && !bare
    {
        list.push(done);
    }
    list
}

/// Files with uncommitted changes in the folder at `path`; 0 when git can't tell.
pub(crate) fn changed_files(path: &Path) -> usize {
    let out = crate::cli::run_in(
        path,
        &GitCommand::new(["status", "--porcelain", "-z", "--untracked-files=all"]),
        None,
    );
    let Ok(out) = out else { return 0 };
    let mut count = 0;
    let mut entries = out.stdout.split('\0').filter(|e| !e.is_empty());
    while let Some(entry) = entries.next() {
        count += 1;
        // A rename or copy is followed by its old path.
        if entry.starts_with(['R', 'C']) || entry.get(1..2).is_some_and(|y| y == "R" || y == "C") {
            entries.next();
        }
    }
    count
}

/// The last part of a path, as the worktree's name.
fn folder_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned())
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn same(a: &Path, b: &Path) -> bool {
    canonical(a) == canonical(b)
}

/// Two spellings of one folder, e.g. one through a symlink (`/tmp` on macOS).
pub(crate) fn same_path(a: &str, b: &str) -> bool {
    a == b || same(Path::new(a), Path::new(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_porcelain_list() {
        let text = "worktree /src/acme\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0\
                    worktree /src/acme-hotfix\0HEAD 2222222222222222222222222222222222222222\0branch refs/heads/fix/login\0locked usb drive\0\0\
                    worktree /src/acme-spike\0HEAD 3333333333333333333333333333333333333333\0detached\0prunable gitdir file points to non-existent location\0\0";
        let list = parse_list(text);
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].name, "acme");
        assert!(list[0].main && !list[1].main);
        assert_eq!(list[0].branch.as_deref(), Some("main"));
        assert_eq!(list[1].branch.as_deref(), Some("fix/login"));
        assert_eq!(list[1].locked.as_deref(), Some("usb drive"));
        assert_eq!(list[2].branch, None);
        assert!(list[2].missing);
        assert!(!list[0].missing && list[0].locked.is_none());
    }

    #[test]
    fn leaves_out_a_bare_main_repository() {
        let text = "worktree /src/acme.git\0bare\0\0worktree /src/acme-main\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0";
        let list = parse_list(text);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "acme-main");
        assert!(!list[0].main);
    }
}
