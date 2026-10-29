//! Pull requests on GitHub: the list of a repository's pull requests, everything the Pull
//! Request screen shows about one (conversation, commits, checks, reviews, whether it can merge)
//! and the calls that change them: open, merge, retarget, mark ready, merge automatically, and
//! review: comment on lines, reply, resolve, mark files viewed, send a review.
//!
//! Reading goes through the REST API, one request per kind of thing, sent side by side. Only
//! what REST can't do goes through GraphQL: whether a review thread is resolved, marking a draft
//! ready for review, auto-merge, files viewed and reviews with replies in them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::github::{Client, Request, WEB};
use crate::repo::Repo;

/// A repository on github.com, the way a remote points at it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepo {
    pub owner: String,
    pub name: String,
    /// The remote that points at it, e.g. `origin`.
    pub remote: String,
}

impl GitHubRepo {
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    fn path(&self, rest: &str) -> String {
        format!("/repos/{}/{}{rest}", self.owner, self.name)
    }
}

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
        let local = format!("refs/heads/{base}");
        let tracking = format!("refs/remotes/{remote}/{base}");
        // Whether base has commits of its own is decided after the fetch, by what it brought:
        // a base that has them stays as it is, Pull sorts that out.
        if !self.has_ref(&local) {
            return Ok(plan);
        }
        if head.as_deref() == Some(base) {
            plan.push(
                GitCommand::new(["merge".to_owned(), "--ff-only".to_owned(), format!("{remote}/{base}")])
                    .comment(format!(
                        "your checked-out {base} catches up, unless it has commits of its own; changes of yours in the way stop it, nothing is overwritten"
                    ))
                    .only_if_ancestor(&local, &tracking),
            );
        } else if self.branch_in_use(base).ok().flatten().is_none() {
            // fetch refuses a base checked out in another worktree itself; this only keeps the
            // refusal from failing the whole step.
            plan.push(
                GitCommand::new(["fetch".to_owned(), remote.to_owned(), format!("{base}:{base}")])
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

    fn has_ref(&self, name: &str) -> bool {
        self.run(&GitCommand::new(["rev-parse", "--verify", "--quiet", name]))
            .is_ok()
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

// --- What the screen shows ---------------------------------------------------------------------

/// A GitHub account as it appears on a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub login: String,
    pub avatar_url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PullState {
    Open,
    Closed,
    Merged,
}

/// A pull request in the repository's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullSummary {
    pub number: u64,
    pub title: String,
    pub state: PullState,
    pub draft: bool,
    /// The branch it brings in.
    pub head: String,
    pub head_sha: String,
    /// The account the head branch lives under: another one for a pull request from a fork.
    pub head_owner: Option<String>,
    /// The branch it goes into.
    pub base: String,
    pub html_url: String,
    pub body: String,
    pub author: Person,
    pub created: i64,
    pub updated: i64,
    /// GraphQL's id, for the calls only GraphQL has.
    pub node_id: String,
    /// Reviews asked for and not given yet: logins, and teams as `org/team`.
    pub requested: Vec<String>,
}

/// Everything the Pull Request screen shows about one pull request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    #[serde(flatten)]
    pub summary: PullSummary,
    pub additions: u32,
    pub deletions: u32,
    pub changed_files: u32,
    pub commit_count: u32,
    /// Whether it merges without conflicts; `None` while GitHub is still working it out.
    pub mergeable: Option<bool>,
    /// GitHub's verdict: `clean`, `unstable` (a check that isn't required fails), `blocked`,
    /// `behind`, `dirty` (conflicts), `draft`, `has_hooks` or `unknown`.
    pub mergeable_state: String,
    pub merged_at: Option<i64>,
    pub merge_commit: Option<String>,
    pub merged_by: Option<String>,
    /// The method auto-merge will use, when it is on.
    pub auto_merge: Option<String>,
    pub commits: Vec<PullCommit>,
    /// `commits` has every commit: GitHub lists at most 250, so a bigger pull request can't
    /// say which commits a restack leaves out.
    pub commits_complete: bool,
    /// Each reviewer's latest say, in the order they first spoke.
    pub reviews: Vec<Review>,
    /// The conversation, oldest first; the description is not part of it.
    pub timeline: Vec<Entry>,
    pub checks: Vec<Check>,
    /// Approvals the base branch's rules ask for, when this account may read them.
    pub required_approvals: Option<u32>,
    pub settings: RepoSettings,
    /// What only a merge request on GitLab has; `None` on GitHub.
    pub gitlab: Option<crate::gitlab_api::GitLabFacts>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullCommit {
    pub sha: String,
    pub summary: String,
    pub message: String,
    pub author: String,
    pub time: i64,
    /// Lines added and deleted, when the commit is here to count them.
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub author: Person,
    /// `approved`, `changesRequested`, `commented` or `dismissed`.
    pub state: String,
    pub time: i64,
}

/// A line of code a review comment is about, with the lines above it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HunkLine {
    /// Its number in the new file; `None` for a deleted line.
    pub number: Option<u32>,
    pub text: String,
    /// The line the comment is on.
    pub mark: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub author: Person,
    pub body: String,
    pub time: i64,
}

/// One entry of the conversation.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Entry {
    Comment {
        author: Person,
        body: String,
        time: i64,
    },
    Review {
        author: Person,
        state: String,
        body: String,
        time: i64,
    },
    /// Comments on a line of code, the first one and its replies.
    Thread {
        /// The first comment's id, which replies go to.
        id: u64,
        /// GraphQL's id of the thread, for resolving it and replying in a review.
        node: Option<String>,
        path: String,
        line: Option<u32>,
        /// The first line of a comment on several lines.
        start_line: Option<u32>,
        /// `RIGHT` for a line of the new file, `LEFT` for a deleted one.
        side: String,
        hunk: Vec<HunkLine>,
        comments: Vec<Comment>,
        /// `None` when GitHub didn't say.
        resolved: Option<bool>,
        /// The code changed since, so the comment is about an older version.
        outdated: bool,
        /// The commit whose lines it is on, when GitHub or GitLab says: a suggestion in it is
        /// applied from that version of the file.
        #[serde(default)]
        commit: Option<String>,
        time: i64,
    },
    /// Something that happened, in one line: "pushed 2 commits".
    Event {
        actor: String,
        text: String,
        /// `push`, `ok`, `fail`, `review`, `draft` or `dot`.
        icon: String,
        /// `ok` or `bad` for a colored dot.
        tone: Option<String>,
        time: i64,
    },
}

