//! Merging and rebasing, and the operations git can stop in the middle of: a merge, rebase,
//! cherry-pick or revert that hit conflicts. What is going on, what a merge would do before it
//! runs, and each conflicted file with both sides, so the user picks what goes into the result.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use gix::bstr::ByteSlice;
use serde::{Deserialize, Serialize};

use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::remote::{CommitBrief, parse_briefs};
use crate::repo::{RefKind, Repo};

/// Where Oxbow notes which branch a squash merge is bringing in; `SQUASH_MSG` does not say.
const SQUASH_SOURCE: &str = "OXBOW_SQUASH_SOURCE";

/// Commits listed per side in a merge preview.
const PREVIEW_COMMITS: usize = 8;

/// Conflict markers longer than git's usual seven, so a line of `=======` in a file is not
/// mistaken for one.
const MARKER: usize = 13;

/// How a branch is brought into the checked-out one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeMethod {
    /// A merge commit with both branches as parents, even when a fast-forward would do.
    Merge,
    /// All changes as one new commit; the other branch is not a parent.
    Squash,
    /// The checked-out branch's own commits replayed on top of the other branch.
    Rebase,
    /// The checked-out branch moves up to the other one; only when it has nothing of its own.
    FastForward,
}

/// What git is in the middle of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum OperationKind {
    Merge,
    /// `git merge --squash` ran; its result waits to be committed.
    Squash,
    Rebase,
    CherryPick,
    Revert,
}

/// One side of a conflict, as git names the index stages: `ours` is stage 2, `theirs` stage 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictSide {
    Ours,
    Theirs,
}

/// A merge, rebase, cherry-pick or revert that has not finished.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub kind: OperationKind,
    /// The branch that changes: the checked-out one, or the one being rebased.
    pub branch: Option<String>,
    /// What comes in: the merged branch, the new base of a rebase, a short sha otherwise.
    pub incoming: Option<String>,
    /// The commit being merged, replayed, picked or reverted.
    pub commit: Option<CommitBrief>,
    /// For a merge: commits it brings in.
    pub incoming_count: usize,
    /// For a rebase: the commit it stopped on and how many there are, from 1.
    pub step: Option<(usize, usize)>,
    /// Files with unresolved conflicts.
    pub conflicted: usize,
    /// Who `ours` and `theirs` are in words, e.g. `auth/3-ui` and `main`.
    pub ours_label: String,
    pub theirs_label: String,
    /// The side that holds the user's own work: `ours` in a merge, `theirs` in a rebase.
    pub yours: ConflictSide,
    /// The message the commit that finishes it gets.
    pub message: Option<String>,
}

/// What merging a branch into the checked-out one would do, before anything runs.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergePreview {
    pub branch: String,
    /// Commits the branch has that `HEAD` does not, newest first (the first few).
    pub incoming: Vec<CommitBrief>,
    pub incoming_count: usize,
    /// Commits `HEAD` has that the branch does not, newest first (the first few).
    pub ours: Vec<CommitBrief>,
    pub ours_count: usize,
    /// Where the two split.
    pub base: Option<CommitBrief>,
    /// Files that would conflict; `None` when this git can't tell (before 2.38).
    pub conflicts: Option<Vec<String>>,
    /// Files the incoming commits change.
    pub files: usize,
    /// Of those, the ones the checked-out branch changed too.
    pub touched_here: usize,
}

/// A conflicted file: the parts both sides agree on, and the conflicts in between.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictFile {
    pub path: String,
    /// Which versions exist: one side may have deleted the file, and new files have no base.
    pub base: bool,
    pub ours: bool,
    pub theirs: bool,
    /// Binary files can only be taken whole from one side.
    pub binary: bool,
    /// Empty when the file can't be merged line by line.
    pub chunks: Vec<Chunk>,
}

/// A run of lines of a conflicted file, without line endings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Chunk {
    /// Lines that are the same on both sides, or that git merged on its own.
    Same { lines: Vec<String> },
    /// Lines both sides changed, with what they were at the base.
    Conflict {
        ours: Vec<String>,
        base: Vec<String>,
        theirs: Vec<String>,
    },
}

/// What goes into the result in place of one conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Pick {
    Ours,
    Theirs,
    OursThenTheirs,
    TheirsThenOurs,
}

