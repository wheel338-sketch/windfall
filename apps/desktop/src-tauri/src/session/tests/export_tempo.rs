//! Native export regression: independently read RIFF and locate actual PCM clicks.
//! Never use the production frame map or metadata reader as the expected result.
use super::{Rig, SAMPLE_RATE};
use crate::session::ClipPlace;
use std::collections::BTreeMap;
use windfall_core::AudioBuffer;
use windfall_dsp::control::EnvelopeFollowerParams;
use windfall_ipc::{
    BitDepth, ExportFormat, ExportOptions, ExportPlaylistStems, ExportStems, PlayMode, StemMode,
};
use windfall_project::*;

#[derive(Clone, Copy)]
enum Tempo {
    Static,
    ConstantLinked,
    SyncedLfo,
    AudioDependent,
}

fn dispatch(rig: &Rig, command: Command) -> Vec<u32> {
    rig.session.dispatch(command, None).unwrap().created
}

fn fixture(tempo: Tempo) -> (Rig, PlaylistTrackId, TrackId) {
    let rig = Rig::new();
    let channel = rig.channel(0);
    let track = rig.project().channel(channel).unwrap().mixer_track;
    let click = rig.file("original-click.wav");
    windfall_codec::write_wav(
        &click,
        &AudioBuffer::from_interleaved(SAMPLE_RATE, 1, vec![0.8; 128]),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    rig.session
        .set_channel_sample_from_file(channel, &click)
        .unwrap();
    dispatch(
        &rig,
        Command::SetSamplerEnvelope {
            id: channel,
            envelope: None,
        },
    );
    dispatch(
        &rig,
        Command::UpdateSampler {
            id: channel,
            patch: SamplerPatch {
                root_key: Some(60),
                gain: Some(1.0),
                ..Default::default()
            },
        },
    );
    dispatch(
        &rig,
        Command::UpdateChannel {
            id: channel,
            patch: ChannelPatch {
                name: Some("Click".into()),
                volume: Some(1.0),
                ..Default::default()
            },
        },
    );
    dispatch(
        &rig,
        Command::AddNotes {
            pattern: rig.pattern(),
            channel,
            notes: [0, 960, 1920, 2880]
                .map(|start| NoteInit {
                    start,
                    length: 960,
                    key: 60,
                    velocity: Some(1.0),
                    pan: None,
                    expression: None,
                })
                .to_vec(),
        },
    );
    let lane = PlaylistTrackId(
        dispatch(
            &rig,
            Command::AddPlaylistTrack {
                name: Some("Clicks".into()),
                index: None,
            },
        )[0],
    );
    dispatch(
        &rig,
        Command::AddClips {
            clips: vec![ClipInit {
                track: lane,
                start: 0,
                length: Some(3840),
                offset: None,
                muted: None,
                content: ClipContent::Pattern {
                    pattern: rig.pattern(),
                },
            }],
        },
    );
    for (tick, name, kind) in [
        (960, "Loop start", MarkerKind::Loop { end: 2880 }),
        (1920, "Middle", MarkerKind::Named),
        (2880, "Loop end", MarkerKind::Named),
    ] {
        dispatch(
            &rig,
            Command::AddTimelineMarker {
                tick,
                name: name.into(),
                kind,
            },
        );
    }
    let project = rig.project();
    let pattern = project.pattern(rig.pattern()).unwrap();
    dispatch(
        &rig,
        Command::EditPatternTimeline {
            pattern: pattern.id,
            expected: pattern.timeline.clone(),
            expected_signature: pattern.time_signature,
            edit: PatternTimelineEdit::AddMarker {
                tick: 960,
                name: "Pattern click".into(),
            },
        },
    );
    if !matches!(tempo, Tempo::Static) {
        // Both source types exist in the project. LFO and audio-dependent
        // cases select different outputs, so neither can pass on a static grid.
        let source = EffectId(
            dispatch(
                &rig,
                Command::AddEffect {
                    track: TrackId::MASTER,
                    kind: EffectKind::EnvelopeFollower,
                    index: None,
                },
            )[0],
        );
        dispatch(
            &rig,
            Command::SetEffectParams {
                track: TrackId::MASTER,
                effect: source,
                params: EffectParams::EnvelopeFollower(EnvelopeFollowerParams {
                    attack_ms: 0.0,
                    release_ms: 80.0,
                    ..Default::default()
                }),
            },
        );
        dispatch(
            &rig,
            Command::SetInternalControllerSettings {
                settings: vec![InternalControllerSettings {
                    controller: source.0,
                    sync_beats: Some(0.25),
                    ..Default::default()
                }],
            },
        );
        let link = |output, min: f32, max: f32| ControllerLink {
            source: ControlSource::Internal {
                controller: source.0,
                output,
            },
            target: LinkTarget::Param {
                target: AutomationTarget::Tempo,
            },
            map: ControlMap {
                min: (min - 10.0) / 512.0,
                max: (max - 10.0) / 512.0,
                smoothing_ms: 0.0,
                pickup: PickupMode::Off,
                ..Default::default()
            },
        };
        let links = match tempo {
            Tempo::ConstantLinked => vec![link(1, 120.0, 120.0)],
            Tempo::SyncedLfo => vec![link(0, 90.0, 150.0), link(1, 90.0, 150.0)],
            Tempo::AudioDependent => vec![link(1, 90.0, 150.0), link(0, 90.0, 150.0)],
            Tempo::Static => unreachable!(),
        };
        dispatch(&rig, Command::SetControllerLinks { links });
    }
    if matches!(tempo, Tempo::AudioDependent) {
        // Removing this lane/track changes the detector's real input. Stem
        // replay must nevertheless retain the full-mix clock and click positions.
        let carrier = rig.file("original-detector.wav");
        let data = (0..SAMPLE_RATE as usize * 4)
            .map(|i| if (i / 4800) % 2 == 0 { 0.1 } else { 0.2 })
            .collect();
        windfall_codec::write_wav(
            &carrier,
            &AudioBuffer::from_interleaved(SAMPLE_RATE, 1, data),
            windfall_codec::WavSampleFormat::Float32,
        )
        .unwrap();
        let added = rig
            .session
            .add_audio_clip_from_file(
                &carrier,
                ClipPlace {
                    start: 0,
                    track: None,
                    mixer_track: None,
                },
            )
            .unwrap();
        let clip = ClipId(*added.created.last().unwrap());
        dispatch(
            &rig,
            Command::UpdateClips {
                updates: vec![ClipUpdate {
                    id: clip,
                    patch: ClipPatch {
                        length: Some(3840),
                        ..Default::default()
                    },
                }],
            },
        );
    }
    // Exercise the saved native project path before any export.
    let save = rig.file("clock.windfall");
    rig.session.project_save(Some(&save)).unwrap();
    rig.session.project_open(&save).unwrap();
    (rig, lane, track)
}

fn options(rig: &Rig, name: &str, mode: PlayMode, region: Option<TickRange>) -> ExportOptions {
    ExportOptions {
        path: rig.file(name),
        format: ExportFormat::Wav,
        bit_depth: BitDepth::Float32,
        sample_rate: SAMPLE_RATE,
        mode,
        region,
        pattern_loops: 1,
        tail_secs: 0.0,
        auto_tail: false,
        ..Default::default()
    }
}

fn le(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
fn chunks(mut bytes: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut result = Vec::new();
    while !bytes.is_empty() {
        assert!(bytes.len() >= 8);
        let length = le(bytes, 4) as usize;
        let next = 8 + length + length % 2;
        assert!(next <= bytes.len());
        result.push((bytes[..4].try_into().unwrap(), &bytes[8..8 + length]));
        bytes = &bytes[next..];
    }
    result
}
struct Written {
    cues: BTreeMap<String, u32>,
    slices: BTreeMap<String, u32>,
    loops: Vec<(u32, u32)>,
    notes: Vec<String>,
    acid: Option<(f32, u32)>,
    clicks: Vec<u32>,
    frames: u32,
}
fn inspect(path: &str) -> Written {
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    assert_eq!(le(&bytes, 4) as usize + 8, bytes.len());
    let mut cue_frames = BTreeMap::new();
    let mut labels = BTreeMap::new();
    let mut lengths = BTreeMap::new();
    let mut written = Written {
        cues: BTreeMap::new(),
        slices: BTreeMap::new(),
        loops: Vec::new(),
        notes: Vec::new(),
        acid: None,
        clicks: Vec::new(),
        frames: 0,
    };
    for (id, body) in chunks(&bytes[12..]) {
        match &id {
            b"fmt " => {
                assert_eq!(u16::from_le_bytes(body[..2].try_into().unwrap()), 3);
                assert_eq!(u16::from_le_bytes(body[2..4].try_into().unwrap()), 2);
                assert_eq!(le(body, 4), SAMPLE_RATE);
            }
            b"cue " => {
                assert_eq!(body.len(), 4 + le(body, 0) as usize * 24);
                for item in body[4..].as_chunks::<24>().0 {
                    assert_eq!(&item[8..12], b"data");
                    assert_eq!(le(item, 4), le(item, 20));
                    cue_frames.insert(le(item, 0), le(item, 20));
                }
            }
            b"LIST" if body.starts_with(b"adtl") => {
                for (id, item) in chunks(&body[4..]) {
                    match &id {
                        b"labl" => {
                            labels.insert(
                                le(item, 0),
                                String::from_utf8(item[4..item.len() - 1].to_vec()).unwrap(),
                            );
                        }
                        b"ltxt" => {
                            lengths.insert(le(item, 0), le(item, 4));
                        }
                        b"note" => written
                            .notes
                            .push(String::from_utf8(item[4..item.len() - 1].to_vec()).unwrap()),
                        _ => {}
                    }
                }
            }
            b"smpl" => {
                assert_eq!(body.len(), 36 + le(body, 28) as usize * 24);
                for item in body[36..].as_chunks::<24>().0 {
                    written.loops.push((le(item, 8), le(item, 12) + 1));
                }
            }
            b"acid" => written.acid = Some((f32::from_bits(le(body, 20)), le(body, 12))),
            b"data" => {
                assert!(body.len().is_multiple_of(8));
                written.frames = (body.len() / 8) as u32;
                let mut previous = 0.0_f32;
                for (frame, samples) in body.as_chunks::<8>().0.iter().enumerate() {
                    let left = f32::from_le_bytes(samples[..4].try_into().unwrap());
                    // Detector carrier steps are <= 0.1. Click attacks are
                    // > 0.25 and lie on exact note-on output frames.
                    if left - previous > 0.25 {
                        written.clicks.push(frame as u32);
                    }
                    previous = left;
                }
            }
            _ => {}
        }
    }
    for (id, frame) in cue_frames {
        let label = labels.remove(&id).unwrap();
        // Repeat-local IDs can share a label; retain them with their frame.
        let key = format!("{label}@{frame}");
        written.cues.insert(key.clone(), frame);
        if let Some(length) = lengths.remove(&id) {
            written.slices.insert(key, length);
        }
    }
    written
}
fn frames_for(written: &Written, label: &str) -> Vec<u32> {
    let mut frames: Vec<_> = written
        .cues
        .iter()
        .filter(|(key, _)| key.starts_with(&format!("{label}@")))
        .map(|(_, frame)| *frame)
        .collect();
    frames.sort_unstable();
    frames
}
fn assert_song(written: &Written, region: bool) {
    assert_eq!(written.clicks.len(), if region { 3 } else { 4 });
    let skip = usize::from(!region);
    assert_eq!(
        frames_for(written, "Loop start"),
        vec![written.clicks[skip]]
    );
    assert_eq!(
        frames_for(written, "Middle"),
        vec![written.clicks[skip + 1]]
    );
    assert_eq!(
        frames_for(written, "Loop end"),
        vec![written.clicks[skip + 2]]
    );
    assert_eq!(
        written.loops,
        vec![(written.clicks[skip], written.clicks[skip + 2])]
    );
    assert!(written.notes.contains(&format!(
        "WindfallNavigation/1 loop {}",
        written.clicks[skip + 2]
    )));
}
fn assert_tempo(written: &Written, constant: bool, beats: u32) {
    if constant {
        assert_eq!(written.acid, Some((120.0, beats)));
        assert!(
            written
                .notes
                .iter()
                .any(|note| note.starts_with("WindfallBeats/1 120 "))
        );
    } else {
        assert_eq!(written.acid, None);
        assert!(
            !written
                .notes
                .iter()
                .any(|note| note.starts_with("WindfallBeats/1"))
        );
    }
}
fn exports_follow_pcm(tempo: Tempo) {
    let (rig, lane, track) = fixture(tempo);
    let constant = matches!(tempo, Tempo::Static | Tempo::ConstantLinked);
    let before = rig.session.document_snapshot();
    for region in [
        None,
        Some(TickRange {
            start: 960,
            end: 3840,
        }),
    ] {
        let export = options(
            &rig,
            if region.is_some() {
                "region.wav"
            } else {
                "full.wav"
            },
            PlayMode::Song,
            region,
        );
        rig.events.take();
        rig.session.export_audio(export.clone()).unwrap();
        let done = rig.events.wait_for_export();
        assert_eq!(done.error, None);
        assert!(
            done.wav_metadata
                .unwrap()
                .contains(if constant { "written" } else { "omitted" })
        );
        let full = inspect(&export.path);
        assert_song(&full, region.is_some());
        assert_tempo(&full, constant, if region.is_some() { 3 } else { 4 });
        if constant {
            assert_eq!(
                full.clicks,
                if region.is_some() {
                    vec![0, 24000, 48000]
                } else {
                    vec![0, 24000, 48000, 72000]
                }
            );
        } else if region.is_none() {
            assert_ne!(full.clicks[1], 24000, "linked tempo had no measured effect");
        }
        if region.is_none() {
            // The shorter range begins in the same deterministic epoch. Its
            // final output boundary must coincide with the next full-file click.
            rig.events.take();
            let short = options(
                &rig,
                "boundary.wav",
                PlayMode::Song,
                Some(TickRange {
                    start: 0,
                    end: 1920,
                }),
            );
            rig.session.export_audio(short.clone()).unwrap();
            assert_eq!(rig.events.wait_for_export().error, None);
            let short = inspect(&short.path);
            assert_eq!(short.frames, full.clicks[2]);
            assert_eq!(frames_for(&short, "Middle"), vec![short.frames]);
        }
        for mode in [StemMode::TrackOutputs, StemMode::ToMaster] {
            let mut export = export.clone();
            export.path = rig.file("mixer.wav");
            export.stems = Some(ExportStems {
                mode,
                tracks: Some(vec![track]),
                include_mix: false,
                numbered: false,
                folder: false,
            });
            rig.events.take();
            rig.session.export_audio(export).unwrap();
            let done = rig.events.wait_for_export();
            assert_eq!(done.error, None);
            let stem = inspect(&done.files.unwrap()[0].path);
            assert_eq!(stem.frames, full.frames);
            assert_eq!(stem.clicks, full.clicks);
            assert_eq!(stem.cues, full.cues);
            assert_song(&stem, region.is_some());
            assert_tempo(&stem, constant, if region.is_some() { 3 } else { 4 });
        }
        rig.events.take();
        rig.session
            .export_playlist_tracks(
                options(&rig, "playlist.wav", PlayMode::Song, region),
                ExportPlaylistStems {
                    tracks: Some(vec![lane]),
                    include_mix: false,
                    numbered: false,
                    folder: false,
                },
            )
            .unwrap();
        let done = rig.events.wait_for_export();
        assert_eq!(done.error, None);
        let stem = inspect(&done.files.unwrap()[0].path);
        assert_eq!(stem.frames, full.frames);
        assert_eq!(stem.clicks, full.clicks);
        assert_song(&stem, region.is_some());
        assert_tempo(&stem, constant, if region.is_some() { 3 } else { 4 });
    }
    let mut export = options(&rig, "pattern.wav", PlayMode::Pattern, None);
    export.pattern_loops = 2;
    rig.events.take();
    rig.session.export_audio(export.clone()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!(done.error, None);
    let pattern = inspect(&export.path);
    assert_eq!(pattern.clicks.len(), 8);
    assert_eq!(frames_for(&pattern, "Click: MIDI 60"), pattern.clicks);
    assert_eq!(
        frames_for(&pattern, "Pattern click"),
        vec![pattern.clicks[1], pattern.clicks[5]]
    );
    for (i, &frame) in pattern.clicks.iter().enumerate() {
        let end = pattern.clicks.get(i + 1).copied().unwrap_or(pattern.frames);
        assert_eq!(
            pattern.slices[&format!("Click: MIDI 60@{frame}")],
            end - frame
        );
    }
    assert_tempo(&pattern, constant, 8);
    for mode in [StemMode::TrackOutputs, StemMode::ToMaster] {
        let mut stem_export = export.clone();
        stem_export.path = rig.file("pattern-stem.wav");
        stem_export.stems = Some(ExportStems {
            mode,
            tracks: Some(vec![track]),
            include_mix: false,
            numbered: false,
            folder: false,
        });
        rig.events.take();
        rig.session.export_audio(stem_export).unwrap();
        let done = rig.events.wait_for_export();
        assert_eq!(done.error, None);
        let stem = inspect(&done.files.unwrap()[0].path);
        assert_eq!(stem.clicks, pattern.clicks);
        assert_eq!(stem.cues, pattern.cues);
        assert_eq!(stem.slices, pattern.slices);
        assert_eq!(stem.frames, pattern.frames);
        assert_tempo(&stem, constant, 8);
    }
    assert_eq!(rig.session.document_snapshot(), before);
}

#[test]
fn native_wav_synced_lfo_cues_loops_regions_note_slices_and_stems_match_pcm() {
    exports_follow_pcm(Tempo::SyncedLfo);
}
#[test]
fn native_wav_audio_dependent_tempo_cues_loops_regions_note_slices_and_stems_match_pcm() {
    exports_follow_pcm(Tempo::AudioDependent);
}
#[test]
fn native_wav_constant_and_constant_linked_tempo_keep_offsets_beats_and_stems() {
    exports_follow_pcm(Tempo::Static);
    exports_follow_pcm(Tempo::ConstantLinked);
}
