//! The server against a fixture project held in a bare [`Document`]: the
//! protocol, the HTTP checks, the stdio bridge, every tool, and the
//! approval flow.

use std::collections::BTreeSet;
use std::io::{self, Cursor, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use windfall_core::PPQ;
use windfall_project::proposal::{CommandProposal, ProposalStamp};
use windfall_project::{
    AutomationTarget, ChannelId, ClipContent, ClipInit, Command, Document, EffectKind,
    InstrumentKind, NoteInit, PatternId, PlayMode, PlaylistTrackId, Project, TICKS_PER_STEP,
    TrackId,
};

use crate::approvals::{Approver, Status};
use crate::bridge::{self, Bridge, Link};
use crate::host::{
    ChannelPresetInfo, Focus, Host, LibraryFile, LibraryPage, NativeEdit, RenderRequest,
    RenderedAudio, Snapshot, SoundFontPreset, Transport,
};
use crate::http::{self, Endpoint, HttpRequest};
use crate::protocol::{self, Incoming, LEGACY, MODERN, Server};

const AGENT: &str = "test-agent";

struct Fixture {
    document: Mutex<Document>,
    recording: AtomicBool,
    focus: Mutex<Focus>,
    rendered: Mutex<Option<RenderRequest>>,
    rendering: AtomicBool,
    /// Renders wait for cancellation instead of returning.
    slow: AtomicBool,
    counter: AtomicU64,
    changes: AtomicUsize,
    /// What the fixture's renders say made no sound.
    silent: Mutex<Vec<ChannelId>>,
    ids: Ids,
}

/// The fixture's library: the only folder an agent may name.
const LIBRARY: &str = "C:/Library/";
const SOUNDFONT: &str = "C:/Library/GM.sf2";


/// The fixture's ids.
#[derive(Clone, Copy)]
struct Ids {
    bass: ChannelId,
    bass_track: TrackId,
    lead: ChannelId,
    lead_track: TrackId,
    pattern: PatternId,
    playlist: PlaylistTrackId,
    reverb: u32,
    clip: u32,
}

impl Fixture {
    fn new() -> Self {
        let mut document = Document::new(Project::new("Fixture"));
        let mut run = |command: Command| -> Vec<u32> {
            let applied = document.dispatch(command, None).unwrap();
            document.patch(&applied.touched);
            applied.created
        };
        let channel = |name: &str| Command::AddChannel {
            name: Some(name.into()),
            sample: None,
            instrument: Some(InstrumentKind::SubtractiveSynth),
            index: None,
            mixer_track: None,
        };
        let bass = run(channel("Bass"));
        let lead = run(channel("Lead"));
        let playlist = run(Command::AddPlaylistTrack {
            name: Some("Main".into()),
            index: None,
        });
        let pattern = PatternId(1);
        // A, C, E, A, G, E, D, C: eight eighths of A minor.
        let keys = [57, 60, 64, 69, 67, 64, 62, 60];
        run(Command::AddNotes {
            pattern,
            channel: ChannelId(bass[0]),
            notes: keys
                .iter()
                .enumerate()
                .map(|(index, key)| NoteInit {
                    start: index as u32 * PPQ / 2,
                    length: PPQ / 2,
                    key: *key,
                    velocity: None,
                    pan: None,
                    expression: None,
                })
                .collect(),
        });
        let reverb = run(Command::AddEffect {
            track: TrackId(bass[1]),
            kind: EffectKind::Reverb,
            index: None,
        });
        let clip = run(Command::AddClips {
            clips: vec![ClipInit {
                track: PlaylistTrackId(playlist[0]),
                start: 0,
                length: None,
                offset: None,
                muted: None,
                content: ClipContent::Pattern { pattern },
            }],
        });
        let ids = Ids {
            bass: ChannelId(bass[0]),
            bass_track: TrackId(bass[1]),
            lead: ChannelId(lead[0]),
            lead_track: TrackId(lead[1]),
            pattern,
            playlist: PlaylistTrackId(playlist[0]),
            reverb: reverb[0],
            clip: clip[0],
        };
        Self {
            document: Mutex::new(document),
            recording: AtomicBool::new(false),
            focus: Mutex::new(Focus::default()),
            rendered: Mutex::new(None),
            rendering: AtomicBool::new(false),
            slow: AtomicBool::new(false),
            counter: AtomicU64::new(0),
            changes: AtomicUsize::new(0),
            silent: Mutex::new(Vec::new()),
            ids,
        }
    }

    fn project(&self) -> Project {
        self.document.lock().unwrap().project().clone()
    }

    fn history(&self) -> windfall_project::HistoryView {
        self.document.lock().unwrap().history()
    }

    /// An edit the user makes by hand.
    fn edit(&self, command: Command) {
        let mut document = self.document.lock().unwrap();
        let applied = document.dispatch(command, None).unwrap();
        document.patch(&applied.touched);
    }
}

impl Host for Fixture {
    fn snapshot(&self) -> Result<Snapshot, String> {
        let document = self.document.lock().unwrap().clone();
        let stamp = ProposalStamp {
            generation: 1,
            revision: document.revision(),
        };
        let pattern = document.project().patterns[0].id;
        Ok(Snapshot {
            document,
            stamp,
            recording: self.recording.load(Ordering::Acquire),
            transport: Transport {
                playing: false,
                mode: PlayMode::Pattern,
                pattern,
            },
        })
    }

    fn focus(&self) -> Focus {
        self.focus.lock().unwrap().clone()
    }

    fn apply(&self, proposal: CommandProposal) -> Result<u64, String> {
        let mut document = self.document.lock().unwrap();
        let stamp = ProposalStamp {
            generation: 1,
            revision: document.revision(),
        };
        let applied =
            proposal.apply(&mut document, stamp, self.recording.load(Ordering::Acquire))?;
        document.patch(&applied.touched);
        Ok(document.revision())
    }

    fn render(
        &self,
        request: RenderRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RenderedAudio, String> {
        *self.rendered.lock().unwrap() = Some(request);
        self.rendering.store(true, Ordering::Release);
        if self.slow.load(Ordering::Acquire) {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(10) {
                if cancelled() {
                    return Err("the render was cancelled".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            return Err("the fixture render was never cancelled".into());
        }
        let rate = 48_000u32;
        let samples = (0..rate as usize * 4)
            .flat_map(|n| {
                let s = (0.5 * (std::f64::consts::TAU * 997.0 * n as f64 / f64::from(rate)).sin())
                    as f32;
                [s, s]
            })
            .collect();
        Ok(RenderedAudio {
            samples,
            sample_rate: rate,
            path: Some("C:/Windfall/Agent renders/Render-1.wav".into()),
            silent_channels: self.silent.lock().unwrap().clone(),
        })
    }

    fn apply_native(&self, proposal: CommandProposal, edit: NativeEdit) -> Result<u64, String> {
        let mut document = self.document.lock().unwrap();
        let stamp = ProposalStamp {
            generation: 1,
            revision: document.revision(),
        };
        if proposal.stamp != stamp {
            return Err("proposal is stale; generate a new preview".into());
        }
        let mut commands = proposal.commands.clone();
        if !commands.is_empty() {
            proposal.batch()?;
        }
        let mut candidate = document.clone();
        if !commands.is_empty() {
            candidate
                .dispatch(proposal.batch()?, None)
                .map_err(|e| e.to_string())?;
        }
        match edit {
            NativeEdit::SoundFont {
                channel,
                path,
                bank,
                program,
                replace_instrument,
                ..
            } => {
                let (load, _) = windfall_project::sample_instrument::soundfont_load_commands(
                    &candidate,
                    channel,
                    "GM".into(),
                    windfall_project::SamplePath::External(path),
                    None,
                    bank,
                    program,
                    replace_instrument,
                )?;
                commands.extend(load);
            }
            NativeEdit::ChannelPreset { channel, name, .. } => {
                commands.push(Command::UpdateChannel {
                    id: channel,
                    patch: windfall_project::ChannelPatch {
                        name: Some(name),
                        ..Default::default()
                    },
                });
            }
        }
        let applied = document
            .dispatch(
                Command::Batch {
                    label: Some(proposal.label.clone()),
                    commands,
                },
                None,
            )
            .map_err(|e| e.to_string())?;
        document.patch(&applied.touched);
        Ok(document.revision())
    }

    fn library_file(&self, path: &str) -> Result<String, String> {
        let path = path.replace('\\', "/");
        if !path.starts_with(LIBRARY) || path.contains("..") {
            return Err(format!(
                "{path} is not inside a folder in Windfall's browser"
            ));
        }
        Ok(path)
    }

    fn soundfont_presets(&self, path: &str) -> Result<Vec<SoundFontPreset>, String> {
        if path != SOUNDFONT {
            return Err("not a SoundFont".into());
        }
        let preset = |bank, program, name: &str| SoundFontPreset {
            name: name.into(),
            bank,
            program,
        };
        Ok(vec![
            preset(0, 0, "Grand Piano"),
            preset(0, 52, "Choir Aahs"),
            preset(0, 60, "French Horn"),
            preset(128, 0, "Standard Kit"),
        ])
    }

    fn search_library(
        &self,
        query: &str,
        kind: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<LibraryPage, String> {
        let files = [
            (SOUNDFONT, "soundFont", 1_000_000),
            ("C:/Library/Choir/Aah.wav", "audio", 48_000),
            ("C:/Library/Kick.wav", "audio", 12_000),
        ];
        let found: Vec<LibraryFile> = files
            .iter()
            .filter(|(path, file_kind, _)| {
                kind.is_none_or(|kind| kind == *file_kind)
                    && path.to_lowercase().contains(&query.to_lowercase())
            })
            .map(|(path, kind, bytes)| LibraryFile {
                path: (*path).into(),
                kind: (*kind).into(),
                bytes: *bytes,
                relative_path: path.trim_start_matches(LIBRARY).into(),
            })
            .collect();
        Ok(LibraryPage {
            total: found.len(),
            items: found.into_iter().skip(offset).take(limit).collect(),
            indexing: false,
            truncated: false,
        })
    }

    fn channel_presets(&self) -> Result<Vec<ChannelPresetInfo>, String> {
        Ok(vec![ChannelPresetInfo {
            name: "Warm Pad".into(),
            path: "C:/Windfall/Presets/Warm Pad.wfpreset".into(),
            origin: "library".into(),
            instrument: "Subtractive Synth".into(),
        }])
    }

    fn proposals_changed(&self) {
        self.changes.fetch_add(1, Ordering::AcqRel);
    }

    fn random_hex(&self) -> String {
        format!(
            "{:032x}",
            self.counter.fetch_add(1, Ordering::AcqRel) + 0xabc
        )
    }
}

fn meta() -> Value {
    json!({
        (protocol::META_VERSION): MODERN,
        (protocol::META_CLIENT): {"name": AGENT, "version": "1.0"},
        (protocol::META_CAPABILITIES): {},
    })
}

fn request(
    server: &Server,
    host: &Fixture,
    id: u64,
    method: &str,
    mut params: Value,
) -> protocol::Outgoing {
    params["_meta"] = meta();
    let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
    server.handle(
        host,
        Incoming {
            message: &message,
            version_header: Some(MODERN),
            session: None,
        },
    )
}

/// Calls a tool and returns the `result` object.
fn call(server: &Server, host: &Fixture, name: &str, arguments: Value) -> Value {
    let out = request(
        server,
        host,
        1,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    );
    assert_eq!(out.status, 200, "{out:?}");
    let body = out.body.unwrap();
    assert!(body.get("error").is_none(), "{body}");
    body["result"].clone()
}

fn ok(result: &Value) -> Value {
    assert_eq!(result["isError"], false, "{result}");
    assert_eq!(result["resultType"], "complete");
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text, result["structuredContent"]);
    result["structuredContent"].clone()
}

fn failure(result: &Value) -> String {
    assert_eq!(result["isError"], true, "{result}");
    result["content"][0]["text"].as_str().unwrap().to_owned()
}

fn pending_commands(server: &Server, id: &str) -> Vec<Command> {
    server.approvals().get(id).unwrap().proposal.commands
}

fn setup() -> (Server, Fixture) {
    (Server::new("1.0.1-test"), Fixture::new())
}

// --- Protocol --------------------------------------------------------------

#[test]
fn initialize_negotiates_a_legacy_version_and_opens_a_session() {
    let (server, host) = setup();
    for (asked, settled) in [
        ("2025-06-18", "2025-06-18"),
        ("2025-11-25", "2025-11-25"),
        ("2025-03-26", "2025-03-26"),
        ("2024-11-05", LEGACY[0]),
    ] {
        let message = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": asked,
            "capabilities": {},
            "clientInfo": {"name": "claude-code", "version": "2.0"},
        }});
        let out = server.handle(
            &host,
            Incoming {
                message: &message,
                version_header: None,
                session: None,
            },
        );
        assert_eq!(out.status, 200);
        let body = out.body.unwrap();
        assert_eq!(body["result"]["protocolVersion"], settled);
        assert_eq!(body["result"]["serverInfo"]["name"], "windfall");
        assert!(body["result"]["capabilities"]["tools"].is_object());
        assert!(body["result"]["capabilities"]["resources"].is_object());
        assert!(
            body["result"]["instructions"]
                .as_str()
                .unwrap()
                .contains("get_project_summary")
        );
        assert!(body["result"].get("resultType").is_none());
        let session = out.session.expect("a session id");
        assert!(server.knows_session(&session));
        // Later requests carry the session and the version.
        let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let out = server.handle(
            &host,
            Incoming {
                message: &list,
                version_header: Some(settled),
                session: Some(&session),
            },
        );
        assert_eq!(out.status, 200);
        assert!(
            !out.body.unwrap()["result"]["tools"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    assert!(
        server
            .approvals()
            .agents()
            .contains(&"claude-code".to_owned())
    );
    // A session this server never gave out.
    let list = json!({"jsonrpc": "2.0", "id": 3, "method": "tools/list"});
    let out = server.handle(
        &host,
        Incoming {
            message: &list,
            version_header: Some("2025-06-18"),
            session: Some("forgotten"),
        },
    );
    assert_eq!(out.status, 404);
    // A legacy version this server does not speak.
    let out = server.handle(
        &host,
        Incoming {
            message: &list,
            version_header: Some("1999-01-01"),
            session: None,
        },
    );
    assert_eq!(out.status, 400);
    assert_eq!(
        out.body.unwrap()["error"]["code"],
        protocol::UNSUPPORTED_VERSION
    );
}

#[test]
fn modern_requests_need_their_meta_and_a_supported_version() {
    let (server, host) = setup();
    let out = request(&server, &host, 1, "ping", json!({}));
    assert_eq!(out.status, 200);
    let body = out.body.unwrap();
    assert_eq!(body["result"]["resultType"], "complete");
    assert_eq!(
        body["result"]["_meta"][protocol::META_SERVER]["name"],
        "windfall"
    );
    // An unknown version is answered with the supported ones.
    let message = json!({"jsonrpc": "2.0", "id": 9, "method": "tools/list", "params": {"_meta": {
        (protocol::META_VERSION): "1900-01-01",
        (protocol::META_CAPABILITIES): {},
    }}});
    let out = server.handle(
        &host,
        Incoming {
            message: &message,
            version_header: Some("1900-01-01"),
            session: None,
        },
    );
    assert_eq!(out.status, 400);
    let body = out.body.unwrap();
    assert_eq!(body["id"], 9);
    assert_eq!(body["error"]["code"], -32022);
    assert_eq!(body["error"]["data"]["requested"], "1900-01-01");
    assert_eq!(body["error"]["data"]["supported"][0], MODERN);
    // Capabilities are required.
    let message = json!({"jsonrpc": "2.0", "id": 10, "method": "tools/list", "params": {"_meta": {(protocol::META_VERSION): MODERN}}});
    let out = server.handle(
        &host,
        Incoming {
            message: &message,
            version_header: Some(MODERN),
            session: None,
        },
    );
    assert_eq!(out.status, 400);
    assert_eq!(out.body.unwrap()["error"]["code"], -32602);
    // Malformed messages.
    for (message, status) in [
        (json!([{"jsonrpc": "2.0", "id": 1, "method": "ping"}]), 400),
        (json!({"jsonrpc": "1.0", "id": 1, "method": "ping"}), 400),
        (json!({"jsonrpc": "2.0", "id": null, "method": "ping"}), 400),
        (
            json!({"jsonrpc": "2.0", "id": 1, "method": "ping", "params": [1]}),
            400,
        ),
        (json!({"jsonrpc": "2.0", "id": 1, "result": {}}), 202),
    ] {
        let out = server.handle(
            &host,
            Incoming {
                message: &message,
                version_header: None,
                session: None,
            },
        );
        assert_eq!(out.status, status, "{message}");
    }
}

#[test]
fn server_discover_lists_both_eras() {
    let (server, host) = setup();
    let body = request(&server, &host, 1, "server/discover", json!({}))
        .body
        .unwrap();
    let result = &body["result"];
    assert_eq!(result["resultType"], "complete");
    let versions: Vec<&str> = result["supportedVersions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(versions, [MODERN, "2025-11-25", "2025-06-18", "2025-03-26"]);
    assert!(result["capabilities"]["tools"].is_object());
    assert_eq!(
        result["_meta"][protocol::META_SERVER]["version"],
        "1.0.1-test"
    );
}

#[test]
fn modern_cacheable_results_say_how_long_to_keep_them() {
    let (server, host) = setup();
    for (method, params, ttl) in [
        ("tools/list", json!({}), protocol::LIST_TTL_MS),
        ("resources/list", json!({}), protocol::LIST_TTL_MS),
        ("resources/templates/list", json!({}), protocol::LIST_TTL_MS),
        ("resources/read", json!({"uri": protocol::SUMMARY_URI}), 0),
        ("server/discover", json!({}), 0),
    ] {
        let body = request(&server, &host, 1, method, params).body.unwrap();
        assert_eq!(body["result"]["ttlMs"], ttl, "{method}");
        assert_eq!(body["result"]["cacheScope"], "private", "{method}");
    }
    // Other results are not cacheable, and legacy results never say so.
    let body = request(&server, &host, 1, "ping", json!({})).body.unwrap();
    assert!(body["result"].get("ttlMs").is_none());
    let message = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    let out = server.handle(
        &host,
        Incoming {
            message: &message,
            version_header: Some(LEGACY[0]),
            session: None,
        },
    );
    let body = out.body.unwrap();
    assert!(body["result"]["tools"].is_array());
    assert!(body["result"].get("ttlMs").is_none());
    assert!(body["result"].get("cacheScope").is_none());
}

#[test]
fn tools_list_gives_every_tool_an_object_schema() {
    let (server, host) = setup();
    let body = request(&server, &host, 1, "tools/list", json!({}))
        .body
        .unwrap();
    let tools = body["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), crate::tools::TOOLS.len());
    let mut names = BTreeSet::new();
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(names.insert(name.to_owned()), "{name} twice");
        assert!(
            name.len() <= 128 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "{name}"
        );
        assert!(!tool["description"].as_str().unwrap().is_empty());
        let schema = &tool["inputSchema"];
        assert_eq!(schema["type"], "object", "{name}");
        assert_eq!(schema["additionalProperties"], false, "{name}");
        for required in schema["required"].as_array().unwrap() {
            assert!(
                schema["properties"]
                    .get(required.as_str().unwrap())
                    .is_some(),
                "{name}: {required}"
            );
        }
        let kind = crate::tools::find(name).unwrap().kind;
        if kind == crate::tools::Kind::Edit {
            assert!(schema["properties"]["title"].is_object(), "{name}");
            assert!(schema["properties"]["explanation"].is_object(), "{name}");
            assert_eq!(tool["annotations"]["readOnlyHint"], false);
        }
    }
    for expected in [
        "get_project_summary",
        "list_notes",
        "list_clips",
        "get_mixer_track",
        "get_parameters",
        "get_selection",
        "list_instruments_and_effects",
        "create_pattern",
        "add_notes",
        "remove_notes",
        "update_notes",
        "set_steps",
        "add_channel",
        "add_effect",
        "remove_effect",
        "move_effect",
        "set_parameter",
        "route_channel",
        "route_track",
        "set_send",
        "place_clips",
        "move_clips",
        "remove_clips",
        "set_tempo_meter",
        "rename_and_color",
        "write_automation",
        "apply_raw_commands",
        "get_proposal",
        "apply_proposal",
        "withdraw_proposal",
        "render_and_measure",
        "search_library",
        "list_soundfont_presets",
        "list_channel_presets",
        "load_soundfont_preset",
        "apply_channel_preset",
    ] {
        assert!(names.contains(expected), "{expected}");
    }
    // Every listed tool runs: with no arguments it answers, never panics.
    for tool in crate::tools::TOOLS {
        let result = call(&server, &host, tool.name, json!({}));
        assert!(result["isError"].is_boolean(), "{}", tool.name);
    }
}

#[test]
fn tool_calls_return_structured_results_and_is_error_for_tool_failures() {
    let (server, host) = setup();
    let summary = ok(&call(&server, &host, "get_project_summary", json!({})));
    assert_eq!(summary["tempoBpm"], 120.0);
    let failed = failure(&call(
        &server,
        &host,
        "list_notes",
        json!({"patternId": 999}),
    ));
    assert!(failed.contains("no pattern 999"), "{failed}");
    let failed = failure(&call(
        &server,
        &host,
        "list_notes",
        json!({"patternId": 1, "bogus": true}),
    ));
    assert!(failed.contains("bogus"), "{failed}");
    // Legacy 2025-03-26 results carry text only.
    let message = json!({"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "get_selection", "arguments": {}}});
    let out = server.handle(
        &host,
        Incoming {
            message: &message,
            version_header: Some("2025-03-26"),
            session: None,
        },
    );
    let result = &out.body.unwrap()["result"];
    assert!(result.get("structuredContent").is_none());
    assert!(result.get("resultType").is_none());
    assert!(
        result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("pattern")
    );
}

#[test]
fn unknown_tools_and_methods_are_protocol_errors() {
    let (server, host) = setup();
    let out = request(
        &server,
        &host,
        1,
        "tools/call",
        json!({"name": "no_such_tool", "arguments": {}}),
    );
    assert_eq!(out.status, 200);
    let body = out.body.unwrap();
    assert_eq!(body["error"]["code"], -32602);
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no_such_tool")
    );
    let out = request(&server, &host, 2, "prompts/list", json!({}));
    assert_eq!(out.status, 404);
    assert_eq!(out.body.unwrap()["error"]["code"], -32601);
    let message = json!({"jsonrpc": "2.0", "id": 3, "method": "prompts/list"});
    let out = server.handle(
        &host,
        Incoming {
            message: &message,
            version_header: Some("2025-06-18"),
            session: None,
        },
    );
    assert_eq!(out.status, 200);
    assert_eq!(out.body.unwrap()["error"]["code"], -32601);
}

#[test]
fn notifications_are_accepted_and_cancel_a_running_request() {
    let (server, host) = setup();
    let initialized = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    let out = server.handle(
        &host,
        Incoming {
            message: &initialized,
            version_header: None,
            session: None,
        },
    );
    assert_eq!(
        out,
        protocol::Outgoing {
            status: 202,
            body: None,
            session: None
        }
    );
    host.slow.store(true, Ordering::Release);
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            request(
                &server,
                &host,
                77,
                "tools/call",
                json!({"name": "render_and_measure", "arguments": {}}),
            )
        });
        let started = Instant::now();
        while !host.rendering.load(Ordering::Acquire) {
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(2));
        }
        let cancel = json!({"jsonrpc": "2.0", "method": "notifications/cancelled", "params": {"requestId": 77, "reason": "user", "_meta": meta()}});
        let out = server.handle(
            &host,
            Incoming {
                message: &cancel,
                version_header: Some(MODERN),
                session: None,
            },
        );
        assert_eq!(out.status, 202);
        let result = worker.join().unwrap().body.unwrap()["result"].clone();
        assert!(failure(&result).contains("cancelled"));
    });
}

#[test]
fn resources_list_and_read_the_summary() {
    let (server, host) = setup();
    let body = request(&server, &host, 1, "resources/list", json!({}))
        .body
        .unwrap();
    assert_eq!(body["result"]["resources"][0]["uri"], protocol::SUMMARY_URI);
    let body = request(
        &server,
        &host,
        2,
        "resources/read",
        json!({"uri": protocol::SUMMARY_URI}),
    )
    .body
    .unwrap();
    let text = body["result"]["contents"][0]["text"].as_str().unwrap();
    let summary: Value = serde_json::from_str(text).unwrap();
    assert_eq!(summary["name"], "Fixture");
    let body = request(
        &server,
        &host,
        3,
        "resources/read",
        json!({"uri": "windfall://nothing"}),
    )
    .body
    .unwrap();
    assert_eq!(body["error"]["code"], -32602);
}

// --- HTTP ------------------------------------------------------------------

const PORT: u16 = 47_401;
const TOKEN: &str = "0123456789abcdef0123456789abcdef";

fn post(body: &Value, headers: &[(&str, &str)]) -> HttpRequest {
    let mut map: std::collections::BTreeMap<String, String> = [
        ("host", format!("127.0.0.1:{PORT}")),
        ("authorization", format!("Bearer {TOKEN}")),
        ("content-type", "application/json".to_owned()),
        ("accept", "application/json, text/event-stream".to_owned()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v))
    .collect();
    for (name, value) in headers {
        if value.is_empty() {
            map.remove(*name);
        } else {
            map.insert((*name).to_owned(), (*value).to_owned());
        }
    }
    HttpRequest {
        method: "POST".into(),
        path: "/mcp".into(),
        headers: map,
        body: body.to_string().into_bytes(),
    }
}

fn legacy_ping() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "ping"})
}

