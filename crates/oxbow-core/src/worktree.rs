//! The working copy: what changed since the last commit, staging and committing.
//!
//! Status and diffs come from the `git` command line, so they honor every setting that affects
//! them (ignore rules, attributes, line endings, fsmonitor), and a hunk shown on screen is
//! exactly the hunk `git apply` gets when it is staged. Every change to the repository is an
//! [`Action`] that turns into the [`GitCommand`]s shown in the confirmation sheet.

use serde::{Deserialize, Serialize};

use std::sync::atomic::AtomicBool;

use crate::cli::{GitCommand, OutputLine};
use crate::commit::{DiffLine, FileChange, FileDiff, FileStatus, Hunk, LineKind, word_diff};
use crate::edit::{ResetMode, plan_reset};
use crate::error::{Error, Result};
use crate::operation::{ConflictSide, MergeMethod, Pick};
use crate::repo::Repo;

/// Files with more changed lines than this are not shown line by line.
const MAX_DIFF_LINES: usize = 20_000;

/// Uncommitted changes, grouped the way they are staged.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkingTree {
    /// Changes in the index, ready to commit.
    pub staged: Vec<FileChange>,
    /// Changes in the working copy that are not staged, and untracked files.
    pub unstaged: Vec<FileChange>,
    /// Files with unresolved conflicts.
    pub conflicted: Vec<FileChange>,
}

impl WorkingTree {
    pub fn is_empty(&self) -> bool {
        self.staged.is_empty() && self.unstaged.is_empty() && self.conflicted.is_empty()
    }

    /// Number of distinct files with any change.
    pub fn file_count(&self) -> usize {
        let mut paths: Vec<&str> = self
            .staged
            .iter()
            .chain(&self.unstaged)
            .chain(&self.conflicted)
            .map(|f| f.path.as_str())
            .collect();
        paths.sort_unstable();
        paths.dedup();
        paths.len()
    }
}

/// Which side of the working copy a diff or hunk belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Side {
    /// Index compared to `HEAD`.
    Staged,
    /// Working copy compared to the index.
    Unstaged,
}

