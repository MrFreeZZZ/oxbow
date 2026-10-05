//! UI-agnostic Git core for Oxbow.
//!
//! Reads (history, graph, refs, diffs) go through gitoxide. Commands that change a repository
//! go through the system `git` (see [`cli`]) so hooks, credentials and SSH behave exactly like on
//! the command line.

pub mod cli;
pub mod commit;
pub mod error;
pub mod graph;
pub mod history;
pub mod remote;
pub mod repo;
pub mod worktree;

pub use cli::{CommandOutput, GitCommand, OutputLine};
pub use commit::{
    CommitDetail, DiffContext, DiffLine, FileChange, FileDiff, FileStatus, Hunk, LineKind, Person, WordPart,
};
pub use error::{Error, Result};
pub use history::{History, HistoryOptions, HistoryRow, Label, WORKTREE_ID, WorktreeSummary};
pub use remote::{CommitBrief, Failure, FailureKind, Tracking, classify_failure};
pub use repo::{HeadInfo, RefInfo, RefKind, Repo, StashInfo};
pub use worktree::{Action, ActionEvent, Plan, Side, WorkingTree};
