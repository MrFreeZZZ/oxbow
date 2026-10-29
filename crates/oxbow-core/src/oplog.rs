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
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::action::Action;
use crate::cli::{GitCommand, literal};
use crate::edit::ResetMode;
use crate::error::{Error, Result};
use crate::fix::Fix;
use crate::operation::{MergeMethod, OperationKind};
use crate::repo::Repo;

/// The ref whose reflog keeps the entries' objects alive.
pub const OPLOG_REF: &str = "refs/oxbow/oplog";
/// Untracked files bigger than this stay out of snapshots, and an undo never deletes them.
pub const MAX_UNTRACKED: u64 = 50 * 1024 * 1024;
/// The empty tree, for a working copy or staging area with nothing in it.
pub(crate) const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

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
    /// Until when the step can be gone back to, in seconds since the Unix epoch: the sooner of
    /// Settings' "Keep undo history for" and git's expiry of the log's reflog entries.
    #[serde(default)]
    pub expires: i64,
    /// Local branches (full names) the step rewrote rather than moved forward, e.g. by a rebase
    /// or an amend: their old and new commits can be compared as a series. `None` in entries
    /// written before Oxbow kept it.
    #[serde(default)]
    pub rewrites: Option<Vec<String>>,
}

/// Where Undo or Restore to Here takes the repository: the steps it undoes, newest first, and
/// `HEAD`, the files and the value of every ref those steps moved, as they were then.
#[derive(Debug, Clone)]
pub(crate) struct RestorePoint {
    pub undone: Vec<OpEntry>,
    pub head: HeadState,
    pub trees: Trees,
    /// Local refs only: branches on remotes show what the remote has, so they are not touched.
    pub refs: BTreeMap<String, Option<String>>,
}