/// A chunk with its lines as they are in the file, line endings included.
#[derive(Debug, Clone, PartialEq, Eq)]
enum RawChunk {
    Same(Vec<String>),
    Conflict {
        ours: Vec<String>,
        base: Vec<String>,
        theirs: Vec<String>,
    },
}

impl Repo {
    fn git_dir(&self) -> PathBuf {
        self.local().git_dir().to_path_buf()
    }

    /// The merge, rebase, cherry-pick or revert in progress, if any.
    pub fn operation(&self) -> Result<Option<Operation>> {
        let dir = self.git_dir();
        let read = |name: &str| {
            std::fs::read_to_string(dir.join(name))
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        };
        let exists = |name: &str| dir.join(name).exists();
        let head = self.head()?;
        let refs = self.refs().unwrap_or_default();
        // A branch or tag at the commit says more than its sha.
        let name_of = |sha: &str| {
            [RefKind::Local, RefKind::Remote, RefKind::Tag]
                .iter()
                .find_map(|kind| refs.iter().find(|r| r.kind == *kind && r.target == sha))
                .map_or_else(|| short(sha), |r| r.name.clone())
        };
        let brief = |sha: &str| self.brief(sha).ok();
        let here = head.branch.clone();
        let here_label = here.clone().unwrap_or_else(|| "HEAD".to_owned());

        // `git am` keeps its state in rebase-apply too, marked by `applying`.
        let rebase_dir = if exists("rebase-merge") {
            Some("rebase-merge")
        } else if exists("rebase-apply") && !exists("rebase-apply/applying") {
            Some("rebase-apply")
        } else {
            None
        };
        let mut op = if let Some(state) = rebase_dir {
            let file = |name: &str| read(&format!("{state}/{name}"));
            let branch = file("head-name").map(|n| n.strip_prefix("refs/heads/").unwrap_or(&n).to_owned());
            let onto = file("onto").map(|sha| name_of(&sha));
            let (current, total) = if state == "rebase-merge" {
                (file("msgnum"), file("end"))
            } else {
                (file("next"), file("last"))
            };
            let step = current
                .and_then(|n| n.parse().ok())
                .zip(total.and_then(|n| n.parse().ok()));
            let commit = read("REBASE_HEAD")
                .or_else(|| file("stopped-sha"))
                .and_then(|sha| brief(&sha));
            Operation {
                kind: OperationKind::Rebase,
                ours_label: onto.clone().unwrap_or_else(|| "the new base".to_owned()),
                theirs_label: branch.clone().unwrap_or_else(|| "your commit".to_owned()),
                yours: ConflictSide::Theirs,
                branch,
                incoming: onto,
                commit,
                incoming_count: 0,
                step,
                conflicted: 0,
                message: None,
            }
        } else if let Some(merge_head) = read("MERGE_HEAD") {
            let sha = merge_head.lines().next().unwrap_or_default().to_owned();
            let incoming = name_of(&sha);
            let count = self
                .run(&GitCommand::new(["rev-list", "--count", &format!("HEAD..{sha}")]))
                .ok()
                .and_then(|out| out.stdout.trim().parse().ok())
                .unwrap_or(0);
            Operation {
                kind: OperationKind::Merge,
                branch: here.clone(),
                theirs_label: incoming.clone(),
                incoming: Some(incoming),
                commit: brief(&sha),
                incoming_count: count,
                step: None,
                conflicted: 0,
                ours_label: here_label,
                yours: ConflictSide::Ours,
                message: read("MERGE_MSG").map(|m| strip_comments(&m)),
            }
        } else if let Some(sha) = read("CHERRY_PICK_HEAD").or_else(|| read("REVERT_HEAD")) {
            let picking = exists("CHERRY_PICK_HEAD");
            Operation {
                kind: if picking {
                    OperationKind::CherryPick
                } else {
                    OperationKind::Revert
                },
                branch: here.clone(),
                incoming: Some(short(&sha)),
                theirs_label: if picking {
                    short(&sha)
                } else {
                    format!("revert of {}", short(&sha))
                },
                commit: brief(&sha),
                incoming_count: 1,
                step: None,
                conflicted: 0,
                ours_label: here_label,
                yours: ConflictSide::Ours,
                message: read("MERGE_MSG").map(|m| strip_comments(&m)),
            }
        } else if let Some(message) = read("SQUASH_MSG") {
            let source = read(SQUASH_SOURCE);
            Operation {
                kind: OperationKind::Squash,
                branch: here.clone(),
                incoming: source.clone(),
                commit: None,
                incoming_count: 0,
                step: None,
                conflicted: 0,
                ours_label: here_label,
                theirs_label: source.unwrap_or_else(|| "the squashed branch".to_owned()),
                yours: ConflictSide::Ours,
                message: Some(squash_message(&message)),
            }
        } else {
            let _ = std::fs::remove_file(dir.join(SQUASH_SOURCE));
            return Ok(None);
        };
        op.conflicted = self.conflicted_paths()?.len();
        if op.kind != OperationKind::Squash {
            // The note outlives the squash it belongs to only if something went very wrong.
            let _ = std::fs::remove_file(dir.join(SQUASH_SOURCE));
        }
        Ok(Some(op))
    }

