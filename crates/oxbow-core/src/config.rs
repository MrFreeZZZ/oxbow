//! Settings that belong to Git itself and how Oxbow runs it.
//!
//! Identity, pull mode, default branch and the like stay in `~/.gitconfig` or the repository's
//! `.git/config`, read and changed with `git config`, so Oxbow and a terminal always agree.
//! Which `git` to run, whether its messages are in English and whether hooks run are Oxbow's
//! own settings, set once for the whole process.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::RwLock;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::cli::GitCommand;
use crate::error::{Error, Result};
use crate::repo::Repo;

static PROGRAM: RwLock<Option<PathBuf>> = RwLock::new(None);
static ENGLISH: AtomicBool = AtomicBool::new(true);
static HOOKS: AtomicBool = AtomicBool::new(true);
static GITHUB_TOKEN: RwLock<Option<TokenSource>> = RwLock::new(None);
static GITLAB_TOKENS: RwLock<Option<GitLabTokens>> = RwLock::new(None);

/// Where the GitHub sign-in's token comes from, asked only when a command talks to a remote.
pub type TokenSource = Box<dyn Fn() -> Option<String> + Send + Sync>;

/// Lend git the GitHub sign-in for `https://github.com` remotes, through a credential helper
/// that answers with the token. Git asks the user's own helpers first, so a password already in
/// the keychain still wins; ours only fills the gap, e.g. right after Publish. `None` stops.
pub fn set_github_token(source: Option<TokenSource>) {
    *GITHUB_TOKEN.write().expect("github token lock") = source;
}

/// The helper: on `get` it prints the token from the environment, so it never shows in the
/// list of running processes; on `store` and `erase` it does nothing.
const GITHUB_HELPER: &str = "credential.https://github.com.helper=!f() { if [ \"$1\" = get ]; then echo username=x-access-token; echo \"password=$OXBOW_GITHUB_TOKEN\"; fi; }; f";

/// The GitLab sign-ins' tokens: given a server's address as a remote's URL starts
/// (`https://gitlab.example.com`), the token of the account signed in there.
pub type GitLabTokens = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// Lend git the GitLab sign-ins for HTTPS remotes on their servers, as [`set_github_token`]
/// does for GitHub: the user's own helpers are asked first. `None` stops.
pub fn set_gitlab_tokens(source: Option<GitLabTokens>) {
    *GITLAB_TOKENS.write().expect("gitlab token lock") = source;
}

/// GitLab's helper, for `credential.<server>.helper`: a token is the password of `oauth2`.
const GITLAB_HELPER: &str =
    "!f() { if [ \"$1\" = get ]; then echo username=oauth2; echo \"password=$OXBOW_GITLAB_TOKEN\"; fi; }; f";

/// Add the GitHub helper to a command that talks to a remote, when signed in and the remote can
/// be github.com over HTTPS, and GitLab's for a remote on a GitLab server signed in to. Commands
/// for other servers don't get a token: their hooks, SSH commands and helpers have no business
/// seeing it.
pub(crate) fn lend_tokens(git: &mut Command, dir: &Path, args: &[String]) {
    let github = GITHUB_TOKEN.read().expect("github token lock");
    let gitlab = GITLAB_TOKENS.read().expect("gitlab token lock");
    if github.is_none() && gitlab.is_none() {
        return;
    }
    let urls = reached_urls(dir, args);
    if let Some(source) = github.as_ref()
        && urls.iter().any(|url| is_github_https(url))
        && let Some(token) = source()
    {
        git.args(["-c", GITHUB_HELPER]).env("OXBOW_GITHUB_TOKEN", token);
    }
    if let Some(source) = gitlab.as_ref() {
        let lent = urls
            .iter()
            .filter_map(|url| web_base(url))
            .filter(|base| !is_github_https(base))
            .find_map(|base| source(&base).map(|token| (base, token)));
        if let Some((base, token)) = lent {
            git.args(["-c", &format!("credential.{base}.helper={GITLAB_HELPER}")])
                .env("OXBOW_GITLAB_TOKEN", token);
        }
    }
}

