//! Provider configuration is per-user, never project data. Reads never return keys.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AssistantConnection {
    pub base_url: String,
    pub model: String,
    pub enabled: bool,
    pub has_key: bool,
}

// Intentionally no Debug: neither IPC diagnostics nor logs may print a key.
#[derive(Clone, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct AssistantConnectionUpdate {
    pub base_url: String,
    pub model: String,
    pub enabled: bool,
    /// None keeps the existing key; an empty string removes it and turns OFF.
    pub api_key: Option<String>,
}