fn respond(server: &Server, host: &Fixture, request: &HttpRequest) -> http::HttpResponse {
    http::respond(
        server,
        host,
        &Endpoint {
            port: PORT,
            token: TOKEN,
        },
        request,
    )
}

#[test]
fn requests_without_or_with_a_wrong_token_are_refused() {
    let (server, host) = setup();
    assert_eq!(
        respond(&server, &host, &post(&legacy_ping(), &[])).status,
        200
    );
    for authorization in [
        "",
        "Bearer wrong",
        "Basic abc",
        &format!("Bearer {TOKEN}x"),
        "Bearer",
    ] {
        let response = respond(
            &server,
            &host,
            &post(&legacy_ping(), &[("authorization", authorization)]),
        );
        assert_eq!(response.status, 401, "{authorization:?}");
        assert!(
            response
                .headers
                .iter()
                .any(|(name, value)| *name == "WWW-Authenticate" && value.starts_with("Bearer"))
        );
    }
    // An empty token never matches.
    let response = http::respond(
        &server,
        &host,
        &Endpoint {
            port: PORT,
            token: "",
        },
        &post(&legacy_ping(), &[("authorization", "Bearer ")]),
    );
    assert_eq!(response.status, 401);
}

#[test]
fn foreign_hosts_and_origins_are_refused_against_dns_rebinding() {
    let (server, host) = setup();
    for (name, value, status) in [
        ("host", "evil.example:47401", 403),
        ("host", "127.0.0.1:1", 403),
        ("host", "", 400),
        ("origin", "http://evil.example:47401", 403),
        ("origin", "https://127.0.0.1:47401", 403),
        ("origin", "null", 403),
        ("origin", "http://127.0.0.1:47401", 200),
        ("origin", "http://localhost:47401", 200),
        ("host", "localhost:47401", 200),
    ] {
        let response = respond(&server, &host, &post(&legacy_ping(), &[(name, value)]));
        assert_eq!(response.status, status, "{name}: {value}");
    }
}

