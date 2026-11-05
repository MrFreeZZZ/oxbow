//! Only the image explicitly selected in settings.json may be read as a window background.

use std::io::Read;
use std::path::Path;

use tauri::AppHandle;

use crate::{CommandResult, preferences::read_settings};

const MAX_BYTES: u64 = 32 * 1024 * 1024;

fn read_image(path: &Path) -> CommandResult<Vec<u8>> {
    if !path.is_absolute() {
        return Err("Choose an image using its full path.".into());
    }
    // Reject special files before open: opening a FIFO can wait indefinitely. The second
    // check below validates the actual handle in case the path was replaced in between.
    if !std::fs::metadata(path)
        .map_err(|err| format!("Can’t open the background image: {err}."))?
        .is_file()
    {
        return Err("The background image must be a regular file.".into());
    }
    let file = std::fs::File::open(path).map_err(|err| format!("Can’t open the background image: {err}."))?;
    let metadata = file
        .metadata()
        .map_err(|err| format!("Can’t read the background image: {err}."))?;
    if !metadata.is_file() {
        return Err("The background image must be a regular file.".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("Choose an image smaller than 32 MiB.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| format!("Can’t read the background image: {err}."))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Choose an image smaller than 32 MiB.".into());
    }
    let png = bytes.starts_with(b"\x89PNG\r\n\x1a\n");
    let jpeg = bytes.starts_with(b"\xff\xd8\xff");
    let webp = bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP");
    if !png && !jpeg && !webp {
        return Err("Choose a JPEG, PNG or WebP image.".into());
    }
    Ok(bytes)
}

fn selected_path(app: &AppHandle) -> CommandResult<std::path::PathBuf> {
    let settings = read_settings(app);
    if settings
        .get("oxbow.background.source")
        .and_then(serde_json::Value::as_str)
        != Some("custom")
    {
        return Err("No custom background image is selected.".into());
    }
    let path = settings
        .get("oxbow.background.image")
        .and_then(serde_json::Value::as_str)
        .filter(|path| !path.is_empty())
        .ok_or("Choose a background image in Settings › Appearance.")?;
    Ok(path.into())
}

fn image_version(path: &Path) -> CommandResult<String> {
    if !path.is_absolute() {
        return Err("Choose an image using its full path.".into());
    }
    let metadata = std::fs::metadata(path).map_err(|err| format!("Can’t open the background image: {err}."))?;
    if !metadata.is_file() {
        return Err("The background image must be a regular file.".into());
    }
    if metadata.len() > MAX_BYTES {
        return Err("Choose an image smaller than 32 MiB.".into());
    }
    let modified = metadata
        .modified()
        .map_err(|err| format!("Can’t read the background image: {err}."))?;
    // Include identity and change time on Unix: replacements can preserve size and mtime.
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        format!(
            "{}:{}:{}:{}",
            metadata.dev(),
            metadata.ino(),
            metadata.ctime(),
            metadata.ctime_nsec()
        )
    };
    #[cfg(not(unix))]
    let identity = format!("{:?}", metadata.created().ok());
    Ok(format!("{}:{modified:?}:{identity}", metadata.len()))
}

/// A focus refresh only stats the configured file; unchanged files need no IPC bytes or decode.
#[tauri::command]
pub(crate) async fn background_image_version(app: AppHandle) -> CommandResult<String> {
    let path = selected_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || image_version(&path))
        .await
        .map_err(|err| format!("Can’t read the background image: {err}."))?
}

#[tauri::command]
pub(crate) async fn background_image(app: AppHandle) -> CommandResult<tauri::ipc::Response> {
    let path = selected_path(&app)?;
    tauri::async_runtime::spawn_blocking(move || read_image(&path))
        .await
        .map_err(|err| format!("Can’t read the background image: {err}."))?
        .map(tauri::ipc::Response::new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_tracks_replacements_and_rejects_special_files() {
        let folder = std::env::temp_dir().join(format!("oxbow-background-version-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("photo.png");
        std::fs::write(&path, b"first").unwrap();
        let version = image_version(&path).unwrap();
        assert_eq!(image_version(&path).unwrap(), version);
        let replacement = folder.join("replacement.png");
        std::fs::write(&replacement, b"other").unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        assert_ne!(image_version(&path).unwrap(), version);
        assert!(image_version(&folder).is_err());
        assert!(image_version(Path::new("relative.png")).is_err());
        #[cfg(unix)]
        {
            let fifo = folder.join("pipe.png");
            assert!(
                std::process::Command::new("mkfifo")
                    .arg(&fifo)
                    .status()
                    .unwrap()
                    .success()
            );
            assert!(image_version(&fifo).unwrap_err().contains("regular file"));
            assert!(read_image(&fifo).unwrap_err().contains("regular file"));
        }
        std::fs::remove_file(&path).unwrap();
        assert!(image_version(&path).is_err());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn rejects_non_images_and_large_files_without_reading_them() {
        let folder = std::env::temp_dir().join(format!("oxbow-background-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join("image.jpg");
        std::fs::write(&path, b"This is not an image").unwrap();
        assert!(read_image(&path).unwrap_err().contains("JPEG, PNG or WebP"));
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_BYTES + 1).unwrap();
        assert!(read_image(&path).unwrap_err().contains("32 MiB"));
        assert!(read_image(&folder).is_err());
        assert!(read_image(&folder.join("missing.png")).is_err());
        assert!(read_image(Path::new("relative.png")).is_err());
        std::fs::remove_dir_all(folder).unwrap();
    }
}
