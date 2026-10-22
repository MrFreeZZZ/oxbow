// Drafts of the Conflicts screen: the sides picked and the text typed for each conflicted file,
// one file per working folder in the app's data folder, never in the repository. The screen
// writes them as it goes, so quitting Oxbow in the middle of a merge keeps them; they go when
// the operation is finished or aborted, or after 30 days untouched.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

/// How long a draft nobody came back to is kept.
const KEEP: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// The draft file of the working folder `worktree`, in `dir`.
pub fn file(dir: &Path, worktree: &Path) -> PathBuf {
    // FNV-1a: the same name on every run and every version of Rust.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in worktree.as_os_str().as_encoded_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    dir.join(format!("{hash:016x}.json"))
}

/// The draft of `worktree` saved in `file`; none while there is none, or when the file belongs
/// to another folder whose name came out the same.
pub fn read(file: &Path, worktree: &Path) -> Result<Option<Value>, String> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("Can’t read the saved draft: {err}.")),
    };
    let Ok(Value::Object(mut draft)) = serde_json::from_str::<Value>(&text) else {
        return Err("The saved draft isn’t one Oxbow can read.".into());
    };
    if draft.remove("worktree").as_ref().and_then(Value::as_str) != worktree.to_str() {
        return Ok(None);
    }
    Ok(Some(Value::Object(draft)))
}

/// Save `draft`, a JSON object, as the draft of `worktree` in `file`.
pub fn write(file: &Path, worktree: &Path, draft: Value) -> Result<(), String> {
    let Value::Object(mut draft) = draft else {
        return Err("A draft is a JSON object.".into());
    };
    let Some(folder) = worktree.to_str() else {
        return Err("The folder’s name isn’t text, so its draft can’t be saved.".into());
    };
    draft.insert("worktree".into(), folder.into());
    let text = serde_json::to_string(&draft).map_err(|err| err.to_string())?;
    crate::settings::replace(file, &text).map_err(|err| format!("Can’t save the draft: {err}."))?;
    if let Some(dir) = file.parent() {
        forget_old(dir, SystemTime::now());
    }
    Ok(())
}

/// The folder `old` was moved to `new`: its draft goes along, unless `new` has one of its own.
/// True when it went.
pub fn moved(dir: &Path, old: &Path, new: &Path) -> Result<bool, String> {
    let from = file(dir, old);
    let Some(draft) = read(&from, old)? else {
        return Ok(false);
    };
    let to = file(dir, new);
    if read(&to, new)?.is_some() {
        return Ok(false);
    }
    write(&to, new, draft)?;
    remove(&from)?;
    Ok(true)
}

/// Remove the draft in `file`, if there is one.
pub fn remove(file: &Path) -> Result<(), String> {
    match std::fs::remove_file(file) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(format!("Can’t remove the saved draft: {err}.")),
        _ => Ok(()),
    }
}

/// Remove the drafts in `dir` last saved more than 30 days before `now`.
fn forget_old(dir: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .is_ok_and(|saved| now.duration_since(saved).is_ok_and(|age| age > KEEP));
        if old && entry.path().extension().is_some_and(|ext| ext == "json") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oxbow-drafts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn each_folder_has_its_own_file() {
        let dir = Path::new("/drafts");
        assert_eq!(file(dir, Path::new("/a/repo")), file(dir, Path::new("/a/repo")));
        assert_ne!(file(dir, Path::new("/a/repo")), file(dir, Path::new("/a/repo-2")));
    }

    #[test]
    fn text_comes_back_exactly() {
        let dir = folder("exact");
        let worktree = Path::new("/work/repo");
        let at = file(&dir, worktree);
        assert_eq!(read(&at, worktree), Ok(None));
        // An empty file, Windows line ends, and a last line with no line break.
        let draft = json!({ "step": "merge:abc:def", "files": [
            { "path": "empty.txt", "text": { "text": "", "start": "" } },
            { "path": "crlf.txt", "text": { "text": "one\r\ntwo\r\n", "start": "" } },
            { "path": "open.txt", "text": { "text": "one\ntwo", "start": "one" } },
        ] });
        write(&at, worktree, draft.clone()).unwrap();
        assert_eq!(read(&at, worktree), Ok(Some(draft)));
        assert!(!at.with_extension("json.new").exists());
        remove(&at).unwrap();
        assert_eq!(read(&at, worktree), Ok(None));
        remove(&at).unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn another_folders_draft_is_not_taken() {
        let dir = folder("other");
        let at = file(&dir, Path::new("/work/one"));
        write(&at, Path::new("/work/one"), json!({ "step": "x" })).unwrap();
        assert_eq!(read(&at, Path::new("/work/two")), Ok(None));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_broken_draft_is_an_error_not_none() {
        let dir = folder("broken");
        std::fs::create_dir_all(&dir).unwrap();
        let at = file(&dir, Path::new("/work/repo"));
        std::fs::write(&at, "{ \"step\": ").unwrap();
        assert!(read(&at, Path::new("/work/repo")).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_draft_follows_its_folder_but_never_replaces_another() {
        let dir = folder("moved");
        let (old, new, other) = (Path::new("/work/old"), Path::new("/work/new"), Path::new("/work/other"));
        write(&file(&dir, old), old, json!({ "step": "a" })).unwrap();
        assert_eq!(moved(&dir, old, new), Ok(true));
        assert_eq!(read(&file(&dir, old), old), Ok(None));
        assert_eq!(read(&file(&dir, new), new), Ok(Some(json!({ "step": "a" }))));

        write(&file(&dir, other), other, json!({ "step": "b" })).unwrap();
        assert_eq!(moved(&dir, new, other), Ok(false));
        assert_eq!(read(&file(&dir, new), new), Ok(Some(json!({ "step": "a" }))));
        assert_eq!(read(&file(&dir, other), other), Ok(Some(json!({ "step": "b" }))));
    }

    #[test]
    fn drafts_untouched_for_30_days_go() {
        let dir = folder("old");
        let at = file(&dir, Path::new("/work/repo"));
        write(&at, Path::new("/work/repo"), json!({})).unwrap();
        forget_old(&dir, SystemTime::now() + Duration::from_secs(29 * 24 * 60 * 60));
        assert!(at.exists());
        forget_old(&dir, SystemTime::now() + Duration::from_secs(31 * 24 * 60 * 60));
        assert!(!at.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
