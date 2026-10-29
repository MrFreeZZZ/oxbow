//! Protected branches: the branches a team shares, `main`, `master` and the production branch
//! unless Settings › This Repository lists others. Oxbow never force pushes them and doesn't
//! rewrite their commits a remote already has (Amend, Edit Message, Drop, Squash, Split, Edit
//! Stack, Reset past them, a rebase); Revert undoes such a commit instead. Their commits not
//! pushed yet stay editable, as on any branch.

use gix::bstr::ByteSlice;

use crate::action::Action;
use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::fix::Fix;
use crate::operation::MergeMethod;
use crate::repo::{RefKind, Repo};

/// Where Settings › This Repository › Protected Branches is kept: names and patterns separated by
/// spaces. Not set means the default list; set and empty, none.
pub const PROTECTED_KEY: &str = "oxbow.protectedBranches";

/// Whether the branch `name` is `pattern`, where `*` stands for any characters, `/` included.
pub fn matches(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if name.len() < first.len() + last.len() || !name.starts_with(first) || !name.ends_with(last) {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for part in &parts[1..parts.len() - 1] {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    true
}

/// The names and patterns of the setting's value, in order, each once.
pub fn parse(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for name in value
        .split([' ', ',', '\t', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if !out.iter().any(|n| n == name) {
            out.push(name.to_owned());
        }
    }
    out
}

/// Whether `branch` is one of `protected`.
pub fn is_protected(protected: &[String], branch: &str) -> bool {
    protected.iter().any(|p| matches(p, branch))
}

/// Why a force push of `branch` doesn't run.
fn no_force(branch: &str) -> Error {
    Error::Git(format!(
        "{branch} is a protected branch: Oxbow doesn’t force push it. Revert the commits instead, or take {branch} out of Protected Branches in Settings › This Repository"
    ))
}

/// Why rewriting pushed commits of `branch` doesn't run.
fn no_rewrite(branch: &str) -> Error {
    Error::Git(format!(
        "{branch} is a protected branch, and this would rewrite its commits that are already pushed. Revert them instead, or take {branch} out of Protected Branches in Settings › This Repository"
    ))
}

impl Repo {
    /// The setting as written, `None` when it isn't set.
    pub fn protected_setting(&self) -> Option<String> {
        let repo = self.local();
        let config = repo.config_snapshot();
        config
            .string(PROTECTED_KEY)
            .map(|value| value.to_str_lossy().into_owned())
    }

    /// The protected branches: the setting's names and patterns, or without it `main`, `master`
    /// and the production branch chosen in Settings.
    pub fn protected_branches(&self) -> Vec<String> {
        if let Some(value) = self.protected_setting() {
            return parse(&value);
        }
        let mut out = vec!["main".to_owned(), "master".to_owned()];
        let production = self.production_setting().and_then(|full| {
            if let Some(name) = full.strip_prefix("refs/heads/") {
                return Some(name.to_owned());
            }
            let rest = full.strip_prefix("refs/remotes/")?;
            let remote = self
                .remotes()
                .into_iter()
                .filter(|r| rest.starts_with(&format!("{r}/")))
                .max_by_key(String::len)?;
            Some(rest[remote.len() + 1..].to_owned())
        });
        if let Some(name) = production
            && !out.contains(&name)
        {
            out.push(name);
        }
        out
    }

    /// Refuse `action` when it would force push a protected branch or rewrite pushed commits of
    /// one. Everything else passes.
    pub(crate) fn check_protected(&self, action: &Action) -> Result<()> {
        let protected = self.protected_branches();
        if protected.is_empty() {
            return Ok(());
        }
        let guarded = |branch: &str| is_protected(&protected, branch);
        match action {
            Action::Push {
                force: true, upstream, ..
            }
            | Action::ReplayAndPush { upstream, .. }
                if guarded(upstream) =>
            {
                Err(no_force(upstream))
            }
            Action::PushBranches { branches, .. } => match branches.iter().find(|b| guarded(&b.upstream)) {
                Some(b) => Err(no_force(&b.upstream)),
                None => Ok(()),
            },
            Action::Commit { amend: true, .. }
            | Action::Reword { .. }
            | Action::Fix {
                fix: Fix::AddToLast { .. } | Fix::UndoLast { .. },
                ..
            } => self.check_head_keeps("HEAD^", &guarded),
            Action::AddToCommit { commit, .. } => self.check_head_keeps(&format!("{commit}^"), &guarded),
            Action::Reset { commit, .. } => self.check_head_keeps(commit, &guarded),
            Action::Merge {
                branch,
                method: MergeMethod::Rebase,
                ..
            } => self.check_head_keeps(branch, &guarded),
            Action::EditStack { plan } => {
                for r in self
                    .refs()?
                    .iter()
                    .filter(|r| r.kind == RefKind::Local && guarded(&r.name))
                {
                    let inside = r.name == plan.top
                        || (self.is_ancestor(&r.target, &plan.top) && !self.is_ancestor(&r.target, &plan.base));
                    if inside && self.drops_pushed(&r.target, Some(&plan.base))? {
                        return Err(no_rewrite(&r.name));
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Refuse when the checked-out branch is protected and moving it onto `keep` would leave out
    /// a commit a remote has. A `keep` that names no commit (the parent of a root commit) keeps
    /// none of them.
    fn check_head_keeps(&self, keep: &str, guarded: &dyn Fn(&str) -> bool) -> Result<()> {
        let head = self.head()?;
        let (Some(branch), Some(commit)) = (head.branch, head.commit) else {
            return Ok(());
        };
        if !guarded(&branch) {
            return Ok(());
        }
        let keep = self.rev(keep);
        if self.drops_pushed(&commit, keep.as_deref())? {
            return Err(no_rewrite(&branch));
        }
        Ok(())
    }

    /// Whether some commit of `tip` that `keep` doesn't have is on a remote-tracking branch;
    /// without `keep`, any commit of `tip`.
    pub(crate) fn drops_pushed(&self, tip: &str, keep: Option<&str>) -> Result<bool> {
        if self.remotes().is_empty() {
            return Ok(false);
        }
        let Some(tip) = self.rev(tip) else {
            return Ok(false);
        };
        let count = |unpushed: bool| -> Result<usize> {
            let mut args = vec!["rev-list".to_owned(), "--count".to_owned()];
            if unpushed {
                args.extend(["--not".to_owned(), "--remotes".to_owned(), "--not".to_owned()]);
            }
            args.push(tip.clone());
            if let Some(keep) = keep {
                args.push(format!("^{keep}"));
            }
            Ok(self.run(&GitCommand::new(args))?.stdout.trim().parse().unwrap_or(0))
        };
        Ok(count(false)? > count(true)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_star_stands_for_any_characters() {
        assert!(matches("main", "main"));
        assert!(!matches("main", "main2"));
        assert!(matches("release/*", "release/2.1"));
        assert!(matches("release/*", "release/2/hotfix"));
        assert!(!matches("release/*", "releases/2"));
        assert!(matches("*", "anything"));
        assert!(matches("*-stable", "6.1-stable"));
        assert!(matches("v*.*", "v2.0"));
        assert!(!matches("v*.*", "v2"));
        assert!(!matches("a*a", "a"));
    }

    #[test]
    fn the_setting_is_names_separated_by_spaces_or_commas() {
        assert_eq!(parse(" main, release/*  main\tprod "), ["main", "release/*", "prod"]);
        assert!(parse("").is_empty());
    }
}
