//! Getting a repository to work on: cloning one, starting a new one, and a quick look at one
//! that is not open, for the Welcome window's Recent Repositories.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::cli::{GitCommand, OutputLine, run_in, run_streaming_in};
use crate::error::{Error, Result};
use crate::repo::Repo;

/// How long the check of a URL in the Clone sheet may take before it gives up.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// What to clone and where.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneOptions {
    pub url: String,
    /// The folder the repository is cloned into; it must not exist yet, or be empty.
    pub path: PathBuf,
    /// Also clone and check out the repository's submodules.
    pub submodules: bool,
    /// Only the latest commit: `--depth 1`.
    pub shallow: bool,
}

/// What a quick look at a remote found.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteProbe {
    /// `SSH`, `HTTPS`, `HTTP`, `Git` or `local`, as the sheet says it.
    pub transport: String,
    /// The branch a clone checks out, `None` for an empty repository.
    pub default_branch: Option<String>,
    pub branches: usize,
    /// Commits on the default branch, when the remote can tell without a clone: a repository
    /// on this computer or on github.com.
    pub commits: Option<u64>,
}

/// How git will reach `url`, in the words the Clone sheet uses.
pub fn transport(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("https://") {
        "HTTPS"
    } else if lower.starts_with("http://") {
        "HTTP"
    } else if lower.starts_with("ssh://") || lower.starts_with("git+ssh://") {
        "SSH"
    } else if lower.starts_with("git://") {
        "Git"
    } else if lower.starts_with("file://") || url.starts_with('/') || url.starts_with('.') || url.starts_with('~') {
        "local"
    } else if url.split_once(':').is_some_and(|(host, _)| !host.contains('/')) && !is_windows_path(url) {
        // `user@host:path`, the short form of SSH.
        "SSH"
    } else {
        "local"
    }
}

fn is_windows_path(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && (bytes[2] == b'\\' || bytes[2] == b'/')
}

/// A URL git would read as an option is refused, so a pasted `--upload-pack=…` never runs.
fn check_url(url: &str) -> Result<&str> {
    let url = url.trim();
    if url.is_empty() {
        return Err(Error::Git("Enter the address of a repository".into()));
    }
    if url.starts_with('-') {
        return Err(Error::Git(format!("{url} is not a repository address")));
    }
    Ok(url)
}

/// Where git runs when there is no repository yet: the home folder, so relative paths in its
/// config resolve as in a terminal.
fn neutral_dir() -> PathBuf {
    crate::config::home_dir().unwrap_or_else(std::env::temp_dir)
}

/// Ask the remote for its branches with `git ls-remote`, giving up after a while, and count the
/// commits of its default branch where that needs no clone; `github` asks github.com.
pub fn probe_remote(url: &str, github: &crate::github::Client) -> Result<RemoteProbe> {
    let url = check_url(url)?;
    let command = GitCommand::new(["ls-remote", "--symref", url]);
    let cancel = Arc::new(AtomicBool::new(false));
    let timer = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(PROBE_TIMEOUT);
        timer.store(true, Ordering::Relaxed);
    });
    let out = match run_streaming_in(&neutral_dir(), &command, &mut |_| {}, &cancel) {
        Err(Error::Cancelled) => {
            return Err(Error::Git(
                "No answer from the remote. Check the address and your network".into(),
            ));
        }
        other => other?,
    };
    let mut probe = RemoteProbe {
        transport: transport(url).to_owned(),
        default_branch: None,
        branches: 0,
        commits: None,
    };
    for line in out.stdout.lines() {
        if let Some(rest) = line.strip_prefix("ref: refs/heads/") {
            if let Some((branch, "HEAD")) = rest.split_once('\t') {
                probe.default_branch = Some(branch.to_owned());
            }
        } else if line
            .split_once('\t')
            .is_some_and(|(_, name)| name.starts_with("refs/heads/"))
        {
            probe.branches += 1;
        }
    }
    if let Some(branch) = &probe.default_branch {
        probe.commits = if probe.transport == "local" {
            local_commits(url, branch)
        } else {
            crate::pulls::parse_github_url(url)
                .and_then(|(owner, name)| github.commit_count(&owner, &name, branch).ok())
        };
    }
    Ok(probe)
}

