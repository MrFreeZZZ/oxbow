//! Oxbow's commands in the macOS menu bar: File, Edit, View, Go and Repository. The window used
//! last says which commands it has, their titles and keys (set_menu); a command picked in the
//! menu goes back to that window as a "menu-command" event, as if its key were pressed there.
//! Only the Mac has a menu bar; elsewhere set_menu finds no menu and does nothing.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;
use tauri::menu::{IsMenuItem, MenuItem, MenuItemKind, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, Wry};

/// A command in the menu, or a separator when `id` is "-", or Open Recent when it's "recent".
#[derive(Deserialize, Clone, PartialEq)]
pub struct MenuCommand {
    id: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    enabled: bool,
    accelerator: Option<String>,
}

/// One menu of the menu bar and its commands, in order.
#[derive(Deserialize, Clone, PartialEq)]
pub struct MenuPart {
    title: String,
    items: Vec<MenuCommand>,
}

/// The commands put in the menu bar, by id, and Open Recent.
#[derive(Default)]
pub struct MenuState {
    built: Mutex<Option<Built>>,
}

struct Built {
    commands: HashMap<String, MenuItem<Wry>>,
    /// What each command shows now, so only changes go to the menu.
    shown: HashMap<String, MenuCommand>,
    recent: Option<Submenu<Wry>>,
}

/// Prefix of the ids of commands from the front end.
const COMMAND: &str = "cmd:";
/// Prefix of the ids of Open Recent's repositories: the rest is the folder.
const RECENT: &str = "recent:";
const CLEAR_RECENT: &str = "recent-clear";

/// Put the commands in the menu bar the first time, and show what they are now after that.
#[tauri::command]
pub fn set_menu(app: AppHandle, state: tauri::State<'_, MenuState>, menus: Vec<MenuPart>) -> Result<(), String> {
    let Some(menu) = app.menu() else { return Ok(()) };
    let mut built = state.built.lock().expect("menu lock");
    if let Some(built) = built.as_mut() {
        for part in &menus {
            for command in &part.items {
                let Some(item) = built.commands.get(&command.id) else {
                    continue;
                };
                if built.shown.get(&command.id) == Some(command) {
                    continue;
                }
                item.set_text(&command.title).map_err(|err| err.to_string())?;
                item.set_enabled(command.enabled).map_err(|err| err.to_string())?;
                // A key the menu can't take leaves the command without one.
                if item.set_accelerator(command.accelerator.as_deref()).is_err() {
                    item.set_accelerator(None::<&str>).map_err(|err| err.to_string())?;
                }
                built.shown.insert(command.id.clone(), command.clone());
            }
        }
        return Ok(());
    }
    let mut made = Built {
        commands: HashMap::new(),
        shown: HashMap::new(),
        recent: None,
    };
    let bar = menu.items().map_err(|err| err.to_string())?;
    let submenu = |title: &str| {
        bar.iter()
            .filter_map(MenuItemKind::as_submenu)
            .find(|submenu| submenu.text().is_ok_and(|text| text == title))
            .cloned()
    };
    for part in &menus {
        let mut items: Vec<MenuItemKind<Wry>> = Vec::new();
        for command in &part.items {
            match command.id.as_str() {
                "-" => items.push(MenuItemKind::Predefined(
                    PredefinedMenuItem::separator(&app).map_err(|err| err.to_string())?,
                )),
                "recent" => {
                    let recent =
                        Submenu::with_id(&app, "recent", "Open Recent", true).map_err(|err| err.to_string())?;
                    made.recent = Some(recent.clone());
                    items.push(MenuItemKind::Submenu(recent));
                }
                id => {
                    let item = |accelerator: Option<&str>| {
                        MenuItem::with_id(
                            &app,
                            format!("{COMMAND}{id}"),
                            &command.title,
                            command.enabled,
                            accelerator,
                        )
                    };
                    let item = item(command.accelerator.as_deref())
                        .or_else(|_| item(None))
                        .map_err(|err| err.to_string())?;
                    made.commands.insert(id.to_owned(), item.clone());
                    made.shown.insert(id.to_owned(), command.clone());
                    items.push(MenuItemKind::MenuItem(item));
                }
            }
        }
        let refs: Vec<&dyn IsMenuItem<Wry>> = items.iter().map(|item| item as &dyn IsMenuItem<Wry>).collect();
        let result = match (submenu(&part.title), part.title.as_str()) {
            // Before Close Window.
            (Some(file), "File") => file.insert_items(&refs, 0),
            // Before Enter Full Screen.
            (Some(view), "View") => {
                let separator = PredefinedMenuItem::separator(&app).map_err(|err| err.to_string())?;
                view.insert_items(&refs, 0)
                    .and_then(|()| view.insert(&separator, refs.len()))
            }
            (Some(existing), _) => {
                let separator = PredefinedMenuItem::separator(&app).map_err(|err| err.to_string())?;
                existing.append(&separator).and_then(|()| existing.append_items(&refs))
            }
            // A menu of its own, before Window.
            (None, title) => {
                let at = menu.items().map_or(0, |bar| bar.len().saturating_sub(2));
                Submenu::with_items(&app, title, true, &refs).and_then(|new| menu.insert(&new, at))
            }
        };
        result.map_err(|err| err.to_string())?;
    }
    let recent = made.recent.clone();
    *built = Some(made);
    drop(built);
    if let Some(recent) = recent {
        fill_recent(&app, &recent);
    }
    Ok(())
}

/// Open Recent lists the repositories again, after the list changed.
pub fn recent_changed(app: &AppHandle) {
    let recent = app
        .state::<MenuState>()
        .built
        .lock()
        .expect("menu lock")
        .as_ref()
        .and_then(|b| b.recent.clone());
    if let Some(recent) = recent {
        fill_recent(app, &recent);
    }
}

fn fill_recent(app: &AppHandle, recent: &Submenu<Wry>) {
    while let Ok(Some(_)) = recent.remove_at(0) {}
    let repos = crate::recent_repos(app.clone());
    for repo in repos.iter().filter(|repo| !repo.missing).take(15) {
        if let Ok(item) = MenuItem::with_id(app, format!("{RECENT}{}", repo.path), &repo.name, true, None::<&str>) {
            let _ = recent.append(&item);
        }
    }
    if let Ok(separator) = PredefinedMenuItem::separator(app) {
        let _ = recent.append(&separator);
    }
    if let Ok(clear) = MenuItem::with_id(app, CLEAR_RECENT, "Clear Menu", !repos.is_empty(), None::<&str>) {
        let _ = recent.append(&clear);
    }
}

/// A command picked in the menu bar: to the window used last.
pub fn picked(app: &AppHandle, id: &str, focused: Option<String>) {
    let target = focused.unwrap_or_else(|| "main".to_owned());
    if let Some(command) = id.strip_prefix(COMMAND) {
        let _ = app.emit_to(&target, "menu-command", command);
    } else if let Some(path) = id.strip_prefix(RECENT) {
        let _ = app.emit_to(&target, "menu-open", path);
    } else if id == CLEAR_RECENT {
        // The repositories open now stay, as Clear in the Welcome window keeps the open one.
        let keep = app
            .state::<crate::Session>()
            .windows
            .lock()
            .expect("session lock")
            .values()
            .filter_map(|slot| slot.repo.as_ref().map(|repo| repo.workdir().display().to_string()))
            .collect();
        crate::clear_recent(app.clone(), keep);
    }
}
