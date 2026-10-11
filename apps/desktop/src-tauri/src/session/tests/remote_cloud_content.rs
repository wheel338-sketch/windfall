//! Acceptance harnesses written without execution by the implementation worker.
use super::{Rig, archive::start_recording};
use crate::paths;
use crate::session::{ClipPlace, phone_remote::Action};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;
use windfall_project::{ClipContent, Command, SettingsPatch, TrackId};

fn place() -> ClipPlace {
    ClipPlace {
        track: None,
        start: 0,
        mixer_track: None,
    }
}
#[test]
fn loop_drop_creates_blank_rows_and_clip_in_one_undo_step() {
    let rig = Rig::new();
    let before = rig.project();
    let catalog = rig.session.loop_catalog();
    let row = before.playlist.tracks.len() + 3;
    rig.session
        .loop_insert_with_rows(
            "house-a",
            None,
            place(),
            catalog.generation,
            catalog.revision,
            Some(row),
        )
        .unwrap();
    let project = rig.project();
    assert_eq!(
        project.playlist.clips[0].track,
        project.playlist.tracks[row].id
    );
    rig.session.undo().unwrap();
    let mut restored = rig.project();
    assert!(restored.next_id > before.next_id);
    restored.next_id = before.next_id; // Allocated IDs remain retired after undo.
    assert_eq!(restored, before);
}
#[test]
fn remote_named_command_undo_stale_and_recording_guards() {
    let rig = Rig::new();
    let generation = rig.session.loop_catalog().generation;
    let before = rig.project();
    rig.session
        .remote_action(
            generation,
            &Action::Fader {
                track: TrackId::MASTER,
                value: 0.4,
            },
        )
        .unwrap();
    assert_eq!(rig.project().mixer.tracks[0].volume, 0.4);
    rig.session.undo().unwrap();
    assert_eq!(rig.project(), before);
    assert!(
        rig.session
            .remote_action(
                generation + 1,
                &Action::Fader {
                    track: TrackId::MASTER,
                    value: 0.1
                }
            )
            .is_err()
    );
    assert!(
        rig.session
            .remote_action(
                generation,
                &Action::Fader {
                    track: TrackId::MASTER,
                    value: f32::NAN
                }
            )
            .is_err()
    );
    assert!(
        rig.session
            .remote_action(
                generation,
                &Action::NoteOn {
                    channel: rig.channel(0),
                    key: 128,
                    velocity: 0.8
                }
            )
            .is_err()
    );
    start_recording(&rig);
    assert!(
        rig.session
            .remote_action(generation, &Action::Play)
            .is_err()
    );
    assert!(
        rig.session
            .remote_action(generation, &Action::Stop)
            .is_err()
    );
    assert!(
        rig.session
            .remote_action(
                generation,
                &Action::Fader {
                    track: TrackId::MASTER,
                    value: 0.1
                }
            )
            .is_err()
    );
    assert!(
        rig.session
            .remote_action(
                generation,
                &Action::NoteOn {
                    channel: rig.channel(0),
                    key: 60,
                    velocity: 0.8
                }
            )
            .is_err()
    );
    rig.session.recording_cancel();
    rig.session.project_new().unwrap();
    assert!(
        rig.session
            .remote_action(generation, &Action::Stop)
            .is_err()
    );
}
fn http(address: &str, method: &str, path: &str, body: &str) -> String {
    let mut stream = TcpStream::connect(address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut out = String::new();
    stream.read_to_string(&mut out).unwrap();
    out
}
fn ws_send(stream: &mut TcpStream, value: &serde_json::Value) {
    let bytes = value.to_string().into_bytes();
    let mut head = vec![0x81];
    if bytes.len() < 126 {
        head.push(0x80 | bytes.len() as u8);
    } else {
        head.push(0xfe);
        head.extend((bytes.len() as u16).to_be_bytes());
    }
    head.extend([0, 0, 0, 0]);
    head.extend(bytes);
    stream.write_all(&head).unwrap();
}
fn ws_receive(stream: &mut TcpStream) -> serde_json::Value {
    let mut head = [0; 2];
    stream.read_exact(&mut head).unwrap();
    assert_eq!(head[0], 0x81);
    let size = if head[1] == 126 {
        let mut n = [0; 2];
        stream.read_exact(&mut n).unwrap();
        usize::from(u16::from_be_bytes(n))
    } else {
        usize::from(head[1])
    };
    let mut payload = vec![0; size];
    stream.read_exact(&mut payload).unwrap();
    serde_json::from_slice(&payload).unwrap()
}
#[test]
fn loopback_pair_websocket_fader_and_undo() {
    let rig = Rig::new();
    let state = rig.session.phone_remote_set(Some("127.0.0.1:0")).unwrap();
    let address = state
        .address
        .unwrap()
        .trim_start_matches("http://")
        .to_owned();
    assert!(http(&address, "GET", "/", "").contains("Windfall phone remote"));
    let pair = http(
        &address,
        "POST",
        "/pair",
        &serde_json::json!({"code":state.pairing_code.unwrap()}).to_string(),
    );
    let body: serde_json::Value =
        serde_json::from_str(pair.split("\r\n\r\n").nth(1).unwrap()).unwrap();
    let token = body["token"].as_str().unwrap();
    let mut stream = TcpStream::connect(&address).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(stream,"GET /ws HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n").unwrap();
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).unwrap();
        header.push(byte[0]);
    }
    assert!(
        String::from_utf8(header)
            .unwrap()
            .contains("101 Switching Protocols")
    );
    let generation = rig.session.loop_catalog().generation;
    let original = rig.project().mixer.tracks[0].volume;
    ws_send(
        &mut stream,
        &serde_json::json!({"token":token,"sequence":1,"generation":generation,"action":{"name":"fader","track":0,"value":0.5}}),
    );
    assert!(ws_receive(&mut stream)["error"].is_null());
    assert_eq!(rig.project().mixer.tracks[0].volume, 0.5);
    rig.session.undo().unwrap();
    assert_eq!(rig.project().mixer.tracks[0].volume, original);
    // Repeat an accepted sequence: authentication rejects before mutation.
    ws_send(
        &mut stream,
        &serde_json::json!({"token":token,"sequence":1,"generation":generation,"action":{"name":"fader","track":0,"value":0.1}}),
    );
    let mut byte = [0];
    assert!(matches!(stream.read(&mut byte), Ok(0) | Err(_)));
    assert_eq!(rig.project().mixer.tracks[0].volume, original);
    rig.session.phone_remote_set(None).unwrap();
    assert!(!rig.session.phone_remote_state().enabled);
    assert!(rig.session.phone_remote_set(Some("0.0.0.0:0")).is_err());
}
#[test]
fn backup_portable_project_restore_round_trip_and_original_generation() {
    let rig = Rig::new();
    let folder = rig.folder.path().join("cloud-folder");
    std::fs::create_dir(&folder).unwrap();
    let generation = rig.session.loop_catalog().generation;
    let before = rig.session.document_snapshot();
    let version = rig
        .session
        .backup_save_with(
            &paths::display(&folder),
            None,
            3,
            generation,
            rig.session.backup_job().unwrap(),
        )
        .unwrap();
    assert_eq!(rig.session.document_snapshot(), before);
    let destination = rig.folder.path().join("restored.zip");
    rig.session
        .backup_restore_with(
            &paths::display(&folder),
            &version.id,
            &paths::display(&destination),
            rig.session.backup_job().unwrap(),
        )
        .unwrap();
    rig.session
        .project_archive_open(&paths::display(&destination))
        .unwrap();
    let project = rig.project();
    assert_eq!(project.channels, before.project.channels);
    assert!(
        project
            .samples
            .iter()
            .all(|s| matches!(s.path, windfall_project::SamplePath::Project(_)))
    );
    assert!(
        rig.session
            .backup_save_with(
                &paths::display(&folder),
                Some(&version.id),
                3,
                generation,
                rig.session.backup_job().unwrap()
            )
            .unwrap_err()
            .contains("project changed")
    );
}
#[test]
fn loop_fit_insertion_one_undo_save_reopen_and_provenance() {
    let mut rig = Rig::new();
    rig.session
        .dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    tempo_bpm: Some(100.0),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let before = rig.project();
    let history = rig.session.document_snapshot().history.entries.len();
    let catalog = rig.session.loop_catalog();
    rig.session
        .loop_insert(
            "house-a",
            Some(2),
            place(),
            catalog.generation,
            catalog.revision,
        )
        .unwrap();
    let after = rig.project();
    assert_eq!(
        rig.session.document_snapshot().history.entries.len(),
        history + 1
    );
    let clip = &after.playlist.clips[0];
    assert_eq!(clip.length, 8 * windfall_core::PPQ);
    let ClipContent::Audio { sample, .. } = clip.content else {
        panic!("audio clip expected")
    };
    assert_eq!(
        rig.session.state().pool.get(sample).unwrap().frames(),
        230400
    );
    let source = after.sample(sample).unwrap();
    let windfall_project::SamplePath::External(path) = &source.path else {
        panic!("external generated source")
    };
    let metadata =
        std::fs::read_to_string(std::path::Path::new(path).with_extension("wav.json")).unwrap();
    assert!(
        metadata.contains("CC0-1.0")
            && metadata.contains("house-a")
            && metadata.contains("loops-v1")
    );
    rig.session.undo().unwrap();
    let mut restored = rig.project();
    assert_eq!(restored.next_id, after.next_id);
    restored.next_id = before.next_id;
    assert_eq!(restored, before);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), after);
    let saved = paths::display(&rig.folder.path().join("loop.windfall"));
    rig.session.project_save(Some(&saved)).unwrap();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(
        rig.project().playlist.clips[0].length,
        8 * windfall_core::PPQ
    );
    rig.session
        .transport_set(windfall_ipc::TransportPatch {
            mode: Some(windfall_ipc::PlayMode::Song),
            ..Default::default()
        })
        .unwrap();
    rig.session.transport_play().unwrap();
    let audio = rig.run(48000);
    assert!(super::rms(&audio) > 0.01);
}
#[test]
fn rendered_loop_cannot_attach_to_replaced_or_recording_document() {
    for recording in [false, true] {
        let rig = Rig::new();
        let catalog = rig.session.loop_catalog();
        let hold = rig.session.hold("loop:rendered");
        let worker = rig.session.background(move |s| {
            s.loop_insert(
                "ambient-c",
                Some(0),
                place(),
                catalog.generation,
                catalog.revision,
            )
        });
        hold.wait();
        if recording {
            start_recording(&rig);
        } else {
            rig.session.project_new().unwrap();
        }
        let before = rig.project();
        hold.release();
        assert!(worker.join().unwrap().is_err());
        assert_eq!(rig.project(), before);
        if recording {
            rig.session.recording_cancel();
        }
    }
}
