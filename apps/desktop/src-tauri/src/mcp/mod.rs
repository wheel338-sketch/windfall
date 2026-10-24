//! The MCP server inside the app: its settings, its loopback listener and
//! the stdio bridge. What it serves is `windfall_mcp`; how a tool reaches
//! the project is `session::mcp`, through the same checked proposals the
//! assistant uses. docs/MCP.md describes it for users.

pub mod config;
mod listener;
pub mod stdio;

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use windfall_mcp::{Focus, Host, Server};

use crate::sync::lock;

pub use stdio::entry as stdio_entry;

/// The server's state for the session's life.
pub struct Service {
    pub(crate) plugin_failure: Mutex<Option<serde_json::Value>>,
    pub(crate) server: Arc<Server>,
    listener: Mutex<Option<listener::Listener>>,
    /// Held while the server is turned on, off or moved to another port.
    pub(crate) configuring: Mutex<()>,
    /// Held while the panel is built and sent, so panels reach the window
    /// in the order they were built.
    pub(crate) emitting: Mutex<()>,
    pub(crate) focus: Mutex<Focus>,
    /// The cancel flag of the render that runs, which the panel can set.
    pub(crate) render: Mutex<Option<Arc<AtomicBool>>>,
    /// Why the server is not listening although it is turned on.
    pub(crate) error: Mutex<Option<String>>,
    /// Set while a panel refresh after a project change is on its way.
    pub(crate) refreshing: AtomicBool,
}

impl Default for Service {
    fn default() -> Self {
        Self {
            plugin_failure: Mutex::new(None),
            server: Arc::new(Server::new(env!("CARGO_PKG_VERSION"))),
            listener: Mutex::new(None),
            configuring: Mutex::new(()),
            emitting: Mutex::new(()),
            focus: Mutex::new(Focus::default()),
            render: Mutex::new(None),
            error: Mutex::new(None),
            refreshing: AtomicBool::new(false),
        }
    }
}

impl Service {
    /// The port the server listens on, if it does.
    pub fn listening(&self) -> Option<u16> {
        lock(&self.listener).as_ref().map(|listener| listener.port)
    }

    /// Starts listening, replacing a listener that runs.
    pub(crate) fn listen(
        &self,
        host: Arc<dyn Host>,
        alive: Box<dyn Fn() -> bool + Send>,
        port: u16,
        token: String,
    ) -> Result<(), String> {
        let mut current = lock(&self.listener);
        current.take();
        let started = listener::start(self.server.clone(), host, alive, port, token);
        match started {
            Ok(listening) => {
                *current = Some(listening);
                *lock(&self.error) = None;
                Ok(())
            }
            Err(error) => {
                *lock(&self.error) = Some(error.clone());
                Err(error)
            }
        }
    }

    /// Stops listening. With `forget`, sessions end, what runs is cancelled,
    /// waiting proposals are withdrawn and every allowance ends.
    pub(crate) fn stop(&self, forget: bool) {
        lock(&self.listener).take();
        if forget {
            self.server.reset();
            *lock(&self.focus) = Focus::default();
        }
    }
}
