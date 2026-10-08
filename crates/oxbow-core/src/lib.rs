//! UI-agnostic Git core for Oxbow.
//!
//! Reads (history, graph, refs, diffs) go through gitoxide. Commands that change a repository
//! go through the system `git` (see [`cli`]) so hooks, credentials and SSH behave exactly like on
//! the command line.

pub mod cli;
pub mod commit;
pub mod compare;
pub mod config;
pub mod edit;
pub mod error;
pub mod filehistory;
pub mod graph;
pub mod history;
pub mod operation;
pub mod oplog;
pub mod remote;
pub mod repo;
pub mod search;
pub mod setup;
pub mod stack;
pub mod stash;
pub mod tags;
pub mod worktree;

pub use cli::{CommandOutput, GitCommand, OutputLine};
pub use commit::{
    CommitDetail, DiffContext, DiffLine, FileChange, FileDiff, FileStatus, Hunk, LineKind, Person, WordPart,
};
pub use compare::{CompareCommit, CompareFile, CompareMode, Comparison};
pub use config::{ConfigScope, GitInfo, RemoteInfo, SshKey, SshSource, Storage};
pub use edit::ResetMode;
pub use error::{Error, Result};
pub use filehistory::{Blame, BlameCommit, BlameLine, FileCommit, FileHistory};
pub use history::{History, HistoryOptions, HistoryRow, Label, WORKTREE_ID, WorktreeSummary};
pub use operation::{Chunk, ConflictFile, ConflictSide, MergeMethod, MergePreview, Operation, OperationKind, Pick};
pub use oplog::{HeadState, OpEntry, RefMove, StashMove, StashRecord, Trees};
pub use remote::{CommitBrief, DeletionCheck, Failure, FailureKind, Tracking, classify_failure};
pub use repo::{DiffOptions, HeadInfo, RefInfo, RefKind, Repo, StashInfo};
pub use search::{SearchHit, SearchMode, SearchQuery, SearchResult};
pub use setup::{CloneOptions, NewRepoOptions, NewRepoPlan, RemoteProbe, RepoGlance};
pub use stack::{
    BranchAfter, Stack, StackBranch, StackCommit, StackConflict, StackPlan, StackPreview, StackStep, StepAction,
};
pub use stash::StashCheck;
pub use worktree::{Action, ActionEvent, BranchPush, Plan, PullMode, RemoteBranch, Side, WorkingTree};
