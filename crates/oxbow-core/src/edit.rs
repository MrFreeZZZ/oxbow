//! Actions on single commits from the History menu: copy one onto the checked-out branch
//! (cherry-pick), undo one with a new commit (revert), move the branch to one (reset), and
//! change the message of the last one. And on one file of a commit: undo or copy just its
//! change to that file, or take the file as the commit has it, into the files on disk.

use serde::{Deserialize, Serialize};

use crate::cli::{GitCommand, literal};
use crate::error::{Error, Result};
use crate::oplog::{EMPTY_TREE, short_id};
use crate::repo::Repo;

/// A file's patch bigger than this isn't applied from here: it goes to git on standard input,
/// and the sheet gets it too.
const MAX_FILE_PATCH: usize = 16 * 1024 * 1024;

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
        // The whole sha: seven characters can match more than one object in a big repository.
        args.push(commit.to_owned());
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
        args.push(commit.to_owned());
        Ok(vec![GitCommand::new(args).comment(comment)])
    }

    /// Whether `commit` has more than one parent.
    fn is_merge(&self, commit: &str) -> Result<bool> {
        Ok(self.parents(commit)?.len() > 1)
    }

    fn parents(&self, commit: &str) -> Result<Vec<String>> {
        let out = self.run(&GitCommand::new(["rev-list", "--parents", "-n", "1", commit, "--"]))?;
        Ok(out.stdout.split_whitespace().skip(1).map(str::to_owned).collect())
    }

    /// Revert This File (`reverse`) or Cherry-Pick This File: the commit's patch of the file,
    /// applied to the file on disk. When the file changed since, so the patch no longer fits as
    /// it is, git merges it in (`--3way`), which needs the file on disk as it is staged. A merge
    /// that would conflict is refused before anything runs: conflicts outside a merge or a
    /// cherry-pick would have nothing to finish them.
    pub(crate) fn plan_file_patch(
        &self,
        commit: &str,
        path: &str,
        old_path: Option<&str>,
        reverse: bool,
    ) -> Result<Vec<GitCommand>> {
        let sha = short_id(commit);
        let patch = self.file_patch(commit, path, old_path)?;
        if patch.is_empty() {
            return Err(Error::Git(format!("{sha} doesn’t change {path}")));
        }
        let mut args = vec!["apply"];
        if reverse {
            args.push("--reverse");
        }
        let check = [args.as_slice(), &["--check", "-"]].concat();
        let fits = self
            .spawn(&GitCommand::new(check), Some(patch.as_bytes()))?
            .status
            .success();
        let comment = if fits {
            if reverse {
                format!("--reverse: what {sha} changed in {path} comes out of the file on disk; nothing is staged")
            } else {
                format!("what {sha} changed in {path} goes into the file on disk; nothing is staged")
            }
        } else {
            if self.has_unstaged(path)? {
                return Err(Error::Git(format!(
                    "{path} changed since {sha}, so its change has to be merged in, and the file has unstaged changes; stage or stash them first"
                )));
            }
            args.push("--3way");
            if !self.merges_cleanly(&args, &patch)? {
                let whole = if reverse {
                    "Revert the whole commit"
                } else {
                    "Cherry-pick the whole commit"
                };
                return Err(Error::Git(format!(
                    "{path} changed since {sha}, and its change conflicts with the file as it is now. {whole} to resolve the conflicts, or change the file by hand"
                )));
            }
            format!("--3way: {path} changed since {sha}, so the change is merged in; the merged file is staged")
        };
        args.push("-");
        Ok(vec![GitCommand::new(args).comment(comment).input(patch)])
    }

    /// What `commit` changed in `path` (and `old_path`, before a rename), from its first parent,
    /// as a patch `git apply` takes whatever the diff settings say.
    fn file_patch(&self, commit: &str, path: &str, old_path: Option<&str>) -> Result<String> {
        let parent = self
            .parents(commit)?
            .into_iter()
            .next()
            .unwrap_or_else(|| EMPTY_TREE.to_owned());
        let mut args: Vec<String> = [
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            "--binary",
            "--find-renames",
            "--src-prefix=a/",
            "--dst-prefix=b/",
        ]
        .map(str::to_owned)
        .to_vec();
        args.extend([parent, commit.to_owned(), "--".to_owned()]);
        if let Some(old) = old_path.filter(|old| *old != path) {
            args.push(literal(old));
        }
        args.push(literal(path));
        let out = self.spawn(&GitCommand::new(args), None)?;
        if !out.status.success() {
            return Err(Error::Git(String::from_utf8_lossy(&out.stderr).trim().to_owned()));
        }
        if out.stdout.len() > MAX_FILE_PATCH {
            return Err(Error::Git(format!(
                "the change to {path} is too big to apply from here; git diff and git apply in a terminal can"
            )));
        }
        String::from_utf8(out.stdout).map_err(|_| {
            Error::Git(format!(
                "{path} is text that isn’t UTF-8, so its change can’t go through here; git diff and git apply in a terminal can"
            ))
        })
    }

    /// Whether `git <args>` merges the patch in without conflicts: tried on a copy of the index.
    fn merges_cleanly(&self, args: &[&str], patch: &str) -> Result<bool> {
        let scratch = self.scratch_index()?;
        let mut try_args = args.to_vec();
        try_args.extend(["--cached", "-"]);
        let out = self.spawn(
            &GitCommand::new(try_args).env("GIT_INDEX_FILE", scratch.to_string_lossy().into_owned()),
            Some(patch.as_bytes()),
        );
        let _ = std::fs::remove_file(&scratch);
        Ok(out?.status.success())
    }

    /// Whether the file on disk differs from its staged version.
    fn has_unstaged(&self, path: &str) -> Result<bool> {
        let out = self.spawn(
            &GitCommand::new(["diff", "--quiet", "--no-ext-diff", "--", &literal(path)]),
            None,
        )?;
        Ok(out.status.code() == Some(1))
    }

    /// Take This Version: the file as `commit` has it, on disk only.
    pub(crate) fn plan_take_version(&self, commit: &str, path: &str) -> Result<Vec<GitCommand>> {
        Ok(vec![
            GitCommand::new([
                "restore".to_owned(),
                format!("--source={commit}"),
                "--worktree".to_owned(),
                "--".to_owned(),
                literal(path),
            ])
            .comment(format!(
                "--source: the file as it is in {}; only the file on disk changes, nothing is staged",
                short_id(commit)
            )),
        ])
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
    vec![GitCommand::new(["reset", flag, commit]).comment(comment)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reset_names_the_whole_sha_and_what_it_keeps() {
        // Seven characters could match another object; the command names exactly this one.
        let plan = plan_reset("0123456789abcdef0123456789abcdef01234567", ResetMode::Hard);
        assert_eq!(
            plan[0].display(),
            "git reset --hard 0123456789abcdef0123456789abcdef01234567"
        );
        let plan = plan_reset("HEAD~1", ResetMode::Soft);
        assert_eq!(plan[0].display(), "git reset --soft HEAD~1");
    }
}