impl Entry {
    fn time(&self) -> i64 {
        match self {
            Entry::Comment { time, .. }
            | Entry::Review { time, .. }
            | Entry::Thread { time, .. }
            | Entry::Event { time, .. } => *time,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub name: String,
    pub detail: String,
    /// `ok`, `fail`, `run` or `skip`; `warn` for a GitLab job that failed but may.
    pub state: String,
    /// How long it took, once it finished.
    pub seconds: Option<i64>,
    pub url: Option<String>,
}

/// What the repository allows when merging.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoSettings {
    pub merge_commit: bool,
    pub squash: bool,
    pub rebase: bool,
    pub auto_merge: bool,
    /// GitHub deletes the head branch itself after merging.
    pub delete_branch_on_merge: bool,
    pub default_branch: String,
    /// The account may push, and so merge.
    pub can_push: bool,
}

// --- What changes them -------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Method {
    Merge,
    Squash,
    Rebase,
}

impl Method {
    fn rest(self) -> &'static str {
        match self {
            Method::Merge => "merge",
            Method::Squash => "squash",
            Method::Rebase => "rebase",
        }
    }

    fn graphql(self) -> &'static str {
        match self {
            Method::Merge => "MERGE",
            Method::Squash => "SQUASH",
            Method::Rebase => "REBASE",
        }
    }
}

/// What a review says of the pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Comment,
    Approve,
    RequestChanges,
}

impl Verdict {
    fn graphql(self) -> &'static str {
        match self {
            Verdict::Comment => "COMMENT",
            Verdict::Approve => "APPROVE",
            Verdict::RequestChanges => "REQUEST_CHANGES",
        }
    }
}

/// The lines a comment is on, as GitHub numbers them: `line` (and `start_line` for several) of
/// the new file on the `RIGHT`, of the old one on the `LEFT` for deleted lines.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spot {
    pub path: String,
    pub line: u32,
    pub side: String,
    #[serde(default)]
    pub start_line: Option<u32>,
    /// Both numbers of its last and first line, as GitLab names a line of a diff.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lines: Option<SpotLines>,
}

/// The last line of a spot and, for several, its first, as GitLab numbers lines of a diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpotLines {
    pub end: LinePair,
    #[serde(default)]
    pub start: Option<LinePair>,
}

/// A line of a diff by both files' counts at its row: an added line has the old file's next
/// number, a deleted one the new file's next, as GitLab counts them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinePair {
    pub old: u32,
    pub new: u32,
    /// `added`, `removed` or `context`.
    pub kind: String,
}

/// What a merge request on GitLab is opened with besides a pull request's fields: people by
/// their account ids, labels by name.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabCreate {
    #[serde(default)]
    pub reviewer_ids: Vec<u64>,
    #[serde(default)]
    pub assignee_ids: Vec<u64>,
    #[serde(default)]
    pub labels: Vec<String>,
    /// GitLab deletes the source branch once it is merged.
    #[serde(default)]
    pub remove_source_branch: bool,
    /// It is squashed into one commit when merged.
    #[serde(default)]
    pub squash: bool,
}

/// A change to a pull request, shown as its request before it is sent. `number` 0 means the
/// pull request a `Create` earlier in the same run made, a `review` of `None` the review a
/// `StartReview` earlier in the run started.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Call {
    Merge {
        number: u64,
        method: Method,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        message: Option<String>,
        /// Merge only if the head is still this commit.
        sha: String,
        /// GitLab deletes the source branch once merged (GitHub's is deleted by a push).
        #[serde(default)]
        delete_branch: bool,
    },
    Create {
        head: String,
        base: String,
        title: String,
        body: String,
        draft: bool,
        /// What only a merge request on GitLab is opened with.
        #[serde(default)]
        gitlab: Option<GitLabCreate>,
    },
    RequestReviewers {
        number: u64,
        reviewers: Vec<String>,
        /// GitLab: every reviewer it is to have, by account id, the ones it has included.
        #[serde(default)]
        ids: Vec<u64>,
    },
    Update {
        number: u64,
        #[serde(default)]
        base: Option<String>,
        #[serde(default)]
        body: Option<String>,
    },
    ReadyForReview {
        number: u64,
        id: String,
        /// The title it gets: GitLab marks a draft by the title's `Draft:`.
        #[serde(default)]
        title: Option<String>,
    },
    /// Make it a draft again.
    ConvertToDraft {
        number: u64,
        id: String,
        #[serde(default)]
        title: Option<String>,
    },
    /// Close it, or open it again.
    SetState { number: u64, open: bool },
    /// Merge the base into the head on GitHub, like its Update branch button, if the head is
    /// still this commit.
    UpdateBranch { number: u64, sha: String },
    /// Turn auto-merge on with `method`, or off without one.
    AutoMerge {
        number: u64,
        id: String,
        #[serde(default)]
        method: Option<Method>,
    },
    /// Tick or untick a file's Viewed box, as on GitHub.
    Viewed { id: String, path: String, viewed: bool },
    /// A comment on lines of `commit`, sent at once rather than in a review.
    Comment {
        number: u64,
        commit: String,
        spot: Spot,
        body: String,
    },
    /// A reply, sent at once, to the thread comment `to` starts.
    Reply {
        number: u64,
        to: u64,
        body: String,
        /// GitLab: the discussion, by its id.
        #[serde(default)]
        thread: Option<String>,
    },
    /// Resolve the thread with GraphQL id `thread`, or open it again.
    Resolve {
        thread: String,
        resolved: bool,
        /// GitLab: the merge request the discussion is on.
        #[serde(default)]
        number: u64,
    },
    /// Start a review of `commit`, pending until it is submitted.
    StartReview { id: String, commit: String },
    /// A comment in a pending review.
    ReviewComment {
        #[serde(default)]
        review: Option<String>,
        spot: Spot,
        body: String,
    },
    /// A reply in a pending review to the thread with GraphQL id `thread`.
    ReviewReply {
        #[serde(default)]
        review: Option<String>,
        thread: String,
        body: String,
    },
    /// Send a pending review with its verdict and summary.
    SubmitReview {
        #[serde(default)]
        review: Option<String>,
        verdict: Verdict,
        body: String,
        /// GitLab: take back the approval this account gave before.
        #[serde(default)]
        revoke: bool,
    },
    /// Throw away a pending review.
    DeleteReview {
        #[serde(default)]
        review: Option<String>,
    },
}

