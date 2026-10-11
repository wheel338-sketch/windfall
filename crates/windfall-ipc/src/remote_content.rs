//! Optional desktop capabilities; no network or provider is enabled by default.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoteSettingsState {
    pub enabled: bool,
    pub address: Option<String>,
    pub pairing_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct BackupVersion {
    pub id: String,
    pub parent: Option<String>,
    pub checksum: String,
    pub bytes: u64,
    pub created_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoopEntry {
    pub id: String,
    pub name: String,
    pub genre: String,
    pub tempo: u16,
    pub key: u8,
    pub mode: String,
    pub bars: u8,
    pub provenance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LoopCatalog {
    pub generation: u64,
    pub revision: u64,
    pub entries: Vec<LoopEntry>,
}
