//! Settings › Accounts › GitLab: one account per server, gitlab.com or your own, each signed in
//! with a personal access token.
//!
//! As with GitHub, the token lives in the computer's credential store (service "Oxbow GitLab",
//! account `<host>/<username>#<when>`), and who is signed in where is kept in accounts.json
//! under `gitlab`, so Settings can show it without reading the Keychain.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use oxbow_core::gitlab_api::{self, Client, Server};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::CommandResult;
use crate::accounts::{Accounts, Keyring, Store, accounts_file, read_all, store_name, write_entry};

const SERVICE: &str = "Oxbow GitLab";

/// A GitLab account, as Settings shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabAccount {
    /// The server, `gitlab.com` or `git.example.com:8443`: one account each.
    pub host: String,
    /// Its address, `https://gitlab.com`.
    pub web: String,
    pub username: String,
    pub name: Option<String>,
    /// The account's picture as a `data:` address: the window loads nothing from the server.
    pub avatar: Option<String>,
    /// The account's page.
    pub web_url: String,
    /// The token's scopes; `None` when GitLab doesn't say.
    pub scopes: Option<Vec<String>>,
    /// The last day the token works, `2026-12-01`; `None` when it never expires.
    pub expires_at: Option<String>,
    /// The server's GitLab version when it was signed in to.
    pub version: String,
    /// The credential store item with the token.
    pub item: String,
}

impl GitLabAccount {
    pub fn server(&self) -> Server {
        Server {
            host: self.host.clone(),
            web: self.web.clone(),
        }
    }
}

/// The tokens read from the credential store, by host, kept for the session.
#[derive(Default)]
pub struct GitLabTokens(Mutex<HashMap<String, String>>);

impl GitLabTokens {
    /// The token of the account on `host`, from memory or the credential store.
    fn token(&self, account: &GitLabAccount) -> Option<String> {
        let mut cached = self.0.lock().expect("token lock");
        if !cached.contains_key(&account.host) {
            let token = crate::accounts::entry(SERVICE, &account.item)
                .ok()?
                .get_password()
                .ok()?;
            cached.insert(account.host.clone(), token);
        }
        cached.get(&account.host).cloned()
    }

    /// A client signed in to `host`; fails with a message for people when nobody is.
    pub fn client(&self, app: &AppHandle, host: &str) -> CommandResult<Client> {
        let account = saved(app)
            .into_iter()
            .find(|a| a.host == host)
            .ok_or_else(|| format!("Sign in to {host} in Settings › Accounts first"))?;
        let token = self.token(&account).ok_or_else(|| {
            format!(
                "Oxbow can't read the token for {host} from the {}. Sign in again in Settings › Accounts.",
                store_name()
            )
        })?;
        Ok(Client::new(account.server()).with_token(token))
    }

    fn forget(&self, host: &str) {
        self.0.lock().expect("token lock").remove(host);
    }
}

/// The GitLab accounts in accounts.json, in the order they were signed in to.
pub fn saved(app: &AppHandle) -> Vec<GitLabAccount> {
    accounts_file(app).map(|file| saved_in(&file)).unwrap_or_default()
}

fn saved_in(file: &Path) -> Vec<GitLabAccount> {
    read_all(file)
        .remove("gitlab")
        .and_then(|list| serde_json::from_value(list).ok())
        .unwrap_or_default()
}

fn save_in(file: &Path, accounts: &[GitLabAccount]) -> CommandResult<()> {
    let value = if accounts.is_empty() {
        None
    } else {
        Some(serde_json::to_value(accounts).map_err(|err| err.to_string())?)
    };
    write_entry(file, "gitlab", value)
}

/// The servers signed in to, for knowing a remote on one as GitLab whatever it is called.
pub fn servers(app: &AppHandle) -> Vec<Server> {
    saved(app).iter().map(GitLabAccount::server).collect()
}