/// A change to the working copy or the index, as asked for by the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Action {
    Stage {
        paths: Vec<String>,
    },
    Unstage {
        paths: Vec<String>,
    },
    /// Throw away unstaged changes; untracked files are deleted.
    Discard {
        paths: Vec<String>,
    },
    /// Stage, unstage or discard one hunk, identified by its `@@` header so a file that changed
    /// in the meantime is refused instead of patched in the wrong place.
    StageHunk {
        path: String,
        header: String,
    },
    UnstageHunk {
        path: String,
        header: String,
    },
    DiscardHunk {
        path: String,
        header: String,
    },
    Commit {
        message: String,
        amend: bool,
    },
    /// Download new commits, branches and tags from `remote`, or from every remote.
    Fetch {
        remote: Option<String>,
    },
    /// Bring the new commits of `remote`/`branch` into the checked-out branch, replaying local
    /// commits on top of them.
    Pull {
        remote: String,
        branch: String,
    },
    /// Send the local `branch` to `remote`, where it is called `upstream`.
    Push {
        remote: String,
        branch: String,
        upstream: String,
        /// Remember `remote`/`upstream` as the branch's upstream (publishing a new branch).
        set_upstream: bool,
        /// Overwrite the remote branch even when it has commits the local one does not.
        force: bool,
        /// With `force`: only when the remote branch still points at this commit.
        lease: Option<String>,
        /// Skip the pre-push hook.
        no_verify: bool,
    },
    /// Pull with rebase, then push: the way out of a push rejected for new remote commits.
    PullAndPush {
        remote: String,
        branch: String,
        upstream: String,
        /// Skip the pre-push hook, when the push being retried skipped it too.
        no_verify: bool,
    },
    /// Give up a pull that stopped on conflicts, back to where the branch was.
    AbortRebase,
    /// Check out the local `branch`. With `stash`, uncommitted changes are stashed first, for
    /// when they would be overwritten. With `keep`, a branch of that name is created at `HEAD`
    /// first, so commits made on a detached `HEAD` are not left behind.
    Switch {
        branch: String,
        stash: bool,
        #[serde(default)]
        keep: Option<String>,
    },
    /// Check out a commit without a branch: `HEAD` points at the commit (a detached `HEAD`).
    /// `commit` is a sha or a tag name.
    Detach {
        commit: String,
        stash: bool,
    },
    /// Create a local branch from the remote one and check it out.
    Track {
        remote: String,
        branch: String,
        stash: bool,
    },
    /// Create the branch `name` at `start` (a branch or commit; `HEAD` when missing), optionally
    /// check it out and publish it to a remote.
    CreateBranch {
        name: String,
        start: Option<String>,
        switch: bool,
        publish: Option<String>,
    },
    /// Rename a local branch; with `upstream`, publish the new name there and delete the old one.
    RenameBranch {
        from: String,
        to: String,
        upstream: Option<RemoteBranch>,
    },
    /// Delete a local branch. `force` deletes it even with commits no other branch has;
    /// `upstream` is deleted on its remote too.
    DeleteBranch {
        name: String,
        force: bool,
        upstream: Option<RemoteBranch>,
    },
    /// Delete a branch on its remote.
    DeleteRemoteBranch {
        remote: String,
        branch: String,
    },
    /// Bring `branch` into the checked-out branch. `message` is the commit's, for a merge commit
    /// or a squash.
    Merge {
        branch: String,
        method: MergeMethod,
        message: Option<String>,
    },
    /// Finish the merge, rebase, cherry-pick or revert in progress once its conflicts are
    /// resolved. `message` replaces the one git prepared.
    Continue {
        message: Option<String>,
    },
    /// Give up the operation in progress: everything goes back to how it was before it.
    Abort,
    /// Leave out the commit a rebase or cherry-pick stopped on.
    Skip,
    /// Write the chosen side of each conflict of `path` into the file and mark it resolved.
    Resolve {
        path: String,
        picks: Vec<Pick>,
    },
    /// Resolve `path` with one side's whole file; a side that deleted it deletes it.
    TakeFile {
        path: String,
        side: ConflictSide,
    },
    /// Copy `commit` onto the checked-out branch as a new commit.
    CherryPick {
        commit: String,
    },
    /// Add a commit that undoes `commit`.
    Revert {
        commit: String,
    },
    /// Point the checked-out branch, or a detached `HEAD`, at `commit` (a sha or `HEAD~1`).
    Reset {
        commit: String,
        mode: ResetMode,
    },
    /// Replace the message of the last commit; its changes and the staged ones stay as they are.
    Reword {
        message: String,
    },
}

/// A branch on a remote: `branch` on `remote`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBranch {
    pub remote: String,
    pub branch: String,
}

/// What happens while an action runs, for the live terminal in the sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ActionEvent {
    /// A command starts; `display` is how it is typed in a shell.
    Command { display: String },
    /// The running command printed a line.
    Line(OutputLine),
}

/// The commands an action runs, for the confirmation sheet.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub commands: Vec<GitCommand>,
}

impl Repo {
    /// Staged, unstaged, untracked and conflicted files.
    pub fn working_tree(&self) -> Result<WorkingTree> {
        let out = self.run(&GitCommand::new([
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
        ]))?;
        let mut tree = parse_status(&out.stdout);

        let staged_stats = self.numstat(Side::Staged)?;
        let unstaged_stats = self.numstat(Side::Unstaged)?;
        for (files, stats) in [(&mut tree.staged, &staged_stats), (&mut tree.unstaged, &unstaged_stats)] {
            for file in files.iter_mut() {
                if let Some(&(add, del, binary)) = stats.get(&file.path) {
                    file.additions = add;
                    file.deletions = del;
                    file.binary = binary;
                }
            }
        }
        for file in tree.unstaged.iter_mut().filter(|f| f.status == FileStatus::Untracked) {
            if let Ok(data) = std::fs::read(self.workdir().join(&file.path)) {
                file.binary = data.iter().take(8000).any(|&b| b == 0);
                if !file.binary {
                    file.additions = data.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count() as u32;
                }
            }
        }
        Ok(tree)
    }

