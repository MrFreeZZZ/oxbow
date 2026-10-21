//! UI-agnostic Git core for Oxbow.
//!
//! Reads (history, graph, refs, diffs) go through gitoxide. Commands that change a repository
//! go through the system `git` (see [`cli`]) so hooks, credentials and SSH behave exactly like on
//! the command line.

pub mod bisect;
pub mod cli;
pub mod commit;
pub mod compare;
pub mod config;
pub mod console;
pub mod edit;
pub mod error;
pub mod fetched;
pub mod filehistory;
pub mod fix;
pub mod folders;
pub mod github;
pub mod graph;
pub mod history;
pub mod lfs;
pub mod notkept;
pub mod operation;
pub mod oplog;
pub mod pulls;
pub mod reflog;
pub mod remote;
pub mod repo;
pub mod search;
pub mod secrets;
pub mod series;
pub mod setup;
pub mod ssh;
pub mod stack;
pub mod stash;
pub mod submodules;
pub mod tags;
pub mod worktree;
pub mod worktrees;

pub use bisect::{Bisect, BisectMark};
pub use cli::{CommandOutput, GitCommand, OutputLine};
pub use commit::{
    CommitDetail, DiffContext, DiffLine, DiffPage, EolChange, FileChange, FileDiff, FileStatus, Hunk, LineKind,
    ModeChange, Person, Source, SourceLine, SourceLines, SubmoduleChange, WordPart,
};
pub use compare::{CompareCommit, CompareFile, CompareMode, Comparison};
pub use config::{ConfigScope, GitInfo, RemoteInfo, SshKey, SshSource, Storage};
pub use edit::ResetMode;
pub use error::{Error, Result};
pub use fetched::RemoteFetch;
pub use filehistory::{Blame, BlameCommit, BlameLine, FileCommit, FileHistory};
pub use fix::{Fix, FixExpect, FixFile, FixState, FixStep, StepKind};
pub use folders::{FolderCache, FolderState};
pub use history::{
    Focus, History, HistoryCache, HistoryOptions, HistoryRow, Label, PAGE, Piece, ReadPart, ReadProblem, WORKTREE_ID,
    WorktreeSummary,
};
pub use lfs::{LfsChange, LfsPattern, LfsPointer, LfsStatus};
pub use notkept::{NotKept, PushedCommit, SizedFile};
pub use operation::{
    Chunk, ConflictFile, ConflictSide, MergeMethod, MergePreview, MergeTool, Operation, OperationKind, Pick, Renamed,
    Resolution,
};
pub use oplog::{HeadState, OpEntry, RefMove, SnapshotComparison, StashMove, StashRecord, Trees};
pub use reflog::ReflogEntry;
pub use remote::{CommitBrief, DeletionCheck, Failure, FailureKind, Replay, Tracking, classify_failure};
pub use repo::{DiffOptions, HeadInfo, RefInfo, RefKind, Repo, StashInfo};
pub use search::{SearchHit, SearchMode, SearchQuery, SearchResult};
pub use secrets::{PatternKind, SecretFinding, SecretPattern, set_secrets_check};
pub use series::{
    Inner, InterLine, InterSection, Outer, PairKind, Rewrite, RewriteSource, SectionKind, Series, SeriesCommit,
    SeriesPair,
};
pub use setup::{CloneOptions, NewRepoOptions, NewRepoPlan, RemoteProbe, RepoGlance};
pub use ssh::{SshCheck, SshProblem, SshStep};
pub use stack::{
    BranchAfter, Landing, Stack, StackBranch, StackCommit, StackConflict, StackPlan, StackPreview, StackStep,
    StepAction,
};
pub use stash::StashCheck;
pub use submodules::{Checkout, Moved, Submodule, SubmoduleMove};
pub use worktree::{Action, ActionEvent, BranchPush, Plan, PullMode, RemoteBranch, Side, WorkingTree};
pub use worktrees::Worktree;