/// Sign in as `account` with `token`, replacing the account on the same server. As with GitHub
/// (`accounts::switch_to`), the token goes into a new item and the file then points at it, so
/// `stopped` or a file that can't be written leaves what was signed in as it was.
fn switch_to(
    store: &dyn Store,
    file: &Path,
    mut account: GitLabAccount,
    token: &str,
    stopped: &dyn Fn() -> bool,
) -> CommandResult<GitLabAccount> {
    let mut all = saved_in(file);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let item = format!("{}/{}#{stamp}", account.host, account.username);
    store.set(&item, token)?;
    if stopped() {
        let _ = store.delete(&item);
        return Err("stopped".into());
    }
    account.item = item.clone();
    let old = all.iter().position(|a| a.host == account.host).map(|i| all.remove(i));
    all.push(account.clone());
    if let Err(err) = save_in(file, &all) {
        let _ = store.delete(&item);
        return Err(err);
    }
    if let Some(old) = old {
        let _ = store.delete(&old.item);
    }
    Ok(account)
}

/// What the sign-in sheet shows for the server typed so far.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLabSignInSetup {
    server: Server,
    /// The requests signing in makes, as `curl`, for the terminal block.
    requests: Vec<String>,
    new_token_url: String,
    scopes: Vec<String>,
    store: &'static str,
    /// The oldest GitLab Oxbow signs in to, `16.0`.
    oldest: String,
}

#[tauri::command]
pub fn gitlab_accounts(app: AppHandle) -> Vec<GitLabAccount> {
    saved(&app)
}

#[tauri::command]
pub fn gitlab_sign_in_setup(server: String) -> CommandResult<GitLabSignInSetup> {
    let server = Server::parse(&server)?;
    let client = Client::new(server.clone());
    Ok(GitLabSignInSetup {
        requests: vec![
            client.user_request().display(),
            client.version_request().display(),
            client.token_request().display(),
        ],
        new_token_url: server.new_token_url(),
        server,
        scopes: gitlab_api::SCOPES.iter().map(|s| (*s).to_owned()).collect(),
        store: store_name(),
        oldest: format!("{}.{}", gitlab_api::OLDEST.0, gitlab_api::OLDEST.1),
    })
}

/// Open the server's page for making a token, with the api scope ticked.
#[tauri::command]
pub fn gitlab_open_new_token(server: String) -> CommandResult<()> {
    crate::open_in::open_url(&Server::parse(&server)?.new_token_url())
}

/// Open the profile page of the account signed in on `host`.
#[tauri::command]
pub fn gitlab_open_profile(app: AppHandle, host: String) -> CommandResult<()> {
    let account = saved(&app)
        .into_iter()
        .find(|a| a.host == host)
        .ok_or("Not signed in there")?;
    if !account.web_url.starts_with(&format!("{}/", account.web)) {
        return Err("The profile isn't on that server".into());
    }
    crate::open_in::open_url(&account.web_url)
}

/// Sign in to `server` with a personal access token: GitLab says whose it is, what it may do
/// and which version the server runs, and Oxbow keeps it if it can work with them.
#[tauri::command]
pub async fn gitlab_sign_in(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    tokens: State<'_, GitLabTokens>,
    server: String,
    token: String,
) -> CommandResult<GitLabAccount> {
    let attempt = accounts.attempt();
    let server = Server::parse(&server)?;
    let token = token.trim().to_owned();
    if token.is_empty() {
        return Err("Paste a token first".into());
    }
    let client = Client::new(server.clone()).with_token(token.clone());
    let (me, avatar) = tauri::async_runtime::spawn_blocking(move || {
        let me = client.me()?;
        let avatar = me.user.avatar_url.as_deref().and_then(|url| client.picture(url));
        Ok::<_, oxbow_core::Error>((me, avatar))
    })
    .await
    .map_err(|err| err.to_string())?
    .map_err(|err| err.to_string())?;
    if accounts.cancelled(attempt) {
        return Err("stopped".into());
    }
    if let Some(problem) = me.problem() {
        return Err(problem);
    }
    let account = GitLabAccount {
        host: server.host.clone(),
        web: server.web,
        username: me.user.username,
        name: me.user.name.filter(|name| !name.is_empty()),
        avatar,
        web_url: me.user.web_url,
        scopes: me.scopes,
        expires_at: me.expires_at,
        version: me.version,
        item: String::new(),
    };
    let file = accounts_file(&app)?;
    // The Keychain may have asked first: a Cancel meanwhile still wins.
    let account = switch_to(&Keyring(SERVICE), &file, account, &token, &|| {
        accounts.cancelled(attempt)
    })?;
    tokens.forget(&account.host);
    let _ = app.emit("account-changed", ());
    Ok(account)
}