/// Reads one byte at a time, slowly.
struct Slow<'a>(&'a [u8]);
impl Read for Slow<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        std::thread::sleep(Duration::from_millis(5));
        if self.0.is_empty() || buf.is_empty() {
            return Ok(0);
        }
        buf[0] = self.0[0];
        self.0 = &self.0[1..];
        Ok(1)
    }
}

#[test]
fn oversized_chunked_and_slow_requests_are_refused() {
    let later = || Instant::now() + Duration::from_secs(5);
    let read = |bytes: &[u8]| http::read_request(&mut &bytes[..], later());
    let good =
        read(b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:1\r\nContent-Length: 2\r\n\r\n{}").unwrap();
    assert_eq!(good.body, b"{}");
    assert_eq!(good.headers["host"], "127.0.0.1:1");
    let big = format!(
        "POST /mcp HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
        http::MAX_BODY + 1
    );
    assert_eq!(read(big.as_bytes()), Err(http::Refusal(413)));
    let long = format!(
        "POST /mcp HTTP/1.1\r\nX: {}\r\n\r\n",
        "a".repeat(http::MAX_HEADER)
    );
    assert_eq!(read(long.as_bytes()), Err(http::Refusal(431)));
    assert_eq!(
        read(b"POST /mcp HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"),
        Err(http::Refusal(501))
    );
    assert_eq!(
        read(b"POST /mcp HTTP/1.1\r\nHost: a\r\n\r\n"),
        Err(http::Refusal(411))
    );
    assert_eq!(
        read(b"POST /mcp HTTP/1.1\r\nContent-Length: 9\r\n\r\n{}"),
        Err(http::Refusal(400))
    );
    assert_eq!(
        read(b"POST /mcp HTTP/1.1\r\nHost: a\r\nhost: b\r\nContent-Length: 0\r\n\r\n"),
        Err(http::Refusal(400))
    );
    assert_eq!(read(b"GET / HTTP/1.0\r\n\r\n"), Err(http::Refusal(400)));
    let slow = b"POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:1\r\nContent-Length: 2\r\n\r\n{}";
    let deadline = Instant::now() + Duration::from_millis(60);
    assert_eq!(
        http::read_request(&mut Slow(slow), deadline),
        Err(http::Refusal(408))
    );
    let response = http::unreadable(http::Refusal(413));
    assert_eq!(response.status, 413);
    let mut wire = Vec::new();
    http::write_response(&mut wire, &response).unwrap();
    assert!(wire.starts_with(b"HTTP/1.1 413 Content Too Large\r\n"));
}

#[test]
fn modern_requests_must_mirror_their_body_in_headers() {
    let (server, host) = setup();
    let mut body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": "get_selection", "arguments": {}}});
    body["params"]["_meta"] = meta();
    let good = [
        ("mcp-protocol-version", MODERN),
        ("mcp-method", "tools/call"),
        ("mcp-name", "get_selection"),
    ];
    assert_eq!(respond(&server, &host, &post(&body, &good)).status, 200);
    for (index, wrong) in [
        ("mcp-protocol-version", "2025-06-18"),
        ("mcp-method", "tools/list"),
        ("mcp-name", "list_notes"),
    ]
    .into_iter()
    .enumerate()
    {
        let mut headers = good;
        headers[index] = wrong;
        let response = respond(&server, &host, &post(&body, &headers));
        assert_eq!(response.status, 400, "{wrong:?}");
        let answer: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(answer["error"]["code"], -32020);
    }
    // A missing header is a mismatch too; the base64 form is decoded.
    let response = respond(&server, &host, &post(&body, &good[..2]));
    assert_eq!(response.status, 400);
    let encoded = [
        good[0],
        good[1],
        ("mcp-name", "=?base64?Z2V0X3NlbGVjdGlvbg==?="),
    ];
    assert_eq!(respond(&server, &host, &post(&body, &encoded)).status, 200);
    // Not JSON, wrong media type, unacceptable answer type.
    let mut garbage = post(&legacy_ping(), &[]);
    garbage.body = b"{not json".to_vec();
    assert_eq!(respond(&server, &host, &garbage).status, 400);
    assert_eq!(
        respond(
            &server,
            &host,
            &post(&legacy_ping(), &[("content-type", "text/plain")])
        )
        .status,
        415
    );
    assert_eq!(
        respond(
            &server,
            &host,
            &post(&legacy_ping(), &[("accept", "text/html")])
        )
        .status,
        406
    );
}

#[test]
fn get_is_not_served_and_delete_ends_a_session() {
    let (server, host) = setup();
    let mut get = post(&legacy_ping(), &[]);
    get.method = "GET".into();
    get.body.clear();
    let response = respond(&server, &host, &get);
    assert_eq!(response.status, 405);
    assert!(response.headers.iter().any(|(name, _)| *name == "Allow"));
    let mut elsewhere = post(&legacy_ping(), &[]);
    elsewhere.path = "/".into();
    assert_eq!(respond(&server, &host, &elsewhere).status, 404);
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "x", "version": "1"}}});
    let response = respond(&server, &host, &post(&init, &[]));
    let session = response
        .headers
        .iter()
        .find(|(name, _)| *name == "Mcp-Session-Id")
        .unwrap()
        .1
        .clone();
    let mut delete = post(&legacy_ping(), &[("mcp-session-id", session.as_str())]);
    delete.method = "DELETE".into();
    assert_eq!(respond(&server, &host, &delete).status, 204);
    assert!(!server.knows_session(&session));
    assert_eq!(respond(&server, &host, &delete).status, 404);
}

// --- The stdio bridge ------------------------------------------------------

struct Loop {
    server: Arc<Server>,
    host: Arc<Fixture>,
    configured: AtomicBool,
    listening: AtomicBool,
}

struct Pipe {
    server: Arc<Server>,
    host: Arc<Fixture>,
    request: Vec<u8>,
    response: Option<Cursor<Vec<u8>>>,
}

impl Write for Pipe {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.request.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Read for Pipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.response.is_none() {
            let answer = match http::read_request(
                &mut self.request.as_slice(),
                Instant::now() + Duration::from_secs(5),
            ) {
                Ok(request) => http::respond(
                    &self.server,
                    &*self.host,
                    &Endpoint {
                        port: PORT,
                        token: TOKEN,
                    },
                    &request,
                ),
                Err(refusal) => http::unreadable(refusal),
            };
            let mut wire = Vec::new();
            http::write_response(&mut wire, &answer)?;
            self.response = Some(Cursor::new(wire));
        }
        self.response.as_mut().unwrap().read(buf)
    }
}

impl Link for Loop {
    type Stream = Pipe;
    fn endpoint(&self) -> Result<(u16, String), String> {
        if self.configured.load(Ordering::Acquire) {
            Ok((PORT, TOKEN.to_owned()))
        } else {
            Err("MCP is off in Windfall's settings".into())
        }
    }
    fn connect(&self, port: u16) -> io::Result<Pipe> {
        assert_eq!(port, PORT);
        if !self.listening.load(Ordering::Acquire) {
            return Err(io::Error::from(io::ErrorKind::ConnectionRefused));
        }
        Ok(Pipe {
            server: self.server.clone(),
            host: self.host.clone(),
            request: Vec::new(),
            response: None,
        })
    }
}

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);
impl Write for Shared {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn link() -> Loop {
    Loop {
        server: Arc::new(Server::new("1.0.1-test")),
        host: Arc::new(Fixture::new()),
        configured: AtomicBool::new(true),
        listening: AtomicBool::new(true),
    }
}

#[test]
fn the_stdio_bridge_relays_a_session() {
    let link = link();
    let server = link.server.clone();
    let bridge = Arc::new(Bridge::new(link));
    bridge.check().unwrap();
    let input = [
        json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "Claude Desktop", "version": "1"}}}),
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
        json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "get_project_summary", "arguments": {}}}),
        json!({"jsonrpc": "2.0", "id": "four", "method": "tools/call", "params": {"name": "create_pattern", "arguments": {"name": "Verse"}}}),
    ]
    .iter()
    .map(|message| message.to_string())
    .collect::<Vec<_>>()
    .join("\n")
        + "\nnot json\n";
    let output = Shared::default();
    bridge::run(
        bridge.clone(),
        Cursor::new(input.into_bytes()),
        Box::new(output.clone()),
    );
    let written = String::from_utf8(output.0.lock().unwrap().clone()).unwrap();
    let answers: Vec<Value> = written
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(answers.len(), 5, "{written}");
    let by_id = |id: Value| {
        answers
            .iter()
            .find(|answer| answer["id"] == id)
            .unwrap_or_else(|| panic!("{id}: {written}"))
    };
    assert_eq!(by_id(json!(1))["result"]["protocolVersion"], "2025-06-18");
    assert!(by_id(json!(2))["result"]["tools"].as_array().unwrap().len() > 20);
    let summary = &by_id(json!(3))["result"];
    assert_eq!(summary["isError"], false);
    assert_eq!(summary["structuredContent"]["name"], "Fixture");
    let proposal = &by_id(json!("four"))["result"]["structuredContent"];
    assert_eq!(proposal["status"]["state"], "pending");
    // The session's client name labels its proposals.
    let entry = server
        .approvals()
        .get(proposal["proposalId"].as_str().unwrap())
        .unwrap();
    assert_eq!(entry.agent, "Claude Desktop");
    assert_eq!(entry.proposal.label, "Claude Desktop: Create pattern");
    assert_eq!(by_id(Value::Null)["error"]["code"], -32700);
}

#[test]
fn the_bridge_reopens_a_forgotten_session_and_reports_a_missing_app() {
    let link = link();
    let server = link.server.clone();
    let bridge = Bridge::new(link);
    let init = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "agent", "version": "1"}}});
    assert!(bridge.exchange(&init).unwrap()["result"].is_object());
    server.reset();
    let list = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
    let answer = bridge.exchange(&list).unwrap();
    assert!(answer["result"]["tools"].is_array(), "{answer}");
    // Not listening: a clear error for requests, nothing for notifications.
    let link = self::link();
    link.listening.store(false, Ordering::Release);
    let bridge = Bridge::new(link);
    assert_eq!(bridge.check(), Err(bridge::NOT_RUNNING.to_owned()));
    let answer = bridge.exchange(&list).unwrap();
    assert_eq!(answer["id"], 2);
    assert!(
        answer["error"]["message"]
            .as_str()
            .unwrap()
            .contains("not running")
    );
    assert_eq!(
        bridge.exchange(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"})),
        None
    );
    let link = self::link();
    link.configured.store(false, Ordering::Release);
    assert!(Bridge::new(link).check().unwrap_err().contains("off"));
}

