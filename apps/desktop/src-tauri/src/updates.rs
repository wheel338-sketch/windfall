//! Operator-configured, signed updates. Never opens or rewrites a project file.
use crate::{session::Session, sync::lock};
use serde::{Deserialize, Serialize};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;
use tauri::{AppHandle, Manager, State, WebviewWindow, ipc::Channel};
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
    shutdown: Mutex<Option<(u64, crate::session::UpdateShutdown)>>,
}
struct Busy<'a>(&'a AtomicBool);
impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl Updates {
    pub(crate) fn window_destroyed(&self, label: &str) {
        if label != "main" {
            return;
        }
        // Native destruction can bypass a React cleanup or retire its caller
        // first. The owned freeze must still thaw the other live windows.
        let shutdown = lock(&self.shutdown).take().map(|(_, shutdown)| shutdown);
        if let Some(shutdown) = shutdown {
            tauri::async_runtime::spawn_blocking(move || drop(shutdown));
        }
    }
    fn begin(&self) -> Result<Busy<'_>, String> {
        self.busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "An update operation is already running.".to_owned())?;
        Ok(Busy(&self.busy))
    }
}
const MAX_UPDATE_REDIRECTS: usize = 3;

pub(crate) fn validate_redirect(url: &url::Url, previous: usize) -> Result<(), &'static str> {
    if previous > MAX_UPDATE_REDIRECTS {
        return Err("The update server exceeded the redirect limit.");
    }
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err("Update redirects must use HTTPS without credentials.");
    }
    Ok(())
}

pub(crate) fn update_redirect_policy() -> reqwest::redirect::Policy {
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
    if lock(&state.shutdown).is_some() {
        return Err("Resolve the pending installation decision first".into());
    }
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
    let Some(manifest) =
        crate::update_transfer::fetch(endpoint, crate::update_transfer::MANIFEST_CAP, |_, _| {})
            .await?
    else {
        return Ok(CheckResult {
            configured: true,
            reason: None,
            update: None,
        });
    };
    let reply = crate::update_transfer::Reply::new(manifest)?;
    let updater = app
        .updater_builder()
        .pubkey(key)
        // Keep the live windows intact if ShellExecute/UAC refuses the installer.
        .on_before_exit(|| {})
        .endpoints(vec![reply.url.clone()])
        .map_err(|error| error.to_string())?
        .timeout(Duration::from_secs(30))
        .configure_client(|client| {
            client
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(2))
        })
        .build()
        .map_err(|error| error.to_string())?;
    let Some(mut update) = updater
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
    update.timeout = Some(Duration::from_secs(15));
    update.no_proxy = true;
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

