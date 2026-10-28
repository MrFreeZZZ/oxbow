//! Repository trust: the settings of a repository that make git run programs of its own, found
//! before Oxbow runs anything there, and Restricted, where git runs with them off.
//!
//! A folder from an archive, a shared disk or another person can carry them in `.git/config`
//! (and the files it includes) and in `.git/hooks`: `core.fsmonitor` runs on every `git status`,
//! a filter on every file it marks, a hook after a checkout. `git clone` brings neither along.
//!
//! Restricted gives every git command run in the repository's folders settings that come after
//! the repository's own (`GIT_CONFIG_COUNT`), so they win: the user's own value of each, where
//! there is one, otherwise one that runs nothing of the repository's.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, RwLock};

use serde::Serialize;

use crate::cli::GitCommand;
use crate::error::Result;

/// What a setting found makes git run, and so what Restricted turns off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrustKind {
    /// `core.fsmonitor` naming a program: runs whenever git reads the changes.
    Fsmonitor,
    /// A hook, or `core.hooksPath` pointing at the repository's own.
    Hook,
    /// `core.sshCommand`: replaces ssh for fetch and push.
    Ssh,
    /// `credential.helper`: given every password git asks for.
    Credential,
    /// `filter.*.clean`, `smudge` or `process`: runs on each file the filter marks.
    Filter,
    /// `diff.*.textconv`: runs on each file shown with that diff driver.
    Textconv,
    /// `merge.*.driver`: runs on each file merged with that driver.
    MergeDriver,
    /// `mergetool.*.cmd` or `path`: the merge tool Open in Merge Tool starts.
    MergeTool,
    /// `gpg.program` and the like: runs on each signed commit and tag.
    Signing,
    /// `remote.*.uploadpack` or `receivepack`: runs for a remote in a local folder.
    Remote,
    /// Any other command a setting gives: `core.askPass`, `core.gitProxy`,
    /// `core.alternateRefsCommand`, `sequence.editor`, `submodule.*.update`, an `ext::`
    /// transport, an LFS transfer agent.
    Command,
}

/// A setting of the repository that makes git run a program.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustFinding {
    pub kind: TrustKind,
    /// The setting as git names it (`core.sshCommand`), or `hooks/post-checkout` for a hook.
    pub key: String,
    /// The command or path it gives.
    pub value: String,
    /// The file it is in: `.git/config`, a file that includes, or the hooks folder.
    pub origin: String,
}

/// The hooks git runs.
const HOOKS: &[&str] = &[
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-commit",
    "pre-merge-commit",
    "prepare-commit-msg",
    "commit-msg",
    "post-commit",
    "pre-rebase",
    "post-checkout",
    "post-merge",
    "pre-push",
    "pre-receive",
    "update",
    "proc-receive",
    "post-receive",
    "post-update",
    "reference-transaction",
    "push-to-checkout",
    "pre-auto-gc",
    "post-rewrite",
    "sendemail-validate",
    "fsmonitor-watchman",
    "p4-changelist",
    "p4-prepare-changelist",
    "p4-post-changelist",
    "p4-pre-submit",
    "post-index-change",
];

/// One setting as `git config --list` gives it: the section and variable lowercased, the
/// subsection as written.
struct Entry {
    key: String,
    value: String,
    origin: String,
}

