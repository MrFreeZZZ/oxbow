//! Shared update lifecycle. Only a verified download and an explicit, idle restart may install.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::Update;
use tauri_plugin_updater::UpdaterExt;
use tokio::sync::watch;

use crate::preferences::read_settings;
use crate::settings::Settings;

#[cfg(feature = "updater-test")]
mod test_transport;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Channel {
    Release,
    Preview,
}

#[derive(Deserialize)]
pub(crate) struct ChannelDefinition {
    pub endpoint: String,
    pub platforms: Vec<String>,
}

#[derive(Deserialize)]
struct Channels {
    release: ChannelDefinition,
    preview: ChannelDefinition,
}

impl Channel {
    pub(crate) fn built() -> Self {
        match env!("OXBOW_BUILD_CHANNEL") {
            "release" => Self::Release,
            "preview" => Self::Preview,
            _ => unreachable!("build.rs validates the channel"),
        }
    }

    pub(crate) fn definition(self) -> &'static ChannelDefinition {
        static CHANNELS: OnceLock<Channels> = OnceLock::new();
        let channels = CHANNELS.get_or_init(|| {
            let channels: Channels =
                serde_json::from_str(include_str!("../update-channels.json")).expect("update channel definitions");
            for channel in [&channels.release, &channels.preview] {
                assert!(
                    channel
                        .endpoint
                        .starts_with("https://github.com/MrFreeZZZ/oxbow/releases/")
                );
                assert!(!channel.platforms.is_empty());
            }
            channels
        });
        match self {
            Self::Release => &channels.release,
            Self::Preview => &channels.preview,
        }
    }

    fn configured(settings: &Settings, built: Self) -> Self {
        match settings
            .get("oxbow.updates.channel")
            .and_then(serde_json::Value::as_str)
        {
            Some("release") => Self::Release,
            Some("preview") => Self::Preview,
            // "auto", a missing key or an invalid hand-edited value follow the build.
            _ => built,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateInfo {
    version: String,
    commit: &'static str,
    build_channel: Channel,
    channel: Channel,
    platform: Option<String>,
    available_channels: Vec<Channel>,
    can_install: bool,
}

impl UpdateInfo {
    fn new(version: String, built: Channel, settings: &Settings, platform: Option<String>) -> Self {
        let available_channels = [Channel::Release, Channel::Preview]
            .into_iter()
            .filter(|channel| {
                platform
                    .as_ref()
                    .is_some_and(|p| channel.definition().platforms.contains(p))
            })
            .collect();
        Self {
            version,
            commit: env!("OXBOW_COMMIT"),
            build_channel: built,
            channel: Channel::configured(settings, built),
            platform,
            available_channels,
            can_install: can_install(),
        }
    }
}

#[tauri::command]
pub(crate) fn update_info(app: AppHandle) -> UpdateInfo {
    UpdateInfo::new(
        app.package_info().version.to_string(),
        Channel::built(),
        &read_settings(&app),
        tauri_plugin_updater::target(),
    )
}

pub(crate) fn build_info(version: String) -> UpdateInfo {
    UpdateInfo::new(
        version,
        Channel::built(),
        &Settings::new(),
        tauri_plugin_updater::target(),
    )
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateRelease {
    version: String,
    notes: Option<String>,
    #[serde(skip_serializing)]
    page: String,
}

#[derive(Clone, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub(crate) enum CheckState {
    Idle,
    Checking,
    Current,
    Available {
        release: UpdateRelease,
    },
    Downloading {
        release: UpdateRelease,
        received: u64,
        total: Option<u64>,
    },
    Verifying {
        release: UpdateRelease,
    },
    Ready {
        release: UpdateRelease,
    },
    Preparing {
        release: UpdateRelease,
    },
    Installing {
        release: UpdateRelease,
    },
    DownloadFailed {
        release: UpdateRelease,
        message: String,
    },
    InstallFailed {
        release: UpdateRelease,
        message: String,
    },
    Failed {
        message: String,
    },
    Unsupported {
        message: String,
    },
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateStatus {
    channel: Channel,
    checked_at: Option<u64>,
    #[serde(flatten)]
    state: CheckState,
}

impl UpdateStatus {
    fn idle(channel: Channel) -> Self {
        Self {
            channel,
            checked_at: None,
            state: CheckState::Idle,
        }
    }
}

struct Checks {
    running: bool,
    status: UpdateStatus,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
    cancel: Option<watch::Sender<bool>>,
    preparing: std::collections::HashMap<String, Option<String>>,
    generation: u64,
    prepared: std::collections::HashSet<String>,
}

impl Checks {
    fn acknowledge(&mut self, label: &str, generation: u64, problem: Option<String>) {
        if self.generation != generation
            || !matches!(self.status.state, CheckState::Preparing { .. })
            || self.prepared.contains(label)
        {
            return;
        }
        if !self.preparing.contains_key(label) {
            return;
        }
        self.prepared.insert(label.to_owned());
        self.preparing.remove(label);
        if let Some(problem) = problem {
            self.preparing.insert(label.to_owned(), Some(problem));
        }
    }

    fn finish_download(
        &mut self,
        selected: Channel,
        release: UpdateRelease,
        result: Result<Vec<u8>, String>,
        cancelled: bool,
        too_large: bool,
    ) {
        self.running = false;
        self.cancel = None;
        if self.status.channel != selected {
            self.status = UpdateStatus::idle(selected);
            self.update = None;
            self.bytes = None;
            return;
        }
        self.status.state = if too_large {
            CheckState::DownloadFailed {
                release,
                message: "The update package is too large. Open the release to download it manually.".into(),
            }
        } else if cancelled {
            CheckState::Available { release }
        } else {
            match result {
                Ok(bytes) if bytes.len() <= 256 * 1024 * 1024 => {
                    self.bytes = Some(bytes);
                    CheckState::Ready { release }
                }
                Err(message) => CheckState::DownloadFailed { release, message },
                Ok(_) => CheckState::DownloadFailed {
                    release,
                    message: "The update package is too large.".into(),
                },
            }
        };
    }

    fn begin(&mut self, channel: Channel) -> bool {
        if self.running {
            return false;
        }
        self.running = true;
        self.update = None;
        self.bytes = None;
        self.status = UpdateStatus {
            channel,
            checked_at: None,
            state: CheckState::Checking,
        };
        true
    }

    fn finish(&mut self, selected: Channel, result: UpdateStatus) {
        self.running = false;
        let same = selected == result.channel;
        self.status = if same { result } else { UpdateStatus::idle(selected) };
        if !same {
            self.update = None;
            self.bytes = None;
        }
    }
}

pub(crate) struct UpdateChecks(Mutex<Checks>);

impl Default for UpdateChecks {
    fn default() -> Self {
        Self(Mutex::new(empty_checks()))
    }
}

fn empty_checks() -> Checks {
    Checks {
        running: false,
        status: UpdateStatus::idle(Channel::built()),
        update: None,
        bytes: None,
        cancel: None,
        preparing: Default::default(),
        generation: 0,
        prepared: Default::default(),
    }
}

pub(crate) fn restarting(app: &AppHandle) -> bool {
    matches!(
        app.state::<UpdateChecks>()
            .0
            .lock()
            .expect("update checks lock")
            .status
            .state,
        CheckState::Preparing { .. } | CheckState::Installing { .. }
    )
}

fn selected_channel(app: &AppHandle) -> Channel {
    Channel::configured(&read_settings(app), Channel::built())
}

fn can_install() -> bool {
    if cfg!(debug_assertions) {
        return false;
    }
    use tauri::utils::{config::BundleType, platform::bundle_type};
    match bundle_type() {
        Some(BundleType::App) => std::env::current_exe()
            .ok()
            .is_some_and(|p| p.ancestors().any(|p| p.extension().is_some_and(|ext| ext == "app"))),
        Some(BundleType::AppImage | BundleType::Nsis | BundleType::Msi) => true,
        _ => false,
    }
}

fn release_of(state: &CheckState) -> Option<&UpdateRelease> {
    match state {
        CheckState::Available { release }
        | CheckState::Downloading { release, .. }
        | CheckState::Verifying { release }
        | CheckState::Ready { release }
        | CheckState::Preparing { release }
        | CheckState::Installing { release }
        | CheckState::DownloadFailed { release, .. }
        | CheckState::InstallFailed { release, .. } => Some(release),
        _ => None,
    }
}

fn broadcast(app: &AppHandle) -> UpdateStatus {
    let status = app
        .state::<UpdateChecks>()
        .0
        .lock()
        .expect("update checks lock")
        .status
        .clone();
    let _ = app.emit("updates-changed", status.clone());
    #[cfg(feature = "updater-test")]
    test_transport::record(&status);
    status
}

/// Only checks run on a timer. Downloads and restarts always require a user's click.
pub(crate) fn start(app: &AppHandle) {
    #[cfg(feature = "updater-test")]
    test_transport::started(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;
        loop {
            let settings = read_settings(&app);
            if !cfg!(debug_assertions)
                && settings
                    .get("oxbow.updates.autoCheck")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true)
            {
                let idle = {
                    let state = app.state::<UpdateChecks>();
                    let checks = state.0.lock().expect("update checks lock");
                    !checks.running && checks.bytes.is_none()
                };
                if idle {
                    check_updates(app.clone()).await;
                }
            }
            tokio::time::sleep(Duration::from_secs(6 * 60 * 60)).await;
        }
    });
}

#[tauri::command]
pub(crate) async fn download_update(app: AppHandle) -> UpdateStatus {
    let state = app.state::<UpdateChecks>();
    let (mut update, release, mut cancelled) = {
        let mut checks = state.0.lock().expect("update checks lock");
        if checks.running || checks.status.channel != selected_channel(&app) || !can_install() {
            return checks.status.clone();
        }
        let (Some(update), Some(release)) = (checks.update.clone(), release_of(&checks.status.state).cloned()) else {
            return checks.status.clone();
        };
        if checks.bytes.is_some() {
            return checks.status.clone();
        }
        let (cancel, cancelled) = watch::channel(false);
        checks.cancel = Some(cancel);
        checks.running = true;
        checks.status.state = CheckState::Downloading {
            release: release.clone(),
            received: 0,
            total: None,
        };
        (update, release, cancelled)
    };
    broadcast(&app);
    update.timeout = Some(Duration::from_secs(10 * 60));
    let mut received = 0u64;
    let mut too_large = false;
    let mut last = std::time::Instant::now();
    let result = tokio::select! {
        _ = cancelled.changed() => Err("cancelled".to_owned()),
        result = update.download(|chunk, total| {
            received = received.saturating_add(chunk as u64);
            // Stop an unexpectedly large response before retaining it in memory.
            if received > 256 * 1024 * 1024 || total.is_some_and(|n| n > 256 * 1024 * 1024) {
                too_large = true;
                if let Some(cancel) = &state.0.lock().expect("update checks lock").cancel { let _ = cancel.send(true); }
            }
            if last.elapsed() >= Duration::from_millis(150) {
                state.0.lock().expect("update checks lock").status.state = CheckState::Downloading { release: release.clone(), received, total };
                broadcast(&app);
                last = std::time::Instant::now();
            }
        }, || {
            state.0.lock().expect("update checks lock").status.state = CheckState::Verifying { release: release.clone() };
            broadcast(&app);
        }) => result.map_err(|error| {
            use tauri_plugin_updater::Error;
            match error {
                Error::Minisign(_) | Error::Base64(_) | Error::SignatureUtf8(_) | Error::SignedVersionMismatch { .. } | Error::MissingSignedVersion =>
                    "The update signature could not be verified. Nothing was installed. Try downloading again or open the release.".into(),
                _ => "The update could not be downloaded. Check your connection and try again.".into(),
            }
        }),
    };
    {
        let mut checks = state.0.lock().expect("update checks lock");
        checks.finish_download(
            selected_channel(&app),
            release,
            result,
            *cancelled.borrow(),
            too_large || received > 256 * 1024 * 1024,
        );
    }
    broadcast(&app)
}

#[tauri::command]
pub(crate) fn cancel_update_download(app: AppHandle) {
    let state = app.state::<UpdateChecks>();
    let checks = state.0.lock().expect("update checks lock");
    if let Some(cancel) = &checks.cancel {
        let _ = cancel.send(true);
    }
}

/// Every webview must finish its own draft queues and refuse unsaved editors. A generation
/// prevents a late acknowledgement from an earlier attempt allowing another restart.
#[tauri::command]
pub(crate) fn update_restart_ready(
    app: AppHandle,
    window: tauri::WebviewWindow,
    generation: u64,
    problem: Option<String>,
) {
    let state = app.state::<UpdateChecks>();
    let mut checks = state.0.lock().expect("update checks lock");
    checks.acknowledge(window.label(), generation, problem);
}

#[tauri::command]
pub(crate) async fn install_update(app: AppHandle) -> UpdateStatus {
    use tauri_plugin_window_state::AppHandleExt;
    let state = app.state::<UpdateChecks>();
    let session = app.state::<crate::session::Session>();
    let release = {
        let mut checks = state.0.lock().expect("update checks lock");
        if checks.running || checks.bytes.is_none() || checks.status.channel != selected_channel(&app) || !can_install()
        {
            return checks.status.clone();
        }
        let Some(release) = release_of(&checks.status.state).cloned() else {
            return checks.status.clone();
        };
        checks.running = true;
        checks.status.state = CheckState::Preparing {
            release: release.clone(),
        };
        checks.generation += 1;
        checks.prepared.clear();
        checks.preparing = app
            .webview_windows()
            .keys()
            .map(|label| (label.clone(), None))
            .collect();
        release
    };
    broadcast(&app);
    let result: Result<(), String> = async {
        if !session.stops.pause_if_idle() || !session.turns.pause_if_idle() {
            return Err("Finish the running Git operations and commands, then try Restart & Install again.".into());
        }
        let generation = state.0.lock().expect("update checks lock").generation;
        app.emit("update-prepare-restart", generation).map_err(|_| "Could not prepare the windows for restart.")?;
        let until = std::time::Instant::now() + Duration::from_secs(8);
        loop {
            {
                let checks = state.0.lock().expect("update checks lock");
                if let Some(problem) = checks.preparing.values().find_map(|p| p.as_ref()) { return Err(problem.clone()); }
                if checks.preparing.is_empty() { break; }
            }
            if std::time::Instant::now() >= until { return Err("A window did not finish preparing for restart. Close unfinished dialogs and try again.".into()); }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if app.webview_windows().keys().any(|label| !state.0.lock().expect("update checks lock").prepared.contains(label)) {
            return Err("A new window opened during restart preparation. Try Restart & Install again.".into());
        }
        if selected_channel(&app) != state.0.lock().expect("update checks lock").status.channel {
            return Err("The update channel changed. Check for updates again.".into());
        }
        app.save_window_state(tauri_plugin_window_state::StateFlags::all()).map_err(|_| "Window positions could not be saved. Try again.")?;
        let (update, bytes) = {
            let mut checks = state.0.lock().expect("update checks lock");
            checks.status.state = CheckState::Installing { release: release.clone() };
            (checks.update.clone().ok_or("Check for updates again.")?, checks.bytes.take().ok_or("Download the update again.")?)
        };
        broadcast(&app);
        let result = tauri::async_runtime::spawn_blocking(move || {
            let result = update.install(&bytes);
            (result, bytes)
        }).await.map_err(|_| "The installer could not finish. Download the update again.")?;
        if result.0.is_err() {
            state.0.lock().expect("update checks lock").bytes = Some(result.1);
            return Err("The update could not be installed. Check that the application and its folder are writable and try again, or install it from the release.".into());
        }
        restart_installed(&app).await
    }.await;
    session.stops.resume();
    session.turns.resume();
    {
        let mut checks = state.0.lock().expect("update checks lock");
        checks.running = false;
        checks.preparing.clear();
        let message = result.err().unwrap_or_default();
        checks.status.state = if checks.bytes.is_some() {
            CheckState::InstallFailed { release, message }
        } else {
            CheckState::DownloadFailed { release, message }
        };
    }
    broadcast(&app)
}

/// Launch Services registers the replacement as a new macOS application instance. A plain
/// executable spawn can leave the replacement running without an accessible application window.
async fn restart_installed(app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let binary = tauri::utils::platform::current_exe().map_err(|_| "Could not find the updated application.")?;
        let bundle = binary
            .ancestors()
            .find(|path| path.extension().is_some_and(|ext| ext == "app"))
            .ok_or("Could not find the updated application bundle.")?
            .to_owned();
        let status = tauri::async_runtime::spawn_blocking(move || {
            std::process::Command::new("/usr/bin/open")
                .arg("-n")
                .arg(bundle)
                .status()
        })
        .await
        .map_err(|_| "Could not restart Oxbow. Quit and open it again to use the installed update.")?
        .map_err(|_| "Could not restart Oxbow. Quit and open it again to use the installed update.")?;
        if !status.success() {
            return Err("Could not restart Oxbow. Quit and open it again to use the installed update.".into());
        }
        app.exit(0);
        std::future::pending().await
    }
    #[cfg(not(target_os = "macos"))]
    app.restart()
}

#[tauri::command]
pub(crate) fn update_status(app: AppHandle) -> UpdateStatus {
    let state = app.state::<UpdateChecks>();
    let mut checks = state.0.lock().expect("update checks lock");
    let selected = selected_channel(&app);
    if !checks.running && selected != checks.status.channel {
        checks.status = UpdateStatus::idle(selected);
        checks.update = None;
        checks.bytes = None;
    }
    checks.status.clone()
}

fn candidate(
    channel: Channel,
    version: String,
    notes: Option<String>,
    url: &str,
    manifest: &serde_json::Value,
) -> Result<UpdateRelease, String> {
    let expected_channel = match channel {
        Channel::Release => "release",
        Channel::Preview => "preview",
    };
    if manifest.get("channel").and_then(serde_json::Value::as_str) != Some(expected_channel) {
        return Err("GitHub returned update information for a different channel. Try again later.".into());
    }
    let commit = manifest.get("commit").and_then(serde_json::Value::as_str).unwrap_or("");
    if commit.len() != 40 || !commit.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
        return Err("The update information does not identify a valid source commit.".into());
    }
    let tag = match channel {
        Channel::Release => format!("v{version}"),
        Channel::Preview => format!("preview-{commit}"),
    };
    let prefix = format!("https://github.com/MrFreeZZZ/oxbow/releases/download/{tag}/");
    if !url
        .strip_prefix(&prefix)
        .is_some_and(|file| !file.is_empty() && !file.contains('/'))
    {
        return Err("The update package does not belong to the expected Oxbow release.".into());
    }
    Ok(UpdateRelease {
        version,
        notes: notes.map(|text| text.chars().take(20_000).collect()),
        page: format!("https://github.com/MrFreeZZZ/oxbow/releases/tag/{tag}"),
    })
}

