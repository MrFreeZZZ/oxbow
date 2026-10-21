// settings.json: the user's settings as flat `oxbow.*` keys, holding only values that differ
// from the defaults. A file Oxbow can't read is never written over, so saving one setting can't
// wipe out the others, and a write goes through a file beside it, so it is never left half done.

use std::path::Path;

use serde_json::{Map, Value};

pub type Settings = Map<String, Value>;

/// The settings in `file`; none while there is no file.
pub fn read(file: &Path) -> Result<Settings, String> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::new()),
        Err(err) => return Err(format!("Can’t read settings.json: {err}.")),
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
    let failed = |err: std::io::Error| format!("Can’t write settings.json: {err}.");
    std::fs::create_dir_all(file.parent().expect("settings file has a parent")).map_err(failed)?;
    let next = file.with_extension("json.new");
    if let Err(err) = std::fs::write(&next, text) {
        let _ = std::fs::remove_file(&next);
        return Err(failed(err));
    }
    // Replaces `file` on Windows too.
    std::fs::rename(&next, file).map_err(|err| {
        let _ = std::fs::remove_file(&next);
        failed(err)
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
