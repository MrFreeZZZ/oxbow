//! Trusted repositories: folders whose own git settings may run programs, because the user said
//! so, or Oxbow cloned or created them. Kept in `trusted-repositories.json` beside the recent
//! list; the first time there is none, every recent repository counts as trusted, since it has
//! been opened before.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use oxbow_core::{Repo, TrustFinding};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedRepo {
    pub path: String,
    /// The settings that run programs it had when last opened: `core.hooksPath`, `hooks/pre-commit`.
    #[serde(default)]
    pub settings: Vec<String>,
}

/// One read-change-write at a time: windows open repositories at once.
static LOCK: Mutex<()> = Mutex::new(());

fn file(app: &AppHandle) -> crate::CommandResult<PathBuf> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join("trusted-repositories.json"))
        .map_err(|err| err.to_string())
}

fn read_file(file: &Path, seed: impl FnOnce() -> Vec<TrustedRepo>) -> crate::CommandResult<Vec<TrustedRepo>> {
    match std::fs::read_to_string(file) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|err| format!("trusted-repositories.json isn't a list Oxbow can read: {err}.")),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let list = seed();
            write(file, &list)?;
            Ok(list)
        }
        Err(err) => Err(format!("Can't read trusted-repositories.json: {err}.")),
    }
}

fn seed(app: &AppHandle) -> Vec<TrustedRepo> {
    crate::recent::read_recent(app)
        .into_iter()
        .map(|r| TrustedRepo {
            path: r.path,
            settings: Vec::new(),
        })
        .collect()
}

fn try_read(app: &AppHandle) -> crate::CommandResult<Vec<TrustedRepo>> {
    read_file(&file(app)?, || seed(app))
}

fn read(app: &AppHandle) -> Vec<TrustedRepo> {
    try_read(app).unwrap_or_else(|err| {
        eprintln!("{err}");
        Vec::new()
    })
}

fn write(file: &Path, list: &[TrustedRepo]) -> crate::CommandResult<()> {
    let text = serde_json::to_string_pretty(list).map_err(|err| err.to_string())?;
    crate::settings::replace(file, &(text + "\n"))
        .map_err(|err| format!("Can't write trusted-repositories.json: {err}."))
}

fn update(
    file: &Path,
    seed: impl FnOnce() -> Vec<TrustedRepo>,
    change: impl FnOnce(&mut Vec<TrustedRepo>),
) -> crate::CommandResult<()> {
    let mut list = read_file(file, seed)?;
    let before = list.clone();
    change(&mut list);
    if list != before {
        write(file, &list)?;
    }
    Ok(())
}