impl Call {
    /// The review a `StartReview` made, for the calls of the run that go into it.
    pub fn in_review(&mut self, id: &str) {
        match self {
            Call::ReviewComment { review, .. }
            | Call::ReviewReply { review, .. }
            | Call::SubmitReview { review, .. }
            | Call::DeleteReview { review }
                if review.is_none() =>
            {
                *review = Some(id.to_owned());
            }
            _ => {}
        }
    }
}

const READY: &str =
    "mutation($id: ID!) { markPullRequestReadyForReview(input: {pullRequestId: $id}) { pullRequest { isDraft } } }";
const DRAFT: &str =
    "mutation($id: ID!) { convertPullRequestToDraft(input: {pullRequestId: $id}) { pullRequest { isDraft } } }";
const AUTO_ON: &str = "mutation($id: ID!, $method: PullRequestMergeMethod!) { enablePullRequestAutoMerge(input: {pullRequestId: $id, mergeMethod: $method}) { clientMutationId } }";
const AUTO_OFF: &str =
    "mutation($id: ID!) { disablePullRequestAutoMerge(input: {pullRequestId: $id}) { clientMutationId } }";
const VIEWED: &str = "mutation($id: ID!, $path: String!) { markFileAsViewed(input: {pullRequestId: $id, path: $path}) { clientMutationId } }";
const UNVIEWED: &str = "mutation($id: ID!, $path: String!) { unmarkFileAsViewed(input: {pullRequestId: $id, path: $path}) { clientMutationId } }";
const RESOLVE: &str = "mutation($id: ID!) { resolveReviewThread(input: {threadId: $id}) { thread { isResolved } } }";
const UNRESOLVE: &str =
    "mutation($id: ID!) { unresolveReviewThread(input: {threadId: $id}) { thread { isResolved } } }";
const START_REVIEW: &str = "mutation($id: ID!, $commit: GitObjectID!) { addPullRequestReview(input: {pullRequestId: $id, commitOID: $commit}) { pullRequestReview { id } } }";
const REVIEW_COMMENT: &str = "mutation($review: ID!, $path: String!, $line: Int!, $side: DiffSide!, $startLine: Int, $startSide: DiffSide, $body: String!) { addPullRequestReviewThread(input: {pullRequestReviewId: $review, path: $path, line: $line, side: $side, startLine: $startLine, startSide: $startSide, body: $body}) { thread { id } } }";
const REVIEW_REPLY: &str = "mutation($review: ID!, $thread: ID!, $body: String!) { addPullRequestReviewThreadReply(input: {pullRequestReviewId: $review, pullRequestReviewThreadId: $thread, body: $body}) { comment { id } } }";
const SUBMIT_REVIEW: &str = "mutation($review: ID!, $event: PullRequestReviewEvent!, $body: String) { submitPullRequestReview(input: {pullRequestReviewId: $review, event: $event, body: $body}) { pullRequestReview { url } } }";
const DELETE_REVIEW: &str =
    "mutation($review: ID!) { deletePullRequestReview(input: {pullRequestReviewId: $review}) { clientMutationId } }";
const THREADS: &str = "query($owner: String!, $name: String!, $number: Int!) { repository(owner: $owner, name: $name) { pullRequest(number: $number) { reviewThreads(first: 100) { nodes { id isResolved comments(first: 1) { nodes { databaseId } } } } } } }";

impl Client {
    // Requests, to show and then send.

    pub fn pulls_request(&self, repo: &GitHubRepo) -> Request {
        self.api_request(
            "GET",
            &repo.path("/pulls?state=all&sort=updated&direction=desc&per_page=100"),
            None,
        )
    }

    pub fn pull_request_request(&self, repo: &GitHubRepo, number: u64) -> Request {
        self.api_request("GET", &repo.path(&format!("/pulls/{number}")), None)
    }

    pub(crate) fn graphql_request(&self, query: &str, variables: Value) -> Request {
        self.api_request(
            "POST",
            "/graphql",
            Some(json!({ "query": query, "variables": variables })),
        )
    }