/// The settings of the repository at `dir` that make git run a program, and its hooks. Reading
/// them runs nothing of the repository's: `git config` reads no index and runs no hook.
pub fn scan(dir: &Path) -> Result<Vec<TrustFinding>> {
    let own = entries(dir, &["local", "worktree"])?;
    let mut found: Vec<TrustFinding> = own
        .iter()
        .filter_map(|entry| {
            Some(TrustFinding {
                kind: kind_of(&entry.key, &entry.value)?,
                key: shown_key(&entry.key),
                value: entry.value.clone(),
                origin: entry.origin.clone(),
            })
        })
        .collect();
    // Hooks run from the repository's own folder unless the user's settings send git elsewhere.
    let set_here = own.iter().rev().find(|e| e.key == "core.hookspath");
    let set_by_user = set_here.is_none() && user_entries().iter().any(|e| e.key == "core.hookspath");
    if !set_by_user && let Some(folder) = hooks_folder(dir, set_here.map(|e| e.value.as_str())) {
        let origin = shown_folder(dir, &folder);
        for name in HOOKS {
            if runnable(&folder.join(name)) {
                found.push(TrustFinding {
                    kind: TrustKind::Hook,
                    key: format!("hooks/{name}"),
                    value: format!("{origin}/{name}"),
                    origin: origin.clone(),
                });
            }
        }
    }
    Ok(found)
}

/// What a setting makes git run, if anything.
fn kind_of(key: &str, value: &str) -> Option<TrustKind> {
    let (section, rest) = key.split_once('.')?;
    let (sub, var) = match rest.rsplit_once('.') {
        Some((sub, var)) => (Some(sub), var),
        None => (None, rest),
    };
    let set = !value.trim().is_empty();
    Some(match (section, sub, var) {
        ("core", None, "fsmonitor") if set && !is_bool(value) => TrustKind::Fsmonitor,
        ("core", None, "hookspath") => TrustKind::Hook,
        ("core", None, "sshcommand") if set => TrustKind::Ssh,
        ("credential", _, "helper") if set => TrustKind::Credential,
        ("filter", Some(_), "clean" | "smudge" | "process") if set => TrustKind::Filter,
        ("diff", Some(_), "textconv") if set => TrustKind::Textconv,
        ("merge", Some(_), "driver") if set => TrustKind::MergeDriver,
        ("mergetool", Some(_), "cmd" | "path") if set => TrustKind::MergeTool,
        ("gpg", _, "program") if set => TrustKind::Signing,
        ("gpg", Some("ssh"), "defaultkeycommand") if set => TrustKind::Signing,
        ("remote", Some(_), "uploadpack" | "receivepack") if set => TrustKind::Remote,
        ("core", None, "askpass" | "gitproxy" | "alternaterefscommand") if set => TrustKind::Command,
        ("sequence", None, "editor") if set => TrustKind::Command,
        ("submodule", Some(_), "update") if value.trim_start().starts_with('!') => TrustKind::Command,
        ("protocol", None, "allow") if value.trim().eq_ignore_ascii_case("always") => TrustKind::Command,
        ("protocol", Some("ext"), "allow") if !value.trim().eq_ignore_ascii_case("never") => TrustKind::Command,
        ("lfs", Some(sub), "path") if sub.starts_with("customtransfer.") && set => TrustKind::Command,
        _ => return None,
    })
}

fn is_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "1" | "0"
    )
}

/// `core.sshcommand` as the documentation writes it: `core.sshCommand`.
fn shown_key(key: &str) -> String {
    const NAMES: &[&str] = &[
        "fsmonitor",
        "hooksPath",
        "sshCommand",
        "helper",
        "clean",
        "smudge",
        "process",
        "textconv",
        "driver",
        "cmd",
        "path",
        "program",
        "defaultKeyCommand",
        "uploadpack",
        "receivepack",
        "askPass",
        "gitProxy",
        "alternateRefsCommand",
        "editor",
        "update",
        "allow",
    ];
    let Some((head, var)) = key.rsplit_once('.') else {
        return key.to_owned();
    };
    let var = NAMES
        .iter()
        .find(|name| name.eq_ignore_ascii_case(var))
        .copied()
        .unwrap_or(var);
    format!("{head}.{var}")
}

/// The settings `git config --list` gives in `dir`, of these scopes only.
fn entries(dir: &Path, scopes: &[&str]) -> Result<Vec<Entry>> {
    let out = crate::cli::run_in(
        dir,
        &GitCommand::new(["config", "--list", "--show-scope", "--show-origin", "-z"]),
        None,
    )?;
    Ok(parse_list(&out.stdout)
        .into_iter()
        .filter(|(scope, _)| scopes.contains(&scope.as_str()))
        .map(|(_, entry)| entry)
        .collect())
}

