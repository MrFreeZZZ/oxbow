//! Why a remote refused an SSH connection: Oxbow asks `ssh-add -l` and `ssh -T` the way a
//! person would in a terminal, so the sheet can offer the fix that fits.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::cli::GitCommand;
use crate::config::home_dir;
use crate::repo::Repo;

/// What stands between the app and the remote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SshProblem {
    /// ssh-agent has no keys, usually after a restart, and the key on disk has a passphrase.
    AgentEmpty,
    /// A key was offered, but the server does not know it.
    KeyNotOnHost,
    /// No key in `~/.ssh` and none in the agent.
    NoKey,
    /// The server is not in `~/.ssh/known_hosts` yet, or its key changed.
    UnknownHost,
    /// `ssh -T` gets in now: the failure was passing, or came from something else.
    Works,
    /// Something else, e.g. the host can't be reached.
    Other,
}

/// One command Oxbow ran to find out, with what it printed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshStep {
    pub command: String,
    pub output: String,
    /// It printed what went wrong, shown in red.
    pub bad: bool,
}

/// What Oxbow found out about a remote that refused an SSH connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshCheck {
    pub problem: SshProblem,
    pub remote: String,
    /// The remote's address, e.g. `git@github.com:acme/api.git`.
    pub url: String,
    /// Who connects, e.g. `git@github.com`.
    pub login: String,
    pub host: String,
    /// The private key in question, as `~/.ssh/id_ed25519`.
    pub key: Option<String>,
    /// The same key as a full path, for `ssh-add` and for its `.pub`.
    pub key_path: Option<String>,
    /// Commands Oxbow ran and what they said, for the sheet's terminal block.
    pub steps: Vec<SshStep>,
    /// The same repository over HTTPS, e.g. `https://github.com/acme/api.git`.
    pub https_url: Option<String>,
}

/// Where an SSH address points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
}

impl SshTarget {
    pub fn login(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }

    pub fn https_url(&self) -> String {
        format!("https://{}/{}", self.host, self.path.trim_start_matches('/'))
    }
}

/// Read `git@host:path`, `ssh://[user@]host[:port]/path` or `git+ssh://…`; `None` for any
/// other kind of address.
pub fn ssh_target(url: &str) -> Option<SshTarget> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("ssh://").or_else(|| url.strip_prefix("git+ssh://")) {
        let (authority, path) = rest.split_once('/')?;
        let (user, hostport) = match authority.rsplit_once('@') {
            Some((user, host)) => (Some(user.to_owned()), host),
            None => (None, authority),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((host, port)) if !host.is_empty() => (host, port.parse().ok()),
            _ => (hostport, None),
        };
        return Some(SshTarget {
            user,
            host: host.to_owned(),
            port,
            path: path.to_owned(),
        });
    }
    if url.contains("://") {
        return None;
    }
    // scp-like: [user@]host:path, where the host part has no slash.
    let (authority, path) = url.split_once(':')?;
    // A single letter is a Windows drive, as git reads it.
    if authority.len() < 2 || authority.contains('/') || path.is_empty() {
        return None;
    }
    let (user, host) = match authority.rsplit_once('@') {
        Some((user, host)) => (Some(user.to_owned()), host.to_owned()),
        None => (None, authority.to_owned()),
    };
    Some(SshTarget {
        user,
        host,
        port: None,
        path: path.to_owned(),
    })
}

/// Private keys `ssh` tries on its own, in its order, that exist in `~/.ssh`.
fn default_keys() -> Vec<PathBuf> {
    let Some(dir) = home_dir().map(|home| home.join(".ssh")) else {
        return Vec::new();
    };
    ["id_ed25519", "id_ecdsa", "id_ed25519_sk", "id_ecdsa_sk", "id_rsa"]
        .iter()
        .map(|name| dir.join(name))
        .filter(|path| path.is_file())
        .collect()
}

