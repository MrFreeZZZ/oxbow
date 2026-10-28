//! Repository windows: opening a new one, closing one, the macOS menu bar and quitting.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::CommandResult;
use crate::outside::worktree_folder;
use crate::preferences::{appearance, read_settings};
use crate::session::{Session, current};

/// Open the worktree at `path` in a window of its own, or bring forward the window that has it.
/// Async, because making a window in a synchronous command deadlocks on Windows.
#[tauri::command]
pub(crate) async fn open_window(
    app: AppHandle,
    window: tauri::WebviewWindow,
    session: State<'_, Session>,
    path: String,
) -> CommandResult<()> {
    let repo = current(&session, &window)?;
    let folder = worktree_folder(&repo, &path)?;
    if let Some(open) = session
        .window_of(&folder)
        .and_then(|label| app.get_webview_window(&label))
    {
        let _ = open.unminimize();
        return open.set_focus().map_err(|err| err.to_string());
    }
    let label = format!("repo-{}", session.next.fetch_add(1, Ordering::Relaxed) + 1);
    session
        .pending
        .lock()
        .expect("session lock")
        .insert(label.clone(), folder.display().to_string());
    // Named after the folder, so the Window menu and Mission Control tell the windows apart.
    let title = folder
        .file_name()
        .map_or_else(|| "Oxbow".to_owned(), |name| name.to_string_lossy().into_owned());
    let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()))
        .title(title)
        .inner_size(1360.0, 860.0)
        .min_inner_size(900.0, 560.0)
        .theme(appearance(&read_settings(&app)));
    // A little down and to the right of the window it was opened from.
    if let (Ok(at), Ok(scale)) = (window.outer_position(), window.scale_factor()) {
        let at = at.to_logical::<f64>(scale);
        builder = builder.position(at.x + 32.0, at.y + 32.0);
    }
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(22.0, 30.0));
    builder.build().map(|_| ()).map_err(|err| err.to_string())
}

/// Close the window of the worktree that was at `path`, after it was removed.
#[tauri::command]
pub(crate) fn close_window_of(app: AppHandle, window: tauri::WebviewWindow, session: State<'_, Session>, path: String) {
    if let Some(open) = session
        .window_of(Path::new(&path))
        .filter(|label| label != window.label())
        .and_then(|label| app.get_webview_window(&label))
    {
        let _ = open.close();
    }
}

/// The default macOS menu, with Settings… (⌘,) after About in the app menu. About shows the
/// version, the commit the build is of and when it was built (build.rs).
#[cfg(target_os = "macos")]
pub(crate) fn app_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let menu = tauri::menu::Menu::default(app)?;
    if let Some(app_menu) = menu.items()?.first().and_then(|item| item.as_submenu().cloned()) {
        let about = tauri::menu::AboutMetadata {
            name: Some(app.package_info().name.clone()),
            // "Version 0.1.113 (b5cc367)": macOS puts the short version in the parentheses.
            version: Some(env!("OXBOW_VERSION").to_owned()),
            short_version: Some(env!("OXBOW_COMMIT").to_owned()),
            copyright: Some(format!("Built {}", env!("OXBOW_BUILT"))),
            // No credits: macOS draws them larger than the version. Third-party notices ship in
            // the bundle's Resources (THIRD-PARTY-NOTICES.md).
            ..Default::default()
        };
        app_menu.remove_at(0)?;
        app_menu.insert(&tauri::menu::PredefinedMenuItem::about(app, None, Some(about))?, 0)?;
        let settings = tauri::menu::MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
        app_menu.insert(&settings, 2)?;
        app_menu.insert(&tauri::menu::PredefinedMenuItem::separator(app)?, 3)?;
        // macOS's own Quit ends the app at once; this one lets the windows save first (quit()).
        let last = app_menu.items()?.len() - 1;
        app_menu.remove_at(last)?;
        let name = &app.package_info().name;
        app_menu.append(&tauri::menu::MenuItem::with_id(
            app,
            "quit",
            format!("Quit {name}"),
            true,
            Some("CmdOrCtrl+Q"),
        )?)?;
    }
    // Oxbow's menus, before the window loads; menu.json can't be broken, a UI test writes it.
    let _ = crate::menu::put_at_start(app, &menu);
    Ok(menu)
}

/// A Quit under way: the repository windows are closing. One that stays open, its conflict
/// draft not saved, calls it off (cancel_quit).
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Quit: each repository window closes as its close button closes it, so the Conflicts screen
/// saves its draft first, and the app ends once they are gone. A window that couldn't save asks
/// first, and stays open on Cancel.
#[cfg(target_os = "macos")]
pub(crate) fn quit(app: &AppHandle) {
    let repo_window = |label: &str| label != "settings" && label != "console";
    let already = QUITTING.swap(true, Ordering::SeqCst);
    for (label, window) in app.webview_windows() {
        if repo_window(&label) {
            let _ = window.close();
        }
    }
    if already {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        while QUITTING.load(Ordering::SeqCst) && app.webview_windows().keys().any(|label| repo_window(label)) {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if QUITTING.swap(false, Ordering::SeqCst) {
            app.exit(0);
        }
    });
}

/// A window stayed open when Quit closed it: Oxbow doesn't quit after all.
#[tauri::command]
pub(crate) fn cancel_quit() {
    QUITTING.store(false, Ordering::SeqCst);
}