    /// The diff of one file on one side of the working copy.
    pub fn working_diff(&self, path: &str, side: Side, whole_file: bool) -> Result<FileDiff> {
        let tree = self.working_tree()?;
        let list = match side {
            Side::Staged => &tree.staged,
            Side::Unstaged => &tree.unstaged,
        };
        let file = list
            .iter()
            .chain(&tree.conflicted)
            .find(|f| f.path == path)
            .cloned()
            .ok_or_else(|| Error::Git(format!("{path} has no {} changes", side.word())))?;
        let patch = self.raw_diff(&file, side, whole_file)?;
        let parsed = parse_patch(&patch);
        let too_large = parsed.hunks.iter().map(|h| h.hunk.lines.len()).sum::<usize>() > MAX_DIFF_LINES;
        Ok(FileDiff {
            file: FileChange {
                binary: file.binary || parsed.binary,
                ..file
            },
            hunks: if too_large {
                Vec::new()
            } else {
                parsed.hunks.into_iter().map(|h| h.hunk).collect()
            },
            too_large,
        })
    }

    /// The commands `action` will run.
    pub fn plan(&self, action: &Action) -> Result<Plan> {
        let paths = |args: &[&str], paths: &[String]| {
            GitCommand::new(
                args.iter()
                    .map(|s| s.to_string())
                    .chain(["--".to_owned()])
                    .chain(paths.iter().cloned()),
            )
        };
        let commands = match action {
            Action::Stage { paths: p } => vec![paths(&["add"], p).comment("add the files to the next commit")],
            Action::Unstage { paths: p } => {
                if self.head()?.commit.is_some() {
                    vec![paths(&["restore", "--staged"], p).comment("take them out of the next commit, keep the edits")]
                } else {
                    vec![
                        paths(&["rm", "--cached", "-r", "--quiet"], p)
                            .comment("nothing is committed yet, so just unstage"),
                    ]
                }
            }
            Action::Discard { paths: p } => {
                let tree = self.working_tree()?;
                let untracked: Vec<String> = p
                    .iter()
                    .filter(|path| {
                        tree.unstaged
                            .iter()
                            .any(|f| &f.path == *path && f.status == FileStatus::Untracked)
                    })
                    .cloned()
                    .collect();
                let tracked: Vec<String> = p.iter().filter(|path| !untracked.contains(path)).cloned().collect();
                let mut commands = Vec::new();
                if !tracked.is_empty() {
                    commands.push(
                        paths(&["restore", "--worktree"], &tracked).comment("put back the staged or committed version"),
                    );
                }
                if !untracked.is_empty() {
                    commands
                        .push(paths(&["clean", "-f"], &untracked).comment("delete the new files git does not track"));
                }
                commands
            }
            Action::StageHunk { path, header } => {
                vec![GitCommand::new(["apply", "--cached", "-"]).comment(stdin_note(path, header))]
            }
            Action::UnstageHunk { path, header } => {
                vec![GitCommand::new(["apply", "--cached", "--reverse", "-"]).comment(stdin_note(path, header))]
            }
            Action::DiscardHunk { path, header } => {
                vec![GitCommand::new(["apply", "--reverse", "-"]).comment(stdin_note(path, header))]
            }
            Action::Commit { message, amend } => {
                let mut args = vec!["commit".to_owned()];
                if *amend {
                    args.push("--amend".to_owned());
                }
                // One -m per paragraph, as people type it; git puts the blank lines back between them.
                for paragraph in paragraphs(message) {
                    args.push("-m".to_owned());
                    args.push(paragraph);
                }
                let cmd = GitCommand::new(args);
                vec![if *amend {
                    cmd.comment("replace the last commit with one that also has the staged changes")
                } else {
                    cmd
                }]
            }
            Action::Fetch { remote: Some(remote) } => vec![
                GitCommand::new(["fetch", "--prune", remote])
                    .comment(format!("--prune: drop {remote}/* branches deleted on the remote"))
                    .with_progress(),
            ],
            Action::Fetch { remote: None } => vec![
                GitCommand::new(["fetch", "--prune", "--all"])
                    .comment("--all: every remote, --prune: drop branches deleted there")
                    .with_progress(),
            ],
            Action::Pull { remote, branch } => vec![pull(remote, branch)],
            Action::Push {
                remote,
                branch,
                upstream,
                set_upstream,
                force,
                lease,
                no_verify,
            } => {
                let mut args = vec!["push".to_owned()];
                let mut notes = Vec::new();
                if *no_verify {
                    args.push("--no-verify".to_owned());
                    notes.push("--no-verify: skip the pre-push hook this one time".to_owned());
                }
                if *set_upstream {
                    args.push("-u".to_owned());
                    notes.push(format!("-u: remember {remote}/{upstream} as the upstream"));
                }
                match (force, lease) {
                    (true, Some(lease)) => {
                        args.push(format!("--force-with-lease={upstream}:{lease}"));
                        notes.push(format!(
                            "--force-with-lease: overwrite only if {remote}/{upstream} is still {}",
                            &lease[..lease.len().min(7)]
                        ));
                    }
                    (true, None) => {
                        args.push("--force".to_owned());
                        notes.push("--force: overwrite whatever the remote has".to_owned());
                    }
                    _ => {}
                }
                args.push(remote.clone());
                args.push(refspec(branch, upstream));
                if notes.is_empty() {
                    notes.push(format!("sends the commits {remote}/{upstream} does not have yet"));
                }
                vec![GitCommand::new(args).comment(notes.join("; ")).with_progress()]
            }
            Action::PullAndPush {
                remote,
                branch,
                upstream,
                no_verify,
            } => {
                let mut push = vec!["push".to_owned()];
                if *no_verify {
                    push.push("--no-verify".to_owned());
                }
                push.extend([remote.clone(), refspec(branch, upstream)]);
                vec![
                    pull(remote, upstream),
                    GitCommand::new(push)
                        .comment("then send your commits on top")
                        .with_progress(),
                ]
            }
            Action::AbortRebase => {
                vec![GitCommand::new(["rebase", "--abort"]).comment("back to where the branch was before the pull")]
            }
            Action::Switch { branch, stash, keep } => {
                let mut commands = Vec::new();
                if let Some(name) = keep {
                    commands.push(
                        GitCommand::new(["branch", name])
                            .comment(format!("{name} keeps the commits made without a branch")),
                    );
                }
                if *stash {
                    commands.push(stash_before(branch));
                }
                commands.push(
                    GitCommand::new(["switch", branch]).comment("switch: the modern form of \"git checkout <branch>\""),
                );
                commands
            }
            Action::Detach { commit, stash } => {
                // A full sha reads better short; a tag name stays as it is.
                let full_sha = commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit());
                let short = if full_sha { &commit[..7] } else { commit.as_str() };
                let mut commands = Vec::new();
                if *stash {
                    commands.push(stash_before(short));
                }
                commands.push(
                    GitCommand::new(["switch", "--detach", short])
                        .comment("--detach: HEAD points at a commit, not at a branch"),
                );
                commands
            }
            Action::Track { remote, branch, stash } => {
                let mut commands = Vec::new();
                if *stash {
                    commands.push(stash_before(branch));
                }
                commands.push(
                    GitCommand::new(["switch".to_owned(), "--track".to_owned(), format!("{remote}/{branch}")])
                        .comment(format!("--track: create {branch} that follows {remote}/{branch}")),
                );
                commands
            }
            Action::CreateBranch {
                name,
                start,
                switch,
                publish,
            } => {
                let at = start
                    .as_deref()
                    .map_or("the current commit".to_owned(), |s| s.to_owned());
                let mut args = if *switch {
                    vec!["switch".to_owned(), "-c".to_owned(), name.clone()]
                } else {
                    vec!["branch".to_owned(), name.clone()]
                };
                args.extend(start.iter().cloned());
                let head = self.head()?.branch;
                let mut commands = vec![GitCommand::new(args).comment(if *switch {
                    format!("-c: create the branch at {at}, then switch to it")
                } else {
                    format!(
                        "creates the branch at {at}, you stay on {}",
                        head.as_deref().unwrap_or("the current commit")
                    )
                })];
                if let Some(remote) = publish {
                    commands.push(
                        GitCommand::new(["push", "-u", remote, name])
                            .comment(format!("-u: remember {remote}/{name} as its upstream"))
                            .with_progress(),
                    );
                }
                commands
            }
            Action::RenameBranch { from, to, upstream } => {
                let mut commands =
                    vec![GitCommand::new(["branch", "-m", from, to]).comment("-m: move, which renames the branch")];
                if let Some(up) = upstream {
                    commands.push(
                        GitCommand::new(["push", "-u", &up.remote, to])
                            .comment(format!("publish the new name and make {}/{to} the upstream", up.remote))
                            .with_progress(),
                    );
                    commands.push(
                        GitCommand::new([
                            "push".to_owned(),
                            up.remote.clone(),
                            "--delete".to_owned(),
                            up.branch.clone(),
                        ])
                        .comment("then remove the old name there")
                        .with_progress(),
                    );
                }
                commands
            }
            Action::DeleteBranch { name, force, upstream } => {
                let mut commands = vec![if *force {
                    GitCommand::new(["branch", "-D", name])
                        .comment("-D: delete even though the commits are not merged anywhere")
                } else {
                    GitCommand::new(["branch", "-d", name]).comment("-d: deletes only when no work would be lost")
                }];
                if let Some(up) = upstream {
                    commands.push(
                        GitCommand::new([
                            "push".to_owned(),
                            up.remote.clone(),
                            "--delete".to_owned(),
                            up.branch.clone(),
                        ])
                        .comment(format!("removes {}/{} for everyone", up.remote, up.branch))
                        .with_progress(),
                    );
                }
                commands
            }
            Action::DeleteRemoteBranch { remote, branch } => vec![
                GitCommand::new(["push", remote, "--delete", branch])
                    .comment("--delete: remove the branch on the remote")
                    .with_progress(),
            ],
            Action::Merge {
                branch,
                method,
                message,
            } => self.plan_merge(branch, *method, message.as_deref())?,
            Action::Continue { message } => self.plan_continue(message.as_deref())?,
            Action::Abort => self.plan_abort()?,
            Action::Skip => self.plan_skip()?,
            Action::Resolve { path, .. } => vec![GitCommand::new(["add", "--", path]).comment(format!(
                "Oxbow writes your choices into {path} first; add marks it resolved"
            ))],
            Action::TakeFile { path, side } => self.plan_take_file(path, *side)?,
            Action::CherryPick { commit } => self.plan_cherry_pick(commit)?,
            Action::Revert { commit } => self.plan_revert(commit)?,
            Action::Reset { commit, mode } => plan_reset(commit, *mode),
            Action::Reword { message } => {
                let mut args = vec!["commit".to_owned(), "--amend".to_owned(), "--only".to_owned()];
                for paragraph in paragraphs(message) {
                    args.push("-m".to_owned());
                    args.push(paragraph);
                }
                vec![GitCommand::new(args).comment("--only: just the message; staged changes stay staged")]
            }
        };
        Ok(Plan { commands })
    }

    /// Run `action`. Returns what the last command printed.
    pub fn perform(&self, action: &Action) -> Result<String> {
        self.perform_with(action, &mut |_| {}, &AtomicBool::new(false))
    }

    /// Run `action`, reporting each command and each line it prints to `on_event`. Setting
    /// `cancel` stops the running command.
    pub fn perform_with(
        &self,
        action: &Action,
        on_event: &mut dyn FnMut(ActionEvent),
        cancel: &AtomicBool,
    ) -> Result<String> {
        if let Action::Commit { message, .. } | Action::Reword { message } = action
            && message.trim().is_empty()
        {
            return Err(Error::Git("the commit message is empty".into()));
        }
        let (side, path, header) = match action {
            Action::StageHunk { path, header } => (Side::Unstaged, path, header),
            Action::DiscardHunk { path, header } => (Side::Unstaged, path, header),
            Action::UnstageHunk { path, header } => (Side::Staged, path, header),
            _ => {
                if let Action::Resolve { path, picks } = action {
                    self.write_resolution(path, picks)?;
                }
                let mut last = String::new();
                for command in self.plan(action)?.commands {
                    on_event(ActionEvent::Command {
                        display: command.display(),
                    });
                    let out = self
                        .run_streaming(&command, &mut |line| on_event(ActionEvent::Line(line)), cancel)
                        .inspect_err(|_| {
                            if let Action::Merge {
                                branch,
                                method: MergeMethod::Squash,
                                message,
                            } = action
                            {
                                self.remember_squash(branch, message.as_deref());
                            }
                        })?;
                    last = if out.stdout.trim().is_empty() {
                        out.stderr
                    } else {
                        out.stdout
                    };
                }
                return Ok(last);
            }
        };
        let patch = self.hunk_patch(path, side, header)?;
        let command = &self.plan(action)?.commands[0];
        on_event(ActionEvent::Command {
            display: command.display(),
        });
        let out = self.run_with_input(command, Some(patch.as_bytes()))?;
        Ok(out.stdout)
    }

    /// A patch with the file header and only the hunk that starts with `header`.
    fn hunk_patch(&self, path: &str, side: Side, header: &str) -> Result<String> {
        let tree = self.working_tree()?;
        let list = match side {
            Side::Staged => &tree.staged,
            Side::Unstaged => &tree.unstaged,
        };
        let file = list
            .iter()
            .find(|f| f.path == path)
            .ok_or_else(|| Error::Git(format!("{path} has no {} changes any more", side.word())))?;
        if file.status == FileStatus::Untracked {
            return Err(Error::Git(format!("{path} is not tracked yet: stage the whole file")));
        }
        let parsed = parse_patch(&self.raw_diff(file, side, false)?);
        let hunk = parsed
            .hunks
            .iter()
            .find(|h| h.header == header)
            .ok_or_else(|| Error::Git(format!("{path} changed since it was shown; refresh and try again")))?;
        Ok(format!("{}{}", parsed.file_header, hunk.text))
    }

    /// `git diff` output for one file.
    fn raw_diff(&self, file: &FileChange, side: Side, whole_file: bool) -> Result<String> {
        let context = if whole_file { "-U100000000" } else { "-U3" };
        let mut args = vec!["diff", "--no-color", "--no-ext-diff", "--no-renames", context];
        if file.status == FileStatus::Untracked {
            // `--no-index` exits with 1 when the files differ, which they always do here.
            args.extend(["--no-index", "--", "/dev/null", &file.path]);
            let out = self.spawn(&GitCommand::new(args), None)?;
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        if side == Side::Staged {
            args.push("--cached");
        }
        args.extend(["--", &file.path]);
        Ok(self.run(&GitCommand::new(args))?.stdout)
    }

    /// Added and deleted lines per path (`None` counts for binary files).
    fn numstat(&self, side: Side) -> Result<std::collections::HashMap<String, (u32, u32, bool)>> {
        let mut args = vec!["diff", "--numstat", "-z", "--no-renames"];
        if side == Side::Staged {
            if self.head()?.commit.is_none() {
                // Nothing committed yet: compare the index with the empty tree.
                args.extend(["--cached", "4b825dc642cb6eb9a060e54bf8d69288fbee4904"]);
            } else {
                args.push("--cached");
            }
        }
        let out = self.run(&GitCommand::new(args))?;
        let mut stats = std::collections::HashMap::new();
        for record in out.stdout.split('\0').filter(|r| !r.is_empty()) {
            let mut parts = record.splitn(3, '\t');
            let (Some(add), Some(del), Some(path)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            let binary = add == "-";
            stats.insert(
                path.to_owned(),
                (add.parse().unwrap_or(0), del.parse().unwrap_or(0), binary),
            );
        }
        Ok(stats)
    }
}

impl Side {
    fn word(self) -> &'static str {
        match self {
            Side::Staged => "staged",
            Side::Unstaged => "unstaged",
        }
    }
}