fn update_trusted(app: &AppHandle, change: impl FnOnce(&mut Vec<TrustedRepo>)) -> crate::CommandResult<()> {
    let _lock = LOCK.lock().expect("trusted lock");
    update(&file(app)?, || seed(app), change)
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
pub fn add(app: &AppHandle, path: &Path, found: &[TrustFinding]) -> crate::CommandResult<()> {
    update_trusted(app, |list| {
        list.retain(|r| !same(&r.path, path));
        list.push(TrustedRepo {
            path: path.display().to_string(),
            settings: keys(found),
        });
    })
}

/// Record implicit trust after a completed operation, which still succeeded if this write fails.
pub fn remember(app: &AppHandle, path: &Path, found: &[TrustFinding]) {
    if let Err(err) = add(app, path, found) {
        eprintln!("{err}");
    }
}

/// A trusted repository was opened with the settings `found`: Settings lists them.
pub fn note(app: &AppHandle, path: &Path, found: &[TrustFinding]) {
    if let Err(err) = update_trusted(app, |list| {
        if let Some(entry) = list.iter_mut().find(|r| same(&r.path, path)) {
            entry.settings = keys(found);
        }
    }) {
        eprintln!("{err}");
    }
}

/// The settings of `repo` that run programs, and whether git may run them: the user trusts it,
/// or the main worktree it belongs to, or doesn't want to be asked. A trusted one's are noted for
/// Settings.
pub fn check(app: &AppHandle, repo: &Repo) -> crate::CommandResult<(Vec<TrustFinding>, bool)> {
    let workdir = repo.workdir();
    let found = oxbow_core::trust::scan(workdir).map_err(|err| err.to_string())?;
    if found.is_empty() {
        return Ok((found, true));
    }
    // A linked worktree shares the main one's settings, and its trust.
    let common = repo.common_dir();
    let main = common.parent().filter(|_| common.ends_with(".git"));
    if is_trusted(app, workdir) || main.is_some_and(|main| is_trusted(app, main)) {
        note(app, workdir, &found);
        return Ok((found, true));
    }
    let asks = crate::preferences::read_settings(app)
        .get("oxbow.git.askTrust")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    Ok((found, !asks))
}

#[tauri::command]
pub fn trusted_repos(app: AppHandle) -> Vec<TrustedRepo> {
    let _lock = LOCK.lock().expect("trusted lock");
    // Only those with settings that run programs: the others never ask anyway.
    read(&app).into_iter().filter(|r| !r.settings.is_empty()).collect()
}

/// Remove: the repository asks again the next time it opens.
#[tauri::command]
pub fn untrust_repo(app: AppHandle, path: String) -> crate::CommandResult<()> {
    update_trusted(&app, |list| list.retain(|r| !same(&r.path, Path::new(&path))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> Vec<TrustedRepo> {
        vec![TrustedRepo {
            path: "/legacy/repo".into(),
            settings: Vec::new(),
        }]
    }

    #[test]
    fn failed_trust_write_leaves_repository_untrusted_on_reopen() {
        let dir = std::env::temp_dir().join(format!("oxbow-trusted-add-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("trusted-repositories.json");
        let next = dir.join("trusted-repositories.json.new");
        write(&file, &[]).unwrap();
        std::fs::create_dir(&next).unwrap();

        let entry = TrustedRepo {
            path: "/external/repo".into(),
            settings: vec!["core.hooksPath".into()],
        };
        let add = |list: &mut Vec<TrustedRepo>| list.push(entry.clone());
        assert!(update(&file, seed, add).unwrap_err().starts_with("Can't write"));
        assert!(read_file(&file, seed).unwrap().is_empty());

        std::fs::remove_dir(&next).unwrap();
        update(&file, seed, add).unwrap();
        assert_eq!(read_file(&file, seed).unwrap(), vec![entry]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn failed_remove_write_leaves_repository_trusted_on_reopen() {
        let dir = std::env::temp_dir().join(format!("oxbow-trusted-remove-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("trusted-repositories.json");
        let next = dir.join("trusted-repositories.json.new");
        let entries = seed();
        write(&file, &entries).unwrap();
        std::fs::create_dir(&next).unwrap();

        assert!(update(&file, seed, Vec::clear).unwrap_err().starts_with("Can't write"));
        assert_eq!(read_file(&file, seed).unwrap(), entries);

        std::fs::remove_dir(&next).unwrap();
        update(&file, seed, Vec::clear).unwrap();
        assert!(read_file(&file, seed).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_an_absent_list_is_seeded_from_recent_repositories() {
        let dir = std::env::temp_dir().join(format!("oxbow-trusted-seed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("trusted-repositories.json");
        let _ = std::fs::remove_file(&file);
        assert_eq!(read_file(&file, seed).unwrap()[0].path, "/legacy/repo");
        assert_eq!(
            read_file(&file, || panic!("seeded twice")).unwrap()[0].path,
            "/legacy/repo"
        );
        update(&file, seed, Vec::clear).unwrap();
        assert!(read_file(&file, || panic!("seeded an empty list")).unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn broken_and_unreadable_lists_are_never_seeded_or_replaced() {
        let dir = std::env::temp_dir().join(format!("oxbow-trusted-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("trusted-repositories.json");
        for broken in ["", "[", "{}", "[{\"settings\":[]}]"] {
            std::fs::write(&file, broken).unwrap();
            assert!(read_file(&file, || panic!("seeded a broken list")).is_err());
            assert!(update(&file, seed, |_| panic!("changed a broken list")).is_err());
            assert_eq!(std::fs::read_to_string(&file).unwrap(), broken);
        }
        std::fs::write(&file, [0xff]).unwrap();
        assert!(update(&file, seed, |_| panic!("changed invalid UTF-8")).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), [0xff]);
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        assert!(
            update(
                &file,
                || panic!("seeded an unreadable list"),
                |_| panic!("changed an unreadable list")
            )
            .is_err()
        );
        assert!(file.is_dir());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
