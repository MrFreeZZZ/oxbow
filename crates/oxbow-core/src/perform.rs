//! Running an [`Action`] ([`Repo::perform`]): its git commands one after another, with the
//! snapshot for Undo before them, the secrets check and the checks before a commit, progress
//! and Stop.

//! The working copy: what changed since the last commit, staging and committing.
//!
//! Status and diffs come from the `git` command line, so they honor every setting that affects
//! them (ignore rules, attributes, line endings, fsmonitor), and a hunk shown on screen is
//! exactly the hunk `git apply` gets when it is staged. Every change to the repository is an
//! [`Action`] that turns into the [`GitCommand`]s shown in the confirmation sheet.

use std::sync::atomic::AtomicBool;

use crate::action::{Action, ActionEvent};
use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::operation::{MergeMethod, OperationKind};
use crate::repo::Repo;
use crate::worktree::Side;

/// What the Console calls the commands that take the snapshots an Undo goes back to.
pub(crate) const UNDO_SNAPSHOT: &str = "snapshot for Undo";

/// A `.gitignore` bigger than this isn't added to: it is read whole and written back.
pub(crate) const MAX_GITIGNORE_BYTES: u64 = 16 * 1024 * 1024;

impl Repo {
    /// Run `action`. Returns what the last command printed.
    pub fn perform(&self, action: &Action) -> Result<String> {
        self.perform_with(action, &mut |_| {}, &AtomicBool::new(false))
    }

    /// Run `action`, reporting each command and each line it prints to `on_event`. Setting
    /// `cancel` stops the running command. The step goes into the Operation Log with the state
    /// before and after it, so it can be undone. When the state before can't be saved nothing
    /// runs ([`Error::NoUndo`]): see [`Repo::perform_without_undo`]. A step that ran but can't
    /// be written to the log is reported as [`ActionEvent::NotLogged`].
    pub fn perform_with(
        &self,
        action: &Action,
        on_event: &mut dyn FnMut(ActionEvent),
        cancel: &AtomicBool,
    ) -> Result<String> {
        self.perform_logged(action, on_event, cancel, false)
    }

    /// [`Repo::perform_with`] once it was chosen to go on without Undo: the state before is
    /// saved when it can be, and nothing stops the action when it can't.
    pub fn perform_without_undo(
        &self,
        action: &Action,
        on_event: &mut dyn FnMut(ActionEvent),
        cancel: &AtomicBool,
    ) -> Result<String> {
        self.perform_logged(action, on_event, cancel, true)
    }