/// The URLs a command with these arguments (those after its subcommand) may reach: the URL or
/// remote it names; naming neither (`git pull`, `git submodule update`), every remote of the
/// repository and of its submodules.
fn reached_urls(dir: &Path, args: &[String]) -> Vec<String> {
    let named: Vec<&str> = args
        .iter()
        .map(String::as_str)
        .filter(|a| !a.starts_with('-'))
        .collect();
    if let Some(url) = named.iter().find(|a| a.contains("://")) {
        return vec![(*url).to_owned()];
    }
    let remotes = remote_urls(dir, None);
    let chosen: Vec<String> = remotes
        .iter()
        .filter(|(name, _)| named.contains(&name.as_str()))
        .map(|(_, url)| url.clone())
        .collect();
    if !chosen.is_empty() {
        return chosen;
    }
    remotes
        .into_iter()
        .chain(remote_urls(dir, Some(".gitmodules")))
        .map(|(_, url)| url)
        .collect()
}

#[cfg(test)]
fn reaches_github(dir: &Path, args: &[String]) -> bool {
    reached_urls(dir, args).iter().any(|url| is_github_https(url))
}

/// `https://host` (or `http://`) of an HTTP remote URL, without a user name or path: where
/// git asks credentials for.
fn web_base(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && scheme != "http" {
        return None;
    }
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    (!host.is_empty()).then(|| format!("{scheme}://{}", host.to_ascii_lowercase()))
}

/// `https://github.com/…`, also with a user name before the host.
fn is_github_https(url: &str) -> bool {
    let Some(rest) = url
        .get(..8)
        .filter(|s| s.eq_ignore_ascii_case("https://"))
        .map(|_| &url[8..])
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host.rsplit_once('@').map_or(host, |(_, host)| host);
    host.eq_ignore_ascii_case("github.com") || host.eq_ignore_ascii_case("github.com:443")
}

/// The URLs of the remotes set up in `dir`, by remote name, or of the submodules in `file`.
fn remote_urls(dir: &Path, file: Option<&str>) -> Vec<(String, String)> {
    let mut config = git();
    config.arg("config");
    if let Some(file) = file {
        config.args(["--file", file]);
    }
    let pattern = if file.is_some() {
        r"^submodule\..*\.url$"
    } else {
        r"^remote\..*\.(url|pushurl)$"
    };
    let Ok(out) = config
        .args(["-z", "--get-regexp", pattern])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    // `-z`: each entry is the key, a newline, the value and NUL.
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter_map(|entry| {
            let (key, url) = entry.split_once('\n')?;
            let (_, rest) = key.split_once('.')?;
            let (name, _) = rest.rsplit_once('.')?;
            Some((name.to_owned(), url.to_owned()))
        })
        .collect()
}

/// Run this `git` instead of the first one on `PATH`; `None` goes back to that one.
pub fn set_git_program(path: Option<PathBuf>) {
    *PROGRAM.write().expect("git program lock") = path;
}

/// Ask git for English messages, as in most guides and search results, rather than the
/// system's language.
pub fn set_english_output(on: bool) {
    ENGLISH.store(on, Ordering::Relaxed);
}

/// Let git run the repository's hooks (pre-commit, commit-msg, pre-push). Off adds
/// `--no-verify` to commits, merges and pushes.
pub fn set_run_hooks(on: bool) {
    HOOKS.store(on, Ordering::Relaxed);
}

pub(crate) fn run_hooks() -> bool {
    HOOKS.load(Ordering::Relaxed)
}

/// `git`, set up the way Oxbow runs it.
pub(crate) fn git() -> Command {
    let program = PROGRAM.read().expect("git program lock").clone();
    let mut git = Command::new(program.unwrap_or_else(|| PathBuf::from("git")));
    git
        // Paths with non-ASCII names come back as they are, not as octal escapes.
        .args(["-c", "core.quotePath=false"])
        // There is no terminal to type a password into: fail instead of hanging.
        .env("GIT_TERMINAL_PROMPT", "0")
        // Nor an editor: `rebase --continue` and the like keep the message git prepared.
        .env("GIT_EDITOR", "true");
    if ENGLISH.load(Ordering::Relaxed) {
        git.env("LC_MESSAGES", "C");
    }
    git
}

