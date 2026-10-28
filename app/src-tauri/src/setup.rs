//! The Welcome window: cloning a repository and creating a new one.

use std::path::Path;
use std::sync::atomic::Ordering;

use oxbow_core::config;
use oxbow_core::console;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::session::{Session, blocking, logged};
use crate::{CommandResult, accounts, trusted};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WelcomeInfo {
    version: String,
    git: oxbow_core::GitInfo,
    name: Option<String>,
    email: Option<String>,
    ssh: oxbow_core::SshSource,
    /// `init.defaultBranch`, when set.
    default_branch: Option<String>,
    home: String,
    /// Where clones and new repositories go unless the user picks another folder.
    projects: String,
    gitignores: Vec<(String, String)>,
    licenses: Vec<(String, String)>,
}

/// What the Welcome window says about this computer's Git setup.
#[tauri::command]
pub(crate) async fn welcome_info() -> CommandResult<WelcomeInfo> {
    let version = format!("{} ({})", env!("OXBOW_VERSION"), env!("OXBOW_COMMIT"));
    logged(None, console::EntryKind::Read, "Read the Git setup", move || {
        let global = config::global_config();
        let home = config::home_dir().unwrap_or_default();
        let projects = ["Developer", "Projects", "projects", "src", "Code", "code"]
            .iter()
            .map(|name| home.join(name))
            .find(|dir| dir.is_dir())
            .unwrap_or_else(|| home.clone());
        Ok(WelcomeInfo {
            version,
            git: config::git_info(),
            name: global.get("user.name").cloned(),
            email: global.get("user.email").cloned(),
            ssh: config::ssh_source(),
            default_branch: global.get("init.defaultbranch").cloned(),
            home: home.display().to_string(),
            projects: projects.display().to_string(),
            gitignores: oxbow_core::setup::GITIGNORES
                .iter()
                .map(|(key, name, _)| ((*key).to_owned(), (*name).to_owned()))
                .collect(),
            licenses: oxbow_core::setup::LICENSES
                .iter()
                .map(|(key, name, _)| ((*key).to_owned(), (*name).to_owned()))
                .collect(),
        })
    })
    .await
}

/// Whether a folder is free to clone into: `missing`, `empty`, `files` or `file`.
#[tauri::command]
pub(crate) fn folder_state(path: String) -> &'static str {
    let path = Path::new(&path);
    if !path.exists() {
        "missing"
    } else if !path.is_dir() {
        "file"
    } else if std::fs::read_dir(path).is_ok_and(|mut entries| entries.next().is_none()) {
        "empty"
    } else {
        "files"
    }
}

#[tauri::command]
pub(crate) async fn probe_remote(
    app: tauri::AppHandle,
    accounts: State<'_, accounts::Accounts>,
    url: String,
) -> CommandResult<oxbow_core::RemoteProbe> {
    // Signed in, a private repository on github.com can count its commits too.
    let mut github = oxbow_core::github::Client::default();
    if let Some(token) = accounts.token(&app) {
        github = github.with_token(token);
    }
    logged(None, console::EntryKind::Read, "Check the remote", move || {
        oxbow_core::setup::probe_remote(&url, &github)
    })
    .await
}

#[tauri::command]
pub(crate) fn clone_command(options: oxbow_core::CloneOptions) -> CommandResult<oxbow_core::GitCommand> {
    oxbow_core::setup::clone_command(&options).map_err(|err| err.to_string())
}

/// Clone, sending each line git prints to the window as a `clone-line`. Stop (`stop_action`)
/// ends it and removes what it made.
#[tauri::command]
pub(crate) async fn clone_repo(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    options: oxbow_core::CloneOptions,
) -> CommandResult<String> {
    let cancel = session.cancel(&window);
    cancel.store(false, Ordering::Relaxed);
    let path = options.path.display().to_string();
    let title = format!("Clone {}", console::mask(&options.url));
    logged(
        Some(options.path.clone()),
        console::EntryKind::Action,
        title,
        move || {
            oxbow_core::setup::clone(
                &options,
                &mut |line| {
                    let _ = window.emit_to(window.label(), "clone-line", line);
                },
                &cancel,
            )
        },
    )
    .await?;
    // A clone brings neither hooks nor settings along: what it has, the user's templates gave it.
    trusted::add(&app, Path::new(&path), &[]);
    Ok(path)
}

#[tauri::command]
pub(crate) async fn plan_new_repo(options: oxbow_core::NewRepoOptions) -> CommandResult<oxbow_core::NewRepoPlan> {
    blocking(move || oxbow_core::setup::plan_new_repo(&options)).await
}

/// The repository New Repository made, and what the secrets check found that held back its
/// first commit.
#[derive(Serialize)]
pub(crate) struct CreatedRepo {
    path: String,
    secrets: Vec<oxbow_core::SecretFinding>,
}

#[tauri::command]
pub(crate) async fn create_repo(app: AppHandle, options: oxbow_core::NewRepoOptions) -> CommandResult<CreatedRepo> {
    let path = options.path.display().to_string();
    let secrets = logged(
        Some(options.path.clone()),
        console::EntryKind::Action,
        "Create a repository",
        move || oxbow_core::setup::create_repo(&options),
    )
    .await?;
    trusted::add(&app, Path::new(&path), &[]);
    Ok(CreatedRepo { path, secrets })
}