    fn perform_logged(
        &self,
        action: &Action,
        on_event: &mut dyn FnMut(ActionEvent),
        cancel: &AtomicBool,
        without_undo: bool,
    ) -> Result<String> {
        // Nothing force pushes a protected branch or rewrites its pushed commits.
        self.check_protected(action)?;
        // A fix planned for another state never starts: the sheet shows the new plan.
        if let Action::Fix { expect, .. } = action {
            self.check_fix(expect)?;
        }
        // A file's change that no longer fits, or would conflict, is refused before anything
        // is saved or runs: no failed step in the Operation Log for it.
        if let Action::RevertFile { .. } | Action::CherryPickFile { .. } | Action::ApplySuggestion { .. } = action {
            self.plan(action)?;
        }
        // A commit the secrets check stops never starts: nothing runs, nothing to undo.
        if let Action::Commit {
            allow_secrets: false, ..
        }
        | Action::AddToCommit {
            allow_secrets: false, ..
        } = action
            && crate::secrets::check_secrets()
        {
            let found = crate::console::role("secrets check", || match action {
                // A changelist's commit adds what the list holds, not what is staged.
                Action::Commit {
                    changelist: Some(_), ..
                } => self.changelist_secrets(&self.plan(action)?.commands),
                _ => self.staged_secrets(),
            })?;
            if !found.is_empty() {
                return Err(Error::Secrets(found));
            }
        }
        // Then the checks before commit; one that fails stops it the same way.
        if let Action::Commit { skip_checks: false, .. } | Action::AddToCommit { skip_checks: false, .. } = action {
            self.run_commit_checks(on_event, cancel)?;
        }
        // Loading a key or saving patches changes nothing in the repository. A worktree is a folder: undoing the
        // branch a new one made would pull it from under that folder, and a removed one's files
        // were never in a snapshot. A submodule's folder is another repository, also in no
        // snapshot.
        if let Action::AddSshKey { .. }
        | Action::SavePatches { .. }
        | Action::WorktreeAdd { .. }
        | Action::WorktreeRemove { .. }
        | Action::WorktreeLock { .. }
        | Action::WorktreeUnlock { .. }
        | Action::WorktreePrune
        | Action::WorktreeRepair { .. }
        | Action::SubmoduleUpdate { .. }
        | Action::SubmoduleSync { .. }
        | Action::SubmoduleAdd { .. }
        | Action::SubmoduleRemove { .. } = action
        {
            // Several submodules go one at a time: one that fails doesn't stop the others.
            if let Action::SubmoduleUpdate { paths, latest } = action
                && paths.len() != 1
                && !self.submodules_among(paths).is_empty()
            {
                return self.update_submodules(paths, *latest, on_event, cancel);
            }
            return self.perform_unlogged(action, on_event, cancel);
        }
        // A bisect step only checks out a commit to test, and undoing one would not take back
        // git's notes on the answers; the stash made before it stays in the list until Finish.
        if let Action::BisectStart { .. } | Action::BisectMark { .. } | Action::BisectReset = action {
            let result = self.perform_unlogged(action, on_event, cancel);
            self.forget_bisect_stash();
            return result;
        }
        if let Action::ClearOperationLog = action {
            let out = self.perform_unlogged(action, on_event, cancel)?;
            self.clear_operation_log()?;
            return Ok(out);
        }
        // The state before, for Undo, saved before anything is written: without it the step
        // can't be undone, which is asked about first rather than found out after. A fetch
        // mostly moves branches on remotes, which Undo leaves alone anyway: it never waits.
        let required = !without_undo && !matches!(action, Action::Fetch { .. });
        let before = match crate::console::role(UNDO_SNAPSHOT, || self.snapshot()) {
            Ok(before) => Some(before),
            Err(err) if required => return Err(Error::NoUndo(err.to_string())),
            Err(_) => None,
        };
        if let Action::Restore { id, .. } = action {
            self.prepare_restore(id)?;
        }
        // Copy and Push writes down what it copies first: a copy resolved by hand after a
        // conflict has other changes, and must not be copied again.
        if let Action::ReplayAndPush {
            branch, base, lease, ..
        } = action
        {
            let commits = self.missing_commits(&format!("refs/heads/{branch}"), base, lease)?;
            if !commits.is_empty() {
                self.record_replay(branch, &commits)?;
            }
        }
        // A pick of Copy and Push finished by hand: which commit it became. Aborted: forgotten.
        let replaying = match action {
            Action::Continue { .. } | Action::Abort => self.replay_in_progress(),
            _ => None,
        };
        // Steps of a fix noted for a pick or revert that is over by now, finished elsewhere.
        self.forget_fix_rest();
        if let Action::EditStack { plan } = action {
            self.remember_return(plan);
        }
        let result = self.perform_unlogged(action, on_event, cancel);
        self.forget_return();
        if let Action::Continue { .. } | Action::Abort | Action::Skip = action {
            self.forget_fix_rest();
        }
        if let Some((branch, source, tip)) = &replaying {
            match action {
                // Continue goes on with the picks left and can stop on the next one's conflict
                // after it made this one's copy: the copy is noted either way. Nothing is noted
                // when it stopped before making a commit.
                Action::Continue { .. } => {
                    let noted = self.note_replay_copy(branch, source, tip);
                    if result.is_ok() {
                        noted?;
                    }
                }
                _ if result.is_ok() => self.forget_replay(branch)?,
                _ => {}
            }
        }
        if let Action::Restore { .. } = action {
            // read-tree forgets what git knew about the files on disk.
            let _ = self.run(&GitCommand::new(["update-index", "-q", "--refresh"]));
        }
        // The action ran (or failed part way): a step that can't be written is said apart from
        // the action's own result, which stands either way.
        let recorded = match before {
            Some(before) => crate::console::role(UNDO_SNAPSHOT, || {
                // Staging changes the index only; it never writes a file.
                let after = match action {
                    Action::Stage { .. }
                    | Action::Unstage { .. }
                    | Action::StageChangelist { .. }
                    | Action::StageHunk { .. }
                    | Action::UnstageHunk { .. } => self.snapshot_of_index(&before)?,
                    _ => self.snapshot()?,
                };
                self.record(action, &before, &after, result.is_err())
            })
            .map(|_| ()),
            None => Err(Error::Git("there was no snapshot from before it".into())),
        };
        if let Err(err) = recorded {
            on_event(ActionEvent::NotLogged {
                reason: err.to_string(),
            });
        }
        if let Action::Commit {
            changelist: Some(id), ..
        } = action
        {
            // The commit stands either way: a list left behind is only tidied up later.
            let _ = self.after_changelist_commit(id, result.is_ok());
        }
        result
    }

