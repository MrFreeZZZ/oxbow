//! The Operation Log: every action Oxbow runs is recorded with the state of the repository
//! before and after it, so any step can be undone, even a hard reset or a discard.
//!
//! A snapshot is a few git objects: the staging area as a tree (`git write-tree`), the working
//! copy as a tree (tracked and untracked files, written through a scratch index), `HEAD`, the
//! stash list and the refs. Each step becomes a commit whose tree holds the trees from before
//! and after it, and whose parents are the commits the refs it moved pointed at, so nothing it
//! needs is garbage collected. Those commits live in the reflog of `refs/oxbow/oplog`, whose tip
//! is kept at `HEAD`'s commit: `git log --all` shows nothing new, and git expires the entries
//! after `gc.reflogExpireUnreachable` (30 days by default). The list itself, with titles and the
//! refs each step moved, is `oxbow/oplog.jsonl` in the git directory of the working tree.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use serde::{Deserialize, Serialize};

use crate::cli::{GitCommand, literal};
use crate::edit::ResetMode;
use crate::error::{Error, Result};
use crate::operation::{MergeMethod, OperationKind};
use crate::repo::Repo;
use crate::worktree::Action;

/// The ref whose reflog keeps the entries' objects alive.
pub const OPLOG_REF: &str = "refs/oxbow/oplog";
/// Untracked files bigger than this stay out of snapshots, and an undo never deletes them.
const MAX_UNTRACKED: u64 = 50 * 1024 * 1024;
/// The empty tree, for a working copy or staging area with nothing in it.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

static KEEP_DAYS: AtomicU32 = AtomicU32::new(30);

/// How many days of the log are kept, from Settings › General.
pub fn set_keep_days(days: u32) {
    KEEP_DAYS.store(days.max(1), Ordering::Relaxed);
}

fn keep_days() -> u32 {
    KEEP_DAYS.load(Ordering::Relaxed)
}

/// Where `HEAD` is: on a branch (which may have no commit yet), or detached at a commit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadState {
    pub branch: Option<String>,
    pub commit: Option<String>,
}

/// A ref a step created, moved or deleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefMove {
    /// The full name, e.g. `refs/heads/main`.
    pub name: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// An entry of the stash list: its commit and the message `git stash list` shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StashRecord {
    pub id: String,
    pub message: String,
}

/// The stash list before and after a step that changed it, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StashMove {
    pub before: Vec<StashRecord>,
    pub after: Vec<StashRecord>,
}

/// The files at one moment: the working copy, and the staging area unless it had conflicts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Trees {
    pub worktree: String,
    pub index: Option<String>,
}

/// One step of the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpEntry {
    /// The commit that keeps the step's objects.
    pub id: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
    /// The icon: commit, discard, reset, checkout, …
    pub kind: String,
    pub title: String,
    pub detail: String,
    /// The action failed or stopped half way, e.g. on conflicts.
    pub failed: bool,
    pub head_before: HeadState,
    pub head_after: HeadState,
    pub refs: Vec<RefMove>,
    pub stash: Option<StashMove>,
    pub before: Trees,
    pub after: Trees,
}

/// The repository at one moment, as far as an undo needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    head: HeadState,
    refs: BTreeMap<String, String>,
    stash: Vec<StashRecord>,
    trees: Trees,
}

impl Repo {
    /// What the repository is like now.
    pub fn snapshot(&self) -> Result<Snapshot> {
        Ok(Snapshot {
            head: self.head_state()?,
            refs: self.all_refs()?,
            stash: self.stash_records()?,
            trees: Trees {
                worktree: self.worktree_tree()?,
                index: self
                    .run(&GitCommand::new(["write-tree"]))
                    .ok()
                    .map(|o| o.stdout.trim().to_owned()),
            },
        })
    }