/// The commits of `branch` in the repository at `url` on this computer.
fn local_commits(url: &str, branch: &str) -> Option<u64> {
    let path = url.strip_prefix("file://").unwrap_or(url);
    let path = match path.strip_prefix("~/") {
        Some(rest) => crate::config::home_dir()?.join(rest),
        None => neutral_dir().join(path),
    };
    let command = GitCommand::new([
        "rev-list".to_owned(),
        "--count".to_owned(),
        format!("refs/heads/{branch}"),
    ]);
    run_in(&path, &command, None).ok()?.stdout.trim().parse().ok()
}

/// `git clone` as it runs and as the sheet shows it.
pub fn clone_command(options: &CloneOptions) -> Result<GitCommand> {
    let url = check_url(&options.url)?;
    let mut args = vec!["clone".to_owned()];
    if options.submodules {
        args.push("--recurse-submodules".into());
    }
    if options.shallow {
        args.extend(["--depth".into(), "1".into()]);
    }
    args.push(url.to_owned());
    args.push(options.path.display().to_string());
    Ok(GitCommand::new(args))
}

/// Clone, passing git's progress lines to `on_line`. A clone that fails or is stopped leaves
/// nothing behind in a folder that did not exist before.
pub fn clone(options: &CloneOptions, on_line: &mut dyn FnMut(OutputLine), cancel: &AtomicBool) -> Result<()> {
    let target = &options.path;
    if target.file_name().is_none() {
        return Err(Error::Git(format!("{} can't hold a clone", target.display())));
    }
    let existed = target.exists();
    if existed && std::fs::read_dir(target).map_or(true, |mut entries| entries.next().is_some()) {
        return Err(Error::Git(format!(
            "{} already exists and is not empty",
            target.display()
        )));
    }
    let command = clone_command(options)?.with_progress();
    let result = run_streaming_in(&neutral_dir(), &command, on_line, cancel).map(|_| ());
    if result.is_err() && !existed && target.exists() {
        // Git cleans up after a failure, but not when it was stopped.
        let _ = std::fs::remove_dir_all(target);
    }
    result
}

/// Starter `.gitignore` files offered by New Repository, by key.
pub const GITIGNORES: &[(&str, &str, &str)] = &[
    ("rust", "Rust", include_str!("../templates/gitignore/Rust.gitignore")),
    ("node", "Node", include_str!("../templates/gitignore/Node.gitignore")),
    (
        "python",
        "Python",
        include_str!("../templates/gitignore/Python.gitignore"),
    ),
    ("swift", "Swift", include_str!("../templates/gitignore/Swift.gitignore")),
    ("macos", "macOS only", ""),
];

/// Added to every starter `.gitignore`: files the Finder leaves in folders.
const MACOS_GITIGNORE: &str = include_str!("../templates/gitignore/macOS.gitignore");

/// Licenses offered by New Repository, by key.
pub const LICENSES: &[(&str, &str, &str)] = &[
    ("mit", "MIT", include_str!("../templates/license/mit.txt")),
    (
        "apache-2.0",
        "Apache 2.0",
        include_str!("../templates/license/apache-2.0.txt"),
    ),
    ("gpl-3.0", "GPL 3.0", include_str!("../templates/license/gpl-3.0.txt")),
];

/// What New Repository makes.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewRepoOptions {
    /// The folder, new or existing.
    pub path: PathBuf,
    /// Name of the first branch.
    pub branch: String,
    pub readme: bool,
    /// A key of [`GITIGNORES`].
    pub gitignore: Option<String>,
    /// A key of [`LICENSES`].
    pub license: Option<String>,
    /// Commit everything in the folder as "Initial commit".
    pub commit: bool,
}

/// What New Repository will do, for the sheet to show before it runs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NewRepoPlan {
    pub path: String,
    /// The folder is there already.
    pub exists: bool,
    /// Files and folders in it, not counting the Finder's `.DS_Store`.
    pub entries: usize,
    /// It is already a repository (or inside one); open it instead.
    pub repository: Option<String>,
    /// Files Oxbow writes before the first commit, by name.
    pub writes: Vec<String>,
    /// Starter files left out because the folder has its own.
    pub kept: Vec<String>,
    /// `git init`, then `git add` and `git commit` when asked for. `init` runs where the
    /// folder's parent is when the folder is new, the rest inside it.
    pub commands: Vec<GitCommand>,
}