    fn perform_unlogged(
        &self,
        action: &Action,
        on_event: &mut dyn FnMut(ActionEvent),
        cancel: &AtomicBool,
    ) -> Result<String> {
        if let Action::Commit { message, .. } | Action::Reword { message, .. } = action
            && message.trim().is_empty()
        {
            return Err(Error::Git("the commit message is empty".into()));
        }
        // Each file is a part, with its progress, and one that fails doesn't stop the others.
        if let Action::LfsPull { include, paths } = action {
            return self.lfs_download(include.as_deref(), paths, on_event, cancel);
        }
        let (side, path, header, check, lines) = match action {
            Action::StageHunk {
                path,
                header,
                check,
                lines,
            } => (Side::Unstaged, path, header, check, lines),
            Action::DiscardHunk {
                path,
                header,
                check,
                lines,
            } => (Side::Unstaged, path, header, check, lines),
            Action::UnstageHunk {
                path,
                header,
                check,
                lines,
            } => (Side::Staged, path, header, check, lines),
            _ => {
                if let Action::Resolve {
                    path,
                    picks,
                    text,
                    stages,
                } = action
                {
                    if stages
                        .as_ref()
                        .is_some_and(|s| *s != self.stage_ids(path).unwrap_or_default())
                    {
                        return Err(Error::Git(format!("{path} changed since it was shown; open it again")));
                    }
                    self.write_resolution(path, picks, text.as_deref())?;
                }
                if let Action::Ignore { pattern, .. } = action {
                    self.add_ignore_rule(pattern)?;
                }
                // The note of a conflicted apply goes when the apply is finished or undone.
                let pending = self.pending_apply().is_some()
                    && matches!(action, Action::Continue { .. } | Action::Abort)
                    && self.operation()?.is_some_and(|op| op.kind == OperationKind::StashApply);
                if pending
                    && matches!(action, Action::Abort)
                    && let Some(apply) = self.pending_apply()
                {
                    self.remove_brought_back(&apply)?;
                }
                // Files git will not restore over, so an undo of the apply leaves them be.
                let in_the_way = match action {
                    Action::StashApply { id, .. } => self.untracked_in_the_way(id),
                    _ => Vec::new(),
                };
                let mut last = String::new();
                let mut updated_submodules = false;
                let stash_before = match action {
                    Action::BisectStart { stash: true, .. } => Some(self.stash_tip()),
                    _ => None,
                };
                // A fix, or a Continue of one, that stops on conflicts notes the steps left.
                let fix_start = match action {
                    Action::Fix { .. } => Some(self.head()?.branch),
                    Action::Continue { .. } | Action::Skip if self.has_fix_rest() => Some(None),
                    _ => None,
                };
                let commands = self.plan(action)?.commands;
                for (at, command) in commands.iter().enumerate() {
                    if let Some((ancestor, of)) = &command.only_if_ancestor
                        && !self.is_ancestor(ancestor, of)
                    {
                        continue;
                    }
                    if let Some((name, was)) = &command.only_if_moved
                        && self.rev(name) == *was
                    {
                        continue;
                    }
                    if command.secrets_check {
                        let found = crate::console::role("secrets check", || match action {
                            Action::Fix {
                                fix:
                                    crate::fix::Fix::AddToLast {
                                        allow_secrets: true, ..
                                    },
                                ..
                            } => Ok(Vec::new()),
                            _ if !crate::secrets::check_secrets() => Ok(Vec::new()),
                            // Only the picked files go into the commit: other staged lines don't.
                            Action::Fix {
                                fix: crate::fix::Fix::AddToLast { paths, .. },
                                ..
                            } if !paths.is_empty() => self.staged_secrets_in(&self.amend_paths(paths)?),
                            _ => self.staged_secrets(),
                        })?;
                        if !found.is_empty() {
                            return Err(Error::Secrets(found));
                        }
                        if let Action::Fix {
                            fix: crate::fix::Fix::AddToLast { skip_checks: false, .. },
                            ..
                        } = action
                        {
                            self.run_commit_checks(on_event, cancel)?;
                        }
                    }
                    self.write_todo(command)?;
                    updated_submodules |= command.args.first().is_some_and(|a| a == "submodule");
                    let command = crate::ssh::interactive(command.clone());
                    on_event(ActionEvent::Command {
                        display: command.display(),
                    });
                    // A patch Oxbow made, e.g. one file's change in a commit, goes in on stdin.
                    let ran = crate::console::main(|| match &command.input {
                        Some(input) => self.run_with_input(&command, Some(input.as_bytes())),
                        None => self.run_streaming(&command, &mut |line| on_event(ActionEvent::Line(line)), cancel),
                    });
                    // The files a fix or a bisect put aside whose staged changes no longer fit:
                    // git changed nothing and kept the stash. Say so rather than bring them back
                    // unstaged.
                    if ran.is_err()
                        && command
                            .args
                            .starts_with(&["stash".to_owned(), "pop".to_owned(), "--index".to_owned()])
                        && (fix_start.is_some() || matches!(action, Action::Abort | Action::BisectReset))
                        && self.conflicted_paths().is_ok_and(|p| p.is_empty())
                    {
                        return Err(Error::StashKept);
                    }
                    let out = ran.inspect_err(|_| {
                        if let Some(start) = &fix_start {
                            self.note_fix_rest(action, start.clone(), &commands, at);
                        }
                        if let Action::Merge {
                            branch,
                            method: MergeMethod::Squash,
                            message,
                        } = action
                        {
                            self.remember_squash(branch, message.as_deref());
                        }
                        if let Action::StashApply { id, pop, .. } = action
                            && self.conflicted_paths().is_ok_and(|p| !p.is_empty())
                        {
                            self.remember_apply(id, *pop, &in_the_way);
                        }
                    })?;
                    if let Some(before) = &stash_before
                        && command.args.first().is_some_and(|a| a == "stash")
                    {
                        self.note_bisect_stash(before.as_deref());
                    }
                    // GitLab says where the merge request is on stderr, beside git's own words.
                    last = if matches!(
                        action,
                        Action::Push {
                            merge_request: Some(_),
                            ..
                        }
                    ) {
                        format!("{}{}", out.stderr, out.stdout)
                    } else if out.stdout.trim().is_empty() {
                        out.stderr
                    } else {
                        out.stdout
                    };
                }
                // The pull brought the first submodules, which the last fetch didn't know of.
                if let Action::Pull { submodules: true, .. } = action
                    && !updated_submodules
                    && !self.submodule_list().unwrap_or_default().is_empty()
                {
                    let command = crate::ssh::interactive(crate::submodules::submodule_update());
                    on_event(ActionEvent::Command {
                        display: command.display(),
                    });
                    crate::console::main(|| {
                        self.run_streaming(&command, &mut |line| on_event(ActionEvent::Line(line)), cancel)
                    })?;
                }
                if pending {
                    self.forget_apply();
                }
                self.after_tags(action);
                return Ok(last);
            }
        };
        // Staging goes forward; unstaging and discarding apply the patch in reverse.
        let forward = matches!(action, Action::StageHunk { .. });
        let patch = self.hunk_patch(path, side, header, check, lines.as_deref(), forward)?;
        let command = &self.plan(action)?.commands[0];
        on_event(ActionEvent::Command {
            display: command.display(),
        });
        let out = crate::console::main(|| self.run_with_input(command, Some(patch.as_bytes())))?;
        Ok(out.stdout)
    }

