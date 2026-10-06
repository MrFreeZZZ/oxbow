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
    /// A local hook (pre-push) stopped the command.
    Hook,
    /// A pull stopped on conflicts.
    Conflict,
    /// Switching branches would overwrite uncommitted changes.
    LocalChanges,
    /// `git branch -d` refused: the branch has commits that are not merged.
    NotMerged,
    /// The user pressed Stop.
    Cancelled,
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
}

impl Repo {
    /// Explain why `action` failed with `error`. A rejected push fetches the remote branch to
    /// tell whose commits are in the way.
    pub fn explain_failure(&self, action: &Action, error: &Error) -> Failure {
        let pushing = matches!(
            action,
            Action::Push { no_verify: false, .. } | Action::PullAndPush { no_verify: false, .. }
        );
        let kind = classify_failure(error, pushing && self.has_pre_push_hook());
        let output = match error {
            Error::Command { output, .. } => output.trim().to_owned(),
            other => other.to_string(),
        };
        let mut failure = Failure {
            kind,
            output,
            incoming: Vec::new(),
            remote_tip: None,
        };
        if matches!(kind, FailureKind::Rejected | FailureKind::StaleLease)
            && let Action::Push { remote, upstream, .. } | Action::PullAndPush { remote, upstream, .. } = action
        {
            failure.incoming = self.incoming(remote, upstream).unwrap_or_default();
            failure.remote_tip = self.remote_tip(remote, upstream);
        }
        failure
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
        Ok(parse_tracking(out.stdout.trim_end()))
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
        if !crate::config::run_hooks() {
            return false;
        }
        let Ok(out) = self.run(&GitCommand::new(["rev-parse", "--git-path", "hooks/pre-push"])) else {
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
    })
}

/// Sort a failure by what git printed.
pub fn classify_failure(error: &Error, pushing_with_hook: bool) -> FailureKind {
    let output = match error {
        Error::Command { output, .. } => output,
        Error::Cancelled => return FailureKind::Cancelled,
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
    fn tracking_counts_both_ways() {
        assert_eq!(
            parse_tracking("origin\0refs/heads/main\0ahead 2, behind 1"),
            Some(Tracking {
                remote: "origin".into(),
                branch: "main".into(),
                ahead: 2,
                behind: 1,
                gone: false
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
