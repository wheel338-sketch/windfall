//! The Streamable HTTP transport, as far as Windfall serves it: one
//! endpoint, `POST /mcp`, answering every request with one JSON object.
//! No server-sent events, no standalone `GET` stream.
//!
//! The listener is bound to `127.0.0.1` only. Every request must name that
//! host (a page that rebinds a domain to the loopback address sends its own
//! domain), must carry no `Origin` but the endpoint's own, and must present
//! the install's bearer token. Reading is bounded in size and time.
//!
//! Sockets are the caller's: [`read_request`] reads from anything that
//! reads, and [`respond`] is a pure function of the request.

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::host::Host;
use crate::protocol::{self, HEADER_MISMATCH, Incoming, META_VERSION, PARSE_ERROR, Server};

pub const ENDPOINT: &str = "/mcp";
pub const MAX_HEADER: usize = 16 * 1024;
/// The largest request body: a raw command batch is at most 512 KiB.
pub const MAX_BODY: usize = 1024 * 1024;
/// How long a client has to send one whole request.
pub const READ_TIME: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: String,
    pub path: String,
    /// Names in lower case.
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

/// A request that could not be read, and the status to refuse it with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal(pub u16);

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        408 => "Request Timeout",
        411 => "Length Required",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        503 => "Service Unavailable",
        _ => "Error",
    }
}

/// Reads one request. The caller sets the socket's own timeouts; this
/// also gives up once `deadline` has passed.
pub fn read_request(input: &mut impl Read, deadline: Instant) -> Result<HttpRequest, Refusal> {
    let late = || Instant::now() > deadline;
    let mut raw = Vec::with_capacity(512);
    let mut byte = [0u8];
    loop {
        if late() {
            return Err(Refusal(408));
        }
        if raw.len() >= MAX_HEADER {
            return Err(Refusal(431));
        }
        match input.read(&mut byte) {
            Ok(0) => return Err(Refusal(400)),
            Ok(_) => raw.push(byte[0]),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Err(Refusal(408));
            }
            Err(_) => return Err(Refusal(400)),
        }
        if raw.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let head = std::str::from_utf8(&raw).map_err(|_| Refusal(400))?;
    let mut lines = head.split("\r\n");
    let parts: Vec<&str> = lines.next().unwrap_or("").split(' ').collect();
    if parts.len() != 3 || parts[2] != "HTTP/1.1" || parts[0].is_empty() {
        return Err(Refusal(400));
    }
    let mut headers = BTreeMap::new();
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(Refusal(400))?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(Refusal(400));
        }
        if headers
            .insert(name.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err(Refusal(400));
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err(Refusal(501));
    }
    let size = match headers.get("content-length") {
        Some(size) => size.parse::<usize>().map_err(|_| Refusal(400))?,
        None if parts[0] == "POST" => return Err(Refusal(411)),
        None => 0,
    };
    if size > MAX_BODY {
        return Err(Refusal(413));
    }
    let mut body = vec![0; size];
    let mut filled = 0;
    while filled < size {
        if late() {
            return Err(Refusal(408));
        }
        match input.read(&mut body[filled..]) {
            Ok(0) => return Err(Refusal(400)),
            Ok(read) => filled += read,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(Refusal(408)),
        }
    }
    Ok(HttpRequest {
        method: parts[0].to_owned(),
        path: parts[1].to_owned(),
        headers,
        body,
    })
}

pub fn write_response(out: &mut impl Write, response: &HttpResponse) -> io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        response.status,
        reason(response.status)
    );
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("Content-Length: ");
    head.push_str(&response.body.len().to_string());
    head.push_str("\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n");
    out.write_all(head.as_bytes())?;
    out.write_all(&response.body)?;
    out.flush()
}

fn json_response(status: u16, body: &Value) -> HttpResponse {
    HttpResponse {
        status,
        headers: vec![("Content-Type", "application/json".to_owned())],
        body: body.to_string().into_bytes(),
    }
}

/// A refusal with a JSON-RPC error body that has no id.
pub fn refused(status: u16, message: &str) -> HttpResponse {
    let mut response = json_response(
        status,
        &protocol::error(None, protocol::INVALID_REQUEST, message, None),
    );
    if status == 401 {
        response
            .headers
            .push(("WWW-Authenticate", "Bearer realm=\"Windfall\"".to_owned()));
    }
    if status == 405 {
        response.headers.push(("Allow", "POST, DELETE".to_owned()));
    }
    response
}