// --- Read tools ------------------------------------------------------------

#[test]
fn summary_lists_ids_and_the_key_the_notes_suggest() {
    let (server, host) = setup();
    let ids = host.ids;
    let summary = ok(&call(&server, &host, "get_project_summary", json!({})));
    assert_eq!(summary["meter"], "4/4");
    assert_eq!(summary["ppq"], 960);
    let channels = summary["channels"].as_array().unwrap();
    assert_eq!(channels[0]["id"], ids.bass.0);
    assert_eq!(channels[0]["name"], "Bass");
    assert_eq!(channels[0]["source"]["instrument"], "subtractiveSynth");
    assert_eq!(channels[0]["mixerTrack"], ids.bass_track.0);
    assert_eq!(summary["patterns"][0]["id"], ids.pattern.0);
    assert_eq!(summary["patterns"][0]["notes"][0]["count"], 8);
    assert_eq!(summary["playlist"]["tracks"][0]["id"], ids.playlist.0);
    let mixer = summary["mixer"].as_array().unwrap();
    assert_eq!(mixer[0]["id"], 0);
    let bass = mixer
        .iter()
        .find(|track| track["id"] == ids.bass_track.0)
        .unwrap();
    assert_eq!(bass["effects"][0]["id"], ids.reverb);
    assert_eq!(bass["effects"][0]["kind"], "reverb");
    assert_eq!(bass["inputs"]["channels"][0], ids.bass.0);
    assert_eq!(summary["keyFromNotes"]["name"], "A minor");
    assert_eq!(summary["songEnd"], "2:1:0");
}

#[test]
fn paged_queries_page() {
    let (server, host) = setup();
    let ids = host.ids;
    let first = ok(&call(
        &server,
        &host,
        "list_notes",
        json!({"patternId": ids.pattern.0, "limit": 3}),
    ));
    assert_eq!(first["total"], 8);
    assert_eq!(first["items"].as_array().unwrap().len(), 3);
    assert_eq!(first["items"][0]["pitch"], "A3");
    assert_eq!(first["items"][1]["start"], "1:1:480");
    assert_eq!(first["nextOffset"], 3);
    let last = ok(&call(
        &server,
        &host,
        "list_notes",
        json!({"patternId": ids.pattern.0, "offset": 6, "limit": 3}),
    ));
    assert_eq!(last["items"].as_array().unwrap().len(), 2);
    assert_eq!(last["nextOffset"], Value::Null);
    let none = ok(&call(
        &server,
        &host,
        "list_notes",
        json!({"patternId": ids.pattern.0, "channelId": ids.lead.0}),
    ));
    assert_eq!(none["total"], 0);
    let clips = ok(&call(
        &server,
        &host,
        "list_clips",
        json!({"from": "1:1:0", "to": "3:1:0"}),
    ));
    assert_eq!(clips["items"][0]["id"], ids.clip);
    assert_eq!(clips["items"][0]["content"]["patternId"], ids.pattern.0);
    let later = ok(&call(
        &server,
        &host,
        "list_clips",
        json!({"from": "5:1:0"}),
    ));
    assert_eq!(later["total"], 0);
    assert!(
        failure(&call(
            &server,
            &host,
            "list_notes",
            json!({"patternId": 1, "limit": 501})
        ))
        .contains("limit")
    );
    let track = ok(&call(
        &server,
        &host,
        "get_mixer_track",
        json!({"trackId": ids.bass_track.0}),
    ));
    assert_eq!(track["effects"][0]["name"], EffectKind::Reverb.name());
    assert!(
        failure(&call(
            &server,
            &host,
            "get_mixer_track",
            json!({"trackId": 4000})
        ))
        .contains("4000")
    );
}

#[test]
fn parameters_list_ranges_units_and_values() {
    let (server, host) = setup();
    let ids = host.ids;
    let synth = ok(&call(
        &server,
        &host,
        "get_parameters",
        json!({"channelId": ids.bass.0}),
    ));
    let descriptors = InstrumentKind::SubtractiveSynth.descriptors();
    assert_eq!(synth["total"], descriptors.len());
    let first = &synth["items"][0];
    assert_eq!(first["id"], descriptors[0].id);
    assert_eq!(first["min"], json!(descriptors[0].min));
    assert!(first["value"].is_number());
    let reverb = ok(&call(
        &server,
        &host,
        "get_parameters",
        json!({"trackId": ids.bass_track.0, "effectId": ids.reverb}),
    ));
    assert_eq!(reverb["total"], EffectKind::Reverb.descriptors().len());
    assert_eq!(reverb["target"]["effect"], "reverb");
    let processing = ok(&call(
        &server,
        &host,
        "get_parameters",
        json!({"trackId": ids.bass_track.0, "limit": 1}),
    ));
    assert_eq!(processing["items"].as_array().unwrap().len(), 1);
    let name = descriptors[0].name;
    let searched = ok(&call(
        &server,
        &host,
        "get_parameters",
        json!({"channelId": ids.bass.0, "search": name}),
    ));
    assert!(searched["items"].as_array().unwrap().iter().all(|item| {
        item["name"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .contains(&name.to_lowercase())
            || item["id"]
                .as_str()
                .unwrap()
                .to_lowercase()
                .contains(&name.to_lowercase())
    }));
    assert!(failure(&call(&server, &host, "get_parameters", json!({}))).contains("channelId"));
}

#[test]
fn selection_reports_focus_and_transport() {
    let (server, host) = setup();
    *host.focus.lock().unwrap() = Focus {
        channel: Some(host.ids.lead),
        track: Some(host.ids.bass_track),
        view: Some("pianoRoll".into()),
    };
    let selection = ok(&call(&server, &host, "get_selection", json!({})));
    assert_eq!(selection["pattern"]["id"], host.ids.pattern.0);
    assert_eq!(selection["channel"]["name"], "Lead");
    assert_eq!(selection["mixerTrack"]["id"], host.ids.bass_track.0);
    assert_eq!(selection["view"], "pianoRoll");
    assert_eq!(selection["mode"], "pattern");
}

#[test]
fn kinds_list_every_instrument_and_effect() {
    let (server, host) = setup();
    let all = ok(&call(
        &server,
        &host,
        "list_instruments_and_effects",
        json!({}),
    ));
    let items = all["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        InstrumentKind::ALL.len() + EffectKind::ALL.len()
    );
    assert!(
        items
            .iter()
            .all(|item| !item["description"].as_str().unwrap().is_empty())
    );
    let effects = ok(&call(
        &server,
        &host,
        "list_instruments_and_effects",
        json!({"category": "effect"}),
    ));
    assert_eq!(
        effects["items"].as_array().unwrap().len(),
        EffectKind::ALL.len()
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "list_instruments_and_effects",
            json!({"category": "x"})
        ))
        .contains("category")
    );
}

// --- Edit tools ------------------------------------------------------------

/// Calls an edit tool, checks it filed a pending proposal without editing,
/// and returns the tool's result and the proposal's commands.
fn propose(server: &Server, host: &Fixture, name: &str, arguments: Value) -> (Value, Vec<Command>) {
    let before = host.project();
    let result = ok(&call(server, host, name, arguments));
    assert_eq!(result["status"]["state"], "pending", "{result}");
    assert_eq!(host.project(), before, "a proposal must not edit");
    let id = result["proposalId"].as_str().unwrap().to_owned();
    (result, pending_commands(server, &id))
}

fn approve(server: &Server, host: &Fixture, result: &Value) -> Status {
    server
        .approvals()
        .apply(host, result["proposalId"].as_str().unwrap(), Approver::User)
        .unwrap()
}

#[test]
fn create_pattern_proposes_the_pattern_and_predicts_its_id() {
    let (server, host) = setup();
    let next = host.project().next_id;
    let (result, commands) = propose(
        &server,
        &host,
        "create_pattern",
        json!({"name": "Verse", "lengthBars": 2, "color": "#3FA7D6"}),
    );
    assert_eq!(result["created"]["patterns"], json!([next]));
    assert_eq!(result["counts"]["patterns"], json!([1, 2]));
    assert_eq!(commands.len(), 2);
    assert!(matches!(&commands[0], Command::AddPattern { name: Some(name) } if name == "Verse"));
    assert!(
        matches!(&commands[1], Command::UpdatePattern { id, patch } if id.0 == next && patch.length_steps == Some(32) && patch.color == Some(0x3FA7D6))
    );
    assert!(matches!(
        approve(&server, &host, &result),
        Status::Applied { .. }
    ));
    let pattern = host.project().pattern(PatternId(next)).cloned().unwrap();
    assert_eq!(pattern.name, "Verse");
    assert_eq!(pattern.length_steps, 32);
    for (arguments, wanted) in [
        (json!({"lengthBars": 1, "lengthSteps": 4}), "not both"),
        (json!({"lengthSteps": 5000}), "lengthSteps"),
        (json!({"color": "blue"}), "colour"),
    ] {
        assert!(
            failure(&call(&server, &host, "create_pattern", arguments)).contains(wanted),
            "{wanted}"
        );
    }
}

#[test]
fn add_notes_reads_note_names_and_positions_and_extends_the_pattern() {
    let (server, host) = setup();
    let ids = host.ids;
    let next = host.project().next_id;
    let (result, commands) = propose(
        &server,
        &host,
        "add_notes",
        json!({
            "patternId": ids.pattern.0,
            "channelId": ids.lead.0,
            "notes": [
                {"pitch": "C5", "start": "1:1:0", "length": "1/4"},
                {"pitch": 76, "start": "2:3:0", "length": "1/8.", "velocity": 100},
                {"pitch": "G4", "start": 480},
            ],
        }),
    );
    assert_eq!(
        result["created"]["notes"],
        json!([next, next + 1, next + 2])
    );
    let Command::AddNotes { notes, channel, .. } = &commands[0] else {
        panic!("{commands:?}")
    };
    assert_eq!(*channel, ids.lead);
    assert_eq!(
        (notes[0].key, notes[0].start, notes[0].length),
        (72, 0, PPQ)
    );
    assert_eq!(
        (notes[1].key, notes[1].start, notes[1].length),
        (76, 6 * PPQ, PPQ * 3 / 4)
    );
    assert!((notes[1].velocity.unwrap() - 100.0 / 127.0).abs() < 1e-6);
    assert_eq!((notes[2].start, notes[2].length), (480, TICKS_PER_STEP));
    // The second note ends in bar 2, so the 16-step pattern grows to 2 bars.
    assert!(
        matches!(&commands[1], Command::UpdatePattern { patch, .. } if patch.length_steps == Some(32))
    );
    approve(&server, &host, &result);
    let project = host.project();
    assert_eq!(
        project
            .pattern(ids.pattern)
            .unwrap()
            .lane(ids.lead)
            .unwrap()
            .notes
            .len(),
        3
    );
    for (note, wanted) in [
        (json!({"pitch": "H4", "start": "1:1:0"}), "note name"),
        (json!({"pitch": 200, "start": "1:1:0"}), "MIDI"),
        (json!({"pitch": "C4", "start": "0:1:0"}), "position"),
        (
            json!({"pitch": "C4", "start": "1:1:0", "length": "1/3"}),
            "length",
        ),
        (
            json!({"pitch": "C4", "start": "1:1:0", "velocity": 300}),
            "velocity",
        ),
        (json!({"pitch": "C4"}), "start"),
    ] {
        let message = failure(&call(
            &server,
            &host,
            "add_notes",
            json!({"patternId": ids.pattern.0, "channelId": ids.lead.0, "notes": [note]}),
        ));
        assert!(
            message.contains(wanted) && message.contains("note 0"),
            "{wanted}: {message}"
        );
    }
    assert!(failure(&call(&server, &host, "add_notes", json!({"patternId": ids.pattern.0, "channelId": 999, "notes": [{"pitch": 60, "start": 0}]}))).contains("channel 999"));
}

#[test]
fn add_notes_replace_lane_removes_the_old_notes_first() {
    let (server, host) = setup();
    let ids = host.ids;
    let (result, commands) = propose(
        &server,
        &host,
        "add_notes",
        json!({
            "patternId": ids.pattern.0, "channelId": ids.bass.0, "replace": "lane",
            "notes": [{"pitch": "A2", "start": "1:1:0", "length": "1/2"}],
        }),
    );
    assert!(matches!(&commands[0], Command::RemoveNotes { notes, .. } if notes.len() == 8));
    assert_eq!(result["counts"]["notes"], json!([8, 1]));
    approve(&server, &host, &result);
    let lane = host
        .project()
        .pattern(ids.pattern)
        .unwrap()
        .lane(ids.bass)
        .unwrap()
        .notes
        .clone();
    assert_eq!(lane.len(), 1);
    assert_eq!(lane[0].key, 45);
    // One proposal, one undo step.
    let history = host.history();
    assert_eq!(
        history.entries[history.cursor as usize - 1].label,
        "test-agent: Add notes"
    );
}

