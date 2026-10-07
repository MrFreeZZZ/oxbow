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
    git.args(args).stdin(Stdio::null());
    if let Some(home) = home_dir() {
        git.current_dir(home);
    }
    let output = git.output().map_err(|err| Error::GitNotFound(err.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(Error::Command {
            command: GitCommand::new(args.iter().copied()).display(),
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

/// Whether the `git` Oxbow runs is at least `major.minor`; `false` when it does not run.
pub(crate) fn git_at_least(major: u32, minor: u32) -> bool {
    let Some(version) = git_info().version else {
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
    std::fs::read_to_string(workdir.join(".gitattributes"))
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
}
