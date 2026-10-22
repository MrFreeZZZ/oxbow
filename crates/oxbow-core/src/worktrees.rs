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
    /// open here (History has its own row for those) or a missing one: 0. `None` when `git
    /// status` failed there, which is not the same as no changes: `status_error` says why.
    pub changed: Option<usize>,
    /// Why `changed` isn't known.
    pub status_error: Option<String>,
    /// `changed` isn't read yet: [`Repo::folder_states`] reads it after the history.
    pub pending: bool,
}

impl Repo {
    /// Every working folder of the repository, the main one first, as `git worktree list` has
    /// them, with their counts of changed files. A bare main repository is left out: it has no
    /// folder to work in.
    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        let mut list = self.worktree_list()?;
        self.fill_folders(&mut list, &mut [])?;
        Ok(list)
    }

    /// [`Repo::worktrees`] without counting their changed files, which takes a `git status` in
    /// each: the other folders that are there are [`Worktree::pending`].
    pub fn worktree_list(&self) -> Result<Vec<Worktree>> {
        let out = self.run(&GitCommand::new(["worktree", "list", "--porcelain", "-z"]))?;
        let here = canonical(self.workdir());
        let mut list = parse_list(&out.stdout);
        for tree in list.iter_mut() {
            let path = Path::new(&tree.path);
            tree.missing = tree.missing || !path.is_dir();
            tree.current = !tree.missing && canonical(path) == here;
            if !tree.current && !tree.missing {
                tree.changed = None;
                tree.pending = true;
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
            .worktree_list()?
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

/// Why a repository's folder can't be opened when it isn't where it was.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum GoneWhy {
    /// It is on a disk or network drive that isn't connected: [`GoneFolder::disk`] names it.
    Disk,
    /// It is in the Trash, at [`GoneFolder::found`].
    Trash,
    /// It was moved: Oxbow found it at [`GoneFolder::found`].
    Moved,
    /// Deleted, or moved somewhere Oxbow didn't look.
    Missing,
    /// It is there, but the system doesn't let Oxbow read it.
    NoAccess,
}

/// A repository folder that isn't there any more, and what is still known about it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoneFolder {
    pub path: String,
    pub name: String,
    pub why: GoneWhy,
    /// The disk it is on, when that isn't connected.
    pub disk: Option<String>,
    /// Where it is now, when Oxbow found it: moved next to where it was, or in the Trash.
    pub found: Option<String>,
    /// When it is a worktree of another folder: what the repository still has about it.
    pub worktree: Option<GoneWorktree>,
}

/// What a repository keeps about one of its worktrees, which stays when the folder goes.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GoneWorktree {
    /// The repository's main folder; `None` when it isn't there either.
    pub main: Option<String>,
    /// The branch checked out in the main folder, which Open Main Folder shows instead.
    pub main_branch: Option<String>,
    /// The branch checked out there; `None` when `HEAD` was detached.
    pub branch: Option<String>,
    /// The commit checked out there when `HEAD` was detached.
    pub head: Option<String>,
    /// Locked, so Prune keeps it: the reason given, empty when none was.
    pub locked: Option<String>,
}

/// Why the folder at `path` can't be opened, or `None` when it is there. `known` are folders of
/// repositories it may be a worktree of (the one open before, the recent ones): git keeps a
/// worktree's records in the main repository, which tell its branch and recognize the folder
/// where it was moved.
pub fn gone_folder(path: &Path, known: &[PathBuf]) -> Option<GoneFolder> {
    let mut why = match std::fs::metadata(path) {
        // macOS lets the folder be seen but not read when Oxbow isn't allowed into it.
        Ok(_) => match std::fs::read_dir(path) {
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => GoneWhy::NoAccess,
            _ => return None,
        },
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => GoneWhy::NoAccess,
        Err(_) => GoneWhy::Missing,
    };
    let disk = (why == GoneWhy::Missing).then(|| unplugged_disk(path)).flatten();
    if disk.is_some() {
        why = GoneWhy::Disk;
    }
    let record = known.iter().find_map(|dir| WorktreeRecord::of(dir, path));
    let mut found = None;
    if why == GoneWhy::Missing
        && let Some((at, trash)) = record.as_ref().and_then(|r| r.find(path))
    {
        why = if trash { GoneWhy::Trash } else { GoneWhy::Moved };
        found = Some(at.display().to_string());
    }
    Some(GoneFolder {
        path: path.display().to_string(),
        name: folder_name(&path.display().to_string()),
        why,
        disk,
        found,
        worktree: record.map(|r| r.about()),
    })
}

