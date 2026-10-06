//! Actions on single commits from the History menu: copy one onto the checked-out branch
//! (cherry-pick), undo one with a new commit (revert), move the branch to one (reset), and
//! change the message of the last one.

use serde::{Deserialize, Serialize};

use crate::cli::GitCommand;
use crate::error::Result;
use crate::repo::Repo;

/// What a reset keeps of the commits it takes off the branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResetMode {
    /// Their changes stay staged.
    Soft,
    /// Their changes stay in the files, unstaged.
    Mixed,
    /// Their changes and every uncommitted change are thrown away.
    Hard,
}

impl Repo {
    pub(crate) fn plan_cherry_pick(&self, commit: &str) -> Result<Vec<GitCommand>> {
        let mut args = vec!["cherry-pick".to_owned()];
        let comment = if self.is_merge(commit)? {
            args.extend(["-m".to_owned(), "1".to_owned()]);
            "-m 1: the changes the merge brought into its first parent, as a new commit"
        } else {
            "the same changes and message, as a new commit with a new SHA"
        };
        args.push(short(commit));
        Ok(vec![GitCommand::new(args).comment(comment)])
    }

    pub(crate) fn plan_revert(&self, commit: &str) -> Result<Vec<GitCommand>> {
        let mut args = vec!["revert".to_owned(), "--no-edit".to_owned()];
        let comment = if self.is_merge(commit)? {
            args.extend(["-m".to_owned(), "1".to_owned()]);
            "-m 1: undo what the merge brought in; the merged commits stay in history"
        } else {
            "a new commit with the opposite changes; the original stays in history"
        };
        args.push(short(commit));
        Ok(vec![GitCommand::new(args).comment(comment)])
    }

    /// Whether `commit` has more than one parent.
    fn is_merge(&self, commit: &str) -> Result<bool> {
        let out = self.run(&GitCommand::new(["rev-list", "--parents", "-n", "1", commit, "--"]))?;
        Ok(out.stdout.split_whitespace().count() > 2)
    }
}

pub(crate) fn plan_reset(commit: &str, mode: ResetMode) -> Vec<GitCommand> {
    let (flag, comment) = match mode {
        ResetMode::Soft => ("--soft", "--soft: move the branch, keep the changes staged"),
        ResetMode::Mixed => (
            "--mixed",
            "--mixed: move the branch, keep the changes in your files, unstaged",
        ),
        ResetMode::Hard => (
            "--hard",
            "--hard: move the branch and overwrite the index and your files",
        ),
    };
    vec![GitCommand::new(["reset".to_owned(), flag.to_owned(), short(commit)]).comment(comment)]
}

/// A full sha reads better short; `HEAD~1` and names stay as they are.
fn short(commit: &str) -> String {
    let full_sha = commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit());
    if full_sha {
        commit[..7].to_owned()
    } else {
        commit.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reset_shows_the_short_sha_and_what_it_keeps() {
        let plan = plan_reset("0123456789abcdef0123456789abcdef01234567", ResetMode::Hard);
        assert_eq!(plan[0].display(), "git reset --hard 0123456");
        let plan = plan_reset("HEAD~1", ResetMode::Soft);
        assert_eq!(plan[0].display(), "git reset --soft HEAD~1");
    }
}
