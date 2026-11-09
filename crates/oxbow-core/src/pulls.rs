//! Pull requests on GitHub: the list of a repository's pull requests, everything the Pull
//! Request screen shows about one (conversation, commits, checks, reviews, whether it can merge)
//! and the calls that change them: open, merge, retarget, mark ready, merge automatically, and
//! review: comment on lines, reply, resolve, mark files viewed, send a review.
//!
//! Reading goes through the REST API, one request per kind of thing, sent side by side. Only
//! what REST can't do goes through GraphQL: whether a review thread is resolved, marking a draft
//! ready for review, auto-merge, files viewed and reviews with replies in them.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::repo::{Repo, local_branch_ref};

mod client;
mod shared;
mod types;

pub(crate) use client::graphql_error;
pub use client::{hunk_tail, pull_url, started_review};
pub use shared::parse_time;
pub(crate) use shared::{Loaded, loaded_read, optional_read, read_pages, text};
pub use types::*;

/// Owner and name from a github.com address, in any form git takes: `https://github.com/o/r.git`,
/// `git@github.com:o/r.git`, `ssh://git@github.com/o/r`.
pub fn parse_github_url(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let rest = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest
    } else {
        let after_scheme = url.split_once("://")?.1;
        // user@ and :port go before the host.
        let after_user = after_scheme.rsplit_once('@').map_or(after_scheme, |(_, host)| host);
        let (host, path) = after_user.split_once('/')?;
        let host = host.split(':').next().unwrap_or(host);
        if !host.eq_ignore_ascii_case("github.com") && !host.eq_ignore_ascii_case("www.github.com") {
            return None;
        }
        path
    };
    let mut parts = rest.trim_matches('/').splitn(3, '/');
    let owner = parts.next()?.to_owned();
    let name = parts.next()?;
    let name = name.strip_suffix(".git").unwrap_or(name).to_owned();
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some((owner, name))
}

impl Repo {
    /// The GitHub repository this one pushes to: the default remote's, or the first remote on
    /// github.com.
    pub fn github_repo(&self) -> Option<GitHubRepo> {
        let remotes = self.remotes_info().ok()?;
        let on_github = |r: &crate::RemoteInfo| {
            parse_github_url(&r.push_url)
                .or_else(|| parse_github_url(&r.fetch_url))
                .map(|(owner, name)| GitHubRepo {
                    owner,
                    name,
                    remote: r.name.clone(),
                })
        };
        let preferred = self.default_remote();
        remotes
            .iter()
            .filter(|r| Some(&r.name) == preferred.as_ref())
            .chain(remotes.iter())
            .find_map(on_github)
    }

    /// Lines added and deleted by each of `shas` that is here, by sha.
    pub fn commit_stats(&self, shas: &[String]) -> HashMap<String, (u32, u32)> {
        if shas.is_empty() {
            return HashMap::new();
        }
        let query: String = shas.iter().map(|sha| format!("{sha}^{{commit}}\n")).collect();
        let Ok(out) = self.run_with_input(
            &GitCommand::new(["cat-file", "--batch-check=%(objectname)"]),
            Some(query.as_bytes()),
        ) else {
            return HashMap::new();
        };
        let here: Vec<String> = out
            .stdout
            .lines()
            .filter(|line| !line.ends_with("missing"))
            .map(str::to_owned)
            .collect();
        if here.is_empty() {
            return HashMap::new();
        }
        let mut args = vec![
            "show".to_owned(),
            "--numstat".to_owned(),
            "--format=%x00%H".to_owned(),
            "--no-renames".to_owned(),
        ];
        args.extend(here);
        let Ok(out) = self.run(&GitCommand::new(args)) else {
            return HashMap::new();
        };
        let mut stats = HashMap::new();
        for block in out.stdout.split('\0').filter(|b| !b.trim().is_empty()) {
            let mut lines = block.lines();
            let Some(sha) = lines.next().map(str::trim) else {
                continue;
            };
            let (mut add, mut del) = (0, 0);
            for line in lines {
                let mut cols = line.split('\t');
                add += cols.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
                del += cols.next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(0);
            }
            stats.insert(sha.to_owned(), (add, del));
        }
        stats
    }

