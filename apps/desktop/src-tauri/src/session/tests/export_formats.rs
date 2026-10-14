//! Export in every format, as stems, and cancelled: what lands on the
//! disk, read back with the decoder.

use std::fs;
use std::path::Path;
use std::sync::mpsc;

use windfall_core::AudioBuffer;
use windfall_ipc::{
    BitDepth, ExportFormat, ExportOptions, ExportProgress, ExportStems, PlayMode, StemMode,
};
use windfall_project::{Command, EffectKind, TrackId};

use super::{Rig, SAMPLE_RATE, rms};
use crate::events::Event;
use crate::sync::lock;

/// One bar at 120 bpm, in frames at the engine's rate.
const BAR: usize = SAMPLE_RATE as usize * 2;

fn options(rig: &Rig, name: &str, format: ExportFormat) -> ExportOptions {
    ExportOptions {
        path: rig.file(name),
        format,
        bit_depth: BitDepth::Int24,
        sample_rate: SAMPLE_RATE,
        mode: PlayMode::Pattern,
        ..ExportOptions::default()
    }
}

fn stems(mode: StemMode) -> ExportStems {
    ExportStems {
        mode,
        tracks: None,
        include_mix: true,
        numbered: true,
        folder: true,
    }
}

/// Lights steps on all four channels of the default kit, each of which has
/// a mixer track of its own.
fn beat(rig: &Rig) {
    let steps: [&[u32]; 4] = [
        &[0, 4, 8, 12],
        &[4, 12],
        &[0, 2, 4, 6, 8, 10, 12, 14],
        &[6, 14],
    ];
    for (index, steps) in steps.into_iter().enumerate() {
        for &step in steps {
            let command = Command::ToggleStep {
                pattern: rig.pattern(),
                channel: rig.channel(index),
                step,
            };
            rig.session.dispatch(command, None).unwrap();
        }
    }
}

/// The mixer track of the kit's channel at `index`.
fn track_of(rig: &Rig, index: usize) -> TrackId {
    rig.project().channels[index].mixer_track
}

