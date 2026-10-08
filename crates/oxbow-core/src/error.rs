use std::fmt::Display;

/// Errors returned by `oxbow-core`.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path} is not a Git repository")]
    NotARepository { path: String },
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
}

impl Error {
    /// Wrap an error from gitoxide, keeping only its message.
    pub(crate) fn git(err: impl Display) -> Self {
        Error::Git(err.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;
