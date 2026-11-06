//! GitLab account, merge request and publishing models with their validation.

use serde::{Deserialize, Serialize};

/// gitlab.com's address, the server when none is named.
pub const WEB: &str = "https://gitlab.com";

/// What Oxbow asks a token for: `api` reads and writes merge requests, projects and reviews.
pub const SCOPES: &[&str] = &["api"];

/// The oldest GitLab Oxbow signs in to: from 16.0 it says whether a merge request can be merged
/// (`detailed_merge_status`), keeps draft review comments and merges once the pipeline passes
/// (`auto_merge`). Older servers keep Create Merge Request through the push.
pub const OLDEST: (u32, u32) = (16, 0);

/// A GitLab server, from what was typed: `gitlab.example.com`, `https://gitlab.example.com/`,
/// `http://10.0.0.5:8080`. `https` unless `http://` was typed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    /// `gitlab.example.com`, with the port when it isn't the usual one.
    pub host: String,
    /// `https://gitlab.example.com`: pages are under it, the API under `/api/v4`.
    pub web: String,
}

impl Server {
    pub fn parse(typed: &str) -> std::result::Result<Server, String> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Server::parse(WEB);
        }
        let lower = typed.to_lowercase();
        let (scheme, rest) = match lower.split_once("://") {
            Some(("https", rest)) => ("https", rest),
            Some(("http", rest)) => ("http", rest),
            Some(_) => return Err("Type the server’s address, such as gitlab.example.com".into()),
            None => ("https", lower.as_str()),
        };
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
        let (name, port) = match host.split_once(':') {
            Some((name, port)) => (name, Some(port)),
            None => (host, None),
        };
        let name_ok = !name.is_empty()
            && !name.starts_with(['.', '-'])
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if !name_ok || port.is_some_and(|p| p.is_empty() || p.parse::<u16>().is_err()) {
            return Err("Type the server’s address, such as gitlab.example.com".into());
        }
        let host = match port {
            Some(p) if (scheme, p) == ("https", "443") || (scheme, p) == ("http", "80") => name.to_owned(),
            _ => host.to_owned(),
        };
        Ok(Server {
            web: format!("{scheme}://{host}"),
            host,
        })
    }

    /// Where to make a personal access token with [`SCOPES`] already ticked.
    pub fn new_token_url(&self) -> String {
        format!(
            "{}/-/user_settings/personal_access_tokens?name=Oxbow&scopes={}",
            self.web,
            SCOPES.join(",")
        )
    }
}

/// A GitLab account's public face.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct User {
    /// The account's number, which GitLab names reviewers and assignees by.
    #[serde(default)]
    pub id: u64,
    pub username: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub web_url: String,
}

/// Who a token belongs to, what it may do and the server it is for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub user: User,
    /// The token's scopes; `None` when the server doesn't say (not a personal access token).
    pub scopes: Option<Vec<String>>,
    /// The last day the token works, `2026-12-01`; `None` when it never expires or isn't known.
    pub expires_at: Option<String>,
    /// The server's version, `16.11.2-ee`.
    pub version: String,
}

impl Me {
    /// Why Oxbow can't work with this sign-in, if it can't.
    pub fn problem(&self) -> Option<String> {
        if version_number(&self.version).is_some_and(|v| v < OLDEST) {
            return Some(format!(
                "This server runs GitLab {}; Oxbow signs in to GitLab {}.{} or later. Create Merge Request still works without signing in.",
                self.version, OLDEST.0, OLDEST.1
            ));
        }
        match &self.scopes {
            Some(scopes) if !scopes.iter().any(|s| s == "api") => Some(format!(
                "This token can’t work with merge requests: it needs the api scope, and it has {}.",
                if scopes.is_empty() {
                    "none".to_owned()
                } else {
                    scopes.join(", ")
                }
            )),
            _ => None,
        }
    }
}

