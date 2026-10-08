//! The stash: put uncommitted changes aside, bring them back (apply or pop), drop them, and
//! finish or undo an apply that stopped on conflicts.
//!
//! git keeps no state for an apply that conflicts, so Oxbow writes [`STASH_APPLY`] next to the
//! index: which stash, and whether it was a pop that should drop it once the conflicts are
//! resolved.

use serde::Serialize;

use crate::cli::{GitCommand, literal};
use crate::error::{Error, Result};
use crate::repo::Repo;

/// Oxbow's note of an apply that stopped on conflicts: the stash's id, then `pop` or `apply`.
pub(crate) const STASH_APPLY: &str = "OXBOW_STASH_APPLY";

/// Whether a stash can be applied to the checked-out commit, worked out without touching any file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StashCheck {
    /// Files that would conflict; `None` when this git can't tell (it needs 2.40).
    pub conflicts: Option<Vec<String>>,
    /// Files with uncommitted changes that the stash changes too: git refuses to apply over them.
    pub in_the_way: Vec<String>,
}

/// The files a stash changes.
pub(crate) struct StashFiles {
    /// Tracked files, with git's status letter (`M`, `A`, `D`).
    pub tracked: Vec<(char, String)>,
    /// Untracked files it put aside.
    pub untracked: Vec<String>,
}

/// An apply that stopped on conflicts, from [`STASH_APPLY`].
pub(crate) struct PendingApply {
    pub id: String,
    pub pop: bool,
    /// Untracked files of the stash that were already on disk before the apply. git leaves
    /// them alone ("already exists, no checkout"), so undoing the apply must too.
    pub kept: Vec<String>,
}

impl Repo {
    /// The id of `stash@{index}`, checked against `id` so an action never hits a stash that moved.
    pub(crate) fn stash_ref(&self, index: usize, id: &str) -> Result<String> {
        let name = format!("stash@{{{index}}}");
        match self.stashes()?.into_iter().find(|s| s.index == index) {
            Some(stash) if stash.id == id => Ok(name),
            _ => Err(Error::Git(format!(
                "{name} is not the stash you picked any more; reload and try again"
            ))),
        }
    }

    /// The current `stash@{n}` of the stash with this id.
    pub(crate) fn stash_name(&self, id: &str) -> Option<String> {
        let stashes = self.stashes().ok()?;
        let stash = stashes.iter().find(|s| s.id == id)?;
        Some(format!("stash@{{{}}}", stash.index))
    }

