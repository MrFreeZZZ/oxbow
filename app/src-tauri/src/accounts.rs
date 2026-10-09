//! Settings › Accounts › GitHub: signing in, and keeping the token.
//!
//! The token lives in the computer's credential store (the Keychain on macOS, Credential Manager
//! on Windows, the Secret Service on Linux), never in a file. Who is signed in (login, name,
//! picture) is kept in `accounts.json` next to settings.json, so Settings can show it without
//! reading the Keychain, which macOS may ask about.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use oxbow_core::github::{self, Client, DeviceCode, DevicePoll, Publish};
use oxbow_core::{ActionEvent, OutputLine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::CommandResult;

/// The Client ID of Oxbow's GitHub OAuth app (registered by MrFreeZZZ, device flow on, tokens
/// that don't expire), for signing in with the browser. It is public, not a secret. Settings can
/// name another app with `oxbow.github.clientId`.
const CLIENT_ID: &str = "Ov23liNWrLpV4KFn313e";

/// The Keychain item: "Oxbow GitHub", account = the login, then `#` and when it was signed in
/// (see [`Account::item`]).
const SERVICE: &str = "Oxbow GitHub";

/// The signed-in GitHub account, as Settings shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub login: String,
    pub name: Option<String>,
    pub avatar_url: String,
    pub html_url: String,
    /// `browser` (device flow) or `token` (pasted).
    pub method: String,
    /// The token's scopes; `None` for a fine-grained token.
    pub scopes: Option<Vec<String>>,
    /// The credential store item with the token. Each sign-in writes a new one, so the item in
    /// use is never written over; accounts saved before that use the bare login.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
}

impl Account {
    fn item(&self) -> &str {
        self.item.as_deref().unwrap_or(&self.login)
    }
}

/// The token, read from the credential store once and kept for the session.
#[derive(Default)]
pub struct Accounts {
    token: Arc<Mutex<Option<String>>>,
    /// Counts the times signing in was cancelled. A sign-in remembers the count it started with
    /// and keeps nothing once it changed, whatever it was waiting for then.
    cancels: AtomicU64,
}

fn accounts_file(app: &AppHandle) -> CommandResult<PathBuf> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join("accounts.json"))
        .map_err(|err| err.to_string())
}

/// The account in accounts.json, if any.
pub fn saved(app: &AppHandle) -> Option<Account> {
    saved_in(&accounts_file(app).ok()?)
}

fn saved_in(file: &Path) -> Option<Account> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut all: serde_json::Map<String, Value> = serde_json::from_str(&text).ok()?;
    serde_json::from_value(all.remove("github.com")?).ok()
}

fn save(app: &AppHandle, account: Option<&Account>) -> CommandResult<()> {
    save_in(&accounts_file(app)?, account)
}

fn save_in(file: &Path, account: Option<&Account>) -> CommandResult<()> {
    std::fs::create_dir_all(file.parent().expect("accounts file has a parent")).map_err(|err| err.to_string())?;
    let mut all = serde_json::Map::new();
    if let Some(account) = account {
        all.insert(
            "github.com".into(),
            serde_json::to_value(account).map_err(|err| err.to_string())?,
        );
    }
    let text = serde_json::to_string_pretty(&all).map_err(|err| err.to_string())?;
    // Written whole or not at all: a half-written file would forget who is signed in.
    let part = file.with_extension("json.part");
    std::fs::write(&part, text + "\n").map_err(|err| err.to_string())?;
    std::fs::rename(&part, file).map_err(|err| {
        let _ = std::fs::remove_file(&part);
        err.to_string()
    })
}

fn entry(item: &str) -> CommandResult<keyring::Entry> {
    keyring::Entry::new(SERVICE, item).map_err(|err| keychain_error(&err))
}