    /// Who CODEOWNERS asks to review what `branch` changes against `base`: logins, and teams as
    /// `org/team`, in the order of the files they own. `gitlab` reads the file where GitLab
    /// looks for it, with its sections.
    pub fn code_owners(&self, base: &str, branch: &str, gitlab: bool) -> Vec<String> {
        let base = self.branch_revision(base);
        let branch = local_branch_ref(branch);
        let places: &[&str] = if gitlab {
            &["CODEOWNERS", "docs/CODEOWNERS", ".gitlab/CODEOWNERS"]
        } else {
            &[".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"]
        };
        let file = places.iter().find_map(|path| {
            self.run(&GitCommand::new(["show".to_owned(), format!("{branch}:{path}")]))
                .ok()
                .map(|out| out.stdout)
        });
        let Some(file) = file else { return Vec::new() };
        let Ok(changed) = self.run(&GitCommand::new([
            "diff".to_owned(),
            "--name-only".to_owned(),
            format!("{base}...{branch}"),
        ])) else {
            return Vec::new();
        };
        let sections = if gitlab {
            parse_gitlab_owners(&file)
        } else {
            vec![parse_code_owners(&file)]
        };
        let mut owners: Vec<String> = Vec::new();
        for path in changed.stdout.lines().filter(|p| !p.is_empty()) {
            // The last rule that matches wins, in each section.
            for rules in &sections {
                if let Some((_, names)) = rules
                    .iter()
                    .rev()
                    .find(|(pattern, _)| owner_pattern_matches(pattern, path))
                {
                    for name in names {
                        if !owners.contains(name) {
                            owners.push(name.clone());
                        }
                    }
                }
            }
        }
        owners
    }

    /// GitLab's merge request templates on `rev`, `.gitlab/merge_request_templates/*.md`, by
    /// name without `.md`: `Default` first, then by name.
    pub fn merge_request_templates(&self, rev: &str) -> Vec<(String, String)> {
        const FOLDER: &str = ".gitlab/merge_request_templates/";
        let Ok(list) = self.run(&GitCommand::new([
            "ls-tree".to_owned(),
            "--name-only".to_owned(),
            "-z".to_owned(),
            rev.to_owned(),
            "--".to_owned(),
            FOLDER.to_owned(),
        ])) else {
            return Vec::new();
        };
        let mut templates: Vec<(String, String)> = list
            .stdout
            .split('\0')
            .filter_map(|path| {
                let name = path.strip_prefix(FOLDER)?;
                let name = name.strip_suffix(".md").or_else(|| name.strip_suffix(".MD"))?;
                let text = self
                    .run(&GitCommand::new(["show".to_owned(), format!("{rev}:{path}")]))
                    .ok()?
                    .stdout;
                Some((name.to_owned(), text))
            })
            .collect();
        templates.sort_by_key(|(name, _)| (!name.eq_ignore_ascii_case("default"), name.to_lowercase()));
        templates
    }
}