/// `16.11.2-ee` as (16, 11).
pub fn version_number(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.split(['.', '-']);
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

/// What only a merge request on GitLab has, for the words of the screen and for its calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabFacts {
    /// GitLab's say on merging it (`detailed_merge_status`): `mergeable`, `ci_must_pass`,
    /// `ci_still_running`, `not_approved`, `discussions_not_resolved`, `draft_status`,
    /// `need_rebase`, `conflict`, `blocked_status`, `checking`…
    pub merge_status: String,
    /// The project's merge method: `merge`, `rebase_merge` (a merge commit, only onto the newest
    /// target) or `ff` (fast-forward only).
    pub merge_method: String,
    /// Whether it may be squashed: `never`, `always`, `default_on` or `default_off`.
    pub squash_option: String,
    /// It asks to be squashed when merged.
    pub squash: bool,
    /// It asks GitLab to delete its source branch once merged.
    pub remove_source_branch: bool,
    /// Commits of the target it doesn't have.
    pub behind: Option<u32>,
    /// The pipeline of its newest commit.
    pub pipeline: Option<Pipeline>,
    /// The commits its diff is between, which comments on lines name.
    pub diff: Option<DiffRefs>,
    /// `additions` and `deletions` are counted: the commits are here.
    pub line_counts: bool,
    /// The reviewers it asks, by account id: asking one more sends them all.
    pub reviewer_ids: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pipeline {
    pub id: u64,
    /// `success`, `failed`, `running`, `pending`, `canceled`, `skipped`, `manual`…
    pub status: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffRefs {
    /// Where the source branch left the target.
    pub base_sha: String,
    /// The target's commit the diff was made against.
    pub start_sha: String,
    pub head_sha: String,
}

/// A namespace a project can go in: the account's own (`user`) or a group's (`group`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Namespace {
    pub id: u64,
    /// `anna`, `acme/platform`.
    pub full_path: String,
    pub kind: String,
}

/// What Publish to GitLab makes: a project in a namespace, then `origin` pointing at it and the
/// branch pushed there.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabPublish {
    pub namespace_id: u64,
    /// The namespace's full path, for the project's address.
    pub namespace: String,
    pub name: String,
    /// `private`, `internal` or `public`.
    pub visibility: String,
    #[serde(default)]
    pub description: String,
    pub branch: String,
}

impl GitLabPublish {
    /// Where the project will be, before it exists: its HTTP address, which the sign-in answers
    /// for.
    pub fn url(&self, server: &Server) -> String {
        format!("{}/{}/{}.git", server.web, self.namespace, self.name)
    }

    /// The git half of publishing, run in the repository after GitLab made the project.
    pub fn commands(&self, server: &Server) -> Vec<crate::cli::GitCommand> {
        crate::github::origin_and_push(&self.url(server), &self.branch, "the new GitLab project becomes origin")
    }
}

/// The project Publish to GitLab made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProject {
    pub web_url: String,
    pub visibility: String,
}

/// What a run of calls says as it goes: the request about to be sent, as curl, and once a call
/// is done, what came of it when there is something to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Request(String),
    Done(Option<String>),
}

/// Someone on GitLab, to ask for a review or assign.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabUser {
    pub id: u64,
    pub username: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Label {
    pub name: String,
    /// `#d9534f`, as GitLab paints it.
    pub color: String,
}

/// What the form that opens a merge request needs from GitLab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateFacts {
    pub labels: Vec<Label>,
    /// `never`, `always`, `default_on` or `default_off`.
    pub squash_option: String,
    /// The project deletes the source branch of a merged merge request by default.
    pub remove_source_branch: bool,
    /// The account signed in.
    pub me: Option<GitLabUser>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_as_typed() {
        let server = |typed| Server::parse(typed).map(|s| (s.host, s.web));
        assert_eq!(server(""), Ok(("gitlab.com".into(), "https://gitlab.com".into())));
        assert_eq!(
            server(" GitLab.Example.com/team/app "),
            Ok(("gitlab.example.com".into(), "https://gitlab.example.com".into()))
        );
        assert_eq!(
            server("https://git.corp:8443/"),
            Ok(("git.corp:8443".into(), "https://git.corp:8443".into()))
        );
        assert_eq!(
            server("http://10.0.0.5:80"),
            Ok(("10.0.0.5".into(), "http://10.0.0.5".into()))
        );
        assert!(server("ftp://gitlab.com").is_err());
        assert!(server("git lab").is_err());
        assert!(server("gitlab.com:port").is_err());
        assert_eq!(
            Server::parse("").unwrap().new_token_url(),
            "https://gitlab.com/-/user_settings/personal_access_tokens?name=Oxbow&scopes=api"
        );
    }

    #[test]
    fn old_servers_and_tokens_without_api_are_refused() {
        let me = |version: &str, scopes: Option<&[&str]>| Me {
            user: User {
                id: 1,
                username: "anna".into(),
                name: None,
                avatar_url: None,
                web_url: String::new(),
            },
            scopes: scopes.map(|s| s.iter().map(|s| (*s).to_owned()).collect()),
            expires_at: None,
            version: version.into(),
        };
        assert_eq!(me("16.0.0", Some(&["api"])).problem(), None);
        assert_eq!(me("17.5.1-ee", None).problem(), None);
        assert!(
            me("15.11.13-ee", Some(&["api"]))
                .problem()
                .unwrap()
                .contains("GitLab 15.11.13-ee")
        );
        assert!(
            me("17.0.0", Some(&["read_api", "read_user"]))
                .problem()
                .unwrap()
                .contains("it has read_api, read_user")
        );
        assert_eq!(version_number("16.11.2-ee"), Some((16, 11)));
    }
}
