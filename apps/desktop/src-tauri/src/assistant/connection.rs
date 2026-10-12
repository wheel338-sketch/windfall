use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use windfall_ipc::{AssistantConnection, AssistantConnectionUpdate};

/// Authorized fallback where no OS credential-store backend is integrated.
/// Never Debug; never embed this in Settings/Project/IPC responses or log errors.
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub(super) struct Connection {
    pub base_url: String,
    pub model: String,
    pub enabled: bool,
    pub key: String,
}
impl Connection {
    pub fn status(&self) -> AssistantConnection {
        AssistantConnection {
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            enabled: self.enabled && !self.key.is_empty(),
            has_key: !self.key.is_empty(),
        }
    }
    pub fn update(&mut self, update: AssistantConnectionUpdate) -> Result<(), String> {
        let key = update
            .api_key
            .unwrap_or_else(|| self.key.clone())
            .trim()
            .to_owned();
        if key.len() > 4096 || key.chars().any(char::is_control) {
            return Err("assistant key is invalid or too long".into());
        }
        let model = update.model.trim().to_owned();
        if model.len() > 256 || model.chars().any(char::is_control) {
            return Err("assistant model is invalid or too long".into());
        }
        let base_url = update.base_url.trim().to_owned();
        // Clearing a key/turning OFF remains possible without a working endpoint.
        if update.enabled && !key.is_empty() {
            endpoint(&base_url)?;
            if model.is_empty() {
                return Err("enter an assistant model name".into());
            }
        } else if base_url.len() > 2048 {
            return Err("assistant endpoint is too long".into());
        }
        *self = Self {
            base_url,
            model,
            enabled: update.enabled && !key.is_empty(),
            key,
        };
        Ok(())
    }
    pub fn ready(&self) -> Result<reqwest::Url, String> {
        if !self.enabled || self.key.is_empty() {
            return Err("BYOK assistant is OFF; enter a key and enable it first".into());
        }
        if self.key.len() > 4096
            || self.key.chars().any(char::is_control)
            || self.model.is_empty()
            || self.model.len() > 256
            || self.model.chars().any(char::is_control)
        {
            return Err("assistant connection settings are invalid".into());
        }
        endpoint(&self.base_url)
    }
}

pub(super) fn endpoint(base: &str) -> Result<reqwest::Url, String> {
    if base.len() > 2048 {
        return Err("assistant endpoint is too long".into());
    }
    let mut url =
        reqwest::Url::parse(base).map_err(|_| "enter an absolute HTTPS assistant base URL")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("assistant URL must not contain credentials, query or fragment".into());
    }
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "[::1]" | "::1"));
    if url.scheme() != "https" && !(url.scheme() == "http" && loopback) {
        return Err(
            "assistant endpoint requires HTTPS (HTTP only for loopback testing/local services)"
                .into(),
        );
    }
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/chat/completions") {
        url.set_path(&format!("{path}/chat/completions"));
    }
    Ok(url)
}

pub(super) fn load(file: &Path) -> Result<Connection, String> {
    let mut file = match fs::File::open(file) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Connection::default()),
        Err(_) => return Err("could not read per-user assistant settings".into()),
    };
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(16_385)
        .read_to_end(&mut bytes)
        .map_err(|_| "could not read per-user assistant settings")?;
    if bytes.len() > 16_384 {
        return Err("per-user assistant settings exceed the byte limit".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        "per-user assistant settings are malformed; reset that file to reconfigure".into()
    })
}
pub(super) fn save(file: &Path, config: &Connection) -> Result<(), String> {
    let folder = file
        .parent()
        .ok_or("assistant settings must have a per-user directory")?;
    fs::create_dir_all(folder).map_err(|_| "could not create per-user assistant directory")?;
    // NamedTempFile is private (0600 on Unix); persist preserves its permissions.
    // Windows inherits the current user's app-config directory ACL.
    let mut staging = tempfile::NamedTempFile::new_in(folder)
        .map_err(|_| "could not stage per-user assistant settings")?;
    let bytes =
        serde_json::to_vec(config).map_err(|_| "could not encode per-user assistant settings")?;
    staging
        .write_all(&bytes)
        .and_then(|_| staging.as_file().sync_all())
        .map_err(|_| "could not write per-user assistant settings")?;
    staging
        .persist(file)
        .map_err(|_| "could not commit per-user assistant settings")?;
    Ok(())
}
