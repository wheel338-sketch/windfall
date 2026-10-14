//! The stdio bridge: clients that start a command (Claude Desktop, Claude
//! Code) speak MCP over the command's standard input and output, one JSON
//! message per line. The bridge relays each line to the running app's
//! loopback endpoint and writes each answer back as a line.
//!
//! It keeps what a legacy client's session needs between messages: the
//! version `initialize` settled on and the session id the app gave out.
//! When the app forgets the session (it was restarted, or MCP was turned
//! off and on), the bridge opens a new one with the client's original
//! `initialize` and sends the message again.

use std::io::{self, BufRead, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use crate::http::{bridge_error, client_request, read_response};

/// Requests relayed at once; more wait their turn.
const MAX_RUNNING: usize = 8;

/// How the bridge finds and reaches the app.
pub trait Link: Send + Sync + 'static {
    type Stream: Read + Write;
    /// The port and token, read from the app's settings each time so a
    /// new token takes effect. `Err` says why the bridge cannot connect.
    fn endpoint(&self) -> Result<(u16, String), String>;
    fn connect(&self, port: u16) -> io::Result<Self::Stream>;
}

#[derive(Default)]
struct State {
    version: Option<String>,
    session: Option<String>,
    initialize: Option<Value>,
}

pub struct Bridge<L: Link> {
    link: L,
    state: Mutex<State>,
}

pub const NOT_RUNNING: &str = "Windfall is not running, or its MCP server is off. Start Windfall and turn on Settings > AI agents (MCP).";

impl<L: Link> Bridge<L> {
    pub fn new(link: L) -> Self {
        Self {
            link,
            state: Mutex::new(State::default()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Whether the app answers, before any message is relayed.
    pub fn check(&self) -> Result<(), String> {
        let (port, _) = self.link.endpoint()?;
        self.link
            .connect(port)
            .map(drop)
            .map_err(|_| NOT_RUNNING.to_owned())
    }

    fn post(&self, message: &Value) -> Result<(u16, Option<String>, Vec<u8>), String> {
        let (port, token) = self.link.endpoint()?;
        let (version, session) = {
            let state = self.state();
            (state.version.clone(), state.session.clone())
        };
        let mut stream = self
            .link
            .connect(port)
            .map_err(|_| NOT_RUNNING.to_owned())?;
        stream
            .write_all(&client_request(
                port,
                &token,
                message,
                version.as_deref(),
                session.as_deref(),
            ))
            .and_then(|()| stream.flush())
            .map_err(|_| NOT_RUNNING.to_owned())?;
        let (status, headers, body) =
            read_response(&mut stream).map_err(|_| NOT_RUNNING.to_owned())?;
        Ok((status, headers.get("mcp-session-id").cloned(), body))
    }

    /// Relays one message and returns the line to answer with, if any.
    pub fn exchange(&self, message: &Value) -> Option<Value> {
        let id = message
            .get("id")
            .filter(|_| message.get("method").is_some());
        let initializing = message.get("method").and_then(Value::as_str) == Some("initialize");
        let mut outcome = self.post(message);
        let forgotten = matches!(&outcome, Ok((404, _, _)));
        if forgotten && !initializing {
            let had_session = self.state().session.is_some();
            if had_session && self.reopen() {
                outcome = self.post(message);
            }
        }
        let (status, session, body) = match outcome {
            Ok(answer) => answer,
            Err(error) => return id.map(|id| bridge_error(Some(id), &error)),
        };
        let answer: Option<Value> = serde_json::from_slice(&body).ok();
        if initializing && status == 200 {
            let mut state = self.state();
            state.version = answer
                .as_ref()
                .and_then(|answer| answer.pointer("/result/protocolVersion"))
                .and_then(Value::as_str)
                .map(str::to_owned);
            state.session = session;
            state.initialize = Some(message.clone());
        }
        let id = id?;
        match answer {
            Some(answer) if answer.get("id").is_some() || answer.get("error").is_some() => {
                let mut answer = answer;
                if answer.get("id").is_none() {
                    answer["id"] = id.clone();
                }
                Some(answer)
            }
            _ => Some(bridge_error(
                Some(id),
                &format!("Windfall answered HTTP {status} with no JSON-RPC message"),
            )),
        }
    }

    /// Opens a new session with the client's own `initialize`.
    fn reopen(&self) -> bool {
        let Some(mut initialize) = self.state().initialize.clone() else {
            return false;
        };
        initialize["id"] = json!("windfall-bridge-reinitialize");
        {
            let mut state = self.state();
            state.session = None;
            state.version = None;
        }
        let Ok((200, session, body)) = self.post(&initialize) else {
            return false;
        };
        let version = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|answer| answer.pointer("/result/protocolVersion").cloned())
            .and_then(|version| version.as_str().map(str::to_owned));
        let mut state = self.state();
        state.session = session;
        state.version = version;
        let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        drop(state);
        let _ = self.post(&note);
        true
    }
}

fn write_line(output: &Mutex<Box<dyn Write + Send>>, value: &Value) {
    let mut output = output
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = writeln!(output, "{value}");
    let _ = output.flush();
}

/// Relays every line of `input` until it ends, writing answers to
/// `output`. Requests other than `initialize` run side by side, so a long
/// render does not hold up a cancellation.
pub fn run<L: Link>(bridge: Arc<Bridge<L>>, input: impl BufRead, output: Box<dyn Write + Send>) {
    let output = Arc::new(Mutex::new(output));
    let running = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    for line in input.lines() {
        let Ok(line) = line else {
            break;
        };
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(_) => {
                write_line(
                    &output,
                    &json!({"jsonrpc": "2.0", "id": null, "error": {"code": crate::protocol::PARSE_ERROR, "message": "Parse error"}}),
                );
                continue;
            }
        };
        let request = message.get("id").is_some() && message.get("method").is_some();
        let initialize = message.get("method").and_then(Value::as_str) == Some("initialize");
        if request && !initialize && running.load(Ordering::Acquire) < MAX_RUNNING {
            running.fetch_add(1, Ordering::AcqRel);
            let (bridge, output, running) = (bridge.clone(), output.clone(), running.clone());
            workers.push(std::thread::spawn(move || {
                if let Some(answer) = bridge.exchange(&message) {
                    write_line(&output, &answer);
                }
                running.fetch_sub(1, Ordering::AcqRel);
            }));
            workers.retain(|worker| !worker.is_finished());
        } else if let Some(answer) = bridge.exchange(&message) {
            write_line(&output, &answer);
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
}
