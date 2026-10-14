//! Export of the playlist tracks apart: the files that land on the disk,
//! read back with the decoder, and what is refused or cancelled.

use std::fs;
use std::path::Path;
use std::sync::mpsc;

use windfall_core::AudioBuffer;
use windfall_ipc::{
    BitDepth, ExportFormat, ExportOptions, ExportPlaylistStems, ExportProgress, ExportStems,
    PlayMode, StemMode,
};
use windfall_project::{
    ClipContent, ClipInit, Command, PlaylistTrackId, PlaylistTrackPatch, TrackId,
};

use super::{Rig, SAMPLE_RATE, rms};
use crate::events::Event;
use crate::sync::lock;

/// One bar at 120 bpm, in frames at the engine's rate.
const BAR: usize = SAMPLE_RATE as usize * 2;

fn options(rig: &Rig, name: &str) -> ExportOptions {
    ExportOptions {
        path: rig.file(name),
        format: ExportFormat::Wav,
        bit_depth: BitDepth::Float32,
        sample_rate: SAMPLE_RATE,
        mode: PlayMode::Song,
        ..ExportOptions::default()
    }
}

fn every_lane() -> ExportPlaylistStems {
    ExportPlaylistStems {
        tracks: None,
        include_mix: true,
        numbered: true,
        folder: true,
    }
}

fn audio(sample: windfall_project::SampleId) -> ClipContent {
    ClipContent::Audio {
        sample,
        mixer_track: TrackId::MASTER,
        output: Default::default(),
        normalize: false,
        gain: 0.5,
        pan: 0.0,
        fade_in: 0,
        fade_out: 0,
        reverse: false,
        pitch: 0.0,
        stretch: Default::default(),
    }
}

/// A kick on every beat of the default pattern, and three lanes: the
/// pattern in the first bar, the kick sample as an audio clip in the second
/// half of that bar, and the same pattern again in the second bar.
fn song(rig: &Rig) -> [PlaylistTrackId; 3] {
    let session = &rig.session;
    for step in [0, 4, 8, 12] {
        let command = Command::ToggleStep {
            pattern: rig.pattern(),
            channel: rig.channel(0),
            step,
        };
        session.dispatch(command, None).unwrap();
    }
    let lanes = ["Drums", "Loop", "Again"].map(|name| {
        let command = Command::AddPlaylistTrack {
            name: Some(name.into()),
            index: None,
        };
        PlaylistTrackId(session.dispatch(command, None).unwrap().created[0])
    });
    let pattern = ClipContent::Pattern {
        pattern: rig.pattern(),
    };
    let clip = |track, start, length, content| ClipInit {
        track,
        start,
        length: Some(length),
        offset: None,
        muted: None,
        content,
    };
    let sample = rig.project().samples[0].id;
    let clips = vec![
        clip(lanes[0], 0, 3_840, pattern.clone()),
        clip(lanes[1], 1_920, 1_920, audio(sample)),
        clip(lanes[2], 3_840, 3_840, pattern),
    ];
    session.dispatch(Command::AddClips { clips }, None).unwrap();
    lanes
}

fn export(rig: &Rig, options: &ExportOptions, stems: &ExportPlaylistStems) -> ExportProgress {
    rig.events.take();
    rig.session
        .export_playlist_tracks(options.clone(), stems.clone())
        .unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!(done.path, options.path);
    done
}

/// Everything in the test's folder but the settings file, sorted, with the
/// files of folders inside it as `folder/file`.
fn written(rig: &Rig) -> Vec<String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(rig.folder.path()).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_dir() {
            let inside: Vec<_> = fs::read_dir(entry.path()).unwrap().collect();
            if inside.is_empty() {
                names.push(format!("{name}/"));
            }
            for file in inside {
                let file = file.unwrap().file_name().to_string_lossy().into_owned();
                names.push(format!("{name}/{file}"));
            }
        } else if name != crate::settings::SETTINGS_FILE {
            names.push(name);
        }
    }
    names.sort();
    names
}

