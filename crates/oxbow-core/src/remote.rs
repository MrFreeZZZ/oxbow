//! Talking to remotes: where the checked-out branch is tracked, and what went wrong when a
//! fetch, pull or push failed.

use serde::Serialize;

use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::repo::Repo;
use crate::worktree::Action;

/// The upstream of the checked-out branch and how far apart the two are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tracking {
    /// Remote the upstream lives on, e.g. `origin`.
    pub remote: String,
    /// Branch name on the remote, e.g. `main`.
    pub branch: String,
    /// Commits on the local branch that the upstream does not have.
    pub ahead: u32,
    /// Commits on the upstream that the local branch does not have, as of the last fetch.
    pub behind: u32,
    /// The upstream branch was deleted on the remote.
    pub gone: bool,
    /// The upstream's tip when every commit it has that the branch lacks was on this branch
    /// before: an edited message, a squash, a drop or Edit Stack rewrote pushed commits. Push
    /// then replaces them with `--force-with-lease` at this tip; a pull would bring them back.
    /// Only for the checked-out branch.
    pub rewritten: Option<String>,
    /// With `rewritten`: commits someone pushed on top of the old versions, which the branch
    /// does not have yet. Push copies them onto the branch first, so neither their work nor the
    /// rewrite is lost; a pull with rebase would drop an edited message as already upstream.
    pub replay: Option<Replay>,
}

/// Commits to copy onto a rewritten branch before it replaces its old versions on the remote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Replay {
    /// The remote branch as this clone last pushed it: the old versions end here.
    pub base: String,
    /// What others pushed after it that the branch has no copy of, oldest first.
    pub commits: Vec<CommitBrief>,
}

/// Why a fetch, pull or push failed, so the app can offer the way out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    /// The remote has commits the local branch does not have.
    Rejected,
    /// `--force-with-lease` refused: the remote moved since it was looked at.
    StaleLease,
    /// The remote did not accept the credentials, or there were none.
    Auth,
    /// The remote could not be reached.
    Network,
    /// A local hook (pre-push, pre-commit, commit-msg) stopped the command.
    Hook,
    /// A pull stopped on conflicts.
    Conflict,
    /// Switching branches would overwrite uncommitted changes.
    LocalChanges,
    /// `git branch -d` refused: the branch has commits that are not merged.
    NotMerged,
    /// The user pressed Stop.
    Cancelled,
    /// The secrets check found what looks like a password or key in the staged lines.
    Secrets,
    /// The repository changed since the action was planned, so nothing ran.
    Changed,
    Other,
}

/// A commit in a short list, e.g. what the remote has that the local branch does not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitBrief {
    pub id: String,
    pub summary: String,
    pub author_name: String,
    /// Commit time, seconds since the Unix epoch.
    pub time: i64,
}

/// A failed action, explained for the sheet that offers the way out.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Failure {
    pub kind: FailureKind,
    /// What git printed, or the error's message.
    pub output: String,
    /// For a rejected push: what the remote has that the local branch does not.
    pub incoming: Vec<CommitBrief>,
    /// For a rejected push: where the remote branch is now, for `--force-with-lease`.
    pub remote_tip: Option<String>,
    /// For a hook that stopped the command: its name, or both names when either of the
    /// `pre-commit` and `commit-msg` hooks could have.
    pub hook: Option<Box<str>>,
    /// For a refused SSH connection: what Oxbow found out, and so which fix fits.
    pub ssh: Option<Box<crate::ssh::SshCheck>>,
    /// For a commit the secrets check stopped: the lines it found.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets: Option<Box<Vec<crate::secrets::SecretFinding>>>,
    /// For a rejected push of commits rewritten here: the branch's tracking after the fetch,
    /// with the lease to use and what others pushed on top of the old versions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracking: Option<Box<Tracking>>,
}