/// Cancel signing in while GitLab hasn't answered yet.
#[tauri::command]
pub fn gitlab_sign_in_stop(accounts: State<'_, Accounts>) {
    accounts.cancel();
}

/// Forget the account on `host`: its token leaves the credential store. GitLab keeps the token
/// until it expires or is revoked there.
#[tauri::command]
pub fn gitlab_sign_out(app: AppHandle, tokens: State<'_, GitLabTokens>, host: String) -> CommandResult<()> {
    let file = accounts_file(&app)?;
    let mut all = saved_in(&file);
    if let Some(i) = all.iter().position(|a| a.host == host) {
        let account = all.remove(i);
        Keyring(SERVICE).delete(&account.item)?;
        save_in(&file, &all)?;
    }
    tokens.forget(&host);
    let _ = app.emit("account-changed", ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::*;

    #[derive(Default)]
    struct Memory(RefCell<BTreeMap<String, String>>);

    impl Store for Memory {
        fn set(&self, item: &str, token: &str) -> CommandResult<()> {
            self.0.borrow_mut().insert(item.into(), token.into());
            Ok(())
        }

        fn delete(&self, item: &str) -> CommandResult<()> {
            self.0.borrow_mut().remove(item);
            Ok(())
        }
    }

    fn account(host: &str, username: &str) -> GitLabAccount {
        GitLabAccount {
            host: host.into(),
            web: format!("https://{host}"),
            username: username.into(),
            name: None,
            avatar: None,
            web_url: format!("https://{host}/{username}"),
            scopes: Some(vec!["api".into()]),
            expires_at: None,
            version: "17.5.0".into(),
            item: String::new(),
        }
    }

    fn file() -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "oxbow-gitlab-accounts-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        (dir.clone(), dir.join("accounts.json"))
    }

    #[test]
    fn one_account_per_server_beside_github() {
        let store = Memory::default();
        let (dir, file) = file();
        write_entry(&file, "github.com", Some(serde_json::json!({ "login": "maria" }))).unwrap();
        switch_to(&store, &file, account("gitlab.com", "anna"), "one", &|| false).unwrap();
        switch_to(&store, &file, account("git.corp:8443", "anna.b"), "two", &|| false).unwrap();
        switch_to(&store, &file, account("gitlab.com", "anna"), "three", &|| false).unwrap();

        let hosts: Vec<String> = saved_in(&file).iter().map(|a| a.host.clone()).collect();
        assert_eq!(hosts, ["git.corp:8443", "gitlab.com"]);
        let mut tokens: Vec<String> = store.0.borrow().values().cloned().collect();
        tokens.sort();
        assert_eq!(tokens, ["three", "two"], "the replaced token is gone");
        assert!(read_all(&file).contains_key("github.com"), "GitHub's sign-in stays");

        let err = switch_to(&store, &file, account("gitlab.com", "other"), "four", &|| true).unwrap_err();
        assert_eq!(err, "stopped");
        assert_eq!(saved_in(&file)[1].username, "anna");
        assert_eq!(store.0.borrow().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