fn template<'a>(table: &'a [(&str, &str, &'a str)], key: &str) -> Result<(&'a str, &'a str)> {
    table
        .iter()
        .find(|(k, _, _)| *k == key)
        .map(|(_, name, text)| (*name, *text))
        .ok_or_else(|| Error::Git(format!("no template {key}")))
}

fn check_branch(branch: &str) -> Result<()> {
    let bad = branch.is_empty()
        || branch.starts_with(['-', '/', '.'])
        || branch.ends_with(['/', '.'])
        || branch.ends_with(".lock")
        || branch.contains("..")
        || branch.contains("@{")
        || branch.contains("//")
        || branch.chars().any(|c| c.is_control() || " ~^:?*[\\".contains(c));
    if bad {
        Err(Error::Git(format!("{branch} can't be a branch name")))
    } else {
        Ok(())
    }
}

/// Look at the folder and say what New Repository would do there.
pub fn plan_new_repo(options: &NewRepoOptions) -> Result<NewRepoPlan> {
    check_branch(&options.branch)?;
    let path = &options.path;
    if path.file_name().is_none() {
        return Err(Error::Git(format!("{} can't hold a repository", path.display())));
    }
    let exists = path.is_dir();
    if path.exists() && !exists {
        return Err(Error::Git(format!("{} is a file", path.display())));
    }
    let entries = if exists {
        std::fs::read_dir(path)
            .map_err(|err| Error::Git(err.to_string()))?
            .flatten()
            .filter(|entry| entry.file_name() != ".DS_Store")
            .count()
    } else {
        0
    };
    let repository = exists
        .then(|| Repo::open(path).ok())
        .flatten()
        .map(|repo| repo.workdir().display().to_string());
    let mut writes = Vec::new();
    let mut kept = Vec::new();
    let mut want = |name: &str| {
        if path.join(name).exists() {
            kept.push(name.to_owned());
        } else {
            writes.push(name.to_owned());
        }
    };
    // A README goes into a new or empty folder only; one with files is a project already.
    if options.readme && entries == 0 {
        want("README.md");
    }
    if let Some(key) = &options.gitignore {
        template(GITIGNORES, key)?;
        want(".gitignore");
    }
    if let Some(key) = &options.license {
        template(LICENSES, key)?;
        want("LICENSE");
    }
    let mut commands = Vec::new();
    let init = if exists {
        GitCommand::new(["init", "-b", &options.branch])
    } else {
        GitCommand::new(["init", "-b", &options.branch, &path.display().to_string()])
    };
    commands.push(init);
    if options.commit {
        commands.push(GitCommand::new(["add", "--all"]));
        commands.push(GitCommand::new(["commit", "-m", "Initial commit"]));
    }
    Ok(NewRepoPlan {
        path: path.display().to_string(),
        exists,
        entries,
        repository,
        writes,
        kept,
        commands,
    })
}

/// Make the repository: `git init`, the starter files, and the first commit when asked for.
pub fn create_repo(options: &NewRepoOptions) -> Result<()> {
    let plan = plan_new_repo(options)?;
    if let Some(repo) = plan.repository {
        return Err(Error::Git(format!("{repo} is a repository already")));
    }
    let path = &options.path;
    let mut commands = plan.commands.into_iter();
    let init = commands.next().expect("the plan starts with git init");
    if plan.exists {
        run_in(path, &init, None)?;
    } else {
        // `git init <path>` makes the folder and any missing ones above it.
        run_in(&neutral_dir(), &init, None)?;
    }
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    for file in &plan.writes {
        let text = match file.as_str() {
            "README.md" => format!("# {name}\n"),
            ".gitignore" => gitignore_text(options.gitignore.as_deref().unwrap_or("macos"))?,
            "LICENSE" => license_text(options.license.as_deref().unwrap_or_default())?,
            _ => continue,
        };
        std::fs::write(path.join(file), text).map_err(|err| Error::Git(format!("could not write {file}: {err}")))?;
    }
    for command in commands {
        run_in(path, &command, None)?;
    }
    Ok(())
}

/// The starter `.gitignore`: the chosen template, then the Finder's files.
fn gitignore_text(key: &str) -> Result<String> {
    let (name, text) = template(GITIGNORES, key)?;
    if text.is_empty() {
        return Ok(MACOS_GITIGNORE.to_owned());
    }
    Ok(format!("# {name}\n{}\n\n# macOS\n{MACOS_GITIGNORE}", text.trim_end()))
}

/// The license with this year and the user's name filled in.
fn license_text(key: &str) -> Result<String> {
    let (_, text) = template(LICENSES, key)?;
    let owner = crate::config::global_config()
        .get("user.name")
        .cloned()
        .unwrap_or_else(|| "[fullname]".to_owned());
    Ok(text
        .replace("[year]", &current_year().to_string())
        .replace("[fullname]", &owner))
}

fn current_year() -> i64 {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    year_of_day(seconds.div_euclid(86_400))
}

/// The year of a day counted from 1970-01-01 (Howard Hinnant's civil-from-days).
fn year_of_day(days: i64) -> i64 {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(month <= 2)
}

/// A repository at a glance, for a row of Recent Repositories.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepoGlance {
    /// Checked-out branch, `None` when `HEAD` is detached.
    pub branch: Option<String>,
    /// Lane color of that branch, as in the graph.
    pub color: u8,
    /// Ahead and behind its upstream, when it has one.
    pub ahead: u32,
    pub behind: u32,
    pub upstream: bool,
    /// The remote the upstream is on.
    pub remote: Option<String>,
    /// Files with uncommitted changes.
    pub changed: usize,
    pub stashes: usize,
    /// A merge, rebase and the like that stopped half way.
    pub operation: Option<crate::operation::OperationKind>,
    pub conflicts: usize,
}