impl Repo {
    /// Explain why `action` failed with `error`. A rejected push fetches the remote branch to
    /// tell whose commits are in the way.
    pub fn explain_failure(&self, action: &Action, error: &Error) -> Failure {
        let pushing = matches!(
            action,
            Action::Push { no_verify: false, .. }
                | Action::PullAndPush { no_verify: false, .. }
                | Action::ReplayAndPush { no_verify: false, .. }
        );
        let pre_push = pushing && self.has_pre_push_hook();
        let mut kind = classify_failure(error, pre_push);
        let mut hook = pre_push.then(|| "pre-push".to_owned());
        // A commit or an edited message the pre-commit or commit-msg hook stopped. With hooks off
        // in Settings they did not run, so they are not to blame.
        if let Action::Commit { no_verify: false, .. } | Action::Reword { no_verify: false, .. } = action
            && crate::config::run_hooks()
            && kind == FailureKind::Other
            && let Error::Command { output, .. } = error
        {
            let hooks: Vec<&str> = ["pre-commit", "commit-msg"]
                .into_iter()
                .filter(|name| self.has_hook(name))
                .collect();
            if !hooks.is_empty() && !commit_refused(output) {
                kind = FailureKind::Hook;
                hook = Some(hooks.join(" or "));
            }
        }
        let output = match error {
            Error::Command { output, .. } => output.trim().to_owned(),
            // Nothing ran: the sheet lists the lines instead.
            Error::Secrets(_) => String::new(),
            other => other.to_string(),
        };
        let mut failure = Failure {
            kind,
            output,
            incoming: Vec::new(),
            remote_tip: None,
            hook: hook.filter(|_| kind == FailureKind::Hook).map(Into::into),
            ssh: None,
            secrets: match error {
                Error::Secrets(found) => Some(Box::new(found.clone())),
                _ => None,
            },
            tracking: None,
        };
        if kind == FailureKind::Auth
            && let Some(remote) = self.remote_of(action)
        {
            failure.ssh = self.check_ssh(&remote).map(Box::new);
        }
        if matches!(kind, FailureKind::Rejected | FailureKind::StaleLease)
            && let Action::Push { remote, upstream, .. }
            | Action::PullAndPush { remote, upstream, .. }
            | Action::ReplayAndPush { remote, upstream, .. } = action
        {
            failure.incoming = self.incoming(remote, upstream).unwrap_or_default();
            failure.remote_tip = self.remote_tip(remote, upstream);
            // After that fetch: whether the push was replacing commits rewritten here, and
            // what others pushed on top of them meanwhile.
            failure.tracking = self
                .tracking()
                .ok()
                .flatten()
                .filter(|t| t.rewritten.is_some())
                .map(Box::new);
        }
        failure
    }

    /// The remote `action` talks to; for a fetch of every remote, the default one.
    fn remote_of(&self, action: &Action) -> Option<String> {
        match action {
            Action::Fetch { remote: None } => self.default_remote(),
            Action::Fetch { remote: Some(remote) }
            | Action::Pull { remote, .. }
            | Action::Push { remote, .. }
            | Action::PullAndPush { remote, .. }
            | Action::ReplayAndPush { remote, .. }
            | Action::PushTags { remote, .. }
            | Action::FetchTags { remote }
            | Action::Unshallow { remote }
            | Action::PushBranches { remote, .. }
            | Action::PullRequestMerged { remote, .. }
            | Action::DeleteRemoteBranch { remote, .. } => Some(remote.clone()),
            _ => None,
        }
    }

    /// Upstream of the checked-out branch; `None` when it has none or `HEAD` is detached.
    pub fn tracking(&self) -> Result<Option<Tracking>> {
        let Some(branch) = self.head()?.branch else {
            return Ok(None);
        };
        let out = self.run(&GitCommand::new([
            "for-each-ref",
            "--format=%(upstream:remotename)%00%(upstream:remoteref)%00%(upstream:track,nobracket)",
            &format!("refs/heads/{branch}"),
        ]))?;
        let mut tracking = parse_tracking(out.stdout.trim_end());
        if let Some(t) = &mut tracking
            && t.behind > 0
        {
            self.find_rewrite(&branch, t);
        }
        Ok(tracking)
    }