/// What a refused read is answered with.
pub fn unreadable(refusal: Refusal) -> HttpResponse {
    let message = match refusal.0 {
        408 => "Request timeout",
        411 => "Content-Length is required",
        413 => "Request body is too large",
        431 => "Request headers are too large",
        501 => "Chunked transfer encoding is not supported",
        _ => "Malformed HTTP request",
    };
    refused(refusal.0, message)
}

fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// The `=?base64?…?=` form a header value takes when it cannot be plain.
fn header_text(value: &str) -> Option<String> {
    match value
        .strip_prefix("=?base64?")
        .and_then(|rest| rest.strip_suffix("?="))
    {
        Some(encoded) => String::from_utf8(base64(encoded)?).ok(),
        None => Some(value.to_owned()),
    }
}

fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut bits = 0u32;
    let mut count = 0;
    for c in text.bytes().filter(|c| *c != b'=') {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    Some(out)
}

/// Where the server listens and what it takes to be let in.
pub struct Endpoint<'a> {
    pub port: u16,
    pub token: &'a str,
}

impl Endpoint<'_> {
    fn hosts(&self) -> [String; 2] {
        [
            format!("127.0.0.1:{}", self.port),
            format!("localhost:{}", self.port),
        ]
    }
}

/// Checks that the headers a modern request mirrors match its body.
fn mirrored(request: &HttpRequest, message: &Value) -> Result<(), String> {
    let params = message.get("params");
    let Some(version) = params
        .and_then(|params| params.get("_meta"))
        .and_then(|meta| meta.get(META_VERSION))
        .and_then(Value::as_str)
    else {
        return Ok(());
    };
    if message.get("id").is_none() {
        return Ok(());
    }
    let header = |name: &str| request.headers.get(name).map(String::as_str);
    if header("mcp-protocol-version") != Some(version) {
        return Err(format!(
            "Header mismatch: MCP-Protocol-Version must be {version}, as the body's _meta says"
        ));
    }
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    if header("mcp-method") != Some(method) {
        return Err(format!("Header mismatch: Mcp-Method must be {method}"));
    }
    let named = match method {
        "tools/call" | "prompts/get" => params.and_then(|p| p.get("name")),
        "resources/read" => params.and_then(|p| p.get("uri")),
        _ => None,
    };
    if let Some(named) = named.and_then(Value::as_str) {
        let given = header("mcp-name").and_then(header_text);
        if given.as_deref() != Some(named) {
            return Err(format!("Header mismatch: Mcp-Name must be {named}"));
        }
    }
    Ok(())
}

/// Answers one request.
pub fn respond(
    server: &Server,
    host: &dyn Host,
    endpoint: &Endpoint<'_>,
    request: &HttpRequest,
) -> HttpResponse {
    let header = |name: &str| request.headers.get(name).map(String::as_str);
    let hosts = endpoint.hosts();
    match header("host") {
        None => return refused(400, "Host header is required"),
        Some(given) if !hosts.iter().any(|known| known.eq_ignore_ascii_case(given)) => {
            return refused(403, "Host is not this endpoint");
        }
        Some(_) => {}
    }
    if let Some(origin) = header("origin")
        && !hosts
            .iter()
            .any(|known| origin.eq_ignore_ascii_case(&format!("http://{known}")))
    {
        return refused(403, "Origin is not allowed");
    }
    let path = request.path.split('?').next().unwrap_or("");
    if path != ENDPOINT {
        return refused(404, "Not found; the MCP endpoint is /mcp");
    }
    let authorized = header("authorization")
        .and_then(|value| {
            let (scheme, token) = value.split_once(' ')?;
            scheme
                .eq_ignore_ascii_case("bearer")
                .then_some(token.trim())
        })
        .is_some_and(|token| {
            !endpoint.token.is_empty() && same(token.as_bytes(), endpoint.token.as_bytes())
        });
    if !authorized {
        return refused(401, "Missing or wrong bearer token");
    }
    match request.method.as_str() {
        "POST" => {}
        "DELETE" => {
            return match header("mcp-session-id") {
                Some(session) if server.end_session(session) => HttpResponse {
                    status: 204,
                    headers: Vec::new(),
                    body: Vec::new(),
                },
                Some(_) => refused(404, "Session not found"),
                None => refused(405, "Only POST and DELETE of a session are served"),
            };
        }
        _ => return refused(405, "Only POST and DELETE of a session are served"),
    }
    if !header("content-type").is_some_and(|kind| {
        kind.split(';')
            .next()
            .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"))
    }) {
        return refused(415, "Content-Type must be application/json");
    }
    if let Some(accept) = header("accept") {
        let accept = accept.to_ascii_lowercase();
        if !accept.contains("application/json")
            && !accept.contains("*/*")
            && !accept.contains("application/*")
        {
            return refused(406, "Accept must include application/json");
        }
    }
    let message: Value = match serde_json::from_slice(&request.body) {
        Ok(message) => message,
        Err(_) => {
            return json_response(
                400,
                &protocol::error(None, PARSE_ERROR, "Parse error", None),
            );
        }
    };
    if let Err(mismatch) = mirrored(request, &message) {
        return json_response(
            400,
            &protocol::error(message.get("id"), HEADER_MISMATCH, &mismatch, None),
        );
    }
    let outgoing = server.handle(
        host,
        Incoming {
            message: &message,
            version_header: header("mcp-protocol-version"),
            session: header("mcp-session-id"),
        },
    );
    let mut response = match &outgoing.body {
        Some(body) => json_response(outgoing.status, body),
        None => HttpResponse {
            status: outgoing.status,
            headers: Vec::new(),
            body: Vec::new(),
        },
    };
    if let Some(session) = outgoing.session {
        response.headers.push(("Mcp-Session-Id", session));
    }
    response
}

