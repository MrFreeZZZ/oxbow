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

#[tauri::command]
pub(crate) async fn background_image(app: AppHandle) -> CommandResult<tauri::ipc::Response> {
    let settings = read_settings(&app);
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
        .ok_or("Choose a background image in Settings › Appearance.")?
        .to_owned();
    tauri::async_runtime::spawn_blocking(move || read_image(Path::new(&path)))
        .await
        .map_err(|err| format!("Can’t read the background image: {err}."))?
        .map(tauri::ipc::Response::new)
}

#[cfg(test)]
mod tests {
    use super::*;

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