fn check_error(error: tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error;
    match error {
        Error::ReleaseNotFound => {
            "GitHub did not provide update information for this channel. Try again later or open the releases page."
                .into()
        }
        Error::Reqwest(error) if error.is_timeout() => {
            "The update check timed out. Check your connection and try again.".into()
        }
        Error::Reqwest(_) => "Could not connect to GitHub. Check your connection and try again.".into(),
        Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
            "No update package is available for this device in the selected channel.".into()
        }
        _ => "The update information from GitHub could not be read. Try again later or open the releases page.".into(),
    }
}

async fn fetch_update(app: &AppHandle, channel: Channel) -> Result<Option<(UpdateRelease, Update)>, String> {
    let builder = app
        .updater_builder()
        .endpoints(vec![
            channel.definition().endpoint.parse().expect("built-in update endpoint"),
        ])
        .map_err(check_error)?
        .timeout(Duration::from_secs(20));
    #[cfg(feature = "updater-test")]
    let builder = test_transport::configure(builder, channel);
    let updater = builder.build().map_err(check_error)?;
    let Some(update) = updater.check().await.map_err(check_error)? else {
        return Ok(None);
    };
    candidate(
        channel,
        update.version.clone(),
        update.body.clone(),
        update.download_url.as_str(),
        &update.raw_json,
    )
    .map(|release| {
        #[cfg(feature = "updater-test")]
        let update = test_transport::download(update);
        Some((release, update))
    })
}