/// Where tokens are kept, by item: the computer's credential store, or a map in tests.
trait Store {
    fn set(&self, item: &str, token: &str) -> CommandResult<()>;
    /// Take `item` out; one that isn't there is fine.
    fn delete(&self, item: &str) -> CommandResult<()>;
}

struct Keyring;

impl Store for Keyring {
    fn set(&self, item: &str, token: &str) -> CommandResult<()> {
        entry(item)?.set_password(token).map_err(|err| keychain_error(&err))
    }

    fn delete(&self, item: &str) -> CommandResult<()> {
        match entry(item)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(keychain_error(&err)),
        }
    }
}

/// Sign in as `account` with `token`, the accounts file being `file`. The token goes into a new
/// item, the file then points at it, and the old account's item goes last: until the file is
/// written nothing in use has changed, so `stopped` (asked once the store took the token, which
/// may have asked first) or a file that can't be written leaves the old account and its token as
/// they were. An item that can't be taken out afterwards is only left unused.
fn switch_to(
    store: &dyn Store,
    file: &Path,
    mut account: Account,
    token: &str,
    stopped: &dyn Fn() -> bool,
) -> CommandResult<Account> {
    let old = saved_in(file);
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    // GitHub logins have no `#`.
    let item = format!("{}#{stamp}", account.login);
    store.set(&item, token)?;
    if stopped() {
        let _ = store.delete(&item);
        return Err("stopped".into());
    }
    account.item = Some(item.clone());
    if let Err(err) = save_in(file, Some(&account)) {
        let _ = store.delete(&item);
        return Err(err);
    }
    if let Some(old) = old
        && old.item() != item
    {
        let _ = store.delete(old.item());
    }
    Ok(account)
}

fn keychain_error(err: &keyring::Error) -> String {
    match err {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            format!("Oxbow can't use the {}: {err}", store_name())
        }
        _ => err.to_string(),
    }
}

/// What the computer's credential store is called, for messages.
pub fn store_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "Keychain"
    } else if cfg!(target_os = "windows") {
        "Windows Credential Manager"
    } else {
        "keyring"
    }
}

impl Accounts {
    /// The token of the signed-in account, from memory or the credential store.
    pub fn token(&self, app: &AppHandle) -> Option<String> {
        let mut cached = self.token.lock().expect("token lock");
        if cached.is_none() {
            let account = saved(app)?;
            *cached = entry(account.item()).ok()?.get_password().ok();
        }
        cached.clone()
    }

    /// A client signed in as the account; fails with a message for people when nobody is.
    pub fn client(&self, app: &AppHandle) -> CommandResult<Client> {
        let token = self.token(app).ok_or_else(|| match saved(app) {
            Some(_) => format!(
                "Oxbow can't read the GitHub token from the {}. Sign in again in Settings › Accounts.",
                store_name()
            ),
            None => "Sign in to GitHub in Settings › Accounts first".to_owned(),
        })?;
        Ok(Client::default().with_token(token))
    }

    /// The count of cancels now, for a sign-in to start with.
    fn attempt(&self) -> u64 {
        self.cancels.load(Ordering::SeqCst)
    }

    /// Stop every sign-in under way: none of them keeps a token any more.
    fn cancel(&self) {
        self.cancels.fetch_add(1, Ordering::SeqCst);
    }

    fn cancelled(&self, attempt: u64) -> bool {
        self.attempt() != attempt
    }

    /// Keep the token of the sign-in that started as `attempt`, unless it was cancelled since.
    fn keep(
        &self,
        app: &AppHandle,
        attempt: u64,
        me: github::Me,
        token: String,
        method: &str,
    ) -> CommandResult<Account> {
        if self.cancelled(attempt) {
            return Err("stopped".into());
        }
        let account = Account {
            login: me.user.login,
            name: me.user.name.filter(|name| !name.is_empty()),
            avatar_url: me.user.avatar_url,
            html_url: me.user.html_url,
            method: method.to_owned(),
            scopes: me.scopes,
            item: None,
        };
        let file = accounts_file(app)?;
        // The Keychain may have asked first: a Cancel meanwhile still wins.
        let account = switch_to(&Keyring, &file, account, &token, &|| self.cancelled(attempt))?;
        *self.token.lock().expect("token lock") = Some(token);
        Ok(account)
    }
}