/// Run git outside any repository and return what it printed, or the error it gave.
fn run_global(args: &[&str]) -> Result<String> {
    let mut git = git();
    git.args(args);
    let home = home_dir();
    if let Some(home) = &home {
        git.current_dir(home);
    }
    let shown = GitCommand::new(args.iter().copied());
    let output = crate::cli::output_of(git, home.as_deref().unwrap_or(Path::new("")), &shown)?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Error::Command {
            command: shown.display(),
            code: output.status.code(),
            output: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// The `git` Oxbow runs.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GitInfo {
    /// Where it is, or just `git` when it is not on `PATH`.
    pub path: String,
    /// `2.51.0`, `None` when it does not run.
    pub version: Option<String>,
    /// Chosen in Settings rather than found on `PATH`.
    pub custom: bool,
}

pub fn git_info() -> GitInfo {
    let custom = PROGRAM.read().expect("git program lock").clone();
    let path = custom.clone().or_else(|| find_on_path("git"));
    let version = run_global(&["--version"])
        .ok()
        .map(|out| out.trim().trim_start_matches("git version ").to_owned());
    GitInfo {
        path: path.map_or_else(|| "git".to_owned(), |p| p.display().to_string()),
        version,
        custom: custom.is_some(),
    }
}

/// The version of the `git` Oxbow runs, asked once for each program chosen in Settings: an
/// Undo snapshot asks it every step. Not kept while git doesn't run.
fn git_version() -> Option<String> {
    static KNOWN: std::sync::Mutex<Option<(Option<PathBuf>, String)>> = std::sync::Mutex::new(None);
    let program = PROGRAM.read().expect("git program lock").clone();
    if let Ok(known) = KNOWN.lock()
        && let Some((of, version)) = &*known
        && *of == program
    {
        return Some(version.clone());
    }
    let version = git_info().version?;
    if let Ok(mut known) = KNOWN.lock() {
        *known = Some((program, version.clone()));
    }
    Some(version)
}

/// Whether the `git` Oxbow runs is at least `major.minor`; `false` when it does not run.
pub(crate) fn git_at_least(major: u32, minor: u32) -> bool {
    let Some(version) = git_version() else {
        return false;
    };
    let mut parts = version.split(['.', ' ']).map(|part| part.parse::<u32>().unwrap_or(0));
    let found = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    found >= (major, minor)
}

/// The first `name` on `PATH`, as a shell would find it.
pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths).find_map(|dir| {
        let file = dir.join(name);
        if file.is_file() {
            return Some(file);
        }
        let exe = dir.join(format!("{name}.exe"));
        exe.is_file().then_some(exe)
    })
}

/// Which config file a value is read from or written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigScope {
    /// `~/.gitconfig`: every repository.
    Global,
    /// The repository's own `.git/config`.
    Local,
}

impl ConfigScope {
    fn flag(self) -> &'static str {
        match self {
            ConfigScope::Global => "--global",
            ConfigScope::Local => "--local",
        }
    }
}

/// The `git config` command that sets `key` (or unsets it, for `None`), as Settings shows it.
pub fn config_command(scope: ConfigScope, key: &str, value: Option<&str>) -> GitCommand {
    match value {
        Some(value) => GitCommand::new(["config", scope.flag(), key, value]),
        None => GitCommand::new(["config", scope.flag(), "--unset", key]),
    }
}

/// Every value in `~/.gitconfig`; for a key set more than once, the last.
pub fn global_config() -> BTreeMap<String, String> {
    // Exit code 1 means there is no such file yet: nothing is set.
    run_global(&["config", "--global", "--null", "--list"])
        .map(|out| parse_list(&out))
        .unwrap_or_default()
}

/// Set a value in `~/.gitconfig`, or remove it.
pub fn set_global_config(key: &str, value: Option<&str>) -> Result<()> {
    let command = config_command(ConfigScope::Global, key, value);
    let args: Vec<&str> = command.args.iter().map(String::as_str).collect();
    match run_global(&args) {
        // 5: unsetting a key that is not there, which is what was wanted.
        Err(Error::Command { code: Some(5), .. }) if value.is_none() => Ok(()),
        other => other.map(|_| ()),
    }
}