impl Repo {
    /// The commands of [`crate::Action::PullRequestMerged`]: fetch the merge, then let the local
    /// `base` catch up when it has nothing of its own. Deleting the branch is a step of its own
    /// ([`crate::Action::DeleteMergedBranch`]), so a failed delete never keeps the merge out.
    pub(crate) fn plan_pull_request_merged(&self, remote: &str, base: &str) -> Result<Vec<GitCommand>> {
        let head = self.head()?.branch;
        let mut plan = vec![
            GitCommand::new(["fetch", "--prune", remote])
                .comment("bring in the merge; --prune forgets branches deleted there")
                .with_progress(),
        ];
        let local = local_branch_ref(base);
        let tracking = format!("refs/remotes/{remote}/{base}");
        // Whether base has commits of its own is decided after the fetch, by what it brought:
        // a base that has them stays as it is, Pull sorts that out.
        if !self.has_ref(&local) {
            return Ok(plan);
        }
        if head.as_deref() == Some(base) {
            plan.push(
                GitCommand::new(["merge", "--ff-only", &tracking])
                    .comment(format!(
                        "your checked-out {base} catches up, unless it has commits of its own; changes of yours in the way stop it, nothing is overwritten"
                    ))
                    .only_if_ancestor(&local, &tracking),
            );
        } else if self.branch_in_use(base).ok().flatten().is_none() {
            // fetch refuses a base checked out in another worktree itself; this only keeps the
            // refusal from failing the whole step.
            plan.push(
                GitCommand::new(["fetch".to_owned(), remote.to_owned(), format!("{local}:{local}")])
                    .comment(format!(
                        "fast-forward your {base} to {remote}/{base}, unless it has commits of its own"
                    ))
                    .only_if_ancestor(&local, &tracking),
            );
        }
        Ok(plan)
    }

    /// The commands of [`crate::Action::DeleteMergedBranch`]: delete `branch` only while it is
    /// still at `sha`, the commit GitHub merged, leaving it for `base` when it is checked out.
    pub(crate) fn plan_delete_merged_branch(&self, base: &str, branch: &str, sha: &str) -> Result<Vec<GitCommand>> {
        let name = format!("refs/heads/{branch}");
        if !self.has_ref(&name) {
            return Ok(Vec::new());
        }
        // update-ref, unlike git branch -D, would pull the branch from under another worktree.
        if let Some(place) = self.branch_in_use(branch)? {
            return Err(Error::Git(format!("{branch} {place}")));
        }
        let mut plan = Vec::new();
        if self.head()?.branch.as_deref() == Some(branch) {
            // Uncommitted changes keep the branch: they may not fit on the base.
            if self.tracked_changes()? {
                return Err(Error::Git(format!("{branch} has uncommitted changes")));
            }
            plan.push(GitCommand::new(["switch", base]).comment(format!("leave {branch} to delete it")));
        }
        let short: String = sha.chars().take(7).collect();
        plan.push(GitCommand::new(["update-ref", "-d", &name, sha]).comment(format!(
            "only while {branch} is still at {short}, the commit GitHub merged: newer commits keep it"
        )));
        let configured = |key: &str| {
            self.run(&GitCommand::new([
                "config".to_owned(),
                "--get".to_owned(),
                format!("branch.{branch}.{key}"),
            ]))
            .is_ok()
        };
        if configured("remote") || configured("merge") {
            plan.push(
                GitCommand::new([
                    "config".to_owned(),
                    "--remove-section".to_owned(),
                    format!("branch.{branch}"),
                ])
                .comment("forget its upstream, as git branch -D does"),
            );
        }
        Ok(plan)
    }

    /// How `branch` is in use where deleting or moving it would break something, as git
    /// branch -D sees it: checked out in another worktree, or being rebased or bisected in any.
    fn branch_in_use(&self, branch: &str) -> Result<Option<String>> {
        let out = self.run(&GitCommand::new([
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
            "--git-dir",
        ]))?;
        let mut lines = out.stdout.lines().map(|l| PathBuf::from(l.trim()));
        let (Some(common), Some(here)) = (lines.next(), lines.next()) else {
            return Ok(None);
        };
        let here = here.canonicalize().unwrap_or(here);
        let mut dirs = vec![common.clone()];
        if let Ok(entries) = std::fs::read_dir(common.join("worktrees")) {
            dirs.extend(entries.flatten().map(|e| e.path()));
        }
        let full = format!("refs/heads/{branch}");
        for dir in dirs {
            let read = |name: &str| {
                std::fs::read_to_string(dir.join(name))
                    .map(|t| t.trim().to_owned())
                    .ok()
            };
            let elsewhere = dir.canonicalize().unwrap_or_else(|_| dir.clone()) != here;
            let doing = if elsewhere && read("HEAD").is_some_and(|h| h == format!("ref: {full}")) {
                "is checked out"
            } else if ["rebase-merge/head-name", "rebase-apply/head-name"]
                .iter()
                .any(|f| read(f).is_some_and(|h| h == full))
            {
                "is being rebased"
            } else if read("BISECT_START").is_some_and(|b| b == branch || b == full) {
                "is being bisected"
            } else {
                continue;
            };
            // A linked worktree's folder is named in its gitdir file; the main one holds .git.
            let folder = match read("gitdir") {
                Some(file) => Path::new(&file).parent().map(Path::to_path_buf),
                None => dir.parent().map(Path::to_path_buf),
            };
            let place = folder.map_or_else(String::new, |f| format!(" in {}", f.display()));
            return Ok(Some(format!("{doing}{place}")));
        }
        Ok(None)
    }
}

