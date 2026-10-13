//! Authored AVI through Session preparation, Processor and the actual wire
//! media clock. Real-window pixels are asserted by video-player-window-smoke.
use super::Rig;
use crate::sync::lock;
use std::sync::{Arc, Mutex};
use windfall_ipc::{MediaClock, PlayMode, RealtimeFrame, TransportPatch};
use windfall_project::{ChannelPatch, Command, StageMedia, TickRange, TrackId, VideoPlacement};

fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = id.to_vec();
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    if !payload.len().is_multiple_of(2) {
        out.push(0);
    }
    out
}
fn list(id: &[u8; 4], children: &[Vec<u8>]) -> Vec<u8> {
    let mut data = id.to_vec();
    for child in children {
        data.extend_from_slice(child);
    }
    chunk(b"LIST", &data)
}
fn stream(kind: &[u8; 4], scale: u32, rate: u32, length: u32, size: u32) -> Vec<u8> {
    let mut data = vec![0; 56];
    data[..4].copy_from_slice(kind);
    for (at, value) in [(20, scale), (24, rate), (32, length), (44, size)] {
        data[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    chunk(b"strh", &data)
}
fn authored_avi() -> Vec<u8> {
    let mut dib = vec![0; 40];
    for (at, value) in [(0, 40_u32), (4, 4), (8, 2), (20, 24)] {
        dib[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    dib[12..14].copy_from_slice(&1_u16.to_le_bytes());
    dib[14..16].copy_from_slice(&24_u16.to_le_bytes());
    let mut wave = vec![0; 16];
    for (at, value) in [(0, 1_u16), (2, 2), (12, 4), (14, 16)] {
        wave[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    wave[4..8].copy_from_slice(&48_000_u32.to_le_bytes());
    wave[8..12].copy_from_slice(&192_000_u32.to_le_bytes());
    let mut avih = vec![0; 56];
    for (at, value) in [(0, 100_000_u32), (16, 80), (24, 2), (32, 4), (36, 2)] {
        avih[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let hdrl = list(
        b"hdrl",
        &[
            chunk(b"avih", &avih),
            list(
                b"strl",
                &[stream(b"vids", 1, 10, 80, 0), chunk(b"strf", &dib)],
            ),
            list(
                b"strl",
                &[
                    stream(b"auds", 4, 192_000, 384_000, 4),
                    chunk(b"strf", &wave),
                ],
            ),
        ],
    );
    let mut movi = Vec::new();
    for frame in 0..80_u8 {
        let mut rgb = Vec::new();
        for y in (0..2).rev() {
            for x in 0..4 {
                rgb.extend_from_slice(&[200 - frame, 30 + x * 11 + y * 7, frame + 1]);
            }
        }
        let mut pcm = Vec::with_capacity(19_200);
        for _ in 0..4800 {
            pcm.extend_from_slice(&(4096_i16 + i16::from(frame) * 32).to_le_bytes());
            pcm.extend_from_slice(&(-8192_i16 - i16::from(frame) * 32).to_le_bytes());
        }
        movi.push(chunk(b"00db", &rgb));
        movi.push(chunk(b"01wb", &pcm));
    }
    let mut body = b"AVI ".to_vec();
    body.extend(hdrl);
    body.extend(list(b"movi", &movi));
    chunk(b"RIFF", &body)
}
fn publication(rig: &Rig, wire: &Mutex<Vec<String>>) -> MediaClock {
    rig.session.realtime_tick();
    let messages = std::mem::take(&mut *lock(wire));
    assert_eq!(messages.len(), 1);
    let frame: RealtimeFrame = serde_json::from_str(&messages[0]).unwrap();
    frame.media_clock.expect("real native media clock")
}
fn assert_pcm(pcm: &[f32], first_source_frame: usize, loop_frames: Option<usize>) {
    // Ignore only the established engine's 5 ms seek fade. Compare every
    // interior stereo sample with the independently authored PCM, not export.
    for (frame, pair) in pcm.chunks_exact(2).enumerate().skip(512) {
        let relative = loop_frames.map_or(frame, |length| frame % length);
        if loop_frames.is_some_and(|length| relative < 512) {
            continue;
        }
        let video_frame = (first_source_frame + relative) / 4800;
        let expected = [
            (4096 + video_frame * 32) as f32 / 32768.0,
            -((8192 + video_frame * 32) as f32) / 32768.0,
        ];
        for side in 0..2 {
            assert!(
                (pair[side] - expected[side]).abs() < 1e-6,
                "frame {frame}, side {side}: {} != {}",
                pair[side],
                expected[side]
            );
        }
    }
}

#[test]
fn authored_rgb_pcm_video_native_clock_offset_seek_loop_stop_and_reopen() {
    let mut rig = Rig::new();
    for channel in rig.project().channels {
        rig.session
            .dispatch(
                Command::UpdateChannel {
                    id: channel.id,
                    patch: ChannelPatch {
                        muted: Some(true),
                        ..Default::default()
                    },
                },
                None,
            )
            .unwrap();
    }
    let path = rig.file("authored-rgb-pcm.avi");
    let bytes = authored_avi();
    std::fs::write(&path, &bytes).unwrap();
    let mut media = StageMedia::default();
    media.videos.push(VideoPlacement {
        path: path.clone(),
        start_tick: 0,
        length_ticks: 7680,
        offset_seconds: 0.0,
        muted: false,
        mixer_track: TrackId::MASTER,
    });
    rig.session
        .dispatch(
            Command::SetStageMedia {
                media: media.clone(),
            },
            None,
        )
        .unwrap();
    assert_eq!(
        rig.session
            .state()
            .pool
            .video_audio(&path)
            .unwrap()
            .frames(),
        384_000
    );
    let guard = rig.session.timeline_state();
    assert_eq!(
        rig.session
            .media_video_read_checked(&path, guard.generation, Some(guard.revision))
            .unwrap(),
        bytes
    );
    rig.session
        .transport_set(TransportPatch {
            mode: Some(PlayMode::Song),
            loop_song: Some(true),
            ..Default::default()
        })
        .unwrap();
    let wire = Arc::new(Mutex::new(Vec::new()));
    let sink = wire.clone();
    rig.session.subscribe_realtime(
        "video-playback-evidence",
        Box::new(move |frame| {
            lock(&sink).push(serde_json::to_string(frame).unwrap());
            true
        }),
    );
    rig.session.transport_play().unwrap();
    assert_pcm(&rig.run(4800), 0, None);
    let start = publication(&rig, &wire);
    assert!(start.playing);
    assert_eq!(start.mode, PlayMode::Song);
    assert!((start.tick - 192.0).abs() < 1e-6);
    rig.session.transport_stop();
    rig.run(480);
    media.videos[0].offset_seconds = 0.75;
    rig.session
        .dispatch(
            Command::SetStageMedia {
                media: media.clone(),
            },
            None,
        )
        .unwrap();
    rig.session.transport_play().unwrap();
    assert_pcm(&rig.run(4800), 36_000, None);
    let offset = publication(&rig, &wire);
    assert!((offset.tick - 192.0).abs() < 1e-6);
    assert_ne!(offset.epoch, start.epoch);
    rig.session.transport_seek(1920.0);
    assert_pcm(&rig.run(4800), 84_000, None);
    let seek = publication(&rig, &wire);
    assert!((seek.tick - 2112.0).abs() < 1e-6);
    assert_ne!(seek.epoch, offset.epoch);
    let guard = rig.session.timeline_state();
    rig.session
        .timeline_region(
            Some(TickRange {
                start: 960,
                end: 1920,
            }),
            guard.generation,
            guard.revision,
        )
        .unwrap();
    rig.session.transport_seek(960.0);
    assert_pcm(&rig.run(28_800), 60_000, Some(24_000));
    let wrapped = publication(&rig, &wire);
    assert!((wrapped.tick - 1152.0).abs() < 1e-6);
    assert!(
        wrapped.output_frame.parse::<u64>().unwrap() > seek.output_frame.parse::<u64>().unwrap()
    );
    rig.session.transport_stop();
    let stopped = rig.run(4096);
    assert!(stopped[1024..].iter().all(|value| *value == 0.0));
    assert!(!publication(&rig, &wire).playing);
    let file = rig.file("video-evidence.windfall");
    rig.session.project_save(Some(&file)).unwrap();
    let reopened = rig.restart();
    reopened.session.project_open(&file).unwrap();
    assert_eq!(reopened.project().settings.stage_media, media);
    let guard = reopened.session.timeline_state();
    assert_eq!(
        reopened
            .session
            .media_video_read_checked(&path, guard.generation, Some(guard.revision))
            .unwrap(),
        bytes
    );
    assert_eq!(
        reopened
            .session
            .state()
            .pool
            .video_audio(&path)
            .unwrap()
            .frames(),
        384_000
    );
}