#[test]
fn remove_and_update_notes_pick_notes_by_id_range_and_pitch() {
    let (server, host) = setup();
    let ids = host.ids;
    let notes = host
        .project()
        .pattern(ids.pattern)
        .unwrap()
        .lane(ids.bass)
        .unwrap()
        .notes
        .clone();
    let (_, commands) = propose(
        &server,
        &host,
        "remove_notes",
        json!({"patternId": 1, "channelId": ids.bass.0, "pitch": "E4"}),
    );
    let Command::RemoveNotes { notes: picked, .. } = &commands[0] else {
        panic!()
    };
    assert_eq!(picked.len(), 2);
    let (_, commands) = propose(
        &server,
        &host,
        "remove_notes",
        json!({"patternId": 1, "channelId": ids.bass.0, "from": "1:3:0", "to": "2:1:0"}),
    );
    let Command::RemoveNotes { notes: picked, .. } = &commands[0] else {
        panic!()
    };
    assert_eq!(picked.len(), 4);
    assert!(
        failure(&call(
            &server,
            &host,
            "remove_notes",
            json!({"patternId": 1, "channelId": ids.bass.0, "pitch": "C#2"})
        ))
        .contains("no notes")
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "remove_notes",
            json!({"patternId": 1, "channelId": ids.bass.0})
        ))
        .contains("noteIds")
    );
    let (result, commands) = propose(
        &server,
        &host,
        "update_notes",
        json!({"patternId": 1, "channelId": ids.bass.0, "updates": [
            {"noteId": notes[0].id.0, "transpose": 12},
            {"noteId": notes[1].id.0, "start": "1:4:0", "velocity": 0.25},
        ]}),
    );
    let Command::UpdateCapturedNotes {
        expected, updates, ..
    } = &commands[0]
    else {
        panic!()
    };
    assert_eq!(expected.len(), 2);
    assert_eq!(updates[0].patch.key, Some(notes[0].key + 12));
    assert_eq!(updates[1].patch.start, Some(3 * PPQ));
    approve(&server, &host, &result);
    let lane = host
        .project()
        .pattern(ids.pattern)
        .unwrap()
        .lane(ids.bass)
        .unwrap()
        .notes
        .clone();
    assert!(
        lane.iter()
            .any(|note| note.id == notes[0].id && note.key == notes[0].key + 12)
    );
    for (update, wanted) in [
        (json!({"noteId": 4_000_000}), "no note"),
        (json!({"noteId": notes[2].id.0}), "change at least"),
        (
            json!({"noteId": notes[2].id.0, "transpose": 100}),
            "MIDI range",
        ),
        (
            json!({"noteId": notes[2].id.0, "pitch": 60, "transpose": 1}),
            "transpose",
        ),
    ] {
        assert!(
            failure(&call(
                &server,
                &host,
                "update_notes",
                json!({"patternId": 1, "channelId": ids.bass.0, "updates": [update]})
            ))
            .contains(wanted),
            "{wanted}"
        );
    }
}

#[test]
fn set_steps_turns_steps_on_and_off() {
    let (server, host) = setup();
    let ids = host.ids;
    let (result, commands) = propose(
        &server,
        &host,
        "set_steps",
        json!({"patternId": 1, "channelId": ids.lead.0, "steps": "X... x... x... x...", "pitch": "C4"}),
    );
    let Command::AddNotes { notes, .. } = &commands[0] else {
        panic!("{commands:?}")
    };
    assert_eq!(
        notes.iter().map(|n| n.start).collect::<Vec<_>>(),
        [
            0,
            4 * TICKS_PER_STEP,
            8 * TICKS_PER_STEP,
            12 * TICKS_PER_STEP
        ]
    );
    assert_eq!(notes[0].velocity, Some(1.0));
    assert_eq!(notes[1].velocity, Some(windfall_project::DEFAULT_VELOCITY));
    assert!(
        notes
            .iter()
            .all(|n| n.key == 60 && n.length == TICKS_PER_STEP)
    );
    approve(&server, &host, &result);
    // Turning the second beat off removes its note; on steps already on are left.
    let (_, commands) = propose(
        &server,
        &host,
        "set_steps",
        json!({"patternId": 1, "channelId": ids.lead.0, "steps": "x___-"}),
    );
    assert_eq!(commands.len(), 1);
    assert!(matches!(&commands[0], Command::RemoveNotes { notes, .. } if notes.len() == 1));
    assert!(
        failure(&call(
            &server,
            &host,
            "set_steps",
            json!({"patternId": 1, "channelId": ids.lead.0, "steps": "x___"})
        ))
        .contains("already")
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "set_steps",
            json!({"patternId": 1, "channelId": ids.lead.0, "steps": "x?"})
        ))
        .contains("steps may hold")
    );
}

#[test]
fn channel_and_effect_chain_tools_produce_checked_commands() {
    let (server, host) = setup();
    let ids = host.ids;
    let next = host.project().next_id;
    let (result, commands) = propose(
        &server,
        &host,
        "add_channel",
        json!({"instrument": "fourOp", "name": "Keys"}),
    );
    assert!(matches!(
        &commands[0],
        Command::AddChannel {
            instrument: Some(InstrumentKind::FourOp),
            mixer_track: None,
            ..
        }
    ));
    assert_eq!(result["created"]["channels"], json!([next]));
    assert_eq!(
        result["created"]["mixerTracks"].as_array().unwrap().len(),
        1
    );
    // The shown name works as well as the command name.
    let shown = InstrumentKind::FourOp.name().to_uppercase();
    let (_, commands) = propose(
        &server,
        &host,
        "add_channel",
        json!({"instrument": shown, "mixerTrackId": ids.lead_track.0}),
    );
    assert!(
        matches!(&commands[0], Command::AddChannel { mixer_track: Some(track), .. } if *track == ids.lead_track)
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "add_channel",
            json!({"instrument": "kazoo"})
        ))
        .contains("not a built-in instrument")
    );
    let (result, commands) = propose(
        &server,
        &host,
        "add_effect",
        json!({"trackId": ids.lead_track.0, "kind": "compressor", "index": 0}),
    );
    assert!(matches!(
        &commands[0],
        Command::AddEffect {
            kind: EffectKind::Compressor,
            index: Some(0),
            ..
        }
    ));
    assert_eq!(result["created"]["effects"].as_array().unwrap().len(), 1);
    let (_, commands) = propose(
        &server,
        &host,
        "move_effect",
        json!({"trackId": ids.bass_track.0, "effectId": ids.reverb, "index": 0, "toTrackId": ids.lead_track.0}),
    );
    assert!(
        matches!(&commands[0], Command::MoveEffect { to_track: Some(track), index: 0, .. } if *track == ids.lead_track)
    );
    let (result, commands) = propose(
        &server,
        &host,
        "remove_effect",
        json!({"trackId": ids.bass_track.0, "effectId": ids.reverb}),
    );
    assert!(matches!(&commands[0], Command::RemoveEffect { .. }));
    approve(&server, &host, &result);
    assert!(
        host.project()
            .mixer
            .track(ids.bass_track)
            .unwrap()
            .effects
            .is_empty()
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "remove_effect",
            json!({"trackId": ids.bass_track.0, "effectId": ids.reverb})
        ))
        .contains("no effect")
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "add_effect",
            json!({"trackId": ids.bass_track.0, "kind": "flux"})
        ))
        .contains("not a built-in effect")
    );
}

/// A float setting whose name no other setting of the set shares.
fn unique_float(
    descriptors: &'static [windfall_dsp::ParamInfo],
) -> &'static windfall_dsp::ParamInfo {
    descriptors
        .iter()
        .find(|info| {
            info.kind == windfall_dsp::ParamKind::Float
                && info.max > info.min
                && descriptors
                    .iter()
                    .filter(|other| other.name.eq_ignore_ascii_case(info.name))
                    .count()
                    == 1
        })
        .expect("a float setting with a unique name")
}

#[test]
fn set_parameter_finds_settings_by_name_and_checks_ranges() {
    let (server, host) = setup();
    let ids = host.ids;
    let descriptors = InstrumentKind::SubtractiveSynth.descriptors();
    let info = unique_float(descriptors);
    let index = descriptors
        .iter()
        .position(|d| std::ptr::eq(d, info))
        .unwrap();
    let middle = (info.min + info.max) / 2.0;
    let (result, commands) = propose(
        &server,
        &host,
        "set_parameter",
        json!({"channelId": ids.bass.0, "parameter": info.name, "value": middle}),
    );
    assert!(
        matches!(&commands[0], Command::SetInstrumentParam { channel, param, value } if *channel == ids.bass && *param as usize == index && (*value - middle).abs() < 1e-4)
    );
    approve(&server, &host, &result);
    // By id, and several of one effect at once with its mix.
    let reverb = EffectKind::Reverb.descriptors();
    let effect_info = unique_float(reverb);
    let (_, commands) = propose(
        &server,
        &host,
        "set_parameter",
        json!({"trackId": ids.bass_track.0, "effectId": ids.reverb, "parameters": [
            {"parameter": effect_info.id, "value": effect_info.max},
            {"parameter": "mix", "value": 0.5},
        ]}),
    );
    assert!(
        matches!(&commands[0], Command::SetEffectParam { value, .. } if *value == effect_info.max)
    );
    assert!(matches!(&commands[1], Command::UpdateEffect { patch, .. } if patch.mix == Some(0.5)));
    let (_, commands) = propose(
        &server,
        &host,
        "set_parameter",
        json!({"channelId": ids.lead.0, "parameter": "volume", "value": "-6 dB"}),
    );
    assert!(
        matches!(&commands[0], Command::UpdateChannel { patch, .. } if (patch.volume.unwrap() - 0.501).abs() < 0.01)
    );
    let (_, commands) = propose(
        &server,
        &host,
        "set_parameter",
        json!({"trackId": ids.lead_track.0, "parameter": "pan", "value": -0.5}),
    );
    assert!(
        matches!(&commands[0], Command::UpdateMixerTrack { patch, .. } if patch.pan == Some(-0.5))
    );
    let above = info.max + (info.max - info.min);
    for (arguments, wanted) in [
        (
            json!({"channelId": ids.bass.0, "parameter": info.name, "value": above}),
            "outside",
        ),
        (
            json!({"channelId": ids.bass.0, "parameter": "no such knob", "value": 1}),
            "no setting",
        ),
        (
            json!({"channelId": ids.bass.0, "parameter": 100_000, "value": 1}),
            "no setting 100000",
        ),
        (
            json!({"channelId": ids.bass.0, "parameter": "volume", "value": 3}),
            "outside 0 to 2",
        ),
        (
            json!({"trackId": ids.bass_track.0, "effectId": 9999, "parameter": "mix", "value": 0.1}),
            "no effect",
        ),
        (json!({"channelId": ids.bass.0}), "parameter"),
    ] {
        assert!(
            failure(&call(&server, &host, "set_parameter", arguments)).contains(wanted),
            "{wanted}"
        );
    }
}

#[test]
fn routing_and_send_tools() {
    let (server, host) = setup();
    let ids = host.ids;
    let (result, commands) = propose(
        &server,
        &host,
        "route_channel",
        json!({"channelId": ids.lead.0, "trackId": ids.bass_track.0}),
    );
    assert!(
        matches!(&commands[0], Command::UpdateChannel { patch, .. } if patch.mixer_track == Some(ids.bass_track))
    );
    approve(&server, &host, &result);
    assert_eq!(
        host.project().channel(ids.lead).unwrap().mixer_track,
        ids.bass_track
    );
    let (result, _) = propose(
        &server,
        &host,
        "route_track",
        json!({"trackId": ids.lead_track.0, "outputId": ids.bass_track.0}),
    );
    approve(&server, &host, &result);
    // The way back would loop: the preview refuses it before it is filed.
    let message = failure(&call(
        &server,
        &host,
        "route_track",
        json!({"trackId": ids.bass_track.0, "outputId": ids.lead_track.0}),
    ));
    assert!(!message.is_empty());
    assert!(
        failure(&call(
            &server,
            &host,
            "route_track",
            json!({"trackId": 0, "outputId": ids.lead_track.0})
        ))
        .contains("master")
    );
    // Sends, in a project with nothing routed between the two tracks.
    let (server, host) = setup();
    let (result, commands) = propose(
        &server,
        &host,
        "set_send",
        json!({"fromTrackId": ids.bass_track.0, "toTrackId": ids.lead_track.0, "level": "-12 dB"}),
    );
    assert!(
        matches!(&commands[0], Command::SetSend { gain: Some(gain), .. } if (*gain - 0.251).abs() < 0.01)
    );
    approve(&server, &host, &result);
    let (_, commands) = propose(
        &server,
        &host,
        "set_send",
        json!({"fromTrackId": ids.bass_track.0, "toTrackId": ids.lead_track.0, "remove": true}),
    );
    assert!(matches!(&commands[0], Command::SetSend { gain: None, .. }));
    assert!(
        failure(&call(
            &server,
            &host,
            "set_send",
            json!({"fromTrackId": ids.lead_track.0, "toTrackId": ids.bass_track.0, "remove": true})
        ))
        .contains("no send")
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "set_send",
            json!({"fromTrackId": ids.bass_track.0, "toTrackId": ids.lead_track.0, "level": 9})
        ))
        .contains("outside")
    );
}

