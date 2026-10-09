//! The working copy: what changed since the last commit, staging and committing.
//!
//! Status and diffs come from the `git` command line, so they honor every setting that affects
//! them (ignore rules, attributes, line endings, fsmonitor), and a hunk shown on screen is
//! exactly the hunk `git apply` gets when it is staged. Every change to the repository is an
//! [`Action`] that turns into the [`GitCommand`]s shown in the confirmation sheet.

use serde::{Deserialize, Serialize};

use std::sync::atomic::AtomicBool;

use crate::cli::{GitCommand, OutputLine, literal};
use crate::commit::{
    DiffLine, FileChange, FileDiff, FileStatus, Hunk, LineKind, ModeChange, Source, eol_change, over_limit, word_diff,
};
use crate::config;
use crate::edit::{ResetMode, plan_reset};
use crate::error::{Error, Result};
use crate::lfs::{self, LfsChange};
use crate::operation::{ConflictSide, MergeMethod, OperationKind, Pick};
use crate::repo::Repo;
use crate::tags;
use std::io::Read;

/// Files with more changed lines than this are not shown line by line.
const MAX_DIFF_LINES: usize = 20_000;

/// Untracked files bigger than this are not read to count their lines.
const MAX_COUNTED_BYTES: u64 = 8 * 1024 * 1024;

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
    /// Stage, unstage or discard one hunk, identified by its `@@` header and `check`, the
    /// fingerprint of its bytes ([`Hunk::check`]), so a file that changed in the meantime is
    /// refused instead of patched in the wrong place. With `lines`, only those changed lines of
    /// it (indexes into the hunk's lines); the others stay as they are.
    StageHunk {
        path: String,
        header: String,
        check: String,
        #[serde(default)]
        lines: Option<Vec<usize>>,
    },
    UnstageHunk {
        path: String,
        header: String,
        check: String,
        #[serde(default)]
        lines: Option<Vec<usize>>,
    },
    DiscardHunk {
        path: String,
        header: String,
        check: String,
        #[serde(default)]
        lines: Option<Vec<usize>>,
    },
    /// Add `pattern` as a line of the top `.gitignore` and stage it.
    Ignore {
        pattern: String,
    },
    Commit {
        message: String,
        amend: bool,
        /// Skip the pre-commit and commit-msg hooks this one time.
        #[serde(default)]
        no_verify: bool,
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
        /// Fetch all of `remote` first, so its other branches are current too; `git pull` alone
        /// only updates `remote`/`branch`.
        #[serde(default)]
        fetch_first: bool,
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
    /// Put uncommitted changes aside in a new stash, untracked files too with `untracked`: those
    /// of `paths`, or every change when it is empty.
    StashPush {
        message: Option<String>,
        untracked: bool,
        #[serde(default)]
        paths: Vec<String>,
    },
    /// Bring the changes of `stash@{index}` (whose id is `id`) back into the working copy;
    /// `pop` deletes the stash afterwards, `keep_index` restages what was staged.
    StashApply {
        index: usize,
        id: String,
        pop: bool,
        keep_index: bool,
    },
    /// Delete `stash@{index}`.
    StashDrop {
        index: usize,
        id: String,
    },
    /// Put a dropped stash back, e.g. to undo a drop or pop.
    StashStore {
        id: String,
        message: String,
    },
    /// Create the branch `name` at the commit the stash was made on and apply it there.
    StashBranch {
        index: usize,
        id: String,
        name: String,
    },
    /// Add a remote; nothing is fetched from it yet.
    AddRemote {
        name: String,
        url: String,
    },
    /// Point a remote at another address.
    SetRemoteUrl {
        name: String,
        url: String,
    },
    /// Load an SSH key into ssh-agent (`ssh-add`), e.g. after a restart emptied it.
    AddSshKey {
        key: String,
    },
    /// Forget a remote and its remote branches.
    RemoveRemote {
        name: String,
    },
    /// Pack loose objects and drop unreachable ones (`git gc`).
    Optimize,
    /// Tag `commit` as `name`: annotated with `message`, lightweight without one; with `push`,
    /// send it to that remote.
    CreateTag {
        name: String,
        commit: String,
        message: Option<String>,
        push: Option<String>,
    },
    /// Send tags to `remote`.
    PushTags {
        remote: String,
        names: Vec<String>,
    },
    /// Delete a tag; with `remote`, on that remote too.
    DeleteTag {
        name: String,
        remote: Option<String>,
    },
    /// Download every tag of `remote`.
    FetchTags {
        remote: String,
    },
    /// Put the repository back to how it was before step `id` of the Operation Log, or right
    /// after it with `after`.
    Restore {
        id: String,
        after: bool,
    },
    /// Forget every step of the Operation Log.
    ClearOperationLog,
    /// Rewrite a stack of branches as `plan` says: an interactive rebase.
    EditStack {
        plan: crate::stack::StackPlan,
    },
    /// Fold the staged changes into `commit`, an older commit of the checked-out branch.
    AddToCommit {
        commit: String,
    },
    /// Push several local branches to `remote` at once, each to its upstream, overwriting what
    /// the rebase replaced there while nobody else pushed (`--force-with-lease`).
    PushBranches {
        remote: String,
        branches: Vec<BranchPush>,
    },
    /// After `branch`'s pull request was merged into `base` on GitHub, bring the result here:
    /// fetch it, fast-forward the local `base`, and delete the branch on `remote` and here when
    /// asked to.
    PullRequestMerged {
        remote: String,
        base: String,
        branch: String,
        delete_remote: bool,
        delete_local: bool,
    },
    /// Keep files matching `pattern` in Git LFS, and stage `.gitattributes` and `paths`.
    LfsTrack {
        pattern: String,
        paths: Vec<String>,
    },
    /// Stop sending new files matching `pattern` to Git LFS.
    LfsUntrack {
        pattern: String,
    },
    /// Download LFS files in place of their pointers: those `include` matches, or all.
    LfsPull {
        include: Option<String>,
    },
    /// Delete local copies of LFS files that are no longer checked out and are on the server.
    LfsPrune,
    /// Install git-lfs with Homebrew at `brew` (when given) and turn it on; with `pull`,
    /// download this repository's LFS files after.
    LfsInstall {
        brew: Option<String>,
        pull: bool,
    },
}

