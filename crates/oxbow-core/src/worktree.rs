//! The working copy: what changed since the last commit, staging and committing.
//!
//! Status and diffs come from the `git` command line, so they honor every setting that affects
//! them (ignore rules, attributes, line endings, fsmonitor), and a hunk shown on screen is
//! exactly the hunk `git apply` gets when it is staged. Every change to the repository is an
//! [`Action`] that turns into the [`GitCommand`]s shown in the confirmation sheet.

use serde::{Deserialize, Serialize};

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::bisect::BisectMark;
use crate::cli::{GitCommand, OutputLine, literal};
use crate::commit::{
    DiffLine, DiffPage, FileChange, FileDiff, FileStatus, Hunk, LineKind, ModeChange, Source, SubmoduleChange,
    eol_change, over_limit, word_diff,
};
use crate::config;
use crate::edit::{ResetMode, plan_reset};
use crate::error::{Error, Result};
use crate::lfs::{self, LfsChange};
use crate::operation::{ConflictSide, MergeMethod, OperationKind, Pick};
use crate::repo::Repo;
use crate::tags;
use std::io::Read;

/// What the Console calls the commands that take the snapshots an Undo goes back to.
const UNDO_SNAPSHOT: &str = "snapshot for Undo";

/// Files with more changed lines than this are not shown line by line.
const MAX_DIFF_LINES: usize = 20_000;

/// Untracked files bigger than this are not read to count their lines.
const MAX_COUNTED_BYTES: u64 = 8 * 1024 * 1024;
/// Bytes of new files a reading of the working tree reads at most to count their lines; the
/// files past them are counted by the readings after.
const MAX_FRESH_BYTES: u64 = 32 * 1024 * 1024;

/// What counting a new file's lines found, with the size and time of change it had then.
#[derive(Clone, Copy)]
struct Counted {
    len: u64,
    modified: std::time::SystemTime,
    binary: bool,
    additions: u32,
}

/// Lines counted in new files by path, kept while the files keep their size and time of change.
#[derive(Clone, Default)]
pub(crate) struct UntrackedCounts(std::sync::Arc<std::sync::Mutex<HashMap<String, Counted>>>);
/// Bytes of a file's patch read at most: a longer one is too large to show, as a longer blob is
/// in a commit's diff.
const MAX_PATCH_BYTES: usize = 16 * 1024 * 1024;
/// Files a page of the Summary's diffs takes at most, whatever its line budget: each is a path
/// on one `git diff` command line.
const PAGE_FILES: usize = 100;

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

