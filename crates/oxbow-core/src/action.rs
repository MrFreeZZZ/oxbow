//! What can be done to a repository. Every change is an [`Action`], which turns into the
//! [`Plan`] of git commands the confirmation sheet shows; [`ActionEvent`]s tell how it runs.

//! The working copy: what changed since the last commit, staging and committing.
//!
//! Status and diffs come from the `git` command line, so they honor every setting that affects
//! them (ignore rules, attributes, line endings, fsmonitor), and a hunk shown on screen is
//! exactly the hunk `git apply` gets when it is staged. Every change to the repository is an
//! [`Action`] that turns into the [`GitCommand`]s shown in the confirmation sheet.

use serde::{Deserialize, Serialize};

use crate::bisect::BisectMark;
use crate::cli::{GitCommand, OutputLine};
use crate::edit::ResetMode;
use crate::operation::{ConflictSide, MergeMethod, Resolution};

/// `LfsTrack` stages what it changes unless asked not to.
pub(crate) fn staged_by_default() -> bool {
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
        /// A tracked file or folder the rule covers: git stops tracking it, and it stays on disk.
        #[serde(default)]
        untrack: Option<String>,
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
        /// Commit without running the checks before commit, this one time.
        #[serde(default)]
        skip_checks: bool,
        /// End the message with a `Signed-off-by:` line of the committer (`--signoff`).
        #[serde(default)]
        sign_off: bool,
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
        /// Ask GitLab to open a merge request of the branch with the push.
        #[serde(default)]
        merge_request: Option<crate::gitlab::MergeRequest>,
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
    /// Write the chosen side of each conflict of `path` into the file and mark it resolved; with
    /// `text`, the whole file as the user wrote it instead.
    Resolve {
        path: String,
        picks: Vec<Resolution>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        text: Option<String>,
        /// The versions the choices were made for (`ConflictFile::stages`): refused when the
        /// index holds others by now.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        stages: Option<String>,
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
    /// Write commits as patch files (`git format-patch`): those of `to` that `since` doesn't
    /// have, or without `since` the newest `count` up to `to`. A numbered file each in the
    /// folder `into`, or with `one_file` all of them in the file `into`.
    SavePatches {
        to: String,
        #[serde(default)]
        since: Option<String>,
        count: usize,
        into: String,
        #[serde(default)]
        one_file: bool,
    },
    /// Apply the patch files at `paths`, in order: mailed patches become commits (`git am`),
    /// a plain diff goes into the files uncommitted (`git apply`).
    ApplyPatches {
        paths: Vec<String>,
    },
    /// Add a commit that undoes `commit`.
    Revert {
        commit: String,
    },
    /// Undo what `commit` changed in the file `path` (`old_path` before a rename), in the files on
    /// disk, uncommitted: its patch of that file, reversed.
    RevertFile {
        commit: String,
        path: String,
        #[serde(default)]
        old_path: Option<String>,
    },
    /// Bring what `commit` changed in the file `path` into the files on disk, uncommitted.
    CherryPickFile {
        commit: String,
        path: String,
        #[serde(default)]
        old_path: Option<String>,
    },
    /// Put the file `path` on disk as it is in `commit`; nothing is staged.
    TakeVersion {
        commit: String,
        path: String,
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
    /// of `paths`, or every change when it is empty. With `staged`, only what is staged
    /// (`--staged`, git 2.35+); unstaged changes stay.
    StashPush {
        message: Option<String>,
        untracked: bool,
        #[serde(default)]
        paths: Vec<String>,
        #[serde(default)]
        staged: bool,
    },
    /// Put the unstaged hunk of `path` that starts at `header` aside in a new stash, only its
    /// changed lines at `lines` when given, and take it out of the file.
    StashHunk {
        path: String,
        header: String,
        check: String,
        #[serde(default)]
        lines: Option<Vec<usize>>,
        message: Option<String>,
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
        /// Add them even though the secrets check finds something, this one time.
        #[serde(default)]
        allow_secrets: bool,
        /// Add them without running the checks before commit, this one time.
        #[serde(default)]
        skip_checks: bool,
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
    /// Download LFS files in place of their pointers: those at `paths`, else those `include`
    /// matches, else all.
    LfsPull {
        include: Option<String>,
        #[serde(default)]
        paths: Vec<String>,
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
    /// The action ran, but its step couldn't be written to the Operation Log, so Undo can't
    /// take it back; `reason` says why. The action's own result is reported as usual.
    NotLogged { reason: String },
    /// Another window of the repository is changing it: this waits until `title` there is done.
    /// `window` is that window's folder name.
    Waiting { title: String, window: String },
    /// The action goes over these one at a time, e.g. Update All over the submodules; said
    /// before the first starts. One that fails doesn't stop the others.
    Parts { names: Vec<String> },
    /// One of the [`ActionEvent::Parts`] started or ended. A failed one has git's `error`, and
    /// `inner` names the nested submodule that failed when it wasn't this one itself.
    Part {
        name: String,
        state: PartState,
        error: Option<String>,
        inner: Option<String>,
    },
    /// How much of one of the [`ActionEvent::Parts`] is done, in bytes, e.g. a file a download
    /// brings.
    PartProgress { name: String, done: u64, total: u64 },
}

/// Where one of an action's [`ActionEvent::Parts`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PartState {
    Started,
    Done,
    Failed,
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

/// How Pull brings in the remote's commits, from `pull.rebase` and `pull.ff`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PullMode {
    /// Oxbow's default when neither is set: your commits go on top, no merge commit.
    Rebase,
    Merge,
    FastForward,
}