    /// Paths with unresolved conflicts, from the index.
    fn conflicted_paths(&self) -> Result<Vec<String>> {
        let repo = self.local();
        let index = repo.open_index().map_err(Error::git)?;
        let mut paths: Vec<String> = index
            .entries()
            .iter()
            .filter(|e| e.stage_raw() != 0)
            .map(|e| e.path(&index).to_str_lossy().into_owned())
            .collect();
        paths.dedup();
        Ok(paths)
    }

    /// One commit as a [`CommitBrief`].
    fn brief(&self, rev: &str) -> Result<CommitBrief> {
        let out = self.run(&GitCommand::new([
            "log",
            "-1",
            "--format=%H%x1f%s%x1f%an%x1f%ct",
            rev,
            "--",
        ]))?;
        parse_briefs(&out.stdout)
            .into_iter()
            .next()
            .ok_or_else(|| Error::UnknownCommit(rev.to_owned()))
    }

    /// Commits in `range` (`a..b`), newest first: the first few and how many there are.
    fn commits_in(&self, range: &str) -> Result<(Vec<CommitBrief>, usize)> {
        let count = self
            .run(&GitCommand::new(["rev-list", "--count", range]))?
            .stdout
            .trim()
            .parse()
            .unwrap_or(0);
        let out = self.run(&GitCommand::new([
            "log".to_owned(),
            format!("--max-count={PREVIEW_COMMITS}"),
            "--format=%H%x1f%s%x1f%an%x1f%ct".to_owned(),
            range.to_owned(),
            "--".to_owned(),
        ]))?;
        Ok((parse_briefs(&out.stdout), count))
    }

