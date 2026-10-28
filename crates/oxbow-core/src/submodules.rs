//! Submodules: other repositories checked out in a folder here. This repository records which
//! commit of each one it builds with (a "gitlink" in its tree); the folder can be at that commit
//! or another one, have its own uncommitted changes, or not be cloned at all.
//!
//! The list comes from `.gitmodules` and the index (read with gitoxide); what is checked out in
//! each folder is asked of git there, like for worktrees, after the history: see
//! [`crate::folders`].

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use gix::bstr::ByteSlice;
use serde::Serialize;

use crate::action::{ActionEvent, PartState};
use crate::cli::GitCommand;
use crate::error::{Error, FailedPart, Result};
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
    /// Files with uncommitted changes in its folder, untracked ones included; 0 when it isn't
    /// cloned. `None` when `git status` failed there: `status_error` says why.
    pub changed: Option<usize>,
    /// Why `changed` isn't known.
    pub status_error: Option<String>,
    /// Commits checked out in the folder that `recorded` doesn't have.
    pub ahead: usize,
    /// Commits of `recorded` the folder doesn't have checked out.
    pub behind: usize,
    /// The recorded commit isn't in its clone yet: it needs a fetch before the folder can have it.
    pub missing: bool,
    /// Commits checked out in the folder, with no branch there, that no branch, remote branch or
    /// tag of its clone has: checking out another commit leaves them only in its reflog. 0 when
    /// a branch is checked out or the folder is at the recorded commit.
    pub on_no_branch: usize,
    /// What its folder has checked out and whether it has changes (`current`, `branch`,
    /// `changed`, `ahead`, `behind`, `missing`, `on_no_branch`) isn't read yet: [`Repo::folder_states`] reads it
    /// after the history.
    pub pending: bool,
}

/// What a submodule's folder has checked out, as [`crate::folders::FolderState`] tells it.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Checkout {
    pub current: Option<String>,
    pub branch: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub missing: bool,
    pub on_no_branch: usize,
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
    /// `old` and `new` themselves, when the clone has them.
    pub old_commit: Option<CommitBrief>,
    pub new_commit: Option<CommitBrief>,
    pub moved: Moved,
    /// The commits between the two, newest first, at most [`LISTED`].
    pub commits: Vec<CommitBrief>,
    /// How many commits there are between the two in all.
    pub count: usize,
    /// For [`Moved::Sideways`]: commits of `old` that `new` doesn't have.
    pub dropped: usize,
    /// A clone exists to read its commits from.
    pub cloned: bool,
    /// For the working copy: files with uncommitted changes in its folder; `None` when git
    /// couldn't tell.
    pub changed: Option<usize>,
    /// `new` is in its clone but on none of its remote branches: nobody else can fetch it yet.
    pub unpushed: bool,
}

impl Repo {
    /// The repository's submodules in the order of `.gitmodules`; empty when it has none or they
    /// can't be read. Where the difference matters, [`Repo::read_submodules`] tells.
    pub fn submodules(&self) -> Vec<Submodule> {
        self.read_submodules().unwrap_or_default()
    }

    /// The repository's submodules in the order of `.gitmodules`, with what each folder has
    /// checked out; empty when it has none, an error when `.gitmodules` or the index can't be
    /// read. A folder git can't read there has its own [`Submodule::status_error`].
    pub fn read_submodules(&self) -> crate::error::Result<Vec<Submodule>> {
        let mut list = self.submodule_list()?;
        self.fill_folders(&mut [], &mut list)?;
        Ok(list)
    }