/// Put uncommitted changes, new files included, aside before switching to `branch`.
fn stash_before(branch: &str) -> GitCommand {
    GitCommand::new([
        "stash".to_owned(),
        "push".to_owned(),
        "--include-untracked".to_owned(),
        "-m".to_owned(),
        format!("Before switching to {branch}"),
    ])
    .comment("keeps your uncommitted files in the stash, new ones too")
}

fn pull(remote: &str, branch: &str) -> GitCommand {
    GitCommand::new(["pull", "--rebase", "--autostash", remote, branch])
        .comment("--rebase: no merge commit, your commits go on top; --autostash: keep uncommitted files")
        .with_progress()
}

/// `main`, or `local:remote` when the branch has another name on the remote.
fn refspec(branch: &str, upstream: &str) -> String {
    if branch == upstream {
        branch.to_owned()
    } else {
        format!("{branch}:{upstream}")
    }
}

fn stdin_note(path: &str, header: &str) -> String {
    format!("the patch of the {header} hunk of {path} comes on stdin")
}

/// A message split at its blank lines, each paragraph trimmed.
fn paragraphs(message: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in message.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(current.join("\n").trim().to_owned());
                current.clear();
            }
        } else {
            current.push(line.trim_end());
        }
    }
    if !current.is_empty() {
        out.push(current.join("\n").trim().to_owned());
    }
    out
}

