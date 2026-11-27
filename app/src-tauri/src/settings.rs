// settings.json: the user's settings as flat `oxbow.*` keys, holding only values that differ
// from the defaults. A file Oxbow can't read is never written over, so saving one setting can't
// wipe out the others, and a write goes through a file beside it, so it is never left half done.

use std::path::Path;

use serde_json::{Map, Value};

pub type Settings = Map<String, Value>;

/// The exact settings text, including whether the file exists.
pub fn read_text(file: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(file) {
        Ok(text) => Ok(Some(text)),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(format!("Can’t read settings.json: {err}.")),
    }
}

pub fn check_text_base(file: &Path, base: Option<&str>) -> Result<(), String> {
    if read_text(file)?.as_deref() != base {
        return Err(
            "settings.json changed on disk. Copy your edits, then reopen the editor to load the latest version.".into(),
        );
    }
    Ok(())
}

pub fn write_edited_text(file: &Path, base: Option<&str>, text: &mut String) -> Result<(), String> {
    check_text_base(file, base)?;
    if !text.ends_with('\n') {
        text.push('\n');
    }
    write_text(file, text)
}

/// The settings in `file`; none while there is no file.
pub fn read(file: &Path) -> Result<Settings, String> {
    let Some(text) = read_text(file)? else {
        return Ok(Settings::new());
    };
    if text.trim().is_empty() {
        return Ok(Settings::new());
    }
    serde_json::from_str(&text).map_err(|err| format!("settings.json isn’t a JSON object Oxbow can read: {err}."))
}

/// Save `settings` as `file`.
pub fn write(file: &Path, settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|err| err.to_string())?;
    write_text(file, &(text + "\n"))
}

/// Save `text` as `file`: written next to it first, then moved over it.
pub fn write_text(file: &Path, text: &str) -> Result<(), String> {
    replace(file, text).map_err(|err| format!("Can’t write settings.json: {err}."))
}

/// Write `text` next to `file`, then move it over `file`, so `file` is never left half written.
pub fn replace(file: &Path, text: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(file.parent().expect("file has a parent"))?;
    let mut next = file.as_os_str().to_owned();
    next.push(".new");
    let next = std::path::PathBuf::from(next);
    if let Err(err) = std::fs::write(&next, text) {
        let _ = std::fs::remove_file(&next);
        return Err(err);
    }
    // Replaces `file` on Windows too.
    std::fs::rename(&next, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&next);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("oxbow-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn no_file_is_no_settings() {
        let file = folder("none").join("settings.json");
        assert_eq!(read(&file), Ok(Settings::new()));
    }

    #[test]
    fn written_settings_read_back() {
        let dir = folder("write");
        let file = dir.join("settings.json");
        let mut settings = Settings::new();
        settings.insert("oxbow.diff.wrap".into(), json!(false));
        write(&file, &settings).unwrap();
        settings.insert("oxbow.text.fontSize".into(), json!(14));
        write(&file, &settings).unwrap();
        assert_eq!(read(&file), Ok(settings));
        assert!(!dir.join("settings.json.new").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn text_save_rejects_a_snapshot_before_a_tag_assignment() {
        let dir = folder("stale-text");
        let file = dir.join("settings.json");
        write_text(&file, "{\r\n  \"oxbow.diff.wrap\": false\r\n}\r\n").unwrap();
        let base = read_text(&file).unwrap().unwrap();
        assert!(check_text_base(&file, Some(&base)).is_ok());
        assert!(check_text_base(&file, Some(&base.replace("\r\n", "\n"))).is_err());

        let mut latest = read(&file).unwrap();
        latest.insert("oxbow.repositoryTabTags.assignments".into(), json!({ "/repo": "red" }));
        write(&file, &latest).unwrap();
        let fresh = read_text(&file).unwrap();
        let stale_save = write_edited_text(&file, Some(&base), &mut "{}".to_owned());
        assert!(stale_save.unwrap_err().contains("changed on disk"));
        assert_eq!(read_text(&file).unwrap(), fresh);
        assert_eq!(read(&file).unwrap(), latest);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn text_save_distinguishes_absence_and_read_errors_from_contents() {
        let dir = folder("text-read");
        let file = dir.join("settings.json");
        assert_eq!(read_text(&file), Ok(None));
        assert!(check_text_base(&file, None).is_ok());
        let mut saved = "{}".to_owned();
        write_edited_text(&file, None, &mut saved).unwrap();
        assert_eq!(saved, "{}\n");
        assert!(check_text_base(&file, None).is_err());
        assert!(check_text_base(&file, Some("{}")).is_err());
        let mut next = "{ \"oxbow.diff.wrap\": true }\n".to_owned();
        write_edited_text(&file, Some(&saved), &mut next).unwrap();
        assert_eq!(read_text(&file).unwrap().as_deref(), Some(next.as_str()));
        std::fs::write(&file, [0xff]).unwrap();
        assert!(
            write_edited_text(&file, Some(&saved), &mut next)
                .unwrap_err()
                .starts_with("Can’t read settings.json")
        );
        assert_eq!(std::fs::read(&file).unwrap(), [0xff]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_broken_file_is_an_error_not_empty() {
        let dir = folder("broken");
        let file = dir.join("settings.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&file, "{ \"oxbow.diff.wrap\": false,, }").unwrap();
        assert!(
            read(&file)
                .unwrap_err()
                .starts_with("settings.json isn’t a JSON object")
        );
        std::fs::write(&file, "[]").unwrap();
        assert!(read(&file).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_folder_that_refuses_writes_is_an_error() {
        use std::os::unix::fs::PermissionsExt;
        let dir = folder("locked");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = write(&dir.join("settings.json"), &Settings::new());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Root may write anyway.
        if let Err(err) = result {
            assert!(err.starts_with("Can’t write settings.json"));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
