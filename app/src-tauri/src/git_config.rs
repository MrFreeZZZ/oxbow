//! Settings › Git: git's own settings, the global ones and the repository's.

use oxbow_core::config::{self, ConfigScope};
use oxbow_core::console;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::session::{Session, at, current, logged, read};
use crate::{CommandResult, open_in};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct GitSettings {
    git: oxbow_core::GitInfo,
    /// Everything in ~/.gitconfig.
    global: std::collections::BTreeMap<String, String>,
    ssh_keys: Vec<oxbow_core::SshKey>,
    editors: Vec<open_in::App>,
    terminals: Vec<open_in::App>,
}

/// What Settings shows from Git's own config and the computer.
#[tauri::command]
pub(crate) async fn git_settings() -> CommandResult<GitSettings> {
    logged(None, console::EntryKind::Read, "Read the Git setup", || {
        Ok(GitSettings {
            git: config::git_info(),
            global: config::global_config(),
            ssh_keys: config::ssh_keys(),
            editors: open_in::editors(),
            terminals: open_in::terminals(),
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepoSettings {
    name: String,
    path: String,
    /// Everything in the repository's .git/config.
    local: std::collections::BTreeMap<String, String>,
    remotes: Vec<oxbow_core::RemoteInfo>,
    storage: Option<oxbow_core::Storage>,
    production: Production,
    /// The checks before commit, in the order they run.
    checks: Vec<oxbow_core::CommitCheck>,
    /// Restricted: the repository's own programs, the checks too, don't run.
    restricted: bool,
}

/// Settings › This Repository › Production branch.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Production {
    /// The chosen branch as a full ref; `None` is Auto.
    chosen: Option<String>,
    /// Whether the chosen branch exists.
    found: bool,
    /// Full ref of the branch Auto picks, if any.
    auto: Option<String>,
    /// Local and remote branches to choose from, as full refs.
    branches: Vec<String>,
}

/// Settings › This Repository, or nothing when no repository is open.
#[tauri::command]
pub(crate) async fn repo_settings(
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
) -> CommandResult<Option<RepoSettings>> {
    let Ok(repo) = current(&session, &window) else {
        return Ok(None);
    };
    read(at(&repo), "Read repository settings", move || {
        Ok(Some(RepoSettings {
            name: repo.name(),
            path: repo.workdir().display().to_string(),
            local: repo.local_config(),
            remotes: repo.remotes_info()?,
            storage: repo.storage().ok(),
            checks: repo.commit_checks()?,
            restricted: oxbow_core::trust::is_restricted(repo.workdir()),
            production: {
                let refs = repo.refs()?;
                let chosen = repo.production_setting();
                Production {
                    found: chosen
                        .as_deref()
                        .is_some_and(|c| oxbow_core::history::pick_trunk(&refs, Some(c)).is_some()),
                    auto: oxbow_core::history::pick_trunk(&refs, None).and_then(oxbow_core::history::full_ref),
                    branches: refs
                        .iter()
                        .filter_map(oxbow_core::history::full_ref)
                        .filter(|r| !r.ends_with("/HEAD"))
                        .collect(),
                    chosen,
                }
            },
        }))
    })
    .await
}

/// Change a value in ~/.gitconfig or the open repository's .git/config; `None` removes it.
#[tauri::command]
pub(crate) async fn set_git_config(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    scope: ConfigScope,
    key: String,
    value: Option<String>,
) -> CommandResult<()> {
    let repo = match scope {
        ConfigScope::Local => Some(current(&session, &window)?),
        ConfigScope::Global => None,
    };
    // The production branch changes how the main window draws the graph.
    let redraw = key.eq_ignore_ascii_case(oxbow_core::history::PRODUCTION_KEY);
    let dir = repo.as_ref().map(at);
    let title = match value {
        Some(_) => format!("Set {key}"),
        None => format!("Unset {key}"),
    };
    logged(dir, console::EntryKind::Action, title, move || match repo {
        Some(repo) => repo.set_local_config(&key, value.as_deref()),
        None => config::set_global_config(&key, value.as_deref()),
    })
    .await?;
    if redraw {
        let _ = app.emit_to(session.label(&window).as_str(), "repo-touched", ());
    }
    Ok(())
}

/// Keep `checks` as the open repository's checks before commit. The user wrote them, so when
/// the repository isn't restricted it is trusted too: it won't ask about them when it opens.
#[tauri::command]
pub(crate) async fn set_commit_checks(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    checks: Vec<oxbow_core::CommitCheck>,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    logged(
        Some(at(&repo)),
        console::EntryKind::Action,
        "Set the checks before commit",
        move || {
            repo.set_commit_checks(&checks)?;
            let workdir = repo.workdir();
            if !oxbow_core::trust::is_restricted(workdir) && checks.iter().any(|c| c.on) {
                crate::trusted::add(&app, workdir, &oxbow_core::trust::scan(workdir).unwrap_or_default());
            }
            Ok(())
        },
    )
    .await
}
