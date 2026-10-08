//! GitHub's REST API: signing in to an account and publishing a repository.
//!
//! Git itself never needs this: fetch and push go through the `git` command line. The API is for
//! what git can't do, such as knowing who is signed in. Signing in uses GitHub's device flow (the
//! browser shows a code to approve, as `gh auth login --web` does) or a personal access token.
//! Each call has a [`Request::display`] form, the same request as `curl`, for the sheets that show
//! what Oxbow will do.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::GitCommand;
use crate::error::{Error, Result};

/// github.com's API and web addresses.
pub const API: &str = "https://api.github.com";
pub const WEB: &str = "https://github.com";

/// What Oxbow asks a token for: private repositories (create, push, pull requests), workflow
/// files (a push that changes `.github/workflows` needs it) and organizations to publish to.
pub const SCOPES: &[&str] = &["repo", "workflow", "read:org"];

/// Where to make a classic personal access token with [`SCOPES`] already ticked.
pub fn new_token_url() -> String {
    format!(
        "{WEB}/settings/tokens/new?scopes={}&description=Oxbow",
        SCOPES.join(",")
    )
}

/// A GitHub account's public face.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct User {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: String,
    pub html_url: String,
}

/// Who a token belongs to and what it may do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Me {
    pub user: User,
    /// The token's scopes; `None` for a fine-grained token, whose permissions GitHub doesn't
    /// list.
    pub scopes: Option<Vec<String>>,
}

/// A repository GitHub just made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct NewRepository {
    pub full_name: String,
    pub html_url: String,
    pub clone_url: String,
    pub private: bool,
}

/// What Publish to GitHub makes: a repository under `owner` (the signed-in account or one of its
/// organizations), then `origin` pointing at it and the branch pushed there.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Publish {
    pub owner: String,
    /// Whether `owner` is the signed-in account rather than an organization.
    pub personal: bool,
    pub name: String,
    pub private: bool,
    #[serde(default)]
    pub description: String,
    /// The branch to push.
    pub branch: String,
}

impl Publish {
    /// Where the repository will be, before it exists: the HTTPS address, which the sign-in
    /// answers for.
    pub fn url(&self) -> String {
        format!("{WEB}/{}/{}.git", self.owner, self.name)
    }

    /// The git half of publishing, run in the repository after GitHub made its copy.
    pub fn commands(&self) -> Vec<GitCommand> {
        let mut push = vec!["push".to_owned()];
        if !crate::config::run_hooks() {
            push.push("--no-verify".to_owned());
        }
        push.extend(["-u".to_owned(), "origin".to_owned(), self.branch.clone()]);
        vec![
            GitCommand::new(["remote", "add", "origin", &self.url()])
                .comment("the new GitHub repository becomes origin"),
            GitCommand::new(push)
                .comment(format!("-u: remember origin/{} as the upstream", self.branch))
                .with_progress(),
        ]
    }
}

/// Whether a name is one GitHub takes for a repository as it is: letters, digits, `-`, `_` and
/// `.`, at most 100 characters, and not `.` or `..`.
pub fn valid_repository_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 100
        && name != "."
        && name != ".."
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}

/// The code to type on github.com/login/device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all(serialize = "camelCase"))]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// Seconds until the code stops working.
    pub expires_in: u64,
    /// Seconds to wait between asking whether it was approved.
    pub interval: u64,
}

/// What GitHub says while the code waits for approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePoll {
    Waiting,
    /// Asked too often: wait this many seconds from now on.
    SlowDown(u64),
    Approved(String),
    Denied,
    Expired,
}

/// An HTTP request, kept to show as `curl` before it is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: &'static str,
    pub url: String,
    /// A form body (`-d key=value`) for github.com's sign-in endpoints.
    pub form: Vec<(String, String)>,
    /// A JSON body for the API.
    pub json: Option<Value>,
    /// Sent with the token.
    pub authorized: bool,
}