/// `remote.<name>.url` and `.pushurl` entries from `git config --null --get-regexp`, in
/// the order the config lists them. A remote without a push address pushes to its URL.
fn parse_remotes(out: &str) -> Vec<RemoteInfo> {
    let mut remotes: Vec<RemoteInfo> = Vec::new();
    for entry in out.split('\0').filter(|entry| !entry.is_empty()) {
        let (key, url) = entry.split_once('\n').unwrap_or((entry, ""));
        let Some(rest) = key.strip_prefix("remote.") else {
            continue;
        };
        let (name, push) = match rest.strip_suffix(".pushurl") {
            Some(name) => (name, true),
            None => match rest.strip_suffix(".url") {
                Some(name) => (name, false),
                None => continue,
            },
        };
        let index = match remotes.iter().position(|r| r.name == name) {
            Some(index) => index,
            None => {
                remotes.push(RemoteInfo {
                    name: name.to_owned(),
                    fetch_url: String::new(),
                    push_url: String::new(),
                });
                remotes.len() - 1
            }
        };
        let remote = &mut remotes[index];
        if push {
            remote.push_url = url.to_owned();
        } else if remote.fetch_url.is_empty() {
            remote.fetch_url = url.to_owned();
        }
    }
    for remote in &mut remotes {
        if remote.push_url.is_empty() {
            remote.push_url = remote.fetch_url.clone();
        }
    }
    remotes.retain(|remote| !remote.fetch_url.is_empty());
    remotes
}

/// `key=value\0` records of `git config --null --list`.
fn parse_list(out: &str) -> BTreeMap<String, String> {
    out.split('\0')
        .filter_map(|record| {
            let (key, value) = record.split_once('\n').unwrap_or((record, ""));
            (!key.is_empty()).then(|| (key.to_owned(), value.to_owned()))
        })
        .collect()
}

impl Repo {
    /// Every value in the repository's own `.git/config`.
    pub fn local_config(&self) -> BTreeMap<String, String> {
        self.run(&GitCommand::new(["config", "--local", "--null", "--list"]))
            .map(|out| parse_list(&out.stdout))
            .unwrap_or_default()
    }

    /// Set a value in the repository's `.git/config`, or remove it.
    pub fn set_local_config(&self, key: &str, value: Option<&str>) -> Result<()> {
        match self.run(&config_command(ConfigScope::Local, key, value)) {
            Err(Error::Command { code: Some(5), .. }) if value.is_none() => Ok(()),
            other => other.map(|_| ()),
        }
    }

    /// The value git uses for `key` here, from whichever file sets it.
    pub fn config_value(&self, key: &str) -> Option<String> {
        self.run(&GitCommand::new(["config", "--get", key]))
            .ok()
            .map(|out| out.stdout.trim().to_owned())
    }

    /// A boolean the way git reads it, `None` when it is not set.
    pub(crate) fn config_bool(&self, key: &str) -> Option<bool> {
        self.config_value(key).map(|value| git_bool(&value))
    }

    /// The remotes and where they point, as written in the config: `git remote -v` would show
    /// addresses after `url.*.insteadOf` rewrites, and editing one would save the rewrite.
    pub fn remotes_info(&self) -> Result<Vec<RemoteInfo>> {
        let out = self.run(&GitCommand::new([
            "config",
            "--null",
            "--get-regexp",
            r"^remote\..*\.(url|pushurl)$",
        ]));
        // Exit code 1: no remote has an address.
        let Ok(out) = out else { return Ok(Vec::new()) };
        Ok(parse_remotes(&out.stdout))
    }

    /// How much room the repository's objects take, and what Git LFS tracks.
    pub fn storage(&self) -> Result<Storage> {
        let out = self.run(&GitCommand::new(["count-objects", "-v"]))?;
        let mut fields = BTreeMap::new();
        for line in out.stdout.lines() {
            if let Some((key, value)) = line.split_once(": ") {
                fields.insert(key.to_owned(), value.trim().parse::<u64>().unwrap_or(0));
            }
        }
        let field = |key: &str| fields.get(key).copied().unwrap_or(0);
        Ok(Storage {
            bytes: (field("size") + field("size-pack")) * 1024,
            objects: field("count") + field("in-pack"),
            loose: field("count"),
            lfs_patterns: lfs_patterns(self.workdir()),
        })
    }
}