    /// The request `call` sends; `created` is the number a `Create` earlier in the run made.
    pub fn call_request(&self, repo: &GitHubRepo, call: &Call, created: Option<u64>) -> Request {
        let number = |n: u64| {
            if n == 0 {
                created.map_or("$NUMBER".to_owned(), |c| c.to_string())
            } else {
                n.to_string()
            }
        };
        match call {
            Call::Merge {
                number: n,
                method,
                title,
                message,
                sha,
                ..
            } => {
                let mut body = json!({ "merge_method": method.rest(), "sha": sha });
                if let Some(title) = title {
                    body["commit_title"] = json!(title);
                }
                if let Some(message) = message {
                    body["commit_message"] = json!(message);
                }
                self.api_request("PUT", &repo.path(&format!("/pulls/{}/merge", number(*n))), Some(body))
            }
            Call::Create {
                head,
                base,
                title,
                body,
                draft,
                ..
            } => self.api_request(
                "POST",
                &repo.path("/pulls"),
                Some(json!({ "title": title, "head": head, "base": base, "body": body, "draft": draft })),
            ),
            Call::RequestReviewers {
                number: n, reviewers, ..
            } => {
                let (teams, users): (Vec<&String>, Vec<&String>) = reviewers.iter().partition(|r| r.contains('/'));
                let mut body = json!({ "reviewers": users });
                if !teams.is_empty() {
                    let slugs: Vec<&str> = teams.iter().filter_map(|t| t.split('/').nth(1)).collect();
                    body["team_reviewers"] = json!(slugs);
                }
                self.api_request(
                    "POST",
                    &repo.path(&format!("/pulls/{}/requested_reviewers", number(*n))),
                    Some(body),
                )
            }
            Call::Update { number: n, base, body } => {
                let mut json = json!({});
                if let Some(base) = base {
                    json["base"] = json!(base);
                }
                if let Some(body) = body {
                    json["body"] = json!(body);
                }
                self.api_request("PATCH", &repo.path(&format!("/pulls/{}", number(*n))), Some(json))
            }
            Call::UpdateBranch { number: n, sha } => self.api_request(
                "PUT",
                &repo.path(&format!("/pulls/{}/update-branch", number(*n))),
                Some(json!({ "expected_head_sha": sha })),
            ),
            Call::ReadyForReview { id, .. } => self.graphql_request(READY, json!({ "id": id })),
            Call::ConvertToDraft { id, .. } => self.graphql_request(DRAFT, json!({ "id": id })),
            Call::SetState { number: n, open } => self.api_request(
                "PATCH",
                &repo.path(&format!("/pulls/{}", number(*n))),
                Some(json!({ "state": if *open { "open" } else { "closed" } })),
            ),
            Call::AutoMerge { id, method, .. } => match method {
                Some(method) => self.graphql_request(AUTO_ON, json!({ "id": id, "method": method.graphql() })),
                None => self.graphql_request(AUTO_OFF, json!({ "id": id })),
            },
            Call::Viewed { id, path, viewed } => self.graphql_request(
                if *viewed { VIEWED } else { UNVIEWED },
                json!({ "id": id, "path": path }),
            ),
            Call::Comment {
                number: n,
                commit,
                spot,
                body,
            } => {
                let mut json = json!({ "body": body, "commit_id": commit, "path": spot.path, "line": spot.line, "side": spot.side });
                if let Some(start) = spot.start_line {
                    json["start_line"] = json!(start);
                    json["start_side"] = json!(spot.side);
                }
                self.api_request(
                    "POST",
                    &repo.path(&format!("/pulls/{}/comments", number(*n))),
                    Some(json),
                )
            }
            Call::Reply {
                number: n, to, body, ..
            } => self.api_request(
                "POST",
                &repo.path(&format!("/pulls/{}/comments/{to}/replies", number(*n))),
                Some(json!({ "body": body })),
            ),
            Call::Resolve { thread, resolved, .. } => {
                self.graphql_request(if *resolved { RESOLVE } else { UNRESOLVE }, json!({ "id": thread }))
            }
            Call::StartReview { id, commit } => {
                self.graphql_request(START_REVIEW, json!({ "id": id, "commit": commit }))
            }
            Call::ReviewComment { review, spot, body } => self.graphql_request(
                REVIEW_COMMENT,
                json!({
                    "review": review_id(review),
                    "path": spot.path,
                    "line": spot.line,
                    "side": spot.side,
                    "startLine": spot.start_line,
                    "startSide": spot.start_line.map(|_| &spot.side),
                    "body": body,
                }),
            ),
            Call::ReviewReply { review, thread, body } => self.graphql_request(
                REVIEW_REPLY,
                json!({ "review": review_id(review), "thread": thread, "body": body }),
            ),
            Call::SubmitReview {
                review, verdict, body, ..
            } => self.graphql_request(
                SUBMIT_REVIEW,
                json!({ "review": review_id(review), "event": verdict.graphql(), "body": body }),
            ),
            Call::DeleteReview { review } => {
                self.graphql_request(DELETE_REVIEW, json!({ "review": review_id(review) }))
            }
        }
    }

    /// Send `call`. A `Create` answers with the new pull request.
    pub fn perform(&self, repo: &GitHubRepo, call: &Call, created: Option<u64>) -> Result<Value> {
        let request = self.call_request(repo, call, created);
        let response = self.call(&request)?;
        if let Some(message) = graphql_error(&response.body) {
            return Err(Error::GitHub {
                status: Some(response.status),
                message,
            });
        }
        Ok(response.body)
    }

    // Reading.

    /// The repository's pull requests, open and recently closed, the latest updated first.
    pub fn pulls(&self, repo: &GitHubRepo) -> Result<Vec<PullSummary>> {
        let body = self.call(&self.pulls_request(repo))?.body;
        Ok(body
            .as_array()
            .map(|list| list.iter().map(summary).collect())
            .unwrap_or_default())
    }

    /// Every item of a list GitHub hands out 100 at a time, from at most `most` pages: a page
    /// with fewer is the last one.
    fn pages(&self, repo: &GitHubRepo, path: &str, most: u32) -> Result<Value> {
        let mut items = Vec::new();
        for page in 1..=most {
            let request = self.api_request("GET", &repo.path(&format!("{path}?per_page=100&page={page}")), None);
            let body = self.call(&request)?.body;
            let list = match body {
                Value::Array(list) => list,
                _ => break,
            };
            let last = list.len() < 100;
            items.extend(list);
            if last {
                break;
            }
        }
        Ok(Value::Array(items))
    }