fn change(path: &str, old_path: Option<&str>, status: FileStatus) -> FileChange {
    FileChange {
        path: path.to_owned(),
        old_path: old_path.map(str::to_owned),
        status,
        additions: 0,
        deletions: 0,
        binary: false,
    }
}

fn status_of(code: char) -> Option<FileStatus> {
    Some(match code {
        'A' => FileStatus::Added,
        'D' => FileStatus::Deleted,
        'R' => FileStatus::Renamed,
        'C' => FileStatus::Copied,
        'M' | 'T' => FileStatus::Modified,
        _ => return None,
    })
}

/// Parse `git status --porcelain=v2 -z`.
fn parse_status(out: &str) -> WorkingTree {
    let mut tree = WorkingTree::default();
    let mut records = out.split('\0').filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        let mut fields = record.split(' ');
        match fields.next() {
            Some("1") | Some("2") => {
                let renamed = record.starts_with('2');
                let xy: Vec<char> = fields.next().unwrap_or("..").chars().collect();
                // 1 XY sub mH mI mW hH hI path / 2 XY sub mH mI mW hH hI score path\0orig
                let skip = if renamed { 7 } else { 6 };
                let path = record.splitn(skip + 3, ' ').nth(skip + 2).unwrap_or_default();
                let old_path = if renamed { records.next() } else { None };
                if let Some(status) = status_of(xy[0]) {
                    tree.staged.push(change(path, old_path, status));
                }
                if let Some(status) = status_of(xy[1]) {
                    tree.unstaged.push(change(path, None, status));
                }
            }
            Some("u") => {
                let path = record.splitn(11, ' ').nth(10).unwrap_or_default();
                tree.conflicted.push(change(path, None, FileStatus::Conflicted));
            }
            Some("?") => tree.unstaged.push(change(&record[2..], None, FileStatus::Untracked)),
            _ => {}
        }
    }
    tree
}

