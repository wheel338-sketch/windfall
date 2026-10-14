//! `windfall-desktop --mcp-stdio`: the bridge for MCP clients that start a
//! command. It runs before Tauri does, opens no window, and relays standard
//! input and output to the running app's loopback endpoint with the token
//! from the app's `mcp.json`.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use windfall_mcp::bridge::{self, Bridge, Link};

use super::config;

pub const FLAG: &str = "--mcp-stdio";

/// tauri.conf.json's `identifier`: the name of the app's config folder.
pub const IDENTIFIER: &str = "org.windfall.Windfall";

const OFF: &str = "The MCP server is off in Windfall. Turn it on in Settings > AI agents (MCP).";

/// The folder Tauri's `app_config_dir` names, worked out without Tauri:
/// the system's config folder and the app's identifier. `WINDFALL_CONFIG_DIR`
/// or `--config-dir` name another, as for the app itself.
fn config_dir(given: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(given) = given {
        return Some(given);
    }
    if let Some(dir) = std::env::var_os(crate::shell::CONFIG_DIR_ENV) {
        return Some(PathBuf::from(dir));
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(base.join(IDENTIFIER))
}

/// Reaches the app over TCP with the port and token of its `mcp.json`.
pub(crate) struct Tcp {
    pub(crate) file: PathBuf,
}

impl Link for Tcp {
    type Stream = TcpStream;

    fn endpoint(&self) -> Result<(u16, String), String> {
        let config = config::load(&self.file)?;
        if !config.enabled || config.token.is_empty() {
            return Err(OFF.into());
        }
        Ok((config.port, config.token))
    }

    fn connect(&self, port: u16) -> io::Result<TcpStream> {
        let stream = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            Duration::from_secs(2),
        )?;
        // A render or a wait for the user's decision takes a while.
        stream.set_read_timeout(Some(Duration::from_secs(300)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;
        Ok(stream)
    }
}

/// Runs the bridge when the executable was started with [`FLAG`], and
/// returns the process's exit code; `None` to start the app as usual.
pub fn entry() -> Option<i32> {
    let mut args = std::env::args_os().skip(1);
    if args.next()? != FLAG {
        return None;
    }
    let mut given = None;
    while let Some(arg) = args.next() {
        if arg == "--config-dir" {
            given = args.next().map(PathBuf::from);
        }
    }
    let Some(dir) = config_dir(given) else {
        eprintln!(
            "windfall --mcp-stdio: no config folder; set WINDFALL_CONFIG_DIR or pass --config-dir"
        );
        return Some(2);
    };
    let bridge = Arc::new(Bridge::new(Tcp {
        file: dir.join(config::FILE),
    }));
    if let Err(error) = bridge.check() {
        eprintln!("windfall --mcp-stdio: {error}");
        return Some(1);
    }
    bridge::run(bridge, io::stdin().lock(), Box::new(io::stdout()));
    Some(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bridge_finds_the_folder_the_app_keeps_its_settings_in() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        assert_eq!(conf["identifier"], IDENTIFIER);
        let given = PathBuf::from("somewhere");
        assert_eq!(config_dir(Some(given.clone())), Some(given));
    }

    #[test]
    fn the_bridge_says_when_the_server_is_off_or_the_app_is_not_running() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join(config::FILE);
        let link = Tcp { file: file.clone() };
        assert_eq!(link.endpoint().unwrap_err(), OFF);
        // On, but nothing listens on the port.
        let free = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = free.local_addr().unwrap().port();
        drop(free);
        config::save(
            &file,
            &config::Config {
                enabled: true,
                port,
                token: "ab".repeat(32),
            },
        )
        .unwrap();
        assert_eq!(link.endpoint().unwrap().0, port);
        let relay = Bridge::new(link);
        assert_eq!(relay.check(), Err(bridge::NOT_RUNNING.to_owned()));
    }
}
