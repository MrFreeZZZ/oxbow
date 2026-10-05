//! Running the system `git` for commands that change a repository.
//!
//! Every write goes through the `git` command line so hooks, credentials, SSH and the user's
//! config behave exactly as in a terminal. A [`GitCommand`] is also what the confirmation sheet
//! shows, so the user sees the very command that will run.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::repo::Repo;

/// One `git` invocation, with an optional explanation for the confirmation sheet.
/// It serializes with its `display` form too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommand {
    /// Arguments after `git`.
    pub args: Vec<String>,
    /// What the command does, shown as a `# comment` next to it.
    pub comment: Option<String>,
    /// Ask git for progress output even though it does not write to a terminal. Not shown, since
    /// nobody types it.
    pub progress: bool,
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
            progress: false,
        }
    }

    pub fn comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }

    pub fn with_progress(mut self) -> Self {
        self.progress = true;
        self
    }

    /// Arguments as they are passed to git, `--progress` included.
    fn run_args(&self) -> Vec<&str> {
        let mut args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        if self.progress && !args.is_empty() {
            args.insert(1, "--progress");
        }
        args
    }

    /// The command as it would be typed in a shell, with arguments quoted where needed.
    pub fn display(&self) -> String {
        std::iter::once("git".to_owned())
            .chain(self.args.iter().map(|a| shell_quote(a)))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl Serialize for GitCommand {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        struct Shown<'a> {
            args: &'a [String],
            comment: &'a Option<String>,
            display: String,
        }
        Shown {
            args: &self.args,
            comment: &self.comment,
            display: self.display(),
        }
        .serialize(serializer)
    }
}

/// One line a running command printed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputLine {
    pub text: String,
    /// Printed to standard error, where git writes its messages and progress.
    pub stderr: bool,
    /// Ended with a carriage return: a progress line that the next one replaces.
    pub progress: bool,
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

    /// `git` set up to run `command` in the working directory.
    fn command(&self, command: &GitCommand) -> Command {
        let mut git = Command::new("git");
        git
            // Paths with non-ASCII names come back as they are, not as octal escapes.
            .args(["-c", "core.quotePath=false"])
            .args(command.run_args())
            .current_dir(self.workdir())
            // There is no terminal to type a password into: fail instead of hanging.
            .env("GIT_TERMINAL_PROMPT", "0")
            // Messages stay in English, as in most guides and search results.
            .env("LC_MESSAGES", "C");
        git
    }

    /// Run `command` and return its raw output whatever its exit code.
    pub(crate) fn spawn(&self, command: &GitCommand, input: Option<&[u8]>) -> Result<std::process::Output> {
        let mut child = self
            .command(command)
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

impl Repo {
    /// Run `command`, passing each line it prints to `on_line` as it comes. Setting `cancel`
    /// stops the command; it then fails with [`Error::Cancelled`].
    pub fn run_streaming(
        &self,
        command: &GitCommand,
        on_line: &mut dyn FnMut(OutputLine),
        cancel: &AtomicBool,
    ) -> Result<CommandOutput> {
        let mut child = self
            .command(command)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| Error::GitNotFound(err.to_string()))?;
        let (send, receive) = mpsc::channel();
        let readers = [
            read_lines(child.stdout.take().expect("stdout is piped"), false, send.clone()),
            read_lines(child.stderr.take().expect("stderr is piped"), true, send),
        ];
        let mut output = CommandOutput::default();
        let status = loop {
            match receive.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => {
                    if !line.progress {
                        let all = if line.stderr {
                            &mut output.stderr
                        } else {
                            &mut output.stdout
                        };
                        all.push_str(&line.text);
                        all.push('\n');
                    }
                    on_line(line);
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                // Both pipes are closed: the command is done, or about to be.
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break child.wait().map_err(|err| Error::GitNotFound(err.to_string()))?;
                }
            }
            if cancel.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Cancelled);
            }
        };
        for reader in readers {
            let _ = reader.join();
        }
        if status.success() {
            Ok(output)
        } else {
            Err(Error::Command {
                command: command.display(),
                code: status.code(),
                output: if output.stderr.trim().is_empty() {
                    output.stdout
                } else {
                    output.stderr
                },
            })
        }
    }
}

/// Read `pipe` on a thread of its own, sending each line split at `\n` or `\r`.
fn read_lines(
    mut pipe: impl Read + Send + 'static,
    stderr: bool,
    send: mpsc::Sender<OutputLine>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut pending: Vec<u8> = Vec::new();
        let mut buffer = [0u8; 4096];
        let emit = |bytes: &[u8], progress: bool| {
            let text = String::from_utf8_lossy(bytes).trim_end().to_owned();
            if !text.is_empty() {
                let _ = send.send(OutputLine { text, stderr, progress });
            }
        };
        loop {
            let n = match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            for &byte in &buffer[..n] {
                if byte == b'\n' || byte == b'\r' {
                    emit(&pending, byte == b'\r');
                    pending.clear();
                } else {
                    pending.push(byte);
                }
            }
        }
        emit(&pending, false);
    })
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