impl Request {
    /// The request as `curl` would send it; the token shows as `$GITHUB_TOKEN`.
    pub fn display(&self) -> String {
        let mut parts = vec!["curl".to_owned()];
        if self.method != "GET" {
            parts.push(format!("-X {}", self.method));
        }
        if self.authorized {
            parts.push("-H \"Authorization: Bearer $GITHUB_TOKEN\"".to_owned());
        }
        parts.push(self.url.clone());
        for (key, value) in &self.form {
            parts.push(format!("-d {}", crate::cli::shell_quote(&format!("{key}={value}"))));
        }
        if let Some(json) = &self.json {
            parts.push(format!("-d {}", crate::cli::shell_quote(&json.to_string())));
        }
        parts.join(" ")
    }
}

/// A connection to github.com, signed in or not.
#[derive(Clone)]
pub struct Client {
    api: String,
    web: String,
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

impl Default for Client {
    /// github.com, or the server `OXBOW_GITHUB_API` names, for trying Oxbow against a stand-in.
    fn default() -> Self {
        match std::env::var("OXBOW_GITHUB_API") {
            Ok(api) if !api.is_empty() => Client::new(&api, &api),
            _ => Client::new(API, WEB),
        }
    }
}

impl Client {
    /// A client for another address, e.g. a local test server.
    pub fn new(api: &str, web: &str) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            // The computer's own trust store, so a company proxy's certificate works as in a
            // browser.
            .root_certs(ureq::tls::RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .user_agent(concat!("Oxbow/", env!("CARGO_PKG_VERSION")))
            .tls_config(tls)
            .build()
            .new_agent();
        Client {
            api: api.trim_end_matches('/').to_owned(),
            web: web.trim_end_matches('/').to_owned(),
            token: None,
            agent,
        }
    }

    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    // Requests, to show and then send.

    pub fn device_code_request(&self, client_id: &str) -> Request {
        Request {
            method: "POST",
            url: format!("{}/login/device/code", self.web),
            form: vec![
                ("client_id".into(), client_id.into()),
                ("scope".into(), SCOPES.join(" ")),
            ],
            json: None,
            authorized: false,
        }
    }

    pub fn device_poll_request(&self, client_id: &str, device_code: &str) -> Request {
        Request {
            method: "POST",
            url: format!("{}/login/oauth/access_token", self.web),
            form: vec![
                ("client_id".into(), client_id.into()),
                ("device_code".into(), device_code.into()),
                (
                    "grant_type".into(),
                    "urn:ietf:params:oauth:grant-type:device_code".into(),
                ),
            ],
            json: None,
            authorized: false,
        }
    }

    pub fn user_request(&self) -> Request {
        self.api_request("GET", "/user", None)
    }

    pub fn orgs_request(&self) -> Request {
        self.api_request("GET", "/user/orgs?per_page=100", None)
    }

    pub fn create_repository_request(&self, publish: &Publish) -> Request {
        let path = if publish.personal {
            "/user/repos".to_owned()
        } else {
            format!("/orgs/{}/repos", publish.owner)
        };
        let mut body = serde_json::json!({ "name": publish.name, "private": publish.private });
        if !publish.description.trim().is_empty() {
            body["description"] = Value::String(publish.description.trim().to_owned());
        }
        self.api_request("POST", &path, Some(body))
    }

    pub(crate) fn api_request(&self, method: &'static str, path: &str, json: Option<Value>) -> Request {
        Request {
            method,
            url: format!("{}{path}", self.api),
            form: Vec::new(),
            json,
            authorized: true,
        }
    }

    // Calls.

    /// Ask for a code to approve in the browser.
    pub fn device_code(&self, client_id: &str) -> Result<DeviceCode> {
        let Response { status, body, .. } = self.send(&self.device_code_request(client_id))?;
        if let Some(error) = body.get("error").and_then(Value::as_str) {
            return Err(github(Some(status), device_error(error, &body)));
        }
        serde_json::from_value(body.clone()).map_err(|_| github(Some(status), api_error(status, &body)))
    }

