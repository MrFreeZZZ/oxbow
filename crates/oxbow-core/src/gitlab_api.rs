//! GitLab's REST API (`/api/v4`), signed in with a personal access token: gitlab.com or a
//! server of your own. Without a sign-in nothing here is used, and Create Merge Request works
//! through the push (see `gitlab.rs`). Each call has a [`Request::display`] form, the same
//! request as `curl`, for the sheets that show what Oxbow will do.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

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

/// An HTTP request, kept to show as `curl` before it is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    pub json: Option<Value>,
}

impl Request {
    /// The request as `curl` would send it; the token shows as `$GITLAB_TOKEN`.
    pub fn display(&self) -> String {
        let mut parts = vec!["curl".to_owned()];
        if self.method != "GET" {
            parts.push(format!("-X {}", self.method));
        }
        parts.push("-H \"PRIVATE-TOKEN: $GITLAB_TOKEN\"".to_owned());
        parts.push(self.url.clone());
        if let Some(json) = &self.json {
            parts.push(format!("-d {}", crate::cli::shell_quote(&json.to_string())));
        }
        parts.join(" ")
    }
}

/// A connection to one GitLab server.
#[derive(Clone)]
pub struct Client {
    server: Server,
    api: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("api", &self.api)
            .field("signed_in", &self.token.is_some())
            .finish()
    }
}

impl Client {
    pub fn new(server: Server) -> Self {
        Client {
            api: format!("{}/api/v4", server.web),
            server,
            token: None,
            agent: crate::github::agent(),
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    pub fn server(&self) -> &Server {
        &self.server
    }

    // Requests, to show and then send.

    pub fn user_request(&self) -> Request {
        self.api_request("GET", "/user", None)
    }

    pub fn token_request(&self) -> Request {
        self.api_request("GET", "/personal_access_tokens/self", None)
    }

    pub fn version_request(&self) -> Request {
        self.api_request("GET", "/version", None)
    }

    pub(crate) fn api_request(&self, method: &'static str, path: &str, json: Option<Value>) -> Request {
        Request {
            method,
            url: format!("{}{path}", self.api),
            json,
        }
    }

    // Calls.

    /// Who the token belongs to, what it may do and which GitLab the server runs.
    pub fn me(&self) -> Result<Me> {
        let user = self.call(&self.user_request())?;
        let user: User = serde_json::from_value(user).map_err(|_| self.not_gitlab())?;
        let version = self
            .call(&self.version_request())?
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        // A token of another kind (a project's, OAuth) has no such page: nothing to judge it by.
        let token = self.call(&self.token_request()).unwrap_or(Value::Null);
        let scopes = token
            .get("scopes")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).map(str::to_owned).collect());
        let expires_at = token.get("expires_at").and_then(Value::as_str).map(str::to_owned);
        Ok(Me {
            user,
            scopes,
            expires_at,
            version,
        })
    }

    /// The picture at `url`, such as an account's avatar, as a `data:` address the window can
    /// show without loading anything from elsewhere; `None` when it can't be had or is too big.
    pub fn picture(&self, url: &str) -> Option<String> {
        let mut response = self.agent.get(url).call().ok()?;
        if response.status() != 200 {
            return None;
        }
        let kind = response.headers().get("content-type")?.to_str().ok()?;
        let kind = kind.split(';').next()?.trim().to_owned();
        if !kind.starts_with("image/") {
            return None;
        }
        let bytes = response.body_mut().with_config().limit(256 * 1024).read_to_vec().ok()?;
        Some(format!("data:{kind};base64,{}", base64(&bytes)))
    }

    /// Send an API request; an error status comes back in words for people.
    pub(crate) fn call(&self, request: &Request) -> Result<Value> {
        let token = self
            .token
            .as_deref()
            .ok_or_else(|| gitlab(Some(401), format!("Not signed in to {}", self.server.host)))?;
        let mut builder = ureq::http::Request::builder()
            .method(request.method)
            .uri(&request.url)
            .header("Accept", "application/json")
            .header("PRIVATE-TOKEN", token);
        let body = match &request.json {
            Some(json) => {
                builder = builder.header("Content-Type", "application/json");
                json.to_string()
            }
            None => String::new(),
        };
        let http = builder.body(body).map_err(|err| gitlab(None, err.to_string()))?;
        let mut response = self.agent.run(http).map_err(|err| self.network(err))?;
        let status = response.status().as_u16();
        let text = response.body_mut().read_to_string().map_err(|err| self.network(err))?;
        let body = if text.trim().is_empty() {
            Value::Null
        } else {
            // A page instead of JSON: something else answers at this address.
            serde_json::from_str(&text).map_err(|_| self.not_gitlab())?
        };
        if status >= 400 {
            return Err(gitlab(Some(status), api_error(status, &body)));
        }
        Ok(body)
    }

    fn not_gitlab(&self) -> Error {
        gitlab(
            None,
            format!(
                "{} doesn’t answer like GitLab. Check the server’s address.",
                self.server.host
            ),
        )
    }

    fn network(&self, err: ureq::Error) -> Error {
        let host = &self.server.host;
        let message = match &err {
            ureq::Error::Io(io) => format!("Can't reach {host}: {io}"),
            ureq::Error::Timeout(_) => format!("{host} didn't answer in time"),
            ureq::Error::HostNotFound => {
                format!("Can't find {host}: check the address, and whether this computer is online")
            }
            other => format!("Can't reach {host}: {other}"),
        };
        gitlab(None, message)
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn gitlab(status: Option<u16>, message: String) -> Error {
    Error::GitLab { status, message }
}

/// GitLab's own words for a failed API call, or ours where it has none.
fn api_error(status: u16, body: &Value) -> String {
    // `message` is a string, or a map of field to its problems when a write is refused.
    let message = match body.get("message") {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Object(fields)) => fields
            .iter()
            .map(|(field, problems)| match problems {
                Value::Array(list) => format!(
                    "{field} {}",
                    list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")
                ),
                other => format!("{field} {other}"),
            })
            .collect::<Vec<_>>()
            .join(". "),
        _ => body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    };
    match status {
        401 => "GitLab didn't accept the token: it is wrong, expired or revoked".to_owned(),
        403 => "The token may not do this: it needs the api scope, or access to the project".to_owned(),
        _ if !message.is_empty() => message,
        _ => format!("GitLab answered with error {status}"),
    }
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
    fn pictures_as_base64() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0x00, 0x01]), "//4AAQ==");
    }

    #[test]
    fn requests_show_as_curl() {
        let client = Client::new(Server::parse("git.corp:8443").unwrap());
        assert_eq!(
            client.user_request().display(),
            "curl -H \"PRIVATE-TOKEN: $GITLAB_TOKEN\" https://git.corp:8443/api/v4/user"
        );
    }

    #[test]
    fn old_servers_and_tokens_without_api_are_refused() {
        let me = |version: &str, scopes: Option<&[&str]>| Me {
            user: User {
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
