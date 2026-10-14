//! JSON-RPC 2.0 and the Model Context Protocol methods Windfall serves,
//! with no transport: [`Server::handle`] takes one message and gives back
//! what to answer and with which HTTP status.
//!
//! Two eras of the protocol are served on one endpoint, as the
//! specification's backward-compatibility section describes:
//!
//! - **Modern**, revision `2026-07-28`: every request carries its version,
//!   client and capabilities in `params._meta`; there is no handshake and
//!   no session. Results carry `resultType` and the server's identity.
//! - **Legacy**, revisions `2025-11-25`, `2025-06-18` and `2025-03-26`:
//!   the client opens with `initialize`, the server answers with a session
//!   id, and later requests carry that id and the negotiated version in
//!   headers. The session only remembers the client's name, for the undo
//!   label and the approval panel.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Map, Value, json};

use crate::approvals::Approvals;
use crate::host::Host;
use crate::tools::{self, Call};

pub const MODERN: &str = "2026-07-28";
/// Newest first: an `initialize` asking for a version not here is answered
/// with the first.
pub const LEGACY: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];
/// What a legacy request without a version header is taken to speak.
pub const UNSTATED: &str = "2025-03-26";

pub const META_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
pub const META_CLIENT: &str = "io.modelcontextprotocol/clientInfo";
pub const META_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
pub const META_SERVER: &str = "io.modelcontextprotocol/serverInfo";

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
pub const HEADER_MISMATCH: i64 = -32020;
pub const UNSUPPORTED_VERSION: i64 = -32022;
/// Resource not found, as revisions up to 2025-11-25 have it.
pub const LEGACY_RESOURCE_NOT_FOUND: i64 = -32002;

pub const SUMMARY_URI: &str = "windfall://project/summary";

/// Legacy sessions kept at most; the oldest is forgotten first.
const MAX_SESSIONS: usize = 64;
/// Requests a client may have running at once.
const MAX_RUNNING: usize = 16;

/// The name used for a client that gives none.
pub const UNNAMED_AGENT: &str = "AI agent";

const INSTRUCTIONS: &str = "Windfall is a digital audio workstation. Call get_project_summary first: it gives the tempo, meter and the ids of channels, patterns, playlist tracks, clips, mixer tracks and effects that every other tool takes. Positions are bar:beat:tick with bars and beats counted from 1 (960 ticks are a quarter note); pitches are note names such as C4 or MIDI numbers. Edit tools never change the project themselves: each files a proposal the user applies or rejects in Windfall, as one undo step, unless the user lets you apply without asking. Use get_proposal with waitSeconds to learn the decision, and read the project again after an edit is applied. render_and_measure renders offline and returns loudness, peaks, spectrum, tempo and key.";

/// One message as a transport received it.
pub struct Incoming<'a> {
    pub message: &'a Value,
    /// The `MCP-Protocol-Version` header, if the transport has headers.
    pub version_header: Option<&'a str>,
    /// The `Mcp-Session-Id` header.
    pub session: Option<&'a str>,
}

/// What to answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Outgoing {
    pub status: u16,
    /// No body for an accepted notification.
    pub body: Option<Value>,
    /// A session id to send as `Mcp-Session-Id`.
    pub session: Option<String>,
}

impl Outgoing {
    fn accepted() -> Self {
        Self {
            status: 202,
            body: None,
            session: None,
        }
    }
}

/// A JSON-RPC error response.
pub fn error(id: Option<&Value>, code: i64, message: &str, data: Option<Value>) -> Value {
    let mut error = json!({"code": code, "message": message});
    if let Some(data) = data {
        error["data"] = data;
    }
    let mut response = json!({"jsonrpc": "2.0", "error": error});
    if let Some(id) = id {
        response["id"] = id.clone();
    }
    response
}