    /// Everything about pull request `number`. Parts this account may not read are left empty.
    pub fn pull_request(&self, repo: &GitHubRepo, number: u64) -> Result<PullRequest> {
        let get = |path: String| {
            let request = self.api_request("GET", &repo.path(&path), None);
            move || self.call(&request).map(|r| r.body)
        };
        let all = |path: String, most: u32| move || self.pages(repo, &path, most);
        let (pull, commits, comments, timeline, settings) = std::thread::scope(|s| {
            let pull = s.spawn(get(format!("/pulls/{number}")));
            // GitHub lists at most 250 commits of a pull request, however many pages are asked for.
            let commits = s.spawn(all(format!("/pulls/{number}/commits"), 3));
            let comments = s.spawn(all(format!("/pulls/{number}/comments"), 10));
            let timeline = s.spawn(all(format!("/issues/{number}/timeline"), 10));
            let settings = s.spawn(get(String::new()));
            (
                pull.join(),
                commits.join(),
                comments.join(),
                timeline.join(),
                settings.join(),
            )
        });
        let joined = |r: std::thread::Result<Result<Value>>| r.unwrap_or(Ok(Value::Null));
        let pull = joined(pull)?;
        let summary = summary(&pull);
        let sha = summary.head_sha.clone();
        let base = summary.base.clone();
        let (runs, statuses, rules, threads) = std::thread::scope(|s| {
            let runs = s.spawn(get(format!("/commits/{sha}/check-runs?per_page=100")));
            let statuses = s.spawn(get(format!("/commits/{sha}/status")));
            let rules = s.spawn(get(format!("/rules/branches/{}", path_part(&base))));
            let threads = s.spawn(|| {
                let request = self.graphql_request(
                    THREADS,
                    json!({ "owner": repo.owner, "name": repo.name, "number": number }),
                );
                self.call(&request).map(|r| r.body)
            });
            (runs.join(), statuses.join(), rules.join(), threads.join())
        });
        let optional = |r: std::thread::Result<Result<Value>>| joined(r).unwrap_or(Value::Null);
        let settings = repo_settings(&joined(settings)?);
        let commits = joined(commits)?;
        let comments = optional(comments);
        let timeline = optional(timeline);
        let resolved = resolved_threads(&optional(threads));

        let commits: Vec<PullCommit> = commits
            .as_array()
            .map(|c| c.iter().map(pull_commit).collect())
            .unwrap_or_default();
        let commit_count = pull.get("commits").and_then(Value::as_u64).unwrap_or(0);
        let mut entries = conversation(&timeline, &comments, &resolved);
        entries.sort_by_key(Entry::time);
        let reviews = latest_reviews(&timeline);
        let checks = checks(&optional(runs), &optional(statuses));
        let required_approvals = required_approvals(&optional(rules));

        let number_field = |key: &str| pull.get(key).and_then(Value::as_u64).unwrap_or(0) as u32;
        let summary_state = summary.state;
        Ok(PullRequest {
            additions: number_field("additions"),
            deletions: number_field("deletions"),
            changed_files: number_field("changed_files"),
            commit_count: number_field("commits"),
            commits_complete: commits.len() as u64 >= commit_count,
            mergeable: pull.get("mergeable").and_then(Value::as_bool),
            mergeable_state: pull
                .get("mergeable_state")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned(),
            merged_at: (summary_state == PullState::Merged).then(|| time(&pull, "merged_at")),
            merge_commit: (summary_state == PullState::Merged)
                .then(|| pull.get("merge_commit_sha").and_then(Value::as_str).map(str::to_owned))
                .flatten(),
            merged_by: pull
                .pointer("/merged_by/login")
                .and_then(Value::as_str)
                .map(str::to_owned),
            auto_merge: pull
                .pointer("/auto_merge/merge_method")
                .and_then(Value::as_str)
                .map(str::to_owned),
            commits,
            reviews,
            timeline: entries,
            checks,
            required_approvals,
            settings,
            gitlab: None,
            summary,
        })
    }
}

/// A review's id, or what stands for the one the run starts until it has one.
fn review_id(review: &Option<String>) -> &str {
    review.as_deref().unwrap_or("$REVIEW")
}