/// A local branch and its name on the remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchPush {
    pub branch: String,
    pub upstream: String,
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
            let path = self.workdir().join(&file.path);
            let Ok(len) = std::fs::metadata(&path).map(|m| m.len()) else {
                continue;
            };
            // A big file is not read whole on every refresh: its start says whether it is binary.
            let data = if len > MAX_COUNTED_BYTES {
                let mut start = Vec::new();
                let _ = std::fs::File::open(&path).and_then(|f| f.take(8000).read_to_end(&mut start));
                start
            } else {
                std::fs::read(&path).unwrap_or_default()
            };
            file.binary = data.iter().take(8000).any(|&b| b == 0);
            if !file.binary && len <= MAX_COUNTED_BYTES {
                file.additions = data.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count() as u32;
            }
        }
        self.mark_lfs_and_sizes(&mut tree);
        Ok(tree)
    }

    /// Mark files `.gitattributes` sends to LFS, and give binary, LFS and big files their size in
    /// the working copy, for the badge in the file list and the note on a file too big for git.
    fn mark_lfs_and_sizes(&self, tree: &mut WorkingTree) {
        let paths: Vec<&str> = tree
            .staged
            .iter()
            .chain(&tree.unstaged)
            .filter(|f| f.status != FileStatus::Deleted)
            .map(|f| f.path.as_str())
            .collect();
        let lfs = self.lfs_paths(&paths);
        for file in tree.staged.iter_mut().chain(tree.unstaged.iter_mut()) {
            if file.status == FileStatus::Deleted {
                continue;
            }
            let tracked = lfs.contains(&file.path);
            if tracked {
                file.lfs = Some(LfsChange::default());
            }
            let Some((size, _)) = self.lfs_file_size(&file.path) else {
                continue;
            };
            if tracked || file.binary || size >= lfs::BIG_FILE {
                file.new_size = Some(size);
            }
        }
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
        let hunks: Vec<Hunk> = parsed.hunks.into_iter().map(|h| h.hunk).collect();
        let mut file = FileChange {
            binary: file.binary || parsed.binary,
            mode: parsed.mode,
            ..file
        };
        if !file.binary {
            file.additions = 0;
            file.deletions = 0;
            for line in hunks.iter().flat_map(|h| &h.lines) {
                match line.kind {
                    LineKind::Added => file.additions += 1,
                    LineKind::Removed => file.deletions += 1,
                    LineKind::Context => {}
                }
            }
            file.eol = eol_change(&hunks);
        }
        // The blob ids of `index <old>..<new>`; all zeros stands for a side that does not exist.
        let blob = |id: &Option<String>| id.clone().filter(|id| id.bytes().any(|b| b != b'0'));
        let old = blob(&parsed.ids.0).map(|id| Source::Blob { id });
        let new = match side {
            Side::Staged => blob(&parsed.ids.1).map(|id| Source::Blob { id }),
            Side::Unstaged => (file.status != FileStatus::Deleted).then(|| Source::Worktree {
                path: file.path.clone(),
            }),
        };
        if file.lfs.is_some() {
            // What git diffs for an LFS file is its pointer: three short lines.
            let side_text = |skip: LineKind| -> String {
                hunks
                    .iter()
                    .flat_map(|h| &h.lines)
                    .filter(|l| l.kind != skip)
                    .map(|l| format!("{}\n", l.text))
                    .collect()
            };
            let lfs = LfsChange {
                old: lfs::parse_pointer(side_text(LineKind::Added).as_bytes()),
                new: lfs::parse_pointer(side_text(LineKind::Removed).as_bytes()),
            };
            file.old_size = lfs.old.as_ref().map(|p| p.size);
            if let Some(new) = &lfs.new {
                file.new_size = Some(new.size);
            }
            file.lfs = Some(lfs);
        } else if file.binary {
            file.old_size = old
                .as_ref()
                .and_then(|s| self.read_source(s).ok())
                .map(|b| b.len() as u64);
            file.new_size = new
                .as_ref()
                .and_then(|s| self.read_source(s).ok())
                .map(|b| b.len() as u64);
        }
        let limited = over_limit(&file, self.diff_options().max_lines);
        let too_large = !limited && hunks.iter().map(|h| h.lines.len()).sum::<usize>() > MAX_DIFF_LINES;
        Ok(FileDiff {
            file,
            hunks: if too_large || limited { Vec::new() } else { hunks },
            too_large,
            limited,
            old,
            new,
        })
    }

    /// The commands `action` will run.
    pub fn plan(&self, action: &Action) -> Result<Plan> {
        let paths = |args: &[&str], paths: &[String]| {
            GitCommand::new(
                args.iter()
                    .map(|s| s.to_string())
                    .chain(["--".to_owned()])
                    .chain(paths.iter().map(|p| literal(p))),
            )
        };
        let commands = match action {
            Action::Stage { paths: p } => vec![paths(&["add"], p).comment("add the files to the next commit")],
            Action::Unstage { paths: p } => {
                if self.head()?.commit.is_some() {
                    // A rename is a deletion of the old path too: unstage both halves.
                    let tree = self.working_tree()?;
                    let mut p = p.clone();
                    for file in &tree.staged {
                        if let Some(old) = file.old_path.as_ref().filter(|_| p.contains(&file.path))
                            && !p.contains(old)
                        {
                            p.push(old.clone());
                        }
                    }
                    vec![
                        paths(&["restore", "--staged"], &p).comment("take them out of the next commit, keep the edits"),
                    ]
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
            Action::StageHunk {
                path,
                header,
                check,
                lines,
            }
            | Action::UnstageHunk {
                path,
                header,
                check,
                lines,
            }
            | Action::DiscardHunk {
                path,
                header,
                check,
                lines,
            } => {
                let (side, args): (_, &[&str]) = match action {
                    Action::StageHunk { .. } => (Side::Unstaged, &["apply", "--cached", "-"]),
                    Action::UnstageHunk { .. } => (Side::Staged, &["apply", "--cached", "--reverse", "-"]),
                    _ => (Side::Unstaged, &["apply", "--reverse", "-"]),
                };
                let cmd = GitCommand::new(args.iter().copied());
                match lines {
                    None => vec![cmd.comment(stdin_note(path, header))],
                    Some(lines) => {
                        let forward = matches!(action, Action::StageHunk { .. });
                        let patch = self.hunk_patch(path, side, header, check, Some(lines), forward)?;
                        let what = match action {
                            Action::StageHunk { .. } => {
                                "--cached: only the staging area changes, your file stays as is. By hand: git add -p, then e"
                            }
                            Action::UnstageHunk { .. } => "--reverse: take these lines back out of the staging area",
                            _ => "--reverse: undo just these lines in the file on disk",
                        };
                        vec![cmd.comment(what).input(patch)]
                    }
                }
            }
            Action::Ignore { pattern } => vec![
                GitCommand::new(["add", "--", ".gitignore"])
                    .comment("stage .gitignore, so the rule reaches everyone with the next commit")
                    .before(format!("echo {} >> .gitignore", crate::cli::shell_quote(pattern))),
            ],
            Action::Commit {
                message,
                amend,
                no_verify,
            } => {
                let mut args = vec!["commit".to_owned()];
                if *amend {
                    args.push("--amend".to_owned());
                }
                if *no_verify || !config::run_hooks() {
                    args.push("--no-verify".to_owned());
                }
                // One -m per paragraph, as people type it; git puts the blank lines back between them.
                for paragraph in paragraphs(message) {
                    args.push("-m".to_owned());
                    args.push(paragraph);
                }
                let cmd = GitCommand::new(args);
                vec![if *amend {
                    cmd.comment("replace the last commit with one that also has the staged changes")
                } else if *no_verify {
                    cmd.comment("--no-verify: skip the pre-commit and commit-msg hooks this once")
                } else if !config::run_hooks() {
                    cmd.comment(HOOKS_OFF)
                } else {
                    cmd
                }]
            }
            Action::Fetch { remote } => vec![self.fetch_command(remote.as_deref()).with_progress()],
            Action::Pull {
                remote,
                branch,
                fetch_first,
            } => {
                let mut plan = Vec::new();
                if *fetch_first {
                    plan.push(self.fetch_command(Some(remote)).with_progress());
                }
                plan.push(self.pull_command(remote, branch));
                plan
            }
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
                } else if !config::run_hooks() {
                    args.push("--no-verify".to_owned());
                    notes.push(HOOKS_OFF.to_owned());
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
                if *no_verify || !config::run_hooks() {
                    push.push("--no-verify".to_owned());
                }
                push.extend([remote.clone(), refspec(branch, upstream)]);
                vec![
                    self.pull_command(remote, upstream),
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
            Action::Resolve { path, .. } => vec![GitCommand::new(["add", "--", &literal(path)]).comment(format!(
                "Oxbow writes your choices into {path} first; add marks it resolved"
            ))],
            Action::TakeFile { path, side } => self.plan_take_file(path, *side)?,
            Action::CherryPick { commit } => self.plan_cherry_pick(commit)?,
            Action::Revert { commit } => self.plan_revert(commit)?,
            Action::Reset { commit, mode } => plan_reset(commit, *mode),
            Action::StashPush {
                message,
                untracked,
                paths,
            } => {
                let mut args = vec!["stash".to_owned(), "push".to_owned()];
                if *untracked {
                    args.push("--include-untracked".to_owned());
                }
                if let Some(message) = message.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
                    args.extend(["-m".to_owned(), message.to_owned()]);
                }
                if !paths.is_empty() {
                    args.push("--".to_owned());
                    args.extend(paths.iter().map(|p| literal(p)));
                }
                let cmd = GitCommand::new(args);
                vec![match (paths.is_empty(), *untracked) {
                    (false, true) => cmd.comment("-- then paths: only these files; --include-untracked: new ones too"),
                    (false, false) => cmd.comment("-- then paths: only these files, the rest stays as it is"),
                    (true, true) => cmd.comment("--include-untracked: new files are stashed too"),
                    (true, false) => cmd.comment("new, untracked files stay where they are"),
                }]
            }
            Action::StashApply {
                index,
                id,
                pop,
                keep_index,
            } => {
                let name = self.stash_ref(*index, id)?;
                let mut args = vec!["stash", if *pop { "pop" } else { "apply" }];
                if *keep_index {
                    args.push("--index");
                }
                args.push(&name);
                let comment = match (*pop, *keep_index) {
                    (true, true) => "pop: apply, then delete the stash; --index: staged files come back staged",
                    (true, false) => "pop: apply, then delete the stash",
                    (false, true) => "the stash stays in the list; --index: staged files come back staged",
                    (false, false) => "the stash stays in the list",
                };
                vec![GitCommand::new(args).comment(comment)]
            }
            Action::StashDrop { index, id } => vec![
                GitCommand::new(["stash".to_owned(), "drop".to_owned(), self.stash_ref(*index, id)?])
                    .comment("later stashes move up one number"),
            ],
            Action::StashStore { id, message } => vec![
                GitCommand::new(["stash", "store", "-m", message, id])
                    .comment("puts the dropped stash back as stash@{0}"),
            ],
            Action::StashBranch { index, id, name } => vec![
                GitCommand::new([
                    "stash".to_owned(),
                    "branch".to_owned(),
                    name.clone(),
                    self.stash_ref(*index, id)?,
                ])
                .comment("a new branch where the stash was made, with the stash applied and dropped"),
            ],
            Action::AddRemote { name, url } => vec![
                GitCommand::new(["remote", "add", name, url])
                    .comment(format!("Fetch shows {name}’s branches as {name}/…")),
            ],
            Action::SetRemoteUrl { name, url } => vec![
                GitCommand::new(["remote", "set-url", name, url]).comment("fetch and push both use the new address"),
            ],
            Action::AddSshKey { key } => vec![crate::ssh::add_key_command(key)],
            Action::RemoveRemote { name } => vec![
                GitCommand::new(["remote", "remove", name])
                    .comment(format!("{name}/… branches go too; local branches stay")),
            ],
            Action::CreateTag {
                name,
                commit,
                message,
                push,
            } => tags::plan_create(name, commit, message.as_deref(), push.as_deref()),
            Action::PushTags { remote, names } => vec![tags::push_command(remote, names)],
            Action::DeleteTag { name, remote } => tags::plan_delete(name, remote.as_deref()),
            Action::FetchTags { remote } => tags::plan_fetch(remote),
            Action::Optimize => {
                vec![GitCommand::new(["gc"]).comment("packs loose objects and drops ones nothing points at any more")]
            }
            Action::Restore { id, after } => self.plan_restore(id, *after)?,
            Action::ClearOperationLog => vec![
                GitCommand::new(["update-ref", "-d", crate::oplog::OPLOG_REF])
                    .comment("the snapshots go with git's next clean-up"),
            ],
            Action::EditStack { plan } => self.plan_edit_stack(plan)?,
            Action::AddToCommit { commit } => self.plan_add_to_commit(commit)?,
            Action::PushBranches { remote, branches } => {
                let mut args = vec!["push".to_owned(), "--force-with-lease".to_owned()];
                if !config::run_hooks() {
                    args.push("--no-verify".to_owned());
                }
                args.push(remote.clone());
                args.extend(branches.iter().map(|b| refspec(&b.branch, &b.upstream)));
                let comment = format!(
                    "--force-with-lease: each one only if {remote} still has what was fetched last, so nobody else’s new commits get overwritten"
                );
                vec![GitCommand::new(args).comment(comment).with_progress()]
            }
            Action::PullRequestMerged {
                remote,
                base,
                branch,
                delete_remote,
                delete_local,
            } => self.plan_pull_request_merged(remote, base, branch, *delete_remote, *delete_local)?,
            Action::LfsTrack { pattern, paths } => lfs::plan_track(pattern, paths),
            Action::LfsUntrack { pattern } => lfs::plan_untrack(pattern),
            Action::LfsPull { include } => lfs::plan_pull(include.as_deref()),
            Action::LfsPrune => lfs::plan_prune(),
            Action::LfsInstall { brew, pull } => lfs::plan_install(brew.as_deref(), *pull),
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
    /// `cancel` stops the running command. The step goes into the Operation Log with the state
    /// before and after it, so it can be undone; a log that can't be written never stops it.
    pub fn perform_with(
        &self,
        action: &Action,
        on_event: &mut dyn FnMut(ActionEvent),
        cancel: &AtomicBool,
    ) -> Result<String> {
        // Loading a key changes nothing in the repository.
        if let Action::AddSshKey { .. } = action {
            return self.perform_unlogged(action, on_event, cancel);
        }
        if let Action::ClearOperationLog = action {
            let out = self.perform_unlogged(action, on_event, cancel)?;
            self.clear_operation_log()?;
            return Ok(out);
        }
        if let Action::Restore { id, .. } = action {
            self.prepare_restore(id)?;
        }
        let before = self.snapshot().ok();
        if let Action::EditStack { plan } = action {
            self.remember_return(plan);
        }
        let result = self.perform_unlogged(action, on_event, cancel);
        self.forget_return();
        if let Action::Restore { .. } = action {
            // read-tree forgets what git knew about the files on disk.
            let _ = self.run(&GitCommand::new(["update-index", "-q", "--refresh"]));
        }
        if let Some(before) = before
            && let Ok(after) = self.snapshot()
        {
            let _ = self.record(action, &before, &after, result.is_err());
        }
        result
    }

    fn perform_unlogged(
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
        let (side, path, header, check, lines) = match action {
            Action::StageHunk {
                path,
                header,
                check,
                lines,
            } => (Side::Unstaged, path, header, check, lines),
            Action::DiscardHunk {
                path,
                header,
                check,
                lines,
            } => (Side::Unstaged, path, header, check, lines),
            Action::UnstageHunk {
                path,
                header,
                check,
                lines,
            } => (Side::Staged, path, header, check, lines),
            _ => {
                if let Action::Resolve { path, picks } = action {
                    self.write_resolution(path, picks)?;
                }
                if let Action::Ignore { pattern } = action {
                    self.add_ignore_rule(pattern)?;
                }
                // The note of a conflicted apply goes when the apply is finished or undone.
                let pending = self.pending_apply().is_some()
                    && matches!(action, Action::Continue { .. } | Action::Abort)
                    && self.operation()?.is_some_and(|op| op.kind == OperationKind::StashApply);
                if pending
                    && matches!(action, Action::Abort)
                    && let Some(apply) = self.pending_apply()
                {
                    self.remove_brought_back(&apply)?;
                }
                // Files git will not restore over, so an undo of the apply leaves them be.
                let in_the_way = match action {
                    Action::StashApply { id, .. } => self.untracked_in_the_way(id),
                    _ => Vec::new(),
                };
                let mut last = String::new();
                for command in self.plan(action)?.commands {
                    self.write_todo(&command)?;
                    let command = crate::ssh::interactive(command);
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
                            if let Action::StashApply { id, pop, .. } = action
                                && self.conflicted_paths().is_ok_and(|p| !p.is_empty())
                            {
                                self.remember_apply(id, *pop, &in_the_way);
                            }
                        })?;
                    last = if out.stdout.trim().is_empty() {
                        out.stderr
                    } else {
                        out.stdout
                    };
                }
                if pending {
                    self.forget_apply();
                }
                self.after_tags(action);
                return Ok(last);
            }
        };
        // Staging goes forward; unstaging and discarding apply the patch in reverse.
        let forward = matches!(action, Action::StageHunk { .. });
        let patch = self.hunk_patch(path, side, header, check, lines.as_deref(), forward)?;
        let command = &self.plan(action)?.commands[0];
        on_event(ActionEvent::Command {
            display: command.display(),
        });
        let out = self.run_with_input(command, Some(patch.as_bytes()))?;
        Ok(out.stdout)
    }

    /// A patch with the file header and only the hunk that starts with `header` and still has
    /// the bytes `check` was made from; with `lines`, only those changed lines of it, for applying `forward`
    /// (staging) or in reverse.
    #[allow(clippy::too_many_arguments)]
    fn hunk_patch(
        &self,
        path: &str,
        side: Side,
        header: &str,
        check: &str,
        lines: Option<&[usize]>,
        forward: bool,
    ) -> Result<String> {
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
            // The header gives only line numbers: an edit inside the hunk keeps them.
            .find(|h| h.header == header && h.hunk.check == check)
            .ok_or_else(|| Error::Git(format!("{path} changed since it was shown; refresh and try again")))?;
        let text = match lines {
            None => hunk.text.clone(),
            Some(lines) => partial_hunk(&hunk.text, lines, forward)
                .ok_or_else(|| Error::Git("pick at least one changed line".into()))?,
        };
        Ok(format!("{}{}", parsed.file_header, text))
    }

    /// Add `pattern` as a line at the end of the top `.gitignore`, creating it if needed.
    fn add_ignore_rule(&self, pattern: &str) -> Result<()> {
        let pattern = pattern.trim_end_matches(['\r', '\n']);
        if pattern.trim().is_empty() || pattern.contains('\n') {
            return Err(Error::Git("an ignore rule is one line of text".into()));
        }
        let file = self.workdir().join(".gitignore");
        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
        if text.lines().any(|line| line == pattern) {
            return Ok(());
        }
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(pattern);
        text.push('\n');
        std::fs::write(&file, text).map_err(|err| Error::Git(format!("could not write .gitignore: {err}")))
    }

    /// `git diff` output for one file.
    fn raw_diff(&self, file: &FileChange, side: Side, whole_file: bool) -> Result<String> {
        let context = if whole_file {
            "-U100000000".to_owned()
        } else {
            format!("-U{}", self.diff_options().context_lines)
        };
        let mut args = vec![
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            "--full-index",
            &context,
        ];
        if file.status == FileStatus::Untracked {
            // `--no-index` exits with 1 when the files differ, which they always do here.
            args.extend(["--no-index", "--", "/dev/null", &file.path]);
            let out = self.spawn(&GitCommand::new(args), None)?;
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        if side == Side::Staged {
            args.push("--cached");
        }
        let path = literal(&file.path);
        args.extend(["--", &path]);
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

/// How Pull brings in the remote's commits, from `pull.rebase` and `pull.ff`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PullMode {
    /// Oxbow's default when neither is set: your commits go on top, no merge commit.
    Rebase,
    Merge,
    FastForward,
}

impl Repo {
    pub fn pull_mode(&self) -> PullMode {
        if self.config_value("pull.ff").is_some_and(|ff| ff == "only") {
            PullMode::FastForward
        } else if self.config_bool("pull.rebase") == Some(false) {
            PullMode::Merge
        } else {
            PullMode::Rebase
        }
    }

    /// Whether Pull puts uncommitted changes aside and back (`rebase.autoStash`, on by default).
    pub fn pull_autostash(&self) -> bool {
        self.config_bool("rebase.autoStash").unwrap_or(true)
    }
}

/// What a commit or push says when Settings turned hooks off.
const HOOKS_OFF: &str = "--no-verify: Git hooks are turned off in Settings";

impl Repo {
    /// `git fetch` of one remote or all, pruning unless `fetch.prune` is turned off.
    pub fn fetch_command(&self, remote: Option<&str>) -> GitCommand {
        let prune = self.config_bool("fetch.prune").unwrap_or(true);
        let mut args = vec!["fetch"];
        if prune {
            args.push("--prune");
        }
        args.push(remote.unwrap_or("--all"));
        let comment = match (remote, prune) {
            (Some(remote), true) => format!("--prune: drop {remote}/* branches deleted on the remote"),
            (Some(remote), false) => format!("gets what is new on {remote}"),
            (None, true) => "--all: every remote, --prune: drop branches deleted there".to_owned(),
            (None, false) => "--all: every remote".to_owned(),
        };
        GitCommand::new(args).comment(comment)
    }

    /// `git pull` the way Settings › Git says: rebase (the default), merge or fast-forward only,
    /// read from `pull.rebase` and `pull.ff` so it matches a terminal.
    fn pull_command(&self, remote: &str, branch: &str) -> GitCommand {
        let (mode, note) = match self.pull_mode() {
            PullMode::FastForward => ("--ff-only", "--ff-only: only if your branch has no commits of its own"),
            PullMode::Merge => (
                "--no-rebase",
                "--no-rebase: a merge commit joins the remote's commits with yours",
            ),
            PullMode::Rebase => ("--rebase", "--rebase: no merge commit, your commits go on top"),
        };
        let mut args = vec!["pull", mode];
        let mut comment = note.to_owned();
        if self.pull_autostash() {
            args.push("--autostash");
            comment.push_str("; --autostash: keep uncommitted files");
        }
        args.extend([remote, branch]);
        GitCommand::new(args).comment(comment).with_progress()
    }
}

/// `main`, or `local:remote` when the branch has another name on the remote.
fn refspec(branch: &str, upstream: &str) -> String {
    if branch == upstream {
        branch.to_owned()
    } else {
        format!("{branch}:{upstream}")
    }
}

/// A fingerprint of a hunk's text (FNV-1a), enough to notice that it changed.
fn fingerprint(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}{:x}", text.len())
}

fn stdin_note(path: &str, header: &str) -> String {
    format!("the patch of the {header} hunk of {path} comes on stdin")
}

/// A message split at its blank lines, each paragraph trimmed.
pub(crate) fn paragraphs(message: &str) -> Vec<String> {
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
        ..FileChange::default()
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
    /// `old mode` and `new mode` from the header.
    mode: Option<ModeChange>,
    /// The blob ids of the `index` line.
    ids: (Option<String>, Option<String>),
}

/// Parse the output of `git diff` for a single file.
fn parse_patch(patch: &str) -> ParsedPatch {
    let mut file_header = String::new();
    let mut hunks: Vec<ParsedHunk> = Vec::new();
    let mut binary = false;
    let (mut old_mode, mut new_mode, mut ids) = (None, None, (None, None));
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
                    check: String::new(),
                },
            });
            continue;
        }
        let Some(current) = hunks.last_mut() else {
            if body.starts_with("Binary files ") || body == "GIT binary patch" {
                binary = true;
            } else if let Some(m) = body.strip_prefix("old mode ") {
                old_mode = Some(m.trim().to_owned());
            } else if let Some(m) = body.strip_prefix("new mode ") {
                new_mode = Some(m.trim().to_owned());
            } else if let Some((a, b)) = body
                .strip_prefix("index ")
                .and_then(|r| r.split(' ').next()?.split_once(".."))
            {
                ids = (Some(a.to_owned()), Some(b.to_owned()));
            }
            file_header.push_str(line);
            continue;
        };
        current.text.push_str(line);
        let text = body.get(1..).unwrap_or_default().trim_end_matches('\r').to_owned();
        let cr = body.ends_with('\r');
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
            cr,
            words: None,
        });
    }
    for hunk in &mut hunks {
        mark_words(&mut hunk.hunk.lines);
        hunk.hunk.check = fingerprint(&hunk.text);
    }
    let mode = match (old_mode, new_mode) {
        (Some(old), Some(new)) => Some(ModeChange { old, new }),
        _ => None,
    };
    ParsedPatch {
        file_header,
        hunks,
        binary,
        mode,
        ids,
    }
}

