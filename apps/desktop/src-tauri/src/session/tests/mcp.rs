//! The MCP server against the real session: proposals applied through
//! Session::proposal_apply, refusals while recording and once stale, the
//! loopback endpoint over TCP with its token, the stdio bridge, and renders.

use std::io::Write;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};
use windfall_ipc::{McpFocus, McpSettingsUpdate};
use windfall_mcp::Host;
use windfall_mcp::bridge::Bridge;
use windfall_mcp::http::{client_request, read_response};
use windfall_mcp::protocol::{
    Incoming, META_CAPABILITIES, META_CLIENT, META_VERSION, MODERN, Outgoing,
};
use windfall_project::{Command, NoteInit, PatternId};

use super::Rig;
use crate::events::Event;

const AGENT: &str = "session-agent";

fn modern(id: u64, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({
        (META_VERSION): MODERN,
        (META_CLIENT): {"name": AGENT, "version": "1"},
        (META_CAPABILITIES): {},
    });
    json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
}

fn handle(rig: &Rig, host: &dyn Host, message: &Value) -> Outgoing {
    rig.session.inner.mcp.server.handle(
        host,
        Incoming {
            message,
            version_header: Some(MODERN),
            session: None,
        },
    )
}

/// Calls a tool and returns its `result`.
fn call(rig: &Rig, host: &dyn Host, name: &str, arguments: Value) -> Value {
    let out = handle(
        rig,
        host,
        &modern(
            1,
            "tools/call",
            json!({"name": name, "arguments": arguments}),
        ),
    );
    assert_eq!(out.status, 200, "{out:?}");
    out.body.unwrap()["result"].clone()
}

fn ok(result: &Value) -> Value {
    assert_eq!(result["isError"], false, "{result}");
    result["structuredContent"].clone()
}

fn failure(result: &Value) -> String {
    assert_eq!(result["isError"], true, "{result}");
    result["content"][0]["text"].as_str().unwrap().to_owned()
}

fn free_port() -> u16 {
    let socket = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    socket.local_addr().unwrap().port()
}

fn turn_on(rig: &Rig) -> u16 {
    let port = free_port();
    let state = rig
        .session
        .mcp_configure(McpSettingsUpdate {
            enabled: true,
            port,
        })
        .unwrap();
    assert!(state.enabled && state.running, "{:?}", state.error);
    assert_eq!(state.url, format!("http://127.0.0.1:{port}/mcp"));
    port
}

#[test]
fn an_agent_proposal_applies_through_the_session_as_one_undo_step() {
    let rig = Rig::new();
    let host = rig.session.mcp_test_host();
    let before = rig.session.document_snapshot();
    let result = ok(&call(
        &rig,
        &host,
        "create_pattern",
        json!({"name": "Verse"}),
    ));
    assert_eq!(result["status"]["state"], "pending");
    // Nothing changed, and the window was told about the proposal.
    assert_eq!(rig.session.document_snapshot().project, before.project);
    let events = rig.events.take();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::McpPanel(panel) if panel.proposals.len() == 1))
    );
    let id = result["proposalId"].as_str().unwrap();
    let panel = rig.session.mcp_panel();
    assert_eq!(panel.proposals[0].status, "pending");
    assert!(!panel.proposals[0].stale);
    assert_eq!(panel.proposals[0].counts[0].kind, "patterns");
    let panel = rig.session.mcp_approve(id).unwrap();
    assert_eq!(panel.proposals[0].status, "applied");
    let after = rig.session.document_snapshot();
    assert_eq!(
        after.project.patterns.len(),
        before.project.patterns.len() + 1
    );
    assert_eq!(after.history.cursor, before.history.cursor + 1);
    assert_eq!(
        after.history.entries[after.history.cursor as usize - 1].label,
        "session-agent: Create pattern"
    );
    // One undo step takes it all back.
    rig.session.undo().unwrap();
    assert_eq!(
        rig.session.document_snapshot().project.patterns,
        before.project.patterns
    );
    // A decided proposal cannot be applied again, and can be dismissed.
    assert!(rig.session.mcp_approve(id).is_err());
    assert!(rig.session.mcp_dismiss(id).proposals.is_empty());
}