    /// What merging `branch` into `HEAD` would bring and whether it would conflict.
    pub fn merge_preview(&self, branch: &str) -> Result<MergePreview> {
        let (incoming, incoming_count) = self.commits_in(&format!("HEAD..{branch}"))?;
        let (ours, ours_count) = self.commits_in(&format!("{branch}..HEAD"))?;
        let base = self
            .run(&GitCommand::new(["merge-base", "HEAD", branch]))
            .ok()
            .map(|out| out.stdout.trim().to_owned())
            .filter(|sha| !sha.is_empty());
        let changed = |from: &str, to: &str| -> Vec<String> {
            self.run(&GitCommand::new([
                "diff",
                "--name-only",
                "-z",
                "--no-renames",
                from,
                to,
            ]))
            .map(|out| {
                out.stdout
                    .split('\0')
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
        };
        let (files, touched_here) = match &base {
            Some(base) => {
                let theirs = changed(base, branch);
                let here = changed(base, "HEAD");
                let touched = theirs.iter().filter(|p| here.contains(p)).count();
                (theirs.len(), touched)
            }
            None => (0, 0),
        };
        // `merge-tree --write-tree` merges in memory: exit 0 is clean, 1 lists the conflicts.
        let conflicts = if incoming_count == 0 {
            Some(Vec::new())
        } else {
            let out = self.spawn(
                &GitCommand::new([
                    "merge-tree",
                    "--write-tree",
                    "--name-only",
                    "--no-messages",
                    "-z",
                    "HEAD",
                    branch,
                ]),
                None,
            )?;
            match out.status.code() {
                Some(0) => Some(Vec::new()),
                Some(1) => {
                    let text = String::from_utf8_lossy(&out.stdout);
                    let mut names: Vec<String> = text
                        .split('\0')
                        .skip(1)
                        .filter(|p| !p.is_empty())
                        .map(str::to_owned)
                        .collect();
                    names.dedup();
                    Some(names)
                }
                _ => None,
            }
        };
        Ok(MergePreview {
            branch: branch.to_owned(),
            incoming,
            incoming_count,
            ours,
            ours_count,
            base: base.and_then(|sha| self.brief(&sha).ok()),
            conflicts,
            files,
            touched_here,
        })
    }

    /// Both sides of a conflicted file and the conflicts between them.
    pub fn conflict_file(&self, path: &str) -> Result<ConflictFile> {
        let [base, ours, theirs] = self.stages(path)?;
        if ours.is_none() && theirs.is_none() && base.is_none() {
            return Err(Error::Git(format!("{path} has no conflicts")));
        }
        let binary = [&base, &ours, &theirs]
            .iter()
            .any(|blob| blob.as_ref().is_some_and(|b| b.iter().take(8000).any(|&c| c == 0)));
        let chunks = match (&ours, &theirs) {
            (Some(o), Some(t)) if !binary => self
                .merge_blobs(o, base.as_deref().unwrap_or_default(), t)?
                .into_iter()
                .map(|chunk| match chunk {
                    RawChunk::Same(lines) => Chunk::Same { lines: bare(lines) },
                    RawChunk::Conflict { ours, base, theirs } => Chunk::Conflict {
                        ours: bare(ours),
                        base: bare(base),
                        theirs: bare(theirs),
                    },
                })
                .collect(),
            _ => Vec::new(),
        };
        Ok(ConflictFile {
            path: path.to_owned(),
            base: base.is_some(),
            ours: ours.is_some(),
            theirs: theirs.is_some(),
            binary,
            chunks,
        })
    }

    /// The file with `picks` in place of its conflicts, in order.
    pub(crate) fn resolved_text(&self, path: &str, picks: &[Pick]) -> Result<String> {
        let [base, ours, theirs] = self.stages(path)?;
        let (Some(ours), Some(theirs)) = (ours, theirs) else {
            return Err(Error::Git(format!("{path} can only be taken whole from one side")));
        };
        let chunks = self.merge_blobs(&ours, base.as_deref().unwrap_or_default(), &theirs)?;
        let conflicts = chunks.iter().filter(|c| matches!(c, RawChunk::Conflict { .. })).count();
        if conflicts != picks.len() {
            return Err(Error::Git(format!("{path} changed since it was shown; open it again")));
        }
        let mut picks = picks.iter();
        let mut out = String::new();
        for chunk in chunks {
            match chunk {
                RawChunk::Same(lines) => out.extend(lines),
                RawChunk::Conflict { ours, theirs, .. } => {
                    let parts: [&[String]; 2] = match picks.next().expect("one pick per conflict") {
                        Pick::Ours => [&ours, &[]],
                        Pick::Theirs => [&theirs, &[]],
                        Pick::OursThenTheirs => [&ours, &theirs],
                        Pick::TheirsThenOurs => [&theirs, &ours],
                    };
                    for part in parts {
                        out.extend(part.iter().cloned());
                    }
                }
            }
        }
        Ok(out)
    }

    /// Write the file with the picks, before `git add` marks it resolved.
    pub(crate) fn write_resolution(&self, path: &str, picks: &[Pick]) -> Result<()> {
        let text = self.resolved_text(path, picks)?;
        let file = self.workdir().join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|err| Error::Git(err.to_string()))?;
        }
        std::fs::write(&file, text).map_err(|err| Error::Git(format!("could not write {path}: {err}")))
    }

    /// After `git merge --squash` stopped on conflicts: keep what the user chose for the commit
    /// that finishes it. Git drops the `-m` message there, and `SQUASH_MSG` alone does not say
    /// which branch was squashed, so Oxbow writes both down beside it.
    pub(crate) fn remember_squash(&self, branch: &str, message: Option<&str>) {
        let dir = self.git_dir();
        if let Some(message) = message.map(str::trim).filter(|m| !m.is_empty()) {
            let _ = std::fs::write(dir.join("SQUASH_MSG"), format!("{message}\n"));
        }
        let _ = std::fs::write(dir.join(SQUASH_SOURCE), format!("{branch}\n"));
    }