/// Open the repository at `path` just long enough to see where it stands.
pub fn glance(path: &Path) -> Result<RepoGlance> {
    let repo = Repo::open(path)?;
    let operation = repo.operation().ok().flatten();
    // A rebase detaches HEAD; the branch it rebases is the one to show.
    let branch = repo
        .head()?
        .branch
        .or_else(|| operation.as_ref().and_then(|op| op.branch.clone()));
    let color = match branch.as_deref() {
        None => crate::graph::NO_BRANCH_COLOR,
        Some("main" | "master") => crate::graph::TRUNK_COLOR,
        Some(name) => crate::graph::color_for_name(name),
    };
    let tracking = repo.tracking().ok().flatten();
    let tree = repo.working_tree()?;
    Ok(RepoGlance {
        branch,
        color,
        ahead: tracking.as_ref().map_or(0, |t| t.ahead),
        behind: tracking.as_ref().map_or(0, |t| t.behind),
        upstream: tracking.is_some(),
        remote: tracking.as_ref().map(|t| t.remote.clone()),
        changed: tree.file_count(),
        stashes: repo.stashes().map_or(0, |s| s.len()),
        conflicts: operation.as_ref().map_or(tree.conflicted.len(), |op| op.conflicted),
        operation: operation.map(|op| op.kind),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transports_are_named_like_the_sheet_says() {
        assert_eq!(transport("git@github.com:acme/api.git"), "SSH");
        assert_eq!(transport("ssh://git@host:22/acme/api.git"), "SSH");
        assert_eq!(transport("https://github.com/acme/api.git"), "HTTPS");
        assert_eq!(transport("/Users/anna/api"), "local");
        assert_eq!(transport("C:/work/api"), "local");
        assert_eq!(transport("file:///srv/api.git"), "local");
    }

    #[test]
    fn days_become_years() {
        assert_eq!(year_of_day(0), 1970);
        assert_eq!(year_of_day(20_453), 2025); // 2025-12-31
        assert_eq!(year_of_day(20_454), 2026); // 2026-01-01
    }

    #[test]
    fn urls_that_look_like_options_are_refused() {
        assert!(check_url("--upload-pack=touch /tmp/x").is_err());
        assert!(check_url("  ").is_err());
    }
}