    pub(crate) fn stash_files(&self, id: &str) -> Result<StashFiles> {
        let out = self.run(&GitCommand::new([
            "diff",
            "--name-status",
            "--no-renames",
            "-z",
            &format!("{id}^1"),
            id,
            "--",
        ]))?;
        let mut tracked = Vec::new();
        let mut fields = out.stdout.split('\0').filter(|f| !f.is_empty());
        while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
            tracked.push((status.chars().next().unwrap_or('M'), path.to_owned()));
        }
        let untracked = match self
            .stashes()?
            .into_iter()
            .find(|s| s.id == id)
            .and_then(|s| s.untracked)
        {
            Some(commit) => self
                .run(&GitCommand::new(["ls-tree", "-r", "-z", "--name-only", &commit]))?
                .stdout
                .split('\0')
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect(),
            None => Vec::new(),
        };
        Ok(StashFiles { tracked, untracked })
    }

    /// Whether `stash@{index}` applies cleanly to `HEAD`, and what is in its way.
    pub fn stash_check(&self, index: usize, id: &str) -> Result<StashCheck> {
        self.stash_ref(index, id)?;
        let files = self.stash_files(id)?;
        let tree = self.working_tree()?;
        let changed: Vec<&str> = tree
            .staged
            .iter()
            .chain(&tree.unstaged)
            .chain(&tree.conflicted)
            .map(|f| f.path.as_str())
            .collect();
        let mut in_the_way: Vec<String> = files
            .tracked
            .iter()
            .map(|(_, p)| p)
            .chain(&files.untracked)
            .filter(|p| {
                changed.contains(&p.as_str()) || (files.untracked.contains(p) && self.workdir().join(p).exists())
            })
            .cloned()
            .collect();
        in_the_way.sort();
        in_the_way.dedup();

        // `merge-tree` merges in memory, with the stash's own base as the merge base.
        let out = self.spawn(
            &GitCommand::new([
                "merge-tree",
                "--write-tree",
                "--name-only",
                "--no-messages",
                "-z",
                "--merge-base",
                &format!("{id}^1"),
                "HEAD",
                id,
            ]),
            None,
        )?;
        let conflicts = match out.status.code() {
            Some(0) => Some(Vec::new()),
            Some(1) => {
                let text = String::from_utf8_lossy(&out.stdout);
                let mut names: Vec<String> = text
                    .split('\0')
                    .skip(1)
                    .filter(|p| !p.is_empty())
                    .map(str::to_owned)
                    .collect();
                names.dedup();
                Some(names)
            }
            _ => None,
        };
        Ok(StashCheck { conflicts, in_the_way })
    }

    pub(crate) fn pending_apply(&self) -> Option<PendingApply> {
        let text = std::fs::read_to_string(self.local().git_dir().join(STASH_APPLY)).ok()?;
        let mut lines = text.lines();
        let id = lines.next()?.trim().to_owned();
        let pop = lines.next().is_some_and(|l| l.trim() == "pop");
        // The rest, one path per NUL: a file name may hold a line break.
        let rest = text.splitn(3, '\n').nth(2).unwrap_or_default();
        let kept = rest.split('\0').filter(|p| !p.is_empty()).map(str::to_owned).collect();
        Some(PendingApply { id, pop, kept })
    }

    /// The stash's untracked files that are on disk now, before it is applied.
    pub(crate) fn untracked_in_the_way(&self, id: &str) -> Vec<String> {
        self.stash_files(id)
            .map(|files| {
                files
                    .untracked
                    .into_iter()
                    .filter(|p| self.workdir().join(p).symlink_metadata().is_ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Note an apply that stopped on conflicts, so it can be finished or undone. `kept` are
    /// the untracked files that were there before it, from [`Repo::untracked_in_the_way`].
    pub(crate) fn remember_apply(&self, id: &str, pop: bool, kept: &[String]) {
        let mut text = format!("{id}\n{}\n", if pop { "pop" } else { "apply" });
        for path in kept {
            text.push_str(path);
            text.push('\0');
        }
        let _ = std::fs::write(self.local().git_dir().join(STASH_APPLY), text);
    }

    pub(crate) fn forget_apply(&self) {
        let _ = std::fs::remove_file(self.local().git_dir().join(STASH_APPLY));
    }

    /// Finish an apply whose conflicts are resolved, leaving the files as a clean apply does:
    /// changed but unstaged, with files the stash added still staged. Other staged files are
    /// the user's own and stay as they are. A pop drops the stash now, which git left in place
    /// because of the conflict.
    pub(crate) fn plan_finish_apply(&self, pending: &PendingApply) -> Result<Vec<GitCommand>> {
        let (in_head, _) = self.stash_paths_in_head(&self.stash_files(&pending.id)?)?;
        let mut commands = Vec::new();
        if !in_head.is_empty() {
            commands.push(
                with_paths(&["reset", "--quiet"], &in_head).comment("clear the conflict marks, the files stay changed"),
            );
        }
        if pending.pop
            && let Some(name) = self.stash_name(&pending.id)
        {
            commands.push(
                GitCommand::new(["stash", "drop", "--quiet", &name])
                    .comment("pop keeps the stash when it hits a conflict"),
            );
        }
        if commands.is_empty() {
            commands.push(GitCommand::new(["update-index", "-q", "--refresh"]).comment("nothing left to clear"));
        }
        Ok(commands)
    }

    /// The stash's tracked files that `HEAD` has, and the ones it adds.
    fn stash_paths_in_head(&self, files: &StashFiles) -> Result<(Vec<String>, Vec<String>)> {
        let paths: Vec<&str> = files.tracked.iter().map(|(_, p)| p.as_str()).collect();
        let mut in_head: Vec<String> = Vec::new();
        if !paths.is_empty() {
            let mut args = vec!["ls-tree", "-r", "-z", "--name-only", "HEAD", "--"];
            let literal: Vec<String> = paths.iter().map(|p| literal(p)).collect();
            args.extend(literal.iter().map(String::as_str));
            in_head = self
                .run(&GitCommand::new(args))?
                .stdout
                .split('\0')
                .filter(|p| !p.is_empty())
                .map(str::to_owned)
                .collect();
        }
        let added = paths
            .iter()
            .filter(|p| !in_head.iter().any(|h| h == *p))
            .map(|p| p.to_string())
            .collect();
        Ok((in_head, added))
    }

    /// The stash's untracked files the apply may have restored, with their blobs: plain files on
    /// disk now, not in `kept`. A stash made without untracked files has none.
    fn restorable(&self, pending: &PendingApply) -> Result<Vec<(String, String)>> {
        let Some(commit) = self
            .stashes()?
            .into_iter()
            .find(|s| s.id == pending.id)
            .and_then(|s| s.untracked)
        else {
            return Ok(Vec::new());
        };
        let out = self.run(&GitCommand::new(["ls-tree", "-r", "-z", &commit]))?;
        let mut files = Vec::new();
        for record in out.stdout.split('\0').filter(|r| !r.is_empty()) {
            let Some((meta, path)) = record.split_once('\t') else {
                continue;
            };
            let mut meta = meta.split(' ');
            let (Some(mode), Some(_), Some(blob)) = (meta.next(), meta.next(), meta.next()) else {
                continue;
            };
            let plain = self.workdir().join(path).symlink_metadata().is_ok_and(|m| m.is_file());
            if (mode == "100644" || mode == "100755") && plain && !pending.kept.iter().any(|k| k == path) {
                files.push((path.to_owned(), blob.to_owned()));
            }
        }
        Ok(files)
    }

    /// git's id for `file` as if it were at `path` in the working tree (same filters).
    fn hash_as(&self, file: &std::path::Path, path: &str) -> Option<String> {
        let out = self
            .run(&GitCommand::new([
                "hash-object".to_owned(),
                format!("--path={path}"),
                "--".to_owned(),
                file.display().to_string(),
            ]))
            .ok()?;
        Some(out.stdout.trim().to_owned())
    }

    /// The stash's untracked files that are on disk now just as the stash has them: the ones
    /// the apply restored. A file that appeared before git got to it is left alone by git ("already
    /// exists, no checkout") and has other content, so it is not one of them; nor is `kept`, or a
    /// restored file edited since. What the confirmation sheet lists; [`Repo::remove_brought_back`]
    /// checks again as it removes them.
    fn brought_back(&self, pending: &PendingApply) -> Result<Vec<String>> {
        Ok(self
            .restorable(pending)?
            .into_iter()
            .filter(|(path, blob)| self.hash_as(&self.workdir().join(path), path).as_deref() == Some(blob))
            .map(|(path, _)| path)
            .collect())
    }

    /// Remove the untracked files an undone apply restored. Each file is first moved aside, which
    /// is atomic, and only then compared with the stash: an edit saved up to that moment is
    /// in the copy and keeps it, and one saved later makes a new file that stays where it is. A
    /// copy that differs goes back in place, or, if its place was taken meanwhile, stays in
    /// `.git/oxbow/kept` and the undo stops before git changes anything.
    pub(crate) fn remove_brought_back(&self, pending: &PendingApply) -> Result<()> {
        let files = self.restorable(pending)?;
        if files.is_empty() {
            return Ok(());
        }
        let io = |err: std::io::Error| Error::Git(err.to_string());
        let aside = self.local().git_dir().join("oxbow").join("abort");
        std::fs::create_dir_all(&aside).map_err(io)?;
        let mut stuck = Vec::new();
        for (n, (path, blob)) in files.iter().enumerate() {
            let file = self.workdir().join(path);
            let moved = aside.join(n.to_string());
            // Another file system (a linked worktree elsewhere) can't move atomically: keep it.
            if std::fs::rename(&file, &moved).is_err() {
                continue;
            }
            if self.hash_as(&moved, path).as_deref() == Some(blob.as_str()) {
                std::fs::remove_file(&moved).map_err(io)?;
            } else if file.symlink_metadata().is_err() {
                std::fs::rename(&moved, &file).map_err(io)?;
            } else {
                let kept = self.local().git_dir().join("oxbow").join("kept").join(path);
                if let Some(parent) = kept.parent() {
                    std::fs::create_dir_all(parent).map_err(io)?;
                }
                std::fs::rename(&moved, &kept).map_err(io)?;
                stuck.push(kept.display().to_string());
            }
        }
        let _ = std::fs::remove_dir(&aside);
        if stuck.is_empty() {
            Ok(())
        } else {
            Err(Error::Git(format!(
                "files changed while the apply was being undone; your versions are in {}",
                stuck.join(", ")
            )))
        }
    }

    /// Undo an apply that stopped on conflicts. git refused to apply over uncommitted changes in
    /// these files, so putting them back to `HEAD` loses only what the stash brought.
    pub(crate) fn plan_undo_apply(&self, pending: &PendingApply) -> Result<Vec<GitCommand>> {
        let files = self.stash_files(&pending.id)?;
        let (in_head, added) = self.stash_paths_in_head(&files)?;
        let mut commands = Vec::new();
        if !in_head.is_empty() {
            commands.push(with_paths(&["checkout", "HEAD"], &in_head).comment("back to how HEAD has them"));
        }
        if !added.is_empty() {
            commands.push(with_paths(&["rm", "-f", "--quiet"], &added).comment("files the stash added"));
        }
        if commands.is_empty() {
            commands.push(GitCommand::new(["reset", "--quiet"]).comment("clear the conflict marks"));
        }
        // Oxbow removes the restored untracked files itself, checking each one as it goes.
        let brought = self.brought_back(pending)?;
        if !brought.is_empty() {
            let quoted: Vec<String> = brought.iter().map(|p| crate::cli::shell_quote(p)).collect();
            let first = commands.remove(0);
            commands.insert(
                0,
                first.before(format!(
                    "rm -- {}  # untracked files the stash brought back, if still as it has them",
                    quoted.join(" ")
                )),
            );
        }
        Ok(commands)
    }
}

/// `git <args> -- <paths>`
fn with_paths(args: &[&str], paths: &[String]) -> GitCommand {
    GitCommand::new(
        args.iter()
            .map(|a| a.to_string())
            .chain(["--".to_owned()])
            .chain(paths.iter().map(|p| literal(p))),
    )
}

#[cfg(test)]
mod tests {
    use crate::repo::split_stash_message;

    #[test]
    fn a_stash_message_splits_into_branch_and_text() {
        assert_eq!(
            split_stash_message("On main: Try a smaller pool"),
            (Some("main".into()), "Try a smaller pool".into())
        );
        assert_eq!(
            split_stash_message("WIP on fix/a: 1a2b3c4 Fix it"),
            (Some("fix/a".into()), "Fix it".into())
        );
        assert_eq!(
            split_stash_message("On (no branch): Detached"),
            (None, "Detached".into())
        );
        assert_eq!(split_stash_message("by hand"), (None, "by hand".into()));
        // Only an id dropped from git's own wording, never a word of the user's message.
        assert_eq!(
            split_stash_message("On main: 1a2b3c4 is wrong"),
            (Some("main".into()), "1a2b3c4 is wrong".into())
        );
    }
}