struct ParsedHunk {
    /// The `@@ -a,b +c,d @@` part of the hunk's first line.
    header: String,
    /// The hunk as it appears in the patch, header line included.
    text: String,
    hunk: Hunk,
}

struct ParsedPatch {
    /// Everything before the first hunk: `diff --git`, mode and index lines, `---` and `+++`.
    file_header: String,
    hunks: Vec<ParsedHunk>,
    binary: bool,
}

/// Parse the output of `git diff` for a single file.
fn parse_patch(patch: &str) -> ParsedPatch {
    let mut file_header = String::new();
    let mut hunks: Vec<ParsedHunk> = Vec::new();
    let mut binary = false;
    let (mut old_no, mut new_no) = (0u32, 0u32);
    for line in patch.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if let Some(rest) = body.strip_prefix("@@ ") {
            let Some(end) = rest.find(" @@") else { continue };
            let ranges = &rest[..end];
            let mut parts = ranges.split(' ');
            let (old_start, old_lines) = parse_range(parts.next().unwrap_or("-0").trim_start_matches('-'));
            let (new_start, new_lines) = parse_range(parts.next().unwrap_or("+0").trim_start_matches('+'));
            old_no = old_start;
            new_no = new_start;
            hunks.push(ParsedHunk {
                header: format!("@@ {ranges} @@"),
                text: line.to_owned(),
                hunk: Hunk {
                    header: format!("@@ {ranges} @@"),
                    old_start,
                    old_lines,
                    new_start,
                    new_lines,
                    lines: Vec::new(),
                },
            });
            continue;
        }
        let Some(current) = hunks.last_mut() else {
            if body.starts_with("Binary files ") || body == "GIT binary patch" {
                binary = true;
            }
            file_header.push_str(line);
            continue;
        };
        current.text.push_str(line);
        let text = body.get(1..).unwrap_or_default().trim_end_matches('\r').to_owned();
        let (kind, old_line, new_line) = match body.chars().next() {
            Some('+') => {
                new_no += 1;
                (LineKind::Added, None, Some(new_no - 1))
            }
            Some('-') => {
                old_no += 1;
                (LineKind::Removed, Some(old_no - 1), None)
            }
            Some(' ') => {
                old_no += 1;
                new_no += 1;
                (LineKind::Context, Some(old_no - 1), Some(new_no - 1))
            }
            // "\ No newline at end of file" belongs to the patch but is not a line of the file.
            _ => continue,
        };
        current.hunk.lines.push(DiffLine {
            kind,
            old_line,
            new_line,
            text,
            words: None,
        });
    }
    for hunk in &mut hunks {
        mark_words(&mut hunk.hunk.lines);
    }
    ParsedPatch {
        file_header,
        hunks,
        binary,
    }
}

