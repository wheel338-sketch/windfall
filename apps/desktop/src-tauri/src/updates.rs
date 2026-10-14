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
    // Redirects retain the stricter remote HTTPS rule, even from loopback.
    validate_endpoint(url)?;
    if url.scheme() != "https" {
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
    // The same rule as the endpoint: HTTPS, or literal loopback HTTP for a
    // local test host. The package is still verified against the key.
    if signature.trim().is_empty() || validate_endpoint(download).is_err() {
        return Err(
            "Update must have a signature and an HTTPS download URL without credentials.".into(),
        );
    }
    Ok(())
}
const ENDPOINT_CAP: usize = 8192;

// Keep this list aligned with the pinned updater's Updater::check, including
// bundle_type (documented on its Rust API). Expand before parsing/fetching.
fn expand_endpoint(
    template: &str,
    version: &str,
    target: &str,
    arch: &str,
    bundle: &str,
) -> Result<url::Url, String> {
    if template.len() > ENDPOINT_CAP {
        return Err("Update endpoint exceeds its URL length budget".into());
    }
    let version = semver::Version::parse(version)
        .map_err(|_| "Invalid installed version")?
        .to_string()
        .replace('+', "%2B");
    let mut expanded = template.to_owned();
    for (name, value) in [
        ("current_version", version.as_str()),
        ("target", target),
        ("arch", arch),
        ("bundle_type", bundle),
    ] {
        expanded = expanded.replace(&format!("{{{{{name}}}}}"), value);
        // Url-encoded templates are also accepted by the pinned updater.
        expanded = expanded.replace(&format!("%7B%7B{name}%7D%7D"), value);
        expanded = expanded.replace(&format!("%7b%7b{name}%7d%7d"), value);
    }
    let lower = expanded.to_ascii_lowercase();
    if expanded.contains(['{', '}']) || lower.contains("%7b") || lower.contains("%7d") {
        return Err("Unknown or malformed update endpoint placeholder".into());
    }
    let endpoint = url::Url::parse(&expanded).map_err(|_| "Invalid update endpoint")?;
    validate_endpoint(&endpoint).map_err(str::to_owned)?;
    Ok(endpoint)
}

pub(crate) fn validate_endpoint(endpoint: &url::Url) -> Result<(), &'static str> {
    let loopback = match endpoint.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    };
    if endpoint.as_str().len() > ENDPOINT_CAP
        || endpoint.host().is_none()
        || (endpoint.scheme() != "https" && !(endpoint.scheme() == "http" && loopback))
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
    {
        return Err(
            "Update URL must use HTTPS (or literal loopback HTTP), have no credentials and fit its length budget",
        );
    }
    Ok(())
}

