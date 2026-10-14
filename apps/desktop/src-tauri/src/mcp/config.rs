//! The MCP server's per-install settings: whether it is on, its port and
//! its bearer token, in `mcp.json` beside the settings file. The running
//! app writes it; the stdio bridge, another process, reads it.
//!
//! Never Debug: the token must not reach a log.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const FILE: &str = "mcp.json";
/// The port the server listens on until the user picks another.
pub const DEFAULT_PORT: u16 = 47_133;
/// Ports below this need privileges on some systems.
pub const LOWEST_PORT: u16 = 1024;

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub enabled: bool,
    pub port: u16,
    pub token: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            token: String::new(),
        }
    }
}

/// A new bearer token: 32 random bytes as hex.
pub fn new_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|_| "Windfall could not make a random token for the MCP server.")?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The settings in `file`, the defaults when there is none.
pub fn load(file: &Path) -> Result<Config, String> {
    let mut opened = match fs::File::open(file) {
        Ok(opened) => opened,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Config::default()),
        Err(_) => return Err("Windfall could not read the MCP settings file.".into()),
    };
    let mut bytes = Vec::new();
    Read::by_ref(&mut opened)
        .take(16_385)
        .read_to_end(&mut bytes)
        .map_err(|_| "Windfall could not read the MCP settings file.")?;
    if bytes.len() > 16_384 {
        return Err("The MCP settings file is too large; delete it to start over.".into());
    }
    let config: Config = serde_json::from_slice(&bytes)
        .map_err(|_| "The MCP settings file is damaged; delete it to start over.")?;
    if config.token.len() > 256 || !config.token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("The MCP settings file is damaged; delete it to start over.".into());
    }
    Ok(config)
}

/// Writes the settings privately: the staging file is readable by this
/// user only on Unix, and inherits the per-user folder's rights on
/// Windows, as the assistant's connection file does.
pub fn save(file: &Path, config: &Config) -> Result<(), String> {
    let folder = file
        .parent()
        .ok_or("The MCP settings need a per-user folder.")?;
    fs::create_dir_all(folder).map_err(|_| "Windfall could not create its settings folder.")?;
    let mut staging = tempfile::NamedTempFile::new_in(folder)
        .map_err(|_| "Windfall could not write the MCP settings file.")?;
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|_| "Windfall could not write the MCP settings file.")?;
    staging
        .write_all(&bytes)
        .and_then(|()| staging.as_file().sync_all())
        .map_err(|_| "Windfall could not write the MCP settings file.")?;
    staging
        .persist(file)
        .map_err(|_| "Windfall could not write the MCP settings file.")?;
    Ok(())
}

/// The settings, with a token made and kept if there was none yet.
pub fn load_with_token(file: &Path) -> Result<Config, String> {
    let mut config = load(file)?;
    if config.token.is_empty() {
        config.token = new_token()?;
        save(file, &config)?;
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_is_made_once_per_install_and_kept_private() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join(FILE);
        let first = load_with_token(&file).unwrap();
        assert!(!first.enabled, "the server is off by default");
        assert_eq!(first.port, DEFAULT_PORT);
        assert_eq!(first.token.len(), 64);
        assert!(first.token.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(load_with_token(&file).unwrap() == first);
        assert_ne!(new_token().unwrap(), first.token);
        std::fs::write(&file, b"{\"token\": \"not hex!\"}").unwrap();
        assert!(load(&file).is_err());
        std::fs::write(&file, b"[").unwrap();
        assert!(load(&file).is_err());
        assert!(load(&folder.path().join("missing.json")).unwrap() == Config::default());
    }
}