/// `LfsTrack` stages what it changes unless asked not to.
fn staged_by_default() -> bool {
    true
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
        /// Commit even though the secrets check finds something, this one time.
        #[serde(default)]
        allow_secrets: bool,
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
        /// Check out the commits the pulled commits record in the submodules too.
        #[serde(default)]
        submodules: bool,
        /// Merge or rebase for this one pull, over `pull.rebase` and `pull.ff`.
        #[serde(default)]
        mode: Option<PullMode>,
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
        /// Push the submodule commits this branch records that their remotes don't have, first.
        #[serde(default)]
        submodules: bool,
    },
    /// Pull with rebase, then push: the way out of a push rejected for new remote commits.
    PullAndPush {
        remote: String,
        branch: String,
        upstream: String,
        /// Skip the pre-push hook, when the push being retried skipped it too.
        no_verify: bool,
    },
    /// Push a branch whose pushed commits were rewritten here while someone pushed on top of
    /// the old versions: copy their commits after `base` (the state this clone last pushed)
    /// onto the branch first, then replace the old versions with `--force-with-lease` at
    /// `lease`. A pull with rebase would drop an edited message as already upstream.
    ReplayAndPush {
        remote: String,
        branch: String,
        upstream: String,
        base: String,
        lease: String,
        #[serde(default)]
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
    /// Delete a branch on its remote; with `expect`, only while it is still at that commit
    /// there (`--force-with-lease`).
    DeleteRemoteBranch {
        remote: String,
        branch: String,
        #[serde(default)]
        expect: Option<String>,
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
        /// Skip the pre-commit and commit-msg hooks this one time.
        #[serde(default)]
        no_verify: bool,
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
    /// Download the rest of the history of a shallow clone from `remote`.
    Unshallow {
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
    /// fetch it and fast-forward the local `base`, checked out or not.
    PullRequestMerged {
        remote: String,
        base: String,
        branch: String,
    },
    /// Delete the local `branch` of a merged pull request, only while it is still at `sha`, the
    /// commit GitHub merged; when it is checked out, `base` is checked out instead.
    DeleteMergedBranch {
        base: String,
        branch: String,
        sha: String,
    },
    /// Keep files matching `pattern` in Git LFS; with `stage`, stage `.gitattributes` and `paths`.
    LfsTrack {
        pattern: String,
        paths: Vec<String>,
        #[serde(default = "staged_by_default")]
        stage: bool,
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
    /// Start bisecting: git checks out commits between `good` and `bad` to find the first bad
    /// one. With `stash`, uncommitted changes are put aside first; with `command`, git tests
    /// every step with it and answers on its own.
    BisectStart {
        good: String,
        bad: String,
        stash: bool,
        #[serde(default)]
        command: Option<String>,
    },
    /// Answer for `commit`, or for the commit under test when there is none.
    BisectMark {
        mark: BisectMark,
        #[serde(default)]
        commit: Option<String>,
    },
    /// Stop bisecting, or finish once the commit is found: back to where it started, with the
    /// changes stashed before it.
    BisectReset,
    /// A new working folder at `path` with `branch` checked out; with `create`, a new branch of
    /// that name made at `start` (`HEAD` when missing).
    WorktreeAdd {
        path: String,
        branch: String,
        create: bool,
        #[serde(default)]
        start: Option<String>,
    },
    /// Delete the worktree at `path`, its folder too. `force` throws its uncommitted changes
    /// away; `stash` puts them in the stash first instead.
    WorktreeRemove {
        path: String,
        force: bool,
        #[serde(default)]
        stash: bool,
    },
    /// Keep git from pruning or removing the worktree at `path`.
    WorktreeLock {
        path: String,
        #[serde(default)]
        reason: Option<String>,
    },
    WorktreeUnlock {
        path: String,
    },
    /// Forget worktrees whose folders are gone.
    WorktreePrune,
    /// A worktree folder moved by hand is now at `path`: reconnect it.
    WorktreeRepair {
        path: String,
    },
    /// Check out the commits this repository records in the submodules at `paths` (all when
    /// empty), cloning them first where needed. With `latest`, the newest commit of the branch
    /// each one follows instead.
    SubmoduleUpdate {
        paths: Vec<String>,
        #[serde(default)]
        latest: bool,
    },
    /// Copy the submodules' URLs from `.gitmodules` to their settings.
    SubmoduleSync {
        paths: Vec<String>,
    },
    /// A new submodule: `url` cloned into `path`, following `branch` when given.
    SubmoduleAdd {
        url: String,
        path: String,
        #[serde(default)]
        branch: Option<String>,
    },
    /// Remove the submodule at `path`, its folder too.
    SubmoduleRemove {
        path: String,
    },
    /// A way out of a typical mistake, in a few steps, planned when the repository looked like
    /// `expect`: it runs only while it still does.
    Fix {
        fix: crate::fix::Fix,
        expect: crate::fix::FixExpect,
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
    /// What the Operation Log can't bring back once it ran; filled in for the sheet only, by
    /// [`Repo::not_kept`].
    pub not_kept: Vec<crate::notkept::NotKept>,
    /// For a fix: its steps in words, each with how many of `commands` are its own.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<crate::fix::FixStep>,
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
        let known = std::mem::take(&mut *self.untracked.0.lock().expect("counts lock"));
        let mut kept = HashMap::new();
        let mut budget = MAX_FRESH_BYTES;
        for file in tree.unstaged.iter_mut().filter(|f| f.status == FileStatus::Untracked) {
            let path = self.workdir().join(&file.path);
            // A symlink counts as one, as for git: what it points at may never end (/dev/zero).
            let Some(meta) = std::fs::symlink_metadata(&path).ok().filter(|m| m.is_file()) else {
                continue;
            };
            let len = meta.len();
            let modified = meta.modified().ok();
            // A file with the size and time of change it was counted with is not read again.
            if let Some(seen) = known
                .get(&file.path)
                .filter(|c| c.len == len && Some(c.modified) == modified)
            {
                file.binary = seen.binary;
                file.additions = seen.additions;
                kept.insert(file.path.clone(), *seen);
                continue;
            }
            // Its start says whether it is binary; only a text file is read whole, unless it is
            // too big or past this reading's budget.
            let mut start = Vec::new();
            let _ = std::fs::File::open(&path).and_then(|f| f.take(8000).read_to_end(&mut start));
            file.binary = start.contains(&0);
            if !file.binary {
                if len > MAX_COUNTED_BYTES {
                    continue;
                }
                if len > budget {
                    file.uncounted = true;
                    continue;
                }
                budget -= len;
                let data = crate::lfs::read_regular(&path, MAX_COUNTED_BYTES).unwrap_or_default();
                file.additions = data.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count() as u32;
            }
            // A file changed within the last moments may change again with the same size and
            // time: it is counted again next time.
            if let Some(modified) = modified.filter(|m| m.elapsed().is_ok_and(|age| age.as_secs() >= 2)) {
                let counted = Counted {
                    len,
                    modified,
                    binary: file.binary,
                    additions: file.additions,
                };
                kept.insert(file.path.clone(), counted);
            }
        }
        *self.untracked.0.lock().expect("counts lock") = kept;
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
        self.diff_of(file, side, whole_file, &AtomicBool::new(false))
    }

    /// The diffs of the files on one side of the working copy, in the order of `paths`, for
    /// the Summary of Uncommitted changes. Paths without changes on that side are left out.
    pub fn working_diffs(&self, paths: &[String], side: Side, whole_file: bool) -> Result<Vec<FileDiff>> {
        self.working_diffs_in(&self.working_tree()?, paths, side, whole_file)
    }

    /// [`Repo::working_diffs`] of the files of `tree`, a reading of the working tree made a moment
    /// ago, so that git is not asked for every file's status again. The diffs themselves are read
    /// now; staging a hunk or lines checks it against the files as they are then.
    pub fn working_diffs_in(
        &self,
        tree: &WorkingTree,
        paths: &[String],
        side: Side,
        whole_file: bool,
    ) -> Result<Vec<FileDiff>> {
        let never = AtomicBool::new(false);
        let mut diffs = Vec::new();
        let mut from = Some(0);
        while let Some(at) = from {
            let page = self.working_diff_page(tree, paths, side, whole_file, at, usize::MAX, &never)?;
            diffs.extend(page.diffs);
            from = page.next;
        }
        Ok(diffs)
    }

    /// A page of [`Repo::working_diffs_in`]: the diffs of the files of `paths` from the `from`th
    /// on, until they have about `budget` lines or [`PAGE_FILES`] files. The patches of tracked
    /// files come from one `git diff`, and new files are read here, so a page of hundreds of
    /// small changes runs a few git commands, not one per file. Setting `cancel` stops it between
    /// commands.
    #[allow(clippy::too_many_arguments)]
    pub fn working_diff_page(
        &self,
        tree: &WorkingTree,
        paths: &[String],
        side: Side,
        whole_file: bool,
        from: usize,
        budget: usize,
        cancel: &AtomicBool,
    ) -> Result<DiffPage> {
        let list = match side {
            Side::Staged => &tree.staged,
            Side::Unstaged => &tree.unstaged,
        };
        // A path in both lists is the side's own change.
        let mut by_path: HashMap<&str, &FileChange> = tree.conflicted.iter().map(|f| (f.path.as_str(), f)).collect();
        by_path.extend(list.iter().map(|f| (f.path.as_str(), f)));
        let files: Vec<&FileChange> = paths.iter().filter_map(|p| by_path.get(p.as_str()).copied()).collect();
        let max_lines = self.diff_options().max_lines;
        let context = self.diff_options().context_lines as usize;
        // The files of one batch are guessed from their counts beforehand; the page itself stops
        // at the lines actually read, and files of the batch past that wait for the next page.
        let guess = |file: &FileChange| -> usize {
            if self.needs_no_patch(file, max_lines) || file.binary || file.submodule.is_some() {
                return 8;
            }
            let changed = (file.additions + file.deletions) as usize;
            // A new file not counted yet shows all its lines too.
            if whole_file || file.uncounted {
                // Every line of the file: about one in 16 bytes, unless it was all added or removed.
                let size = std::fs::symlink_metadata(self.workdir().join(&file.path)).map_or(0, |m| m.len());
                changed.max((size / 16) as usize) + 6
            } else {
                changed + 6 + 2 * context
            }
        };
        let mut end = from.min(files.len());
        let mut guessed = 0usize;
        while end < files.len() && (end == from || (guessed < budget && end - from < PAGE_FILES)) {
            guessed = guessed.saturating_add(guess(files[end]));
            end += 1;
        }
        let page = &files[from.min(end)..end];
        let stopped = || cancel.load(Ordering::Relaxed);
        if stopped() {
            return Err(Error::Cancelled);
        }

        let mut patches: HashMap<String, String> = HashMap::new();
        let tracked: Vec<&str> = page
            .iter()
            .filter(|f| {
                f.submodule.is_none()
                    && !matches!(f.status, FileStatus::Untracked | FileStatus::Conflicted)
                    && !self.needs_no_patch(f, max_lines)
            })
            .map(|f| f.path.as_str())
            .collect();
        if !tracked.is_empty() {
            patches.extend(self.batch_patches(&tracked, side, whole_file, cancel)?);
        }
        let fresh: Vec<&FileChange> = page
            .iter()
            .copied()
            .filter(|f| f.status == FileStatus::Untracked && !self.needs_no_patch(f, max_lines))
            .collect();
        if !fresh.is_empty() {
            if stopped() {
                return Err(Error::Cancelled);
            }
            patches.extend(self.untracked_patches(&fresh, cancel)?);
        }

        let mut diffs = Vec::with_capacity(page.len());
        let mut lines = 0usize;
        let mut next = (end < files.len()).then_some(end);
        for (at, file) in page.iter().enumerate() {
            if !diffs.is_empty() && lines >= budget {
                next = Some(from + at);
                break;
            }
            if stopped() {
                return Err(Error::Cancelled);
            }
            let file = (*file).clone();
            let diff = match patches.remove(&file.path) {
                Some(patch) => self.diff_from_patch(file, side, &patch, false),
                // Files outside the batch, or whose part of it git did not print as expected,
                // take a command of their own.
                None => self.diff_of(file, side, whole_file, cancel)?,
            };
            // A card (binary, too large, a submodule) takes about as much room as a few lines.
            lines += diff.hunks.iter().map(|h| h.lines.len()).sum::<usize>().max(8);
            diffs.push(diff);
        }
        Ok(DiffPage {
            diffs,
            next,
            files: files.len(),
        })
    }

    /// The patches of tracked `paths` on `side` from one `git diff`, by path. A path whose part
    /// is missing or cut short is left out, for a command of its own.
    fn batch_patches(
        &self,
        paths: &[&str],
        side: Side,
        whole_file: bool,
        cancel: &AtomicBool,
    ) -> Result<HashMap<String, String>> {
        let context = self.context_arg(whole_file);
        let mut args: Vec<String> = [
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            "--full-index",
            "--src-prefix=a/",
            "--dst-prefix=b/",
            &context,
        ]
        .map(str::to_owned)
        .to_vec();
        if side == Side::Staged {
            args.push("--cached".into());
        }
        args.push("--".into());
        args.extend(paths.iter().map(|p| literal(p)));
        let out = self.spawn_cancellable_within(&GitCommand::new(args), MAX_PATCH_BYTES, cancel)?;
        // A failed batch leaves every file to its own command, which says what went wrong.
        if !out.success && !out.cut {
            return Ok(HashMap::new());
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut parts = split_patches(&text);
        // The last part of a cut patch stops short.
        if out.cut {
            parts.pop();
        }
        let wanted: std::collections::HashSet<&str> = paths.iter().copied().collect();
        Ok(parts
            .into_iter()
            .filter(|(path, _)| wanted.contains(path.as_str()))
            .map(|(path, patch)| (path, patch.to_owned()))
            .collect())
    }

    /// The patches of new files, as `git diff --no-index /dev/null <file>` prints them, made from
    /// the files themselves. Files git would show another way (a symlink, an attribute that
    /// turns off or changes their diff, one that can't be read) are left out, for git itself.
    fn untracked_patches(&self, files: &[&FileChange], cancel: &AtomicBool) -> Result<HashMap<String, String>> {
        let mut args = vec![
            "check-attr".to_owned(),
            "-z".to_owned(),
            "diff".to_owned(),
            "--".to_owned(),
        ];
        args.extend(files.iter().map(|f| f.path.clone()));
        let out = match self.run_cancellable(&GitCommand::new(args), cancel) {
            Ok(out) => out,
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(_) => return Ok(HashMap::new()),
        };
        // `<path>\0diff\0<value>\0` for each path.
        let fields: Vec<&str> = out.stdout.split('\0').collect();
        let plain: std::collections::HashSet<&str> = fields
            .chunks(3)
            .filter(|f| f.len() == 3 && matches!(f[2], "unspecified" | "set"))
            .map(|f| f[0])
            .collect();
        let mut patches = HashMap::new();
        for file in files.iter().filter(|f| plain.contains(f.path.as_str())) {
            if cancel.load(Ordering::Relaxed) {
                return Err(Error::Cancelled);
            }
            let path = self.workdir().join(&file.path);
            let Some(meta) = std::fs::symlink_metadata(&path).ok().filter(|m| m.is_file()) else {
                continue;
            };
            if let Some(data) = crate::lfs::read_regular(&path, MAX_COUNTED_BYTES) {
                patches.insert(file.path.clone(), untracked_patch(&file.path, &data, executable(&meta)));
            }
        }
        Ok(patches)
    }

    /// The `-U` option for a compact or whole-file diff.
    fn context_arg(&self, whole_file: bool) -> String {
        if whole_file {
            "-U100000000".to_owned()
        } else {
            format!("-U{}", self.diff_options().context_lines)
        }
    }

    /// Whether `file`'s lines are not shown, so its patch is never read: past the line limit, or
    /// a new file too big to have its lines counted.
    fn needs_no_patch(&self, file: &FileChange, max_lines: u32) -> bool {
        let changed = (file.additions + file.deletions) as usize;
        let uncounted = file.status == FileStatus::Untracked
            && std::fs::symlink_metadata(self.workdir().join(&file.path))
                .is_ok_and(|m| m.is_file() && m.len() > MAX_COUNTED_BYTES);
        !file.binary && file.lfs.is_none() && (over_limit(file, max_lines) || changed > MAX_DIFF_LINES || uncounted)
    }

    /// The diff of `file`, one entry of the working tree, on `side`.
    fn diff_of(&self, file: FileChange, side: Side, whole_file: bool, cancel: &AtomicBool) -> Result<FileDiff> {
        if let Some(sub) = file.submodule {
            return Ok(self.submodule_diff(file, sub, side));
        }
        // The counts of `git diff --numstat` say beforehand when no lines will be shown: the
        // patch is then never read, only the ids of the two sides. An untracked file too big to
        // have its lines counted is all added lines.
        let max_lines = self.diff_options().max_lines;
        if self.needs_no_patch(&file, max_lines) {
            let ids = self.blob_ids(&file, side).unwrap_or_default();
            let blob = |id: &Option<String>| id.clone().filter(|id| id.bytes().any(|b| b != b'0'));
            let old = blob(&ids.0).map(|id| Source::Blob { id });
            let new = match side {
                Side::Staged => blob(&ids.1).map(|id| Source::Blob { id }),
                Side::Unstaged => (file.status != FileStatus::Deleted).then(|| Source::Worktree {
                    path: file.path.clone(),
                }),
            };
            let limited = over_limit(&file, max_lines);
            return Ok(FileDiff {
                file,
                hunks: Vec::new(),
                too_large: !limited,
                limited,
                old,
                new,
                submodule: None,
            });
        }
        let (patch, cut) = self.raw_diff_within(&file, side, whole_file, MAX_PATCH_BYTES, cancel)?;
        Ok(self.diff_from_patch(file, side, &patch, cut))
    }

    /// The diff of `file` on `side` from its `patch`, which was `cut` short when too long.
    fn diff_from_patch(&self, file: FileChange, side: Side, patch: &str, cut: bool) -> FileDiff {
        let max_lines = self.diff_options().max_lines;
        // Past this many lines nothing is shown, so the rest of the patch is not turned into lines.
        let mut parsed = parse_patch_within(patch, MAX_DIFF_LINES);
        parsed.cut |= cut;
        let mut hunks: Vec<Hunk> = parsed.hunks.into_iter().map(|h| h.hunk).collect();
        // Lines the secrets check would stop a commit on get a mark before they are staged.
        if crate::secrets::check_secrets() && !parsed.cut {
            for line in hunks.iter_mut().flat_map(|h| &mut h.lines) {
                if line.kind == LineKind::Added {
                    line.secret = crate::secrets::check_line(&line.text).map(|(kind, _)| kind);
                }
            }
        }
        let mut file = FileChange {
            binary: file.binary || parsed.binary,
            mode: parsed.mode,
            ..file
        };
        // A patch cut short keeps the counts of `git diff --numstat`.
        if !file.binary && !parsed.cut {
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
            file.old_size = old.as_ref().and_then(|s| self.source_size(s));
            file.new_size = new.as_ref().and_then(|s| self.source_size(s));
        }
        let limited = !parsed.cut && over_limit(&file, max_lines);
        let too_large = parsed.cut || (!limited && hunks.iter().map(|h| h.lines.len()).sum::<usize>() > MAX_DIFF_LINES);
        FileDiff {
            file,
            hunks: if too_large || limited { Vec::new() } else { hunks },
            too_large,
            limited,
            old,
            new,
            submodule: None,
        }
    }

    /// A submodule's entry on one side of the working copy: the commit the index records against
    /// `HEAD`'s (staged), or the one checked out in its folder against the index's (unstaged).
    fn submodule_diff(&self, file: FileChange, sub: SubmoduleChange, side: Side) -> FileDiff {
        let id = |spec: String| {
            self.run(&GitCommand::new(["rev-parse", "--verify", "-q", &spec]))
                .ok()
                .map(|out| out.stdout.trim().to_owned())
                .filter(|id| !id.is_empty())
        };
        let path = &file.path;
        let (old, new, changed) = match side {
            Side::Staged => (id(format!("HEAD:{path}")), id(format!(":{path}")), 0),
            Side::Unstaged => {
                let folder = self.workdir().join(path);
                let current =
                    crate::cli::run_in(&folder, &GitCommand::new(["rev-parse", "--verify", "-q", "HEAD"]), None)
                        .ok()
                        .map(|out| out.stdout.trim().to_owned())
                        .filter(|id| !id.is_empty());
                let changed = if sub.changed {
                    crate::worktrees::changed_files(&folder)
                } else {
                    0
                };
                (id(format!(":{path}")), current, changed)
            }
        };
        let moved = self.submodule_move(path, old.as_deref(), new.as_deref(), changed);
        FileDiff {
            file,
            hunks: Vec::new(),
            too_large: false,
            limited: false,
            old: None,
            new: None,
            submodule: Some(moved),
        }
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
        let mut steps = Vec::new();
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
                ..
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
                submodules,
                mode,
            } => {
                let mut plan = Vec::new();
                if *fetch_first {
                    plan.push(self.fetch_command(Some(remote)).with_progress());
                }
                plan.push(self.pull_command(remote, branch, *mode));
                if *submodules {
                    plan.extend(self.submodules_after_pull(remote, branch));
                }
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
                submodules,
            } => {
                let mut args = vec!["push".to_owned()];
                let mut notes = Vec::new();
                if *submodules {
                    args.push("--recurse-submodules=on-demand".to_owned());
                    notes.push("--recurse-submodules=on-demand: push the submodule commits it records first".to_owned());
                }
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
                    self.pull_command(remote, upstream, None),
                    GitCommand::new(push)
                        .comment("then send your commits on top")
                        .with_progress(),
                ]
            }
            Action::ReplayAndPush {
                remote,
                branch,
                upstream,
                base,
                lease,
                no_verify,
            } => self.plan_replay_and_push(remote, branch, upstream, base, lease, *no_verify)?,
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
            Action::DeleteRemoteBranch {
                remote,
                branch,
                expect: None,
            } => vec![
                GitCommand::new(["push", remote, "--delete", branch])
                    .comment("--delete: remove the branch on the remote")
                    .with_progress(),
            ],
            Action::DeleteRemoteBranch {
                remote,
                branch,
                expect: Some(sha),
            } => vec![
                GitCommand::new([
                    "push".to_owned(),
                    format!("--force-with-lease=refs/heads/{branch}:{sha}"),
                    remote.clone(),
                    "--delete".to_owned(),
                    branch.clone(),
                ])
                .comment(format!(
                    "--force-with-lease: only while {branch} there is still the commit that was merged, so newer pushes stay"
                ))
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
            Action::Unshallow { remote } => vec![
                GitCommand::new(["fetch", "--unshallow", remote.as_str()])
                    .comment("--unshallow: fetch the commits a shallow clone left out")
                    .with_progress(),
            ],
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
            Action::PullRequestMerged { remote, base, .. } => self.plan_pull_request_merged(remote, base)?,
            Action::DeleteMergedBranch { base, branch, sha } => self.plan_delete_merged_branch(base, branch, sha)?,
            Action::LfsTrack { pattern, paths, stage } => lfs::plan_track(pattern, paths, *stage),
            Action::LfsUntrack { pattern } => lfs::plan_untrack(pattern),
            Action::LfsPull { include } => lfs::plan_pull(include.as_deref()),
            Action::LfsPrune => lfs::plan_prune(),
            Action::LfsInstall { brew, pull } => lfs::plan_install(brew.as_deref(), *pull),
            Action::BisectStart {
                good,
                bad,
                stash,
                command,
            } => self.plan_bisect_start(good, bad, *stash, command.as_deref()),
            Action::BisectMark { mark, commit } => self.plan_bisect_mark(*mark, commit.as_deref())?,
            Action::BisectReset => self.plan_bisect_reset()?,
            Action::WorktreeAdd {
                path,
                branch,
                create,
                start,
            } => self.plan_worktree_add(path, branch, *create, start.as_deref()),
            Action::WorktreeRemove { path, force, stash } => self.plan_worktree_remove(path, *force, *stash)?,
            Action::WorktreeLock { path, reason } => self.plan_worktree_lock(path, reason.as_deref()),
            Action::WorktreeUnlock { path } => self.plan_worktree_unlock(path),
            Action::WorktreePrune => self.plan_worktree_prune(),
            Action::WorktreeRepair { path } => self.plan_worktree_repair(path),
            Action::SubmoduleUpdate { paths, latest } => self.plan_submodule_update(paths, *latest),
            Action::SubmoduleSync { paths } => self.plan_submodule_sync(paths),
            Action::SubmoduleAdd { url, path, branch } => self.plan_submodule_add(url, path, branch.as_deref()),
            Action::SubmoduleRemove { path } => self.plan_submodule_remove(path),
            Action::Fix { fix, .. } => {
                let plan = self.plan_fix(fix)?;
                steps = plan.steps;
                plan.commands
            }
            Action::Reword { message, no_verify } => {
                let mut args = vec!["commit".to_owned(), "--amend".to_owned(), "--only".to_owned()];
                // The same hooks rule as Commit: off in Settings, or skipped this once.
                let skip = *no_verify || !config::run_hooks();
                if skip {
                    args.push("--no-verify".to_owned());
                }
                for paragraph in paragraphs(message) {
                    args.push("-m".to_owned());
                    args.push(paragraph);
                }
                let cmd = GitCommand::new(args);
                vec![if *no_verify {
                    cmd.comment("--only: just the message, staged changes stay staged; --no-verify: skip the pre-commit and commit-msg hooks this once")
                } else if skip {
                    cmd.comment(format!("--only: just the message; staged changes stay staged. {HOOKS_OFF}"))
                } else {
                    cmd.comment("--only: just the message; staged changes stay staged")
                }]
            }
        };
        Ok(Plan {
            commands,
            not_kept: Vec::new(),
            steps,
        })
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
        // A fix planned for another state never starts: the sheet shows the new plan.
        if let Action::Fix { expect, .. } = action {
            self.check_fix(expect)?;
        }
        // A commit the secrets check stops never starts: nothing runs, nothing to undo.
        if let Action::Commit {
            allow_secrets: false, ..
        } = action
            && crate::secrets::check_secrets()
        {
            let found = crate::console::role("secrets check", || self.staged_secrets())?;
            if !found.is_empty() {
                return Err(Error::Secrets(found));
            }
        }
        // Loading a key changes nothing in the repository. A worktree is a folder: undoing the
        // branch a new one made would pull it from under that folder, and a removed one's files
        // were never in a snapshot. A submodule's folder is another repository, also in no
        // snapshot.
        if let Action::AddSshKey { .. }
        | Action::WorktreeAdd { .. }
        | Action::WorktreeRemove { .. }
        | Action::WorktreeLock { .. }
        | Action::WorktreeUnlock { .. }
        | Action::WorktreePrune
        | Action::WorktreeRepair { .. }
        | Action::SubmoduleUpdate { .. }
        | Action::SubmoduleSync { .. }
        | Action::SubmoduleAdd { .. }
        | Action::SubmoduleRemove { .. } = action
        {
            return self.perform_unlogged(action, on_event, cancel);
        }
        // A bisect step only checks out a commit to test, and undoing one would not take back
        // git's notes on the answers; the stash made before it stays in the list until Finish.
        if let Action::BisectStart { .. } | Action::BisectMark { .. } | Action::BisectReset = action {
            let result = self.perform_unlogged(action, on_event, cancel);
            self.forget_bisect_stash();
            return result;
        }
        if let Action::ClearOperationLog = action {
            let out = self.perform_unlogged(action, on_event, cancel)?;
            self.clear_operation_log()?;
            return Ok(out);
        }
        if let Action::Restore { id, .. } = action {
            self.prepare_restore(id)?;
        }
        // Copy and Push writes down what it copies first: a copy resolved by hand after a
        // conflict has other changes, and must not be copied again.
        if let Action::ReplayAndPush {
            branch, base, lease, ..
        } = action
        {
            let commits = self.missing_commits(&format!("refs/heads/{branch}"), base, lease)?;
            if !commits.is_empty() {
                self.record_replay(branch, &commits)?;
            }
        }
        // A pick of Copy and Push finished by hand: which commit it became. Aborted: forgotten.
        let replaying = match action {
            Action::Continue { .. } | Action::Abort => self.replay_in_progress(),
            _ => None,
        };
        // Steps of a fix noted for a pick or revert that is over by now, finished elsewhere.
        self.forget_fix_rest();
        let before = crate::console::role(UNDO_SNAPSHOT, || self.snapshot().ok());
        if let Action::EditStack { plan } = action {
            self.remember_return(plan);
        }
        let result = self.perform_unlogged(action, on_event, cancel);
        self.forget_return();
        if let Action::Continue { .. } | Action::Abort | Action::Skip = action {
            self.forget_fix_rest();
        }
        if let Some((branch, source, tip)) = &replaying {
            match action {
                // Continue goes on with the picks left and can stop on the next one's conflict
                // after it made this one's copy: the copy is noted either way. Nothing is noted
                // when it stopped before making a commit.
                Action::Continue { .. } => {
                    let noted = self.note_replay_copy(branch, source, tip);
                    if result.is_ok() {
                        noted?;
                    }
                }
                _ if result.is_ok() => self.forget_replay(branch)?,
                _ => {}
            }
        }
        if let Action::Restore { .. } = action {
            // read-tree forgets what git knew about the files on disk.
            let _ = self.run(&GitCommand::new(["update-index", "-q", "--refresh"]));
        }
        crate::console::role(UNDO_SNAPSHOT, || {
            if let Some(before) = before
                && let Ok(after) = self.snapshot()
            {
                let _ = self.record(action, &before, &after, result.is_err());
            }
        });
        result
    }

    fn perform_unlogged(
        &self,
        action: &Action,
        on_event: &mut dyn FnMut(ActionEvent),
        cancel: &AtomicBool,
    ) -> Result<String> {
        if let Action::Commit { message, .. } | Action::Reword { message, .. } = action
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
                let mut updated_submodules = false;
                let stash_before = match action {
                    Action::BisectStart { stash: true, .. } => Some(self.stash_tip()),
                    _ => None,
                };
                // A fix, or a Continue of one, that stops on conflicts notes the steps left.
                let fix_start = match action {
                    Action::Fix { .. } => Some(self.head()?.branch),
                    Action::Continue { .. } | Action::Skip if self.has_fix_rest() => Some(None),
                    _ => None,
                };
                let commands = self.plan(action)?.commands;
                for (at, command) in commands.iter().enumerate() {
                    if let Some((ancestor, of)) = &command.only_if_ancestor
                        && !self.is_ancestor(ancestor, of)
                    {
                        continue;
                    }
                    if let Some((name, was)) = &command.only_if_moved
                        && self.rev(name) == *was
                    {
                        continue;
                    }
                    if command.secrets_check {
                        let found = crate::console::role("secrets check", || match action {
                            Action::Fix {
                                fix:
                                    crate::fix::Fix::AddToLast {
                                        allow_secrets: true, ..
                                    },
                                ..
                            } => Ok(Vec::new()),
                            _ if !crate::secrets::check_secrets() => Ok(Vec::new()),
                            // Only the picked files go into the commit: other staged lines don't.
                            Action::Fix {
                                fix: crate::fix::Fix::AddToLast { paths, .. },
                                ..
                            } if !paths.is_empty() => self.staged_secrets_in(&self.amend_paths(paths)?),
                            _ => self.staged_secrets(),
                        })?;
                        if !found.is_empty() {
                            return Err(Error::Secrets(found));
                        }
                    }
                    self.write_todo(command)?;
                    updated_submodules |= command.args.first().is_some_and(|a| a == "submodule");
                    let command = crate::ssh::interactive(command.clone());
                    on_event(ActionEvent::Command {
                        display: command.display(),
                    });
                    let ran = crate::console::main(|| {
                        self.run_streaming(&command, &mut |line| on_event(ActionEvent::Line(line)), cancel)
                    });
                    // The files a fix put aside whose staged changes no longer fit: git changed
                    // nothing and kept the stash. Say so rather than bring them back unstaged.
                    if ran.is_err()
                        && command.args == ["stash", "pop", "--index"]
                        && (fix_start.is_some() || matches!(action, Action::Abort))
                        && self.conflicted_paths().is_ok_and(|p| p.is_empty())
                    {
                        return Err(Error::StashKept);
                    }
                    let out = ran.inspect_err(|_| {
                        if let Some(start) = &fix_start {
                            self.note_fix_rest(action, start.clone(), &commands, at);
                        }
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
                    if let Some(before) = &stash_before
                        && command.args.first().is_some_and(|a| a == "stash")
                    {
                        self.note_bisect_stash(before.as_deref());
                    }
                    last = if out.stdout.trim().is_empty() {
                        out.stderr
                    } else {
                        out.stdout
                    };
                }
                // The pull brought the first submodules, which the last fetch didn't know of.
                if let Action::Pull { submodules: true, .. } = action
                    && !updated_submodules
                    && !self.submodules().is_empty()
                {
                    let command = crate::ssh::interactive(crate::submodules::submodule_update());
                    on_event(ActionEvent::Command {
                        display: command.display(),
                    });
                    crate::console::main(|| {
                        self.run_streaming(&command, &mut |line| on_event(ActionEvent::Line(line)), cancel)
                    })?;
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
        let out = crate::console::main(|| self.run_with_input(command, Some(patch.as_bytes())))?;
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
        let (patch, _) = self.raw_diff_within(file, side, whole_file, usize::MAX, &AtomicBool::new(false))?;
        Ok(patch)
    }

    /// [`Repo::raw_diff`], reading at most `max` bytes of the patch: whether it was cut there
    /// comes with it. A whole-file patch of a huge file is never read further than shown.
    fn raw_diff_within(
        &self,
        file: &FileChange,
        side: Side,
        whole_file: bool,
        max: usize,
        cancel: &AtomicBool,
    ) -> Result<(String, bool)> {
        let context = self.context_arg(whole_file);
        let mut args = vec![
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            "--full-index",
            &context,
        ];
        let path = literal(&file.path);
        let untracked = file.status == FileStatus::Untracked;
        if untracked {
            args.extend(["--no-index", "--", "/dev/null", &file.path]);
        } else {
            if side == Side::Staged {
                args.push("--cached");
            }
            args.extend(["--", &path]);
        }
        let command = GitCommand::new(args);
        let out = self.spawn_cancellable_within(&command, max, cancel)?;
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        // `--no-index` exits with 1 when the files differ, which they always do here.
        if !out.success && !out.cut && !untracked {
            let (stdout, stderr) = (text(&out.stdout), text(&out.stderr));
            return Err(Error::Command {
                command: command.display(),
                code: out.code,
                output: if stderr.trim().is_empty() { stdout } else { stderr },
            });
        }
        Ok((text(&out.stdout), out.cut))
    }

    /// The blob ids of both sides of `file`'s change on `side`, as the `index` line of its patch
    /// gives them (all zeros for a side that has none), without reading the patch.
    fn blob_ids(&self, file: &FileChange, side: Side) -> Result<(Option<String>, Option<String>)> {
        if file.status == FileStatus::Untracked {
            return Ok((None, None));
        }
        let mut args = vec!["diff", "--raw", "-z", "--no-renames", "--no-abbrev"];
        if side == Side::Staged {
            args.push("--cached");
        }
        let path = literal(&file.path);
        args.extend(["--", &path]);
        let out = self.run(&GitCommand::new(args))?;
        // `:<old mode> <new mode> <old id> <new id> <status>\0<path>\0`
        let mut fields = out.stdout.split('\0').next().unwrap_or_default().split(' ').skip(2);
        Ok((fields.next().map(str::to_owned), fields.next().map(str::to_owned)))
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    fn plan_replay_and_push(
        &self,
        remote: &str,
        branch: &str,
        upstream: &str,
        base: &str,
        lease: &str,
        no_verify: bool,
    ) -> Result<Vec<GitCommand>> {
        if !self.is_ancestor(base, lease) || self.has_merges(&format!("{base}..{lease}"))? {
            return Err(Error::Git(format!(
                "{remote}/{upstream} was rewritten or merged into since your last push; push with force or sort it out by hand"
            )));
        }
        let mut commands = Vec::new();
        let commits = self.missing_commits(&format!("refs/heads/{branch}"), base, lease)?;
        if !commits.is_empty() {
            let mut people: Vec<&str> = Vec::new();
            for c in &commits {
                if !people.contains(&c.author_name.as_str()) {
                    people.push(&c.author_name);
                }
            }
            let mut args = vec!["cherry-pick".to_owned()];
            args.extend(commits.iter().map(|c| c.id.clone()));
            commands.push(GitCommand::new(args).comment(format!(
                "copy {} by {} onto your rewritten {branch}; they keep their author",
                if commits.len() == 1 {
                    "the commit".to_owned()
                } else {
                    format!("the {} commits", commits.len())
                },
                people.join(", ")
            )));
        }
        let mut push = vec!["push".to_owned()];
        if no_verify || !config::run_hooks() {
            push.push("--no-verify".to_owned());
        }
        push.push(format!("--force-with-lease={upstream}:{lease}"));
        push.extend([remote.to_owned(), refspec(branch, upstream)]);
        commands.push(
            GitCommand::new(push)
                .comment(format!(
                    "--force-with-lease: replace the old versions only if {remote}/{upstream} is still {}",
                    &lease[..lease.len().min(7)]
                ))
                .with_progress(),
        );
        Ok(commands)
    }

    /// `mode` is the one picked for this pull; without it, the repository's setting.
    fn pull_command(&self, remote: &str, branch: &str, mode: Option<PullMode>) -> GitCommand {
        let (mode, note) = match mode.unwrap_or_else(|| self.pull_mode()) {
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
                // `S<c><m><u>` for a submodule: its commit changed, tracked or untracked files in it.
                let sub: Vec<char> = fields.next().unwrap_or("N...").chars().collect();
                let gitlink = sub.first() == Some(&'S');
                if let Some(status) = status_of(xy[0]) {
                    let mut file = change(path, old_path, status);
                    if gitlink {
                        file.submodule = Some(SubmoduleChange {
                            moved: true,
                            changed: false,
                        });
                    }
                    tree.staged.push(file);
                }
                if let Some(status) = status_of(xy[1]) {
                    let mut file = change(path, None, status);
                    if gitlink {
                        file.submodule = Some(SubmoduleChange {
                            moved: sub.get(1) == Some(&'C'),
                            changed: sub.get(2) == Some(&'M') || sub.get(3) == Some(&'U'),
                        });
                    }
                    tree.unstaged.push(file);
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

/// Split the output of `git diff` (with `a/` and `b/` prefixes and no renames) into each file's
/// part, by path. A part whose path can't be read from its `diff --git` line is left out.
fn split_patches(text: &str) -> Vec<(String, &str)> {
    let mut starts: Vec<usize> = Vec::new();
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("diff --git ") {
            starts.push(at);
        }
        at += line.len();
    }
    starts.push(text.len());
    starts
        .windows(2)
        .filter_map(|w| {
            let part = &text[w[0]..w[1]];
            let first = part.lines().next()?;
            Some((patch_path(first.strip_prefix("diff --git ")?)?, part))
        })
        .collect()
}

/// The path of a `diff --git a/<path> b/<path>` line without renames: both halves are the same
/// path, quoted by git when it has unusual characters.
fn patch_path(rest: &str) -> Option<String> {
    if rest.starts_with('"') {
        let (old, tail) = unquote(rest)?;
        let new = tail.strip_prefix(' ')?;
        let new = if new.starts_with('"') {
            unquote(new)?.0
        } else {
            new.to_owned()
        };
        let path = old.strip_prefix("a/")?;
        return (new.strip_prefix("b/")? == path).then(|| path.to_owned());
    }
    // `a/` + path + ` b/` + path.
    let len = rest.len().checked_sub(5)? / 2;
    let path = rest.get(2..2 + len)?;
    (rest.get(..2)? == "a/" && rest.get(2 + len..5 + len)? == " b/" && rest.get(5 + len..)? == path)
        .then(|| path.to_owned())
}

/// A path git quoted C-style (`"dir/\303\244.txt"`), and what follows it.
fn unquote(text: &str) -> Option<(String, &str)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 1;
    loop {
        match *bytes.get(i)? {
            b'"' => return Some((String::from_utf8_lossy(&out).into_owned(), &text[i + 1..])),
            b'\\' => {
                let c = *bytes.get(i + 1)?;
                if c.is_ascii_digit() {
                    let digits = text.get(i + 1..i + 4)?;
                    out.push(u8::from_str_radix(digits, 8).ok()?);
                    i += 4;
                    continue;
                }
                out.push(match c {
                    b'a' => 7,
                    b'b' => 8,
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'v' => 11,
                    b'f' => 12,
                    b'r' => b'\r',
                    other => other,
                });
                i += 2;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
}

/// Whether a file has its executable bit, which git records as mode 100755.
#[cfg(unix)]
fn executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_meta: &std::fs::Metadata) -> bool {
    false
}

/// The patch `git diff --no-index /dev/null <path>` prints for a new file holding `data`: every
/// line added, or a note that the file is binary.
fn untracked_patch(path: &str, data: &[u8], executable: bool) -> String {
    let mode = if executable { "100755" } else { "100644" };
    let mut patch = format!("diff --git a/{path} b/{path}\nnew file mode {mode}\n");
    if data.iter().take(8000).any(|&b| b == 0) {
        patch.push_str(&format!("Binary files /dev/null and b/{path} differ\n"));
        return patch;
    }
    if data.is_empty() {
        return patch;
    }
    let text = String::from_utf8_lossy(data);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let range = if lines.len() == 1 {
        "+1".to_owned()
    } else {
        format!("+1,{}", lines.len())
    };
    patch.push_str(&format!("--- /dev/null\n+++ b/{path}\n@@ -0,0 {range} @@\n"));
    for line in &lines {
        patch.push('+');
        patch.push_str(line);
    }
    if !text.ends_with('\n') {
        patch.push_str("\n\\ No newline at end of file\n");
    }
    patch
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
    /// The hunks stop short: the patch had more lines than were asked for.
    cut: bool,
}

/// Parse the output of `git diff` for a single file.
fn parse_patch(patch: &str) -> ParsedPatch {
    parse_patch_within(patch, usize::MAX)
}

/// [`parse_patch`], stopping once the hunks have more than `max` lines: the patch is then `cut`
/// and its hunks are only good for telling that it was too long.
fn parse_patch_within(patch: &str, max: usize) -> ParsedPatch {
    let mut file_header = String::new();
    let mut lines = 0usize;
    let mut cut = false;
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
        if lines >= max {
            cut = true;
            break;
        }
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
        lines += 1;
        current.hunk.lines.push(DiffLine {
            kind,
            old_line,
            new_line,
            text,
            cr,
            words: None,
            secret: None,
        });
    }
    // Changed words and the fingerprint that staging checks are for hunks that are shown.
    if !cut {
        for hunk in &mut hunks {
            mark_words(&mut hunk.hunk.lines);
            hunk.hunk.check = fingerprint(&hunk.text);
        }
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
        cut,
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
    fn a_patch_past_the_budget_is_cut() {
        let patch = "diff --git a/f b/f\nindex 1111111..2222222 100644\n--- a/f\n+++ b/f\n\
                     @@ -1,2 +1,3 @@\n a\n-b\n+c\n+d\n";
        let whole = parse_patch_within(patch, 10);
        assert!(!whole.cut);
        assert_eq!(whole.hunks[0].hunk.lines.len(), 4);
        assert!(!whole.hunks[0].hunk.check.is_empty());
        let cut = parse_patch_within(patch, 2);
        assert!(cut.cut);
        assert_eq!(cut.hunks[0].hunk.lines.len(), 2);
    }

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