/// The folder that was at `old` is at `new` now: check that it is the same folder and point git
/// at its new place with `git worktree repair`. For a worktree that is the one whose records
/// it holds; a main folder only needs to be a repository, and its worktrees are pointed back at
/// it. Answers the folder to open.
pub fn reconnect_folder(old: &Path, new: &Path, known: &[PathBuf]) -> Result<PathBuf> {
    let name = folder_name(&old.display().to_string());
    let new = canonical(new);
    if let Some(record) = known.iter().find_map(|dir| WorktreeRecord::of(dir, old)) {
        if crate::folders::git_dir(&new).map(|dir| canonical(&dir)) != Some(canonical(&record.dir)) {
            return Err(crate::error::Error::Git(format!(
                "{} isn't the worktree {name}: choose the folder {name} was moved to",
                new.display()
            )));
        }
        crate::cli::run_in(&new, &GitCommand::new(["worktree", "repair"]), None)?;
        return Ok(new);
    }
    let repo = Repo::open(&new)?;
    let common = repo.common_dir();
    let linked = std::fs::read_dir(common.join("worktrees")).is_ok_and(|mut entries| entries.next().is_some());
    if linked {
        // Worktrees whose folders are gone too can't be pointed back: git says so, and the
        // others are fixed all the same.
        let _ = crate::cli::run_in(repo.workdir(), &GitCommand::new(["worktree", "repair"]), None);
    }
    Ok(repo.workdir().to_path_buf())
}

/// The disk `path` is on, when the folder disks are connected at (`/Volumes`, `/media`) doesn't
/// have it, or the drive letter is not there.
fn unplugged_disk(path: &Path) -> Option<String> {
    let missing: Vec<&Path> = path
        .ancestors()
        .take_while(|dir| !dir.as_os_str().is_empty() && std::fs::metadata(dir).is_err())
        .collect();
    let outermost = missing.last()?;
    if outermost.parent().is_none() {
        return Some(outermost.display().to_string());
    }
    let under = |dir: &Path, depth: usize, mounts: &[&str]| {
        dir.ancestors()
            .nth(depth)
            .is_some_and(|up| mounts.iter().any(|m| up == Path::new(m)))
    };
    // Linux mounts a disk at /media/<user>/<disk>, macOS at /Volumes/<disk>.
    let disk = missing
        .iter()
        .find(|dir| under(dir, 2, &["/media", "/run/media"]))
        .or_else(|| {
            missing
                .iter()
                .find(|dir| under(dir, 1, &["/Volumes", "/media", "/mnt"]))
        })?;
    disk.file_name().map(|n| n.to_string_lossy().into_owned())
}

/// A worktree's folder in its main repository (`.git/worktrees/<id>`), which keeps its `HEAD`
/// and where its folder was.
struct WorktreeRecord {
    dir: PathBuf,
    /// The repository's own git folder, which keeps the main folder's `HEAD`.
    common: PathBuf,
    /// The repository's main folder, when it has one.
    main: Option<PathBuf>,
}

impl WorktreeRecord {
    /// The record of the worktree that was at `path`, in the repository of the folder `known`.
    fn of(known: &Path, path: &Path) -> Option<Self> {
        let git = crate::folders::git_dir(known)?;
        let common = match std::fs::read_to_string(git.join("commondir")) {
            Ok(text) => git.join(text.trim()),
            Err(_) => git,
        };
        let common = canonical(&common);
        let dir = std::fs::read_dir(common.join("worktrees"))
            .ok()?
            .flatten()
            .find_map(|entry| {
                let text = std::fs::read_to_string(entry.path().join("gitdir")).ok()?;
                // It names the folder's `.git` file.
                let was = Path::new(text.trim()).parent()?;
                same_place(was, path).then(|| entry.path())
            })?;
        let main = (common.file_name().is_some_and(|n| n == ".git"))
            .then(|| common.parent().map(Path::to_path_buf))
            .flatten();
        Some(WorktreeRecord { dir, common, main })
    }