fn decode(path: &str) -> AudioBuffer {
    windfall_codec::decode_file(path).unwrap()
}

fn off_the_sum(whole: &AudioBuffer, parts: &[AudioBuffer]) -> f32 {
    let mut total = vec![0.0_f32; whole.samples().len()];
    for part in parts {
        assert_eq!(part.frames(), whole.frames());
        for (total, sample) in total.iter_mut().zip(part.samples()) {
            *total += sample;
        }
    }
    let differences = total
        .iter()
        .zip(whole.samples())
        .map(|(a, b)| (a - b).abs());
    differences.fold(0.0, f32::max)
}

#[test]
fn sidechain_stems_measure_lane_isolation_and_region_decoded_alignment() {
    use windfall_dsp::{CompressorParams, EffectKind, EffectParams};
    use windfall_project::{EffectId, MixerTrackPatch, TickRange};
    let rig = Rig::new();
    let carrier = TrackId(
        rig.session
            .dispatch(
                Command::AddMixerTrack {
                    name: Some("Carrier".into()),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let key = TrackId(
        rig.session
            .dispatch(
                Command::AddMixerTrack {
                    name: Some("Detector".into()),
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    for id in [TrackId::MASTER, carrier, key] {
        rig.session
            .dispatch(
                Command::UpdateMixerTrack {
                    id,
                    patch: MixerTrackPatch {
                        volume: Some(1.0),
                        pan: Some(0.0),
                        ..Default::default()
                    },
                },
                None,
            )
            .unwrap();
    }
    let make = |name: &str, level: f32, mixer_track| {
        let path = rig.file(name);
        windfall_codec::write_wav(
            Path::new(&path),
            &AudioBuffer::from_interleaved(
                SAMPLE_RATE,
                2,
                vec![level; 4 * SAMPLE_RATE as usize * 2],
            ),
            windfall_codec::WavSampleFormat::Float32,
        )
        .unwrap();
        let result = rig
            .session
            .add_audio_clip_from_file(
                &path,
                crate::session::ClipPlace {
                    track: None,
                    start: 0,
                    mixer_track: Some(mixer_track),
                },
            )
            .unwrap();
        let clip = windfall_project::ClipId(*result.created.last().unwrap());
        rig.project()
            .playlist
            .clips
            .iter()
            .find(|item| item.id == clip)
            .unwrap()
            .track
    };
    let carrier_lane = make("carrier.wav", 0.1, carrier);
    let key_lane = make("key.wav", 0.8, key);
    rig.session
        .dispatch(
            Command::SetTrackOutput {
                id: key,
                output: None,
            },
            None,
        )
        .unwrap();
    let effect = EffectId(
        rig.session
            .dispatch(
                Command::AddEffect {
                    track: carrier,
                    kind: EffectKind::Compressor,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    rig.session
        .dispatch(
            Command::SetEffectParams {
                track: carrier,
                effect,
                params: EffectParams::Compressor(CompressorParams {
                    sidechain: true,
                    knee_db: 0.0,
                    ..Default::default()
                }),
            },
            None,
        )
        .unwrap();
    rig.session
        .dispatch(
            Command::SetSidechain {
                from: key,
                to: carrier,
                gain: Some(1.0),
            },
            None,
        )
        .unwrap();
    let source = rig.session.timeline_state();
    let mut request = options(&rig, "region.wav");
    request.region = Some(TickRange {
        start: 960,
        end: 2880,
    });
    request.region_generation = Some(source.generation);
    request.region_revision = Some(source.revision);
    request.tail_secs = 0.0;
    let done = export(
        &rig,
        &request,
        &ExportPlaylistStems {
            tracks: Some(vec![carrier_lane, key_lane]),
            ..every_lane()
        },
    );
    assert_eq!(done.error, None);
    let files = done.files.unwrap();
    let mix = decode(
        &files
            .iter()
            .find(|file| file.playlist_track.is_none())
            .unwrap()
            .path,
    );
    let stem = decode(
        &files
            .iter()
            .find(|file| file.playlist_track == Some(carrier_lane))
            .unwrap()
            .path,
    );
    let detector = decode(
        &files
            .iter()
            .find(|file| file.playlist_track == Some(key_lane))
            .unwrap()
            .path,
    );
    assert_eq!(mix.frames(), SAMPLE_RATE as usize);
    assert_eq!(stem.frames(), mix.frames());
    assert_eq!(rms(detector.samples()), 0.0, "key bus stays inaudible");
    let steady = |buffer: &AudioBuffer| rms(&buffer.samples()[24000..72000]);
    let expected_ratio = 10.0_f32.powf((-18.0 - 20.0 * 0.8_f32.log10()) * (1.0 - 1.0 / 4.0) / 20.0);
    assert!((steady(&mix) / steady(&stem) - expected_ratio).abs() < 0.02);
    // Independently render the same selected region with the key lane muted.
    rig.session
        .dispatch(
            Command::UpdatePlaylistTrack {
                id: key_lane,
                patch: PlaylistTrackPatch {
                    muted: Some(true),
                    ..Default::default()
                },
            },
            None,
        )
        .unwrap();
    let now = rig.session.timeline_state();
    request.path = rig.file("dry-reference.wav");
    request.region_generation = Some(now.generation);
    request.region_revision = Some(now.revision);
    rig.events.take();
    rig.session.export_audio(request).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);
    let dry = decode(&rig.file("dry-reference.wav"));
    assert_eq!(
        dry.samples(),
        stem.samples(),
        "same region start, end and tail; lane-only detector semantics"
    );
}

#[test]
fn every_playlist_track_lands_in_its_own_file_and_they_add_up_to_the_mix() {
    let rig = Rig::new();
    let lanes = song(&rig);
    let plain = options(&rig, "plain.wav");
    rig.events.take();
    rig.session.export_audio(plain.clone()).unwrap();
    assert_eq!(rig.events.wait_for_export().error, None);

    let apart = options(&rig, "Song.wav");
    let before = rig.session.document_snapshot();
    let done = export(&rig, &apart, &every_lane());
    assert_eq!((done.error.clone(), done.cancelled), (None, None));
    assert_eq!(done.fraction, 1.0);
    assert_eq!(
        rig.session.document_snapshot(),
        before,
        "an export edits nothing"
    );

    let names = [
        "Song/Song - 01 Drums.wav",
        "Song/Song - 02 Loop.wav",
        "Song/Song - 03 Again.wav",
        "Song/Song - Mix.wav",
    ];
    // No file has the name the export was given: that is the folder's.
    assert_eq!(written(&rig)[..4], names);
    assert_eq!(written(&rig), [&names[..], &["plain.wav"]].concat());

    // The last event lists the mix first, then the lanes in playlist order,
    // each with its playlist track and none with a mixer track.
    let files = done.files.unwrap();
    let listed: Vec<_> = files
        .iter()
        .map(|file| {
            let name = Path::new(&file.path).file_name().unwrap();
            (
                name.to_string_lossy().into_owned(),
                file.playlist_track,
                file.track,
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("Song - Mix.wav".to_owned(), None, None),
            ("Song - 01 Drums.wav".to_owned(), Some(lanes[0]), None),
            ("Song - 02 Loop.wav".to_owned(), Some(lanes[1]), None),
            ("Song - 03 Again.wav".to_owned(), Some(lanes[2]), None),
        ]
    );
    for file in &files {
        assert_eq!(
            file.frames,
            2 * BAR as u64,
            "every file is as long as the song"
        );
        assert_eq!(file.bytes, fs::metadata(&file.path).unwrap().len());
    }

    // The mix is the file a plain export writes, byte for byte.
    assert!(fs::read(&files[0].path).unwrap() == fs::read(&plain.path).unwrap());
    let mix = decode(&files[0].path);
    let parts: Vec<AudioBuffer> = files[1..].iter().map(|file| decode(&file.path)).collect();
    for part in &parts {
        assert_eq!((part.sample_rate(), part.channels()), (SAMPLE_RATE, 2));
    }
    // Each lane holds its own music where it sits on the timeline, though
    // two of them play one pattern and all of them one kick sample.
    let (first_bar, second_bar) = (0..BAR * 2, BAR * 2..BAR * 4);
    let (first_half, second_half) = (0..BAR, BAR..BAR * 2);
    let (drums, tape, again) = (parts[0].samples(), parts[1].samples(), parts[2].samples());
    assert!(rms(&drums[first_bar.clone()]) > 0.002);
    // A kick may ring past the bar line; the lane is silent once it has.
    assert!(drums[BAR * 3..].iter().all(|sample| *sample == 0.0));
    assert!(tape[first_half].iter().all(|sample| *sample == 0.0));
    assert!(rms(&tape[second_half]) > 0.002);
    assert!(tape[second_bar.clone()].iter().all(|sample| *sample == 0.0));
    assert!(again[first_bar].iter().all(|sample| *sample == 0.0));
    assert!(rms(&again[second_bar]) > 0.002);

    // Nothing on the way to the output depends on level, so they add up.
    let off = off_the_sum(&mix, &parts);
    assert!(off < 1e-6, "the lanes are off the mix by {off}");
    assert!(off_the_sum(&mix, &parts[1..]) > 0.01);
}

#[test]
fn chosen_lanes_are_written_beside_the_path_and_follow_the_arrangement() {
    let rig = Rig::new();
    let lanes = song(&rig);
    let chosen = ExportPlaylistStems {
        tracks: Some(vec![lanes[2], lanes[0]]),
        include_mix: false,
        numbered: false,
        folder: false,
    };
    let done = export(&rig, &options(&rig, "Take.wav"), &chosen);
    assert_eq!(done.error, None);
    assert_eq!(written(&rig), ["Take - Again.wav", "Take - Drums.wav"]);
    let files = done.files.unwrap();
    let order: Vec<_> = files.iter().map(|file| file.playlist_track).collect();
    assert_eq!(order, [Some(lanes[0]), Some(lanes[2])], "in playlist order");

    // A copied layout with one lane's clip removed exports what it shows.
    let created = rig
        .session
        .dispatch(
            Command::AddArrangementLayout {
                name: "Short".into(),
                copy: true,
            },
            None,
        )
        .unwrap()
        .created;
    let copies = &created[1..];
    let again = rig
        .project()
        .playlist
        .clips
        .iter()
        .find(|clip| copies.contains(&clip.id.0) && clip.track == lanes[2])
        .unwrap()
        .id;
    let remove = Command::RemoveClips { clips: vec![again] };
    rig.session.dispatch(remove, None).unwrap();
    let done = export(&rig, &options(&rig, "Short.wav"), &every_lane());
    assert_eq!(done.error, None);
    let files = done.files.unwrap();
    let order: Vec<_> = files.iter().map(|file| file.playlist_track).collect();
    assert_eq!(order, [None, Some(lanes[0]), Some(lanes[1])]);
    assert!(files.iter().all(|file| file.frames == BAR as u64));
    // The lane the layout left empty cannot be asked for.
    let empty = ExportPlaylistStems {
        tracks: Some(vec![lanes[2]]),
        ..every_lane()
    };
    let error = rig
        .session
        .export_playlist_tracks(options(&rig, "Empty.wav"), empty)
        .unwrap_err();
    let error = windfall_core::english::english(&error);
    assert!(error.contains("plays nothing"), "{error}");
}

#[test]
fn a_playlist_track_export_that_cannot_work_is_refused_before_it_starts() {
    let rig = Rig::new();
    let lanes = song(&rig);
    let session = &rig.session;
    let mute = Command::UpdatePlaylistTrack {
        id: lanes[1],
        patch: PlaylistTrackPatch {
            muted: Some(true),
            ..Default::default()
        },
    };
    session.dispatch(mute, None).unwrap();
    rig.events.take();
    let refused = |options: ExportOptions, stems: ExportPlaylistStems| {
        windfall_core::english::shown(&session.export_playlist_tracks(options, stems).unwrap_err())
    };
    let asked = |tracks: Vec<PlaylistTrackId>| ExportPlaylistStems {
        tracks: Some(tracks),
        include_mix: false,
        ..every_lane()
    };

    let both = ExportOptions {
        stems: Some(ExportStems {
            mode: StemMode::ToMaster,
            tracks: None,
            include_mix: true,
            numbered: false,
            folder: false,
        }),
        ..options(&rig, "No.wav")
    };
    assert_eq!(
        refused(both, every_lane()),
        "Export the mixer tracks or the playlist tracks apart, not both at once."
    );
    let pattern = ExportOptions {
        mode: PlayMode::Pattern,
        ..options(&rig, "No.wav")
    };
    assert_eq!(
        refused(pattern, every_lane()),
        "Playlist tracks belong to the song. Export the song to get a file for each of them."
    );
    assert_eq!(
        refused(
            options(&rig, "No.wav"),
            asked(vec![PlaylistTrackId(999_999)])
        ),
        "The playlist has no track with the id 999999 in this arrangement."
    );
    assert_eq!(
        refused(options(&rig, "No.wav"), asked(Vec::new())),
        "Choose at least one playlist track to export."
    );
    let error = refused(options(&rig, "No.wav"), asked(vec![lanes[1]]));
    assert!(error.contains("plays nothing"), "{error}");
    assert_eq!(
        refused(
            ExportOptions {
                path: String::new(),
                ..options(&rig, "No.wav")
            },
            every_lane()
        ),
        "Choose where to save the file."
    );

    assert!(rig.events.take().is_empty());
    assert!(written(&rig).is_empty());
    // None of the refusals left the session thinking an export is running,
    // and the muted lane is simply left out of an export of every lane.
    let done = export(&rig, &options(&rig, "Fine.wav"), &every_lane());
    assert_eq!(done.error, None);
    let files = done.files.unwrap();
    let order: Vec<_> = files.iter().map(|file| file.playlist_track).collect();
    assert_eq!(order, [None, Some(lanes[0]), Some(lanes[2])]);
}

#[test]
fn a_cancelled_playlist_track_export_leaves_no_file_and_no_folder() {
    let rig = Rig::new();
    song(&rig);
    // Something the export would have replaced.
    fs::create_dir(rig.folder.path().join("Song")).unwrap();
    let kept = rig.folder.path().join("Song").join("Song - Mix.wav");
    fs::write(&kept, b"the take before").unwrap();

    // Holds the export at its first progress event until it is cancelled.
    let (reached, wait_reached) = mpsc::channel();
    let (release, wait_release) = mpsc::channel::<()>();
    let mut held = false;
    *lock(&rig.events.hook) = Some(Box::new(move |event| {
        if matches!(event, Event::ExportProgress(_)) && !held {
            held = true;
            reached.send(()).unwrap();
            wait_release.recv().unwrap();
        }
    }));
    let options = options(&rig, "Song.wav");
    rig.session
        .export_playlist_tracks(options.clone(), every_lane())
        .unwrap();
    wait_reached.recv().unwrap();
    assert_eq!(
        rig.session
            .export_playlist_tracks(options.clone(), every_lane())
            .unwrap_err(),
        "An export is already running."
    );
    rig.session.export_cancel();
    release.send(()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!(
        (done.error, done.cancelled, done.files),
        (None, Some(true), None)
    );
    assert_eq!(written(&rig), ["Song/Song - Mix.wav"]);
    assert_eq!(fs::read(&kept).unwrap(), b"the take before");

    // The next export goes through and replaces it.
    *lock(&rig.events.hook) = None;
    let done = export(&rig, &options, &every_lane());
    assert_eq!((done.error, done.cancelled), (None, None));
    assert_eq!(written(&rig).len(), 4);
    assert_ne!(fs::read(&kept).unwrap(), b"the take before");
}
