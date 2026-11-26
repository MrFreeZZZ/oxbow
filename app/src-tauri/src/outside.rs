//! The repository's files outside Oxbow: in an editor, a terminal, Finder or Quick Look.

use std::path::{Path, PathBuf};

use oxbow_core::Repo;
use serde_json::Value;
use tauri::{AppHandle, State};

use crate::open_in::WorkingFileStatus;
use crate::preferences::read_settings;
use crate::session::{RepoRequest, Session, current};
use crate::{CommandResult, open_in};

/// The Git LFS download page, for installing it without Homebrew.
#[tauri::command]
pub(crate) fn open_lfs_download() -> CommandResult<()> {
    open_in::open_url("https://git-lfs.com")
}

const OXBOW_GITHUB: &str = "https://github.com/MrFreeZZZ/oxbow";

#[tauri::command]
pub(crate) fn open_oxbow_github() -> CommandResult<()> {
    open_in::open_url(OXBOW_GITHUB)
}

#[tauri::command]
pub(crate) async fn report_oxbow_bug(app: AppHandle) -> CommandResult<()> {
    let version = app.package_info().version.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        open_in::open_url(bug_report_url(&version, &report_platform()).as_str())
    })
    .await
    .map_err(|err| err.to_string())?
}

fn bug_report_url(version: &str, platform: &str) -> tauri::Url {
    let mut url = tauri::Url::parse(&format!("{OXBOW_GITHUB}/issues/new")).expect("Oxbow issue URL");
    let commit = env!("OXBOW_COMMIT");
    let channel = env!("OXBOW_BUILD_CHANNEL");
    url.query_pairs_mut().append_pair(
        "body",
        &format!("Version: {version}\nCommit: {commit}\nBuild channel: {channel}\nPlatform: {platform}"),
    );
    url
}

fn report_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        os => os,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        arch => arch,
    };
    match os_version() {
        Some(version) => format!("{os} {arch} {version}"),
        None => format!("{os} {arch}"),
    }
}

#[cfg(target_os = "macos")]
fn os_version() -> Option<String> {
    let read = |flag: &str| {
        let output = std::process::Command::new("/usr/bin/sw_vers").arg(flag).output().ok()?;
        let value = std::str::from_utf8(&output.stdout).ok()?.trim();
        (output.status.success() && !value.is_empty()).then(|| value.to_owned())
    };
    let version = read("-productVersion")?;
    Some(match read("-buildVersion") {
        Some(build) => format!("{version} ({build})"),
        None => version,
    })
}

#[cfg(not(any(target_os = "macos", windows)))]
fn os_version() -> Option<String> {
    let output = std::process::Command::new("/usr/bin/uname")
        .arg("-r")
        .output()
        .or_else(|_| std::process::Command::new("/bin/uname").arg("-r").output())
        .ok()?;
    let version = std::str::from_utf8(&output.stdout).ok()?.trim();
    (output.status.success() && !version.is_empty()).then(|| version.to_owned())
}

#[cfg(windows)]
fn os_version() -> Option<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    let key = RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion")
        .ok()?;
    let build: String = key.get_value("CurrentBuildNumber").ok()?;
    Some(match key.get_value::<u32, _>("UBR") {
        Ok(revision) => format!("{build}.{revision}"),
        Err(_) => build,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bug_report_prefills_one_body_parameter() {
        let url = bug_report_url("0.1.741", "macOS arm64 27.2.1 (26B131)&test");
        assert_eq!(url.host_str(), Some("github.com"));
        assert_eq!(url.path(), "/MrFreeZZZ/oxbow/issues/new");
        let expected = format!(
            "Version: 0.1.741\nCommit: {}\nBuild channel: {}\nPlatform: macOS arm64 27.2.1 (26B131)&test",
            env!("OXBOW_COMMIT"),
            env!("OXBOW_BUILD_CHANNEL")
        );
        assert!(url.query_pairs().eq([("body".into(), expected.into())]));
    }
}

/// Preview a file of the working copy: Quick Look on macOS, the default app elsewhere.
#[tauri::command]
pub(crate) fn quick_look(window: RepoRequest, session: State<'_, Session>, path: String) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    open_in::quick_look(&repo.workdir().join(Path::new(&path)))
}

/// Read the working copy status for each path, in the same order as the request.
#[tauri::command]
pub(crate) async fn working_file_statuses(
    window: RepoRequest,
    session: State<'_, Session>,
    paths: Vec<String>,
) -> CommandResult<Vec<WorkingFileStatus>> {
    let root = current(&session, &window)?.workdir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        paths
            .iter()
            .map(|path| match open_in::inspect_working_file(&root, path) {
                Ok((_, status)) => status,
                Err(reason) => WorkingFileStatus::Unavailable { reason },
            })
            .collect()
    })
    .await
    .map_err(|err| err.to_string())
}