    /// The repository's submodules as `.gitmodules` and the index have them, without looking
    /// into their folders: each cloned one is [`Submodule::pending`]. Takes no git process.
    pub fn submodule_list(&self) -> crate::error::Result<Vec<Submodule>> {
        let repo = self.local();
        let Some(list) = repo.submodules().map_err(crate::error::Error::git)? else {
            return Ok(Vec::new());
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
            out.push(Submodule {
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
                changed: if cloned { None } else { Some(0) },
                status_error: None,
                ahead: 0,
                behind: 0,
                missing: false,
                on_no_branch: 0,
                pending: cloned,
            });
        }
        Ok(out)
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
    pub fn submodule_move(
        &self,
        path: &str,
        old: Option<&str>,
        new: Option<&str>,
        changed: Option<usize>,
    ) -> SubmoduleMove {
        // Its name and URL only: its folder is read for the working copy's card, by the caller.
        let info = self
            .submodule_list()
            .ok()
            .and_then(|list| list.into_iter().find(|s| s.path == path));
        let dir = self.submodule_dir(path);
        let mut out = SubmoduleMove {
            path: path.to_owned(),
            title: title(path),
            url: info.as_ref().and_then(|s| s.url.clone()),
            follows: info.as_ref().and_then(|s| s.follows.clone()),
            old: old.map(str::to_owned),
            new: new.map(str::to_owned),
            old_commit: None,
            new_commit: None,
            moved: Moved::Unknown,
            commits: Vec::new(),
            count: 0,
            dropped: 0,
            cloned: dir.is_some(),
            changed,
            unpushed: false,
        };
        if let Some(dir) = dir.as_deref() {
            out.old_commit = old.and_then(|id| brief(dir, id));
            out.new_commit = new.and_then(|id| brief(dir, id));
            out.unpushed = out.new_commit.is_some() && new.is_some_and(|id| !on_remote(dir, id));
        }
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

/// What the submodule folder at `dir` has checked out, against `recorded`, the commit this
/// repository records for it.
pub(crate) fn read_checkout(dir: &Path, recorded: Option<&str>) -> Checkout {
    let mut out = Checkout {
        current: git(dir, ["rev-parse", "--verify", "-q", "HEAD"]).map(|s| s.trim().to_owned()),
        branch: git(dir, ["symbolic-ref", "-q", "--short", "HEAD"])
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty()),
        ..Checkout::default()
    };
    if let (Some(current), Some(recorded)) = (&out.current, recorded)
        && current != recorded
    {
        out.missing = !has_commit(dir, recorded);
        if !out.missing
            && let Some((ahead, behind)) = counts(dir, recorded, current)
        {
            out.ahead = ahead;
            out.behind = behind;
        }
        // Only then: a branch keeps its commits, and the recorded commit is this repository's.
        if out.branch.is_none() {
            out.on_no_branch = git(
                dir,
                [
                    "rev-list",
                    "--count",
                    "HEAD",
                    "--not",
                    "--branches",
                    "--remotes",
                    "--tags",
                ],
            )
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or(0);
        }
    }
    out
}

/// What git prints in `dir`, or `None` when it fails.
fn git<const N: usize>(dir: &Path, args: [&str; N]) -> Option<String> {
    crate::cli::run_in(dir, &GitCommand::new(args), None)
        .ok()
        .map(|out| out.stdout)
}

/// Some remote branch of the clone in `dir` has commit `id`; also when that can't be told.
fn on_remote(dir: &Path, id: &str) -> bool {
    git(dir, ["branch", "-r", "--contains", id]).is_none_or(|out| !out.trim().is_empty())
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

/// Commit `id` of the clone in `dir`, when it has it.
fn brief(dir: &Path, id: &str) -> Option<CommitBrief> {
    let commit = format!("{id}^{{commit}}");
    git(dir, ["log", "-1", "--format=%H%x00%s%x00%an%x00%ct", &commit, "--"])
        .and_then(|out| parse_briefs(&out).into_iter().next())
}

/// Commits of `to` that `from` doesn't have, newest first, at most [`LISTED`].
fn briefs(dir: &Path, from: &str, to: &str) -> Vec<CommitBrief> {
    let limit = format!("-{LISTED}");
    git(
        dir,
        [
            "log",
            &limit,
            "--format=%H%x00%s%x00%an%x00%ct",
            &format!("{from}..{to}"),
        ],
    )
    .map(|out| parse_briefs(&out))
    .unwrap_or_default()
}

/// A submodule whose recorded commit isn't on any of its remotes yet, for the warning before a
/// push.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UnpushedSubmodule {
    pub path: String,
    pub title: String,
    /// The commit `HEAD` records for it.
    pub commit: String,
}

impl Repo {
    /// Submodules whose commit `HEAD` records is in their clone but on none of its remote
    /// branches: pushing this repository would record a commit nobody else can fetch. One that
    /// isn't cloned, or lacks the commit, is left out: it came from elsewhere.
    pub fn unpushed_submodules(&self) -> Vec<UnpushedSubmodule> {
        self.submodule_list()
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.cloned)
            .filter_map(|s| {
                let commit = s.committed.clone()?;
                let dir = self.workdir().join(&s.path);
                if !has_commit(&dir, &commit) {
                    return None;
                }
                let on_remote =
                    git(&dir, ["branch", "-r", "--contains", &commit]).is_none_or(|out| !out.trim().is_empty());
                (!on_remote).then_some(UnpushedSubmodule {
                    path: s.path,
                    title: s.title,
                    commit,
                })
            })
            .collect()
    }

    /// `git submodule update` for `paths` (all when empty): the folders get the commits this
    /// repository records, cloned first where they aren't yet. With `latest`, they get the
    /// newest commit of the branch each follows instead. More than one goes one at a time, each
    /// with its own command, so one that fails doesn't stop the others: see
    /// [`Repo::update_submodules`].
    pub(crate) fn plan_submodule_update(&self, paths: &[String], latest: bool) -> Vec<GitCommand> {
        if let [path] = paths {
            return vec![update_command(Some(path), latest)];
        }
        let each = self.submodules_among(paths);
        if each.is_empty() {
            return vec![update_command(None, latest)];
        }
        // The comment once: the commands differ only in the path.
        each.iter()
            .enumerate()
            .map(|(at, path)| {
                let mut command = update_command(Some(path), latest);
                if at > 0 {
                    command.comment = None;
                } else if let Some(comment) = &mut command.comment {
                    comment.push_str("; one at a time, so one that fails doesn't stop the others");
                }
                command
            })
            .collect()
    }

    /// The paths of the submodules among `paths` (all when empty), in the order of `.gitmodules`.
    pub(crate) fn submodules_among(&self, paths: &[String]) -> Vec<String> {
        self.submodule_list()
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.path)
            .filter(|p| paths.is_empty() || paths.contains(p))
            .collect()
    }

