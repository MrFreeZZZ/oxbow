//! The macOS About menu opens an ordinary window with the same native behavior as Console.
#[cfg(target_os = "macos")]
pub(crate) fn show_about(app: &tauri::AppHandle) -> crate::CommandResult<()> {
    use tauri::Manager;
    if let Some(window) = app.get_webview_window("about") {
        window.unminimize().map_err(|err| err.to_string())?;
        return window.set_focus().map_err(|err| err.to_string());
    }
    tauri::WebviewWindowBuilder::new(app, "about", tauri::WebviewUrl::App("index.html#about".into()))
        .title("About Oxbow")
        .inner_size(480.0, 420.0)
        .min_inner_size(380.0, 360.0)
        .resizable(true)
        .theme(crate::preferences::appearance(&crate::preferences::read_settings(app)))
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(22.0, 30.0))
        .build()
        .map(|_| ())
        .map_err(|err| err.to_string())
}