    /// Add `pattern` as a line at the end of the top `.gitignore`, creating it if needed.
    fn add_ignore_rule(&self, pattern: &str) -> Result<()> {
        let pattern = pattern.trim_end_matches(['\r', '\n']);
        if pattern.trim().is_empty() || pattern.contains('\n') {
            return Err(Error::Git("an ignore rule is one line of text".into()));
        }
        let file = self.workdir().join(".gitignore");
        // Bytes, not text: git reads a `.gitignore` that isn't UTF-8 just fine, and its rules
        // must stay. Only a missing file starts empty; any other read error stops here. Like
        // git, never through a symlink: a repository could commit one that points at a file of
        // yours outside it, and the rule would be written there.
        let mut text = match std::fs::symlink_metadata(&file) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(Error::Git(format!("could not read .gitignore: {err}"))),
            Ok(meta) if !meta.is_file() => {
                return Err(Error::Git(
                    ".gitignore is a link or a folder, not a plain file; Oxbow leaves it alone".into(),
                ));
            }
            Ok(_) => match crate::lfs::read_regular(&file, MAX_GITIGNORE_BYTES + 1) {
                Some(bytes) if bytes.len() as u64 <= MAX_GITIGNORE_BYTES => bytes,
                Some(_) => return Err(Error::Git(".gitignore is too big to add a rule to here".into())),
                None => return Err(Error::Git("could not read .gitignore as a plain file".into())),
            },
        };
        let line = |l: &[u8]| l.strip_suffix(b"\r").unwrap_or(l) == pattern.as_bytes();
        if text.split(|&c| c == b'\n').any(line) {
            return Ok(());
        }
        if !text.is_empty() && !text.ends_with(b"\n") {
            text.push(b'\n');
        }
        text.extend_from_slice(pattern.as_bytes());
        text.push(b'\n');
        // A new file renamed over the old one: a rename replaces a symlink put there in the
        // meantime instead of writing through it.
        let fresh = self.workdir().join(format!(".gitignore.oxbow-{}", std::process::id()));
        let written = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&fresh)
            .and_then(|mut f| {
                std::io::Write::write_all(&mut f, &text)?;
                // The file keeps its permissions.
                match std::fs::symlink_metadata(&file) {
                    Ok(meta) if meta.is_file() => f.set_permissions(meta.permissions()),
                    _ => Ok(()),
                }
            })
            .and_then(|()| std::fs::rename(&fresh, &file));
        if written.is_err() {
            let _ = std::fs::remove_file(&fresh);
        }
        written.map_err(|err| Error::Git(format!("could not write .gitignore: {err}")))
    }
}