/// `scope NUL origin NUL key LF value NUL`, the value and its LF left out for a key set with
/// no `=`.
fn parse_list(text: &str) -> Vec<(String, Entry)> {
    let mut fields = text.split('\0');
    let mut out = Vec::new();
    while let (Some(scope), Some(origin), Some(pair)) = (fields.next(), fields.next(), fields.next()) {
        let (key, value) = pair.split_once('\n').unwrap_or((pair, ""));
        let origin = origin.strip_prefix("file:").unwrap_or(origin);
        out.push((
            scope.to_owned(),
            Entry {
                key: key.to_owned(),
                value: value.to_owned(),
                origin: origin.to_owned(),
            },
        ));
    }
    out
}

/// The user's own settings: the system's and their global ones, read outside any repository.
fn user_entries() -> Vec<Entry> {
    entries(&std::env::temp_dir(), &["system", "global"]).unwrap_or_default()
}

/// Where git looks for the hooks of the repository at `dir`: the `core.hooksPath` it sets, or
/// the hooks folder of its git folder. Not asked of git, which may run restricted there already
/// and name Oxbow's empty folder.
fn hooks_folder(dir: &Path, set: Option<&str>) -> Option<PathBuf> {
    let path = match set {
        Some(path) => match path.strip_prefix("~/") {
            Some(rest) => crate::config::home_dir()?.join(rest),
            None => PathBuf::from(path),
        },
        None => {
            let out = crate::cli::run_in(dir, &GitCommand::new(["rev-parse", "--git-common-dir"]), None).ok()?;
            PathBuf::from(out.stdout.trim_end_matches(['\n', '\r'])).join("hooks")
        }
    };
    Some(if path.is_absolute() { path } else { dir.join(path) })
}

/// `folder` as the sheet shows it: inside the working tree from its top, otherwise whole.
fn shown_folder(dir: &Path, folder: &Path) -> String {
    let shown = folder.strip_prefix(dir).unwrap_or(folder);
    shown.to_string_lossy().replace('\\', "/")
}

/// A file git would run as a hook: executable on Unix, any file on Windows.
fn runnable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    true
}

/// How git runs in the folders of a restricted repository.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Restriction {
    /// Settings given after the repository's own, in order.
    config: Vec<(String, String)>,
    /// The merge tool comes from the repository's settings, and the user has none of the same
    /// name to use instead.
    no_merge_tool: bool,
}

/// The restricted repositories: each folder git may run in, with how.
static RESTRICTED: RwLock<Vec<(PathBuf, Arc<Restriction>)>> = RwLock::new(Vec::new());

/// Run git in the repository at `dir`, and in its linked worktrees, with `found` turned off.
pub fn restrict(dir: &Path, found: &[TrustFinding]) -> Result<()> {
    let restriction = Arc::new(restriction(found, &user_entries())?);
    let mut folders = vec![dir.to_path_buf()];
    folders.extend(linked_worktrees(dir));
    let mut list = RESTRICTED.write().expect("restricted lock");
    for folder in folders {
        for path in [folder.clone(), real(&folder)] {
            list.retain(|(known, _)| known != &path);
            list.push((path, restriction.clone()));
        }
    }
    Ok(())
}

/// Run git in the repository at `dir`, and in its linked worktrees, as it is set up again.
pub fn lift(dir: &Path) {
    let mut folders = vec![dir.to_path_buf()];
    folders.extend(linked_worktrees(dir));
    let folders: Vec<PathBuf> = folders.iter().flat_map(|f| [f.clone(), real(f)]).collect();
    RESTRICTED
        .write()
        .expect("restricted lock")
        .retain(|(known, _)| !folders.contains(known));
}

/// Whether git runs restricted in `dir`.
pub fn is_restricted(dir: &Path) -> bool {
    restriction_for(dir).is_some()
}