#[test]
fn proposals_are_refused_while_recording_and_once_stale() {
    let rig = Rig::new();
    let host = rig.session.mcp_test_host();
    rig.session
        .inner
        .performance_capturing
        .store(true, std::sync::atomic::Ordering::Release);
    let message = failure(&call(&rig, &host, "create_pattern", json!({})));
    assert!(message.contains("recording"), "{message}");
    rig.session
        .inner
        .performance_capturing
        .store(false, std::sync::atomic::Ordering::Release);
    let result = ok(&call(&rig, &host, "create_pattern", json!({})));
    let id = result["proposalId"].as_str().unwrap().to_owned();
    // An unrelated edit by the user makes it stale.
    rig.session
        .dispatch(
            Command::AddPattern {
                name: Some("By hand".into()),
            },
            None,
        )
        .unwrap();
    assert!(rig.session.mcp_panel().proposals[0].stale);
    let before = rig.session.document_snapshot().project;
    let error = rig.session.mcp_approve(&id).unwrap_err();
    assert!(error.contains("stale"), "{error}");
    assert_eq!(rig.session.document_snapshot().project, before);
    // Applying during a recording is refused by the session itself.
    let result = ok(&call(&rig, &host, "create_pattern", json!({})));
    rig.session
        .inner
        .performance_capturing
        .store(true, std::sync::atomic::Ordering::Release);
    assert!(
        rig.session
            .mcp_approve(result["proposalId"].as_str().unwrap())
            .is_err()
    );
    assert_eq!(rig.session.document_snapshot().project, before);
}

#[test]
fn a_trusted_agent_applies_without_asking_and_turning_off_ends_it() {
    let rig = Rig::new();
    let _port = turn_on(&rig);
    let host = rig.session.mcp_test_host();
    let channels = rig.project().channels.len();
    // The agent appears in the panel once it calls a tool.
    ok(&call(&rig, &host, "get_project_summary", json!({})));
    let panel = rig.session.mcp_trust(AGENT, true);
    assert!(
        panel
            .agents
            .iter()
            .any(|agent| agent.name == AGENT && agent.trusted)
    );
    let result = ok(&call(
        &rig,
        &host,
        "add_channel",
        json!({"instrument": "subtractiveSynth", "name": "Agent synth"}),
    ));
    assert_eq!(result["status"]["state"], "applied");
    assert_eq!(rig.project().channels.len(), channels + 1);
    assert!(rig.session.mcp_panel().proposals[0].automatic);
    // Turning the server off withdraws what waits and ends the allowance.
    rig.session.mcp_trust(AGENT, false);
    let pending = ok(&call(&rig, &host, "create_pattern", json!({})));
    assert_eq!(pending["status"]["state"], "pending");
    rig.session
        .mcp_configure(McpSettingsUpdate {
            enabled: false,
            port: free_port(),
        })
        .unwrap();
    let panel = rig.session.mcp_panel();
    assert!(panel.agents.is_empty());
    assert!(
        panel
            .proposals
            .iter()
            .any(|proposal| proposal.status == "withdrawn")
    );
}

/// Sends raw bytes and reads the answer.
fn exchange(port: u16, bytes: &[u8]) -> (u16, std::collections::BTreeMap<String, String>, Value) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    stream.write_all(bytes).unwrap();
    let (status, headers, body) = read_response(&mut stream).unwrap();
    let body = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, headers, body)
}

