//! settings.json's keys: reading them, changing one, applying them to the app, and the
//! Settings window, with the file edited by hand and its backups.

use std::path::{Path, PathBuf};

use oxbow_core::DiffOptions;
use oxbow_core::config;
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State, Theme, WebviewUrl, WebviewWindowBuilder};

use crate::session::Session;
use crate::{CommandResult, open_in, settings};

/// User settings live in `settings.json` in the app's config folder, as flat `oxbow.*` keys,
/// holding only values that differ from the defaults.
fn settings_file(app: &AppHandle) -> CommandResult<PathBuf> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join("settings.json"))
        .map_err(|err| err.to_string())
}

/// The settings, or none when settings.json can't be read.
pub(crate) fn read_settings(app: &AppHandle) -> serde_json::Map<String, serde_json::Value> {
    settings_file(app)
        .and_then(|file| settings::read(&file))
        .unwrap_or_default()
}

#[tauri::command]
pub(crate) fn get_setting(app: AppHandle, key: String) -> Option<Value> {
    read_settings(&app).remove(&key)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AllSettings {
    /// The number of the last change in them, as `settings-changed` sends it.
    revision: u64,
    /// Every setting that differs from its default.
    values: serde_json::Map<String, Value>,
    /// Why settings.json can't be read; the defaults are used meanwhile.
    problem: Option<String>,
}

#[tauri::command]
pub(crate) fn all_settings(app: AppHandle, session: State<'_, Session>) -> AllSettings {
    let revision = session.settings.lock().expect("session lock");
    let (values, problem) = match settings_file(&app).and_then(|file| settings::read(&file)) {
        Ok(values) => (values, None),
        Err(problem) => (serde_json::Map::new(), Some(problem)),
    };
    AllSettings {
        revision: *revision,
        values,
        problem,
    }
}

/// The system's Accessibility › Display switches the page can't read itself: WebKit has no media
/// query for Reduce Transparency, and Increase Contrast is read here too so both come from one place.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DisplayOptions {
    reduce_transparency: bool,
    increase_contrast: bool,
}