/// Runs an export to its end and returns its last event.
fn export(rig: &Rig, options: &ExportOptions) -> ExportProgress {
    rig.events.take();
    rig.session.export_audio(options.clone()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!(done.path, options.path);
    done
}

/// The names of everything in the test's folder but the settings file,
/// with the files of folders inside it as `folder/file`, sorted.
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

/// The largest difference between the first signal and the sum of the
/// others, sample by sample.
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
fn every_format_is_exported_and_reads_back_as_the_song() {
    let rig = Rig::new();
    beat(&rig);
    for (format, name, codec) in [
        (ExportFormat::Wav, "beat.wav", "pcm_s24le"),
        (ExportFormat::Flac, "beat.flac", "flac"),
        (ExportFormat::Ogg, "beat.ogg", "vorbis"),
        (ExportFormat::Mp3, "beat.mp3", "mp3"),
        (ExportFormat::Wv, "beat.wv", "wavpack"),
    ] {
        for sample_rate in [44_100, 48_000] {
            let options = ExportOptions {
                sample_rate,
                pattern_loops: 2,
                tail_secs: 0.5,
                ..options(&rig, name, format)
            };
            let done = export(&rig, &options);
            let label = format!("{format:?} at {sample_rate} Hz");
            assert_eq!(done.error, None, "{label}");
            assert_eq!((done.fraction, done.cancelled), (1.0, None), "{label}");

            // Two bars of two seconds and half a second of tail.
            let frames = sample_rate as usize * 9 / 2;
            let info = windfall_codec::probe_file(&options.path).unwrap();
            assert_eq!(info.codec, codec, "{label}");
            let audio = decode(&options.path);
            assert_eq!(audio.sample_rate(), sample_rate, "{label}");
            assert_eq!(audio.channels(), 2, "{label}");
            assert_eq!(audio.frames(), frames, "{label}");
            assert!(rms(audio.samples()) > 0.02, "{label}");

            // The last event lists the one file, with its size.
            let files = done.files.expect("the export went through");
            assert_eq!(files.len(), 1, "{label}");
            assert_eq!(files[0].path, options.path, "{label}");
            assert_eq!(files[0].track, None, "{label}");
            assert_eq!(files[0].frames, frames as u64, "{label}");
            assert_eq!(
                files[0].bytes,
                fs::metadata(&options.path).unwrap().len(),
                "{label}"
            );
        }
    }
    assert_eq!(
        written(&rig),
        ["beat.flac", "beat.mp3", "beat.ogg", "beat.wav", "beat.wv"]
    );
}

#[test]
fn lossy_stems_in_both_modes_and_mono_mp3_read_back() {
    let rig = Rig::new();
    beat(&rig);
    for (format, extension) in [(ExportFormat::Ogg, "ogg"), (ExportFormat::Mp3, "mp3")] {
        for mode in [StemMode::TrackOutputs, StemMode::ToMaster] {
            let name = format!("{mode:?}.{extension}");
            let options = ExportOptions {
                stems: Some(stems(mode)),
                ..options(&rig, &name, format)
            };
            let done = export(&rig, &options);
            assert_eq!(done.error, None);
            let files = done.files.unwrap();
            assert_eq!(files.len(), 5);
            for file in files {
                let audio = decode(&file.path);
                assert_eq!(
                    (audio.sample_rate(), audio.channels(), audio.frames()),
                    (SAMPLE_RATE, 2, BAR)
                );
                assert!(rms(audio.samples()) > 0.001, "{}", file.path);
            }
        }
    }
    let options = ExportOptions {
        bit_depth: BitDepth::Float32,
        mp3: Some(windfall_ipc::Mp3Settings {
            rate: windfall_ipc::Mp3Rate::Vbr { quality: 2 },
            channels: windfall_ipc::Mp3Channels::Mono,
        }),
        ..options(&rig, "mono.mp3", ExportFormat::Mp3)
    };
    let done = export(&rig, &options);
    assert_eq!(done.error, None);
    let audio = decode(&options.path);
    assert_eq!((audio.channels(), audio.frames()), (1, BAR));
    assert!(rms(audio.samples()) > 0.02);
}

#[test]
fn invalid_lossy_export_settings_return_readable_errors() {
    let rig = Rig::new();
    let invalid = [
        ExportOptions {
            ogg_quality: Some(11.0),
            ..options(&rig, "bad.ogg", ExportFormat::Ogg)
        },
        ExportOptions {
            sample_rate: 96_000,
            ..options(&rig, "bad.mp3", ExportFormat::Mp3)
        },
        ExportOptions {
            sample_rate: 24_000,
            ..options(&rig, "bad.mp3", ExportFormat::Mp3)
        },
        ExportOptions {
            mp3: Some(windfall_ipc::Mp3Settings {
                rate: windfall_ipc::Mp3Rate::Vbr { quality: 10 },
                channels: windfall_ipc::Mp3Channels::Stereo,
            }),
            ..options(&rig, "bad.mp3", ExportFormat::Mp3)
        },
    ];
    for options in invalid {
        let error = windfall_core::english::shown(&rig.session.export_audio(options).unwrap_err());
        assert!(!error.is_empty() && error.ends_with('.'), "{error}");
        assert!(written(&rig).is_empty());
    }
}

#[test]
fn a_flac_export_holds_exactly_what_a_wav_export_of_its_depth_holds() {
    let rig = Rig::new();
    beat(&rig);
    for bit_depth in [BitDepth::Int16, BitDepth::Int24] {
        let export_as = |format, name: &str, flac_level| {
            let options = ExportOptions {
                bit_depth,
                tail_secs: 0.25,
                flac_level,
                ..options(&rig, name, format)
            };
            assert_eq!(export(&rig, &options).error, None);
            let size = fs::metadata(&options.path).unwrap().len();
            (decode(&options.path), size)
        };
        let (wav, wav_size) = export_as(ExportFormat::Wav, "same.wav", None);
        let (flac, flac_size) = export_as(ExportFormat::Flac, "same.flac", None);
        assert!(flac.samples() == wav.samples(), "{bit_depth:?}");
        assert!(flac_size < wav_size * 7 / 10, "{flac_size} of {wav_size}");

        // The level changes the size of the file and nothing in it.
        let (fast, fast_size) = export_as(ExportFormat::Flac, "fast.flac", Some(0));
        let (small, small_size) = export_as(ExportFormat::Flac, "small.flac", Some(8));
        assert!(fast.samples() == wav.samples(), "{bit_depth:?}");
        assert!(small.samples() == wav.samples(), "{bit_depth:?}");
        assert!(small_size < fast_size, "{small_size} and {fast_size}");
    }
}

#[test]
fn track_output_stems_land_in_a_folder_and_add_up_to_the_mix() {
    let rig = Rig::new();
    beat(&rig);
    // A limiter puts the kick's track 5 ms behind the others inside the
    // engine. The stems only add up if each is lined up with the mix.
    let limiter = Command::AddEffect {
        track: track_of(&rig, 0),
        kind: EffectKind::Limiter,
        index: None,
    };
    rig.session.dispatch(limiter, None).unwrap();

    let plain = ExportOptions {
        bit_depth: BitDepth::Float32,
        tail_secs: 0.5,
        ..options(&rig, "plain.wav", ExportFormat::Wav)
    };
    assert_eq!(export(&rig, &plain).error, None);
    let apart = ExportOptions {
        path: rig.file("Beat.wav"),
        stems: Some(stems(StemMode::TrackOutputs)),
        ..plain.clone()
    };
    let done = export(&rig, &apart);
    assert_eq!(done.error, None);

    let names = [
        "Beat/Beat - 01 Kick Punch.wav",
        "Beat/Beat - 02 Clap Wide.wav",
        "Beat/Beat - 03 Hat Closed 1.wav",
        "Beat/Beat - 04 Snare Tight.wav",
        "Beat/Beat - Mix.wav",
    ];
    // No file has the name the export was given: that is the folder's.
    assert_eq!(written(&rig)[..5], names);
    assert_eq!(written(&rig).len(), 6);

    // The last event lists the mix first and then the stems in mixer
    // order, each with its track.
    let files = done.files.unwrap();
    let listed: Vec<_> = files
        .iter()
        .map(|file| {
            let name = Path::new(&file.path).file_name().unwrap();
            (name.to_string_lossy().into_owned(), file.track)
        })
        .collect();
    let expected: Vec<_> = [None, Some(0), Some(1), Some(2), Some(3)]
        .into_iter()
        .zip([
            "Mix",
            "01 Kick Punch",
            "02 Clap Wide",
            "03 Hat Closed 1",
            "04 Snare Tight",
        ])
        .map(|(index, name)| {
            (
                format!("Beat - {name}.wav"),
                index.map(|i| track_of(&rig, i)),
            )
        })
        .collect();
    assert_eq!(listed, expected);
    let frames = BAR + SAMPLE_RATE as usize / 2;
    for file in &files {
        assert_eq!(file.frames, frames as u64);
        assert_eq!(file.bytes, fs::metadata(&file.path).unwrap().len());
    }

    // The mix is the file a plain export writes, byte for byte, and the
    // four stems are it taken apart.
    assert!(fs::read(&files[0].path).unwrap() == fs::read(&plain.path).unwrap());
    let mix = decode(&files[0].path);
    let parts: Vec<AudioBuffer> = files[1..].iter().map(|file| decode(&file.path)).collect();
    for part in &parts {
        assert_eq!((part.sample_rate(), part.channels()), (SAMPLE_RATE, 2));
        assert!(rms(part.samples()) > 0.002);
    }
    let off = off_the_sum(&mix, &parts);
    assert!(off < 1e-6, "the stems are off the mix by {off}");
    assert!(off_the_sum(&mix, &parts[1..]) > 0.05);
}

#[test]
fn to_master_stems_carry_the_master_effects_and_add_up_to_the_mix() {
    let rig = Rig::new();
    beat(&rig);
    let reverb = Command::AddEffect {
        track: TrackId::MASTER,
        kind: EffectKind::Reverb,
        index: None,
    };
    rig.session.dispatch(reverb, None).unwrap();

    let base = ExportOptions {
        tail_secs: 8.0,
        auto_tail: true,
        ..options(&rig, "Wet.flac", ExportFormat::Flac)
    };
    let export_stems = |mode, tracks: Option<Vec<TrackId>>| {
        let options = ExportOptions {
            stems: Some(ExportStems {
                tracks,
                folder: false,
                numbered: false,
                ..stems(mode)
            }),
            ..base.clone()
        };
        let done = export(&rig, &options);
        assert_eq!(done.error, None);
        let files = done.files.unwrap();
        let audio: Vec<AudioBuffer> = files.iter().map(|file| decode(&file.path)).collect();
        (files, audio)
    };

    // Without a folder the files lie where the path points.
    let (files, audio) = export_stems(StemMode::ToMaster, None);
    assert_eq!(
        written(&rig),
        [
            "Wet - Clap Wide.flac",
            "Wet - Hat Closed 1.flac",
            "Wet - Kick Punch.flac",
            "Wet - Mix.flac",
            "Wet - Snare Tight.flac",
        ]
    );
    // The tail ends with the reverb, and every stem is as long as the mix.
    let frames = audio[0].frames();
    assert!(frames > BAR + 4_800 && frames < BAR + 8 * SAMPLE_RATE as usize);
    assert!(files.iter().all(|file| file.frames == frames as u64));
    // A reverb is linear, so the stems add up to the mix: to within the
    // dither of five 24-bit files and the rounding of a reverb.
    let off = off_the_sum(&audio[0], &audio[1..]);
    assert!(off < 1e-5, "the stems are off the mix by {off}");

    // The kick's stem rings on with the master's reverb after the bar is
    // over. As a track output it is dry and ends with its last hit.
    let kick = track_of(&rig, 0);
    let tail = |audio: &AudioBuffer| rms(&audio.samples()[(BAR + 2_400) * 2..(BAR + 4_800) * 2]);
    assert!(tail(&audio[1]) > 1e-4);
    let (_, dry) = export_stems(StemMode::TrackOutputs, Some(vec![kick]));
    assert_eq!(dry.len(), 2);
    assert_eq!(dry[1].frames(), frames);
    assert!(tail(&dry[1]) < 1e-6);
}

#[test]
fn a_cancelled_export_leaves_no_file_and_keeps_the_ones_it_would_replace() {
    let rig = Rig::new();
    beat(&rig);
    fs::write(rig.file("Long.flac"), b"an earlier export").unwrap();
    fs::create_dir(rig.file("Kept")).unwrap();
    fs::write(rig.file("Kept/Kept - Mix.flac"), b"an earlier mix").unwrap();

    for (name, format, stems) in [
        ("Long.flac", ExportFormat::Flac, None),
        (
            "Fresh.flac",
            ExportFormat::Flac,
            Some(stems(StemMode::TrackOutputs)),
        ),
        (
            "Kept.flac",
            ExportFormat::Flac,
            Some(stems(StemMode::ToMaster)),
        ),
        ("Long.ogg", ExportFormat::Ogg, None),
        ("Long.wv", ExportFormat::Wv, None),
        (
            "Long.mp3",
            ExportFormat::Mp3,
            Some(stems(StemMode::ToMaster)),
        ),
    ] {
        // Holds the export at its first progress event, with most of an
        // hour of audio still to render, until the test has cancelled it.
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
        let options = ExportOptions {
            pattern_loops: 1_000,
            stems,
            ..options(&rig, name, format)
        };
        rig.events.take();
        rig.session.export_audio(options.clone()).unwrap();
        wait_reached.recv().unwrap();
        // The files are being written under other names.
        assert!(written(&rig).iter().any(|name| name.ends_with(".tmp")));
        rig.session.export_cancel();
        release.send(()).unwrap();

        let done = rig.events.wait_for_export();
        assert_eq!(done.path, options.path, "{name}");
        assert_eq!((done.error, done.cancelled), (None, Some(true)), "{name}");
        assert_eq!(done.files, None, "{name}");
        assert!(done.fraction < 0.5, "{name}");
        // Nothing new is there, not even the folder made for the stems,
        // and what was there is as it was.
        assert_eq!(
            written(&rig),
            ["Kept/Kept - Mix.flac", "Long.flac"],
            "{name}"
        );
        assert_eq!(
            fs::read(rig.file("Long.flac")).unwrap(),
            b"an earlier export"
        );
        assert_eq!(
            fs::read(rig.file("Kept/Kept - Mix.flac")).unwrap(),
            b"an earlier mix"
        );
    }

    // The next export is not cancelled by the last one's request, nor by
    // one made while nothing was running.
    rig.session.export_cancel();
    let after = options(&rig, "after.flac", ExportFormat::Flac);
    let done = export(&rig, &after);
    assert_eq!((done.error, done.cancelled), (None, None));
    assert!(decode(&after.path).frames() == BAR);
}

#[cfg(windows)]
#[test]
fn native_m4a_export_uses_song_render_tags_exact_duration_and_atomic_cancellation() {
    let rig = Rig::new();
    beat(&rig);
    let track = rig
        .session
        .dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0];
    let bar = rig.project().patterns[0].length_ticks();
    rig.session
        .dispatch(
            Command::AddClips {
                clips: vec![windfall_project::ClipInit {
                    track: windfall_project::PlaylistTrackId(track),
                    start: 0,
                    length: Some(bar),
                    offset: None,
                    muted: None,
                    content: windfall_project::ClipContent::Pattern {
                        pattern: rig.pattern(),
                    },
                }],
            },
            None,
        )
        .unwrap();
    let request = ExportOptions {
        mode: PlayMode::Song,
        ..options(&rig, "Native.m4a", ExportFormat::M4a)
    };
    let done = export(&rig, &request);
    assert_eq!(done.error, None);
    let bytes = fs::read(&request.path).unwrap();
    let container = windfall_codec::inspect_m4a(&bytes).unwrap();
    assert_eq!(container.frames(), BAR as u64);
    assert_eq!(container.config.sample_rate, SAMPLE_RATE);
    let decoded = windfall_codec::decode_m4a_bytes(&bytes, &Default::default()).unwrap();
    assert_eq!(decoded.frames(), BAR);
    assert!(rms(decoded.samples()) > 0.01);
    let old = bytes;
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
    rig.events.take();
    rig.session
        .export_audio(ExportOptions {
            mode: PlayMode::Pattern,
            pattern_loops: 1000,
            ..request
        })
        .unwrap();
    wait_reached.recv().unwrap();
    rig.session.export_cancel();
    release.send(()).unwrap();
    let done = rig.events.wait_for_export();
    assert_eq!(done.cancelled, Some(true));
    assert_eq!(done.error, None);
    assert_eq!(fs::read(rig.file("Native.m4a")).unwrap(), old);
    assert_eq!(written(&rig), ["Native.m4a"]);
    let bad = options(&rig, "Bad.m4a", ExportFormat::M4a);
    assert!(
        rig.session
            .export_audio(ExportOptions {
                aac_bitrate: Some(320),
                ..bad
            })
            .is_err()
    );
    assert!(!Path::new(&rig.file("Bad.m4a")).exists());
}