    /// Ask once whether the code was approved.
    pub fn poll_device(&self, client_id: &str, device_code: &str) -> Result<DevicePoll> {
        let Response { status, body, .. } = self.send(&self.device_poll_request(client_id, device_code))?;
        if let Some(token) = body.get("access_token").and_then(Value::as_str) {
            return Ok(DevicePoll::Approved(token.to_owned()));
        }
        match body.get("error").and_then(Value::as_str) {
            Some("authorization_pending") => Ok(DevicePoll::Waiting),
            Some("slow_down") => Ok(DevicePoll::SlowDown(
                body.get("interval").and_then(Value::as_u64).unwrap_or(10),
            )),
            Some("access_denied") => Ok(DevicePoll::Denied),
            Some("expired_token") => Ok(DevicePoll::Expired),
            Some(error) => Err(github(Some(status), device_error(error, &body))),
            None => Err(github(Some(status), api_error(status, &body))),
        }
    }

    /// Who the token belongs to.
    pub fn me(&self) -> Result<Me> {
        let response = self.call(&self.user_request())?;
        let scopes = response.scopes;
        let user = serde_json::from_value(response.body)
            .map_err(|err| github(None, format!("GitHub sent an unexpected answer: {err}")))?;
        Ok(Me { user, scopes })
    }