    /// Run the update of [`Repo::plan_submodule_update`] for the submodules among `paths`, one
    /// at a time, telling which starts and how each ended. One that fails doesn't stop the
    /// others; when any failed, the answer is [`Error::PartlyDone`].
    pub(crate) fn update_submodules(
        &self,
        paths: &[String],
        latest: bool,
        on_event: &mut dyn FnMut(ActionEvent),
        cancel: &AtomicBool,
    ) -> Result<String> {
        let names = self.submodules_among(paths);
        let commands = names.iter().map(|path| update_command(Some(path), latest));
        on_event(ActionEvent::Parts { names: names.clone() });
        let mut failed = Vec::new();
        for (name, command) in names.iter().zip(commands) {
            on_event(ActionEvent::Part {
                name: name.clone(),
                state: PartState::Started,
                error: None,
                inner: None,
            });
            let command = crate::ssh::interactive(command);
            on_event(ActionEvent::Command {
                display: command.display(),
            });
            let ran = crate::console::main(|| {
                self.run_streaming(&command, &mut |line| on_event(ActionEvent::Line(line)), cancel)
            });
            match ran {
                Ok(_) => on_event(ActionEvent::Part {
                    name: name.clone(),
                    state: PartState::Done,
                    error: None,
                    inner: None,
                }),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(err) => {
                    let output = match err {
                        Error::Command { output, .. } => output.trim().to_owned(),
                        other => other.to_string(),
                    };
                    let inner = inner_submodule(&output, name);
                    on_event(ActionEvent::Part {
                        name: name.clone(),
                        state: PartState::Failed,
                        error: Some(output.clone()),
                        inner: inner.clone(),
                    });
                    failed.push(FailedPart {
                        name: name.clone(),
                        error: output,
                        inner,
                    });
                }
            }
        }
        if failed.is_empty() {
            Ok(String::new())
        } else {
            Err(Error::PartlyDone {
                of: names.len(),
                failed,
            })
        }
    }