/// The restriction of the repository `dir` is in: the deepest folder registered that holds it.
fn restriction_for(dir: &Path) -> Option<Arc<Restriction>> {
    let list = RESTRICTED.read().expect("restricted lock");
    if list.is_empty() {
        return None;
    }
    let find = |dir: &Path| {
        list.iter()
            .filter(|(known, _)| dir.starts_with(known))
            .max_by_key(|(known, _)| known.as_os_str().len())
            .map(|(_, r)| r.clone())
    };
    find(dir).or_else(|| find(&real(dir)))
}

/// Give `git`, about to run in `dir`, the settings of its repository's restriction.
pub(crate) fn apply(git: &mut Command, dir: &Path) {
    let Some(restriction) = restriction_for(dir) else {
        return;
    };
    // Settings the user's environment already gives come first.
    let start: usize = std::env::var("GIT_CONFIG_COUNT")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    for (i, (key, value)) in restriction.config.iter().enumerate() {
        git.env(format!("GIT_CONFIG_KEY_{}", start + i), key)
            .env(format!("GIT_CONFIG_VALUE_{}", start + i), value);
    }
    git.env("GIT_CONFIG_COUNT", (start + restriction.config.len()).to_string());
    // `core.gitProxy` takes the first value it finds, the repository's; this comes before all.
    if std::env::var_os("GIT_PROXY_COMMAND").is_none() {
        git.env("GIT_PROXY_COMMAND", "");
    }
}

/// Whether Open in Merge Tool is off in `dir`: its tool is the repository's.
pub(crate) fn merge_tool_off(dir: &Path) -> bool {
    restriction_for(dir).is_some_and(|r| r.no_merge_tool)
}

/// The settings that turn `found` off, given the user's own settings `user`.
fn restriction(found: &[TrustFinding], user: &[Entry]) -> Result<Restriction> {
    let mut out = Restriction::default();
    let users = |key: &str| -> Option<String> {
        let key = key.to_ascii_lowercase();
        user.iter()
            .rev()
            .find(|e| e.key.to_ascii_lowercase() == key)
            .map(|e| e.value.clone())
    };
    let set = |config: &mut Vec<(String, String)>, key: &str, value: String| {
        config.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
        config.push((key.to_owned(), value));
    };
    // Always: these cost nothing when the repository doesn't set them.
    set(&mut out.config, "core.fsmonitor", "false".to_owned());
    let hooks = match users("core.hooksPath") {
        Some(own) => own,
        None => no_hooks_folder()?.to_string_lossy().into_owned(),
    };
    set(&mut out.config, "core.hooksPath", hooks);
    set(&mut out.config, "protocol.ext.allow", "never".to_owned());

    let mut credentials = false;
    for finding in found {
        let key = finding.key.as_str();
        let lower = key.to_ascii_lowercase();
        match finding.kind {
            TrustKind::Fsmonitor | TrustKind::Hook => {}
            TrustKind::Credential => credentials = true,
            TrustKind::Ssh => set(&mut out.config, key, users(key).unwrap_or_else(|| "ssh".to_owned())),
            TrustKind::Filter => {
                set(&mut out.config, key, users(key).unwrap_or_default());
                if let Some(driver) = key.strip_prefix("filter.").and_then(|k| k.rsplit_once('.')) {
                    let required = format!("filter.{}.required", driver.0);
                    let value = users(&required).unwrap_or_else(|| "false".to_owned());
                    set(&mut out.config, &required, value);
                }
            }
            TrustKind::Textconv => set(&mut out.config, key, users(key).unwrap_or_else(|| "cat".to_owned())),
            TrustKind::MergeDriver => set(
                &mut out.config,
                key,
                users(key).unwrap_or_else(|| "git merge-file -L ours -L base -L theirs %A %O %B".to_owned()),
            ),
            TrustKind::MergeTool => match users(key) {
                Some(own) => set(&mut out.config, key, own),
                None => out.no_merge_tool = true,
            },
            TrustKind::Signing => {
                let default = if lower == "gpg.x509.program" {
                    "gpgsm"
                } else if lower == "gpg.ssh.program" {
                    "ssh-keygen"
                } else if lower == "gpg.ssh.defaultkeycommand" {
                    ""
                } else {
                    "gpg"
                };
                set(&mut out.config, key, users(key).unwrap_or_else(|| default.to_owned()));
            }
            TrustKind::Remote => {
                let default = if lower.ends_with(".uploadpack") {
                    "git-upload-pack"
                } else {
                    "git-receive-pack"
                };
                set(&mut out.config, key, default.to_owned());
            }
            TrustKind::Command => {
                let neutral = if lower.starts_with("submodule.") {
                    "checkout"
                } else if lower == "sequence.editor" {
                    "true"
                } else if lower.starts_with("protocol.") {
                    // `protocol.ext.allow=never`, always set, already wins.
                    continue;
                } else {
                    ""
                };
                set(&mut out.config, key, users(key).unwrap_or_else(|| neutral.to_owned()));
            }
        }
    }
    if credentials {
        // An empty helper clears the list so far; then the user's own, in their order.
        out.config.push(("credential.helper".to_owned(), String::new()));
        for entry in user {
            if kind_of(&entry.key, &entry.value) == Some(TrustKind::Credential) {
                out.config.push((entry.key.clone(), entry.value.clone()));
            }
        }
    }
    Ok(out)
}