/// The id of the review a `StartReview` answered with.
pub fn started_review(answer: &Value) -> Option<String> {
    answer
        .pointer("/data/addPullRequestReview/pullRequestReview/id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The message of a GraphQL answer that failed, which comes with status 200.
pub(crate) fn graphql_error(body: &Value) -> Option<String> {
    let errors = body.get("errors")?.as_array()?;
    let messages: Vec<&str> = errors
        .iter()
        .filter_map(|e| e.get("message").and_then(Value::as_str))
        .collect();
    (!messages.is_empty()).then(|| messages.join(". "))
}

/// A branch name in a URL path: slashes stay, other unsafe characters are encoded.
fn path_part(name: &str) -> String {
    name.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub(crate) fn text(value: &Value, key: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

fn time(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(Value::as_str).map(parse_time).unwrap_or(0)
}

fn person(value: Option<&Value>) -> Person {
    let field = |key: &str| {
        value
            .and_then(|v| v.get(key))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Person {
        login: field("login"),
        avatar_url: field("avatar_url"),
    }
}

fn summary(pull: &Value) -> PullSummary {
    let merged = pull.get("merged_at").is_some_and(|m| !m.is_null())
        || pull.get("merged").and_then(Value::as_bool) == Some(true);
    let state = match pull.get("state").and_then(Value::as_str) {
        _ if merged => PullState::Merged,
        Some("closed") => PullState::Closed,
        _ => PullState::Open,
    };
    PullSummary {
        number: pull.get("number").and_then(Value::as_u64).unwrap_or(0),
        title: text(pull, "title"),
        state,
        draft: pull.get("draft").and_then(Value::as_bool).unwrap_or(false),
        head: pull
            .pointer("/head/ref")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        head_sha: pull
            .pointer("/head/sha")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        head_owner: pull
            .pointer("/head/repo/owner/login")
            .and_then(Value::as_str)
            .map(str::to_owned),
        base: pull
            .pointer("/base/ref")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        html_url: text(pull, "html_url"),
        body: text(pull, "body"),
        author: person(pull.get("user")),
        created: time(pull, "created_at"),
        updated: time(pull, "updated_at"),
        node_id: text(pull, "node_id"),
        requested: requested(pull),
    }
}

fn pull_commit(item: &Value) -> PullCommit {
    let message = item
        .pointer("/commit/message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    PullCommit {
        sha: text(item, "sha"),
        summary: message.lines().next().unwrap_or_default().to_owned(),
        author: item
            .pointer("/commit/author/name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        time: item
            .pointer("/commit/committer/date")
            .and_then(Value::as_str)
            .map(parse_time)
            .unwrap_or(0),
        message,
        additions: None,
        deletions: None,
    }
}

fn repo_settings(repo: &Value) -> RepoSettings {
    // Fields an account without admin rights doesn't get count as GitHub's defaults.
    let flag = |key: &str, default: bool| repo.get(key).and_then(Value::as_bool).unwrap_or(default);
    RepoSettings {
        merge_commit: flag("allow_merge_commit", true),
        squash: flag("allow_squash_merge", true),
        rebase: flag("allow_rebase_merge", true),
        auto_merge: flag("allow_auto_merge", false),
        delete_branch_on_merge: flag("delete_branch_on_merge", false),
        default_branch: repo
            .get("default_branch")
            .and_then(Value::as_str)
            .unwrap_or("main")
            .to_owned(),
        can_push: repo
            .pointer("/permissions/push")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn requested(pull: &Value) -> Vec<String> {
    let users = pull
        .get("requested_reviewers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|u| u.get("login").and_then(Value::as_str).map(str::to_owned));
    let owner = pull
        .pointer("/base/repo/owner/login")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let teams = pull
        .get("requested_teams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|t| {
            t.get("slug")
                .and_then(Value::as_str)
                .map(|slug| format!("{owner}/{slug}"))
        });
    users.chain(teams).collect()
}

fn review_state(state: &str) -> String {
    match state.to_ascii_lowercase().as_str() {
        "approved" => "approved",
        "changes_requested" => "changesRequested",
        "dismissed" => "dismissed",
        "pending" => "pending",
        _ => "commented",
    }
    .to_owned()
}

/// Each reviewer's latest review that counts: an approval or a request for changes outweighs a
/// later plain comment, as on GitHub.
fn latest_reviews(timeline: &Value) -> Vec<Review> {
    let mut order: Vec<String> = Vec::new();
    let mut latest: HashMap<String, Review> = HashMap::new();
    for item in timeline.as_array().into_iter().flatten() {
        if item.get("event").and_then(Value::as_str) != Some("reviewed") {
            continue;
        }
        let author = person(item.get("user"));
        let state = review_state(item.get("state").and_then(Value::as_str).unwrap_or_default());
        if state == "pending" || author.login.is_empty() {
            continue;
        }
        let review = Review {
            author: author.clone(),
            state,
            time: time(item, "submitted_at"),
        };
        match latest.get(&author.login) {
            None => {
                order.push(author.login.clone());
                latest.insert(author.login, review);
            }
            Some(before) if review.state != "commented" || before.state == "commented" => {
                latest.insert(author.login, review);
            }
            Some(_) => {}
        }
    }
    order.into_iter().filter_map(|login| latest.remove(&login)).collect()
}

/// The conversation from the timeline and the review comments: comments, reviews with something
/// to say, threads on lines of code and one-line events. Commits in a row are one event.
fn conversation(timeline: &Value, comments: &Value, resolved: &HashMap<u64, (bool, String)>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut pushed: Option<(String, usize, i64)> = None;
    let mut merged = false;
    let flush = |pushed: &mut Option<(String, usize, i64)>, out: &mut Vec<Entry>| {
        if let Some((actor, count, at)) = pushed.take() {
            out.push(Entry::Event {
                actor,
                text: format!("pushed {count} {}", if count == 1 { "commit" } else { "commits" }),
                icon: "push".into(),
                tone: None,
                time: at,
            });
        }
    };
    for item in timeline.as_array().into_iter().flatten() {
        let event = item.get("event").and_then(Value::as_str).unwrap_or_default();
        if event == "committed" {
            let author = item
                .pointer("/author/name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let at = item
                .pointer("/committer/date")
                .and_then(Value::as_str)
                .map(parse_time)
                .unwrap_or(0);
            pushed = Some(match pushed.take() {
                Some((who, count, _)) => (who, count + 1, at),
                None => (author, 1, at),
            });
            continue;
        }
        flush(&mut pushed, &mut out);
        let actor = item
            .pointer("/actor/login")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let at = time(item, "created_at");
        merged |= event == "merged";
        let line = |text: String, icon: &str, tone: Option<&str>| Entry::Event {
            actor: actor.clone(),
            text,
            icon: icon.into(),
            tone: tone.map(str::to_owned),
            time: at,
        };
        let entry = match event {
            "commented" => Some(Entry::Comment {
                author: person(item.get("user").or(item.get("actor"))),
                body: text(item, "body"),
                time: at,
            }),
            "reviewed" => {
                let state = review_state(item.get("state").and_then(Value::as_str).unwrap_or_default());
                let body = text(item, "body");
                // A plain comment review with no words of its own only holds line comments,
                // which show as threads.
                (state != "pending" && !(state == "commented" && body.trim().is_empty())).then(|| Entry::Review {
                    author: person(item.get("user")),
                    state,
                    body,
                    time: time(item, "submitted_at"),
                })
            }
            "review_requested" => {
                let who = item
                    .pointer("/requested_reviewer/login")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .or_else(|| {
                        item.pointer("/requested_team/name")
                            .and_then(Value::as_str)
                            .map(|t| format!("team {t}"))
                    })
                    .unwrap_or_default();
                Some(line(format!("requested a review from {who}"), "review", None))
            }
            "merged" => {
                let sha = item.get("commit_id").and_then(Value::as_str).unwrap_or_default();
                Some(line(
                    format!("merged it as {}", &sha[..sha.len().min(7)]),
                    "ok",
                    Some("ok"),
                ))
            }
            // GitHub closes a pull request as it merges it: the merge says it all.
            "closed" if merged => None,
            "closed" => Some(line("closed it".into(), "fail", Some("bad"))),
            "reopened" => Some(line("reopened it".into(), "dot", None)),
            "ready_for_review" => Some(line("marked it ready for review".into(), "review", None)),
            "convert_to_draft" => Some(line("turned it back into a draft".into(), "draft", None)),
            "head_ref_force_pushed" => Some(line("force-pushed the branch".into(), "push", None)),
            "head_ref_deleted" => Some(line("deleted the branch".into(), "dot", None)),
            "head_ref_restored" => Some(line("restored the branch".into(), "dot", None)),
            "base_ref_changed" => Some(line("changed the base branch".into(), "dot", None)),
            "renamed" => {
                let to = item.pointer("/rename/to").and_then(Value::as_str).unwrap_or_default();
                Some(line(format!("renamed it to “{to}”"), "dot", None))
            }
            "auto_merge_enabled" => Some(line("turned on auto-merge".into(), "ok", None)),
            "auto_merge_disabled" => Some(line("turned off auto-merge".into(), "dot", None)),
            _ => None,
        };
        out.extend(entry);
    }
    flush(&mut pushed, &mut out);
    out.extend(threads(comments, resolved));
    out
}

/// Review comments grouped into threads: each first comment with the replies to it.
fn threads(comments: &Value, resolved: &HashMap<u64, (bool, String)>) -> Vec<Entry> {
    let all: Vec<&Value> = comments.as_array().into_iter().flatten().collect();
    let mut out = Vec::new();
    for first in all
        .iter()
        .filter(|c| c.get("in_reply_to_id").is_none_or(Value::is_null))
    {
        let id = first.get("id").and_then(Value::as_u64).unwrap_or(0);
        let comment = |c: &Value| Comment {
            author: person(c.get("user")),
            body: text(c, "body"),
            time: time(c, "created_at"),
        };
        let mut list = vec![comment(first)];
        list.extend(
            all.iter()
                .filter(|c| c.get("in_reply_to_id").and_then(Value::as_u64) == Some(id))
                .map(|c| comment(c)),
        );
        let number = |key: &str| first.get(key).and_then(Value::as_u64).map(|n| n as u32);
        let line = number("line");
        let thread = resolved.get(&id);
        out.push(Entry::Thread {
            id,
            node: thread.map(|(_, node)| node.clone()),
            path: text(first, "path"),
            hunk: hunk_tail(
                &text(first, "diff_hunk"),
                4,
                number("start_line").filter(|_| line.is_some() && text(first, "side") != "LEFT"),
            ),
            outdated: line.is_none(),
            start_line: if line.is_some() {
                number("start_line")
            } else {
                number("original_start_line")
            },
            side: first.get("side").and_then(Value::as_str).unwrap_or("RIGHT").to_owned(),
            line: line.or_else(|| number("original_line")),
            resolved: thread.map(|(resolved, _)| *resolved),
            commit: first.get("commit_id").and_then(Value::as_str).map(str::to_owned),
            time: list[0].time,
            comments: list,
        });
    }
    out
}

/// The last `count` lines of a review comment's hunk, numbered as in the new file; the last one
/// is the line commented on. A comment on new lines `from` and on keeps all of them, marked.
pub fn hunk_tail(hunk: &str, count: usize, from: Option<u32>) -> Vec<HunkLine> {
    let mut lines = hunk.lines();
    let start = lines
        .next()
        .and_then(|header| header.split('+').nth(1))
        .and_then(|new| new.split([',', ' ']).next())
        .and_then(|n| n.parse::<u32>().ok())
        .unwrap_or(1);
    let mut number = start;
    let mut out: Vec<HunkLine> = Vec::new();
    for line in lines {
        let (mark, rest) = line.split_at(line.len().min(1));
        let numbered = if mark == "-" {
            None
        } else {
            number += 1;
            Some(number - 1)
        };
        out.push(HunkLine {
            number: numbered,
            text: rest.to_owned(),
            mark: false,
        });
    }
    let mut skip = out.len().saturating_sub(count);
    if let Some(from) = from
        && let Some(at) = out.iter().position(|l| l.number == Some(from))
    {
        skip = skip.min(at);
        for line in &mut out[at..] {
            line.mark = line.number.is_some();
        }
    }
    let mut tail: Vec<HunkLine> = out.into_iter().skip(skip).collect();
    if let Some(last) = tail.last_mut() {
        last.mark = true;
    }
    tail
}

/// Whether each review thread is resolved, and its GraphQL id, by the id of its first comment.
fn resolved_threads(answer: &Value) -> HashMap<u64, (bool, String)> {
    answer
        .pointer("/data/repository/pullRequest/reviewThreads/nodes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|thread| {
            let id = thread.pointer("/comments/nodes/0/databaseId").and_then(Value::as_u64)?;
            Some((
                id,
                (
                    thread.get("isResolved").and_then(Value::as_bool).unwrap_or(false),
                    text(thread, "id"),
                ),
            ))
        })
        .collect()
}

/// Check runs and commit statuses as one list; a check run again only counts its latest run.
fn checks(runs: &Value, statuses: &Value) -> Vec<Check> {
    let mut out: Vec<Check> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut list: Vec<&Value> = runs
        .get("check_runs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    list.sort_by_key(|run| std::cmp::Reverse(time(run, "started_at")));
    for run in list {
        let name = text(run, "name");
        if !seen.insert(name.clone()) {
            continue;
        }
        let state = match (
            run.get("status").and_then(Value::as_str),
            run.get("conclusion").and_then(Value::as_str),
        ) {
            (Some("completed"), Some("success")) => "ok",
            (Some("completed"), Some("skipped" | "neutral" | "stale")) => "skip",
            (Some("completed"), _) => "fail",
            _ => "run",
        };
        let (started, finished) = (time(run, "started_at"), time(run, "completed_at"));
        out.push(Check {
            name,
            detail: String::new(),
            state: state.into(),
            seconds: (state != "run" && started > 0 && finished >= started).then_some(finished - started),
            url: run.get("html_url").and_then(Value::as_str).map(str::to_owned),
        });
    }
    for status in statuses.get("statuses").and_then(Value::as_array).into_iter().flatten() {
        let name = text(status, "context");
        if !seen.insert(name.clone()) {
            continue;
        }
        let state = match status.get("state").and_then(Value::as_str) {
            Some("success") => "ok",
            Some("pending") => "run",
            _ => "fail",
        };
        out.push(Check {
            name,
            detail: text(status, "description"),
            state: state.into(),
            seconds: None,
            url: status.get("target_url").and_then(Value::as_str).map(str::to_owned),
        });
    }
    out
}

fn required_approvals(rules: &Value) -> Option<u32> {
    rules
        .as_array()?
        .iter()
        .filter(|rule| rule.get("type").and_then(Value::as_str) == Some("pull_request"))
        .filter_map(|rule| {
            rule.pointer("/parameters/required_approving_review_count")
                .and_then(Value::as_u64)
        })
        .max()
        .map(|n| n as u32)
}

/// Seconds since 1970 from GitHub's `2026-10-08T15:22:22Z`; 0 for anything else.
pub fn parse_time(text: &str) -> i64 {
    let digits = |range: std::ops::Range<usize>| text.get(range).and_then(|s| s.parse::<i64>().ok());
    let (Some(year), Some(month), Some(day), Some(hour), Some(minute), Some(second)) = (
        digits(0..4),
        digits(5..7),
        digits(8..10),
        digits(11..13),
        digits(14..16),
        digits(17..19),
    ) else {
        return 0;
    };
    // Days from civil, after Howard Hinnant.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut seconds = days * 86_400 + hour * 3600 + minute * 60 + second;
    // An offset like +02:00 instead of Z.
    if let Some(sign @ ('+' | '-')) = text.chars().nth(19)
        && let (Some(h), Some(m)) = (digits(20..22), digits(23..25))
    {
        let offset = h * 3600 + m * 60;
        seconds += if sign == '+' { -offset } else { offset };
    }
    seconds
}

/// The address of pull request `number` on github.com.
pub fn pull_url(repo: &GitHubRepo, number: u64) -> String {
    format!("{WEB}/{}/pull/{number}", repo.full_name())
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
    fn times_are_read() {
        assert_eq!(parse_time("1970-01-01T00:00:00Z"), 0);
        assert_eq!(parse_time("2026-10-08T15:22:22Z"), 1_791_472_942);
        assert_eq!(parse_time("2026-10-08T17:22:22+02:00"), 1_791_472_942);
        assert_eq!(parse_time(""), 0);
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

    #[test]
    fn a_hunk_keeps_its_last_lines_numbered() {
        let hunk = "@@ -6,4 +6,5 @@ pub struct RefreshToken {\n pub struct RefreshToken {\n-    pub hash: String,\n+    pub hash: [u8; 32],\n+    pub family: Uuid,";
        let tail = hunk_tail(hunk, 3, None);
        let shown: Vec<(Option<u32>, &str, bool)> = tail.iter().map(|l| (l.number, l.text.as_str(), l.mark)).collect();
        assert_eq!(
            shown,
            [
                (None, "    pub hash: String,", false),
                (Some(7), "    pub hash: [u8; 32],", false),
                (Some(8), "    pub family: Uuid,", true)
            ]
        );
    }

    #[test]
    fn a_hunk_of_a_comment_on_lines_keeps_them_all_marked() {
        let hunk = "@@ -1,2 +1,6 @@\n a\n+b\n+c\n+d\n-x\n+e\n f";
        let tail = hunk_tail(hunk, 4, Some(2));
        let shown: Vec<(Option<u32>, bool)> = tail.iter().map(|l| (l.number, l.mark)).collect();
        assert_eq!(
            shown,
            [
                (Some(2), true),
                (Some(3), true),
                (Some(4), true),
                (None, false),
                (Some(5), true),
                (Some(6), true)
            ]
        );
    }

    #[test]
    fn calls_show_as_curl() {
        let repo = GitHubRepo {
            owner: "acme".into(),
            name: "api".into(),
            remote: "origin".into(),
        };
        let client = Client::default();
        let merge = Call::Merge {
            number: 418,
            method: Method::Squash,
            title: Some("Add models (#418)".into()),
            message: None,
            sha: "2d8b6e1".into(),
            delete_branch: true,
        };
        assert_eq!(
            client.call_request(&repo, &merge, None).display(),
            r#"curl -X PUT -H "Authorization: Bearer $GITHUB_TOKEN" https://api.github.com/repos/acme/api/pulls/418/merge -d '{"commit_title":"Add models (#418)","merge_method":"squash","sha":"2d8b6e1"}'"#
        );
        let reviewers = Call::RequestReviewers {
            number: 0,
            reviewers: vec!["maria".into(), "acme/core".into()],
            ids: vec![],
        };
        assert_eq!(
            client.call_request(&repo, &reviewers, None).display(),
            r#"curl -X POST -H "Authorization: Bearer $GITHUB_TOKEN" https://api.github.com/repos/acme/api/pulls/$NUMBER/requested_reviewers -d '{"reviewers":["maria"],"team_reviewers":["core"]}'"#
        );
        let update = Call::UpdateBranch {
            number: 419,
            sha: "9c1f2e7".into(),
        };
        assert_eq!(
            client.call_request(&repo, &update, None).display(),
            r#"curl -X PUT -H "Authorization: Bearer $GITHUB_TOKEN" https://api.github.com/repos/acme/api/pulls/419/update-branch -d '{"expected_head_sha":"9c1f2e7"}'"#
        );
        assert!(
            client
                .call_request(&repo, &reviewers, Some(422))
                .url
                .ends_with("/pulls/422/requested_reviewers")
        );
    }
}
