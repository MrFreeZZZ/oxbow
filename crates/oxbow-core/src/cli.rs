//! Running the system `git` for commands that change a repository.
//!
//! Every write goes through the `git` command line so hooks, credentials, SSH and the user's
//! config behave exactly as in a terminal. A [`GitCommand`] is also what the confirmation sheet
//! shows, so the user sees the very command that will run.

use std::io::Write;
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::error::{Error, Result};
use crate::repo::Repo;

/// One `git` invocation, with an optional explanation for the confirmation sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommand {
    /// Arguments after `git`.
    pub args: Vec<String>,
    /// What the command does, shown as a `# comment` next to it.
    pub comment: Option<String>,
}

impl GitCommand {
    pub fn new<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        GitCommand {
            args: args.into_iter().map(Into::into).collect(),
            comment: None,
        }
    }

    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }

    /// The command as it would be typed in a shell, with arguments quoted where needed.
    pub fn display(&self) -> String {
        std::iter::once("git".to_owned())
            .chain(self.args.iter().map(|a| shell_quote(a)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// What a finished command printed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
}

impl Repo {
    /// Run `command` in the repository's working directory and wait for it.
    pub fn run(&self, command: &GitCommand) -> Result<CommandOutput> {
        self.run_with_input(command, None)
    }

    /// Run `command`, feeding `input` to its standard input (a patch for `git apply`).
    pub fn run_with_input(&self, command: &GitCommand, input: Option<&[u8]>) -> Result<CommandOutput> {
        let output = self.spawn(command, input)?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if output.status.success() {
            Ok(CommandOutput { stdout, stderr })
        } else {
            Err(Error::Command {
                command: command.display(),
                code: output.status.code(),
                output: if stderr.trim().is_empty() { stdout } else { stderr },
            })
        }
    }

    /// Run `command` and return its raw output whatever its exit code.
    pub(crate) fn spawn(&self, command: &GitCommand, input: Option<&[u8]>) -> Result<std::process::Output> {
        let mut child = Command::new("git")
            // Paths with non-ASCII names come back as they are, not as octal escapes.
            .args(["-c", "core.quotePath=false"])
            .args(&command.args)
            .current_dir(self.workdir())
            // There is no terminal to type a password into: fail instead of hanging.
            .env("GIT_TERMINAL_PROMPT", "0")
            // Messages stay in English, as in most guides and search results.
            .env("LC_MESSAGES", "C")
            .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| Error::GitNotFound(err.to_string()))?;
        if let Some(input) = input {
            let mut stdin = child.stdin.take().expect("stdin is piped");
            // A command that exits early closes its end; its exit status tells what went wrong.
            let _ = stdin.write_all(input);
        }
        child
            .wait_with_output()
            .map_err(|err| Error::GitNotFound(err.to_string()))
    }
}

/// Quote an argument for display the way a POSIX shell would need it.
fn shell_quote(arg: &str) -> String {
    let plain = !arg.is_empty() && arg.chars().all(|c| c.is_alphanumeric() || "-_./:=@+,%^~{}".contains(c));
    if plain {
        arg.to_owned()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_quotes_only_what_a_shell_needs() {
        let cmd = GitCommand::new(["commit", "-m", "Fix the user's login", "--", "src/a b.rs"]);
        assert_eq!(
            cmd.display(),
            r"git commit -m 'Fix the user'\''s login' -- 'src/a b.rs'"
        );
        assert_eq!(
            GitCommand::new(["switch", "auth/3-ui"]).display(),
            "git switch auth/3-ui"
        );
        assert_eq!(GitCommand::new(["reset", "HEAD~1"]).display(), "git reset HEAD~1");
        assert_eq!(
            GitCommand::new(["stash", "apply", "stash@{0}"]).display(),
            "git stash apply stash@{0}"
        );
    }
}