    /// Steps of the log, newest first, without the ones older than Settings allows.
    pub fn operation_log(&self) -> Result<Vec<OpEntry>> {
        let file = self.oplog_file()?;
        let Ok(text) = std::fs::read_to_string(&file) else {
            return Ok(Vec::new());
        };
        let oldest = now() - i64::from(keep_days()) * 86_400;
        let mut entries: Vec<OpEntry> = text
            .lines()
            .filter_map(|line| serde_json::from_str::<OpEntry>(line).ok())
            .filter(|entry| entry.time >= oldest)
            .collect();
        // git may have expired the objects of old steps already.
        if let Some(first) = entries.first()
            && !self.object_exists(&first.id)
        {
            entries.retain(|entry| self.object_exists(&entry.id));
        }
        entries.reverse();
        Ok(entries)
    }

    /// Record `action` as a step, given the state before and after it. Nothing is recorded when
    /// it changed nothing that can be undone.
    pub fn record(
        &self,
        action: &Action,
        before: &Snapshot,
        after: &Snapshot,
        failed: bool,
    ) -> Result<Option<OpEntry>> {
        let names: BTreeSet<&String> = before.refs.keys().chain(after.refs.keys()).collect();
        let refs: Vec<RefMove> = names
            .into_iter()
            .filter(|name| before.refs.get(*name) != after.refs.get(*name))
            .map(|name| RefMove {
                name: name.clone(),
                before: before.refs.get(name).cloned(),
                after: after.refs.get(name).cloned(),
            })
            .collect();
        let stash = (before.stash != after.stash).then(|| StashMove {
            before: before.stash.clone(),
            after: after.stash.clone(),
        });
        // A fetch or push only moves branches on remotes, which an undo can't take back: they
        // show what the remote has.
        let local = refs.iter().any(|m| !m.name.starts_with("refs/remotes/"));
        if !local && stash.is_none() && before.head == after.head && before.trees == after.trees {
            return Ok(None);
        }
        let (kind, title) = self.describe(action, &before.head);
        let detail = self.detail(action, before, after, &refs);

        // The entry commit: the trees in its tree, every commit it may need as a parent.
        let mut tree = vec![
            format!("040000 tree {}\tbefore-worktree", before.trees.worktree),
            format!("040000 tree {}\tafter-worktree", after.trees.worktree),
        ];
        if let Some(index) = &before.trees.index {
            tree.push(format!("040000 tree {index}\tbefore-index"));
        }
        if let Some(index) = &after.trees.index {
            tree.push(format!("040000 tree {index}\tafter-index"));
        }
        let mut wanted: Vec<String> = refs
            .iter()
            .flat_map(|m| m.before.iter().chain(&m.after))
            .cloned()
            .collect();
        wanted.extend(before.head.commit.iter().chain(&after.head.commit).cloned());
        if let Some(stash) = &stash {
            wanted.extend(stash.before.iter().chain(&stash.after).map(|s| s.id.clone()));
        }
        let mut parents = BTreeSet::new();
        for (id, kind) in self.object_kinds(&wanted)? {
            match kind.as_str() {
                "commit" => {
                    parents.insert(id);
                }
                // A tag object can't be a parent: its text goes into the tree, so it can be
                // written again with the same id.
                "tag" => {
                    let text = self.run(&GitCommand::new(["cat-file", "tag", &id]))?.stdout;
                    let blob = self
                        .run_with_input(
                            &GitCommand::new(["hash-object", "-w", "--stdin"]),
                            Some(text.as_bytes()),
                        )?
                        .stdout;
                    tree.push(format!("100644 blob {}\ttag-{id}", blob.trim()));
                    if let Ok(out) = self.run(&GitCommand::new([
                        "rev-parse",
                        "--verify",
                        "-q",
                        &format!("{id}^{{commit}}"),
                    ])) {
                        parents.insert(out.stdout.trim().to_owned());
                    }
                }
                _ => {}
            }
        }
        tree.sort_by(|a, b| a.split('\t').nth(1).cmp(&b.split('\t').nth(1)));
        let tree_id = self
            .run_with_input(&GitCommand::new(["mktree"]), Some((tree.join("\n") + "\n").as_bytes()))?
            .stdout
            .trim()
            .to_owned();
        let mut args = vec!["commit-tree".to_owned(), tree_id];
        for parent in parents {
            args.extend(["-p".to_owned(), parent]);
        }
        args.extend(["-m".to_owned(), format!("Oxbow: {title}")]);
        let id = self.run(&identity(GitCommand::new(args)))?.stdout.trim().to_owned();
        self.run(&GitCommand::new([
            "update-ref",
            "--create-reflog",
            "-m",
            &format!("oxbow: {title}"),
            OPLOG_REF,
            &id,
        ]))?;
        // The tip goes back to HEAD's commit, so `git log --all` shows no extra commit.
        if let Some(head) = &after.head.commit {
            self.run(&GitCommand::new(["update-ref", "-m", "oxbow: tip", OPLOG_REF, head]))?;
        }
        self.keep_long_enough();

        let entry = OpEntry {
            id,
            time: now(),
            kind: kind.to_owned(),
            title,
            detail,
            failed,
            head_before: before.head.clone(),
            head_after: after.head.clone(),
            refs,
            stash,
            before: before.trees.clone(),
            after: after.trees.clone(),
        };
        let file = self.oplog_file()?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .map_err(io)?;
        writeln!(
            out,
            "{}",
            serde_json::to_string(&entry).map_err(|err| Error::Git(err.to_string()))?
        )
        .map_err(io)?;
        Ok(Some(entry))
    }

