//! Checks before a commit: commands of the user's own, such as a linter, a formatter or the
//! tests, that run in the repository's folder after the secrets check and stop the commit when
//! one fails.
//!
//! They are kept in the repository's `.git/config` as `oxbow.check`, one value per command in
//! the order they run, so a terminal sees them too (`git config --get-all oxbow.check`). A value
//! that starts with `#` is a check turned off. A repository from someone else can carry them,
//! as it can hooks, so [`crate::trust`] lists them, and in Restricted none runs.

use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::action::ActionEvent;
use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::repo::Repo;

/// The setting that holds the checks.
pub const CHECK_KEY: &str = "oxbow.check";

/// What the Console calls the checks' commands.
pub(crate) const CHECKS_ROLE: &str = "checks before commit";

/// The most lines of a failed check's output kept for the sheet; the Console has them all.
const KEPT_LINES: usize = 200;

/// One check: the command as typed in a shell, and whether it runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitCheck {
    pub command: String,
    pub on: bool,
}

/// A check that stopped a commit: its command, its exit code, and the end of what it printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckFailure {
    pub command: String,
    pub code: Option<i32>,
    pub output: String,
}

/// A value of `oxbow.check` as a check; `None` for an empty one.
fn parse_value(value: &str) -> Option<CommitCheck> {
    let value = value.trim();
    let (command, on) = match value.strip_prefix('#') {
        Some(rest) => (rest.trim(), false),
        None => (value, true),
    };
    (!command.is_empty()).then(|| CommitCheck {
        command: command.to_owned(),
        on,
    })
}

/// The value of `oxbow.check` that keeps `check`.
fn value_of(check: &CommitCheck) -> String {
    let command = check.command.trim();
    if check.on {
        command.to_owned()
    } else {
        format!("# {command}")
    }
}

/// Whether `value` of `oxbow.check` is a check that runs.
pub(crate) fn runs(value: &str) -> bool {
    parse_value(value).is_some_and(|check| check.on)
}

/// The command that runs `command` the way a terminal would: in the user's login shell, so
/// tools that shell finds (npm, ruff, cargo) are found here too, though an app started from the
/// Dock gets a short `PATH`.
pub(crate) fn shell_command(command: &str) -> GitCommand {
    #[cfg(windows)]
    let run = GitCommand::new(["/C", command]).program("cmd");
    #[cfg(not(windows))]
    let run = {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| std::path::Path::new(s).is_absolute() && std::path::Path::new(s).is_file())
            .unwrap_or_else(|| "/bin/sh".to_owned());
        GitCommand::new(["-l", "-c", command]).program(shell)
    };
    // A test run can be quiet for a long while; Stop ends it.
    run.waits_for_user()
}

impl Repo {
    /// The checks of this repository, in the order they run.
    pub fn commit_checks(&self) -> Result<Vec<CommitCheck>> {
        match self.run(&GitCommand::new([
            "config",
            "--local",
            "--null",
            "--get-all",
            CHECK_KEY,
        ])) {
            Ok(out) => Ok(out.stdout.split('\0').filter_map(parse_value).collect()),
            // 1: none set.
            Err(Error::Command { code: Some(1), .. }) => Ok(Vec::new()),
            Err(err) => Err(err),
        }
    }

    /// Keep `checks` as this repository's, in this order; empty ones are left out.
    pub fn set_commit_checks(&self, checks: &[CommitCheck]) -> Result<()> {
        match self.run(&GitCommand::new(["config", "--local", "--unset-all", CHECK_KEY])) {
            // 5: there were none.
            Ok(_) | Err(Error::Command { code: Some(5), .. }) => {}
            Err(err) => return Err(err),
        }
        for check in checks.iter().filter(|c| !c.command.trim().is_empty()) {
            self.run(&GitCommand::new([
                "config",
                "--local",
                "--add",
                CHECK_KEY,
                &value_of(check),
            ]))?;
        }
        Ok(())
    }

    /// The checks that will run before a commit, as the sheets show them before its command:
    /// `ruff check . && npm test`. `None` when none is on, or in Restricted.
    pub(crate) fn checks_before(&self) -> Option<String> {
        if crate::trust::is_restricted(self.workdir()) {
            return None;
        }
        let on: Vec<String> = self
            .commit_checks()
            .ok()?
            .into_iter()
            .filter(|c| c.on)
            .map(|c| c.command)
            .collect();
        (!on.is_empty()).then(|| on.join(" && "))
    }

    /// Run the checks that are on, one after another, each shown in `on_event` and the Console.
    /// The first that fails stops the rest: [`Error::Check`]. In Restricted none runs, as no hook
    /// does. Setting `cancel` stops the running one.
    pub(crate) fn run_commit_checks(&self, on_event: &mut dyn FnMut(ActionEvent), cancel: &AtomicBool) -> Result<()> {
        if crate::trust::is_restricted(self.workdir()) {
            return Ok(());
        }
        let checks = self.commit_checks()?;
        for check in checks.iter().filter(|c| c.on) {
            let command = shell_command(&check.command);
            on_event(ActionEvent::Command {
                display: command.display(),
            });
            let mut lines: std::collections::VecDeque<String> = std::collections::VecDeque::new();
            let ran = crate::console::role(CHECKS_ROLE, || {
                self.run_streaming(
                    &command,
                    &mut |line| {
                        if !line.progress {
                            if lines.len() == KEPT_LINES {
                                lines.pop_front();
                            }
                            lines.push_back(line.text.clone());
                        }
                        on_event(ActionEvent::Line(line));
                    },
                    cancel,
                )
            });
            match ran {
                Ok(_) => {}
                Err(Error::Command { code, .. }) => {
                    return Err(Error::Check(Box::new(CheckFailure {
                        command: check.command.clone(),
                        code,
                        output: Vec::from(lines).join("\n").trim_end().to_owned(),
                    })));
                }
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_with_a_hash_is_a_check_turned_off() {
        assert_eq!(
            parse_value("ruff check ."),
            Some(CommitCheck {
                command: "ruff check .".into(),
                on: true
            })
        );
        assert_eq!(
            parse_value("#  npm test "),
            Some(CommitCheck {
                command: "npm test".into(),
                on: false
            })
        );
        assert_eq!(parse_value("  "), None);
        assert_eq!(parse_value("#"), None);
        assert!(runs("cargo fmt --check"));
        assert!(!runs("# cargo fmt --check"));
    }

    #[test]
    fn a_check_keeps_its_value_back() {
        for check in [
            CommitCheck {
                command: "npm test".into(),
                on: true,
            },
            CommitCheck {
                command: "npm test".into(),
                on: false,
            },
        ] {
            assert_eq!(parse_value(&value_of(&check)), Some(check));
        }
    }
}