/// Lend git the token for https://github.com remotes, read when a command first needs it.
pub fn lend_to_git(app: &AppHandle) {
    let app = app.clone();
    oxbow_core::config::set_github_token(Some(Box::new(move || app.state::<Accounts>().token(&app))));
}

fn client_id(app: &AppHandle) -> Option<String> {
    let from_settings = crate::read_settings(app)
        .get("oxbow.github.clientId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_owned);
    from_settings.or_else(|| Some(CLIENT_ID.to_owned()).filter(|id| !id.is_empty()))
}

/// What the sign-in sheet needs to know before it starts.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInSetup {
    /// Signing in with the browser works: Oxbow has a GitHub app.
    client_id: Option<String>,
    /// The requests each way makes, as `curl`, for the terminal block.
    device_requests: Vec<String>,
    token_request: String,
    new_token_url: String,
    scopes: Vec<String>,
    store: &'static str,
}

#[tauri::command]
pub fn github_account(app: AppHandle) -> Option<Account> {
    saved(&app)
}

#[tauri::command]
pub fn github_sign_in_setup(app: AppHandle) -> SignInSetup {
    let client = Client::default();
    let id = client_id(&app);
    let shown = id.clone().unwrap_or_else(|| "<client id>".into());
    SignInSetup {
        device_requests: vec![
            client.device_code_request(&shown).display(),
            client.device_poll_request(&shown, "<device code>").display(),
        ],
        client_id: id,
        token_request: client.user_request().display(),
        new_token_url: github::new_token_url(),
        scopes: github::SCOPES.iter().map(|s| (*s).to_owned()).collect(),
        store: store_name(),
    }
}

/// Ask GitHub for a code to approve in the browser.
#[tauri::command]
pub async fn github_device_start(app: AppHandle) -> CommandResult<DeviceCode> {
    let id = client_id(&app).ok_or("Signing in with the browser needs Oxbow’s GitHub app")?;
    tauri::async_runtime::spawn_blocking(move || Client::default().device_code(&id).map_err(|err| err.to_string()))
        .await
        .map_err(|err| err.to_string())?
}

