//! Turning an [`Action`] into the git commands it runs ([`Repo::plan`]): what the
//! confirmation sheet shows is exactly what [`Repo::perform`] runs.

//! The working copy: what changed since the last commit, staging and committing.
//!
//! Status and diffs come from the `git` command line, so they honor every setting that affects
//! them (ignore rules, attributes, line endings, fsmonitor), and a hunk shown on screen is
//! exactly the hunk `git apply` gets when it is staged. Every change to the repository is an
//! [`Action`] that turns into the [`GitCommand`]s shown in the confirmation sheet.

use crate::action::{Action, Plan, PullMode};
use crate::cli::{GitCommand, literal};
use crate::commit::FileStatus;
use crate::config;
use crate::edit::plan_reset;
use crate::error::{Error, Result};
use crate::lfs;
use crate::repo::Repo;
use crate::tags;
use crate::worktree::Side;

/// Put uncommitted changes, new files included, aside before switching to `branch`.
pub(crate) fn stash_before(branch: &str) -> GitCommand {
    GitCommand::new([
        "stash".to_owned(),
        "push".to_owned(),
        "--include-untracked".to_owned(),
        "-m".to_owned(),
        format!("Before switching to {branch}"),
    ])
    .comment("keeps your uncommitted files in the stash, new ones too")
}

/// What a commit or push says when Settings turned hooks off.
pub(crate) const HOOKS_OFF: &str = "--no-verify: Git hooks are turned off in Settings";

/// `main`, or `local:remote` when the branch has another name on the remote.
pub(crate) fn refspec(branch: &str, upstream: &str) -> String {
    if branch == upstream {
        branch.to_owned()
    } else {
        format!("{branch}:{upstream}")
    }
}

pub(crate) fn stdin_note(path: &str, header: &str) -> String {
    format!("the patch of the {header} hunk of {path} comes on stdin")
}

/// A message split at its blank lines, each paragraph trimmed.
pub(crate) fn paragraphs(message: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in message.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(current.join("\n").trim().to_owned());
                current.clear();
            }
        } else {
            current.push(line.trim_end());
        }
    }
    if !current.is_empty() {
        out.push(current.join("\n").trim().to_owned());
    }
    out
}