/// CODEOWNERS lines as (pattern, owners without `@`); owners that are e-mail addresses are left
/// out, since a review is asked of an account.
fn parse_code_owners(text: &str) -> Vec<(String, Vec<String>)> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let pattern = words.next()?.to_owned();
            let owners = words.filter_map(|w| w.strip_prefix('@')).map(str::to_owned).collect();
            Some((pattern, owners))
        })
        .collect()
}

/// GitLab's CODEOWNERS as sections of rules: a line `[Section]` starts one, maybe with a count
/// of approvals (`[Section][2]`) and owners for its lines that name none; `^[Section]` is an
/// optional one GitLab doesn't ask, so it is left out. Lines before any section are a section
/// of their own.
fn parse_gitlab_owners(text: &str) -> Vec<Vec<(String, Vec<String>)>> {
    let mut sections: Vec<Vec<(String, Vec<String>)>> = vec![Vec::new()];
    let mut defaults: Vec<String> = Vec::new();
    let mut optional = false;
    for line in text.lines().map(|line| line.split('#').next().unwrap_or("").trim()) {
        if line.is_empty() {
            continue;
        }
        let header = line.strip_prefix('^').unwrap_or(line);
        if let Some(rest) = header.strip_prefix('[')
            && let Some((_, after)) = rest.split_once(']')
        {
            optional = line.starts_with('^');
            // `[2]`, the approvals it needs, comes right after the name.
            let after = after.trim_start();
            let after = match after.strip_prefix('[').and_then(|a| a.split_once(']')) {
                Some((count, rest)) if count.chars().all(|c| c.is_ascii_digit()) => rest,
                _ => after,
            };
            defaults = after
                .split_whitespace()
                .filter_map(|w| w.strip_prefix('@'))
                .map(str::to_owned)
                .collect();
            sections.push(Vec::new());
            continue;
        }
        if optional {
            continue;
        }
        let mut words = line.split_whitespace();
        let Some(pattern) = words.next() else { continue };
        let named: Vec<String> = words.filter_map(|w| w.strip_prefix('@')).map(str::to_owned).collect();
        let owners = if named.is_empty() { defaults.clone() } else { named };
        if let Some(rules) = sections.last_mut() {
            rules.push((pattern.to_owned(), owners));
        }
    }
    sections.retain(|rules| !rules.is_empty());
    sections
}

/// Whether a CODEOWNERS pattern (gitignore rules) covers `path`.
pub(crate) fn owner_pattern_matches(pattern: &str, path: &str) -> bool {
    let anchored = pattern.starts_with('/') || pattern.trim_end_matches('/').contains('/');
    let directory = pattern.ends_with('/');
    // `docs/*` is the files right in docs, not deeper ones (GitHub's own example).
    let files_only = pattern.ends_with("/*");
    let pattern = pattern.trim_start_matches('/').trim_end_matches('/');
    if pattern == "*" {
        return true;
    }
    let segments: Vec<&str> = path.split('/').collect();
    let pat: Vec<&str> = pattern.split('/').collect();
    // A pattern matches the path itself or any folder above it (everything inside a folder).
    let starts: Vec<usize> = if anchored {
        vec![0]
    } else {
        (0..segments.len()).collect()
    };
    starts.iter().any(|&start| {
        let ends = glob_prefixes(&pat, &segments[start..]);
        (start + 1..=segments.len()).any(|end| {
            let covers_inside = end < segments.len();
            if (directory && !covers_inside) || (files_only && covers_inside) {
                return false;
            }
            ends[end - start]
        })
    })
}

