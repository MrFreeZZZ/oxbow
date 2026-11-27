//! Repository windows: opening a new one, closing one, the macOS menu bar and quitting.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::CommandResult;
use crate::outside::worktree_folder;
use crate::preferences::{appearance, read_settings};
use crate::session::{Located, RepoRequest, Session, WindowTarget, current};

/// Open the worktree at `path` in a window of its own, or bring forward the window that has it.
/// Async, because making a window in a synchronous command deadlocks on Windows.
#[tauri::command]
pub(crate) async fn open_window(
    app: AppHandle,
    window: RepoRequest,
    session: State<'_, Session>,
    path: String,
) -> CommandResult<()> {
    let _topology = session.topology()?;
    let repo = current(&session, &window)?;
    let folder = worktree_folder(&repo, &path)?;
    let label = match session.reserve_window(&folder)? {
        WindowTarget::Existing(target) => return raise_repo(&app, &target),
        WindowTarget::New(label) => label,
    };
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
        .traffic_light_position(tauri::LogicalPosition::new(22.0, 22.0));
    let reserved = || {
        session
            .repositories
            .lock()
            .expect("session lock")
            .contains_window(&label)
    };
    if !reserved() {
        return Ok(());
    }
    match builder.build() {
        Ok(open) if !reserved() => open.destroy().map_err(|err| err.to_string()),
        Ok(_) => Ok(()),
        Err(err) => {
            session.remove_window(&label);
            Err(err.to_string())
        }
    }
}

/// Bring forward the owner. Its UI activates the existing tab after handling the request.
pub(crate) fn raise_repo(app: &AppHandle, target: &Located) -> CommandResult<()> {
    // The reservation can precede the native window: its first invocation builds it.
    if let Some(window) = app.get_webview_window(target.window()) {
        let _ = window.unminimize();
        window.set_focus().map_err(|err| err.to_string())?;
        if let Located::Open(address) = target {
            app.state::<Session>()
                .emit_repo(app, address, "repo-activate-request", (), false);
        }
    }
    Ok(())
}

pub(crate) fn worktree_removed(app: &AppHandle, session: &Session, path: &Path) {
    let (tabs, windows) = session.finish_worktree_remove(path);
    // A restored tab can still be a placeholder and therefore have no repository session.
    let _ = app.emit("worktree-removed", path.to_string_lossy());
    for address in &tabs {
        session.emit_closed(address);
    }
    for label in &windows {
        if let Some(open) = app.get_webview_window(label) {
            // Draft writes were checked before removal. A close request would try to save again.
            let _ = open.destroy();
        }
    }
    if !tabs.is_empty() || !windows.is_empty() {
        crate::repos::changed(app);
    }
}

/// The default macOS menu, with Check for Updates… and Settings… (⌘,) after About. About shows the
/// version, the commit the build is of and when it was built (build.rs).
#[cfg(target_os = "macos")]
pub(crate) fn app_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let menu = tauri::menu::Menu::default(app)?;
    if let Some(app_menu) = menu.items()?.first().and_then(|item| item.as_submenu().cloned()) {
        app_menu.remove_at(0)?;
        app_menu.insert(
            &tauri::menu::MenuItem::with_id(app, "about", "About Oxbow", true, None::<&str>)?,
            0,
        )?;
        let updates = tauri::menu::MenuItem::with_id(app, "check-updates", "Check for Updates…", true, None::<&str>)?;
        app_menu.insert(&updates, 1)?;
        let settings = tauri::menu::MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
        app_menu.insert(&settings, 3)?;
        app_menu.insert(&tauri::menu::PredefinedMenuItem::separator(app)?, 4)?;
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
    let repo_window = |label: &str| {
        label != "settings" && label != "console" && !crate::user_command_window::is_command_window(label)
    };
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