#[test]
fn clip_tools_place_move_and_remove() {
    let (server, host) = setup();
    let ids = host.ids;
    let next = host.project().next_id;
    let (result, commands) = propose(
        &server,
        &host,
        "place_clips",
        json!({"clips": [
        {"patternId": ids.pattern.0, "trackId": ids.playlist.0, "start": "3:1:0"},
        {"patternId": ids.pattern.0, "start": "5:1:0", "length": "2:0:0"},
    ], "newTrackName": "Agent"}),
    );
    // A new playlist track first, then one clip id each.
    assert!(
        matches!(&commands[0], Command::AddPlaylistTrack { name: Some(name), .. } if name == "Agent")
    );
    let Command::AddClips { clips } = &commands[1] else {
        panic!("{commands:?}")
    };
    assert_eq!(clips[0].track, ids.playlist);
    assert_eq!(clips[0].start, 8 * PPQ);
    assert_eq!(clips[1].track, PlaylistTrackId(next));
    assert_eq!(clips[1].length, Some(8 * PPQ));
    assert_eq!(result["created"]["clips"], json!([next + 1, next + 2]));
    approve(&server, &host, &result);
    assert_eq!(host.project().playlist.clips.len(), 3);
    let (result, commands) = propose(
        &server,
        &host,
        "move_clips",
        json!({"clips": [{"clipId": ids.clip, "start": "2:1:0", "muted": true}]}),
    );
    assert!(
        matches!(&commands[0], Command::UpdateClips { updates } if updates[0].patch.start == Some(4 * PPQ) && updates[0].patch.muted == Some(true))
    );
    approve(&server, &host, &result);
    let (result, _) = propose(
        &server,
        &host,
        "remove_clips",
        json!({"clipIds": [ids.clip]}),
    );
    approve(&server, &host, &result);
    assert!(
        !host
            .project()
            .playlist
            .clips
            .iter()
            .any(|clip| clip.id.0 == ids.clip)
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "remove_clips",
            json!({"clipIds": [ids.clip]})
        ))
        .contains("no clip")
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "move_clips",
            json!({"clips": [{"clipId": next + 1}]})
        ))
        .contains("change at least")
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "place_clips",
            json!({"clips": [{"start": 0}]})
        ))
        .contains("patternId")
    );
}

#[test]
fn tempo_meter_and_rename_tools() {
    let (server, host) = setup();
    let ids = host.ids;
    let (result, commands) = propose(
        &server,
        &host,
        "set_tempo_meter",
        json!({"tempoBpm": 92.5, "meter": "6/8"}),
    );
    assert!(
        matches!(&commands[0], Command::UpdateSettings { patch } if patch.tempo_bpm == Some(92.5) && patch.time_signature.map(|m| (m.numerator, m.denominator)) == Some((6, 8)))
    );
    assert_eq!(result["counts"]["tempoBpm"], json!([120.0, 92.5]));
    assert_eq!(result["counts"]["meter"], json!(["4/4", "6/8"]));
    approve(&server, &host, &result);
    assert_eq!(host.project().settings.tempo_bpm, 92.5);
    assert!(
        failure(&call(
            &server,
            &host,
            "set_tempo_meter",
            json!({"tempoBpm": 900})
        ))
        .contains("tempoBpm")
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "set_tempo_meter",
            json!({"meter": "7/3"})
        ))
        .contains("meter")
    );
    for (target, id) in [
        ("channel", ids.bass.0),
        ("pattern", ids.pattern.0),
        ("mixerTrack", ids.bass_track.0),
        ("playlistTrack", ids.playlist.0),
    ] {
        let (result, _) = propose(
            &server,
            &host,
            "rename_and_color",
            json!({"target": target, "id": id, "name": "Renamed", "color": "#112233"}),
        );
        approve(&server, &host, &result);
    }
    let project = host.project();
    assert_eq!(project.channel(ids.bass).unwrap().name, "Renamed");
    assert_eq!(project.pattern(ids.pattern).unwrap().color, 0x112233);
    assert_eq!(project.mixer.track(ids.bass_track).unwrap().name, "Renamed");
    assert_eq!(project.playlist.tracks[0].name, "Renamed");
    assert!(
        failure(&call(
            &server,
            &host,
            "rename_and_color",
            json!({"target": "sample", "id": 1, "name": "x"})
        ))
        .contains("target")
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "rename_and_color",
            json!({"target": "channel", "id": ids.bass.0})
        ))
        .contains("name")
    );
}

#[test]
fn write_automation_converts_values_to_the_target_range() {
    let (server, host) = setup();
    let ids = host.ids;
    let next = host.project().next_id;
    let (result, commands) = propose(
        &server,
        &host,
        "write_automation",
        json!({
            "target": {"trackId": ids.bass_track.0, "parameter": "volume"},
            "points": [{"at": "3:1:0", "value": 0.0}, {"at": "2:1:0", "value": 1.0}, {"at": "4:1:0", "value": "0 dB"}],
            "name": "Fade",
        }),
    );
    let Command::AddAutomation {
        target,
        points: Some(points),
        name,
    } = &commands[0]
    else {
        panic!("{commands:?}")
    };
    assert_eq!(
        *target,
        AutomationTarget::TrackVolume {
            track: ids.bass_track
        }
    );
    assert_eq!(name.as_deref(), Some("Fade"));
    // Sorted, made relative to the first point, and normalized: a gain of
    // 1 is sqrt(1/2) on the square taper, and 0 is silence.
    assert_eq!(
        points.iter().map(|p| p.tick).collect::<Vec<_>>(),
        [0, 4 * PPQ, 8 * PPQ]
    );
    assert!((points[0].value - 0.5f32.sqrt()).abs() < 1e-4, "{points:?}");
    assert_eq!(points[1].value, 0.0);
    // Then a playlist track, and a clip that shows it from bar 2.
    assert!(matches!(&commands[1], Command::AddPlaylistTrack { .. }));
    let Command::AddClips { clips } = &commands[2] else {
        panic!()
    };
    assert_eq!(clips[0].start, 4 * PPQ);
    assert_eq!(
        clips[0].content,
        ClipContent::Automation {
            automation: windfall_project::AutomationId(next)
        }
    );
    approve(&server, &host, &result);
    assert_eq!(host.project().automations.len(), 1);
    // Replacing the points of the same automation.
    let (_, commands) = propose(
        &server,
        &host,
        "write_automation",
        json!({
            "target": {"trackId": ids.bass_track.0, "parameter": "volume"},
            "automationId": next,
            "points": [{"at": "2:1:0", "value": 0.5}],
        }),
    );
    assert!(
        matches!(&commands[0], Command::SetAutomationPoints { points, .. } if points[0].tick == 0)
    );
    // A setting of an effect, by name.
    let info = unique_float(EffectKind::Reverb.descriptors());
    let (_, commands) = propose(
        &server,
        &host,
        "write_automation",
        json!({
            "target": {"trackId": ids.bass_track.0, "effectId": ids.reverb, "parameter": info.name},
            "points": [{"at": 0, "value": info.min}, {"at": "2:1:0", "value": info.max}],
            "placeClip": false,
        }),
    );
    assert_eq!(commands.len(), 1);
    let Command::AddAutomation {
        points: Some(points),
        ..
    } = &commands[0]
    else {
        panic!()
    };
    assert_eq!(points[0].value, 0.0);
    assert!((points[1].value - 1.0).abs() < 1e-6);
    let (_, commands) = propose(
        &server,
        &host,
        "write_automation",
        json!({"target": {"parameter": "tempo"}, "points": [{"at": 0, "value": 140}], "placeClip": false}),
    );
    assert!(matches!(
        &commands[0],
        Command::AddAutomation {
            target: AutomationTarget::Tempo,
            ..
        }
    ));
    for (arguments, wanted) in [
        (
            json!({"target": {"trackId": ids.bass_track.0, "parameter": "volume"}, "points": [{"at": 0, "value": 5}]}),
            "outside",
        ),
        (
            json!({"target": {"parameter": "tempo"}, "points": [{"at": 0, "value": 600}]}),
            "outside",
        ),
        (
            json!({"target": {"channelId": ids.bass.0, "parameter": "volume"}, "automationId": next, "points": [{"at": 0, "value": 1}]}),
            "another target",
        ),
        (
            json!({"target": {"parameter": "volume"}, "points": [{"at": 0, "value": 1}]}),
            "channelId",
        ),
    ] {
        assert!(
            failure(&call(&server, &host, "write_automation", arguments)).contains(wanted),
            "{wanted}"
        );
    }
}

#[test]
fn raw_commands_keep_the_proposal_registry() {
    let (server, host) = setup();
    let ids = host.ids;
    let (result, commands) = propose(
        &server,
        &host,
        "apply_raw_commands",
        json!({"commands": [
        {"type": "addMixerTrack", "name": "Bus"},
        {"type": "updateChannel", "id": ids.lead.0, "patch": {"pan": -0.25}},
    ], "title": "Bus and pan"}),
    );
    assert_eq!(commands.len(), 2);
    assert_eq!(result["label"], "test-agent: Bus and pan");
    approve(&server, &host, &result);
    assert_eq!(host.project().channel(ids.lead).unwrap().pan, -0.25);
    for (command, wanted) in [
        (
            json!({"type": "removeChannel", "id": ids.lead.0}),
            "registry",
        ),
        (json!({"type": "batch", "commands": []}), "registry"),
        (
            json!({"type": "addSample", "name": "x", "path": {"kind": "external", "path": "C:/x.wav"}}),
            "registry",
        ),
        (
            json!({"type": "updateSettings", "patch": {"name": "Renamed"}}),
            "registry",
        ),
        (json!({"type": "teleport"}), "not a Windfall command"),
    ] {
        assert!(
            failure(&call(
                &server,
                &host,
                "apply_raw_commands",
                json!({"commands": [command]})
            ))
            .contains(wanted),
            "{wanted}"
        );
    }
}

// --- Approvals -------------------------------------------------------------

#[test]
fn proposals_wait_for_the_user_and_apply_as_one_undo_step() {
    let (server, host) = setup();
    let before = host.history().cursor;
    let (result, _) = propose(
        &server,
        &host,
        "add_channel",
        json!({"instrument": "pluck", "title": "Pluck lead"}),
    );
    let id = result["proposalId"].as_str().unwrap().to_owned();
    assert!(
        host.changes.load(Ordering::Acquire) > 0,
        "the panel is told"
    );
    let status = ok(&call(
        &server,
        &host,
        "get_proposal",
        json!({"proposalId": id}),
    ));
    assert_eq!(status["status"]["state"], "pending");
    assert_eq!(status["stale"], false);
    // The agent may not apply it itself.
    assert!(
        failure(&call(
            &server,
            &host,
            "apply_proposal",
            json!({"proposalId": id})
        ))
        .contains("waiting for the user")
    );
    assert!(matches!(
        approve(&server, &host, &result),
        Status::Applied { .. }
    ));
    let history = host.history();
    assert_eq!(history.cursor, before + 1);
    assert_eq!(
        history.entries[history.cursor as usize - 1].label,
        "test-agent: Pluck lead"
    );
    assert_eq!(host.project().channels.len(), 3);
    let status = ok(&call(
        &server,
        &host,
        "get_proposal",
        json!({"proposalId": id}),
    ));
    assert_eq!(status["status"]["state"], "applied");
    // Applying twice is refused.
    assert!(
        server
            .approvals()
            .apply(&host, &id, Approver::User)
            .is_err()
    );
}