    /// The commands that put the repository back to how it was before step `id`, or right
    /// after it with `after`. Every newer step is undone too; branches on remotes are not
    /// touched, since they show what the remote has.
    pub fn plan_restore(&self, id: &str, after: bool) -> Result<Vec<GitCommand>> {
        let log = self.operation_log()?;
        let at = log
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| Error::Git("this step is no longer in the Operation Log".into()))?;
        let entry = &log[at];
        let undone = &log[..if after { at } else { at + 1 }];
        let (head, trees) = if after {
            (&entry.head_after, &entry.after)
        } else {
            (&entry.head_before, &entry.before)
        };
        let index = trees.index.as_deref().ok_or_else(|| {
            Error::Git(
                "the repository was in the middle of a merge or rebase then; pick the step before or after".into(),
            )
        })?;
        if let Some(op) = self.operation()? {
            let what = match op.kind {
                OperationKind::Merge | OperationKind::Squash => "A merge",
                OperationKind::Rebase => "A rebase",
                OperationKind::CherryPick => "A cherry-pick",
                OperationKind::Revert => "A revert",
                OperationKind::StashApply => "Applying a stash",
            };
            return Err(Error::Git(format!("{what} is in progress: finish or abort it first")));
        }

        // The value each ref had then: before the oldest undone step that moved it.
        let mut targets: BTreeMap<&str, Option<&String>> = BTreeMap::new();
        for step in undone.iter().rev() {
            for moved in &step.refs {
                if !moved.name.starts_with("refs/remotes/") {
                    targets.entry(&moved.name).or_insert(moved.before.as_ref());
                }
            }
        }
        let now = self.all_refs()?;
        let mut commands = Vec::new();
        for (name, target) in targets {
            if now.get(name) == target {
                continue;
            }
            let short = short_ref(name);
            commands.push(match target {
                Some(id) => GitCommand::new(["update-ref", "-m", "oxbow: restore", name, id.as_str()]).comment(
                    if now.contains_key(name) {
                        format!("{short} goes back to {}", short_id(id))
                    } else {
                        format!("{short} comes back at {}", short_id(id))
                    },
                ),
                None => GitCommand::new(["update-ref", "-m", "oxbow: restore", "-d", name])
                    .comment(format!("{short} did not exist then")),
            });
        }