/// The public key and the channel's endpoint template, from the
/// `plugins.updater` block of `tauri.conf.json`: the one place they are set.
/// The stable channel is its first endpoint. There is no preview channel
/// yet. A test build points at another host with `tauri build --config`.
fn configured_source(
    config: &tauri::Config,
    channel: UpdateChannel,
) -> Result<(String, String), String> {
    let updater = config.plugins.0.get("updater");
    let text = |pointer: &str| {
        updater
            .and_then(|updater| updater.pointer(pointer))
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let key = text("/pubkey").ok_or("This build has no updater public key.")?;
    let endpoint = match channel {
        UpdateChannel::Stable => text("/endpoints/0"),
        UpdateChannel::Preview => None,
    }
    .ok_or("This build has no update address for the selected channel.")?;
    Ok((key, endpoint))
}

fn operator_configuration(
    app: &AppHandle,
    channel: UpdateChannel,
) -> Result<(String, url::Url), String> {
    let (key, endpoint) = configured_source(app.config(), channel)?;
    let platform = tauri_plugin_updater::target().ok_or("Unsupported update platform")?;
    let (target, arch) = platform.split_once('-').ok_or("Invalid update platform")?;
    use tauri::utils::config::BundleType;
    let bundle = match tauri::utils::platform::bundle_type() {
        Some(BundleType::Deb) => "deb",
        Some(BundleType::Rpm) => "rpm",
        Some(BundleType::AppImage) => "appimage",
        Some(BundleType::Msi) => "msi",
        Some(BundleType::Nsis) => "nsis",
        Some(BundleType::App) => "app",
        _ => "unknown",
    };
    let endpoint = expand_endpoint(
        &endpoint,
        &app.package_info().version.to_string(),
        target,
        arch,
        bundle,
    )?;
    Ok((key, endpoint))
}

#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, Updates>,
    channel: UpdateChannel,
    background: Option<bool>,
    caller: crate::window_calls::WindowCaller,
) -> Result<CheckResult, String> {
    let _window_call = caller.enter(&window, "update_check")?;

    main_only(&window)?;
    let _busy = state.begin()?;
    if lock(&state.shutdown).is_some() {
        return Err("Resolve the pending installation decision first".into());
    }
    // A development build is not an installed release: it never checks by
    // itself, so a test run does not reach the release host. Asked, it does.
    if background == Some(true) && cfg!(debug_assertions) {
        return Ok(CheckResult {
            configured: false,
            reason: Some("Development builds do not check for updates by themselves.".into()),
            update: None,
        });
    }
    *lock(&state.pending) = None;
    let (key, endpoint) = match operator_configuration(&app, channel) {
        Ok(config) => config,
        Err(reason) => {
            return Ok(CheckResult {
                configured: false,
                reason: Some(reason),
                update: None,
            });
        }
    };
    // Offline, a refused connection or a missing manifest: said as a failed
    // check, with the transport's own words as its cause.
    let Some(manifest) =
        crate::update_transfer::fetch(endpoint, crate::update_transfer::MANIFEST_CAP, |_, _| {})
            .await
            .map_err(|error| {
                windfall_ipc::ui_message("updates.checkFailed", serde_json::json!({"error": error}))
            })?
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
    let Some(mut update) = updater.check().await.map_err(|error| {
        windfall_ipc::ui_message(
            "updates.checkFailed",
            serde_json::json!({"error": error.to_string()}),
        )
    })?
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
    .await
    .map_err(|error| {
        windfall_ipc::ui_message(
            "updates.downloadFailed",
            serde_json::json!({"error": error}),
        )
    })?
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
        windfall_ipc::ui_message(
            "updates.downloadFailed",
            serde_json::json!({"error": error.to_string()}),
        )
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
                return Err(windfall_ipc::ui_message(
                    "updates.installFailed",
                    serde_json::json!({"error": error.to_string(), "rollback": rollback
                        .err()
                        .map(|error| format!("; {error}"))
                        .unwrap_or_default()}),
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
    #[test]
    fn endpoint_templates_expand_every_documented_placeholder_and_encode_version_build_metadata() {
        for (placeholder, expected) in [
            ("current_version", "1.2.3-beta.1%2Bbuild.7"),
            ("target", "darwin"),
            ("arch", "aarch64"),
            ("bundle_type", "app"),
        ] {
            for variable in [
                format!("{{{{{placeholder}}}}}"),
                format!("%7B%7B{placeholder}%7D%7D"),
            ] {
                let template = format!("https://updates.invalid/{variable}?value={variable}");
                assert_eq!(
                    expand_endpoint(
                        &template,
                        "1.2.3-beta.1+build.7",
                        "darwin",
                        "aarch64",
                        "app"
                    )
                    .unwrap()
                    .as_str(),
                    format!("https://updates.invalid/{expected}?value={expected}")
                );
            }
        }
        for (target, arch, bundle) in [
            ("windows", "i686", "msi"),
            ("windows", "x86_64", "nsis"),
            ("linux", "armv7", "deb"),
            ("linux", "riscv64", "rpm"),
            ("linux", "aarch64", "appimage"),
            ("darwin", "aarch64", "app"),
            ("windows", "x86_64", "unknown"),
        ] {
            assert_eq!(expand_endpoint("https://updates.invalid/{{target}}/{{arch}}/{{bundle_type}}/{{current_version}}", "1.0.0", target, arch, bundle).unwrap().path(), format!("/{target}/{arch}/{bundle}/1.0.0"));
        }
    }
    #[test]
    fn endpoint_templates_refuse_unknown_placeholders_credentials_remote_http_and_length_overflow()
    {
        for template in [
            "https://updates.invalid/{{version}}",
            "https://updates.invalid/{{platform}}",
            "https://updates.invalid/%7B%7Bunknown%7D%7D",
            "https://updates.invalid/{{arch}",
            "https://user:secret@updates.invalid/{{target}}",
            "http://updates.invalid/{{arch}}",
            "http://localhost/{{arch}}",
            "file:///{{target}}",
        ] {
            assert!(
                expand_endpoint(template, "1.2.3", "windows", "x86_64", "nsis").is_err(),
                "{template}"
            );
        }
        for template in ["http://127.0.0.1/{{target}}", "http://[::1]/{{target}}"] {
            assert!(expand_endpoint(template, "1.2.3", "windows", "x86_64", "nsis").is_ok());
        }
        assert!(
            expand_endpoint(
                &format!("https://updates.invalid/{}", "a".repeat(ENDPOINT_CAP)),
                "1.2.3",
                "windows",
                "x86_64",
                "nsis"
            )
            .is_err()
        );
        assert!(
            expand_endpoint(
                &format!(
                    "https://updates.invalid/{}{{{{arch}}}}",
                    "a".repeat(ENDPOINT_CAP - 40)
                ),
                "1.2.3",
                "windows",
                &"a".repeat(80),
                "nsis"
            )
            .is_err()
        );
    }
    #[test]
    fn bounded_streaming_manifest_fetch_requests_the_expanded_endpoint() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let template = format!(
            "http://{}/{{{{target}}}}/{{{{arch}}}}/{{{{bundle_type}}}}?v={{{{current_version}}}}",
            listener.local_addr().unwrap()
        );
        let endpoint =
            expand_endpoint(&template, "1.2.3+build.1", "windows", "x86_64", "nsis").unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
                assert!(request.len() < 8192);
            }
            stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n").unwrap();
            String::from_utf8(request).unwrap()
        });
        tauri::async_runtime::block_on(async {
            let mut received = 0;
            assert_eq!(
                crate::update_transfer::fetch(
                    endpoint,
                    crate::update_transfer::MANIFEST_CAP,
                    |bytes, _| {
                        assert!(bytes <= crate::update_transfer::MANIFEST_CAP as u64);
                        received = bytes;
                    }
                )
                .await
                .unwrap()
                .unwrap(),
                b"abcde"
            );
            assert_eq!(received, 5);
        });
        assert!(
            server
                .join()
                .unwrap()
                .starts_with("GET /windows/x86_64/nsis?v=1.2.3%2Bbuild.1 HTTP/1.1\r\n")
        );
    }
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
    fn generated_key_packages_use_the_production_verifier_and_reject_corruption_truncation_wrong_keys_and_versions()
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
        let bytes = b"Windfall signed package fixture: exact bounded bytes".to_vec();
        // The manifest announces 9.9.9. The committed configuration requires
        // the signature to name the version, so the same bytes signed for no
        // version, or for another one, are refused like a wrong key.
        let signatures: Vec<(Option<&str>, String)> = [Some("9.9.9"), None, Some("9.9.8")]
            .into_iter()
            .enumerate()
            .map(|(index, version)| {
                let package = folder.path().join(format!("package-{index}.bin"));
                std::fs::write(&package, &bytes).unwrap();
                let mut args = vec![
                    OsStr::new("signer"),
                    OsStr::new("sign"),
                    OsStr::new("--private-key-path"),
                    key.as_os_str(),
                    OsStr::new("--password"),
                    OsStr::new(""),
                ];
                if let Some(version) = version {
                    args.extend([OsStr::new("--app-version"), OsStr::new(version)]);
                }
                args.push(package.as_os_str());
                signer(&args);
                let signature = std::fs::read_to_string(package.with_extension("bin.sig"));
                (version, signature.unwrap())
            })
            .collect();
        let app = tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(crate::context())
            .unwrap();
        let publics = [key.with_extension("pub"), wrong.with_extension("pub")];
        let cases = publics
            .iter()
            .flat_map(|public| signatures.iter().map(move |signature| (public, signature)));
        tauri::async_runtime::block_on(async {
            for (public, (version, signature)) in cases {
                let key_text = std::fs::read_to_string(public).unwrap();
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
                let correct = *public == key.with_extension("pub") && *version == Some("9.9.9");
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
        for download in [
            "http://127.0.0.1:8765/Windfall_0.2.0_x64-setup.exe",
            "http://[::1]/package",
        ] {
            let download = url::Url::parse(download).unwrap();
            assert!(
                policy(
                    "0.1.0",
                    "0.2.0",
                    UpdateChannel::Stable,
                    &stable,
                    "s",
                    &download
                )
                .is_ok()
            );
        }
        for download in [
            "http://localhost/package",
            "https://user:secret@updates.example.invalid/package",
        ] {
            let download = url::Url::parse(download).unwrap();
            assert!(
                policy(
                    "0.1.0",
                    "0.2.0",
                    UpdateChannel::Stable,
                    &stable,
                    "s",
                    &download
                )
                .is_err()
            );
        }
    }
    #[test]
    fn the_committed_configuration_has_the_key_and_the_stable_release_host_only() {
        let config: tauri::Config =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let config = &config;
        let (key, endpoint) = configured_source(config, UpdateChannel::Stable).unwrap();
        // The key is the base64 of a minisign public key file.
        assert!(key.starts_with("dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6"));
        assert_eq!(
            endpoint,
            "https://github.com/wheel338-sketch/windfall/releases/latest/download/latest.json"
        );
        let expanded = expand_endpoint(&endpoint, "1.0.1", "windows", "x86_64", "nsis").unwrap();
        assert_eq!(expanded.as_str(), endpoint);
        assert!(configured_source(config, UpdateChannel::Preview).is_err());
        let updater = &config.plugins.0["updater"];
        assert_eq!(updater["requireSignedVersion"], serde_json::json!(true));
        assert_eq!(updater["endpoints"].as_array().map(Vec::len), Some(1));
        let mut empty = config.clone();
        empty.plugins.0.remove("updater");
        assert!(configured_source(&empty, UpdateChannel::Stable).is_err());
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