    /// Fill `rewritten` (and `replay`) when the remote branch still has old versions of commits
    /// the local `branch` has since rewritten or dropped: an edited message, a squash, Undo
    /// Commit, Edit Stack. See `rewrite_base` for how the old versions are told apart from
    /// someone else's commits: a remote state that only came in with a fetch or pull, which
    /// the branch once fast-forwarded to and an undo took back, is not replaced.
    pub(crate) fn find_rewrite(&self, branch: &str, t: &mut Tracking) {
        let local = format!("refs/heads/{branch}");
        let Some(tip) = self.remote_tip(&t.remote, &t.branch) else {
            return;
        };
        let Some(base) = self.rewrite_base(&local, &t.remote, &t.branch, &tip) else {
            return;
        };
        if base == tip {
            t.rewritten = Some(tip);
            return;
        }
        // Someone pushed on top of the old versions. Their commits are copied over, which a
        // merge among them would not survive: that one is left to sort out by hand. So is
        // any doubt about which of theirs the branch already has.
        if !self.is_ancestor(&base, &tip) || self.has_merges(&format!("{base}..{tip}")).unwrap_or(true) {
            return;
        }
        let Ok(commits) = self.missing_commits(&local, &base, &tip) else {
            return;
        };
        t.rewritten = Some(tip);
        if !commits.is_empty() {
            t.replay = Some(Replay { base, commits });
        }
    }

    /// Where the old versions on the remote end, when the branch rewrote them:
    /// - the state this clone last pushed, once the branch has left it (any rewrite, dropping
    ///   commits too); or
    /// - for commits this clone never pushed (cloned, pulled), the newest remote commit the
    ///   branch once pointed at, when every old version up to it has a copy with the same
    ///   changes on the branch (an edited message, a new order): replacing them loses nothing.
    fn rewrite_base(&self, local: &str, remote: &str, upstream: &str, tip: &str) -> Option<String> {
        if let Some(pushed) = self.last_pushed(remote, upstream)
            && !self.is_ancestor(&pushed, local)
            && self.in_reflog(local, &pushed)
        {
            return Some(pushed);
        }
        let reflog = self.reflog(local)?;
        let fork = self.run(&GitCommand::new(["merge-base", local, tip])).ok()?;
        let fork = fork.stdout.trim();
        let theirs = self
            .run(&GitCommand::new(["rev-list", &format!("{fork}..{tip}")]))
            .ok()?;
        let base = theirs.stdout.lines().find(|id| reflog.contains(*id))?.to_owned();
        if self.has_merges(&format!("{fork}..{base}")).unwrap_or(true) {
            return None;
        }
        let lost = self
            .run(&GitCommand::new([
                "rev-list",
                "--cherry-pick",
                "--right-only",
                "--no-merges",
                &format!("{local}...{base}"),
            ]))
            .ok()?;
        lost.stdout.trim().is_empty().then_some(base)
    }

    fn reflog(&self, reference: &str) -> Option<std::collections::HashSet<String>> {
        let out = self
            .run(&GitCommand::new([
                "reflog",
                "show",
                "--format=%H",
                "--max-count=500",
                reference,
            ]))
            .ok()?;
        Some(out.stdout.lines().map(str::to_owned).collect())
    }

    /// Where this clone last pushed `remote`/`upstream`: the newest "update by push" entry of
    /// its reflog.
    fn last_pushed(&self, remote: &str, upstream: &str) -> Option<String> {
        let out = self
            .run(&GitCommand::new([
                "reflog",
                "show",
                "--format=%H%x09%gs",
                "--max-count=500",
                &format!("refs/remotes/{remote}/{upstream}"),
            ]))
            .ok()?;
        out.stdout
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .find(|(_, subject)| *subject == "update by push")
            .map(|(id, _)| id.to_owned())
    }

    fn in_reflog(&self, reference: &str, id: &str) -> bool {
        self.run(&GitCommand::new([
            "reflog",
            "show",
            "--format=%H",
            "--max-count=500",
            reference,
        ]))
        .is_ok_and(|out| out.stdout.lines().any(|line| line == id))
    }

    /// The commits of `from..to` that `local` has no copy of, oldest first. A copy has the same
    /// changes (the patch, as rebase compares them), or is one Copy and Push made: a cherry-pick
    /// that stopped on conflicts and was resolved by hand changes the patch, so those are told
    /// by their place in its record. Matching names, dates or messages prove nothing: a commit
    /// wrongly taken for copied would be deleted from the remote by the force push.
    pub(crate) fn missing_commits(&self, local: &str, from: &str, to: &str) -> Result<Vec<CommitBrief>> {
        let uncopied = self.run(&GitCommand::new([
            "rev-list",
            "--cherry-pick",
            "--right-only",
            "--no-merges",
            &format!("{local}...{to}"),
        ]))?;
        let uncopied: std::collections::HashSet<&str> = uncopied.stdout.lines().collect();
        let copied = self.recorded_copies(local);
        let theirs = self.run(&GitCommand::new([
            "log",
            "--reverse",
            "--format=%H%x1f%s%x1f%an%x1f%ct",
            &format!("{from}..{to}"),
        ]))?;
        Ok(parse_briefs(&theirs.stdout)
            .into_iter()
            .filter(|c| uncopied.contains(c.id.as_str()) && !copied.contains(&c.id))
            .collect())
    }