/// The hunk `text` (its `@@` line first) with only the changed lines whose indexes, counted
/// as in [`Hunk::lines`], are in `selected`, for `git apply` going `forward` or with `--reverse`.
/// git has no command for single lines; this is what `git add -p` and `e` do by hand. A changed
/// line left out is dropped when it only exists on the side the patch makes, and becomes
/// context when the side the patch applies to already has it. `None` when nothing is selected.
fn partial_hunk(text: &str, selected: &[usize], forward: bool) -> Option<String> {
    let mut rows = text.split_inclusive('\n');
    let head = rows.next()?;
    let rest = head.strip_prefix("@@ ")?;
    let end = rest.find(" @@")?;
    let mut ranges = rest[..end].split(' ');
    let (old_start, old_lines) = parse_range(ranges.next()?.trim_start_matches('-'));
    let (new_start, new_lines) = parse_range(ranges.next()?.trim_start_matches('+'));
    // The function name git shows after the second @@, and the line end.
    let tail = &rest[end + 3..];

    let mut body = String::new();
    let (mut old, mut new, mut index, mut picked) = (0u32, 0u32, 0usize, 0usize);
    // Whether the line before a "\ No newline at end of file" made it into the patch.
    let mut kept = false;
    for row in rows {
        match row.as_bytes().first() {
            Some(b'+') | Some(b'-') => {
                let added = row.starts_with('+');
                if selected.contains(&index) {
                    picked += 1;
                    body.push_str(row);
                    if added {
                        new += 1;
                    } else {
                        old += 1;
                    }
                    kept = true;
                } else if added != forward {
                    body.push(' ');
                    body.push_str(&row[1..]);
                    old += 1;
                    new += 1;
                    kept = true;
                } else {
                    kept = false;
                }
                index += 1;
            }
            Some(b' ') => {
                body.push_str(row);
                old += 1;
                new += 1;
                kept = true;
                index += 1;
            }
            Some(b'\\') if kept => body.push_str(row),
            _ => {}
        }
    }
    if picked == 0 {
        return None;
    }
    // A side with no lines names the line before it, so the start moves by one when that changes.
    let start = |start: u32, was: u32, now: u32| match (was, now) {
        (0, n) if n > 0 => start + 1,
        (w, 0) if w > 0 => start.saturating_sub(1),
        _ => start,
    };
    Some(format!(
        "@@ -{},{} +{},{} @@{tail}{body}",
        start(old_start, old_lines, old),
        old,
        start(new_start, new_lines, new),
        new
    ))
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