    pub(crate) fn plan_merge(
        &self,
        branch: &str,
        method: MergeMethod,
        message: Option<&str>,
    ) -> Result<Vec<GitCommand>> {
        let here = self.head()?.branch.unwrap_or_else(|| "HEAD".to_owned());
        let with_message = |args: &[&str]| {
            let mut args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            match message.map(str::trim).filter(|m| !m.is_empty()) {
                Some(message) => args.extend(["-m".to_owned(), message.to_owned()]),
                None => args.push("--no-edit".to_owned()),
            }
            args
        };
        Ok(match method {
            MergeMethod::Merge => {
                let mut args = with_message(&["merge", "--no-ff"]);
                args.push(branch.to_owned());
                vec![GitCommand::new(args).comment("--no-ff: a merge commit even when git could just move the branch")]
            }
            MergeMethod::FastForward => vec![
                GitCommand::new(["merge", "--ff-only", branch])
                    .comment(format!("--ff-only: {here} moves up to {branch}, no new commit")),
            ],
            MergeMethod::Squash => {
                let mut commit = vec!["commit".to_owned()];
                match message.map(str::trim).filter(|m| !m.is_empty()) {
                    Some(message) => commit.extend(["-m".to_owned(), message.to_owned()]),
                    None => commit.push("--no-edit".to_owned()),
                }
                vec![
                    GitCommand::new(["merge", "--squash", branch])
                        .comment(format!("--squash: stage all of {branch}’s changes, without a commit")),
                    GitCommand::new(commit).comment(format!("one new commit on {here}; {branch} stays as it is")),
                ]
            }
            MergeMethod::Rebase => vec![GitCommand::new(["rebase", branch]).comment(format!(
                "replays {here}’s own commits on top of {branch}, as new commits"
            ))],
        })
    }

    fn current_operation(&self) -> Result<Operation> {
        self.operation()?
            .ok_or_else(|| Error::Git("no merge, rebase, cherry-pick or revert is in progress".into()))
    }

    pub(crate) fn plan_continue(&self, message: Option<&str>) -> Result<Vec<GitCommand>> {
        let op = self.current_operation()?;
        // git's own message would keep its `# Conflicts:` lines with `--no-edit`.
        let message = message
            .or(op.message.as_deref())
            .map(str::trim)
            .filter(|m| !m.is_empty());
        let commit = |comment: &str| {
            let mut args = vec!["commit".to_owned()];
            match message {
                Some(message) => args.extend(["-m".to_owned(), message.to_owned()]),
                None => args.push("--no-edit".to_owned()),
            }
            GitCommand::new(args).comment(comment)
        };
        Ok(vec![match op.kind {
            OperationKind::Merge => commit("records the merge, with both branches as its parents"),
            OperationKind::Squash => commit("one new commit with all the squashed changes"),
            OperationKind::Rebase => {
                GitCommand::new(["rebase", "--continue"]).comment("commits the resolved files and replays the rest")
            }
            OperationKind::CherryPick => GitCommand::new(["cherry-pick", "--continue"])
                .comment("commits the picked change with the resolved files"),
            OperationKind::Revert => {
                GitCommand::new(["revert", "--continue"]).comment("commits the revert with the resolved files")
            }
        }])
    }

    pub(crate) fn plan_abort(&self) -> Result<Vec<GitCommand>> {
        let op = self.current_operation()?;
        let branch = op.branch.unwrap_or_else(|| "the branch".to_owned());
        Ok(vec![match op.kind {
            OperationKind::Merge => GitCommand::new(["merge", "--abort"])
                .comment(format!("{branch} and its files go back to before the merge")),
            OperationKind::Squash => GitCommand::new(["reset", "--merge"])
                .comment("--merge: the files go back to before the squash, other edits stay"),
            OperationKind::Rebase => {
                GitCommand::new(["rebase", "--abort"]).comment(format!("{branch} goes back to before the rebase"))
            }
            OperationKind::CherryPick => GitCommand::new(["cherry-pick", "--abort"])
                .comment(format!("{branch} goes back to before the cherry-pick")),
            OperationKind::Revert => {
                GitCommand::new(["revert", "--abort"]).comment(format!("{branch} goes back to before the revert"))
            }
        }])
    }