    /// Remember what Copy and Push is about to copy onto `branch`: `pick <source>` lines. A
    /// pick that stops on conflicts and is finished by hand gets a `copy <source> <target>`
    /// line once Continue makes its commit (`note_replay_copy`); only those count as copies
    /// in `missing_commits`. Copies already noted for the branch are kept.
    pub(crate) fn record_replay(&self, branch: &str, commits: &[CommitBrief]) -> Result<()> {
        let path = self.git_file(REPLAY_RECORD)?;
        let mut text = format!("branch {branch}\n");
        for line in self.replay_record(branch).iter().filter(|l| l.starts_with("copy ")) {
            text.push_str(line);
            text.push('\n');
        }
        for c in commits {
            text.push_str(&format!("pick {}\n", c.id));
        }
        write_record(&path, &text)
    }

    /// The lines of the Copy and Push record for `branch`, after its `branch` line; none when
    /// there is no record, it is for another branch, or it can't be read.
    fn replay_record(&self, branch: &str) -> Vec<String> {
        let Some(text) = self
            .git_file(REPLAY_RECORD)
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
        else {
            return Vec::new();
        };
        let mut lines = text.lines();
        if lines.next() != Some(&format!("branch {branch}")) {
            return Vec::new();
        }
        lines.map(str::to_owned).collect()
    }

    /// The cherry-pick a Continue is about to finish, when Copy and Push started it: its
    /// source commit and the branch's tip before the Continue.
    pub(crate) fn replay_in_progress(&self) -> Option<(String, String, String)> {
        let branch = self.head().ok()?.branch?;
        let source = std::fs::read_to_string(self.git_file("CHERRY_PICK_HEAD").ok()?).ok()?;
        let source = source.trim().to_owned();
        if !self.replay_record(&branch).contains(&format!("pick {source}")) {
            return None;
        }
        let tip = self
            .run(&GitCommand::new(["rev-parse", &format!("refs/heads/{branch}")]))
            .ok()?;
        Some((branch, source, tip.stdout.trim().to_owned()))
    }

    /// After a Continue finished a pick of Copy and Push by hand: the commit it made right
    /// after `before` is the copy of `source`.
    pub(crate) fn note_replay_copy(&self, branch: &str, source: &str, before: &str) -> Result<()> {
        let made = self.run(&GitCommand::new([
            "rev-list",
            "--reverse",
            "--first-parent",
            &format!("{before}..refs/heads/{branch}"),
        ]))?;
        let Some(target) = made.stdout.lines().next() else {
            return Ok(());
        };
        let path = self.git_file(REPLAY_RECORD)?;
        let mut text = format!("branch {branch}\n");
        for line in self.replay_record(branch) {
            text.push_str(&line);
            text.push('\n');
        }
        text.push_str(&format!("copy {source} {target}\n"));
        write_record(&path, &text)
    }

    /// After an Abort of Copy and Push's cherry-pick: what it was about to copy is no longer
    /// in progress, and copies the abort took back are gone. Only copies still on the branch
    /// are kept.
    pub(crate) fn forget_replay(&self, branch: &str) -> Result<()> {
        let path = self.git_file(REPLAY_RECORD)?;
        let local = format!("refs/heads/{branch}");
        let mut text = format!("branch {branch}\n");
        for line in self.replay_record(branch) {
            if let Some((_, target)) = line.strip_prefix("copy ").and_then(|rest| rest.split_once(' '))
                && self.is_ancestor(target, &local)
            {
                text.push_str(&line);
                text.push('\n');
            }
        }
        write_record(&path, &text)
    }