#[test]
fn rejected_withdrawn_and_stale_proposals_never_apply() {
    let (server, host) = setup();
    let (first, _) = propose(&server, &host, "create_pattern", json!({}));
    let first = first["proposalId"].as_str().unwrap().to_owned();
    server.approvals().reject(&first).unwrap();
    assert!(
        server
            .approvals()
            .apply(&host, &first, Approver::User)
            .is_err()
    );
    assert_eq!(
        ok(&call(
            &server,
            &host,
            "get_proposal",
            json!({"proposalId": first})
        ))["status"]["state"],
        "rejected"
    );
    let (second, _) = propose(&server, &host, "create_pattern", json!({}));
    let second = second["proposalId"].as_str().unwrap().to_owned();
    ok(&call(
        &server,
        &host,
        "withdraw_proposal",
        json!({"proposalId": second}),
    ));
    assert_eq!(
        server.approvals().get(&second).unwrap().status,
        Status::Withdrawn
    );
    // An unrelated edit by hand makes a pending proposal stale.
    let (third, _) = propose(&server, &host, "create_pattern", json!({}));
    let third = third["proposalId"].as_str().unwrap().to_owned();
    host.edit(Command::AddMixerTrack {
        name: Some("By hand".into()),
    });
    assert_eq!(
        ok(&call(
            &server,
            &host,
            "get_proposal",
            json!({"proposalId": third})
        ))["stale"],
        true
    );
    let before = host.project();
    let Status::Failed { error } = server
        .approvals()
        .apply(&host, &third, Approver::User)
        .unwrap()
    else {
        panic!("a stale proposal applied")
    };
    assert!(error.contains("stale"), "{error}");
    assert_eq!(host.project(), before);
    // Proposals of other agents are not this agent's to see.
    let other = server
        .approvals()
        .submit(
            "someone else",
            server.approvals().get(&third).unwrap().proposal,
            Default::default(),
        )
        .unwrap();
    assert!(
        failure(&call(
            &server,
            &host,
            "get_proposal",
            json!({"proposalId": other})
        ))
        .contains("another agent")
    );
    assert!(
        failure(&call(
            &server,
            &host,
            "get_proposal",
            json!({"proposalId": "p999"})
        ))
        .contains("no proposal")
    );
}

#[test]
fn a_trusted_agent_applies_without_asking() {
    let (server, host) = setup();
    server.approvals().set_trusted(AGENT, true);
    let before = host.history().cursor;
    let result = ok(&call(
        &server,
        &host,
        "create_pattern",
        json!({"name": "Auto"}),
    ));
    assert_eq!(result["status"]["state"], "applied");
    assert!(
        host.project()
            .patterns
            .iter()
            .any(|pattern| pattern.name == "Auto")
    );
    assert_eq!(host.history().cursor, before + 1);
    let entry = server
        .approvals()
        .get(result["proposalId"].as_str().unwrap())
        .unwrap();
    assert!(entry.automatic);
    // Trust withdrawn: back to asking.
    server.approvals().set_trusted(AGENT, false);
    let (pending, _) = propose(&server, &host, "create_pattern", json!({}));
    // Allowed after filing: the agent may apply its own pending proposal.
    server.approvals().set_trusted(AGENT, true);
    let applied = ok(&call(
        &server,
        &host,
        "apply_proposal",
        json!({"proposalId": pending["proposalId"]}),
    ));
    assert_eq!(applied["status"]["state"], "applied");
    // Turning the server off ends every allowance.
    server.reset();
    assert!(!server.approvals().trusted(AGENT));
}

#[test]
fn proposals_are_refused_while_recording() {
    let (server, host) = setup();
    host.recording.store(true, Ordering::Release);
    let message = failure(&call(&server, &host, "create_pattern", json!({})));
    assert!(message.contains("recording"), "{message}");
    assert!(server.approvals().entries().is_empty());
    // A proposal filed before recording began cannot apply during it.
    host.recording.store(false, Ordering::Release);
    let (result, _) = propose(&server, &host, "create_pattern", json!({}));
    host.recording.store(true, Ordering::Release);
    let before = host.project();
    assert!(matches!(
        approve(&server, &host, &result),
        Status::Failed { .. }
    ));
    assert_eq!(host.project(), before);
}

#[test]
fn waiting_returns_when_the_user_decides() {
    let (server, host) = setup();
    let (result, _) = propose(&server, &host, "create_pattern", json!({}));
    let id = result["proposalId"].as_str().unwrap().to_owned();
    std::thread::scope(|scope| {
        let waiter = scope.spawn(|| {
            ok(&call(
                &server,
                &host,
                "get_proposal",
                json!({"proposalId": id, "waitSeconds": 30}),
            ))
        });
        std::thread::sleep(Duration::from_millis(50));
        server.approvals().reject(&id).unwrap();
        let started = Instant::now();
        let status = waiter.join().unwrap();
        assert_eq!(status["status"]["state"], "rejected");
        assert!(started.elapsed() < Duration::from_secs(10));
    });
    // With no decision the wait ends at its time.
    let (result, _) = propose(&server, &host, "create_pattern", json!({}));
    let status = ok(&call(
        &server,
        &host,
        "get_proposal",
        json!({"proposalId": result["proposalId"], "waitSeconds": 1}),
    ));
    assert_eq!(status["status"]["state"], "pending");
}

#[test]
fn at_most_sixteen_proposals_wait_at_once() {
    let (server, host) = setup();
    for _ in 0..crate::approvals::MAX_WAITING {
        ok(&call(&server, &host, "create_pattern", json!({})));
    }
    assert!(
        failure(&call(&server, &host, "create_pattern", json!({}))).contains("already waiting")
    );
}

// --- Listen ----------------------------------------------------------------

#[test]
fn render_and_measure_reports_the_render_and_its_file() {
    let (server, host) = setup();
    let ids = host.ids;
    let result = call(
        &server,
        &host,
        "render_and_measure",
        json!({"region": {"from": "1:1:0", "to": "3:1:0"}, "soloTrackId": ids.bass_track.0, "maxSeconds": 30}),
    );
    let value = ok(&result);
    let measured = &value["measurements"];
    assert!(
        (measured["integratedLufs"].as_f64().unwrap() + 6.02).abs() < 0.2,
        "{measured}"
    );
    assert!((measured["truePeakDbtp"].as_f64().unwrap() + 6.02).abs() < 0.15);
    assert_eq!(measured["clipping"], false);
    assert_eq!(measured["octaveBands"].as_array().unwrap().len(), 10);
    assert_eq!(value["wavPath"], "C:/Windfall/Agent renders/Render-1.wav");
    let link = &result["content"][1];
    assert_eq!(link["type"], "resource_link");
    assert_eq!(
        link["uri"],
        "file:///C:/Windfall/Agent%20renders/Render-1.wav"
    );
    assert_eq!(link["mimeType"], "audio/wav");
    let request = host.rendered.lock().unwrap().unwrap();
    assert_eq!(
        request.region,
        crate::host::RenderRegion::Range {
            start: 0,
            end: 8 * PPQ
        }
    );
    assert_eq!(request.solo, Some(ids.bass_track));
    assert_eq!(request.max_seconds, 30.0);
    ok(&call(
        &server,
        &host,
        "render_and_measure",
        json!({"region": {"patternId": ids.pattern.0, "loops": 2}}),
    ));
    assert_eq!(
        host.rendered.lock().unwrap().unwrap().region,
        crate::host::RenderRegion::Pattern {
            pattern: ids.pattern,
            loops: 2
        }
    );
    for (arguments, wanted) in [
        (json!({"maxSeconds": 500}), "maxSeconds"),
        (json!({"region": "everything"}), "region"),
        (json!({"region": {"from": "3:1:0", "to": "2:1:0"}}), "after"),
        (json!({"soloTrackId": 777}), "777"),
    ] {
        assert!(
            failure(&call(&server, &host, "render_and_measure", arguments)).contains(wanted),
            "{wanted}"
        );
    }
}

#[test]
fn file_uris_escape_what_a_uri_cannot_hold() {
    assert_eq!(
        crate::tools::file_uri("/home/me/a b#.wav"),
        "file:///home/me/a%20b%23.wav"
    );
    assert_eq!(
        crate::tools::file_uri("C:\\Users\\Me\\x.wav"),
        "file:///C:/Users/Me/x.wav"
    );
}

#[test]
fn agent_names_are_clean_and_bounded() {
    assert_eq!(
        protocol::agent_name(Some(
            &json!({"name": "claude-code", "title": "Claude Code"})
        )),
        "Claude Code"
    );
    assert_eq!(
        protocol::agent_name(Some(&json!({"name": "x\u{7}y"}))),
        "xy"
    );
    assert_eq!(protocol::agent_name(None), protocol::UNNAMED_AGENT);
    assert_eq!(
        protocol::agent_name(Some(&json!({"name": "a".repeat(100)}))).len(),
        40
    );
    assert!(crate::edits::history_label(&"é".repeat(100), "Edit").len() <= 128);
}

// --- SoundFonts, the library, channel presets, silent channels -------------