fn parse_range(range: &str) -> (u32, u32) {
    match range.split_once(',') {
        Some((start, count)) => (start.parse().unwrap_or(0), count.parse().unwrap_or(0)),
        None => (range.parse().unwrap_or(0), 1),
    }
}

/// Changed words for runs of removed lines followed by the same number of added lines.
fn mark_words(lines: &mut [DiffLine]) {
    let mut i = 0;
    while i < lines.len() {
        if lines[i].kind != LineKind::Removed {
            i += 1;
            continue;
        }
        let removed_end = (i..lines.len())
            .find(|&j| lines[j].kind != LineKind::Removed)
            .unwrap_or(lines.len());
        let added_end = (removed_end..lines.len())
            .find(|&j| lines[j].kind != LineKind::Added)
            .unwrap_or(lines.len());
        let count = removed_end - i;
        if count == added_end - removed_end {
            for k in 0..count {
                let (r, a) = word_diff(&lines[i + k].text, &lines[removed_end + k].text);
                lines[i + k].words = Some(r);
                lines[removed_end + k].words = Some(a);
            }
        }
        i = added_end.max(i + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_messages_split_into_paragraphs() {
        assert_eq!(
            paragraphs("  Summary\n\n\nFirst line\nsecond line  \n\n- item\n"),
            ["Summary", "First line\nsecond line", "- item"]
        );
        assert!(paragraphs(" \n ").is_empty());
    }

    #[test]
    fn status_records_are_sorted_into_groups() {
        let out = "1 MM N... 100644 100644 100644 aaa bbb src/a b.rs\0\
                   1 A. N... 000000 100644 100644 000 ccc new.rs\0\
                   2 R. N... 100644 100644 100644 ddd ddd R100 renamed.rs\0old.rs\0\
                   u UU N... 100644 100644 100644 100644 e1 e2 e3 both.rs\0\
                   ? notes.txt\0";
        let tree = parse_status(out);
        let staged: Vec<_> = tree.staged.iter().map(|f| (f.path.as_str(), f.status)).collect();
        assert_eq!(
            staged,
            [
                ("src/a b.rs", FileStatus::Modified),
                ("new.rs", FileStatus::Added),
                ("renamed.rs", FileStatus::Renamed)
            ]
        );
        assert_eq!(tree.staged[2].old_path.as_deref(), Some("old.rs"));
        let unstaged: Vec<_> = tree.unstaged.iter().map(|f| (f.path.as_str(), f.status)).collect();
        assert_eq!(
            unstaged,
            [
                ("src/a b.rs", FileStatus::Modified),
                ("notes.txt", FileStatus::Untracked)
            ]
        );
        assert_eq!(tree.conflicted[0].path, "both.rs");
        assert_eq!(tree.file_count(), 5);
    }

    #[test]
    fn patch_hunks_keep_their_text_and_line_numbers() {
        let patch = "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n\
                     @@ -1,3 +1,3 @@ fn main\n a\n-b c\n+b d\n e\n\
                     @@ -10 +10,2 @@\n x\n+y\n\\ No newline at end of file\n";
        let parsed = parse_patch(patch);
        assert_eq!(
            parsed.file_header,
            "diff --git a/f b/f\nindex 1..2 100644\n--- a/f\n+++ b/f\n"
        );
        assert_eq!(parsed.hunks.len(), 2);
        assert_eq!(parsed.hunks[0].header, "@@ -1,3 +1,3 @@");
        let first = &parsed.hunks[0].hunk;
        let lines: Vec<_> = first
            .lines
            .iter()
            .map(|l| (l.kind, l.old_line, l.new_line, l.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            [
                (LineKind::Context, Some(1), Some(1), "a"),
                (LineKind::Removed, Some(2), None, "b c"),
                (LineKind::Added, None, Some(2), "b d"),
                (LineKind::Context, Some(3), Some(3), "e"),
            ]
        );
        assert!(first.lines[1].words.is_some());
        let second = &parsed.hunks[1];
        assert_eq!((second.hunk.old_start, second.hunk.old_lines), (10, 1));
        assert!(second.text.ends_with("\\ No newline at end of file\n"));
        assert_eq!(second.hunk.lines.len(), 2);
    }
}