/// An empty folder only this user can write to, for `core.hooksPath`: no hooks run from it.
fn no_hooks_folder() -> Result<PathBuf> {
    let made = crate::cli::private_temp_dir().and_then(|dir| {
        let folder = dir.join("no-hooks");
        crate::cli::create_private_dir(&folder).map(|()| folder)
    });
    made.map_err(|err| crate::error::Error::Git(format!("can't make a folder for no hooks: {err}")))
}

/// `path` with its links followed, so that two spellings of one folder match.
fn real(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// The folders of the repository's other worktrees.
fn linked_worktrees(dir: &Path) -> Vec<PathBuf> {
    let Ok(out) = crate::cli::run_in(dir, &GitCommand::new(["worktree", "list", "--porcelain", "-z"]), None) else {
        return Vec::new();
    };
    out.stdout
        .split('\0')
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .filter(|path| path != dir)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bool_fsmonitor_runs_nothing_of_the_repository() {
        assert_eq!(kind_of("core.fsmonitor", "true"), None);
        assert_eq!(kind_of("core.fsmonitor", "./watch.sh"), Some(TrustKind::Fsmonitor));
        assert_eq!(kind_of("submodule.lib.update", "checkout"), None);
        assert_eq!(kind_of("submodule.lib.update", "!sh x"), Some(TrustKind::Command));
        assert_eq!(
            kind_of("credential.https://example.com.helper", "store"),
            Some(TrustKind::Credential)
        );
        assert_eq!(kind_of("filter.lfs.required", "true"), None);
        assert_eq!(kind_of("protocol.ext.allow", "never"), None);
        assert_eq!(kind_of("protocol.ext.allow", "user"), Some(TrustKind::Command));
    }

    #[test]
    fn keys_are_shown_as_the_documentation_writes_them() {
        assert_eq!(shown_key("core.sshcommand"), "core.sshCommand");
        assert_eq!(shown_key("filter.Crypt.clean"), "filter.Crypt.clean");
        assert_eq!(shown_key("gpg.ssh.defaultkeycommand"), "gpg.ssh.defaultKeyCommand");
    }

    #[test]
    fn the_list_is_read_with_values_holding_newlines() {
        let text = "local\0file:.git/config\0core.fsmonitor\n./a\nb\0local\0file:.git/config\0x.flag\0";
        let parsed = parse_list(text);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].1.value, "./a\nb");
        assert_eq!(parsed[0].1.origin, ".git/config");
        assert_eq!(parsed[1].1.key, "x.flag");
        assert_eq!(parsed[1].1.value, "");
    }
}