/// Wait until the code is approved in the browser, then keep the token.
#[tauri::command]
pub async fn github_device_wait(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    device_code: String,
    interval: u64,
    expires_in: u64,
) -> CommandResult<Account> {
    let id = client_id(&app).ok_or("Signing in with the browser needs Oxbow’s GitHub app")?;
    let attempt = accounts.attempt();
    let waiter = app.clone();
    let token = tauri::async_runtime::spawn_blocking(move || -> CommandResult<String> {
        let accounts = waiter.state::<Accounts>();
        let client = Client::default();
        let deadline = Instant::now() + Duration::from_secs(expires_in.max(60));
        let mut every = Duration::from_secs(interval.max(1));
        loop {
            // Sleep in short steps so Cancel answers at once.
            let wake = Instant::now() + every;
            while Instant::now() < wake {
                if accounts.cancelled(attempt) {
                    return Err("stopped".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            if Instant::now() > deadline {
                return Err("The code expired. Start again to get a new one.".into());
            }
            match client.poll_device(&id, &device_code).map_err(|err| err.to_string())? {
                DevicePoll::Waiting => {}
                DevicePoll::SlowDown(seconds) => every = Duration::from_secs(seconds.max(every.as_secs() + 5)),
                DevicePoll::Approved(token) => return Ok(token),
                DevicePoll::Denied => return Err("Oxbow wasn’t approved on GitHub.".into()),
                DevicePoll::Expired => return Err("The code expired. Start again to get a new one.".into()),
            }
        }
    })
    .await
    .map_err(|err| err.to_string())??;
    finish(&app, &accounts, attempt, token, "browser").await
}

/// Cancel signing in, whatever it is waiting for: the code, the browser or GitHub's answer.
#[tauri::command]
pub fn github_device_stop(accounts: State<'_, Accounts>) {
    accounts.cancel();
}

/// Sign in with a personal access token.
#[tauri::command]
pub async fn github_sign_in_token(
    app: AppHandle,
    accounts: State<'_, Accounts>,
    token: String,
) -> CommandResult<Account> {
    let attempt = accounts.attempt();
    let token = token.trim().to_owned();
    if token.is_empty() {
        return Err("Paste a token first".into());
    }
    finish(&app, &accounts, attempt, token, "token").await
}

async fn finish(
    app: &AppHandle,
    accounts: &Accounts,
    attempt: u64,
    token: String,
    method: &str,
) -> CommandResult<Account> {
    let checked = token.clone();
    let me = tauri::async_runtime::spawn_blocking(move || Client::default().with_token(checked).me())
        .await
        .map_err(|err| err.to_string())?
        .map_err(|err| err.to_string())?;
    let account = accounts.keep(app, attempt, me, token, method)?;
    let _ = app.emit("account-changed", ());
    Ok(account)
}

/// Forget the account: the token leaves the credential store. GitHub keeps the authorization
/// until it is revoked there.
#[tauri::command]
pub fn github_sign_out(app: AppHandle, accounts: State<'_, Accounts>) -> CommandResult<()> {
    if let Some(account) = saved(&app) {
        Keyring.delete(account.item())?;
    }
    save(&app, None)?;
    *accounts.token.lock().expect("token lock") = None;
    let _ = app.emit("account-changed", ());
    Ok(())
}

/// Open a github.com page in the browser.
#[tauri::command]
pub fn open_github(url: String) -> CommandResult<()> {
    if !url.starts_with("https://github.com/") {
        return Err("Only github.com pages open from here".into());
    }
    crate::open_in::open_url(&url)
}

/// Where Publish can put a repository: the account itself and its organizations.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Owners {
    login: String,
    orgs: Vec<String>,
}

#[tauri::command]
pub async fn github_owners(app: AppHandle, accounts: State<'_, Accounts>) -> CommandResult<Owners> {
    let account = saved(&app).ok_or("Sign in to GitHub in Settings › Accounts first")?;
    let client = accounts.client(&app)?;
    // Without read:org the list is empty, and the account itself still works.
    let orgs = tauri::async_runtime::spawn_blocking(move || client.orgs().unwrap_or_default())
        .await
        .map_err(|err| err.to_string())?;
    Ok(Owners {
        login: account.login,
        orgs,
    })
}

/// Make the repository on GitHub, add it as origin and push the branch. Each step goes to the
/// window that asked as an `action-event`, like any action; Stop stops the push. With `create`
/// false the repository is on GitHub already (an earlier Publish made it, then stopped): only
/// origin and the push are left.
#[tauri::command]
pub async fn github_publish(
    app: AppHandle,
    window: tauri::WebviewWindow,
    accounts: State<'_, Accounts>,
    path: String,
    publish: Publish,
    create: Option<bool>,
) -> CommandResult<String> {
    if !github::valid_repository_name(&publish.name) {
        return Err("Use letters, digits, - _ and . for the name".into());
    }
    let client = accounts.client(&app)?;
    let label = window.label().to_owned();
    let cancel = app.state::<crate::Session>().cancel.clone();
    cancel.store(false, Ordering::Relaxed);
    tauri::async_runtime::spawn_blocking(move || {
        let emit = |event: ActionEvent| {
            let _ = app.emit_to(label.as_str(), "action-event", event);
        };
        let say = |text: String| {
            emit(ActionEvent::Line(OutputLine {
                text,
                stderr: false,
                progress: false,
            }))
        };
        let html_url = if create == Some(false) {
            format!("https://github.com/{}/{}", publish.owner, publish.name)
        } else {
            emit(ActionEvent::Command {
                display: client.create_repository_request(&publish).display(),
            });
            let made = client.create_repository(&publish).map_err(|err| err.to_string())?;
            say(format!(
                "Created {} ({})",
                made.html_url,
                if made.private { "private" } else { "public" }
            ));
            made.html_url
        };
        let dir = PathBuf::from(&path);
        for command in publish.commands() {
            emit(ActionEvent::Command {
                display: command.display(),
            });
            oxbow_core::cli::run_streaming_in(&dir, &command, &mut |line| emit(ActionEvent::Line(line)), &cancel)
                .map_err(|err| match err {
                    oxbow_core::Error::Command { output, .. } => output,
                    other => other.to_string(),
                })?;
        }
        Ok(html_url)
    })
    .await
    .map_err(|err| err.to_string())?
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    use super::*;

    /// Items in memory; `refuse` makes taking any out fail, as a locked store would.
    #[derive(Default)]
    struct Memory {
        items: RefCell<BTreeMap<String, String>>,
        refuse: bool,
    }

    impl Store for Memory {
        fn set(&self, item: &str, token: &str) -> CommandResult<()> {
            self.items.borrow_mut().insert(item.into(), token.into());
            Ok(())
        }

        fn delete(&self, item: &str) -> CommandResult<()> {
            if self.refuse {
                return Err("refused".into());
            }
            self.items.borrow_mut().remove(item);
            Ok(())
        }
    }

    fn account(login: &str) -> Account {
        Account {
            login: login.into(),
            name: None,
            avatar_url: String::new(),
            html_url: String::new(),
            method: "token".into(),
            scopes: None,
            item: None,
        }
    }

    /// An accounts file signed in as maria with `old` under her bare login, as before items.
    fn signed_in(store: &Memory) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "oxbow-accounts-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("accounts.json");
        save_in(&file, Some(&account("maria"))).unwrap();
        store.set("maria", "old").unwrap();
        (dir, file)
    }

    fn token_in_use(store: &Memory, file: &Path) -> Option<String> {
        store.items.borrow().get(saved_in(file)?.item()).cloned()
    }

    #[test]
    fn signing_in_again_replaces_the_token_only_once_it_is_saved() {
        let store = Memory::default();
        let (dir, file) = signed_in(&store);
        switch_to(&store, &file, account("maria"), "new", &|| false).unwrap();
        assert_eq!(token_in_use(&store, &file).as_deref(), Some("new"));
        assert_eq!(store.items.borrow().len(), 1, "the old item is gone");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_cancelled_sign_in_keeps_the_token_in_use_even_when_nothing_can_be_taken_out() {
        for login in ["maria", "alex"] {
            let store = Memory {
                refuse: true,
                ..Memory::default()
            };
            let (dir, file) = signed_in(&store);
            let err = switch_to(&store, &file, account(login), "new", &|| true).unwrap_err();
            assert_eq!(err, "stopped");
            assert_eq!(saved_in(&file).unwrap().login, "maria");
            assert_eq!(token_in_use(&store, &file).as_deref(), Some("old"));
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn an_account_file_that_cant_be_written_keeps_the_old_sign_in() {
        let store = Memory::default();
        let (dir, file) = signed_in(&store);
        // A folder where the new file would go makes the rename fail.
        std::fs::create_dir_all(dir.join("accounts.json.part")).unwrap();
        switch_to(&store, &file, account("maria"), "new", &|| false).unwrap_err();
        assert_eq!(token_in_use(&store, &file).as_deref(), Some("old"));
        assert_eq!(store.items.borrow().len(), 1, "the new item is taken out again");
        let _ = std::fs::remove_dir_all(dir);
    }
}