    /// The commits Copy and Push copied onto `local` by hand after a conflict: each noted
    /// target must still be on the branch. Nothing else counts, not a commit's place after
    /// the start, nor its author or time.
    fn recorded_copies(&self, local: &str) -> std::collections::HashSet<String> {
        let Some(branch) = local.strip_prefix("refs/heads/") else {
            return std::collections::HashSet::new();
        };
        self.replay_record(branch)
            .iter()
            .filter_map(|line| line.strip_prefix("copy ")?.split_once(' '))
            .filter(|(_, target)| self.is_ancestor(target, local))
            .map(|(source, _)| source.to_owned())
            .collect()
    }

    /// Upstream of every local branch that has one, by branch name.
    pub fn branch_tracking(&self) -> Result<std::collections::HashMap<String, Tracking>> {
        let out = self.run(&GitCommand::new([
            "for-each-ref",
            "--format=%(refname:short)%00%(upstream:remotename)%00%(upstream:remoteref)%00%(upstream:track,nobracket)",
            "refs/heads",
        ]))?;
        Ok(out
            .stdout
            .lines()
            .filter_map(|line| {
                let (name, rest) = line.split_once('\0')?;
                Some((name.to_owned(), parse_tracking(rest)?))
            })
            .collect())
    }

    /// What deleting the local `branch` would lose.
    pub fn deletion_check(&self, branch: &str) -> Result<DeletionCheck> {
        let upstream = self
            .branch_tracking()?
            .remove(branch)
            .filter(|t| !t.gone)
            .map(|t| format!("{}/{}", t.remote, t.branch));
        let lost = self.only_on(Some(branch), None)?;
        let lost_with_upstream = match &upstream {
            Some(upstream) => self.only_on(Some(branch), Some(upstream))?,
            None => lost.clone(),
        };
        Ok(DeletionCheck {
            lost,
            lost_with_upstream,
        })
    }

    /// Commits of the remote branch `remote_branch` (`origin/x`) that no other branch, remote
    /// branch or tag has: what deleting it on the remote would lose.
    pub fn remote_deletion_check(&self, remote_branch: &str) -> Result<Vec<CommitBrief>> {
        self.only_on(None, Some(remote_branch))
    }

    /// Commits, newest first, of the local `branch` and the remote branch `remote_branch`
    /// (`origin/x`) that no other branch, remote branch or tag has.
    fn only_on(&self, branch: Option<&str>, remote_branch: Option<&str>) -> Result<Vec<CommitBrief>> {
        let mut args = vec![
            "log".to_owned(),
            "--max-count=50".to_owned(),
            "--format=%H%x1f%s%x1f%an%x1f%ct".to_owned(),
        ];
        args.extend(branch.map(|b| format!("refs/heads/{b}")));
        args.extend(remote_branch.map(|b| format!("refs/remotes/{b}")));
        args.push("--not".to_owned());
        // `--exclude` applies to the next `--branches` or `--remotes`, with names relative to it.
        args.extend(branch.map(|b| format!("--exclude={b}")));
        args.push("--branches".to_owned());
        args.extend(remote_branch.map(|b| format!("--exclude={b}")));
        args.extend(["--remotes".to_owned(), "--tags".to_owned()]);
        Ok(parse_briefs(&self.run(&GitCommand::new(args))?.stdout))
    }

    /// The remote to publish new branches to: `origin`, or the only remote there is.
    pub fn default_remote(&self) -> Option<String> {
        let remotes = self.remotes();
        if remotes.iter().any(|r| r == "origin") {
            Some("origin".to_owned())
        } else {
            remotes.into_iter().next()
        }
    }

    /// Commits on `remote`/`branch` that `HEAD` does not have, newest first, after fetching that
    /// branch so the list is current.
    pub fn incoming(&self, remote: &str, branch: &str) -> Result<Vec<CommitBrief>> {
        self.run(&GitCommand::new(["fetch", "--quiet", remote, branch]))?;
        let out = self.run(&GitCommand::new([
            "log",
            "--max-count=20",
            "--format=%H%x1f%s%x1f%an%x1f%ct",
            &format!("HEAD..refs/remotes/{remote}/{branch}"),
        ]))?;
        Ok(parse_briefs(&out.stdout))
    }

    /// The commit a remote-tracking branch points at, as of the last fetch.
    pub fn remote_tip(&self, remote: &str, branch: &str) -> Option<String> {
        self.run(&GitCommand::new([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{remote}/{branch}"),
        ]))
        .ok()
        .map(|out| out.stdout.trim().to_owned())
        .filter(|id| !id.is_empty())
    }