/// `true`, `yes`, `on` and non-zero numbers are true to git.
fn git_bool(value: &str) -> bool {
    match value.trim().to_ascii_lowercase().as_str() {
        "" | "false" | "no" | "off" => false,
        "true" | "yes" | "on" => true,
        other => other.parse::<i64>() != Ok(0),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    /// Loose objects and packs together.
    pub bytes: u64,
    pub objects: u64,
    /// Objects not in a pack yet, which `git gc` packs.
    pub loose: u64,
    /// Patterns `.gitattributes` hands to Git LFS.
    pub lfs_patterns: Vec<String>,
}

fn lfs_patterns(workdir: &Path) -> Vec<String> {
    crate::lfs::read_attributes(workdir)
        .unwrap_or_default()
        .lines()
        .filter(|line| line.split_whitespace().any(|attr| attr == "filter=lfs"))
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .collect()
}

/// A public key in `~/.ssh`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshKey {
    /// File name without `.pub`: `id_ed25519`.
    pub name: String,
    /// Path of the `.pub` file.
    pub path: String,
    /// `ssh-ed25519`, `ssh-rsa`…
    pub kind: String,
    /// `SHA256:…`, as GitHub shows it, when `ssh-keygen` could tell.
    pub fingerprint: Option<String>,
    pub comment: String,
    /// The whole public key line, to paste into GitHub.
    pub public: String,
}

pub fn ssh_keys() -> Vec<SshKey> {
    let Some(dir) = home_dir().map(|home| home.join(".ssh")) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut keys: Vec<SshKey> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "pub"))
        .filter_map(|path| {
            let public = std::fs::read_to_string(&path).ok()?.trim().to_owned();
            let mut parts = public.splitn(3, ' ');
            let kind = parts.next()?.to_owned();
            parts.next()?;
            let comment = parts.next().unwrap_or("").to_owned();
            Some(SshKey {
                name: path.file_stem()?.to_string_lossy().into_owned(),
                path: path.display().to_string(),
                fingerprint: fingerprint(&path),
                kind,
                comment,
                public,
            })
        })
        .collect();
    keys.sort_by(|a, b| a.name.cmp(&b.name));
    keys
}

/// How SSH will authenticate, in a line for the Welcome window.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SshSource {
    pub label: String,
    /// False when there is no key and no agent: only HTTPS addresses will work.
    pub ok: bool,
}

pub fn ssh_source() -> SshSource {
    let agent = std::env::var("SSH_AUTH_SOCK").unwrap_or_default();
    let config = home_dir()
        .and_then(|home| std::fs::read_to_string(home.join(".ssh").join("config")).ok())
        .unwrap_or_default();
    let one_password = agent.to_lowercase().contains("1password")
        || config.lines().any(|line| {
            let line = line.trim().to_lowercase();
            line.starts_with("identityagent") && line.contains("1password")
        });
    if one_password {
        return SshSource {
            label: "SSH via 1Password agent".into(),
            ok: true,
        };
    }
    let keys = ssh_keys();
    match (keys.first(), keys.len()) {
        (Some(key), 1) => SshSource {
            label: format!("SSH key {}", key.name),
            ok: true,
        },
        (Some(key), n) => SshSource {
            label: format!("SSH key {} and {} more", key.name, n - 1),
            ok: true,
        },
        (None, _) if !agent.is_empty() => SshSource {
            label: "SSH via ssh-agent".into(),
            ok: true,
        },
        (None, _) => SshSource {
            label: "No SSH key yet · HTTPS works".into(),
            ok: false,
        },
    }
}