#[test]
fn stems_that_cannot_all_be_written_are_not_written_at_all() {
    let rig = Rig::new();
    beat(&rig);
    // A folder has the name the clap's stem is to get.
    fs::create_dir(rig.file("Out")).unwrap();
    fs::create_dir(rig.file("Out/Out - 02 Clap Wide.wav")).unwrap();
    fs::write(rig.file("Out/Out - Mix.wav"), b"the previous mix").unwrap();
    let options = ExportOptions {
        stems: Some(stems(StemMode::TrackOutputs)),
        ..options(&rig, "Out.wav", ExportFormat::Wav)
    };
    let done = export(&rig, &options);
    let error = windfall_core::english::english(&done.error.unwrap());
    assert!(error.starts_with("Could not export to \""), "{error}");
    assert!(error.contains("\"Out - 02 Clap Wide.wav\": "), "{error}");
    assert_eq!((done.files, done.cancelled), (None, None));
    // The kick and the mix were on their way and are gone again. The
    // folder was not this export's to remove.
    assert_eq!(
        written(&rig),
        ["Out/Out - 02 Clap Wide.wav", "Out/Out - Mix.wav"]
    );
    assert_eq!(
        fs::read(rig.file("Out/Out - Mix.wav")).unwrap(),
        b"the previous mix"
    );
    assert!(Path::new(&rig.file("Out/Out - 02 Clap Wide.wav")).is_dir());

    // A file where the stems' folder is to be is in the way too.
    fs::write(rig.file("Taken"), b"not a folder").unwrap();
    let options = ExportOptions {
        path: rig.file("Taken.wav"),
        ..options
    };
    let error = windfall_core::english::english(&export(&rig, &options).error.unwrap());
    assert!(error.contains("the folder \""), "{error}");
    assert!(error.contains("could not be made"), "{error}");
    assert_eq!(fs::read(rig.file("Taken")).unwrap(), b"not a folder");
}