    /// Whether a pre-push hook is installed, honoring `core.hooksPath`.
    pub(crate) fn has_pre_push_hook(&self) -> bool {
        self.has_hook("pre-push")
    }

    /// Whether the hook `name` is installed and hooks are on in Settings, honoring
    /// `core.hooksPath`.
    pub(crate) fn has_hook(&self, name: &str) -> bool {
        if !crate::config::run_hooks() {
            return false;
        }
        let Ok(out) = self.run(&GitCommand::new(["rev-parse", "--git-path", &format!("hooks/{name}")])) else {
            return false;
        };
        self.workdir().join(out.stdout.trim()).is_file()
    }
}

/// What deleting a local branch would lose, for the confirmation sheet.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletionCheck {
    /// Commits on no other branch, remote branch or tag, the branch's upstream included.
    pub lost: Vec<CommitBrief>,
    /// The same when the upstream is deleted too, its own commits included.
    pub lost_with_upstream: Vec<CommitBrief>,
}

/// Lines of `git log --format=%H%x1f%s%x1f%an%x1f%ct`.
pub(crate) fn parse_briefs(out: &str) -> Vec<CommitBrief> {
    out.lines()
        .filter_map(|line| {
            let mut parts = line.split('\u{1f}');
            Some(CommitBrief {
                id: parts.next()?.to_owned(),
                summary: parts.next()?.to_owned(),
                author_name: parts.next()?.to_owned(),
                time: parts.next()?.parse().unwrap_or(0),
            })
        })
        .collect()
}

/// Where Copy and Push writes down what it copies, in the git directory.
const REPLAY_RECORD: &str = "oxbow/replay";

fn write_record(path: &std::path::Path, text: &str) -> Result<()> {
    let failed = |err: std::io::Error| Error::Git(format!("could not write {}: {err}", path.display()));
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    std::fs::write(path, text).map_err(failed)
}

/// `origin\0refs/heads/main\0ahead 2, behind 1` from `git for-each-ref`.
fn parse_tracking(line: &str) -> Option<Tracking> {
    let mut parts = line.split('\0');
    let remote = parts.next()?.to_owned();
    let branch = parts.next()?.strip_prefix("refs/heads/")?.to_owned();
    if remote.is_empty() || branch.is_empty() {
        return None;
    }
    let track = parts.next().unwrap_or_default();
    let count = |word: &str| {
        track
            .split(", ")
            .find_map(|part| part.strip_prefix(word)?.trim().parse().ok())
            .unwrap_or(0)
    };
    Some(Tracking {
        remote,
        branch,
        ahead: count("ahead "),
        behind: count("behind "),
        gone: track == "gone",
        rewritten: None,
        replay: None,
    })
}

/// Whether git itself refused a commit, before or without any hook: nothing staged, no name
/// or email, an empty message.
fn commit_refused(output: &str) -> bool {
    [
        "nothing to commit",
        "nothing added to commit",
        "no changes added to commit",
        "Author identity unknown",
        "empty ident name",
        "unable to auto-detect email",
        "Aborting commit due to empty commit message",
        "You have unmerged paths",
        "Committing is not possible",
    ]
    .iter()
    .any(|needle| output.contains(needle))
}

/// A line of `git fetch` saying a remote branch or tag is new, moved or gone, e.g.
/// ` * [new branch]      auth -> origin/auth` or `   1a2b3c4..5d6e7f8  main -> origin/main`.
pub fn is_ref_update(line: &str) -> bool {
    let mut chars = line.chars();
    chars.next() == Some(' ')
        && chars.next().is_some_and(|flag| " +-*t!=".contains(flag))
        && chars.next() == Some(' ')
        && line.contains(" -> ")
        && !line.contains("[up to date]")
}