#[test]
fn the_endpoint_serves_mcp_over_loopback_with_its_token_only() {
    let rig = Rig::new();
    let port = turn_on(&rig);
    let token = rig.session.mcp_state().token;
    assert_eq!(token.len(), 64);
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "tcp-agent", "version": "1"},
    }});
    // No token, a wrong one, a foreign host and a foreign origin.
    let request = client_request(port, &token, &init, None, None);
    let text = String::from_utf8(request.clone()).unwrap();
    for (from, to, status) in [
        (format!("Bearer {token}"), String::new(), 401),
        (format!("Bearer {token}"), "Bearer nope".to_owned(), 401),
        (
            format!("127.0.0.1:{port}"),
            format!("attacker.example:{port}"),
            403,
        ),
        (
            "Accept:".to_owned(),
            "Origin: http://attacker.example\r\nAccept:".to_owned(),
            403,
        ),
    ] {
        let tampered = text.replacen(&from, &to, 1);
        let (got, _, _) = exchange(port, tampered.as_bytes());
        assert_eq!(got, status, "{from} -> {to}");
    }
    // With the token: a session, then its tools.
    let (status, headers, body) = exchange(port, &request);
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["result"]["protocolVersion"], "2025-06-18");
    let session = headers
        .get("mcp-session-id")
        .cloned()
        .expect("a session id");
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    let (status, _, body) = exchange(
        port,
        &client_request(
            port,
            &token,
            &list,
            Some("2025-06-18"),
            Some(session.as_str()),
        ),
    );
    assert_eq!(status, 200);
    assert!(body["result"]["tools"].as_array().unwrap().len() > 20);
    // A modern request with its mirrored headers.
    let summary = modern(
        3,
        "tools/call",
        json!({"name": "get_project_summary", "arguments": {}}),
    );
    let (status, _, body) = exchange(port, &client_request(port, &token, &summary, None, None));
    assert_eq!(status, 200);
    assert_eq!(body["result"]["structuredContent"]["name"], "Untitled");
    // Too large a body is refused before it is read.
    let big =
        format!("POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 99999999\r\n\r\n");
    let (status, _, _) = exchange(port, big.as_bytes());
    assert_eq!(status, 413);
    // Off: the port closes and the file says so.
    rig.session
        .mcp_configure(McpSettingsUpdate {
            enabled: false,
            port,
        })
        .unwrap();
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
    let state = rig.session.mcp_state();
    assert!(!state.enabled && !state.running);
}

#[test]
fn a_new_token_locks_out_the_old_one() {
    let rig = Rig::new();
    let port = turn_on(&rig);
    let old = rig.session.mcp_state().token;
    let fresh = rig.session.mcp_regenerate_token().unwrap();
    assert_ne!(fresh.token, old);
    assert!(fresh.running);
    let ping = modern(1, "ping", json!({}));
    let (status, _, _) = exchange(port, &client_request(port, &old, &ping, None, None));
    assert_eq!(status, 401);
    let (status, _, _) = exchange(port, &client_request(port, &fresh.token, &ping, None, None));
    assert_eq!(status, 200);
}

#[test]
fn the_server_is_off_by_default_and_restores_when_left_on() {
    let rig = Rig::new();
    let state = rig.session.mcp_state();
    assert!(!state.enabled && !state.running);
    rig.session.mcp_restore();
    assert!(!rig.session.mcp_state().running);
    let port = turn_on(&rig);
    let rig = rig.restart();
    assert!(
        !rig.session.mcp_state().running,
        "only start-up restores it"
    );
    rig.session.mcp_restore();
    let state = rig.session.mcp_state();
    assert!(state.running);
    assert_eq!(state.port, port);
    assert!(
        rig.session
            .mcp_configure(McpSettingsUpdate {
                enabled: true,
                port: 80
            })
            .is_err()
    );
}