    /// The organizations the account belongs to, by login.
    pub fn orgs(&self) -> Result<Vec<String>> {
        let body = self.call(&self.orgs_request())?.body;
        Ok(body
            .as_array()
            .map(|orgs| {
                orgs.iter()
                    .filter_map(|org| org.get("login").and_then(Value::as_str).map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Make an empty repository on GitHub.
    pub fn create_repository(&self, publish: &Publish) -> Result<NewRepository> {
        let response = self.call(&self.create_repository_request(publish))?;
        serde_json::from_value(response.body)
            .map_err(|err| github(None, format!("GitHub sent an unexpected answer: {err}")))
    }

    /// How many commits `branch` of `owner/name` has: a list of one commit per page, so the
    /// number of the last page. Signed in when there is a token, so private repositories count.
    pub fn commit_count(&self, owner: &str, name: &str, branch: &str) -> Result<u64> {
        let mut request = self.api_request(
            "GET",
            &format!("/repos/{owner}/{name}/commits?sha={}&per_page=1", percent(branch)),
            None,
        );
        request.authorized = self.token.is_some();
        let response = self.call(&request)?;
        if let Some(last) = response.link.as_deref().and_then(last_page) {
            return Ok(last);
        }
        Ok(response.body.as_array().map_or(0, |list| list.len() as u64))
    }

    /// Send an API request; an error status comes back as GitHub's own message.
    pub(crate) fn call(&self, request: &Request) -> Result<Response> {
        let response = self.send(request)?;
        if response.status >= 400 {
            return Err(github(
                Some(response.status),
                api_error(response.status, &response.body),
            ));
        }
        Ok(response)
    }

    fn send(&self, request: &Request) -> Result<Response> {
        let accept = if request.authorized {
            "application/vnd.github+json"
        } else {
            "application/json"
        };
        let mut builder = ureq::http::Request::builder()
            .method(request.method)
            .uri(&request.url)
            .header("Accept", accept);
        if request.authorized {
            let token = self
                .token
                .as_deref()
                .ok_or_else(|| github(Some(401), "Not signed in to GitHub".into()))?;
            builder = builder
                .header("Authorization", format!("Bearer {token}"))
                .header("X-GitHub-Api-Version", "2022-11-28");
        }
        let body = if let Some(json) = &request.json {
            builder = builder.header("Content-Type", "application/json");
            json.to_string()
        } else if !request.form.is_empty() {
            builder = builder.header("Content-Type", "application/x-www-form-urlencoded");
            form_encode(&request.form)
        } else {
            String::new()
        };
        let http = builder.body(body).map_err(|err| github(None, err.to_string()))?;
        let mut response = self.agent.run(http).map_err(network)?;
        let status = response.status().as_u16();
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        // Fine-grained and app tokens list no scopes: nothing to judge them by.
        let scopes = header("x-oauth-scopes")
            .map(|list| {
                list.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .filter(|scopes| !scopes.is_empty());
        let link = header("link");
        let text = response.body_mut().read_to_string().map_err(network)?;
        let body = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        };
        Ok(Response {
            status,
            scopes,
            link,
            body,
        })
    }
}

/// An API answer.
#[derive(Debug, Clone)]
pub(crate) struct Response {
    pub status: u16,
    pub scopes: Option<Vec<String>>,
    /// The `Link` header, which points at the other pages of a list.
    pub link: Option<String>,
    pub body: Value,
}

/// The page number of the `rel="last"` link in a `Link` header.
fn last_page(link: &str) -> Option<u64> {
    link.split(',')
        .find(|part| part.contains("rel=\"last\""))
        .and_then(|part| {
            let url = part
                .split(';')
                .next()?
                .trim()
                .trim_start_matches('<')
                .trim_end_matches('>');
            let query = url.split_once('?')?.1;
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix("page="))
                .and_then(|n| n.parse().ok())
        })
}

fn github(status: Option<u16>, message: String) -> Error {
    Error::GitHub { status, message }
}

fn network(err: ureq::Error) -> Error {
    let message = match &err {
        ureq::Error::Io(io) => format!("Can't reach GitHub: {io}"),
        ureq::Error::Timeout(_) => "GitHub didn't answer in time".to_owned(),
        ureq::Error::HostNotFound => "Can't reach GitHub: is this computer online?".to_owned(),
        other => format!("Can't reach GitHub: {other}"),
    };
    github(None, message)
}

/// GitHub's own words for a failed API call, or ours where it has none.
fn api_error(status: u16, body: &Value) -> String {
    let message = body.get("message").and_then(Value::as_str).unwrap_or_default();
    let details: Vec<&str> = body
        .get("errors")
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .filter_map(|e| e.get("message").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    match status {
        401 => "GitHub didn't accept the token: it is wrong, expired or revoked".to_owned(),
        403 | 404 if message.contains("Resource not accessible") || message == "Not Found" => {
            "The token may not do this: it needs the repo scope, or access to the organization".to_owned()
        }
        _ if details.iter().any(|d| d.contains("name already exists")) => {
            "A repository with this name already exists there. Pick another name.".to_owned()
        }
        _ if !details.is_empty() => details.join(". "),
        _ if !message.is_empty() => message.to_owned(),
        _ if status >= 400 => format!("GitHub answered with error {status}"),
        _ => "GitHub sent an unexpected answer".to_owned(),
    }
}

fn device_error(error: &str, body: &Value) -> String {
    match error {
        "device_flow_disabled" => "Device flow is turned off for Oxbow’s GitHub app".to_owned(),
        "incorrect_client_credentials" => "GitHub doesn't know Oxbow’s app ID".to_owned(),
        _ => body
            .get("error_description")
            .and_then(Value::as_str)
            .unwrap_or(error)
            .to_owned(),
    }
}

fn form_encode(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", percent(k), percent(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn percent(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "+".to_owned(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_show_as_curl() {
        let client = Client::default();
        assert_eq!(
            client.device_code_request("Ov23abc").display(),
            "curl -X POST https://github.com/login/device/code -d client_id=Ov23abc -d 'scope=repo workflow read:org'"
        );
        assert_eq!(
            client.user_request().display(),
            "curl -H \"Authorization: Bearer $GITHUB_TOKEN\" https://api.github.com/user"
        );
    }

    #[test]
    fn publishing_shows_the_request_and_commands() {
        let publish = Publish {
            owner: "acme".into(),
            personal: false,
            name: "api".into(),
            private: true,
            description: String::new(),
            branch: "main".into(),
        };
        assert_eq!(
            Client::default().create_repository_request(&publish).display(),
            r#"curl -X POST -H "Authorization: Bearer $GITHUB_TOKEN" https://api.github.com/orgs/acme/repos -d '{"name":"api","private":true}'"#
        );
        let shown: Vec<String> = publish.commands().iter().map(GitCommand::display).collect();
        assert_eq!(
            shown,
            [
                "git remote add origin https://github.com/acme/api.git",
                "git push -u origin main"
            ]
        );
        assert!(valid_repository_name("my-app.rs_2"));
        assert!(!valid_repository_name("my app") && !valid_repository_name("..") && !valid_repository_name(""));
    }

    #[test]
    fn forms_are_encoded() {
        assert_eq!(
            form_encode(&[("scope".into(), "repo read:org".into()), ("a".into(), "b&c".into())]),
            "scope=repo+read%3Aorg&a=b%26c"
        );
    }
}