async fn bounded_verified_download(
    mut update: Update,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    let bytes = crate::update_transfer::fetch(
        update.download_url.clone(),
        crate::update_transfer::PACKAGE_CAP,
        progress,
    )
    .await?
    .ok_or("Update package is empty")?;
    verify_bounded_bytes(&mut update, bytes).await
}
async fn verify_bounded_bytes(update: &mut Update, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.is_empty() || bytes.len() > crate::update_transfer::PACKAGE_CAP {
        return Err("Update package is empty or exceeds 512 MiB".into());
    }
    let reply = crate::update_transfer::Reply::new(bytes)?;
    update.download_url = reply.url.clone();
    update.timeout = Some(Duration::from_secs(15));
    update.no_proxy = true;
    update.download(|_, _| {}, || {}).await.map_err(|error| {
        format!("Download or signature verification failed. Nothing was installed. {error}")
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
    if lock(&state.shutdown).is_some() {
        return Err("Resolve the pending installation decision first".into());
    }
    let update = {
        let pending = lock(&state.pending);
        let candidate = pending
            .as_ref()
            .filter(|candidate| candidate.info.token == token)
            .ok_or("Stale update selection; check again.")?;
        candidate.update.clone()
    };
    let bytes = bounded_verified_download(update, |downloaded, total| {
        let _ = progress.send(Progress {
            downloaded,
            total,
            verified: false,
        });
    })
    .await?;
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
pub async fn update_install_begin(
    window: WebviewWindow,
    state: State<'_, Updates>,
    session: State<'_, Session>,
    token: u64,
    caller: crate::window_calls::WindowCaller,
) -> Result<windfall_project::DocumentSnapshot, String> {
    let _window_call = caller.enter(&window, "update_install_begin")?;
    main_only(&window)?;
    let _busy = state.begin()?;
    if lock(&state.shutdown).is_some() {
        return Err("Update installation already awaits a decision".into());
    }
    if !lock(&state.pending)
        .as_ref()
        .is_some_and(|candidate| candidate.info.token == token && candidate.bytes.is_some())
    {
        return Err("Download and verify the selected update first".into());
    }
    let session = session.inner().clone();
    let (shutdown, snapshot) = tauri::async_runtime::spawn_blocking(move || {
        let shutdown = session.begin_update_shutdown()?;
        Ok::<_, String>((shutdown, session.document_snapshot()))
    })
    .await
    .map_err(|error| error.to_string())??;
    *lock(&state.shutdown) = Some((token, shutdown));
    Ok(snapshot)
}
#[tauri::command]
pub async fn update_install_cancel(
    window: WebviewWindow,
    state: State<'_, Updates>,
    token: u64,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "update_install_cancel")?;
    main_only(&window)?;
    let _busy = state.begin()?;
    let shutdown = {
        let mut owned = lock(&state.shutdown);
        if owned.as_ref().is_some_and(|(current, _)| *current != token) {
            return Err("Stale installation decision".into());
        }
        owned.take().map(|(_, shutdown)| shutdown)
    };
    if let Some(shutdown) = shutdown {
        tauri::async_runtime::spawn_blocking(move || shutdown.rollback())
            .await
            .map_err(|error| error.to_string())??;
    }
    Ok(())
}

#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, Updates>,
    token: u64,
    decision: Option<String>,
    path: Option<String>,
    caller: crate::window_calls::WindowCaller,
) -> Result<(), String> {
    let _window_call = caller.enter(&window, "update_install")?;

    main_only(&window)?;
    let _busy = state.begin()?;
    let mut shutdown = {
        let mut owned = lock(&state.shutdown);
        if owned.as_ref().is_some_and(|(current, _)| *current != token) {
            return Err("Stale installation decision".into());
        }
        owned
            .take()
            .map(|(_, shutdown)| shutdown)
            .ok_or("Begin installation before answering the unsaved-work prompt")?
    };
    let decision = decision
        .as_deref()
        .ok_or("Choose save, discard or cancel before installing")?;
    if !matches!(decision, "save" | "discard") {
        shutdown.rollback()?;
        return Err("Installation cancelled".into());
    }
    if decision == "save" {
        shutdown.save(path.as_deref())?;
    }
    if decision == "save" && app.state::<Session>().document_snapshot().dirty {
        return Err("The project was not saved; installation cancelled".into());
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
    // Preserve the verified candidate on installer refusal so retry never
    // requires downloading untrusted bytes again or pretending verification.
    let (candidate, result) = tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            shutdown.quiesce()?;
            let bytes = candidate
                .bytes
                .as_ref()
                .ok_or("No verified update package.")?;
            if let Err(error) = candidate.update.install(bytes) {
                let rollback = shutdown.rollback();
                return Err(format!(
                    "Installation failed. Keep the current installation and retry. {error}{}",
                    rollback
                        .err()
                        .map(|error| format!("; {error}"))
                        .unwrap_or_default()
                ));
            }
            // macOS/Linux return on success; retain the freeze until restart.
            std::mem::forget(shutdown);
            Ok::<_, String>(())
        })();
        (candidate, result)
    })
    .await
    .map_err(|error| error.to_string())?;
    if let Err(error) = result {
        *lock(&state.pending) = Some(candidate);
        return Err(error);
    }
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    fn signer(args: &[&std::ffi::OsStr]) {
        let cli = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../node_modules/@tauri-apps/cli/tauri.js");
        let mut command = std::process::Command::new("node");
        command
            .arg(cli)
            .args(args)
            .env("CI", "true")
            .env_remove("TAURI_SIGNING_PRIVATE_KEY")
            .env_remove("TAURI_SIGNING_PRIVATE_KEY_PATH")
            .env_remove("TAURI_SIGNING_PRIVATE_KEY_PASSWORD")
            .stdin(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let result = command
            .output()
            .expect("the workspace's installed Tauri signer CLI is required");
        assert!(
            result.status.success(),
            "Tauri signer failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        // Never print signer stdout: it includes the temporary private key.
    }
    #[test]
    fn generated_key_packages_use_the_production_verifier_and_reject_corruption_truncation_and_wrong_keys()
     {
        use std::ffi::OsStr;
        let folder = tempfile::tempdir().unwrap();
        let key = folder.path().join("test-key");
        let wrong = folder.path().join("wrong-key");
        for path in [&key, &wrong] {
            signer(&[
                OsStr::new("signer"),
                OsStr::new("generate"),
                OsStr::new("--ci"),
                OsStr::new("--password"),
                OsStr::new(""),
                OsStr::new("--write-keys"),
                path.as_os_str(),
            ]);
        }
        let package = folder.path().join("package.bin");
        let bytes = b"Windfall signed package fixture: exact bounded bytes".to_vec();
        std::fs::write(&package, &bytes).unwrap();
        signer(&[
            OsStr::new("signer"),
            OsStr::new("sign"),
            OsStr::new("--private-key-path"),
            key.as_os_str(),
            OsStr::new("--password"),
            OsStr::new(""),
            package.as_os_str(),
        ]);
        let signature = std::fs::read_to_string(package.with_extension("bin.sig")).unwrap();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(tauri::generate_context!())
            .unwrap();
        tauri::async_runtime::block_on(async {
            for public in [key.with_extension("pub"), wrong.with_extension("pub")] {
                let key_text = std::fs::read_to_string(&public).unwrap();
                let manifest=serde_json::to_vec(&serde_json::json!({"version":"9.9.9","url":"https://updates.invalid/package","signature":signature})).unwrap();
                let reply = crate::update_transfer::Reply::new(manifest).unwrap();
                let updater = app
                    .updater_builder()
                    .pubkey(key_text.trim())
                    .endpoints(vec![reply.url.clone()])
                    .unwrap()
                    .no_proxy()
                    .configure_client(|client| {
                        client
                            .no_proxy()
                            .redirect(reqwest::redirect::Policy::none())
                    })
                    .build()
                    .unwrap();
                let update = updater.check().await.unwrap().unwrap();
                let correct = public == key.with_extension("pub");
                let accepted = verify_bounded_bytes(&mut update.clone(), bytes.clone()).await;
                if correct {
                    assert_eq!(accepted.unwrap(), bytes);
                } else {
                    assert!(accepted.is_err());
                }
                let mut corrupt = bytes.clone();
                corrupt[3] ^= 0x80;
                assert!(
                    verify_bounded_bytes(&mut update.clone(), corrupt)
                        .await
                        .is_err()
                );
                assert!(
                    verify_bounded_bytes(&mut update.clone(), bytes[..bytes.len() - 1].to_vec())
                        .await
                        .is_err()
                );
            }
        });
    }

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
