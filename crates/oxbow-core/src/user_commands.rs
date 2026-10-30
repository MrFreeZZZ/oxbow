//! Explicit personal commands. Context travels as environment values, never shell source.
use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

use crate::{Error, Repo, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCommand {
    pub id: String,
    pub name: String,
    pub script: String,
    /// Empty means all repositories. Kept in personal settings, never read from a repository.
    pub repository: String,
}

impl UserCommand {
    pub fn validate(&self) -> Result<()> {
        if self.id.is_empty()
            || !self.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || self.name.trim().is_empty()
            || self.script.trim().is_empty()
            || self.script.len() > 65536
            || self.name.len() > 200
            || [&self.id, &self.name, &self.script, &self.repository]
                .iter()
                .any(|v| v.contains('\0'))
        {
            return Err(Error::Git(
                "A command needs an ID, a name and a script without NUL characters.".into(),
            ));
        }
        Ok(())
    }
}

impl Repo {
    /// Run in the original repository. Selected refs describe context; they never check out files.
    pub fn run_user_command(
        &self,
        command: &UserCommand,
        branch: &str,
        commit: &str,
        cancel: &AtomicBool,
    ) -> Result<()> {
        command.validate()?;
        if crate::trust::is_restricted(self.workdir()) {
            return Err(Error::Git("Custom commands are unavailable in Restricted mode. Trust this repository before running its programs.".into()));
        }
        let path = self.workdir().to_string_lossy();
        if !command.repository.is_empty() && command.repository != path {
            return Err(Error::Changed("This command belongs to another repository.".into()));
        }
        if branch.contains('\0') || commit.contains('\0') {
            return Err(Error::Git("Invalid command context.".into()));
        }
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(Error::Cancelled);
        }
        let run = crate::checks::shell_command(&command.script)
            .env("OXBOW_REPOSITORY", path.as_ref())
            .env("OXBOW_BRANCH", branch)
            .env("OXBOW_COMMIT", commit);
        crate::console::main(|| self.run_streaming(&run, &mut |_| {}, cancel)).map(|_| ())
    }
}