#[tauri::command]
pub(crate) fn display_options() -> DisplayOptions {
    #[cfg(target_os = "macos")]
    {
        let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
        DisplayOptions {
            reduce_transparency: workspace.accessibilityDisplayShouldReduceTransparency(),
            increase_contrast: workspace.accessibilityDisplayShouldIncreaseContrast(),
        }
    }
    #[cfg(not(target_os = "macos"))]
    DisplayOptions {
        reduce_transparency: false,
        increase_contrast: false,
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct SettingChanged {
    key: String,
    value: Value,
    revision: u64,
}

/// Save a setting (`null` puts it back to its default) and tell every window. Returns the
/// number of the change. Nothing is written when settings.json can't be read, so the other
/// settings in it stay.
#[tauri::command]
pub(crate) fn set_setting(
    app: AppHandle,
    session: State<'_, Session>,
    key: String,
    value: Value,
) -> CommandResult<u64> {
    let mut revision = session.settings.lock().expect("session lock");
    let file = settings_file(&app)?;
    let mut settings = settings::read(&file).map_err(|problem| {
        format!("{problem} Oxbow doesn’t write over it until it is fixed in Edit in settings.json.")
    })?;
    if value.is_null() {
        settings.remove(&key);
    } else {
        settings.insert(key.clone(), value.clone());
    }
    settings::write(&file, &settings)?;
    *revision += 1;
    apply_settings(&app, &session, &settings);
    // A window that misses this keeps showing the old value until it reloads; not an error.
    let _ = app.emit(
        "settings-changed",
        SettingChanged {
            key,
            value,
            revision: *revision,
        },
    );
    Ok(*revision)
}

/// The settings the backend itself follows: how diffs are made and the windows' appearance.
pub(crate) fn apply_settings(app: &AppHandle, session: &Session, settings: &serde_json::Map<String, Value>) {
    let defaults = DiffOptions::default();
    *session.diff.lock().expect("session lock") = DiffOptions {
        context_lines: settings
            .get("oxbow.diff.contextLines")
            .and_then(Value::as_u64)
            .map_or(defaults.context_lines, |n| n.min(100) as u32),
        ignore_whitespace: settings
            .get("oxbow.diff.ignoreWhitespace")
            .and_then(Value::as_bool)
            .unwrap_or(defaults.ignore_whitespace),
        max_lines: settings
            .get("oxbow.diff.maxLines")
            .and_then(Value::as_u64)
            .map_or(defaults.max_lines, |n| n.min(10_000_000) as u32),
    };
    config::set_git_program(
        settings
            .get("oxbow.git.path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from),
    );
    config::set_english_output(
        settings
            .get("oxbow.git.englishOutput")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    );
    config::set_run_hooks(
        settings
            .get("oxbow.git.runHooks")
            .and_then(Value::as_bool)
            .unwrap_or(true),
    );
    oxbow_core::set_secrets_check(
        settings
            .get("oxbow.commit.checkSecrets")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        &settings
            .get("oxbow.secrets.patterns")
            .and_then(|list| serde_json::from_value::<Vec<oxbow_core::SecretPattern>>(list.clone()).ok())
            .unwrap_or_default(),
    );
    oxbow_core::oplog::set_keep_days(
        settings
            .get("oxbow.undo.keepDays")
            .and_then(Value::as_u64)
            .map_or(30, |days| days.min(3650) as u32),
    );
    oxbow_core::localhistory::set_keep_days(
        settings
            .get("oxbow.localHistory.keepDays")
            .and_then(Value::as_u64)
            .map_or(oxbow_core::localhistory::DEFAULT_DAYS, |days| days.min(30) as u32),
    );
    let theme = appearance(settings);
    for window in app.webview_windows().values() {
        let _ = window.set_theme(theme);
    }
}

/// `oxbow.appearance`: light, dark, or `None` to follow the system.
pub(crate) fn appearance(settings: &serde_json::Map<String, Value>) -> Option<Theme> {
    match settings.get("oxbow.appearance").and_then(Value::as_str) {
        Some("light") => Some(Theme::Light),
        Some("dark") => Some(Theme::Dark),
        _ => None,
    }
}

/// Open the Settings window. Async, because making a window in a synchronous command
/// deadlocks on Windows.
#[tauri::command]
pub(crate) async fn open_settings(app: AppHandle) -> CommandResult<()> {
    show_settings(&app)
}

/// Open the Settings window, or bring it to the front.
pub(crate) fn show_settings(app: &AppHandle) -> CommandResult<()> {
    if let Some(window) = app.get_webview_window("settings") {
        return window.set_focus().map_err(|err| err.to_string());
    }
    let builder = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html#settings".into()))
        .title("Settings")
        .inner_size(940.0, 720.0)
        .min_inner_size(760.0, 520.0)
        .theme(appearance(&read_settings(app)));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(22.0, 24.0));
    builder.build().map(|_| ()).map_err(|err| err.to_string())
}

/// The key the app menu's Settings… shows and answers to: the one Settings › Keyboard Shortcuts
/// gives Open Settings, or none. Only the Mac has the menu.
#[tauri::command]
pub(crate) async fn set_settings_key(app: AppHandle, accelerator: Option<String>) -> CommandResult<()> {
    #[cfg(target_os = "macos")]
    {
        let item = app
            .menu()
            .and_then(|menu| menu.items().ok()?.first()?.as_submenu()?.get("settings"));
        if let Some(item) = item.as_ref().and_then(|item| item.as_menuitem()) {
            item.set_accelerator(accelerator).map_err(|err| err.to_string())?;
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, accelerator);
    Ok(())
}

/// The monospaced font families installed on this computer, sorted, for Settings › Diff & Text.
#[tauri::command]
pub(crate) async fn monospace_fonts() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(|| {
        let mut fonts = fontdb::Database::new();
        fonts.load_system_fonts();
        let mut families: Vec<String> = fonts
            .faces()
            .filter(|face| face.monospaced)
            .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
            // Hidden system fonts (".SF NS Mono") are not meant to be picked by name, and emoji
            // fonts call themselves monospaced.
            .filter(|name| !name.starts_with('.') && !name.contains("Emoji"))
            .collect();
        families.sort_by_key(|name| name.to_lowercase());
        families.dedup();
        families
    })
    .await
    .unwrap_or_default()
}

/// settings.json as it is on disk, for editing it as text.
#[tauri::command]
pub(crate) fn settings_text(app: AppHandle) -> CommandResult<String> {
    let file = settings_file(&app)?;
    Ok(std::fs::read_to_string(file).unwrap_or_else(|_| "{}\n".to_owned()))
}

/// Save settings.json edited as text. It must be a JSON object; every window takes the new
/// values at once.
#[tauri::command]
pub(crate) fn save_settings_text(app: AppHandle, session: State<'_, Session>, text: String) -> CommandResult<()> {
    let settings: serde_json::Map<String, Value> = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    let mut revision = session.settings.lock().expect("session lock");
    let before = read_settings(&app);
    let file = settings_file(&app)?;
    settings::write_text(&file, &if text.ends_with('\n') { text } else { text + "\n" })?;
    settings_replaced(&app, &session, &mut revision, &before, &settings);
    Ok(())
}

/// Follow settings that replaced `before` all at once, and tell every window which keys changed.
/// `revision` is the held number of the last change.
fn settings_replaced(
    app: &AppHandle,
    session: &Session,
    revision: &mut u64,
    before: &serde_json::Map<String, Value>,
    after: &serde_json::Map<String, Value>,
) {
    *revision += 1;
    apply_settings(app, session, after);
    let removed = before
        .keys()
        .filter(|key| !after.contains_key(*key))
        .map(|key| (key.clone(), Value::Null));
    for (key, value) in after.clone().into_iter().chain(removed) {
        if before.get(&key) != Some(&value) {
            let _ = app.emit(
                "settings-changed",
                SettingChanged {
                    key,
                    value,
                    revision: *revision,
                },
            );
        }
    }
}

/// Where the old settings go when they are put back to the defaults.
fn settings_backup(file: &Path) -> PathBuf {
    file.with_file_name("settings.backup.json")
}

/// Rename `from` to `to`, replacing `to` (which a rename on Windows won't do by itself).
fn move_file(from: &Path, to: &Path) -> CommandResult<()> {
    #[cfg(windows)]
    let _ = std::fs::remove_file(to);
    std::fs::rename(from, to).map_err(|err| err.to_string())
}

#[derive(Serialize)]
pub(crate) struct SettingsLocation {
    /// settings.json and its backup as typed in a shell, with `~` for the home folder.
    file: String,
    backup: String,
}

#[tauri::command]
pub(crate) fn settings_location(app: AppHandle) -> CommandResult<SettingsLocation> {
    let file = settings_file(&app)?;
    let home = app.path().home_dir().ok();
    let shown = |path: &Path| match home.as_deref().and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    };
    Ok(SettingsLocation {
        file: shown(&file),
        backup: shown(&settings_backup(&file)),
    })
}

/// Put every Oxbow setting back to its default. settings.json moves aside to
/// settings.backup.json, so the old values can come back; Git's own settings are not touched.
#[tauri::command]
pub(crate) fn restore_default_settings(app: AppHandle, session: State<'_, Session>) -> CommandResult<()> {
    let mut revision = session.settings.lock().expect("session lock");
    let before = read_settings(&app);
    let file = settings_file(&app)?;
    if file.exists() {
        move_file(&file, &settings_backup(&file))?;
    }
    settings_replaced(&app, &session, &mut revision, &before, &serde_json::Map::new());
    Ok(())
}

/// Bring back the settings that Restore Defaults moved aside.
#[tauri::command]
pub(crate) fn restore_settings_backup(app: AppHandle, session: State<'_, Session>) -> CommandResult<()> {
    let file = settings_file(&app)?;
    let backup = settings_backup(&file);
    let text = std::fs::read_to_string(&backup).map_err(|_| "There is no copy of the old settings.".to_owned())?;
    let after: serde_json::Map<String, Value> = serde_json::from_str(&text).map_err(|err| err.to_string())?;
    let mut revision = session.settings.lock().expect("session lock");
    let before = read_settings(&app);
    move_file(&backup, &file)?;
    settings_replaced(&app, &session, &mut revision, &before, &after);
    Ok(())
}

/// Show settings.json in Finder, Explorer or the file manager; an empty one is made first, so
/// there is a file to show.
#[tauri::command]
pub(crate) fn reveal_settings(app: AppHandle) -> CommandResult<()> {
    let file = settings_file(&app)?;
    if !file.exists() {
        std::fs::create_dir_all(file.parent().expect("settings file has a parent")).map_err(|err| err.to_string())?;
        std::fs::write(&file, "{}\n").map_err(|err| err.to_string())?;
    }
    open_in::reveal(&file)
}
