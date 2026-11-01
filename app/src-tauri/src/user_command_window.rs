//! A native output window per repository window. Stable labels let window-state restore
//! its geometry; the captured state also covers updates that arrive before the webview loads.
use std::collections::HashMap;
use std::sync::Mutex;

use oxbow_core::console::Entry;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::CommandResult;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandView {
    pub revision: u64,
    pub title: String,
    pub path: String,
    pub branch: String,
    pub commit: String,
    pub running: bool,
    pub result: String,
    pub entry: Option<Entry>,
}

#[derive(Default)]
pub(crate) struct CommandWindows(Mutex<HashMap<String, CommandView>>);

pub(crate) fn is_command_window(label: &str) -> bool {
    label.starts_with("command-")
}

pub(crate) fn update(app: &AppHandle, label: &str, change: impl FnOnce(&mut CommandView)) {
    let windows = app.state::<CommandWindows>();
    let next = {
        let mut views = windows.0.lock().expect("command windows lock");
        let Some(view) = views.get_mut(label) else { return };
        change(view);
        view.revision += 1;
        view.clone()
    };
    let _ = app.emit_to(label, "user-command-state", next);
}

pub(crate) fn open(app: &AppHandle, owner: &str, mut view: CommandView) -> CommandResult<String> {
    let label = format!("command-{owner}");
    {
        let windows = app.state::<CommandWindows>();
        let mut views = windows.0.lock().expect("command windows lock");
        if views.get(&label).is_some_and(|v| v.running) {
            return Err("A command is already running in this window.".into());
        }
        view.revision = views.get(&label).map_or(1, |old| old.revision + 1);
        views.insert(label.clone(), view.clone());
    }
    let opened = (|| {
        if let Some(window) = app.get_webview_window(&label) {
            let _ = window.unminimize();
            window.set_title(&view.title).map_err(|err| err.to_string())?;
            window.show().map_err(|err| err.to_string())?;
            window.set_focus().map_err(|err| err.to_string())?;
            let _ = window.emit("user-command-state", &view);
            return Ok(());
        }
        let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html#command".into()))
            .title(&view.title)
            .inner_size(760.0, 520.0)
            .min_inner_size(560.0, 320.0)
            .resizable(true)
            .theme(crate::preferences::appearance(&crate::preferences::read_settings(app)));
        #[cfg(target_os = "macos")]
        let builder = builder
            .title_bar_style(tauri::TitleBarStyle::Overlay)
            .hidden_title(true)
            .traffic_light_position(tauri::LogicalPosition::new(22.0, 30.0));
        builder.build().map(|_| ()).map_err(|err| err.to_string())
    })();
    if let Err(err) = opened {
        update(app, &label, |view| {
            view.running = false;
            view.result = err.clone();
        });
        return Err(err);
    }
    Ok(label)
}

#[tauri::command]
pub(crate) fn user_command_state(
    window: tauri::WebviewWindow,
    views: State<'_, CommandWindows>,
) -> CommandResult<CommandView> {
    views
        .0
        .lock()
        .expect("command windows lock")
        .get(window.label())
        .cloned()
        .ok_or_else(|| "No command in this window.".into())
}