    fn about(&self) -> GoneWorktree {
        let head = std::fs::read_to_string(self.dir.join("HEAD")).unwrap_or_default();
        let head = head.trim();
        let branch = branch_of(head);
        GoneWorktree {
            main_branch: branch_of(
                std::fs::read_to_string(self.common.join("HEAD"))
                    .unwrap_or_default()
                    .trim(),
            ),
            main: self
                .main
                .as_ref()
                .filter(|main| main.is_dir())
                .map(|main| main.display().to_string()),
            head: (branch.is_none() && !head.is_empty()).then(|| head.to_owned()),
            branch,
            locked: std::fs::read_to_string(self.dir.join("locked"))
                .ok()
                .map(|why| why.trim().to_owned()),
        }
    }

    /// Where the folder went: one of the folders next to where it was or next to the main
    /// folder whose `.git` points at this record, else one in the Trash. True when in the Trash.
    fn find(&self, was: &Path) -> Option<(PathBuf, bool)> {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let places = [
            was.parent().map(Path::to_path_buf),
            self.main.as_ref().and_then(|m| m.parent().map(Path::to_path_buf)),
        ]
        .into_iter()
        .flatten()
        .map(|dir| (dir, false))
        .chain(
            home.iter()
                .flat_map(|home| [home.join(".Trash"), home.join(".local/share/Trash/files")])
                .map(|dir| (dir, true)),
        );
        let record = canonical(&self.dir);
        for (place, trash) in places {
            let Ok(entries) = std::fs::read_dir(&place) else {
                continue;
            };
            // A folder of thousands is not looked through to the end.
            for entry in entries.flatten().take(2000) {
                let candidate = entry.path();
                if std::fs::symlink_metadata(candidate.join(".git")).is_ok_and(|m| m.is_file())
                    && crate::folders::git_dir(&candidate).is_some_and(|dir| canonical(&dir) == record)
                {
                    return Some((candidate, trash));
                }
            }
        }
        None
    }
}

/// The branch a `HEAD` file names; `None` when it holds a commit.
fn branch_of(head: &str) -> Option<String> {
    head.strip_prefix("ref: ")
        .map(|r| r.strip_prefix("refs/heads/").unwrap_or(r).to_owned())
}

/// `a` and `b` are one place, also when neither is there any more: their parents are compared
/// as the system resolves them (`/tmp` and `/private/tmp` on macOS), their names as they are.
fn same_place(a: &Path, b: &Path) -> bool {
    a == b
        || (a.file_name() == b.file_name()
            && a.file_name().is_some()
            && match (a.parent(), b.parent()) {
                (Some(x), Some(y)) => same(x, y),
                _ => false,
            })
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
                    changed: Some(0),
                    status_error: None,
                    pending: false,
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

/// Files with uncommitted changes in the folder at `path`. An error when git can't tell, which
/// callers must not show as 0.
pub(crate) fn changed_files(path: &Path) -> Result<usize> {
    // --no-optional-locks: git doesn't write that folder's index while it looks, so a commit
    // made there meanwhile doesn't find it locked, and the index stays as its owner left it.
    let out = crate::cli::run_in(
        path,
        &GitCommand::new([
            "--no-optional-locks",
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=all",
        ]),
        None,
    )?;
    let mut count = 0;
    let mut entries = out.stdout.split('\0').filter(|e| !e.is_empty());
    while let Some(entry) = entries.next() {
        count += 1;
        // A rename or copy is followed by its old path.
        if entry.starts_with(['R', 'C']) || entry.get(1..2).is_some_and(|y| y == "R" || y == "C") {
            entries.next();
        }
    }
    Ok(count)
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
