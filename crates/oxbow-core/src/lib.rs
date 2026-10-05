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
pub mod repo;

pub use cli::{CommandOutput, GitCommand};
pub use commit::{
    CommitDetail, DiffContext, DiffLine, FileChange, FileDiff, FileStatus, Hunk, LineKind, Person, WordPart,
};
pub use error::{Error, Result};
pub use history::{History, HistoryOptions, HistoryRow, Label};
pub use repo::{HeadInfo, RefInfo, RefKind, Repo, StashInfo};
