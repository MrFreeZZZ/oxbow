//! Compile-time-only transport for the separately identified updater acceptance app.
//! Normal builds contain neither this module nor its local CA/endpoint/trace path.
use super::{Channel, UpdateStatus};
use std::io::Write;
use tauri_plugin_updater::{Update, UpdaterBuilder};

const ORIGIN: &str = "https://127.0.0.1:43179";
const ROOT: &str = env!("OXBOW_UPDATER_TEST_ROOT");

pub(super) fn started(app: &tauri::AppHandle) {
    use tauri::Manager;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let windows: Vec<_> = app
            .webview_windows()
            .into_iter()
            .map(|(label, window)| serde_json::json!({"label":label,"visible":window.is_visible().ok()}))
            .collect();
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(format!("{ROOT}/launches.jsonl"))
        {
            let _ = writeln!(
                file,
                "{}",
                serde_json::json!({"version":app.package_info().version.to_string(),"pid":std::process::id(),"windows":windows})
            );
        }
    });
}

pub(super) fn configure(builder: UpdaterBuilder, channel: Channel) -> UpdaterBuilder {
    let name = match channel {
        Channel::Release => "release",
        Channel::Preview => "preview",
    };
    let certificate = reqwest::Certificate::from_pem(include_bytes!(concat!(
        env!("OXBOW_UPDATER_TEST_ROOT"),
        "/certificate.pem"
    )))
    .expect("test CA");
    builder
        .endpoints(vec![format!("{ORIGIN}/{name}/latest.json").parse().unwrap()])
        .unwrap()
        .configure_client(move |client| client.add_root_certificate(certificate.clone()).no_proxy())
}

pub(super) fn download(mut update: Update) -> Update {
    // The production validator has already checked the original GitHub package URL.
    update.download_url = format!("{ORIGIN}{}", update.download_url.path()).parse().unwrap();
    update
}

pub(super) fn record(status: &UpdateStatus) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{ROOT}/states.jsonl"))
    {
        let _ = writeln!(file, "{}", serde_json::to_string(status).unwrap());
    }
}
