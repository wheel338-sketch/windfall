//! Operator-configured, signed updates. Never opens or rewrites a project file.
use crate::{session::Session, sync::lock};
use serde::{Deserialize, Serialize};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use tauri::{AppHandle, State, WebviewWindow, ipc::Channel};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum UpdateChannel {
    Stable,
    Preview,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    token: u64,
    version: String,
    notes: String,
    channel: UpdateChannel,
    project_format: u32,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    configured: bool,
    reason: Option<String>,
    update: Option<UpdateInfo>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    downloaded: u64,
    total: Option<u64>,
    verified: bool,
}
struct Candidate {
    info: UpdateInfo,
    update: Update,
    bytes: Option<Vec<u8>>,
}
#[derive(Default)]
pub struct Updates {
    busy: AtomicBool,
    sequence: AtomicU64,
    pending: Mutex<Option<Candidate>>,
}
struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl Updates {
    fn begin(&self) -> Result<Busy<'_>, String> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "An update operation is already running.".to_owned())?;
        Ok(Busy(&self.busy))
    }
}
const MAX_UPDATE_REDIRECTS: usize = 3;

fn validate_redirect(url: &url::Url, previous: usize) -> Result<(), &'static str> {
    if previous > MAX_UPDATE_REDIRECTS {
        return Err("The update server exceeded the redirect limit.");
    }
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err("Update redirects must use HTTPS without credentials.");
    }
    Ok(())
}

fn update_redirect_policy() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        match validate_redirect(attempt.url(), attempt.previous().len()) {
            Ok(()) => attempt.follow(),
            Err(reason) => attempt.error(reason),
        }
    })
}
fn main_only(window: &WebviewWindow) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Updates are available in the main window only.".into());
    }
    Ok(())
}
fn policy(
    current: &str,
    version: &str,
    channel: UpdateChannel,
    metadata: &serde_json::Value,
    signature: &str,
    download: &url::Url,
) -> Result<(), String> {
    let current = semver::Version::parse(current).map_err(|_| "Invalid installed version.")?;
    let next = semver::Version::parse(version).map_err(|_| "Invalid update version.")?;
    if next <= current {
        return Err("Downgrades and same-version updates are refused.".into());
    }
    if channel == UpdateChannel::Stable && !next.pre.is_empty() {
        return Err("Preview releases cannot be installed from the stable channel.".into());
    }
    let expected = match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Preview => "preview",
    };
    if metadata["windfall"]["channel"].as_str() != Some(expected) {
        return Err("Update channel metadata does not match the selected channel.".into());
    }
    if metadata["windfall"]["projectFormatVersion"].as_u64()
        != Some(u64::from(windfall_project::FORMAT_VERSION))
    {
        return Err("This update changes or omits the project format. Use a separately reviewed manual upgrade.".into());
    }
    if signature.trim().is_empty()
        || download.scheme() != "https"
        || !download.username().is_empty()
        || download.password().is_some()
    {
        return Err(
            "Update must have a signature and an HTTPS download URL without credentials.".into(),
        );
    }
    Ok(())
}
fn operator_configuration(channel: UpdateChannel) -> Result<(&'static str, url::Url), String> {
    let key = option_env!("WINDFALL_UPDATE_PUBLIC_KEY")
        .filter(|key| !key.trim().is_empty())
        .ok_or("Operator must provide WINDFALL_UPDATE_PUBLIC_KEY at build time.")?;
    let endpoint = match channel {
        UpdateChannel::Stable => option_env!("WINDFALL_UPDATE_STABLE_URL"),
        UpdateChannel::Preview => option_env!("WINDFALL_UPDATE_PREVIEW_URL"),
    }
    .filter(|endpoint| !endpoint.trim().is_empty())
    .ok_or("Operator must provide the selected channel's HTTPS update endpoint at build time.")?;
    let endpoint = url::Url::parse(endpoint).map_err(|_| "Invalid update endpoint.")?;
    if endpoint.scheme() != "https"
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
    {
        return Err("Update endpoint must use HTTPS without credentials.".into());
    }
    Ok((key, endpoint))
}

#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, Updates>,
    channel: UpdateChannel,
    caller: crate::window_calls::WindowCaller,
) -> Result<CheckResult, String> {
    let _window_call = caller.enter(&window, "update_check")?;

    main_only(&window)?;
    let _busy = state.begin()?;
    *lock(&state.pending) = None;
    let (key, endpoint) = match operator_configuration(channel) {
        Ok(config) => config,
        Err(reason) => {
            return Ok(CheckResult {
                configured: false,
                reason: Some(reason),
                update: None,
            });
        }
    };
    let updater = app
        .updater_builder()
        .pubkey(key)
        .endpoints(vec![endpoint])
        .map_err(|error| error.to_string())?
        .timeout(Duration::from_secs(30))
        .configure_client(|client| {
            client
                .redirect(update_redirect_policy())
                .connect_timeout(Duration::from_secs(10))
        })
        .build()
        .map_err(|error| error.to_string())?;
    let Some(update) = updater
        .check()
        .await
        .map_err(|error| format!("Update check failed; retry when online. {error}"))?
    else {
        return Ok(CheckResult {
            configured: true,
            reason: None,
            update: None,
        });
    };
    policy(
        &update.current_version,
        &update.version,
        channel,
        &update.raw_json,
        &update.signature,
        &update.download_url,
    )?;
    let info = UpdateInfo {
        token: state.sequence.fetch_add(1, Ordering::Relaxed) + 1,
        version: update.version.clone(),
        notes: update.body.clone().unwrap_or_default(),
        channel,
        project_format: windfall_project::FORMAT_VERSION,
    };
    *lock(&state.pending) = Some(Candidate {
        info: info.clone(),
        update,
        bytes: None,
    });
    Ok(CheckResult {
        configured: true,
        reason: None,
        update: Some(info),
    })
}

