//! UI-agnostic Git core for Oxbow.
//!
//! Reads (history, graph, refs, diffs) go through gitoxide. Commands that change a repository
//! go through the system `git` (see [`cli`]) so hooks, credentials and SSH behave exactly like on
//! the command line.

pub mod cli;
pub mod commit;
pub mod edit;
pub mod error;
pub mod graph;
pub mod history;
pub mod operation;
pub mod remote;
pub mod repo;
pub mod stash;
pub mod worktree;

pub use cli::{CommandOutput, GitCommand, OutputLine};
pub use commit::{
    CommitDetail, DiffContext, DiffLine, FileChange, FileDiff, FileStatus, Hunk, LineKind, Person, WordPart,
};
pub use edit::ResetMode;
pub use error::{Error, Result};
pub use history::{History, HistoryOptions, HistoryRow, Label, WORKTREE_ID, WorktreeSummary};
pub use operation::{Chunk, ConflictFile, ConflictSide, MergeMethod, MergePreview, Operation, OperationKind, Pick};
pub use remote::{CommitBrief, DeletionCheck, Failure, FailureKind, Tracking, classify_failure};
pub use repo::{DiffOptions, HeadInfo, RefInfo, RefKind, Repo, StashInfo};
pub use stash::StashCheck;
pub use worktree::{Action, ActionEvent, Plan, RemoteBranch, Side, WorkingTree};