/// For each `n`, whether `pat` matches the first `n` segments of `path`. A table of pattern by
/// path position rather than trying every split for every `**`, which a CODEOWNERS line of
/// many `**` would make take forever.
fn glob_prefixes(pat: &[&str], path: &[&str]) -> Vec<bool> {
    let mut reached = vec![true];
    reached.resize(path.len() + 1, false);
    for p in pat {
        let mut next = vec![false; path.len() + 1];
        if *p == "**" {
            // Any number of segments, from the first position reached on.
            if let Some(first) = reached.iter().position(|&r| r) {
                next[first..].fill(true);
            }
        } else {
            for i in 0..path.len() {
                next[i + 1] = reached[i] && glob(p, path[i]);
            }
        }
        reached = next;
    }
    reached
}

/// `*` and `?` within one path segment.
fn glob(pattern: &str, text: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_addresses_are_recognized() {
        let pair = |o: &str, n: &str| Some((o.to_owned(), n.to_owned()));
        assert_eq!(
            parse_github_url("https://github.com/MrFreeZZZ/oxbow.git"),
            pair("MrFreeZZZ", "oxbow")
        );
        assert_eq!(parse_github_url("https://github.com/acme/api"), pair("acme", "api"));
        assert_eq!(
            parse_github_url("https://user@github.com/acme/api.git/"),
            pair("acme", "api")
        );
        assert_eq!(parse_github_url("git@github.com:acme/api.git"), pair("acme", "api"));
        assert_eq!(
            parse_github_url("ssh://git@github.com:22/acme/api.git"),
            pair("acme", "api")
        );
        assert_eq!(parse_github_url("https://gitlab.com/acme/api.git"), None);
        assert_eq!(parse_github_url("https://github.com/acme"), None);
        assert_eq!(parse_github_url("/home/me/api"), None);
    }

    #[test]
    fn code_owner_patterns_match_like_gitignore() {
        assert!(owner_pattern_matches("*", "src/a.rs"));
        assert!(owner_pattern_matches("*.rs", "src/store/a.rs"));
        assert!(!owner_pattern_matches("*.rs", "src/a.ts"));
        assert!(owner_pattern_matches("/src/api/", "src/api/sessions.rs"));
        assert!(!owner_pattern_matches("/src/api/", "lib/src/api/x.rs"));
        assert!(owner_pattern_matches("api/", "lib/src/api/x.rs"));
        assert!(owner_pattern_matches("docs/*", "docs/a.md"));
        assert!(!owner_pattern_matches("docs/*", "docs/deep/a.md"));
        assert!(owner_pattern_matches("docs/**/a.md", "docs/x/y/a.md"));
        assert!(owner_pattern_matches("README.md", "sub/README.md"));
        assert!(owner_pattern_matches("docs/**", "docs/x/y/a.md"));
        assert!(owner_pattern_matches("**/a.md", "a.md"));
        assert!(!owner_pattern_matches("docs/**/b.md", "docs/x/y/a.md"));
        // Many `**` before a name that isn't there: answered at once, not by trying every split.
        let deep = vec!["d"; 40].join("/") + "/file.rs";
        let started = std::time::Instant::now();
        assert!(!owner_pattern_matches(&("**/".repeat(30) + "nothere"), &deep));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        let rules = parse_code_owners("# owners\n*  @acme/core\n/src/api/ @kirill maria@acme.io # api\n");
        assert_eq!(rules[1], ("/src/api/".to_owned(), vec!["kirill".to_owned()]));
    }
}