impl Fixture {
    /// A SoundFont channel with no SoundFont loaded, and notes, as an agent
    /// left the "Choir" in a song.
    fn empty_choir(&self) -> ChannelId {
        let mut document = self.document.lock().unwrap();
        let applied = document
            .dispatch(
                Command::AddChannel {
                    name: Some("Choir".into()),
                    sample: None,
                    instrument: Some(InstrumentKind::SoundFont),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap();
        document.patch(&applied.touched);
        let choir = ChannelId(applied.created[0]);
        let applied = document
            .dispatch(
                Command::AddNotes {
                    pattern: self.ids.pattern,
                    channel: choir,
                    notes: vec![NoteInit {
                        start: 0,
                        length: PPQ,
                        key: 60,
                        velocity: None,
                        pan: None,
                        expression: None,
                    }],
                },
                None,
            )
            .unwrap();
        document.patch(&applied.touched);
        choir
    }

    /// Undoes the newest step, as Ctrl+Z does.
    fn undo(&self) {
        let mut document = self.document.lock().unwrap();
        let touched = document.undo().expect("a step to undo");
        document.patch(&touched);
    }

    fn last_label(&self) -> String {
        let history = self.history();
        history.entries[history.cursor as usize - 1].label.clone()
    }
}

fn soundfont_of(project: &Project, channel: ChannelId) -> Option<(u16, u16, String)> {
    match &project.channel(channel)?.sample_instrument {
        Some(windfall_project::SampleInstrumentAsset::SoundFont {
            bank,
            program,
            path,
            ..
        }) => {
            let windfall_project::SamplePath::External(path) = path else {
                return None;
            };
            Some((*bank, *program, path.clone()))
        }
        _ => None,
    }
}

#[test]
fn list_soundfont_presets_reads_the_file_through_windfall_with_families() {
    let (server, host) = setup();
    let listed = ok(&call(
        &server,
        &host,
        "list_soundfont_presets",
        json!({"path": SOUNDFONT}),
    ));
    assert_eq!(listed["path"], SOUNDFONT);
    assert_eq!(listed["presetCount"], 4);
    let items = listed["items"].as_array().unwrap();
    assert_eq!(items[1]["name"], "Choir Aahs");
    assert_eq!(items[1]["family"], "Ensemble");
    assert_eq!(items[1]["generalMidiName"], "Choir Aahs");
    assert_eq!(items[2]["family"], "Brass");
    assert_eq!(items[2]["generalMidiName"], "French Horn");
    assert_eq!(items[3]["family"], "Drum Kits");
    assert!(items[3]["generalMidiName"].is_null());
    // Search by name or family, and paging.
    let horn = ok(&call(
        &server,
        &host,
        "list_soundfont_presets",
        json!({"path": SOUNDFONT, "search": "brass"}),
    ));
    assert_eq!(horn["total"], 1);
    assert_eq!(horn["items"][0]["program"], 60);
    let first = ok(&call(
        &server,
        &host,
        "list_soundfont_presets",
        json!({"path": SOUNDFONT, "limit": 1, "offset": 1}),
    ));
    assert_eq!(first["items"].as_array().unwrap().len(), 1);
    assert_eq!(first["nextOffset"], 2);
    // Files outside the browser's folders are refused before anything is read.
    for path in ["D:/Elsewhere/GM.sf2", "C:/Library/../Windows/GM.sf2"] {
        let refused = failure(&call(
            &server,
            &host,
            "list_soundfont_presets",
            json!({"path": path}),
        ));
        assert!(refused.contains("not inside"), "{refused}");
    }
    assert!(
        failure(&call(&server, &host, "list_soundfont_presets", json!({})))
            .contains("path")
    );
}

#[test]
fn load_soundfont_preset_is_a_proposal_applied_as_one_undo_step() {
    let (server, host) = setup();
    let choir = host.empty_choir();
    let before = host.project();
    let cursor = host.history().cursor;
    let (result, commands) = propose(
        &server,
        &host,
        "load_soundfont_preset",
        json!({"channelId": choir.0, "path": SOUNDFONT, "program": 52, "title": "Choir voice"}),
    );
    // The load is Windfall's own edit, not raw commands.
    assert!(commands.is_empty());
    let entry = server
        .approvals()
        .get(result["proposalId"].as_str().unwrap())
        .unwrap();
    assert!(matches!(
        &entry.native,
        Some(NativeEdit::SoundFont { channel, bank: 0, program: 52, preset, replace_instrument: false, .. })
            if *channel == choir && preset == "Choir Aahs"
    ));
    assert!(
        result["changes"][0]
            .as_str()
            .unwrap()
            .contains("\"Choir Aahs\" (bank 0, program 52)")
    );
    assert_eq!(host.project(), before);
    assert!(matches!(
        approve(&server, &host, &result),
        Status::Applied { .. }
    ));
    assert_eq!(
        soundfont_of(&host.project(), choir),
        Some((0, 52, SOUNDFONT.to_owned()))
    );
    assert_eq!(host.history().cursor, cursor + 1);
    assert_eq!(host.last_label(), "test-agent: Choir voice");
    // The summary no longer calls it silent.
    let summary = ok(&call(&server, &host, "get_project_summary", json!({})));
    let listed = summary["channels"]
        .as_array()
        .unwrap()
        .iter()
        .find(|channel| channel["id"] == choir.0)
        .unwrap()
        .clone();
    assert_eq!(listed["silent"], false);
    host.undo();
    assert_eq!(host.project().channel(choir), before.channel(choir));
    assert_eq!(host.history().cursor, cursor);
}

#[test]
fn load_soundfont_preset_refuses_other_instruments_unknown_presets_and_outside_files() {
    let (server, host) = setup();
    let ids = host.ids;
    let choir = host.empty_choir();
    for (arguments, wanted) in [
        (
            json!({"channelId": ids.lead.0, "path": SOUNDFONT, "program": 52}),
            "replaceInstrument",
        ),
        (
            json!({"channelId": choir.0, "path": SOUNDFONT, "program": 99}),
            "no preset at bank 0, program 99",
        ),
        (
            json!({"channelId": choir.0, "path": SOUNDFONT, "bank": 1, "program": 0}),
            "no preset at bank 1",
        ),
        (
            json!({"channelId": choir.0, "path": "D:/Downloads/GM.sf2", "program": 52}),
            "not inside",
        ),
        (
            json!({"channelId": choir.0, "path": "C:\\Library\\..\\x.sf2", "program": 52}),
            "not inside",
        ),
        (
            json!({"channelId": 999, "path": SOUNDFONT, "program": 52}),
            "no channel 999",
        ),
        (
            json!({"channelId": choir.0, "path": SOUNDFONT}),
            "program",
        ),
    ] {
        let refused = failure(&call(&server, &host, "load_soundfont_preset", arguments));
        assert!(refused.contains(wanted), "{wanted}: {refused}");
    }
    assert!(!server.approvals().waiting());
    // Raw commands still cannot register a SoundFont or set a bank.
    for command in [
        json!({"type": "registerSoundFontSource", "sample": 1, "expected": {"kind": "external", "path": SOUNDFONT}}),
        json!({"type": "setSampleInstrumentAsset", "channel": choir.0, "asset": {"type": "soundFont", "path": {"kind": "external", "path": SOUNDFONT}, "bank": 0, "program": 52}}),
    ] {
        let refused = failure(&call(
            &server,
            &host,
            "apply_raw_commands",
            json!({"commands": [command]}),
        ));
        assert!(refused.contains("registry"), "{refused}");
    }
}

#[test]
fn load_soundfont_preset_can_make_a_synth_channel_a_soundfont_player() {
    let (server, host) = setup();
    let ids = host.ids;
    let before = host.project();
    let cursor = host.history().cursor;
    let (result, _) = propose(
        &server,
        &host,
        "load_soundfont_preset",
        json!({"channelId": ids.lead.0, "path": SOUNDFONT, "program": 60, "replaceInstrument": true}),
    );
    assert!(
        result["changes"][0]
            .as_str()
            .unwrap()
            .contains("SoundFont player instead of")
    );
    approve(&server, &host, &result);
    let after = host.project();
    let lead = after.channel(ids.lead).unwrap();
    assert!(matches!(
        &lead.source,
        windfall_project::ChannelSource::Instrument { params, .. } if params.kind() == InstrumentKind::SoundFont
    ));
    assert_eq!(lead.name, "Lead");
    assert_eq!(lead.mixer_track, ids.lead_track);
    assert_eq!(
        soundfont_of(&after, ids.lead),
        Some((0, 60, SOUNDFONT.to_owned()))
    );
    assert_eq!(host.history().cursor, cursor + 1);
    host.undo();
    assert_eq!(host.project().channel(ids.lead), before.channel(ids.lead));
}

#[test]
fn a_soundfont_proposal_goes_stale_like_any_other() {
    let (server, host) = setup();
    let choir = host.empty_choir();
    let (result, _) = propose(
        &server,
        &host,
        "load_soundfont_preset",
        json!({"channelId": choir.0, "path": SOUNDFONT, "program": 52}),
    );
    host.edit(Command::UpdateSettings {
        patch: windfall_project::SettingsPatch {
            tempo_bpm: Some(99.0),
            ..Default::default()
        },
    });
    let id = result["proposalId"].as_str().unwrap();
    let status = server
        .approvals()
        .apply(&host, id, Approver::User)
        .unwrap();
    assert!(matches!(status, Status::Failed { error } if error.contains("stale")));
    assert_eq!(soundfont_of(&host.project(), choir), None);
}

#[test]
fn add_channel_with_a_soundfont_is_playable_in_one_proposal() {
    let (server, host) = setup();
    let next = host.project().next_id;
    let cursor = host.history().cursor;
    let (result, commands) = propose(
        &server,
        &host,
        "add_channel",
        json!({"name": "Horns", "soundFont": {"path": SOUNDFONT, "program": 60}}),
    );
    assert_eq!(result["created"]["channels"], json!([next]));
    assert!(result.get("warning").is_none(), "{result}");
    assert!(
        matches!(&commands[..], [Command::AddChannel { instrument: Some(InstrumentKind::SoundFont), .. }])
    );
    approve(&server, &host, &result);
    let after = host.project();
    assert_eq!(after.channel(ChannelId(next)).unwrap().name, "Horns");
    assert_eq!(
        soundfont_of(&after, ChannelId(next)),
        Some((0, 60, SOUNDFONT.to_owned()))
    );
    assert_eq!(host.history().cursor, cursor + 1);
    host.undo();
    assert!(host.project().channel(ChannelId(next)).is_none());
    // Refusals.
    for (arguments, wanted) in [
        (
            json!({"instrument": "fourOp", "soundFont": {"path": SOUNDFONT, "program": 60}}),
            "soundFont is for a SoundFont channel",
        ),
        (
            json!({"soundFont": {"path": "E:/x.sf2", "program": 60}}),
            "not inside",
        ),
        (json!({"soundFont": "GM.sf2"}), "soundFont must be"),
        (json!({}), "instrument is required"),
    ] {
        let refused = failure(&call(&server, &host, "add_channel", arguments));
        assert!(refused.contains(wanted), "{wanted}: {refused}");
    }
}

#[test]
fn silent_instruments_are_marked_and_warned_about() {
    let (server, host) = setup();
    let kinds = ok(&call(
        &server,
        &host,
        "list_instruments_and_effects",
        json!({"category": "instrument"}),
    ));
    let kind = |name: &str| {
        kinds["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["kind"] == name)
            .unwrap()
            .clone()
    };
    for name in [
        "soundFont",
        "multiSampler",
        "samplePlayer",
        "padBank",
        "textVoice",
        "sliceMap",
        "grainCloud",
    ] {
        assert_eq!(kind(name)["needsSource"], true, "{name}");
        assert!(
            kind(name)["description"]
                .as_str()
                .unwrap()
                .contains("silent until"),
            "{name}"
        );
    }
    for name in ["subtractiveSynth", "pianoKeys", "fourOp", "kick"] {
        assert_eq!(kind(name)["needsSource"], false, "{name}");
    }
    // add_channel warns that the channel will be silent.
    let (result, _) = propose(
        &server,
        &host,
        "add_channel",
        json!({"instrument": "multiSampler"}),
    );
    assert!(
        result["warning"]
            .as_str()
            .unwrap()
            .contains("will be silent until it has a source")
    );
    let (result, _) = propose(
        &server,
        &host,
        "add_channel",
        json!({"instrument": "subtractiveSynth"}),
    );
    assert!(result.get("warning").is_none());
    // The summary flags the channel with no source.
    let choir = host.empty_choir();
    let summary = ok(&call(&server, &host, "get_project_summary", json!({})));
    let channel = |id: ChannelId| {
        summary["channels"]
            .as_array()
            .unwrap()
            .iter()
            .find(|channel| channel["id"] == id.0)
            .unwrap()
            .clone()
    };
    assert_eq!(channel(choir)["silent"], true);
    assert!(
        channel(choir)["needs"]
            .as_str()
            .unwrap()
            .contains("load_soundfont_preset")
    );
    assert_eq!(channel(host.ids.bass)["silent"], false);
    let warnings = summary["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0]
            .as_str()
            .unwrap()
            .contains("\"Choir\" is silent")
    );
}

#[test]
fn render_and_measure_lists_the_channels_that_stayed_silent() {
    let (server, host) = setup();
    let choir = host.empty_choir();
    *host.silent.lock().unwrap() = vec![choir, host.ids.lead];
    let value = ok(&call(&server, &host, "render_and_measure", json!({})));
    let silent = value["silentChannels"].as_array().unwrap();
    assert_eq!(silent.len(), 2);
    assert_eq!(silent[0]["id"], choir.0);
    assert_eq!(silent[0]["name"], "Choir");
    assert!(
        silent[0]["reason"]
            .as_str()
            .unwrap()
            .contains("no source loaded")
    );
    assert!(
        silent[1]["reason"]
            .as_str()
            .unwrap()
            .contains("made no sound")
    );
    *host.silent.lock().unwrap() = Vec::new();
    let value = ok(&call(&server, &host, "render_and_measure", json!({})));
    assert_eq!(value["silentChannels"], json!([]));
}

#[test]
fn search_library_pages_the_browser_folders_by_kind() {
    let (server, host) = setup();
    let all = ok(&call(&server, &host, "search_library", json!({})));
    assert_eq!(all["total"], 3);
    let soundfonts = ok(&call(
        &server,
        &host,
        "search_library",
        json!({"kind": "soundFont"}),
    ));
    assert_eq!(soundfonts["total"], 1);
    assert_eq!(soundfonts["items"][0]["path"], SOUNDFONT);
    assert_eq!(soundfonts["items"][0]["kind"], "soundFont");
    assert_eq!(soundfonts["items"][0]["bytes"], 1_000_000);
    let choir = ok(&call(
        &server,
        &host,
        "search_library",
        json!({"query": "choir", "kind": "audio"}),
    ));
    assert_eq!(choir["items"][0]["relativePath"], "Choir/Aah.wav");
    let paged = ok(&call(
        &server,
        &host,
        "search_library",
        json!({"limit": 2}),
    ));
    assert_eq!(paged["items"].as_array().unwrap().len(), 2);
    assert_eq!(paged["nextOffset"], 2);
    assert!(
        failure(&call(
            &server,
            &host,
            "search_library",
            json!({"kind": "plugin"})
        ))
        .contains("kind must be one of")
    );
}

#[test]
fn channel_presets_are_listed_and_applied_as_one_undo_step() {
    let (server, host) = setup();
    let ids = host.ids;
    let listed = ok(&call(&server, &host, "list_channel_presets", json!({})));
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["items"][0]["name"], "Warm Pad");
    assert_eq!(
        ok(&call(
            &server,
            &host,
            "list_channel_presets",
            json!({"search": "organ"})
        ))["total"],
        0
    );
    let cursor = host.history().cursor;
    let (result, commands) = propose(
        &server,
        &host,
        "apply_channel_preset",
        json!({"channelId": ids.lead.0, "preset": "warm pad"}),
    );
    assert!(commands.is_empty());
    assert!(matches!(
        server.approvals().get(result["proposalId"].as_str().unwrap()).unwrap().native,
        Some(NativeEdit::ChannelPreset { channel, ref path, .. })
            if channel == ids.lead && path.ends_with("Warm Pad.wfpreset")
    ));
    approve(&server, &host, &result);
    assert_eq!(host.history().cursor, cursor + 1);
    assert_eq!(host.last_label(), "test-agent: Load channel preset");
    assert!(
        failure(&call(
            &server,
            &host,
            "apply_channel_preset",
            json!({"channelId": ids.lead.0, "preset": "Nope"})
        ))
        .contains("no channel preset")
    );
}

#[test]
fn a_trusted_agent_applies_its_soundfont_load_at_once() {
    let (server, host) = setup();
    let choir = host.empty_choir();
    server.approvals().set_trusted(AGENT, true);
    let result = ok(&call(
        &server,
        &host,
        "load_soundfont_preset",
        json!({"channelId": choir.0, "path": SOUNDFONT, "program": 52}),
    ));
    assert_eq!(result["status"]["state"], "applied", "{result}");
    assert_eq!(
        soundfont_of(&host.project(), choir),
        Some((0, 52, SOUNDFONT.to_owned()))
    );
}