#[tauri::command]
pub async fn update_download(
    window: WebviewWindow,
    state: State<'_, Updates>,
    token: u64,
    progress: Channel<Progress>,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "update_download")?;

    main_only(&window)?;
    let _busy = state.begin()?;
    let update = {
        let pending = lock(&state.pending);
        let candidate = pending
            .as_ref()
            .filter(|candidate| candidate.info.token == token)
            .ok_or("Stale update selection; check again.")?;
        candidate.update.clone()
    };
    let mut downloaded = 0u64;
    // Tauri verifies the detached signature before returning any installable bytes.
    let bytes = update
        .download(
            |chunk, total| {
                downloaded = downloaded.saturating_add(chunk as u64);
                let _ = progress.send(Progress {
                    downloaded,
                    total,
                    verified: false,
                });
            },
            || {},
        )
        .await
        .map_err(|error| {
            format!("Download or signature verification failed. Nothing was installed. {error}")
        })?;
    if bytes.is_empty() || bytes.len() > 512 * 1024 * 1024 {
        return Err("Update package is empty or exceeds 512 MiB. Nothing was installed.".into());
    }
    let total = bytes.len() as u64;
    let mut pending = lock(&state.pending);
    let candidate = pending
        .as_mut()
        .filter(|candidate| candidate.info.token == token)
        .ok_or("Stale update selection.")?;
    candidate.bytes = Some(bytes);
    let _ = progress.send(Progress {
        downloaded: total,
        total: Some(total),
        verified: true,
    });
    Ok(())
}

#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, Updates>,
    session: State<'_, Session>,
    token: u64,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "update_install")?;

    main_only(&window)?;
    let _busy = state.begin()?;
    if session.document_snapshot().dirty
        || session.transport_state().playing
        || session.recording_state().active
    {
        return Err("Save your project and stop playback before installing an update.".into());
    }
    let candidate = {
        let mut pending = lock(&state.pending);
        let candidate = pending
            .as_ref()
            .filter(|candidate| candidate.info.token == token)
            .ok_or("Stale update selection; check again.")?;
        if candidate.bytes.is_none() {
            return Err("Download and verify the update first.".into());
        }
        pending.take().ok_or("No update selected.")?
    };
    let bytes = candidate.bytes.ok_or("No verified update package.")?;
    // Installer work is never on the audio callback or a document lock.
    tauri::async_runtime::spawn_blocking(move || candidate.update.install(bytes))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| {
            format!("Installation failed. Keep the current installation and retry. {error}")
        })?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn update_redirects_are_bounded_and_refuse_downgrade_and_credentials() {
        let safe = url::Url::parse("https://updates.example.invalid/package").unwrap();
        for count in 0..=MAX_UPDATE_REDIRECTS {
            assert!(validate_redirect(&safe, count).is_ok());
        }
        for count in MAX_UPDATE_REDIRECTS + 1..=100 {
            assert!(validate_redirect(&safe, count).is_err());
        }
        for unsafe_url in [
            "http://updates.example.invalid/package",
            "https://user:secret@updates.example.invalid/package",
            "file:///installer.exe",
            "data:text/plain,installer",
        ] {
            assert!(validate_redirect(&url::Url::parse(unsafe_url).unwrap(), 1).is_err());
        }
    }
    fn metadata(channel: &str, format: u32) -> serde_json::Value {
        serde_json::json!({"windfall": {"channel": channel, "projectFormatVersion": format}})
    }
    #[test]
    fn update_fixture_rejects_downgrade_channel_format_and_unsigned_packages() {
        let https = url::Url::parse("https://updates.example.invalid/windfall.exe").unwrap();
        let stable = metadata("stable", windfall_project::FORMAT_VERSION);
        assert!(
            policy(
                "0.1.0-alpha.1",
                "0.1.0",
                UpdateChannel::Stable,
                &stable,
                "signature",
                &https
            )
            .is_ok()
        );
        for version in ["0.0.9", "0.1.0-alpha.1", "0.2.0-beta.1", "broken"] {
            assert!(
                policy(
                    "0.1.0-alpha.1",
                    version,
                    UpdateChannel::Stable,
                    &stable,
                    "signature",
                    &https
                )
                .is_err()
            );
        }
        assert!(
            policy(
                "0.1.0",
                "0.2.0",
                UpdateChannel::Stable,
                &metadata("preview", 1),
                "signature",
                &https
            )
            .is_err()
        );
        assert!(
            policy(
                "0.1.0",
                "0.2.0",
                UpdateChannel::Stable,
                &metadata("stable", 2),
                "signature",
                &https
            )
            .is_err()
        );
        assert!(
            policy(
                "0.1.0",
                "0.2.0",
                UpdateChannel::Stable,
                &serde_json::json!({}),
                "signature",
                &https
            )
            .is_err()
        );
        assert!(policy("0.1.0", "0.2.0", UpdateChannel::Stable, &stable, "", &https).is_err());
        assert!(
            policy(
                "0.1.0",
                "0.2.0",
                UpdateChannel::Stable,
                &stable,
                "signature",
                &url::Url::parse("http://updates.example.invalid/x").unwrap()
            )
            .is_err()
        );
    }
    #[test]
    fn update_operations_are_serialized_and_recover_after_failure() {
        let state = Updates::default();
        let guard = state.begin().unwrap();
        assert!(state.begin().is_err());
        drop(guard);
        assert!(state.begin().is_ok());
    }
}