#[tauri::command]
pub(crate) async fn check_updates(app: AppHandle) -> UpdateStatus {
    let channel = selected_channel(&app);
    let state = app.state::<UpdateChecks>();
    let checking = {
        let mut checks = state.0.lock().expect("update checks lock");
        if checks.status.channel == channel && checks.bytes.is_some() {
            return checks.status.clone();
        }
        if !checks.begin(channel) {
            return checks.status.clone();
        }
        checks.status.clone()
    };
    let _ = app.emit("updates-changed", checking);
    let supported =
        tauri_plugin_updater::target().is_some_and(|target| channel.definition().platforms.contains(&target));
    let (result, checked_at) = if supported {
        match fetch_update(&app, channel).await {
            Ok(update) => {
                let at = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                (
                    update.map_or(CheckState::Current, |(release, update)| {
                        state.0.lock().expect("update checks lock").update = Some(update);
                        CheckState::Available { release }
                    }),
                    Some(at),
                )
            }
            Err(message) => (CheckState::Failed { message }, None),
        }
    } else {
        let message = if channel == Channel::Preview {
            "Preview updates are available only for macOS Apple Silicon. Select Release on this device."
        } else {
            "Updates are not available for this operating system or architecture."
        };
        (
            CheckState::Unsupported {
                message: message.into(),
            },
            None,
        )
    };
    let status = {
        let mut checks = state.0.lock().expect("update checks lock");
        checks.finish(
            selected_channel(&app),
            UpdateStatus {
                channel,
                checked_at,
                state: result,
            },
        );
        checks.status.clone()
    };
    let _ = app.emit("updates-changed", status.clone());
    #[cfg(feature = "updater-test")]
    test_transport::record(&status);
    status
}