/// A request a client sends, for the stdio bridge and the tests: the
/// headers the transport asks for, derived from the message itself.
pub fn client_request(
    port: u16,
    token: &str,
    message: &Value,
    legacy_version: Option<&str>,
    session: Option<&str>,
) -> Vec<u8> {
    let body = message.to_string();
    let mut head = String::from("POST /mcp HTTP/1.1\r\n");
    let mut line = |name: &str, value: &str| {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    };
    line("Host", &format!("127.0.0.1:{port}"));
    line("Authorization", &format!("Bearer {token}"));
    line("Content-Type", "application/json");
    line("Accept", "application/json, text/event-stream");
    let modern = message
        .get("params")
        .and_then(|params| params.get("_meta"))
        .and_then(|meta| meta.get(META_VERSION))
        .and_then(Value::as_str);
    if let Some(version) = modern.or(legacy_version) {
        line("MCP-Protocol-Version", version);
    }
    if let Some(method) = message.get("method").and_then(Value::as_str) {
        line("Mcp-Method", &header_value(method));
        let named = match method {
            "tools/call" | "prompts/get" => message.pointer("/params/name"),
            "resources/read" => message.pointer("/params/uri"),
            _ => None,
        };
        if let Some(named) = named.and_then(Value::as_str) {
            line("Mcp-Name", &header_value(named));
        }
    }
    if let Some(session) = session {
        line("Mcp-Session-Id", session);
    }
    line("Content-Length", &body.len().to_string());
    head.push_str("\r\n");
    let mut bytes = head.into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

/// A header value as is when it is plain ASCII, else in the base64 form.
fn header_value(text: &str) -> String {
    let plain = text.bytes().all(|b| (0x20..0x7f).contains(&b))
        && text.trim() == text
        && !(text.starts_with("=?base64?") && text.ends_with("?="));
    if plain {
        return text.to_owned();
    }
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::from("=?base64?");
    for chunk in text.as_bytes().chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out.push_str("?=");
    out
}

/// Parses a response the server wrote, for the bridge and the tests.
pub fn read_response(
    input: &mut impl Read,
) -> io::Result<(u16, BTreeMap<String, String>, Vec<u8>)> {
    let mut bytes = Vec::new();
    input.take(MAX_BODY as u64 * 8).read_to_end(&mut bytes)?;
    let split = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no end of headers"))?;
    let head = std::str::from_utf8(&bytes[..split])
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "headers are not text"))?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "no status"))?;
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    Ok((status, headers, bytes[split + 4..].to_vec()))
}

/// A JSON-RPC error for the stdio side, when the app cannot be reached.
pub fn bridge_error(id: Option<&Value>, message: &str) -> Value {
    protocol::error(
        id,
        protocol::INTERNAL_ERROR,
        message,
        Some(json!({"source": "windfall --mcp-stdio"})),
    )
}