/// Sort a failure by what git printed.
pub fn classify_failure(error: &Error, pushing_with_hook: bool) -> FailureKind {
    let output = match error {
        Error::Command { output, .. } => output,
        Error::Cancelled => return FailureKind::Cancelled,
        Error::Secrets(_) => return FailureKind::Secrets,
        Error::Changed(_) => return FailureKind::Changed,
        _ => return FailureKind::Other,
    };
    let has = |needle: &str| output.contains(needle);
    if has("stale info") {
        FailureKind::StaleLease
    } else if has("[rejected]") || has("non-fast-forward") || has("(fetch first)") {
        FailureKind::Rejected
    } else if has("CONFLICT") || has("could not apply") || has("Resolve all conflicts") {
        FailureKind::Conflict
    } else if has("would be overwritten by checkout") || has("would be overwritten by switch") {
        FailureKind::LocalChanges
    } else if has("is not fully merged") {
        FailureKind::NotMerged
    } else if has("Authentication failed")
        || has("could not read Username")
        || has("could not read Password")
        || has("terminal prompts disabled")
        || has("Invalid username or token")
        || has("Permission denied (publickey")
        || has("Host key verification failed")
        || has("The requested URL returned error: 403")
        || has("The requested URL returned error: 401")
    {
        FailureKind::Auth
    } else if has("Could not resolve host")
        || has("Could not resolve hostname")
        || has("Connection timed out")
        || has("Connection refused")
        || has("Network is unreachable")
        || has("Operation timed out")
        || has("unable to access")
    {
        FailureKind::Network
    } else if pushing_with_hook && !output.lines().any(|l| l.starts_with("To ")) {
        // The hook runs before anything is sent, so the remote never answered.
        FailureKind::Hook
    } else {
        FailureKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(output: &str) -> Error {
        Error::Command {
            command: "git push".into(),
            code: Some(1),
            output: output.into(),
        }
    }

    #[test]
    fn fetch_lines_that_move_a_ref_are_counted() {
        assert!(is_ref_update(" * [new branch]      auth/3-ui  -> origin/auth/3-ui"));
        assert!(is_ref_update("   1a2b3c4..5d6e7f8  main       -> origin/main"));
        assert!(is_ref_update(
            " + 1a2b3c4...5d6e7f8 wip       -> origin/wip  (forced update)"
        ));
        assert!(is_ref_update(" - [deleted]         (none)     -> origin/old"));
        assert!(!is_ref_update("From github.com:acme/api"));
        assert!(!is_ref_update("Receiving objects:  64% (1203/1880)"));
        assert!(!is_ref_update(" = [up to date]      main       -> origin/main"));
    }

    #[test]
    fn tracking_counts_both_ways() {
        assert_eq!(
            parse_tracking("origin\0refs/heads/main\0ahead 2, behind 1"),
            Some(Tracking {
                remote: "origin".into(),
                branch: "main".into(),
                ahead: 2,
                behind: 1,
                gone: false,
                rewritten: None,
                replay: None
            })
        );
        let gone = parse_tracking("origin\0refs/heads/old\0gone").unwrap();
        assert!(gone.gone);
        assert_eq!(parse_tracking("origin\0refs/heads/x\0").unwrap().ahead, 0);
        assert_eq!(parse_tracking("\0\0"), None);
    }

    #[test]
    fn failures_are_sorted_by_what_git_printed() {
        let kind = |out: &str| classify_failure(&failed(out), false);
        assert_eq!(
            kind(" ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs"),
            FailureKind::Rejected
        );
        assert_eq!(
            kind(" ! [rejected]        main -> main (stale info)"),
            FailureKind::StaleLease
        );
        assert_eq!(
            kind("fatal: unable to access 'https://github.com/a/b.git/': Could not resolve host: github.com"),
            FailureKind::Network
        );
        assert_eq!(
            kind("git@github.com: Permission denied (publickey).\nfatal: Could not read from remote repository."),
            FailureKind::Auth
        );
        assert_eq!(
            kind("CONFLICT (content): Merge conflict in a.txt\nerror: could not apply 1234567... Edit"),
            FailureKind::Conflict
        );
        assert_eq!(
            classify_failure(&failed("tests failed\nerror: failed to push some refs to 'x'"), true),
            FailureKind::Hook
        );
        assert_eq!(
            kind("error: Your local changes to the following files would be overwritten by checkout:\n\ta.txt"),
            FailureKind::LocalChanges
        );
        assert_eq!(
            kind("error: the branch 'x' is not fully merged\nhint: If you are sure you want to delete it"),
            FailureKind::NotMerged
        );
        assert_eq!(kind("something else"), FailureKind::Other);
        assert_eq!(classify_failure(&Error::Cancelled, false), FailureKind::Cancelled);
    }
}