#[tauri::command]
pub(crate) fn open_update_release(app: AppHandle) -> crate::CommandResult<()> {
    let channel = selected_channel(&app);
    let state = app.state::<UpdateChecks>();
    let checks = state.0.lock().expect("update checks lock");
    let page = match &checks.status.state {
        state if checks.status.channel == channel && release_of(state).is_some() => {
            release_of(state).unwrap().page.clone()
        }
        _ => match channel {
            Channel::Release => "https://github.com/MrFreeZZZ/oxbow/releases/latest",
            Channel::Preview => "https://github.com/MrFreeZZZ/oxbow/releases?q=prerelease%3Atrue",
        }
        .to_owned(),
    };
    drop(checks);
    crate::open_in::open_url(&page)
}

#[cfg(target_os = "macos")]
pub(crate) fn show_updates(app: &AppHandle) -> crate::CommandResult<()> {
    crate::preferences::show_settings(app)?;
    let _ = app.emit_to("settings", "settings-section", "updates");
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        check_updates(app).await;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn release() -> UpdateRelease {
        UpdateRelease {
            version: "0.1.551".into(),
            notes: None,
            page: "https://github.com/MrFreeZZZ/oxbow/releases/tag/v0.1.551".into(),
        }
    }

    #[test]
    fn only_a_successful_verified_download_becomes_installable() {
        for (result, cancelled, too_large, expected) in [
            (Ok(vec![1, 2, 3]), false, false, "ready"),
            (Err("Invalid signature".into()), false, false, "downloadFailed"),
            (Ok(vec![1, 2, 3]), true, false, "available"),
            (Ok(vec![1, 2, 3]), true, true, "downloadFailed"),
        ] {
            let mut checks = empty_checks();
            checks.status.channel = Channel::Release;
            checks.running = true;
            checks.finish_download(Channel::Release, release(), result, cancelled, too_large);
            assert!(!checks.running);
            assert_eq!(serde_json::to_value(&checks.status).unwrap()["state"], expected);
            assert_eq!(checks.bytes.is_some(), expected == "ready");
        }
        let mut checks = empty_checks();
        checks.status.channel = Channel::Preview;
        checks.finish_download(Channel::Release, release(), Ok(vec![1]), false, false);
        assert!(checks.bytes.is_none());
        assert!(matches!(checks.status.state, CheckState::Idle));
        assert_eq!(checks.status.channel, Channel::Release);
    }

    #[test]
    fn restart_requires_every_window_and_keeps_failures_and_attempts_separate() {
        let mut checks = empty_checks();
        checks.generation = 2;
        checks.status.state = CheckState::Preparing { release: release() };
        checks.preparing.insert("main".into(), None);
        checks.preparing.insert("settings".into(), None);
        checks.acknowledge("main", 1, None);
        checks.acknowledge("unknown", 2, None);
        assert_eq!(checks.preparing.len(), 2);
        checks.acknowledge("main", 2, None);
        assert_eq!(checks.preparing.len(), 1);
        checks.acknowledge("settings", 2, Some("Unsaved editor".into()));
        checks.acknowledge("settings", 2, None);
        assert_eq!(checks.preparing["settings"].as_deref(), Some("Unsaved editor"));
        assert_eq!(checks.prepared.len(), 2);
    }

    #[test]
    fn automatic_channel_follows_the_build_and_explicit_choices_persist() {
        for built in [Channel::Release, Channel::Preview] {
            for value in [json!(null), json!("auto"), json!("invalid"), json!(false)] {
                let mut settings = Settings::new();
                settings.insert("oxbow.updates.channel".into(), value);
                assert_eq!(Channel::configured(&settings, built), built);
            }
            for (value, expected) in [("release", Channel::Release), ("preview", Channel::Preview)] {
                let mut settings = Settings::new();
                settings.insert("oxbow.updates.channel".into(), json!(value));
                assert_eq!(Channel::configured(&settings, built), expected);
            }
        }
    }

    #[test]
    fn preview_only_supports_apple_silicon_and_unknown_platforms_support_neither() {
        for (platform, expected) in [
            (Some("darwin-aarch64"), vec![Channel::Release, Channel::Preview]),
            (Some("darwin-x86_64"), vec![Channel::Release]),
            (Some("linux-x86_64"), vec![Channel::Release]),
            (Some("windows-x86_64"), vec![Channel::Release]),
            (None, vec![]),
        ] {
            let info = UpdateInfo::new(
                "0.1.550".into(),
                Channel::Preview,
                &Settings::new(),
                platform.map(str::to_owned),
            );
            assert_eq!(info.available_channels, expected);
            assert_eq!(
                info.channel,
                Channel::Preview,
                "an unsupported choice must not silently switch channels"
            );
        }
    }

    #[test]
    fn metadata_preserves_the_package_version_and_has_the_ui_shape() {
        let info = UpdateInfo::new(
            "0.1.550".into(),
            Channel::Release,
            &Settings::new(),
            Some("darwin-aarch64".into()),
        );
        let value = serde_json::to_value(info).unwrap();
        assert_eq!(value["version"], "0.1.550");
        assert_eq!(value["buildChannel"], "release");
        assert_eq!(value["channel"], "release");
        assert_eq!(value["availableChannels"], json!(["release", "preview"]));
        assert_eq!(value["platform"], "darwin-aarch64");
        assert!(value["commit"].is_string());
        assert_eq!(value.as_object().unwrap().len(), 7);
    }

    #[test]
    fn manual_checks_are_shared_and_a_late_result_cannot_replace_another_channel() {
        let mut checks = Checks {
            running: false,
            status: UpdateStatus::idle(Channel::Preview),
            ..empty_checks()
        };
        assert!(checks.begin(Channel::Preview));
        assert!(!checks.begin(Channel::Preview));
        assert!(!checks.begin(Channel::Release));
        checks.finish(
            Channel::Release,
            UpdateStatus {
                channel: Channel::Preview,
                checked_at: Some(1),
                state: CheckState::Current,
            },
        );
        assert!(!checks.running);
        assert_eq!(checks.status.channel, Channel::Release);
        assert!(matches!(checks.status.state, CheckState::Idle));
        assert!(checks.begin(Channel::Release));
    }

    #[test]
    fn release_links_must_match_the_channel_commit_and_version() {
        let commit = "a".repeat(40);
        let release = json!({"channel":"release","commit":commit});
        let url = "https://github.com/MrFreeZZZ/oxbow/releases/download/v0.1.551/Oxbow.app.tar.gz";
        let item = candidate(Channel::Release, "0.1.551".into(), None, url, &release).unwrap();
        assert_eq!(item.page, "https://github.com/MrFreeZZZ/oxbow/releases/tag/v0.1.551");
        for wrong in [
            "https://evil.example/Oxbow.app.tar.gz",
            "https://github.com/MrFreeZZZ/oxbow/releases/download/v0.1.550/Oxbow.app.tar.gz",
            "https://github.com/another/oxbow/releases/download/v0.1.551/Oxbow.app.tar.gz",
        ] {
            assert!(candidate(Channel::Release, "0.1.551".into(), None, wrong, &release).is_err());
        }
        assert!(candidate(Channel::Preview, "0.1.551".into(), None, url, &release).is_err());
        let preview = json!({"channel":"preview","commit":commit});
        let preview_url =
            format!("https://github.com/MrFreeZZZ/oxbow/releases/download/preview-{commit}/Oxbow.app.tar.gz");
        assert!(candidate(Channel::Preview, "0.1.551".into(), None, &preview_url, &preview).is_ok());
        assert!(
            candidate(
                Channel::Preview,
                "0.1.551".into(),
                None,
                &preview_url,
                &json!({"channel":"preview"})
            )
            .is_err()
        );
    }

    #[test]
    fn check_results_limit_notes_and_serialize_only_the_ui_contract() {
        let manifest = json!({"channel":"release","commit":"a".repeat(40)});
        let release = candidate(
            Channel::Release,
            "0.1.551".into(),
            Some("я".repeat(25_000)),
            "https://github.com/MrFreeZZZ/oxbow/releases/download/v0.1.551/Oxbow.app.tar.gz",
            &manifest,
        )
        .unwrap();
        assert_eq!(release.notes.as_ref().unwrap().chars().count(), 20_000);
        let value = serde_json::to_value(UpdateStatus {
            channel: Channel::Release,
            checked_at: Some(1),
            state: CheckState::Available { release },
        })
        .unwrap();
        assert_eq!(value["state"], "available");
        assert_eq!(value["checkedAt"], 1);
        assert_eq!(value["release"]["version"], "0.1.551");
        assert!(value["release"].get("page").is_none());
        assert!(value.get("signature").is_none());
    }
}