/// Pick the app using the file on disk when clicked, even if it changed after the UI checked.
#[tauri::command]
pub(crate) async fn open_working_file(
    app: AppHandle,
    window: RepoRequest,
    session: State<'_, Session>,
    path: String,
    line: Option<u32>,
) -> CommandResult<()> {
    let root = current(&session, &window)?.workdir().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let (file, status) = open_in::inspect_working_file(&root, &path)?;
        if status == WorkingFileStatus::Binary {
            return open_in::open_default(&file, window);
        }
        let settings = read_settings(&app);
        let editor = settings
            .get("oxbow.openIn.editor")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .or_else(|| open_in::default_editor_id(&open_in::editors()))
            .ok_or("Choose an editor in Settings › External Tools.")?;
        let terminal = settings.get("oxbow.openIn.terminal").and_then(Value::as_str);
        open_in::open_file(editor, &file, line, terminal)
    })
    .await
    .map_err(|err| err.to_string())?
}

/// Show a file of the repository in Finder (Explorer, or the file manager on Linux).
#[tauri::command]
pub(crate) fn reveal_file(window: RepoRequest, session: State<'_, Session>, path: String) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    open_in::reveal(&repo.workdir().join(Path::new(&path)))
}

/// Open the repository folder in the terminal chosen in Settings; with `worktree`, that other
/// working folder of it.
#[tauri::command]
pub(crate) fn open_in_terminal(
    app: AppHandle,
    window: RepoRequest,
    session: State<'_, Session>,
    worktree: Option<String>,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    let dir = match worktree {
        Some(path) => worktree_folder(&repo, &path)?,
        None => repo.workdir().to_path_buf(),
    };
    let terminal = read_settings(&app)
        .get("oxbow.openIn.terminal")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| open_in::terminals().first().map(|app| app.id.to_owned()))
        .ok_or("No terminal found.")?;
    open_in::open_terminal(&terminal, &dir)
}

/// `path` when it is one of the repository's worktrees or cloned submodules; commands that open
/// things take no other.
pub(crate) fn worktree_folder(repo: &Repo, path: &str) -> CommandResult<PathBuf> {
    let known = repo.worktree_list().map_err(|err| err.to_string())?;
    // A submodule's path is relative to the repository's folder.
    let full = repo.workdir().join(path);
    let wanted = full.canonicalize().ok();
    let same = |folder: &Path| folder == full || wanted.is_some() && folder.canonicalize().ok() == wanted;
    if let Some(tree) = known.iter().filter(|t| !t.missing).find(|t| same(Path::new(&t.path))) {
        return Ok(PathBuf::from(&tree.path));
    }
    // A submodule's folder is a repository of its own, opened and shown like a worktree.
    repo.submodule_list()
        .unwrap_or_default()
        .iter()
        .filter(|s| s.cloned)
        .map(|s| repo.workdir().join(&s.path))
        .find(|folder| same(folder))
        .ok_or_else(|| format!("{path} isn’t a worktree or submodule of this repository any more"))
}

/// Show a worktree's folder in Finder (Explorer, or the file manager on Linux).
#[tauri::command]
pub(crate) fn reveal_worktree(window: RepoRequest, session: State<'_, Session>, path: String) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    open_in::reveal(&worktree_folder(&repo, &path)?)
}
