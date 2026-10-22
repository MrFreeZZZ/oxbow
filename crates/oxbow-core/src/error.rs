use std::fmt::Display;

/// Errors returned by `oxbow-core`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path} is not a Git repository")]
    NotARepository { path: String },
    /// The folder of the open repository isn't there any more: deleted, moved, or on a disk
    /// that isn't connected. [`crate::worktrees::gone_folder`] tells which.
    #[error("the folder {path} isn't there any more")]
    FolderGone { path: String },
    #[error("bare repositories are not supported yet")]
    Bare,
    #[error("no commit {0}")]
    UnknownCommit(String),
    #[error("{0}")]
    Git(String),
    #[error("could not run git: {0}")]
    GitNotFound(String),
    /// A `git` command exited with an error; `output` is what it printed.
    #[error("`{command}` failed: {output}")]
    Command {
        command: String,
        code: Option<i32>,
        output: String,
    },
    /// A GitHub API call failed; `message` says why in words for people.
    #[error("{message}")]
    GitHub { status: Option<u16>, message: String },
    #[error("stopped")]
    Cancelled,
    /// The repository is not how it was when the action was planned, so it did not run.
    #[error("{0}")]
    Changed(String),
    /// The secrets check stopped a commit: the staged lines that look like secrets.
    #[error("the staged changes look like they hold a password or key")]
    Secrets(Vec<crate::secrets::SecretFinding>),
    /// The steps ran, but the files put aside could not come back as they were: their staged
    /// changes no longer fit on the new commits. The stash keeps them.
    #[error(
        "the steps ran, but your uncommitted files are still in the stash: their staged changes no longer fit on top of the new commits. Apply the stash in Stashes to bring them back, unstaged"
    )]
    StashKept,
    /// The state before the action couldn't be saved for Undo, so it did not run: it can run
    /// again without Undo once that is chosen.
    #[error("Oxbow couldn't save the state for Undo, so nothing ran: {0}")]
    NoUndo(String),
    /// An action that goes over several things one at a time, e.g. Update All over the
    /// submodules, ran each of the `of`, but these failed; the others are done.
    #[error("{} of {of} failed: {}", failed.len(), failed.iter().map(|f| f.to_string()).collect::<Vec<_>>().join("; "))]
    PartlyDone { of: usize, failed: Vec<FailedPart> },
}

/// One part of an action that failed while the others went on, e.g. one submodule of Update All.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedPart {
    pub name: String,
    /// What git printed.
    pub error: String,
    /// The nested submodule that failed, as a path from the repository, when it wasn't `name`
    /// itself.
    pub inner: Option<String>,
}

impl Display for FailedPart {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.inner.as_ref().unwrap_or(&self.name), self.error)
    }
}

impl Error {
    /// Wrap an error from gitoxide, keeping only its message.
    pub(crate) fn git(err: impl Display) -> Self {
        Error::Git(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