/// `~/…` for a path in the home folder, as people write it.
fn tilde(path: &Path) -> String {
    match home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

/// The key needs a passphrase: `ssh-keygen -y` can't read it with an empty one.
fn has_passphrase(key: &Path) -> bool {
    Command::new("ssh-keygen")
        .args(["-y", "-P", ""])
        .arg("-f")
        .arg(key)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| !status.success())
        .unwrap_or(false)
}

/// Run a command and return its exit code and what it printed.
fn ask(program: &str, args: &[String]) -> (Option<i32>, String) {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .env("LC_ALL", "C")
        .output()
    {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            (output.status.code(), text.trim().to_owned())
        }
        Err(err) => (None, format!("{program}: {err}")),
    }
}

/// The server let the key in: GitHub, GitLab, Bitbucket and Gitea all say so in their own words.
fn greeted(output: &str) -> bool {
    let lower = output.to_lowercase();
    [
        "successfully authenticated",
        "welcome to gitlab",
        "logged in as",
        "authenticated via",
        "you've successfully",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

impl Repo {
    /// Find out why `remote` refused an SSH connection; `None` when it is not an SSH address.
    pub fn check_ssh(&self, remote: &str) -> Option<SshCheck> {
        let url = self.config_value(&format!("remote.{remote}.url"))?;
        let target = ssh_target(&url)?;
        Some(check(remote, &url, &target))
    }
}

fn check(remote: &str, url: &str, target: &SshTarget) -> SshCheck {
    let mut steps = Vec::new();
    let (agent_code, agent_out) = ask("ssh-add", &["-l".to_owned()]);
    // 0: keys listed, 1: an empty agent, 2: no agent at all.
    let agent_has_keys = agent_code == Some(0);
    steps.push(SshStep {
        command: "ssh-add -l".into(),
        output: if agent_out.is_empty() {
            "The agent has no identities.".into()
        } else {
            agent_out.clone()
        },
        bad: false,
    });
    let mut args = vec![
        "-T".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=10".to_owned(),
    ];
    if let Some(port) = target.port {
        args.push("-p".to_owned());
        args.push(port.to_string());
    }
    args.push(target.login());
    let (_, out) = ask("ssh", &args);
    let mut shown = format!("ssh -T {}", target.login());
    if let Some(port) = target.port {
        shown = format!("ssh -T -p {port} {}", target.login());
    }
    let keys = default_keys();
    let key = keys.first().cloned();
    let locked = key.as_deref().is_some_and(has_passphrase);
    let problem = if greeted(&out) {
        SshProblem::Works
    } else if out.contains("Host key verification failed") || out.contains("REMOTE HOST IDENTIFICATION HAS CHANGED") {
        SshProblem::UnknownHost
    } else if out.contains("Permission denied") {
        if !agent_has_keys && locked {
            SshProblem::AgentEmpty
        } else if agent_has_keys || key.is_some() {
            SshProblem::KeyNotOnHost
        } else {
            SshProblem::NoKey
        }
    } else {
        SshProblem::Other
    };
    steps.push(SshStep {
        command: shown,
        output: out,
        bad: !matches!(problem, SshProblem::Works),
    });
    SshCheck {
        problem,
        remote: remote.to_owned(),
        url: url.to_owned(),
        login: target.login(),
        host: target.host.clone(),
        key: key.as_deref().map(tilde),
        key_path: key.map(|path| path.display().to_string()),
        steps,
        https_url: Some(target.https_url()),
    }
}

/// `ssh-add` of `key`, keeping its passphrase in Keychain on macOS so the next restart does not
/// empty the agent again. A passphrase is asked for in a small system dialog.
pub fn add_key_command(key: &str) -> GitCommand {
    let mut args: Vec<String> = Vec::new();
    if cfg!(target_os = "macos") {
        args.push("--apple-use-keychain".into());
    }
    args.push(key.to_owned());
    let mut command = GitCommand::new(args).program("ssh-add");
    command = command.comment(if cfg!(target_os = "macos") {
        "load the key, keep its passphrase in Keychain"
    } else {
        "load the key into ssh-agent"
    });
    interactive(command)
}

/// `command`, set up to ask in a small dialog for what it would ask in a terminal: an SSH
/// key's passphrase, an HTTPS password or token, whether to trust a new server. Only for
/// commands someone is waiting on, never for a fetch in the background.
pub fn interactive(command: GitCommand) -> GitCommand {
    let Some(askpass) = askpass() else {
        return command;
    };
    let askpass = askpass.display().to_string();
    command
        .env("GIT_ASKPASS", askpass.clone())
        .env("SSH_ASKPASS", askpass)
        .env("SSH_ASKPASS_REQUIRE", "force")
        .env("DISPLAY", std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into()))
}

/// The public half of `key`, to paste into the server's settings.
pub fn public_key(key: &str) -> crate::Result<String> {
    let path = format!("{key}.pub");
    std::fs::read_to_string(&path)
        .map(|text| text.trim().to_owned())
        .map_err(|err| crate::Error::Git(format!("can't read {path}: {err}")))
}

/// A script git and ssh run to ask something, since there is no terminal: a dialog of macOS,
/// or zenity / kdialog on Linux. `None` on Windows, where Git for Windows brings its own.
fn askpass() -> Option<PathBuf> {
    if cfg!(windows) {
        return None;
    }
    let script = r#"#!/bin/sh
# Oxbow asks here for what git or ssh would ask in a terminal: a passphrase, a password or
# token, a user name, or whether to trust a server. The question is "$1".
hidden="with hidden answer"
case "$1" in
  Username*|*"(yes/no"*) hidden="" ;;
esac
if [ -x /usr/bin/osascript ]; then
  exec /usr/bin/osascript -e 'on run argv' -e "display dialog (item 1 of argv) default answer \"\" $hidden with title \"Oxbow\" with icon caution" -e 'return text returned of result' -e 'end run' -- "$1"
fi
if command -v zenity >/dev/null 2>&1; then
  if [ -n "$hidden" ]; then exec zenity --password --title="$1"; else exec zenity --entry --title=Oxbow --text="$1"; fi
fi
if command -v kdialog >/dev/null 2>&1; then
  if [ -n "$hidden" ]; then exec kdialog --password "$1"; else exec kdialog --inputbox "$1"; fi
fi
exit 1
"#;
    let dir = std::env::temp_dir().join(format!("oxbow-{}", whoami()));
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("askpass.sh");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(script) {
        std::fs::write(&path, script).ok()?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).ok()?;
    }
    Some(path)
}

fn whoami() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_addresses_are_read_in_both_forms() {
        let scp = ssh_target("git@github.com:acme/acme-api.git").unwrap();
        assert_eq!(scp.login(), "git@github.com");
        assert_eq!(scp.https_url(), "https://github.com/acme/acme-api.git");
        let long = ssh_target("ssh://git@gitlab.example.com:2222/team/app.git").unwrap();
        assert_eq!(long.host, "gitlab.example.com");
        assert_eq!(long.port, Some(2222));
        assert_eq!(long.https_url(), "https://gitlab.example.com/team/app.git");
        assert!(ssh_target("https://github.com/acme/api.git").is_none());
        assert!(ssh_target("/Users/anna/repos/api").is_none());
        assert!(ssh_target("C:/repos/api").is_none());
    }

    #[test]
    fn greetings_of_the_big_hosts_count_as_success() {
        assert!(greeted(
            "Hi anna! You've successfully authenticated, but GitHub does not provide shell access."
        ));
        assert!(greeted("Welcome to GitLab, @anna!"));
        assert!(greeted(
            "authenticated via ssh key.\n\nYou can use git to connect to Bitbucket."
        ));
        assert!(!greeted("git@github.com: Permission denied (publickey)."));
    }
}