#[test]
fn the_stdio_bridge_relays_to_the_running_app() {
    let rig = Rig::new();
    turn_on(&rig);
    let file = rig.session.store().mcp_file();
    let bridge = Arc::new(Bridge::new(crate::mcp::stdio::Tcp { file }));
    bridge.check().unwrap();
    let input = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "Claude Desktop", "version": "1"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {"name": "get_selection", "arguments": {}}}),
    ]
    .iter()
    .map(Value::to_string)
    .collect::<Vec<_>>()
    .join("\n");
    #[derive(Clone, Default)]
    struct Shared(Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let output = Shared::default();
    windfall_mcp::bridge::run(
        bridge,
        std::io::Cursor::new(input.into_bytes()),
        Box::new(output.clone()),
    );
    let written = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
    let answers: Vec<Value> = written
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(answers.len(), 2, "{written}");
    assert!(
        answers
            .iter()
            .any(|a| a["id"] == 1 && a["result"]["protocolVersion"] == "2025-11-25")
    );
    assert!(
        answers
            .iter()
            .any(|a| a["id"] == 2 && a["result"]["isError"] == false)
    );
    // With the server off the bridge says so and connects to nothing.
    rig.session
        .mcp_configure(McpSettingsUpdate {
            enabled: false,
            port: free_port(),
        })
        .unwrap();
    let file = rig.session.store().mcp_file();
    assert!(
        Bridge::new(crate::mcp::stdio::Tcp { file })
            .check()
            .unwrap_err()
            .contains("off")
    );
}

#[test]
fn focus_is_kept_only_while_the_server_listens() {
    let rig = Rig::new();
    let host = rig.session.mcp_test_host();
    let channel = rig.channel(0);
    let focus = McpFocus {
        channel: Some(channel),
        track: None,
        view: Some("pianoRoll".into()),
    };
    rig.session.mcp_focus(focus.clone());
    assert_eq!(host.focus().channel, None);
    turn_on(&rig);
    rig.session.mcp_focus(focus);
    assert_eq!(host.focus().channel, Some(channel));
    let selection = ok(&call(&rig, &host, "get_selection", json!({})));
    assert_eq!(selection["view"], "pianoRoll");
    assert_eq!(selection["channel"]["id"], channel.0);
}

#[test]
fn an_agent_render_measures_the_project_solos_a_track_and_keeps_a_wav() {
    let rig = Rig::new();
    let host = rig.session.mcp_test_host();
    let pattern: PatternId = rig.pattern();
    let kick = rig.project().channels[0].clone();
    let hat = rig.project().channels[2].clone();
    rig.session
        .dispatch(
            Command::AddNotes {
                pattern,
                channel: kick.id,
                notes: (0..4)
                    .map(|beat| NoteInit {
                        start: beat * windfall_core::PPQ,
                        length: windfall_core::PPQ / 4,
                        key: 60,
                        velocity: None,
                        pan: None,
                        expression: None,
                    })
                    .collect(),
            },
            None,
        )
        .unwrap();
    let result = call(
        &rig,
        &host,
        "render_and_measure",
        json!({"region": {"patternId": pattern.0, "loops": 2}}),
    );
    let value = ok(&result);
    let measured = &value["measurements"];
    assert!(
        measured["integratedLufs"].as_f64().unwrap() > -60.0,
        "{measured}"
    );
    assert!(
        (measured["seconds"].as_f64().unwrap() - 4.0).abs() < 0.1,
        "{measured}"
    );
    let path = value["wavPath"].as_str().unwrap();
    assert!(std::path::Path::new(path).is_file());
    assert!(path.contains("Agent renders"));
    assert_eq!(result["content"][1]["type"], "resource_link");
    // The hat's track alone is silent: only the kick plays.
    let solo = ok(&call(
        &rig,
        &host,
        "render_and_measure",
        json!({"region": {"patternId": pattern.0}, "soloTrackId": hat.mixer_track.0}),
    ));
    assert_eq!(solo["measurements"]["integratedLufs"], -120.0);
    // Too long a region is refused before rendering.
    let message = failure(&call(
        &rig,
        &host,
        "render_and_measure",
        json!({"region": {"patternId": pattern.0, "loops": 16}, "maxSeconds": 5}),
    ));
    assert!(message.contains("maxSeconds"), "{message}");
    // The render did not change the project.
    assert!(rig.project().mixer.tracks.iter().all(|track| !track.solo));
}