/// The repository now beside how it was at a step of the log: what Undo or Restore to Here would
/// take back. The comparison runs from the files then to the files now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotComparison {
    pub comparison: crate::compare::Comparison,
    /// Refs that point elsewhere now: `before` is then, `after` is now.
    pub refs: Vec<RefMove>,
    pub head_then: HeadState,
    pub head_now: HeadState,
    /// The staging area then differs from the one now; the diff shows the files on disk only.
    pub staging_differs: bool,
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
        // The files take the longest; HEAD, the refs and the stash are read beside them.
        let helpers = crate::console::Helpers::here();
        let stop = crate::cli::current_stop();
        let (worktree, rest) = std::thread::scope(|scope| {
            let worktree = scope.spawn(|| {
                helpers.help(|| match &stop {
                    Some(stop) => crate::cli::stoppable(stop, || self.worktree_tree()),
                    None => self.worktree_tree(),
                })
            });
            let index = self.index_tree();
            let rest = (|| Ok((index?, self.head_state()?, self.all_refs()?, self.stash_records()?)))();
            (worktree.join(), rest)
        });
        helpers.join();
        let worktree = worktree.map_err(|_| Error::Git("the snapshot of the files failed".into()))??;
        let (index, head, refs, stash) = rest?;
        Ok(Snapshot {
            head,
            refs,
            stash,
            trees: Trees { worktree, index },
        })
    }

    /// What the repository is like after a step that changed only the index (staging): `before`
    /// with the index as it is now. Files, branches and stashes are as they were, so the files
    /// aren't read again: that is most of a snapshot's cost.
    pub(crate) fn snapshot_of_index(&self, before: &Snapshot) -> Result<Snapshot> {
        Ok(Snapshot {
            trees: Trees {
                worktree: before.trees.worktree.clone(),
                index: self.index_tree()?,
            },
            ..before.clone()
        })
    }

    /// Steps of the log, newest first, without the ones older than Settings allows.
    pub fn operation_log(&self) -> Result<Vec<OpEntry>> {
        let file = self.oplog_file()?;
        let oldest = now() - i64::from(keep_days()) * 86_400;
        let Some(all) = read_log(&file)? else {
            return Ok(Vec::new());
        };
        let mut entries: Vec<OpEntry> = all.iter().filter(|entry| entry.time >= oldest).cloned().collect();
        let days = self.snapshot_days();
        for entry in &mut entries {
            entry.expires = entry.time + i64::from(days) * 86_400;
            if entry.rewrites.is_none() {
                // Older entries: only the kinds of step that rewrite are worth asking git about.
                let may = matches!(entry.kind.as_str(), "rebase" | "edit" | "push")
                    || entry.title.starts_with("Amend ")
                    || entry.title.starts_with("Add ");
                entry.rewrites = Some(if may { self.rewrites(&entry.refs) } else { Vec::new() });
            }
        }
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

        let rewrites = Some(self.rewrites(&refs));
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
            expires: 0,
            rewrites,
        };
        let file = self.oplog_file()?;
        if let Some(dir) = file.parent() {
            make_folder(dir)?;
        }
        // Said with the file, for the message that the step isn't in the log.
        let cant_write = |err: std::io::Error| Error::Git(format!("can't write .git/oxbow/oplog.jsonl: {err}"));
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .map_err(cant_write)?;
        writeln!(
            out,
            "{}",
            serde_json::to_string(&entry).map_err(|err| Error::Git(err.to_string()))?
        )
        .map_err(cant_write)?;
        Ok(Some(OpEntry {
            expires: entry.time + i64::from(self.snapshot_days()) * 86_400,
            ..entry
        }))
    }

    /// The commands that put the repository back to how it was before step `id`, or right
    /// after it with `after`. Every newer step is undone too; branches on remotes are not
    /// touched, since they show what the remote has.
    pub fn plan_restore(&self, id: &str, after: bool) -> Result<Vec<GitCommand>> {
        let point = self.restore_point(id, after)?;
        let (head, trees, undone) = (&point.head, &point.trees, &point.undone);
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
                OperationKind::Am => "Applying patches",
                OperationKind::StashApply => "Applying a stash",
            };
            return Err(Error::Git(format!("{what} is in progress: finish or abort it first")));
        }
        if self.bisect()?.is_some() {
            return Err(Error::Git("A bisect is in progress: stop or finish it first".into()));
        }

        let now = self.all_refs()?;
        let mut commands = Vec::new();
        for (name, target) in &point.refs {
            let (name, target) = (name.as_str(), target.as_ref());
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

    /// The steps Undo (`after` false) or Restore to Here (`after` true) on step `id` takes back,
    /// and the state it goes to.
    pub(crate) fn restore_point(&self, id: &str, after: bool) -> Result<RestorePoint> {
        let mut log = self.operation_log()?;
        let at = log
            .iter()
            .position(|entry| entry.id == id)
            .ok_or_else(|| Error::Git("this step is no longer in the Operation Log".into()))?;
        let entry = log[at].clone();
        log.truncate(if after { at } else { at + 1 });
        let (head, trees) = if after {
            (entry.head_after, entry.after)
        } else {
            (entry.head_before, entry.before)
        };
        // The value each ref had then: before the oldest undone step that moved it.
        let mut refs = BTreeMap::new();
        for step in log.iter().rev() {
            for moved in &step.refs {
                if !moved.name.starts_with("refs/remotes/") {
                    refs.entry(moved.name.clone()).or_insert_with(|| moved.before.clone());
                }
            }
        }
        Ok(RestorePoint {
            undone: log,
            head,
            trees,
            refs,
        })
    }

    /// The repository now beside how it was before step `id`, or right after it with `after`:
    /// the files that differ, and the branches and tags that point elsewhere.
    pub fn compare_snapshot(&self, id: &str, after: bool) -> Result<SnapshotComparison> {
        let point = self.restore_point(id, after)?;
        let now_refs = self.all_refs()?;
        let refs = point
            .refs
            .iter()
            .filter(|(name, then)| now_refs.get(*name) != then.as_ref())
            .map(|(name, then)| RefMove {
                name: name.clone(),
                before: then.clone(),
                after: now_refs.get(name).cloned(),
            })
            .collect();
        let then = point.trees.worktree.clone();
        let now = self.worktree_tree()?;
        let staging_now = self
            .run(&GitCommand::new(["write-tree"]))
            .ok()
            .map(|o| o.stdout.trim().to_owned());
        let files = self
            // Counted by the Compare view, after the first page of diffs.
            .tree_files_with(&then, &now, None, &std::sync::atomic::AtomicBool::new(false))?
            .into_iter()
            .map(|file| crate::compare::CompareFile {
                file,
                last: None,
                only_base: false,
            })
            .collect();
        Ok(SnapshotComparison {
            comparison: crate::compare::Comparison {
                base_id: then.clone(),
                target_id: now.clone(),
                merge_base: None,
                ahead: Vec::new(),
                behind: Vec::new(),
                ahead_count: 0,
                behind_count: 0,
                mode: crate::compare::CompareMode::Tips,
                from: then.clone(),
                files,
                command: format!("git diff {} {}", short_id(&then), short_id(&now)),
            },
            refs,
            head_then: point.head,
            head_now: self.head_state()?,
            staging_differs: point.trees.index != staging_now,
        })
    }

    /// How many days a step can be gone back to: the setting, unless git expires the entries
    /// that keep its objects sooner.
    fn snapshot_days(&self) -> u32 {
        let git = self
            .config_value(&format!("gc.{OPLOG_REF}.reflogExpireUnreachable"))
            .or_else(|| self.config_value("gc.reflogExpireUnreachable"))
            .map_or(Some(30), |value| expiry_days(&value));
        git.map_or(keep_days(), |git| git.min(keep_days()))
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

    pub(crate) fn small_file(&self, path: &str) -> bool {
        self.workdir()
            .join(path)
            .symlink_metadata()
            .is_ok_and(|m| (m.is_file() || m.is_symlink()) && m.len() <= MAX_UNTRACKED)
    }

    /// A copy of the index of its own to work on, so the real one is untouched. Each call has
    /// its own: a compare running next to a step must not delete the step's.
    pub(crate) fn scratch_index(&self) -> Result<std::path::PathBuf> {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let scratch = self
            .git_path("oxbow")?
            .join(format!("snapshot-{}-{n}.index", std::process::id()));
        if let Some(dir) = scratch.parent() {
            make_folder(dir)?;
        }
        let real = self.git_path("index")?;
        if real.exists() {
            std::fs::copy(&real, &scratch).map_err(io)?;
            // The copy keeps the index's time, or git takes a file changed in the same second
            // as the index for unchanged (same size and time) and the snapshot misses the edit.
            let modified = std::fs::metadata(&real).and_then(|m| m.modified()).map_err(io)?;
            std::fs::File::options()
                .write(true)
                .open(&scratch)
                .and_then(|f| f.set_modified(modified))
                .map_err(io)?;
        } else {
            let _ = std::fs::remove_file(&scratch);
        }
        Ok(scratch)
    }

    /// What the index stages, as a tree; `None` while it has conflicts, as in a merge or rebase
    /// that stopped. Written from a copy: `write-tree` on the real index may take `index.lock`
    /// and fail while anything else holds it, which is no reason for a snapshot without it.
    fn index_tree(&self) -> Result<Option<String>> {
        let scratch = self.scratch_index()?;
        let with_scratch = |cmd: GitCommand| cmd.env("GIT_INDEX_FILE", scratch.to_string_lossy().into_owned());
        // write-tree refuses an index with conflicts: only then are they looked for.
        let result = self
            .run(&with_scratch(GitCommand::new(["write-tree"])))
            .map(|out| Some(out.stdout.trim().to_owned()))
            .or_else(|err| {
                let conflicts = self.run(&with_scratch(GitCommand::new(["ls-files", "-u", "-z"])))?;
                if conflicts.stdout.is_empty() {
                    Err(err)
                } else {
                    Ok(None)
                }
            });
        let _ = std::fs::remove_file(&scratch);
        result
    }

    /// The working copy as a tree: tracked files as they are on disk, and untracked ones that
    /// are not ignored or too big, written through a scratch index so the real one is untouched.
    fn worktree_tree(&self) -> Result<String> {
        let scratch = self.scratch_index()?;
        let file = scratch.to_string_lossy().into_owned();
        let with_scratch = |cmd: GitCommand| cmd.env("GIT_INDEX_FILE", file.clone());
        let result = (|| {
            // No pathspec: the whole tree, and no error when nothing is tracked yet (newer git
            // refuses `:/` then, as a pathspec that matches no file).
            self.run(&with_scratch(GitCommand::new(["add", "-u"])))?;
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

    pub(crate) fn head_state(&self) -> Result<HeadState> {
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
    pub(crate) fn all_refs(&self) -> Result<BTreeMap<String, String>> {
        let out = self.run(&GitCommand::new(["for-each-ref", "--format=%(objectname) %(refname)"]))?;
        Ok(out
            .stdout
            .lines()
            .filter_map(|line| line.split_once(' '))
            // git's notes on a bisect are its own: a restore must not move them under it.
            .filter(|(_, name)| {
                *name != "refs/stash" && !name.starts_with("refs/oxbow/") && !name.starts_with("refs/bisect/")
            })
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

    /// The local branches among `refs` that moved to a commit that doesn't have the old one.
    fn rewrites(&self, refs: &[RefMove]) -> Vec<String> {
        refs.iter()
            .filter(|m| m.name.starts_with("refs/heads/"))
            .filter_map(|m| match (&m.before, &m.after) {
                (Some(before), Some(after)) if !self.is_ancestor(before, after) => Some(m.name.clone()),
                _ => None,
            })
            .collect()
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

    pub(crate) fn object_exists(&self, id: &str) -> bool {
        self.run(&GitCommand::new(["cat-file", "-e", id])).is_ok()
    }

    pub(crate) fn git_path(&self, name: &str) -> Result<PathBuf> {
        if let Some(path) = self.git_paths.lock().ok().and_then(|paths| paths.get(name).cloned()) {
            return Ok(path);
        }
        let out = self.run(&GitCommand::new(["rev-parse", "--git-path", name]))?;
        let path = self.workdir().join(out.stdout.trim());
        if let Ok(mut paths) = self.git_paths.lock() {
            paths.insert(name.to_owned(), path.clone());
        }
        Ok(path)
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

    /// What `action` does, in a few words, as the Undo history names it: "Commit to main".
    pub fn action_title(&self, action: &Action) -> String {
        let head = self.head().map_or(
            HeadState {
                branch: None,
                commit: None,
            },
            |h| HeadState {
                branch: h.branch,
                commit: h.commit,
            },
        );
        self.describe(action, &head).1
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
            Action::Ignore { pattern, .. } => ("ignore", format!("Ignore {pattern}")),
            Action::Untrack { paths } => ("ignore", format!("Stop tracking {}", files(paths))),
            Action::Commit { amend: true, .. } => ("commit", format!("Amend the last commit on {here}")),
            Action::Commit {
                changelist: Some(id), ..
            } => match self.changelist_name(id) {
                Ok(name) => ("commit", format!("Commit “{name}” to {here}")),
                Err(_) => ("commit", format!("Commit a changelist to {here}")),
            },
            Action::Commit { .. } => ("commit", format!("Commit to {here}")),
            Action::StageChangelist { paths, .. } => ("stage", format!("Stage {}", files(paths))),
            Action::DiscardChangelist { paths, .. } => ("discard", format!("Discard changes in {}", files(paths))),
            Action::Fetch { remote } => ("fetch", format!("Fetch {}", remote.as_deref().unwrap_or("all remotes"))),
            Action::Pull { remote, branch, .. } => ("pull", format!("Pull {remote}/{branch}")),
            Action::Push {
                remote,
                branch,
                merge_request: Some(_),
                ..
            } => ("push", format!("Push {branch} to {remote} with a merge request")),
            Action::Push {
                remote, branch, force, ..
            } => (
                "push",
                format!("{} {branch} to {remote}", if *force { "Force push" } else { "Push" }),
            ),
            Action::PullAndPush { branch, .. } => ("push", format!("Pull and push {branch}")),
            Action::ReplayAndPush { branch, remote, .. } => {
                ("push", format!("Copy new commits and force push {branch} to {remote}"))
            }
            Action::AbortRebase => ("undo", "Undo the pull".to_owned()),
            Action::Switch { branch, .. } => ("checkout", format!("Check out {branch}")),
            Action::Detach { commit, .. } => ("checkout", format!("Check out {}", short_id(commit))),
            Action::Track { remote, branch, .. } => ("checkout", format!("Check out {branch} from {remote}")),
            Action::CreateBranch { name, .. } => ("branch", format!("Create branch {name}")),
            Action::RenameBranch { from, to, .. } => ("branch", format!("Rename {from} to {to}")),
            Action::DeleteBranch { name, .. } => ("drop", format!("Delete branch {name}")),
            Action::DeleteRemoteBranch { remote, branch, .. } => ("drop", format!("Delete {branch} on {remote}")),
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
            Action::SavePatches { .. } => ("commit", "Save commits as patches".to_owned()),
            Action::ApplyPatches { paths } => (
                "commit",
                match paths.as_slice() {
                    [one] => format!(
                        "Apply {} to {here}",
                        std::path::Path::new(one)
                            .file_name()
                            .map_or_else(|| one.clone(), |n| n.to_string_lossy().into_owned())
                    ),
                    _ => format!("Apply {} patch files to {here}", paths.len()),
                },
            ),
            Action::Revert { commit } => ("revert", format!("Revert {}", short_id(commit))),
            Action::RevertFile { commit, path, .. } => ("revert", format!("Revert {path} of {}", short_id(commit))),
            Action::CherryPickFile { commit, path, .. } => {
                ("cherry", format!("Cherry-pick {path} from {}", short_id(commit)))
            }
            Action::TakeVersion { commit, path } => ("checkout", format!("Take {path} from {}", short_id(commit))),
            Action::RestoreLocalVersion { path, .. } => ("checkout", format!("Restore {path} from Local History")),
            Action::ApplySuggestion { path, .. } => ("edit", format!("Apply a suggestion to {path}")),
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
            Action::StashPush { staged: true, .. } => ("stash", "Stash staged changes".to_owned()),
            Action::StashPush { .. } => ("stash", "Stash changes".to_owned()),
            Action::StashHunk {
                lines: Some(_), path, ..
            } => ("stash", format!("Stash lines of {path}")),
            Action::StashHunk { path, .. } => ("stash", format!("Stash a hunk of {path}")),
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
            Action::Unshallow { remote } => ("fetch", format!("Fetch the full history from {remote}")),
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
            Action::EditStack { plan } if !plan.splits.is_empty() => {
                let split: Vec<&str> = plan.splits.iter().map(|s| &s.id[..s.id.len().min(7)]).collect();
                ("rebase", format!("Edit stack {}: split {}", plan.top, split.join(", ")))
            }
            Action::EditStack { plan } => ("rebase", format!("Edit stack {}", plan.top)),
            Action::PullRequestMerged { branch, base, .. } => (
                "merge",
                format!("Bring in the merged pull request of {branch} into {base}"),
            ),
            Action::DeleteMergedBranch { branch, .. } => ("drop", format!("Delete the merged branch {branch}")),
            Action::LfsTrack { pattern, .. } => ("lfs", format!("Track {pattern} with Git LFS")),
            Action::LfsUntrack { pattern } => ("lfs", format!("Stop tracking {pattern} with Git LFS")),
            Action::LfsPull { paths, .. } if paths.len() == 1 => ("fetch", format!("Download {}", paths[0])),
            Action::LfsPull { paths, .. } if !paths.is_empty() => {
                ("fetch", format!("Download {} LFS files", paths.len()))
            }
            Action::LfsPull {
                include: Some(pattern), ..
            } => ("fetch", format!("Download LFS files of {pattern}")),
            Action::LfsPull { include: None, .. } => ("fetch", "Download LFS files".to_owned()),
            Action::LfsPrune => ("lfs", "Free up LFS space".to_owned()),
            Action::LfsInstall { .. } => ("lfs", "Install Git LFS".to_owned()),
            Action::BisectStart { .. } => ("bisect", "Start a bisect".to_owned()),
            Action::BisectMark { mark, commit } => (
                "bisect",
                format!(
                    "Mark {} as {}",
                    commit.as_deref().map_or_else(|| "the commit".to_owned(), short_id),
                    match mark {
                        crate::bisect::BisectMark::Good => "good",
                        crate::bisect::BisectMark::Bad => "bad",
                        crate::bisect::BisectMark::Skip => "skipped",
                    }
                ),
            ),
            Action::BisectReset => ("bisect", "Finish the bisect".to_owned()),
            // Worktree steps stay out of the log; these only name them.
            Action::WorktreeAdd { path, .. } => ("worktree", format!("Add the worktree {path}")),
            Action::WorktreeRemove { path, .. } => ("worktree", format!("Remove the worktree {path}")),
            Action::WorktreeLock { path, .. } => ("worktree", format!("Lock the worktree {path}")),
            Action::WorktreeUnlock { path } => ("worktree", format!("Unlock the worktree {path}")),
            Action::WorktreePrune => ("worktree", "Prune worktrees".to_owned()),
            Action::WorktreeRepair { path } => ("worktree", format!("Repair the worktree {path}")),
            Action::SubmoduleUpdate { paths, latest } => (
                "box",
                match (paths.as_slice(), latest) {
                    ([], false) => "Update submodules".to_owned(),
                    ([], true) => "Pull the latest of every submodule".to_owned(),
                    ([path], false) => format!("Update the submodule {path}"),
                    ([path], true) => format!("Pull the latest of the submodule {path}"),
                    (_, false) => format!("Update {} submodules", paths.len()),
                    (_, true) => format!("Pull the latest of {} submodules", paths.len()),
                },
            ),
            Action::SubmoduleSync { paths } => (
                "box",
                match paths.as_slice() {
                    [path] => format!("Sync the URL of the submodule {path}"),
                    _ => "Sync submodule URLs".to_owned(),
                },
            ),
            Action::SubmoduleAdd { path, .. } => ("box", format!("Add the submodule {path}")),
            Action::SubmoduleRemove { path } => ("box", format!("Remove the submodule {path}")),
            Action::PushBranches { remote, branches } => {
                ("push", format!("Push {} branches to {remote}", branches.len()))
            }
            Action::AddToCommit { commit, .. } => (
                "commit",
                format!("Add staged changes to {}", &commit[..commit.len().min(7)]),
            ),
            Action::Fix { fix, .. } => match fix {
                Fix::MoveCommits { count: 1, to, .. } => ("branch", format!("Move 1 commit from {here} to {to}")),
                Fix::MoveCommits { count, to, .. } => ("branch", format!("Move {count} commits from {here} to {to}")),
                Fix::AddToLast { paths, .. } if paths.is_empty() => {
                    ("commit", format!("Add staged changes to the last commit on {here}"))
                }
                Fix::AddToLast { paths, .. } => {
                    ("commit", format!("Add {} to the last commit on {here}", files(paths)))
                }
                Fix::UndoLast { .. } => ("undo", format!("Undo the last commit on {here}")),
                Fix::RevertPushed { commit, push: true } => {
                    ("revert", format!("Revert {} and push {here}", short_id(commit)))
                }
                Fix::RevertPushed { commit, .. } => ("revert", format!("Revert {}", short_id(commit))),
                Fix::SaveDetached { name, .. } => ("branch", format!("Keep work on the new branch {name}")),
            },
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

/// Days in a git expiry such as `30 days`, `2.weeks` or `never`; `None` for never. What can't be
/// read counts as git's default, 30 days.
pub(crate) fn expiry_days(value: &str) -> Option<u32> {
    let value = value.trim().to_ascii_lowercase();
    if matches!(value.as_str(), "never" | "false") {
        return None;
    }
    if value == "now" || value == "all" {
        return Some(0);
    }
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    let unit = value[digits.len()..].trim_start_matches(['.', ' ']);
    let Ok(n) = digits.parse::<u32>() else {
        return Some(30);
    };
    Some(if unit.starts_with("week") {
        n.saturating_mul(7)
    } else if unit.starts_with("month") {
        n.saturating_mul(30)
    } else if unit.starts_with("year") {
        n.saturating_mul(365)
    } else if unit.starts_with("day") {
        n
    } else if unit.starts_with("hour") || unit.starts_with("minute") || unit.starts_with("second") {
        0
    } else {
        30
    })
}

pub(crate) fn short_id(id: &str) -> String {
    id[..id.len().min(7)].to_owned()
}

/// `main` for `refs/heads/main`, `tag v1.0` for `refs/tags/v1.0`.
pub(crate) fn short_ref(name: &str) -> String {
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

/// Every step in the log file, read again only when the file changed: every reload of the
/// History asks for the log, and it grows with each action for weeks. `None` when there is none;
/// an error when the file is there but can't be read, which must not look like an empty log.
fn read_log(file: &std::path::Path) -> Result<Option<Arc<Vec<OpEntry>>>> {
    type Read = (std::path::PathBuf, std::time::SystemTime, u64, Arc<Vec<OpEntry>>);
    static LAST: Mutex<Vec<Read>> = Mutex::new(Vec::new());
    let unreadable = |err: std::io::Error| Error::Git(format!("oplog.jsonl: {err}"));
    let meta = match std::fs::metadata(file) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(unreadable(err)),
    };
    let stamp = (meta.modified().map_err(unreadable)?, meta.len());
    if let Ok(last) = LAST.lock()
        && let Some((_, _, _, entries)) = last
            .iter()
            .find(|(path, modified, len, _)| path == file && (*modified, *len) == stamp)
    {
        return Ok(Some(entries.clone()));
    }
    let text = std::fs::read_to_string(file).map_err(unreadable)?;
    let entries: Arc<Vec<OpEntry>> = Arc::new(
        text.lines()
            .filter_map(|line| serde_json::from_str::<OpEntry>(line).ok())
            .collect(),
    );
    if let Ok(mut last) = LAST.lock() {
        last.retain(|(path, ..)| path != file);
        // One per repository open in a window; a handful at most.
        if last.len() >= 8 {
            last.remove(0);
        }
        last.push((file.to_path_buf(), stamp.0, stamp.1, entries.clone()));
    }
    Ok(Some(entries))
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn io(err: std::io::Error) -> Error {
    Error::Git(err.to_string())
}

/// Makes `.git/oxbow`, where the snapshots and the log go, saying in plain words why it can't:
/// the words show when Undo isn't ready for a step.
fn make_folder(dir: &std::path::Path) -> Result<()> {
    use std::io::ErrorKind;
    std::fs::create_dir_all(dir).map_err(|err| {
        Error::Git(match err.kind() {
            ErrorKind::AlreadyExists | ErrorKind::NotADirectory => {
                "a file named .git/oxbow is where Oxbow keeps a folder of its own. Move the file away, and Undo works again".into()
            }
            ErrorKind::PermissionDenied => {
                "Oxbow isn’t allowed to write in the repository’s .git folder. Check who owns it".into()
            }
            ErrorKind::ReadOnlyFilesystem => "the disk the repository is on can’t be written to".into(),
            ErrorKind::StorageFull => "the disk is full".into(),
            _ => format!("can’t make the folder .git/oxbow: {err}"),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::expiry_days;

    #[test]
    fn git_expiries_read_as_days() {
        assert_eq!(expiry_days("30 days"), Some(30));
        assert_eq!(expiry_days("90.days.ago"), Some(90));
        assert_eq!(expiry_days("2.weeks"), Some(14));
        assert_eq!(expiry_days("3 months"), Some(90));
        assert_eq!(expiry_days("never"), None);
        assert_eq!(expiry_days("now"), Some(0));
        assert_eq!(expiry_days("someday"), Some(30));
        assert_eq!(expiry_days("12.hours"), Some(0));
    }
}