fn result(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

struct Session {
    id: String,
    agent: String,
    version: &'static str,
}

/// Which protocol a request speaks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Era {
    Modern,
    Legacy(&'static str),
}

impl Era {
    /// Whether results may carry `structuredContent` and resource links,
    /// which arrived in 2025-06-18.
    fn structured(self) -> bool {
        !matches!(self, Era::Legacy("2025-03-26"))
    }
}

pub struct Server {
    version: String,
    approvals: Approvals,
    sessions: Mutex<VecDeque<Session>>,
    running: Mutex<HashMap<(String, String), Arc<AtomicBool>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// An agent's name as its client gives it: title, else name, printable
/// and at most 40 characters.
pub fn agent_name(info: Option<&Value>) -> String {
    let given = info
        .and_then(|info| {
            info.get("title")
                .and_then(Value::as_str)
                .filter(|title| !title.trim().is_empty())
                .or_else(|| info.get("name").and_then(Value::as_str))
        })
        .unwrap_or("");
    let clean: String = given.chars().filter(|c| !c.is_control()).take(40).collect();
    let clean = clean.trim();
    if clean.is_empty() {
        UNNAMED_AGENT.to_owned()
    } else {
        clean.to_owned()
    }
}

fn id_key(id: &Value) -> String {
    id.to_string()
}

impl Server {
    /// `version` is Windfall's, for the server's identity.
    pub fn new(version: &str) -> Self {
        Self {
            version: version.to_owned(),
            approvals: Approvals::new(),
            sessions: Mutex::new(VecDeque::new()),
            running: Mutex::new(HashMap::new()),
        }
    }

    pub fn approvals(&self) -> &Approvals {
        &self.approvals
    }

    fn server_info(&self) -> Value {
        json!({"name": "windfall", "title": "Windfall", "version": self.version})
    }

    fn capabilities() -> Value {
        json!({
            "tools": {"listChanged": false},
            "resources": {"listChanged": false, "subscribe": false},
        })
    }

    /// Forgets every legacy session and cancels what runs: the server was
    /// turned off, or its token changed.
    pub fn reset(&self) {
        lock(&self.sessions).clear();
        for flag in lock(&self.running).values() {
            flag.store(true, Ordering::Release);
        }
        self.approvals.clear();
    }

    /// Whether the session id is one this server gave out.
    pub fn knows_session(&self, id: &str) -> bool {
        lock(&self.sessions).iter().any(|session| session.id == id)
    }

    /// Ends a legacy session, as an HTTP DELETE asks.
    pub fn end_session(&self, id: &str) -> bool {
        let mut sessions = lock(&self.sessions);
        let before = sessions.len();
        sessions.retain(|session| session.id != id);
        sessions.len() != before
    }

    pub fn handle(&self, host: &dyn Host, incoming: Incoming<'_>) -> Outgoing {
        let Some(message) = incoming.message.as_object() else {
            let body = if incoming.message.is_array() {
                "Batched messages are not supported; send one message per request"
            } else {
                "Invalid Request: a message must be a JSON object"
            };
            return Outgoing {
                status: 400,
                body: Some(error(None, INVALID_REQUEST, body, None)),
                session: None,
            };
        };
        if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Outgoing {
                status: 400,
                body: Some(error(
                    message.get("id"),
                    INVALID_REQUEST,
                    "Invalid Request: jsonrpc must be \"2.0\"",
                    None,
                )),
                session: None,
            };
        }
        let Some(method) = message.get("method").and_then(Value::as_str) else {
            // A response to a request this server never sends.
            return Outgoing::accepted();
        };
        let params = message.get("params");
        if params.is_some_and(|params| !params.is_object()) {
            return Outgoing {
                status: 400,
                body: Some(error(
                    message.get("id"),
                    INVALID_REQUEST,
                    "Invalid Request: params must be an object",
                    None,
                )),
                session: None,
            };
        }
        let params = params.and_then(Value::as_object);
        let Some(id) = message.get("id") else {
            self.notification(method, params, &incoming);
            return Outgoing::accepted();
        };
        if !(id.is_string() || id.is_i64() || id.is_u64()) {
            return Outgoing {
                status: 400,
                body: Some(error(
                    None,
                    INVALID_REQUEST,
                    "Invalid Request: id must be a string or an integer",
                    None,
                )),
                session: None,
            };
        }
        let meta = params
            .and_then(|params| params.get("_meta"))
            .and_then(Value::as_object);
        let modern = meta.and_then(|meta| meta.get(META_VERSION));
        if method == "initialize" {
            return self.initialize(host, id, params);
        }
        let (era, agent) = match modern {
            Some(version) => {
                let Some(version) = version.as_str() else {
                    return bad(
                        id,
                        INVALID_PARAMS,
                        "Invalid params: the protocol version must be text",
                    );
                };
                if version != MODERN {
                    let mut supported = vec![MODERN];
                    supported.extend(LEGACY);
                    return Outgoing {
                        status: 400,
                        body: Some(error(
                            Some(id),
                            UNSUPPORTED_VERSION,
                            "Unsupported protocol version",
                            Some(json!({"supported": supported, "requested": version})),
                        )),
                        session: None,
                    };
                }
                if !meta
                    .is_some_and(|meta| meta.get(META_CAPABILITIES).is_some_and(Value::is_object))
                {
                    return bad(
                        id,
                        INVALID_PARAMS,
                        "Invalid params: _meta must carry io.modelcontextprotocol/clientCapabilities",
                    );
                }
                (
                    Era::Modern,
                    agent_name(meta.and_then(|meta| meta.get(META_CLIENT))),
                )
            }
            None => {
                let session = match incoming.session {
                    Some(given) => {
                        let sessions = lock(&self.sessions);
                        match sessions.iter().find(|session| session.id == given) {
                            Some(session) => Some((session.agent.clone(), session.version)),
                            None => {
                                return Outgoing {
                                    status: 404,
                                    body: Some(error(
                                        Some(id),
                                        INVALID_REQUEST,
                                        "Session not found; initialize again",
                                        None,
                                    )),
                                    session: None,
                                };
                            }
                        }
                    }
                    None => None,
                };
                let version = match incoming.version_header {
                    Some(header) => match LEGACY.iter().find(|known| **known == header) {
                        Some(known) => *known,
                        None => {
                            let mut supported = vec![MODERN];
                            supported.extend(LEGACY);
                            return Outgoing {
                                status: 400,
                                body: Some(error(
                                    Some(id),
                                    UNSUPPORTED_VERSION,
                                    "Unsupported protocol version",
                                    Some(json!({"supported": supported, "requested": header})),
                                )),
                                session: None,
                            };
                        }
                    },
                    None => session.as_ref().map_or(UNSTATED, |(_, version)| *version),
                };
                (
                    Era::Legacy(version),
                    session.map_or_else(|| UNNAMED_AGENT.to_owned(), |(agent, _)| agent),
                )
            }
        };
        let answer = self.request(host, era, &agent, method, id, params);
        match answer {
            Ok(mut value) => {
                if era == Era::Modern {
                    value["resultType"] = json!("complete");
                    match value.get_mut("_meta").and_then(Value::as_object_mut) {
                        Some(meta) => {
                            meta.insert(META_SERVER.into(), self.server_info());
                        }
                        None => value["_meta"] = json!({(META_SERVER): self.server_info()}),
                    }
                }
                Outgoing {
                    status: 200,
                    body: Some(result(id, value)),
                    session: None,
                }
            }
            Err((code, message)) => Outgoing {
                status: if era == Era::Modern && code == METHOD_NOT_FOUND {
                    404
                } else {
                    200
                },
                body: Some(error(Some(id), code, &message, None)),
                session: None,
            },
        }
    }

    fn notification(
        &self,
        method: &str,
        params: Option<&Map<String, Value>>,
        incoming: &Incoming<'_>,
    ) {
        if method != "notifications/cancelled" {
            return;
        }
        let Some(request) = params.and_then(|params| params.get("requestId")) else {
            return;
        };
        let meta = params
            .and_then(|params| params.get("_meta"))
            .and_then(Value::as_object);
        let agent = match meta.and_then(|meta| meta.get(META_CLIENT)) {
            Some(info) => agent_name(Some(info)),
            None => incoming
                .session
                .and_then(|given| {
                    lock(&self.sessions)
                        .iter()
                        .find(|session| session.id == given)
                        .map(|session| session.agent.clone())
                })
                .unwrap_or_else(|| UNNAMED_AGENT.to_owned()),
        };
        if let Some(flag) = lock(&self.running).get(&(agent, id_key(request))) {
            flag.store(true, Ordering::Release);
        }
    }

    fn initialize(
        &self,
        host: &dyn Host,
        id: &Value,
        params: Option<&Map<String, Value>>,
    ) -> Outgoing {
        let asked = params
            .and_then(|params| params.get("protocolVersion"))
            .and_then(Value::as_str);
        let version = LEGACY
            .iter()
            .find(|known| Some(**known) == asked)
            .copied()
            .unwrap_or(LEGACY[0]);
        let agent = agent_name(params.and_then(|params| params.get("clientInfo")));
        let session = host.random_hex();
        {
            let mut sessions = lock(&self.sessions);
            sessions.push_back(Session {
                id: session.clone(),
                agent: agent.clone(),
                version,
            });
            while sessions.len() > MAX_SESSIONS {
                sessions.pop_front();
            }
        }
        self.approvals.seen(&agent);
        Outgoing {
            status: 200,
            body: Some(result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": Self::capabilities(),
                    "serverInfo": self.server_info(),
                    "instructions": INSTRUCTIONS,
                }),
            )),
            session: Some(session),
        }
    }

    fn request(
        &self,
        host: &dyn Host,
        era: Era,
        agent: &str,
        method: &str,
        id: &Value,
        params: Option<&Map<String, Value>>,
    ) -> Result<Value, (i64, String)> {
        match method {
            "ping" => Ok(json!({})),
            "server/discover" => {
                let mut supported = vec![MODERN];
                supported.extend(LEGACY);
                Ok(json!({
                    "supportedVersions": supported,
                    "capabilities": Self::capabilities(),
                    "_meta": {(META_SERVER): self.server_info()},
                    "instructions": INSTRUCTIONS,
                }))
            }
            "tools/list" => Ok(json!({
                "tools": tools::TOOLS.iter().map(tools::Tool::definition).collect::<Vec<_>>(),
            })),
            "tools/call" => self.call_tool(host, era, agent, id, params),
            "resources/list" => Ok(json!({
                "resources": [{
                    "uri": SUMMARY_URI,
                    "name": "project-summary",
                    "title": "Project summary",
                    "description": "The open project at a glance, as get_project_summary gives it.",
                    "mimeType": "application/json",
                }],
            })),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "resources/read" => {
                let uri = params
                    .and_then(|params| params.get("uri"))
                    .and_then(Value::as_str)
                    .ok_or((INVALID_PARAMS, "Invalid params: uri is required".to_owned()))?;
                if uri != SUMMARY_URI {
                    let code = match era {
                        Era::Modern => INVALID_PARAMS,
                        Era::Legacy(_) => LEGACY_RESOURCE_NOT_FOUND,
                    };
                    return Err((code, format!("Resource not found: {uri}")));
                }
                let snapshot = host.snapshot().map_err(|error| (INTERNAL_ERROR, error))?;
                Ok(json!({
                    "contents": [{
                        "uri": SUMMARY_URI,
                        "mimeType": "application/json",
                        "text": crate::summary::project_summary(&snapshot).to_string(),
                    }],
                }))
            }
            _ => Err((METHOD_NOT_FOUND, format!("Method not found: {method}"))),
        }
    }

    fn call_tool(
        &self,
        host: &dyn Host,
        era: Era,
        agent: &str,
        id: &Value,
        params: Option<&Map<String, Value>>,
    ) -> Result<Value, (i64, String)> {
        let name = params
            .and_then(|params| params.get("name"))
            .and_then(Value::as_str)
            .ok_or((
                INVALID_PARAMS,
                "Invalid params: name is required".to_owned(),
            ))?;
        let tool =
            tools::find(name).ok_or_else(|| (INVALID_PARAMS, format!("Unknown tool: {name}")))?;
        let key = (agent.to_owned(), id_key(id));
        let flag = Arc::new(AtomicBool::new(false));
        {
            let mut running = lock(&self.running);
            if running.keys().filter(|(owner, _)| owner == agent).count() >= MAX_RUNNING {
                return Err((
                    INTERNAL_ERROR,
                    format!("{MAX_RUNNING} requests of this agent are already running"),
                ));
            }
            running.insert(key.clone(), flag.clone());
        }
        let cancelled = || flag.load(Ordering::Acquire);
        let call = Call {
            host,
            approvals: &self.approvals,
            agent,
            cancelled: &cancelled,
        };
        let outcome = tools::run(
            tool,
            &call,
            params.and_then(|params| params.get("arguments")),
        );
        lock(&self.running).remove(&key);
        Ok(match outcome {
            Ok(reply) => {
                let mut content = vec![json!({"type": "text", "text": reply.value.to_string()})];
                let mut answer = json!({"isError": false});
                if era.structured() {
                    content.extend(reply.links);
                    answer["structuredContent"] = reply.value;
                }
                answer["content"] = json!(content);
                answer
            }
            Err(message) => json!({
                "content": [{"type": "text", "text": message}],
                "isError": true,
            }),
        })
    }
}

fn bad(id: &Value, code: i64, message: &str) -> Outgoing {
    Outgoing {
        status: 400,
        body: Some(error(Some(id), code, message, None)),
        session: None,
    }
}