#[test]
fn what_a_format_or_a_stem_cannot_be_is_refused_before_anything_is_written() {
    let rig = Rig::new();
    beat(&rig);
    let refused = |options: ExportOptions| {
        rig.events.take();
        let error = rig.session.export_audio(options).unwrap_err();
        assert!(rig.events.take().is_empty());
        windfall_core::english::english(&error)
    };
    let flac = || options(&rig, "no.flac", ExportFormat::Flac);

    assert_eq!(
        refused(ExportOptions {
            bit_depth: BitDepth::Float32,
            ..flac()
        }),
        "A FLAC file holds 16-bit or 24-bit audio. Choose one of the two, or export a WAV file to keep 32-bit float."
    );
    assert_eq!(
        refused(ExportOptions {
            flac_level: Some(9),
            ..flac()
        }),
        "FLAC compression levels go from 0 to 8, and 9 was asked for."
    );
    assert_eq!(
        refused(options(&rig, "no.wav", ExportFormat::Flac)),
        "The export is a FLAC file, so its name cannot end in .wav. End it in .flac, leave the ending off, or choose another format."
    );
    assert_eq!(
        refused(ExportOptions {
            sample_rate: 500_000,
            ..flac()
        }),
        "500000 Hz is not a sample rate Windfall can export."
    );
    // A level is no concern of a WAV file, and is not looked at.
    let wav = ExportOptions {
        flac_level: Some(200),
        ..options(&rig, "fine.wav", ExportFormat::Wav)
    };
    assert_eq!(export(&rig, &wav).error, None);

    let with_tracks = |tracks: Vec<TrackId>, include_mix| ExportOptions {
        stems: Some(ExportStems {
            tracks: Some(tracks),
            include_mix,
            ..stems(StemMode::TrackOutputs)
        }),
        ..flac()
    };
    assert_eq!(
        refused(with_tracks(vec![TrackId::MASTER], true)),
        "The master track cannot be a stem. Its sound is the mix, which can be exported with the stems."
    );
    assert_eq!(
        refused(with_tracks(vec![TrackId(9_999)], true)),
        "The project has no mixer track with the id 9999."
    );
    assert_eq!(
        refused(with_tracks(Vec::new(), false)),
        "Choose at least one mixer track to export."
    );
    // A hundred thousand bars are more than a day of audio.
    let error = refused(ExportOptions {
        pattern_loops: 100_000,
        ..flac()
    });
    assert_eq!(
        error,
        "The export would be 3333 minutes long. The longest FLAC file Windfall can write at 48000 Hz is 1440 minutes."
    );
    assert_eq!(written(&rig), ["fine.wav"]);
}

#[test]
fn a_track_nothing_plays_into_is_no_stem() {
    let rig = Rig::new();
    beat(&rig);
    // A track nothing plays into is no stem, and a channel that plays
    // straight into the master is in the mix alone.
    rig.session
        .dispatch(Command::AddMixerTrack { name: None }, None)
        .unwrap();
    let options = ExportOptions {
        stems: Some(ExportStems {
            include_mix: false,
            folder: false,
            ..stems(StemMode::TrackOutputs)
        }),
        ..options(&rig, "Kit", ExportFormat::Flac)
    };
    let done = export(&rig, &options);
    assert_eq!(done.error, None);
    assert_eq!(
        written(&rig),
        [
            "Kit - 01 Kick Punch.flac",
            "Kit - 02 Clap Wide.flac",
            "Kit - 03 Hat Closed 1.flac",
            "Kit - 04 Snare Tight.flac",
        ]
    );
    let files = done.files.unwrap();
    assert!(files.iter().all(|file| file.track.is_some()));
}