    pub(crate) fn plan_skip(&self) -> Result<Vec<GitCommand>> {
        let op = self.current_operation()?;
        let commit = op.commit.map_or_else(|| "this commit".to_owned(), |c| short(&c.id));
        Ok(vec![match op.kind {
            OperationKind::Rebase => GitCommand::new(["rebase", "--skip"])
                .comment(format!("leaves {commit} out and goes on with the next one")),
            OperationKind::CherryPick => {
                GitCommand::new(["cherry-pick", "--skip"]).comment(format!("leaves {commit} out"))
            }
            _ => return Err(Error::Git("only a rebase or cherry-pick can skip a commit".into())),
        }])
    }

    pub(crate) fn plan_take_file(&self, path: &str, side: ConflictSide) -> Result<Vec<GitCommand>> {
        let op = self.current_operation()?;
        let [_, ours, theirs] = self.stages(path)?;
        let (exists, flag, label) = match side {
            ConflictSide::Ours => (ours.is_some(), "--ours", op.ours_label),
            ConflictSide::Theirs => (theirs.is_some(), "--theirs", op.theirs_label),
        };
        Ok(if exists {
            vec![
                GitCommand::new(["checkout", flag, "--", path])
                    .comment(format!("{flag}: the whole file as it is on {label}")),
                GitCommand::new(["add", "--", path]).comment("marks it resolved"),
            ]
        } else {
            vec![
                GitCommand::new(["rm", "--quiet", "--", path]).comment(format!("{label} deleted the file, so it goes")),
            ]
        })
    }

    /// The base, ours and theirs versions of `path` in the index.
    fn stages(&self, path: &str) -> Result<[Option<Vec<u8>>; 3]> {
        let repo = self.local();
        let index = repo.open_index().map_err(Error::git)?;
        let mut out: [Option<Vec<u8>>; 3] = [None, None, None];
        for entry in index.entries() {
            let stage = entry.stage_raw() as usize;
            if stage == 0 || entry.path(&index) != path.as_bytes() {
                continue;
            }
            let object = repo.find_object(entry.id).map_err(Error::git)?;
            out[stage - 1] = Some(object.data.clone());
        }
        Ok(out)
    }

    /// Merge three versions with `git merge-file`, the way git marks conflicts in the file.
    fn merge_blobs(&self, ours: &[u8], base: &[u8], theirs: &[u8]) -> Result<Vec<RawChunk>> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "oxbow-merge-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).map_err(|err| Error::Git(err.to_string()))?;
        let result = (|| {
            let mut files = Vec::new();
            for (name, data) in [("ours", ours), ("base", base), ("theirs", theirs)] {
                let file = dir.join(name);
                std::fs::write(&file, data).map_err(|err| Error::Git(err.to_string()))?;
                files.push(file.display().to_string());
            }
            let out = self.spawn(
                &GitCommand::new([
                    "merge-file".to_owned(),
                    "-p".to_owned(),
                    "--diff3".to_owned(),
                    format!("--marker-size={MARKER}"),
                    files[0].clone(),
                    files[1].clone(),
                    files[2].clone(),
                ]),
                None,
            )?;
            // The exit code is the number of conflicts; only a negative one is an error.
            if out.status.code().is_none_or(|code| !(0..=127).contains(&code)) {
                return Err(Error::Git(String::from_utf8_lossy(&out.stderr).trim().to_owned()));
            }
            Ok(parse_merged(&String::from_utf8_lossy(&out.stdout)))
        })();
        let _ = std::fs::remove_dir_all(&dir);
        result
    }
}

