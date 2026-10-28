//! Trusted repositories: folders whose own git settings may run programs, because the user said
//! so, or Oxbow cloned or created them. Kept in `trusted-repositories.json` beside the recent
//! list; the first time there is none, every recent repository counts as trusted, since it has
//! been opened before.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use oxbow_core::{Repo, TrustFinding};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedRepo {
    pub path: String,
    /// The settings that run programs it had when last opened: `core.hooksPath`, `hooks/pre-commit`.
    #[serde(default)]
    pub settings: Vec<String>,
}

/// One read-change-write at a time: windows open repositories at once.
static LOCK: Mutex<()> = Mutex::new(());

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join("trusted-repositories.json"))
}

fn read(app: &AppHandle) -> Vec<TrustedRepo> {
    let Some(file) = file(app) else { return Vec::new() };
    match std::fs::read_to_string(&file) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => {
            let seeded: Vec<TrustedRepo> = crate::recent::read_recent(app)
                .into_iter()
                .map(|r| TrustedRepo {
                    path: r.path,
                    settings: Vec::new(),
                })
                .collect();
            write(app, &seeded);
            seeded
        }
    }
}

fn write(app: &AppHandle, list: &[TrustedRepo]) {
    let Some(file) = file(app) else { return };
    if let Ok(text) = serde_json::to_string_pretty(list) {
        let _ = crate::settings::replace(&file, &(text + "\n"));
    }
}

fn same(a: &str, b: &Path) -> bool {
    let a = Path::new(a);
    a == b
        || a.canonicalize()
            .ok()
            .is_some_and(|a| b.canonicalize().is_ok_and(|b| a == b))
}

fn keys(found: &[TrustFinding]) -> Vec<String> {
    let mut keys: Vec<String> = found.iter().map(|f| f.key.clone()).collect();
    keys.dedup();
    keys
}

pub fn is_trusted(app: &AppHandle, path: &Path) -> bool {
    let _lock = LOCK.lock().expect("trusted lock");
    read(app).iter().any(|r| same(&r.path, path))
}

/// Trust the repository at `path`, which has the settings `found`.
pub fn add(app: &AppHandle, path: &Path, found: &[TrustFinding]) {
    let _lock = LOCK.lock().expect("trusted lock");
    let mut list = read(app);
    list.retain(|r| !same(&r.path, path));
    list.push(TrustedRepo {
        path: path.display().to_string(),
        settings: keys(found),
    });
    write(app, &list);
}

/// A trusted repository was opened with the settings `found`: Settings lists them.
pub fn note(app: &AppHandle, path: &Path, found: &[TrustFinding]) {
    let _lock = LOCK.lock().expect("trusted lock");
    let mut list = read(app);
    let keys = keys(found);
    let Some(entry) = list.iter_mut().find(|r| same(&r.path, path)) else {
        return;
    };
    if entry.settings != keys {
        entry.settings = keys;
        write(app, &list);
    }
}

/// The settings of `repo` that run programs, and whether git may run them: the user trusts it,
/// or the main worktree it belongs to, or doesn't want to be asked. A trusted one's are noted for
/// Settings.
pub fn check(app: &AppHandle, repo: &Repo) -> (Vec<TrustFinding>, bool) {
    let workdir = repo.workdir();
    let found = oxbow_core::trust::scan(workdir).unwrap_or_default();
    if found.is_empty() {
        return (found, true);
    }
    // A linked worktree shares the main one's settings, and its trust.
    let common = repo.common_dir();
    let main = common.parent().filter(|_| common.ends_with(".git"));
    if is_trusted(app, workdir) || main.is_some_and(|main| is_trusted(app, main)) {
        note(app, workdir, &found);
        return (found, true);
    }
    let asks = crate::preferences::read_settings(app)
        .get("oxbow.git.askTrust")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    (found, !asks)
}

#[tauri::command]
pub fn trusted_repos(app: AppHandle) -> Vec<TrustedRepo> {
    let _lock = LOCK.lock().expect("trusted lock");
    // Only those with settings that run programs: the others never ask anyway.
    read(&app).into_iter().filter(|r| !r.settings.is_empty()).collect()
}

/// Remove: the repository asks again the next time it opens.
#[tauri::command]
pub fn untrust_repo(app: AppHandle, path: String) {
    let _lock = LOCK.lock().expect("trusted lock");
    let mut list = read(&app);
    list.retain(|r| !same(&r.path, Path::new(&path)));
    write(&app, &list);
}