    /// `git submodule sync`: copy the URLs in `.gitmodules` to the settings of `paths` (all when
    /// empty), after a submodule moved to another address.
    pub(crate) fn plan_submodule_sync(&self, paths: &[String]) -> Vec<GitCommand> {
        let mut args = vec!["submodule".to_owned(), "sync".to_owned(), "--recursive".to_owned()];
        if !paths.is_empty() {
            args.push("--".to_owned());
            args.extend(paths.iter().cloned());
        }
        vec![GitCommand::new(args).comment("take the URLs from .gitmodules, so fetches go to the new address")]
    }

    /// `git submodule add`: clone `url` into `path` and stage it with its `.gitmodules` entry;
    /// with `branch`, it follows that branch for Pull Latest.
    pub(crate) fn plan_submodule_add(&self, url: &str, path: &str, branch: Option<&str>) -> Vec<GitCommand> {
        let mut args = vec!["submodule".to_owned(), "add".to_owned()];
        let mut note = "clones it and stages .gitmodules and the folder for the next commit".to_owned();
        if let Some(branch) = branch.filter(|b| !b.is_empty()) {
            args.extend(["-b".to_owned(), branch.to_owned()]);
            note = format!("-b: check out {branch} and follow it; {note}");
        }
        args.extend(["--".to_owned(), url.to_owned(), path.to_owned()]);
        vec![GitCommand::new(args).comment(note).with_progress()]
    }

    /// Remove the submodule at `path`: empty its folder and forget it, then delete it from the
    /// index and `.gitmodules`. Its clone stays in `.git/modules`, so adding it back is quick.
    pub(crate) fn plan_submodule_remove(&self, path: &str) -> Vec<GitCommand> {
        vec![
            GitCommand::new(["submodule", "deinit", "-f", "--", path])
                .comment("-f: empty the folder even with changes in it, and forget its settings in .git/config"),
            GitCommand::new(["rm", "-f", "--", path])
                .comment("remove the folder and its .gitmodules entry, staged for the next commit"),
        ]
    }

    /// The command that brings the submodules along after pulling `remote`/`branch`: when there
    /// are submodules, or the remote branch has them as of the last fetch, e.g. its first one.
    pub(crate) fn submodules_after_pull(&self, remote: &str, branch: &str) -> Option<GitCommand> {
        let incoming = format!("refs/remotes/{remote}/{branch}:.gitmodules");
        if self.submodule_list().unwrap_or_default().is_empty()
            && self.run(&GitCommand::new(["cat-file", "-e", &incoming])).is_err()
        {
            return None;
        }
        Some(submodule_update())
    }
}

/// `git submodule update` of `path` (all without one), as [`Repo::plan_submodule_update`] says.
fn update_command(path: Option<&String>, latest: bool) -> GitCommand {
    let mut args = vec!["submodule".to_owned(), "update".to_owned()];
    let note = if latest {
        args.push("--remote".to_owned());
        "--remote: fetch it and check out the newest commit of the branch it follows; commit the result to record it"
    } else {
        args.push("--init".to_owned());
        "--init: clone it if it isn't yet; then check out the commit this repository records"
    };
    args.push("--recursive".to_owned());
    if let Some(path) = path {
        args.push("--".to_owned());
        args.push(path.clone());
    }
    GitCommand::new(args)
        .comment(format!("{note}; --recursive: its submodules too"))
        .with_progress()
}

/// The submodule inside `top` that git's `output` says failed, as a path from this repository,
/// e.g. `libs/app/vendor/icons` for `fatal: Unable to checkout '…' in submodule path
/// 'libs/app/vendor/icons'`; `None` when it was `top` itself.
pub(crate) fn inner_submodule(output: &str, top: &str) -> Option<String> {
    output
        .split("submodule path '")
        .skip(1)
        .filter_map(|rest| rest.split('\'').next())
        .filter(|path| path.len() > top.len() && path.starts_with(top) && path[top.len()..].starts_with('/'))
        .max_by_key(|path| path.len())
        .map(str::to_owned)
}

/// `git submodule update` after a pull: the submodules check out the commits recorded now.
pub(crate) fn submodule_update() -> GitCommand {
    GitCommand::new(["submodule", "update", "--init", "--recursive"])
        .comment("the submodules follow: check out the commits the pulled commits record")
        .with_progress()
}