/// Split `git merge-file --diff3` output into agreed lines and conflicts.
fn parse_merged(text: &str) -> Vec<RawChunk> {
    enum In {
        Same,
        Ours,
        Base,
        Theirs,
    }
    let marker = |c: char| c.to_string().repeat(MARKER);
    let (start, mid_base, mid, end) = (marker('<'), marker('|'), marker('='), marker('>'));
    let mut chunks = Vec::new();
    let mut same = Vec::new();
    let (mut ours, mut base, mut theirs) = (Vec::new(), Vec::new(), Vec::new());
    let mut state = In::Same;
    for line in text.split_inclusive('\n') {
        let bare = line.trim_end_matches(['\n', '\r']);
        let is = |m: &str| bare == m || bare.strip_prefix(m).is_some_and(|rest| rest.starts_with(' '));
        match state {
            In::Same if is(&start) => {
                if !same.is_empty() {
                    chunks.push(RawChunk::Same(std::mem::take(&mut same)));
                }
                state = In::Ours;
            }
            In::Same => same.push(line.to_owned()),
            In::Ours if is(&mid_base) => state = In::Base,
            In::Ours | In::Base if bare == mid => state = In::Theirs,
            In::Ours => ours.push(line.to_owned()),
            In::Base => base.push(line.to_owned()),
            In::Theirs if is(&end) => {
                chunks.push(RawChunk::Conflict {
                    ours: std::mem::take(&mut ours),
                    base: std::mem::take(&mut base),
                    theirs: std::mem::take(&mut theirs),
                });
                state = In::Same;
            }
            In::Theirs => theirs.push(line.to_owned()),
        }
    }
    if !same.is_empty() {
        chunks.push(RawChunk::Same(same));
    }
    chunks
}

fn bare(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|l| l.trim_end_matches(['\n', '\r']).to_owned())
        .collect()
}

fn short(sha: &str) -> String {
    sha.chars().take(7).collect()
}

/// A commit message without git's `#` comment lines.
fn strip_comments(message: &str) -> String {
    message
        .lines()
        .filter(|l| !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// The message for finishing a squash merge. Git's own `SQUASH_MSG` lists every commit with its
/// author and date; one commit keeps its message, several get a summary line each.
fn squash_message(text: &str) -> String {
    if !text.starts_with("Squashed commit of the following:") {
        return strip_comments(text);
    }
    let mut messages: Vec<Vec<&str>> = Vec::new();
    for line in text.lines() {
        if line.starts_with("commit ") {
            messages.push(Vec::new());
        } else if let (Some(message), Some(body)) = (messages.last_mut(), line.strip_prefix("    ")) {
            message.push(body);
        }
    }
    match messages.as_slice() {
        [one] => one.join("\n").trim().to_owned(),
        many => {
            let summaries: Vec<String> = many
                .iter()
                .rev()
                .filter_map(|m| m.first())
                .map(|s| format!("- {s}"))
                .collect();
            format!("Squash {} commits\n\n{}", many.len(), summaries.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_text_splits_into_agreed_lines_and_conflicts() {
        let m = |c: char| c.to_string().repeat(MARKER);
        let text = format!(
            "a\n{} ours\nours 1\nours 2\n{} base\nold\n{}\ntheirs\n{} theirs\nz\n=======\n",
            m('<'),
            m('|'),
            m('='),
            m('>')
        );
        let chunks = parse_merged(&text);
        assert_eq!(
            chunks,
            [
                RawChunk::Same(vec!["a\n".into()]),
                RawChunk::Conflict {
                    ours: vec!["ours 1\n".into(), "ours 2\n".into()],
                    base: vec!["old\n".into()],
                    theirs: vec!["theirs\n".into()],
                },
                // A line of seven `=` in the file is just a line.
                RawChunk::Same(vec!["z\n".into(), "=======\n".into()]),
            ]
        );
    }

    #[test]
    fn squash_messages_become_one_message() {
        let one = "Squashed commit of the following:\n\ncommit abc\nAuthor: A <a@b>\nDate:   now\n\n    Add login\n    \n    With a body\n";
        assert_eq!(squash_message(one), "Add login\n\nWith a body");
        let two = "Squashed commit of the following:\n\ncommit b\nAuthor: A\n\n    Second\n\ncommit a\nAuthor: A\n\n    First\n";
        assert_eq!(squash_message(two), "Squash 2 commits\n\n- First\n- Second");
        assert_eq!(squash_message("Ship it\n# Conflicts:\n#\tf\n"), "Ship it");
    }
}