fn fingerprint(path: &Path) -> Option<String> {
    let output = Command::new("ssh-keygen")
        .arg("-lf")
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let out = String::from_utf8_lossy(&output.stdout);
    out.split_whitespace().nth(1).map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_github_sign_in_goes_only_to_github_over_https() {
        assert!(is_github_https("https://github.com/a/b.git"));
        assert!(is_github_https("HTTPS://x-access-token@GitHub.com/a/b"));
        assert!(!is_github_https("https://github.com.evil.example/a/b"));
        assert!(!is_github_https("https://gitlab.com/a/b"));
        assert!(!is_github_https("git@github.com:a/b.git"));
        assert!(!is_github_https("http://github.com/a/b"));

        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(dir.path())
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        };
        git(&["init", "-q"]);
        git(&["remote", "add", "origin", "https://github.com/a/b.git"]);
        git(&["remote", "add", "work", "https://git.example.com/a/b.git"]);
        let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        assert!(reaches_github(dir.path(), &args(&["origin", "main"])));
        assert!(!reaches_github(dir.path(), &args(&["-u", "work", "main"])));
        assert!(!reaches_github(dir.path(), &args(&["https://git.example.com/c.git"])));
        // Naming no remote, any of them may be the one.
        assert!(reaches_github(dir.path(), &args(&["--prune"])));
    }

    #[test]
    fn config_list_is_parsed() {
        let list = parse_list("user.name\nAnna Petrova\0pull.rebase\ntrue\0core.bare\nfalse\0");
        assert_eq!(list.get("pull.rebase").map(String::as_str), Some("true"));
        assert_eq!(list.get("user.name").map(String::as_str), Some("Anna Petrova"));
        assert_eq!(list.len(), 3);
    }

    #[test]
    fn remotes_keep_their_written_addresses() {
        let out = "remote.origin.url\ngit@github.com:acme/api.git\0remote.fork.v2.url\nhttps://example.com/f.git\0remote.fork.v2.pushurl\ngit@example.com:f.git\0";
        assert_eq!(
            parse_remotes(out),
            vec![
                RemoteInfo {
                    name: "origin".into(),
                    fetch_url: "git@github.com:acme/api.git".into(),
                    push_url: "git@github.com:acme/api.git".into(),
                },
                RemoteInfo {
                    name: "fork.v2".into(),
                    fetch_url: "https://example.com/f.git".into(),
                    push_url: "git@example.com:f.git".into(),
                },
            ]
        );
    }

    #[test]
    fn booleans_read_as_git_reads_them() {
        assert!(git_bool("true") && git_bool("Yes") && git_bool("1") && git_bool("on"));
        assert!(!git_bool("false") && !git_bool("0") && !git_bool("off") && !git_bool(""));
    }

    #[test]
    fn the_github_helper_answers_git() {
        let mut git = Command::new("git");
        git.args(["-c", "credential.helper=", "-c", GITHUB_HELPER, "credential", "fill"])
            .env("OXBOW_GITHUB_TOKEN", "gho_secret")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = git.spawn().expect("git runs");
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"protocol=https\nhost=github.com\n\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("username=x-access-token"), "{text}");
        assert!(text.contains("password=gho_secret"), "{text}");
    }

    #[test]
    fn a_gitlab_sign_in_goes_only_to_its_server() {
        assert_eq!(
            web_base("https://anna@Git.Corp:8443/team/app.git").as_deref(),
            Some("https://git.corp:8443")
        );
        assert_eq!(web_base("git@git.corp:team/app.git"), None);
        assert_eq!(web_base("ssh://git@git.corp/team/app.git"), None);

        let dir = tempfile::tempdir().unwrap();
        let init = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(args)
                    .current_dir(dir.path())
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        };
        init(&["init", "-q"]);
        init(&[
            "remote",
            "add",
            "origin",
            "http://gitlab.acme.test:8929/acme/billing.git",
        ]);
        init(&["remote", "add", "other", "https://git.example.com/a/b.git"]);
        set_gitlab_tokens(Some(Box::new(|base: &str| {
            (base == "http://gitlab.acme.test:8929").then(|| "glpat-secret".to_owned())
        })));
        let lent = |args: &[&str]| {
            let mut git = Command::new("git");
            lend_tokens(
                &mut git,
                dir.path(),
                &args.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>(),
            );
            git
        };
        let other = lent(&["other", "main"]);
        let origin = lent(&["-u", "origin", "main"]);
        set_gitlab_tokens(None);
        assert_eq!(other.get_args().count(), 0);
        let args: Vec<String> = origin.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args.len(), 2);
        assert!(args[1].starts_with("credential.http://gitlab.acme.test:8929.helper=!f()"));

        // The helper answers git for that server.
        let mut git = Command::new("git");
        git.args(["-c", "credential.helper=", "-c", &args[1], "credential", "fill"])
            .env("OXBOW_GITLAB_TOKEN", "glpat-secret")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        let mut child = git.spawn().expect("git runs");
        use std::io::Write;
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"protocol=http\nhost=gitlab.acme.test:8929\n\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("username=oauth2"), "{text}");
        assert!(text.contains("password=glpat-secret"), "{text}");
    }
}