        let current = self.head_state()?;
        if current.branch != head.branch || (head.branch.is_none() && current.commit != head.commit) {
            commands.push(match (&head.branch, &head.commit) {
                (Some(branch), _) => GitCommand::new(["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")])
                    .comment(format!("HEAD is on {branch} again; the files follow below")),
                (None, Some(commit)) => {
                    GitCommand::new(["update-ref", "--no-deref", "-m", "oxbow: restore", "HEAD", commit])
                        .comment(format!("HEAD at {}, no branch", short_id(commit)))
                }
                (None, None) => return Err(Error::Git("HEAD had no commit then".into())),
            });
        }

        if let Some(stash) = undone.iter().rev().find_map(|step| step.stash.as_ref())
            && self.stash_records()? != stash.before
        {
            commands
                .push(GitCommand::new(["update-ref", "-d", "refs/stash"]).comment("the stash list is written again"));
            for record in stash.before.iter().rev() {
                commands.push(GitCommand::new(["stash", "store", "-m", &record.message, &record.id]));
            }
        }

        commands.push(GitCommand::new(["read-tree", index]).comment("the staging area as it was"));
        let gone = self.files_not_in(&trees.worktree, index)?;
        if !gone.is_empty() {
            let note = format!(
                "{} that did not exist then; the log keeps them",
                if gone.len() == 1 {
                    "a file".to_owned()
                } else {
                    format!("{} files", gone.len())
                }
            );
            commands.push(
                GitCommand::new(
                    ["clean".to_owned(), "-f".to_owned(), "--".to_owned()]
                        .into_iter()
                        .chain(gone.iter().map(|p| literal(p))),
                )
                .comment(note),
            );
        }
        commands.push(
            GitCommand::new([
                "restore",
                &format!("--source={}", trees.worktree),
                "--worktree",
                "--",
                ":/",
            ])
            .comment("your files as they were"),
        );
        Ok(commands)
    }

    /// Bring back the objects a restore needs that git may have dropped: tag objects are
    /// written again from the text the step kept.
    pub(crate) fn prepare_restore(&self, id: &str) -> Result<()> {
        let listing = self.run(&GitCommand::new(["ls-tree", id]))?.stdout;
        for line in listing.lines() {
            let Some((meta, name)) = line.split_once('\t') else {
                continue;
            };
            let Some(tag) = name.strip_prefix("tag-") else { continue };
            if self.object_exists(tag) {
                continue;
            }
            let blob = meta.split(' ').nth(2).unwrap_or_default();
            let text = self.run(&GitCommand::new(["cat-file", "blob", blob]))?.stdout;
            self.run_with_input(&GitCommand::new(["mktag"]), Some(text.as_bytes()))?;
        }
        Ok(())
    }

    /// Forget every step. Their objects go with git's next clean-up.
    pub(crate) fn clear_operation_log(&self) -> Result<()> {
        let file = self.oplog_file()?;
        if file.exists() {
            std::fs::remove_file(&file).map_err(io)?;
        }
        Ok(())
    }

    /// Files there now (tracked, or untracked and not ignored) that are not in `worktree` and
    /// not tracked by `index`: the ones `git restore` leaves behind. Big untracked files are
    /// never in snapshots, so they are never deleted.
    fn files_not_in(&self, worktree: &str, index: &str) -> Result<Vec<String>> {
        let list = |args: &[&str]| -> Result<BTreeSet<String>> {
            Ok(self
                .run(&GitCommand::new(args.iter().copied()))?
                .stdout
                .split('\0')
                .filter(|p| !p.is_empty() && !p.ends_with('/'))
                .map(str::to_owned)
                .collect())
        };
        let mut keep = list(&["ls-tree", "-r", "-z", "--name-only", worktree])?;
        keep.extend(list(&["ls-tree", "-r", "-z", "--name-only", index])?);
        let tracked = list(&["ls-files", "-z"])?;
        let untracked = list(&["ls-files", "-z", "-o", "--exclude-standard"])?;
        Ok(tracked
            .into_iter()
            .chain(untracked.into_iter().filter(|p| self.small_file(p)))
            .filter(|p| !keep.contains(p) && self.workdir().join(p).symlink_metadata().is_ok())
            .collect())
    }

    fn small_file(&self, path: &str) -> bool {
        self.workdir()
            .join(path)
            .symlink_metadata()
            .is_ok_and(|m| (m.is_file() || m.is_symlink()) && m.len() <= MAX_UNTRACKED)
    }

    /// The working copy as a tree: tracked files as they are on disk, and untracked ones that
    /// are not ignored or too big, written through a scratch index so the real one is untouched.
    fn worktree_tree(&self) -> Result<String> {
        let scratch = self.git_path("oxbow/snapshot.index")?;
        if let Some(dir) = scratch.parent() {
            std::fs::create_dir_all(dir).map_err(io)?;
        }
        let real = self.git_path("index")?;
        if real.exists() {
            std::fs::copy(&real, &scratch).map_err(io)?;
        } else {
            let _ = std::fs::remove_file(&scratch);
        }
        let file = scratch.to_string_lossy().into_owned();
        let with_scratch = |cmd: GitCommand| cmd.env("GIT_INDEX_FILE", file.clone());
        let result = (|| {
            self.run(&with_scratch(GitCommand::new(["add", "-u", "--", ":/"])))?;
            let untracked: Vec<String> = self
                .run(&GitCommand::new(["ls-files", "-z", "-o", "--exclude-standard"]))?
                .stdout
                .split('\0')
                .filter(|p| !p.is_empty() && !p.ends_with('/') && self.small_file(p))
                .map(str::to_owned)
                .collect();
            if !untracked.is_empty() {
                let list = untracked.iter().map(|p| literal(p)).collect::<Vec<_>>().join("\0");
                self.run_with_input(
                    &with_scratch(GitCommand::new([
                        "add",
                        "--pathspec-from-file=-",
                        "--pathspec-file-nul",
                    ])),
                    Some(list.as_bytes()),
                )?;
            }
            Ok(self
                .run(&with_scratch(GitCommand::new(["write-tree"])))?
                .stdout
                .trim()
                .to_owned())
        })();
        let _ = std::fs::remove_file(&scratch);
        // With nothing at all to write, the scratch index has no entries: the empty tree.
        result.map(|tree: String| if tree.is_empty() { EMPTY_TREE.to_owned() } else { tree })
    }

    fn head_state(&self) -> Result<HeadState> {
        let branch = self
            .run(&GitCommand::new(["symbolic-ref", "-q", "HEAD"]))
            .ok()
            .map(|o| o.stdout.trim().trim_start_matches("refs/heads/").to_owned())
            .filter(|b| !b.is_empty());
        let commit = self
            .run(&GitCommand::new(["rev-parse", "-q", "--verify", "HEAD"]))
            .ok()
            .map(|o| o.stdout.trim().to_owned())
            .filter(|c| !c.is_empty());
        Ok(HeadState { branch, commit })
    }

    /// Every ref and the object it points at, except the stash (see [`Self::stash_records`])
    /// and the log's own ref.
    fn all_refs(&self) -> Result<BTreeMap<String, String>> {
        let out = self.run(&GitCommand::new(["for-each-ref", "--format=%(objectname) %(refname)"]))?;
        Ok(out
            .stdout
            .lines()
            .filter_map(|line| line.split_once(' '))
            .filter(|(_, name)| *name != "refs/stash" && !name.starts_with("refs/oxbow/"))
            .map(|(id, name)| (name.to_owned(), id.to_owned()))
            .collect())
    }

    fn stash_records(&self) -> Result<Vec<StashRecord>> {
        if self
            .run(&GitCommand::new(["rev-parse", "-q", "--verify", "refs/stash"]))
            .is_err()
        {
            return Ok(Vec::new());
        }
        let out = self.run(&GitCommand::new([
            "log",
            "-g",
            "--format=%H%x00%gs",
            "refs/stash",
            "--",
        ]))?;
        Ok(out
            .stdout
            .lines()
            .filter_map(|line| line.split_once('\0'))
            .map(|(id, message)| StashRecord {
                id: id.to_owned(),
                message: message.to_owned(),
            })
            .collect())
    }

    /// The kind of each object that exists: `commit`, `tag`, `tree` or `blob`.
    fn object_kinds(&self, ids: &[String]) -> Result<Vec<(String, String)>> {
        let unique: BTreeSet<&String> = ids.iter().collect();
        if unique.is_empty() {
            return Ok(Vec::new());
        }
        let input = unique.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("\n") + "\n";
        let out = self.run_with_input(
            &GitCommand::new(["cat-file", "--batch-check=%(objectname) %(objecttype)"]),
            Some(input.as_bytes()),
        )?;
        Ok(out
            .stdout
            .lines()
            .filter_map(|line| line.split_once(' '))
            .filter(|(_, kind)| *kind != "missing")
            .map(|(id, kind)| (id.to_owned(), kind.to_owned()))
            .collect())
    }

    fn object_exists(&self, id: &str) -> bool {
        self.run(&GitCommand::new(["cat-file", "-e", id])).is_ok()
    }

    fn git_path(&self, name: &str) -> Result<PathBuf> {
        let out = self.run(&GitCommand::new(["rev-parse", "--git-path", name]))?;
        Ok(self.workdir().join(out.stdout.trim()))
    }

    fn oplog_file(&self) -> Result<PathBuf> {
        self.git_path("oxbow/oplog.jsonl")
    }

    /// git expires reflog entries like the log's after 30 days; a longer setting needs a
    /// longer expiry for this one ref.
    fn keep_long_enough(&self) {
        let days = keep_days();
        if days <= 30 {
            return;
        }
        let key = format!("gc.{OPLOG_REF}.reflogExpireUnreachable");
        let value = format!("{days} days");
        if self.config_value(&key).as_deref() != Some(value.as_str()) {
            let _ = self.run(&GitCommand::new(["config", &key, &value]));
        }
    }

    /// The icon and title of the step `action` makes.
    fn describe(&self, action: &Action, head: &HeadState) -> (&'static str, String) {
        let here = head.branch.clone().unwrap_or_else(|| "HEAD".to_owned());
        let files = |paths: &[String]| match paths {
            [one] => one.clone(),
            many => format!("{} files", many.len()),
        };
        let hunk = |lines: &Option<Vec<usize>>, path: &str| match lines {
            Some(lines) if lines.len() == 1 => format!("1 line of {path}"),
            Some(lines) => format!("{} lines of {path}", lines.len()),
            None => format!("a hunk of {path}"),
        };
        match action {
            Action::Stage { paths } => ("stage", format!("Stage {}", files(paths))),
            Action::Unstage { paths } => ("unstage", format!("Unstage {}", files(paths))),
            Action::Discard { paths } => ("discard", format!("Discard changes in {}", files(paths))),
            Action::StageHunk { path, lines, .. } => ("stage", format!("Stage {}", hunk(lines, path))),
            Action::UnstageHunk { path, lines, .. } => ("unstage", format!("Unstage {}", hunk(lines, path))),
            Action::DiscardHunk { path, lines, .. } => ("discard", format!("Discard {}", hunk(lines, path))),
            Action::Ignore { pattern } => ("ignore", format!("Ignore {pattern}")),
            Action::Commit { amend: true, .. } => ("commit", format!("Amend the last commit on {here}")),
            Action::Commit { .. } => ("commit", format!("Commit to {here}")),
            Action::Fetch { remote } => ("fetch", format!("Fetch {}", remote.as_deref().unwrap_or("all remotes"))),
            Action::Pull { remote, branch } => ("pull", format!("Pull {remote}/{branch}")),
            Action::Push {
                remote, branch, force, ..
            } => (
                "push",
                format!("{} {branch} to {remote}", if *force { "Force push" } else { "Push" }),
            ),
            Action::PullAndPush { branch, .. } => ("push", format!("Pull and push {branch}")),
            Action::AbortRebase => ("undo", "Undo the pull".to_owned()),
            Action::Switch { branch, .. } => ("checkout", format!("Check out {branch}")),
            Action::Detach { commit, .. } => ("checkout", format!("Check out {}", short_id(commit))),
            Action::Track { remote, branch, .. } => ("checkout", format!("Check out {branch} from {remote}")),
            Action::CreateBranch { name, .. } => ("branch", format!("Create branch {name}")),
            Action::RenameBranch { from, to, .. } => ("branch", format!("Rename {from} to {to}")),
            Action::DeleteBranch { name, .. } => ("drop", format!("Delete branch {name}")),
            Action::DeleteRemoteBranch { remote, branch } => ("drop", format!("Delete {branch} on {remote}")),
            Action::Merge { branch, method, .. } => match method {
                MergeMethod::Merge => ("merge", format!("Merge {branch} into {here}")),
                MergeMethod::Squash => ("merge", format!("Squash {branch} into {here}")),
                MergeMethod::Rebase => ("rebase", format!("Rebase {here} onto {branch}")),
                MergeMethod::FastForward => ("merge", format!("Fast-forward {here} to {branch}")),
            },
            Action::Continue { .. } => ("merge", "Continue after conflicts".to_owned()),
            Action::Abort => ("undo", "Abort the operation".to_owned()),
            Action::Skip => ("rebase", "Skip a commit".to_owned()),
            Action::Resolve { path, .. } | Action::TakeFile { path, .. } => ("merge", format!("Resolve {path}")),
            Action::CherryPick { commit } => ("cherry", format!("Cherry-pick {} onto {here}", short_id(commit))),
            Action::Revert { commit } => ("revert", format!("Revert {}", short_id(commit))),
            Action::Reset { commit, mode } => (
                "reset",
                format!(
                    "{} reset {here} to {}",
                    match mode {
                        ResetMode::Soft => "Soft",
                        ResetMode::Mixed => "Mixed",
                        ResetMode::Hard => "Hard",
                    },
                    if commit.len() == 40 {
                        short_id(commit)
                    } else {
                        commit.clone()
                    }
                ),
            ),
            Action::Reword { .. } => ("edit", format!("Edit the last message on {here}")),
            Action::StashPush { .. } => ("stash", "Stash changes".to_owned()),
            Action::StashApply { pop: true, .. } => ("pop", "Pop a stash".to_owned()),
            Action::StashApply { .. } => ("pop", "Apply a stash".to_owned()),
            Action::StashDrop { .. } => ("drop", "Drop a stash".to_owned()),
            Action::StashStore { .. } => ("stash", "Put a stash back".to_owned()),
            Action::StashBranch { name, .. } => ("branch", format!("Create branch {name} from a stash")),
            Action::AddRemote { name, .. } => ("remote", format!("Add remote {name}")),
            Action::SetRemoteUrl { name, .. } => ("remote", format!("Change the address of {name}")),
            Action::AddSshKey { .. } => ("remote", "Add an SSH key to the agent".to_owned()),
            Action::RemoveRemote { name } => ("remote", format!("Remove remote {name}")),
            Action::Optimize => ("box", "Optimize the repository".to_owned()),
            Action::CreateTag { name, .. } => ("tag", format!("Create tag {name}")),
            Action::PushTags { remote, .. } => ("tag", format!("Push tags to {remote}")),
            Action::DeleteTag { name, .. } => ("tag", format!("Delete tag {name}")),
            Action::FetchTags { remote } => ("fetch", format!("Fetch tags from {remote}")),
            Action::Restore { id, after } => {
                let title = self
                    .operation_log()
                    .ok()
                    .and_then(|log| log.into_iter().find(|e| &e.id == id))
                    .map_or_else(|| "a step".to_owned(), |e| format!("“{}”", e.title));
                (
                    "undo",
                    if *after {
                        format!("Restore to after {title}")
                    } else {
                        format!("Undo {title}")
                    },
                )
            }
            Action::ClearOperationLog => ("drop", "Clear the Operation Log".to_owned()),
            Action::EditStack { plan } => ("rebase", format!("Edit stack {}", plan.top)),
            Action::PushBranches { remote, branches } => {
                ("push", format!("Push {} branches to {remote}", branches.len()))
            }
            Action::AddToCommit { commit } => (
                "commit",
                format!("Add staged changes to {}", &commit[..commit.len().min(7)]),
            ),
        }
    }

    /// One line on what the step did: the new commit's message, or what moved and changed.
    fn detail(&self, action: &Action, before: &Snapshot, after: &Snapshot, refs: &[RefMove]) -> String {
        let made_commit = matches!(
            action,
            Action::Commit { .. }
                | Action::Reword { .. }
                | Action::CherryPick { .. }
                | Action::Revert { .. }
                | Action::Continue { .. }
        );
        if made_commit
            && after.head.commit != before.head.commit
            && let Some(commit) = &after.head.commit
            && let Ok(out) = self.run(&GitCommand::new(["log", "-1", "--format=%s", commit]))
        {
            return out.stdout.trim().to_owned();
        }
        let mut parts = Vec::new();
        let local: Vec<&RefMove> = refs.iter().filter(|m| !m.name.starts_with("refs/remotes/")).collect();
        if let [one] = local.as_slice() {
            let name = short_ref(&one.name);
            parts.push(match (&one.before, &one.after) {
                (Some(a), Some(b)) => format!("{name} {} → {}", short_id(a), short_id(b)),
                (None, Some(b)) => format!("{name} at {}", short_id(b)),
                (Some(a), None) => format!("{name} was at {}", short_id(a)),
                (None, None) => name,
            });
        } else if !local.is_empty() {
            parts.push(format!("{} refs moved", local.len()));
        }
        let remote = refs.len() - local.len();
        if remote > 0 && local.is_empty() {
            parts.push(format!(
                "{remote} remote {}",
                if remote == 1 {
                    "branch updated"
                } else {
                    "branches updated"
                }
            ));
        }
        let changed = |a: &str, b: &str| {
            self.run(&GitCommand::new(["diff-tree", "-r", "--name-only", "-z", a, b]))
                .map(|o| o.stdout.split('\0').filter(|p| !p.is_empty()).count())
                .unwrap_or(0)
        };
        if before.trees.worktree != after.trees.worktree {
            let n = changed(&before.trees.worktree, &after.trees.worktree);
            if n > 0 {
                parts.push(format!("{n} {} changed", if n == 1 { "file" } else { "files" }));
            }
        } else if before.trees.index != after.trees.index
            && let (Some(a), Some(b)) = (&before.trees.index, &after.trees.index)
        {
            let n = changed(a, b);
            if n > 0 {
                parts.push(format!(
                    "{n} {} in the staging area",
                    if n == 1 { "file" } else { "files" }
                ));
            }
        }
        if before.stash != after.stash {
            parts.push(format!("{} in the stash list", after.stash.len()));
        }
        parts.join(" · ")
    }
}

/// Author and committer for the log's own commits, which must work without a configured name.
fn identity(cmd: GitCommand) -> GitCommand {
    cmd.env("GIT_AUTHOR_NAME", "Oxbow")
        .env("GIT_AUTHOR_EMAIL", "oxbow@localhost")
        .env("GIT_COMMITTER_NAME", "Oxbow")
        .env("GIT_COMMITTER_EMAIL", "oxbow@localhost")
}

fn short_id(id: &str) -> String {
    id[..id.len().min(7)].to_owned()
}

/// `main` for `refs/heads/main`, `tag v1.0` for `refs/tags/v1.0`.
fn short_ref(name: &str) -> String {
    if let Some(branch) = name.strip_prefix("refs/heads/") {
        branch.to_owned()
    } else if let Some(tag) = name.strip_prefix("refs/tags/") {
        format!("tag {tag}")
    } else if let Some(remote) = name.strip_prefix("refs/remotes/") {
        remote.to_owned()
    } else {
        name.to_owned()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn io(err: std::io::Error) -> Error {
    Error::Git(err.to_string())
}