impl Repo {
    /// The commands `action` will run.
    pub fn plan(&self, action: &Action) -> Result<Plan> {
        self.check_protected(action)?;
        let paths = |args: &[&str], paths: &[String]| {
            GitCommand::new(
                args.iter()
                    .map(|s| s.to_string())
                    .chain(["--".to_owned()])
                    .chain(paths.iter().map(|p| literal(p))),
            )
        };
        let mut steps = Vec::new();
        let commands = match action {
            Action::Stage { paths: p } => vec![paths(&["add"], p).comment("add the files to the next commit")],
            Action::Unstage { paths: p } => {
                if self.head()?.commit.is_some() {
                    // A rename is a deletion of the old path too: unstage both halves.
                    let tree = self.working_tree()?;
                    let mut p = p.clone();
                    for file in &tree.staged {
                        if let Some(old) = file.old_path.as_ref().filter(|_| p.contains(&file.path))
                            && !p.contains(old)
                        {
                            p.push(old.clone());
                        }
                    }
                    vec![
                        paths(&["restore", "--staged"], &p).comment("take them out of the next commit, keep the edits"),
                    ]
                } else {
                    vec![
                        paths(&["rm", "--cached", "-r", "--quiet"], p)
                            .comment("nothing is committed yet, so just unstage"),
                    ]
                }
            }
            Action::Discard { paths: p } => {
                let tree = self.working_tree()?;
                let untracked: Vec<String> = p
                    .iter()
                    .filter(|path| {
                        tree.unstaged
                            .iter()
                            .any(|f| &f.path == *path && f.status == FileStatus::Untracked)
                    })
                    .cloned()
                    .collect();
                let tracked: Vec<String> = p.iter().filter(|path| !untracked.contains(path)).cloned().collect();
                let mut commands = Vec::new();
                if !tracked.is_empty() {
                    commands.push(
                        paths(&["restore", "--worktree"], &tracked).comment("put back the staged or committed version"),
                    );
                }
                if !untracked.is_empty() {
                    commands
                        .push(paths(&["clean", "-f"], &untracked).comment("delete the new files git does not track"));
                }
                commands
            }
            Action::StageHunk {
                path,
                header,
                check,
                lines,
            }
            | Action::UnstageHunk {
                path,
                header,
                check,
                lines,
            }
            | Action::DiscardHunk {
                path,
                header,
                check,
                lines,
            } => {
                let (side, args): (_, &[&str]) = match action {
                    Action::StageHunk { .. } => (Side::Unstaged, &["apply", "--cached", "-"]),
                    Action::UnstageHunk { .. } => (Side::Staged, &["apply", "--cached", "--reverse", "-"]),
                    _ => (Side::Unstaged, &["apply", "--reverse", "-"]),
                };
                let cmd = GitCommand::new(args.iter().copied());
                match lines {
                    None => vec![cmd.comment(stdin_note(path, header))],
                    Some(lines) => {
                        let forward = matches!(action, Action::StageHunk { .. });
                        let patch = self.hunk_patch(path, side, header, check, Some(lines), forward)?;
                        let what = match action {
                            Action::StageHunk { .. } => {
                                "--cached: only the staging area changes, your file stays as is. By hand: git add -p, then e"
                            }
                            Action::UnstageHunk { .. } => "--reverse: take these lines back out of the staging area",
                            _ => "--reverse: undo just these lines in the file on disk",
                        };
                        vec![cmd.comment(what).input(patch)]
                    }
                }
            }
            Action::StageChangelist { list, paths } => self.plan_list_part(list.as_deref(), paths, false)?,
            Action::DiscardChangelist { list, paths } => self.plan_list_part(list.as_deref(), paths, true)?,
            Action::Ignore { pattern, untrack } => {
                let mut commands = Vec::new();
                if let Some(path) = untrack {
                    commands.push(
                        GitCommand::new(["rm", "-r", "--cached", "--quiet", "--", &literal(path)])
                            .comment("stop tracking it: it stays on disk, and leaves the repository with the next commit"),
                    );
                }
                commands.push(
                    GitCommand::new(["add", "--", ".gitignore"])
                        .comment("stage .gitignore, so the rule reaches everyone with the next commit")
                        .before(format!("echo {} >> .gitignore", crate::cli::shell_quote(pattern))),
                );
                commands
            }
            Action::Commit {
                message,
                amend,
                no_verify,
                sign_off,
                skip_checks,
                changelist,
                ..
            } => {
                let mut args = vec!["commit".to_owned()];
                if *amend {
                    args.push("--amend".to_owned());
                }
                if *no_verify || !config::run_hooks() {
                    args.push("--no-verify".to_owned());
                }
                if *sign_off {
                    args.push("--signoff".to_owned());
                }
                // One -m per paragraph, as people type it; git puts the blank lines back between them.
                for paragraph in paragraphs(message) {
                    args.push("-m".to_owned());
                    args.push(paragraph);
                }
                let mut cmd = GitCommand::new(args);
                if !skip_checks {
                    cmd.before = self.checks_before();
                }
                if let Some(id) = changelist {
                    let cmd = if *amend {
                        cmd.comment("replace the last commit with one that also has the changes of the list")
                    } else if *no_verify {
                        cmd.comment("--no-verify: skip the pre-commit and commit-msg hooks this once")
                    } else if !config::run_hooks() {
                        cmd.comment(HOOKS_OFF)
                    } else {
                        cmd.comment("commit what that staging area has")
                    };
                    self.plan_changelist_commit(id, cmd)?
                } else {
                    vec![if *amend {
                    cmd.comment("replace the last commit with one that also has the staged changes")
                } else if *no_verify {
                    cmd.comment("--no-verify: skip the pre-commit and commit-msg hooks this once")
                } else if !config::run_hooks() {
                    cmd.comment(HOOKS_OFF)
                } else if *sign_off {
                    cmd.comment("--signoff: the message ends with Signed-off-by and your name and email")
                } else {
                    cmd
                }]
                }
            }
            Action::Fetch { remote } => vec![self.fetch_command(remote.as_deref()).with_progress()],
            Action::Pull {
                remote,
                branch,
                fetch_first,
                submodules,
                mode,
            } => {
                let mut plan = Vec::new();
                if *fetch_first {
                    plan.push(self.fetch_command(Some(remote)).with_progress());
                }
                plan.push(self.pull_command(remote, branch, *mode));
                if *submodules {
                    plan.extend(self.submodules_after_pull(remote, branch));
                }
                plan
            }
            Action::Push {
                remote,
                branch,
                upstream,
                set_upstream,
                force,
                lease,
                no_verify,
                submodules,
                merge_request,
            } => {
                let mut args = vec!["push".to_owned()];
                let mut notes = Vec::new();
                if *submodules {
                    args.push("--recurse-submodules=on-demand".to_owned());
                    notes.push("--recurse-submodules=on-demand: push the submodule commits it records first".to_owned());
                }
                if *no_verify {
                    args.push("--no-verify".to_owned());
                    notes.push("--no-verify: skip the pre-push hook this one time".to_owned());
                } else if !config::run_hooks() {
                    args.push("--no-verify".to_owned());
                    notes.push(HOOKS_OFF.to_owned());
                }
                if *set_upstream {
                    args.push("-u".to_owned());
                    notes.push(format!("-u: remember {remote}/{upstream} as the upstream"));
                }
                match (force, lease) {
                    (true, Some(lease)) => {
                        args.push(format!("--force-with-lease={upstream}:{lease}"));
                        notes.push(format!(
                            "--force-with-lease: overwrite only if {remote}/{upstream} is still {}",
                            &lease[..lease.len().min(7)]
                        ));
                    }
                    (true, None) => {
                        args.push("--force".to_owned());
                        notes.push("--force: overwrite whatever the remote has".to_owned());
                    }
                    _ => {}
                }
                for (option, note) in merge_request.iter().flat_map(|m| m.push_options()) {
                    args.extend(["-o".to_owned(), option]);
                    if !note.is_empty() {
                        notes.push(note);
                    }
                }
                args.push(remote.clone());
                args.push(refspec(branch, upstream));
                if notes.is_empty() {
                    notes.push(format!("sends the commits {remote}/{upstream} does not have yet"));
                }
                vec![GitCommand::new(args).comment(notes.join("; ")).with_progress()]
            }
            Action::PullAndPush {
                remote,
                branch,
                upstream,
                no_verify,
            } => {
                let mut push = vec!["push".to_owned()];
                if *no_verify || !config::run_hooks() {
                    push.push("--no-verify".to_owned());
                }
                push.extend([remote.clone(), refspec(branch, upstream)]);
                vec![
                    self.pull_command(remote, upstream, None),
                    GitCommand::new(push)
                        .comment("then send your commits on top")
                        .with_progress(),
                ]
            }
            Action::ReplayAndPush {
                remote,
                branch,
                upstream,
                base,
                lease,
                no_verify,
            } => self.plan_replay_and_push(remote, branch, upstream, base, lease, *no_verify)?,
            Action::AbortRebase => {
                vec![GitCommand::new(["rebase", "--abort"]).comment("back to where the branch was before the pull")]
            }
            Action::Switch { branch, stash, keep } => {
                let mut commands = Vec::new();
                if let Some(name) = keep {
                    commands.push(
                        GitCommand::new(["branch", name])
                            .comment(format!("{name} keeps the commits made without a branch")),
                    );
                }
                if *stash {
                    commands.push(stash_before(branch));
                }
                commands.push(
                    GitCommand::new(["switch", branch]).comment("switch: the modern form of \"git checkout <branch>\""),
                );
                commands
            }
            Action::Detach { commit, stash } => {
                // The stash message reads better with a short sha; the command takes the whole
                // one, since seven characters can match more than one object.
                let full_sha = commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit());
                let short = if full_sha { &commit[..7] } else { commit.as_str() };
                let mut commands = Vec::new();
                if *stash {
                    commands.push(stash_before(short));
                }
                commands.push(
                    GitCommand::new(["switch", "--detach", commit.as_str()])
                        .comment("--detach: HEAD points at a commit, not at a branch"),
                );
                commands
            }
            Action::Track { remote, branch, stash } => {
                let mut commands = Vec::new();
                if *stash {
                    commands.push(stash_before(branch));
                }
                commands.push(
                    GitCommand::new(["switch".to_owned(), "--track".to_owned(), format!("{remote}/{branch}")])
                        .comment(format!("--track: create {branch} that follows {remote}/{branch}")),
                );
                commands
            }
            Action::CreateBranch {
                name,
                start,
                switch,
                publish,
            } => {
                let at = start
                    .as_deref()
                    .map_or("the current commit".to_owned(), |s| s.to_owned());
                let mut args = if *switch {
                    vec!["switch".to_owned(), "-c".to_owned(), name.clone()]
                } else {
                    vec!["branch".to_owned(), name.clone()]
                };
                args.extend(start.iter().cloned());
                let head = self.head()?.branch;
                let mut commands = vec![GitCommand::new(args).comment(if *switch {
                    format!("-c: create the branch at {at}, then switch to it")
                } else {
                    format!(
                        "creates the branch at {at}, you stay on {}",
                        head.as_deref().unwrap_or("the current commit")
                    )
                })];
                if let Some(remote) = publish {
                    commands.push(
                        GitCommand::new(["push", "-u", remote, name])
                            .comment(format!("-u: remember {remote}/{name} as its upstream"))
                            .with_progress(),
                    );
                }
                commands
            }
            Action::RenameBranch { from, to, upstream } => {
                let mut commands =
                    vec![GitCommand::new(["branch", "-m", from, to]).comment("-m: move, which renames the branch")];
                if let Some(up) = upstream {
                    commands.push(
                        GitCommand::new(["push", "-u", &up.remote, to])
                            .comment(format!("publish the new name and make {}/{to} the upstream", up.remote))
                            .with_progress(),
                    );
                    commands.push(
                        GitCommand::new([
                            "push".to_owned(),
                            up.remote.clone(),
                            "--delete".to_owned(),
                            up.branch.clone(),
                        ])
                        .comment("then remove the old name there")
                        .with_progress(),
                    );
                }
                commands
            }
            Action::DeleteBranch { name, force, upstream } => {
                let mut commands = vec![if *force {
                    GitCommand::new(["branch", "-D", name])
                        .comment("-D: delete even though the commits are not merged anywhere")
                } else {
                    GitCommand::new(["branch", "-d", name]).comment("-d: deletes only when no work would be lost")
                }];
                if let Some(up) = upstream {
                    commands.push(
                        GitCommand::new([
                            "push".to_owned(),
                            up.remote.clone(),
                            "--delete".to_owned(),
                            up.branch.clone(),
                        ])
                        .comment(format!("removes {}/{} for everyone", up.remote, up.branch))
                        .with_progress(),
                    );
                }
                commands
            }
            Action::DeleteRemoteBranch {
                remote,
                branch,
                expect: None,
            } => vec![
                GitCommand::new(["push", remote, "--delete", branch])
                    .comment("--delete: remove the branch on the remote")
                    .with_progress(),
            ],
            Action::DeleteRemoteBranch {
                remote,
                branch,
                expect: Some(sha),
            } => vec![
                GitCommand::new([
                    "push".to_owned(),
                    format!("--force-with-lease=refs/heads/{branch}:{sha}"),
                    remote.clone(),
                    "--delete".to_owned(),
                    branch.clone(),
                ])
                .comment(format!(
                    "--force-with-lease: only while {branch} there is still the commit that was merged, so newer pushes stay"
                ))
                .with_progress(),
            ],
            Action::Merge {
                branch,
                method,
                message,
            } => self.plan_merge(branch, *method, message.as_deref())?,
            Action::Continue { message } => self.plan_continue(message.as_deref())?,
            Action::Abort => self.plan_abort()?,
            Action::Skip => self.plan_skip()?,
            Action::Resolve { path, .. } => vec![GitCommand::new(["add", "--", &literal(path)]).comment(format!(
                "Oxbow writes your choices into {path} first; add marks it resolved"
            ))],
            Action::TakeFile { path, side } => self.plan_take_file(path, *side)?,
            Action::CherryPick { commit } => self.plan_cherry_pick(commit)?,
            Action::SavePatches {
                to,
                since,
                count,
                into,
                one_file,
            } => self.plan_save_patches(to, since.as_deref(), *count, into, *one_file)?,
            Action::ApplyPatches { paths } => self.plan_apply_patches(paths)?,
            Action::Revert { commit } => self.plan_revert(commit)?,
            Action::RevertFile { commit, path, old_path } => {
                self.plan_file_patch(commit, path, old_path.as_deref(), true)?
            }
            Action::CherryPickFile { commit, path, old_path } => {
                self.plan_file_patch(commit, path, old_path.as_deref(), false)?
            }
            Action::TakeVersion { commit, path } => self.plan_take_version(commit, path)?,
            Action::RestoreLocalVersion { path, id } => self.plan_restore_local(path, id)?,
            Action::ApplySuggestion {
                commit,
                path,
                start,
                end,
                lines,
            } => self.plan_apply_suggestion(commit, path, *start, *end, lines)?,
            Action::Reset { commit, mode } => plan_reset(commit, *mode),
            Action::StashPush {
                message,
                paths,
                staged: true,
                ..
            } => {
                if !config::git_at_least(2, 35) {
                    return Err(Error::Git(
                        "Stash Staged needs git 2.35 or later; update git, or stash the files instead".into(),
                    ));
                }
                let mut args = vec!["stash".to_owned(), "push".to_owned(), "--staged".to_owned()];
                if let Some(message) = message.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
                    args.extend(["-m".to_owned(), message.to_owned()]);
                }
                if !paths.is_empty() {
                    args.push("--".to_owned());
                    args.extend(paths.iter().map(|p| literal(p)));
                }
                vec![GitCommand::new(args).comment("--staged: only what is staged goes into the stash; unstaged changes stay")]
            }
            Action::StashHunk {
                path,
                header,
                check,
                lines,
                message,
            } => {
                let lines = lines.as_deref();
                let forward = self.hunk_patch(path, Side::Unstaged, header, check, lines, true)?;
                let back = self.hunk_patch(path, Side::Unstaged, header, check, lines, false)?;
                let (stash, message) = self.stash_commit_of(&forward, message.as_deref())?;
                vec![
                    GitCommand::new(["stash", "store", "-m", &message, &stash])
                        .comment("a stash Oxbow made of these lines on the last commit joins the list"),
                    GitCommand::new(["apply", "--reverse", "-"])
                        .comment("--reverse: take just these lines out of the file on disk")
                        .input(back),
                ]
            }
            Action::StashPush {
                message,
                untracked,
                paths,
                ..
            } => {
                let mut args = vec!["stash".to_owned(), "push".to_owned()];
                if *untracked {
                    args.push("--include-untracked".to_owned());
                }
                if let Some(message) = message.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
                    args.extend(["-m".to_owned(), message.to_owned()]);
                }
                if !paths.is_empty() {
                    args.push("--".to_owned());
                    args.extend(paths.iter().map(|p| literal(p)));
                }
                let cmd = GitCommand::new(args);
                vec![match (paths.is_empty(), *untracked) {
                    (false, true) => cmd.comment("-- then paths: only these files; --include-untracked: new ones too"),
                    (false, false) => cmd.comment("-- then paths: only these files, the rest stays as it is"),
                    (true, true) => cmd.comment("--include-untracked: new files are stashed too"),
                    (true, false) => cmd.comment("new, untracked files stay where they are"),
                }]
            }
            Action::StashApply {
                index,
                id,
                pop,
                keep_index,
            } => {
                let name = self.stash_ref(*index, id)?;
                let mut args = vec!["stash", if *pop { "pop" } else { "apply" }];
                if *keep_index {
                    args.push("--index");
                }
                args.push(&name);
                let comment = match (*pop, *keep_index) {
                    (true, true) => "pop: apply, then delete the stash; --index: staged files come back staged",
                    (true, false) => "pop: apply, then delete the stash",
                    (false, true) => "the stash stays in the list; --index: staged files come back staged",
                    (false, false) => "the stash stays in the list",
                };
                vec![GitCommand::new(args).comment(comment)]
            }
            Action::StashDrop { index, id } => vec![
                GitCommand::new(["stash".to_owned(), "drop".to_owned(), self.stash_ref(*index, id)?])
                    .comment("later stashes move up one number"),
            ],
            Action::StashStore { id, message } => vec![
                GitCommand::new(["stash", "store", "-m", message, id])
                    .comment("puts the dropped stash back as stash@{0}"),
            ],
            Action::StashBranch { index, id, name } => vec![
                GitCommand::new([
                    "stash".to_owned(),
                    "branch".to_owned(),
                    name.clone(),
                    self.stash_ref(*index, id)?,
                ])
                .comment("a new branch where the stash was made, with the stash applied and dropped"),
            ],
            Action::AddRemote { name, url } => vec![
                GitCommand::new(["remote", "add", name, url])
                    .comment(format!("Fetch shows {name}’s branches as {name}/…")),
            ],
            Action::SetRemoteUrl { name, url } => vec![
                GitCommand::new(["remote", "set-url", name, url]).comment("fetch and push both use the new address"),
            ],
            Action::AddSshKey { key } => vec![crate::ssh::add_key_command(key)],
            Action::RemoveRemote { name } => vec![
                GitCommand::new(["remote", "remove", name])
                    .comment(format!("{name}/… branches go too; local branches stay")),
            ],
            Action::CreateTag {
                name,
                commit,
                message,
                push,
            } => tags::plan_create(name, commit, message.as_deref(), push.as_deref()),
            Action::PushTags { remote, names } => vec![tags::push_command(remote, names)],
            Action::DeleteTag { name, remote } => tags::plan_delete(name, remote.as_deref()),
            Action::FetchTags { remote } => tags::plan_fetch(remote),
            Action::Unshallow { remote } => vec![
                GitCommand::new(["fetch", "--unshallow", remote.as_str()])
                    .comment("--unshallow: fetch the commits a shallow clone left out")
                    .with_progress(),
            ],
            Action::Optimize => {
                vec![GitCommand::new(["gc"]).comment("packs loose objects and drops ones nothing points at any more")]
            }
            Action::Restore { id, after } => self.plan_restore(id, *after)?,
            Action::ClearOperationLog => vec![
                GitCommand::new(["update-ref", "-d", crate::oplog::OPLOG_REF])
                    .comment("the snapshots go with git's next clean-up"),
            ],
            Action::EditStack { plan } => self.plan_edit_stack(plan)?,
            Action::AddToCommit { commit, skip_checks, .. } => {
                let mut commands = self.plan_add_to_commit(commit)?;
                // The checks run before anything: shown before the first command.
                if !skip_checks && let Some(first) = commands.first_mut().filter(|c| c.before.is_none()) {
                    first.before = self.checks_before();
                }
                commands
            }
            Action::PushBranches { remote, branches } => {
                let mut args = vec!["push".to_owned(), "--force-with-lease".to_owned()];
                if !config::run_hooks() {
                    args.push("--no-verify".to_owned());
                }
                args.push(remote.clone());
                args.extend(branches.iter().map(|b| refspec(&b.branch, &b.upstream)));
                let comment = format!(
                    "--force-with-lease: each one only if {remote} still has what was fetched last, so nobody else’s new commits get overwritten"
                );
                vec![GitCommand::new(args).comment(comment).with_progress()]
            }
            Action::PullRequestMerged { remote, base, .. } => self.plan_pull_request_merged(remote, base)?,
            Action::DeleteMergedBranch { base, branch, sha } => self.plan_delete_merged_branch(base, branch, sha)?,
            Action::LfsTrack { pattern, paths, stage } => lfs::plan_track(pattern, paths, *stage),
            Action::LfsUntrack { pattern } => lfs::plan_untrack(pattern),
            Action::LfsPull { include, paths } => lfs::plan_pull(include.as_deref(), paths),
            Action::LfsPrune => lfs::plan_prune(),
            Action::LfsInstall { brew, pull } => lfs::plan_install(brew.as_deref(), *pull),
            Action::BisectStart {
                good,
                bad,
                stash,
                command,
            } => self.plan_bisect_start(good, bad, *stash, command.as_deref()),
            Action::BisectMark { mark, commit } => self.plan_bisect_mark(*mark, commit.as_deref())?,
            Action::BisectReset => self.plan_bisect_reset()?,
            Action::WorktreeAdd {
                path,
                branch,
                create,
                start,
            } => self.plan_worktree_add(path, branch, *create, start.as_deref()),
            Action::WorktreeRemove { path, force, stash } => self.plan_worktree_remove(path, *force, *stash)?,
            Action::WorktreeLock { path, reason } => self.plan_worktree_lock(path, reason.as_deref()),
            Action::WorktreeUnlock { path } => self.plan_worktree_unlock(path),
            Action::WorktreePrune => self.plan_worktree_prune(),
            Action::WorktreeRepair { path } => self.plan_worktree_repair(path),
            Action::SubmoduleUpdate { paths, latest } => self.plan_submodule_update(paths, *latest),
            Action::SubmoduleSync { paths } => self.plan_submodule_sync(paths),
            Action::SubmoduleAdd { url, path, branch } => self.plan_submodule_add(url, path, branch.as_deref()),
            Action::SubmoduleRemove { path } => self.plan_submodule_remove(path),
            Action::Fix { fix, .. } => {
                let plan = self.plan_fix(fix)?;
                steps = plan.steps;
                plan.commands
            }
            Action::Reword { message, no_verify } => {
                let mut args = vec!["commit".to_owned(), "--amend".to_owned(), "--only".to_owned()];
                // The same hooks rule as Commit: off in Settings, or skipped this once.
                let skip = *no_verify || !config::run_hooks();
                if skip {
                    args.push("--no-verify".to_owned());
                }
                for paragraph in paragraphs(message) {
                    args.push("-m".to_owned());
                    args.push(paragraph);
                }
                let cmd = GitCommand::new(args);
                vec![if *no_verify {
                    cmd.comment("--only: just the message, staged changes stay staged; --no-verify: skip the pre-commit and commit-msg hooks this once")
                } else if skip {
                    cmd.comment(format!("--only: just the message; staged changes stay staged. {HOOKS_OFF}"))
                } else {
                    cmd.comment("--only: just the message; staged changes stay staged")
                }]
            }
        };
        Ok(Plan {
            commands,
            not_kept: Vec::new(),
            steps,
        })
    }

    pub fn pull_mode(&self) -> PullMode {
        if self.config_value("pull.ff").is_some_and(|ff| ff == "only") {
            PullMode::FastForward
        } else if self.config_bool("pull.rebase") == Some(false) {
            PullMode::Merge
        } else {
            PullMode::Rebase
        }
    }

    /// Whether Pull puts uncommitted changes aside and back (`rebase.autoStash`, on by default).
    pub fn pull_autostash(&self) -> bool {
        self.config_bool("rebase.autoStash").unwrap_or(true)
    }

    /// `git fetch` of one remote or all, pruning unless `fetch.prune` is turned off.
    pub fn fetch_command(&self, remote: Option<&str>) -> GitCommand {
        let prune = self.config_bool("fetch.prune").unwrap_or(true);
        let mut args = vec!["fetch"];
        if prune {
            args.push("--prune");
        }
        args.push(remote.unwrap_or("--all"));
        let comment = match (remote, prune) {
            (Some(remote), true) => format!("--prune: drop {remote}/* branches deleted on the remote"),
            (Some(remote), false) => format!("gets what is new on {remote}"),
            (None, true) => "--all: every remote, --prune: drop branches deleted there".to_owned(),
            (None, false) => "--all: every remote".to_owned(),
        };
        GitCommand::new(args).comment(comment)
    }

    /// `git pull` the way Settings › Git says: rebase (the default), merge or fast-forward only,
    /// read from `pull.rebase` and `pull.ff` so it matches a terminal.
    fn plan_replay_and_push(
        &self,
        remote: &str,
        branch: &str,
        upstream: &str,
        base: &str,
        lease: &str,
        no_verify: bool,
    ) -> Result<Vec<GitCommand>> {
        if !self.is_ancestor(base, lease) || self.has_merges(&format!("{base}..{lease}"))? {
            return Err(Error::Git(format!(
                "{remote}/{upstream} was rewritten or merged into since your last push; push with force or sort it out by hand"
            )));
        }
        let mut commands = Vec::new();
        let commits = self.missing_commits(&format!("refs/heads/{branch}"), base, lease)?;
        if !commits.is_empty() {
            let mut people: Vec<&str> = Vec::new();
            for c in &commits {
                if !people.contains(&c.author_name.as_str()) {
                    people.push(&c.author_name);
                }
            }
            let mut args = vec!["cherry-pick".to_owned()];
            args.extend(commits.iter().map(|c| c.id.clone()));
            commands.push(GitCommand::new(args).comment(format!(
                "copy {} by {} onto your rewritten {branch}; they keep their author",
                if commits.len() == 1 {
                    "the commit".to_owned()
                } else {
                    format!("the {} commits", commits.len())
                },
                people.join(", ")
            )));
        }
        let mut push = vec!["push".to_owned()];
        if no_verify || !config::run_hooks() {
            push.push("--no-verify".to_owned());
        }
        push.push(format!("--force-with-lease={upstream}:{lease}"));
        push.extend([remote.to_owned(), refspec(branch, upstream)]);
        commands.push(
            GitCommand::new(push)
                .comment(format!(
                    "--force-with-lease: replace the old versions only if {remote}/{upstream} is still {}",
                    &lease[..lease.len().min(7)]
                ))
                .with_progress(),
        );
        Ok(commands)
    }

    /// `mode` is the one picked for this pull; without it, the repository's setting.
    fn pull_command(&self, remote: &str, branch: &str, mode: Option<PullMode>) -> GitCommand {
        let (mode, note) = match mode.unwrap_or_else(|| self.pull_mode()) {
            PullMode::FastForward => ("--ff-only", "--ff-only: only if your branch has no commits of its own"),
            PullMode::Merge => (
                "--no-rebase",
                "--no-rebase: a merge commit joins the remote's commits with yours",
            ),
            PullMode::Rebase => ("--rebase", "--rebase: no merge commit, your commits go on top"),
        };
        let mut args = vec!["pull", mode];
        let mut comment = note.to_owned();
        if self.pull_autostash() {
            args.push("--autostash");
            comment.push_str("; --autostash: keep uncommitted files");
        }
        args.extend([remote, branch]);
        GitCommand::new(args).comment(comment).with_progress()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_messages_split_into_paragraphs() {
        assert_eq!(
            paragraphs("  Summary\n\n\nFirst line\nsecond line  \n\n- item\n"),
            ["Summary", "First line\nsecond line", "- item"]
        );
        assert!(paragraphs(" \n ").is_empty());
    }
}
