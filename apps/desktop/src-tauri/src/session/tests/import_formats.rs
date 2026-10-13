//! Compressed files through the same browser/sample/playlist flow as WAV.

use std::fs;

use windfall_codec::{Encoder, EncoderSettings, FlacBitDepth, Mp3Channels, Mp3Settings};
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::{ChannelId, ClipId, SampleId};

use super::{Rig, SAMPLE_RATE, rms};
use crate::session::ClipPlace;

const FRAMES: usize = SAMPLE_RATE as usize / 2;

fn formats() -> [EncoderSettings; 4] {
    [
        EncoderSettings::Flac {
            depth: FlacBitDepth::Int24,
            level: 5,
        },
        EncoderSettings::Vorbis { quality: 6.0 },
        EncoderSettings::Mp3 {
            settings: Mp3Settings {
                channels: Mp3Channels::Mono,
                ..Mp3Settings::default()
            },
        },
        EncoderSettings::WavPack {
            depth: windfall_codec::WavPackDepth::Int24,
        },
    ]
}

fn tone_file(rig: &Rig, settings: &EncoderSettings) -> String {
    let file = rig.file(&format!("Tone.{}", settings.format().extension()));
    let samples: Vec<_> = (0..FRAMES)
        .map(|frame| {
            (std::f32::consts::TAU * 440.0 * frame as f32 / SAMPLE_RATE as f32).sin() * 0.25
        })
        .collect();
    let mut encoder = Encoder::open(&file, settings, SAMPLE_RATE, 1).unwrap();
    encoder.write(&samples).unwrap();
    encoder.finalize().unwrap();
    file
}

fn place() -> ClipPlace {
    ClipPlace {
        track: None,
        start: 960,
        mixer_track: None,
    }
}

#[test]
fn compressed_samples_preview_play_and_survive_save_reopen() {
    for settings in formats() {
        let mut rig = Rig::new();
        let file = tone_file(&rig, &settings);
        let info = rig.session.sample_info(&file).unwrap();
        assert_eq!(info.name, "Tone");
        assert_eq!((info.sample_rate, info.channels), (SAMPLE_RATE, 1));
        assert_eq!(info.frames, FRAMES as u64);
        assert!((info.duration_secs - 0.5).abs() < 1e-9);
        assert!(info.peaks.iter().any(|peak| *peak > 0.1));

        let before = rig.session.document_snapshot().history.entries.len();
        let added = rig.session.add_channel_from_file(&file, None).unwrap();
        let [sample, channel, _track] = added.created[..] else {
            panic!("expected sample, channel and mixer track");
        };
        assert!(rig.has_audio(SampleId(sample)));
        assert_eq!(
            rig.session.document_snapshot().history.entries.len(),
            before + 1
        );
        rig.session.audition_note_on(ChannelId(channel), 60, 1.0);
        assert!(rms(&rig.run(4_800)) > 0.05, "{settings:?}");
        rig.session.audition_note_off(ChannelId(channel), 60);

        let saved = rig
            .session
            .project_save(Some(&rig.file("Compressed.windfall")))
            .unwrap();
        let project = rig.project();
        rig.session.project_new().unwrap();
        rig.session.project_open(&saved).unwrap();
        assert_eq!(rig.project(), project);
        assert!(rig.has_audio(SampleId(sample)));
        rig.session.audition_note_on(ChannelId(channel), 60, 1.0);
        assert!(rms(&rig.run(4_800)) > 0.05, "reopened {settings:?}");
    }
}

#[test]
fn compressed_playlist_clips_have_gapless_duration_and_one_undo_step() {
    for settings in formats() {
        let mut rig = Rig::new();
        let file = tone_file(&rig, &settings);
        let before = rig.project();
        let added = rig
            .session
            .add_audio_clip_from_file(&file, place())
            .unwrap();
        let clip_id = ClipId(*added.created.last().unwrap());
        let imported = rig.project();
        let clip = imported
            .playlist
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .unwrap();
        assert_eq!((clip.start, clip.length), (960, 960), "{settings:?}");
        assert_eq!(rig.session.document_snapshot().history.entries.len(), 1);

        rig.session
            .transport_set(TransportPatch {
                mode: Some(PlayMode::Song),
                loop_song: Some(false),
                ..TransportPatch::default()
            })
            .unwrap();
        rig.session.transport_play().unwrap();
        let out = rig.run(SAMPLE_RATE as usize);
        assert!(out[..FRAMES * 2].iter().all(|sample| *sample == 0.0));
        assert!(rms(&out[FRAMES * 2..]) > 0.05, "{settings:?}");
        rig.session.transport_stop();

        rig.session.undo().unwrap();
        let mut undone = rig.project();
        // Allocated IDs stay reserved after undo.
        undone.next_id = before.next_id;
        assert_eq!(undone, before);
        rig.session.redo().unwrap();
        assert_eq!(rig.project(), imported);
    }
}

#[test]
fn broken_compressed_files_do_not_change_the_project_or_publish_edits() {
    for settings in formats() {
        let rig = Rig::new();
        let file = tone_file(&rig, &settings);
        let bytes = fs::read(&file).unwrap();
        fs::write(&file, &bytes[..8]).unwrap();
        let before = rig.session.document_snapshot();
        rig.events.take();
        assert!(rig.session.add_channel_from_file(&file, None).is_err());
        assert!(
            rig.session
                .add_audio_clip_from_file(&file, place())
                .is_err()
        );
        assert_eq!(rig.session.document_snapshot(), before);
        assert!(rig.events.take().is_empty());
    }
}

const DRUMSYNTH: &[u8] = b"[General]\nVersion=DrumSynth v2.0\n[Tone]\nOn=1\nLevel=128\nF1=440\nF2=440\nPhase=0\nEnvelope=0,100 4410,0\n[Own Extension]\nText=retain original text\n";

#[test]
fn drumsynth_native_sampler_import_reports_limits_and_reopens_original_preset_without_rewriting_it()
{
    let rig = Rig::new();
    let source = rig.file("original-preset.ds");
    fs::write(&source, DRUMSYNTH).unwrap();
    let info = rig.session.sample_info(&source).unwrap();
    assert_eq!(
        (info.sample_rate, info.channels, info.frames),
        (44100, 1, 6000)
    );
    assert!(
        info.source_warnings
            .iter()
            .any(|w| w.contains("reference model"))
    );
    assert!(
        info.source_warnings
            .iter()
            .any(|w| w.contains("own extension.text"))
    );
    let before = rig.session.document_snapshot();
    let result = rig.session.add_channel_from_file(&source, None).unwrap();
    let sample = SampleId(result.created[0]);
    let channel = ChannelId(result.created[1]);
    let source_sample = SampleId(*result.created.last().unwrap());
    let imported = rig.project();
    assert!(rig.has_audio(sample));
    assert_eq!(
        rig.session.sample_info_by_id(sample).unwrap().sample_rate,
        SAMPLE_RATE
    );
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    rig.session.undo().unwrap();
    assert!(!rig.project().channels.iter().any(|c| c.id == channel));
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), imported);
    let saved = rig
        .session
        .project_save(Some(&rig.file("preset-source.windfall")))
        .unwrap();
    let portable = rig.project();
    let windfall_project::SamplePath::Project(relative) =
        &portable.sample(source_sample).unwrap().path
    else {
        panic!("original preset must be owned by the saved project");
    };
    assert_eq!(
        fs::read(rig.folder.path().join(relative)).unwrap(),
        DRUMSYNTH
    );
    rig.session.project_new().unwrap();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(rig.project(), portable);
    assert!(rig.has_audio(sample));
    assert_eq!(fs::read(&source).unwrap(), DRUMSYNTH);
    let pcm = windfall_codec::decode_file(&source).unwrap();
    for frame in [0, 1, 100, 2205, 4409] {
        let expected = 0.5
            * (1.0 - frame as f64 / 4410.0)
            * (std::f64::consts::TAU * 440.0 * (frame + 1) as f64 / 44100.0).sin();
        assert_eq!(
            pcm.samples()[frame],
            (expected * 32768.0).trunc() as f32 / 32768.0
        );
    }
}

#[test]
fn owned_preset_survives_original_deletion_first_save_move_history_and_fresh_reopen() {
    let rig = Rig::new();
    let source = rig.file("portable-original.ds");
    fs::write(&source, DRUMSYNTH).unwrap();
    let added = rig.session.add_channel_from_file(&source, None).unwrap();
    let sample = SampleId(added.created[0]);
    let source_sample = SampleId(*added.created.last().unwrap());
    let imported = rig.project();
    let windfall_project::SamplePath::External(owned) =
        &imported.sample(source_sample).unwrap().path
    else {
        panic!("unsaved owned source");
    };
    assert_ne!(owned, &source);
    assert_eq!(fs::read(owned).unwrap(), DRUMSYNTH);
    fs::remove_file(&source).unwrap();
    let saved = rig
        .session
        .project_save(Some(&rig.file("portable/preset.windfall")))
        .unwrap();
    let portable = rig.project();
    let windfall_project::SamplePath::Project(relative) =
        &portable.sample(source_sample).unwrap().path
    else {
        panic!("portable source");
    };
    let carried = std::path::Path::new(&saved)
        .parent()
        .unwrap()
        .join(relative);
    assert_eq!(fs::read(&carried).unwrap(), DRUMSYNTH);
    rig.session.undo().unwrap();
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), portable);
    let moved = rig
        .session
        .project_save(Some(&rig.file("moved/preset.windfall")))
        .unwrap();
    fs::remove_file(&carried).unwrap();
    fs::remove_file(owned).unwrap();
    rig.session.project_new().unwrap();
    rig.session.project_open(&moved).unwrap();
    assert!(rig.has_audio(sample));
    assert_eq!(rig.session.sample_info_by_id(sample).unwrap().frames, 7200);
    let project = rig.project();
    let windfall_project::SamplePath::Project(relative) =
        &project.sample(source_sample).unwrap().path
    else {
        panic!("moved owned source");
    };
    let pcm = windfall_codec::decode_file(
        std::path::Path::new(&moved)
            .parent()
            .unwrap()
            .join(relative),
    )
    .unwrap();
    let windfall_project::SamplePath::Project(wav_relative) = &project.sample(sample).unwrap().path
    else {
        panic!("rendered WAV must also be portable");
    };
    let wav = windfall_codec::decode_file(
        std::path::Path::new(&moved)
            .parent()
            .unwrap()
            .join(wav_relative),
    )
    .unwrap();
    let reference = windfall_codec::DrumSynthPreset::parse(DRUMSYNTH)
        .unwrap()
        .render(
            SAMPLE_RATE,
            windfall_codec::MAX_DRUMSYNTH_FRAMES,
            windfall_codec::DRUMSYNTH_SOURCE_SEED,
        )
        .unwrap();
    assert_eq!(wav.sample_rate(), SAMPLE_RATE);
    assert_eq!(wav.samples(), reference.audio.samples());
    for frame in [0, 100, 2205, 4409] {
        let expected = 0.5
            * (1.0 - frame as f64 / 4410.0)
            * (std::f64::consts::TAU * 440.0 * (frame + 1) as f64 / 44100.0).sin();
        assert_eq!(
            pcm.samples()[frame],
            (expected * 32768.0).trunc() as f32 / 32768.0
        );
    }
}

#[test]
fn drumsynth_browser_add_and_drop_on_sampler_own_both_assets_in_one_undo_step() {
    let rig = Rig::new();
    let original_samples = rig.project().samples.len();
    let source = rig.file("browser-drum.ds");
    fs::write(&source, DRUMSYNTH).unwrap();
    rig.session
        .browser_add_root(&rig.folder.path().to_string_lossy())
        .unwrap();
    let token = rig.session.library_file(&source).unwrap();
    let added = rig
        .session
        .browser_add_channel(&source, None, token)
        .unwrap();
    let channel = ChannelId(added.created[1]);
    assert_eq!(rig.project().samples.len(), original_samples + 2);
    assert_eq!(rig.session.document_snapshot().history.cursor, 1);
    let before = rig.project();
    let token = rig.session.library_file(&source).unwrap();
    let replaced = rig
        .session
        .browser_replace_sample(channel, &source, token)
        .unwrap();
    let sample = SampleId(replaced.created[0]);
    assert_eq!(
        rig.session.sample_info_by_id(sample).unwrap().sample_rate,
        SAMPLE_RATE
    );
    assert_eq!(rig.session.document_snapshot().history.cursor, 2);
    let after = rig.project();
    rig.session.undo().unwrap();
    let mut undone = rig.project();
    undone.next_id = before.next_id;
    assert_eq!(undone, before);
    rig.session.redo().unwrap();
    assert_eq!(rig.project(), after);
    fs::remove_file(source).unwrap();
    let saved = rig
        .session
        .project_save(Some(&rig.file("browser-owned.windfall")))
        .unwrap();
    rig.session.project_new().unwrap();
    rig.session.project_open(&saved).unwrap();
    assert!(rig.has_audio(sample));
}

#[test]
fn drumsynth_source_changes_replacement_and_unsupported_synthesis_never_install_stale_or_wrong_audio()
 {
    for point in ["import:decoded", "import:prepared"] {
        for replace in [false, true] {
            let rig = Rig::new();
            let source = rig.file("guarded-preset.ds");
            fs::write(&source, DRUMSYNTH).unwrap();
            let held = rig.session.hold(point);
            let file = source.clone();
            let work = rig
                .session
                .background(move |s| s.add_channel_from_file(&file, None));
            held.wait();
            if replace {
                rig.session.project_new().unwrap();
            } else {
                fs::write(&source, [DRUMSYNTH, b"[Distortion]\nOn=1\n"].concat()).unwrap();
            }
            let before = rig.session.document_snapshot();
            held.release();
            assert!(work.join().unwrap().is_err());
            assert_eq!(rig.session.document_snapshot(), before);
        }
    }
    let rig = Rig::new();
    for bytes in [
        b"[General]\nVersion=DrumSynth v2.0\n[Overtones]\nOn=1\nMethod=4\n".as_slice(),
        b"[General]\nVersion=DrumSynth v2.0\n[Tone]\nOn=1\nF1=NaN\n".as_slice(),
    ] {
        let file = rig.file("unsupported.ds");
        fs::write(&file, bytes).unwrap();
        let before = rig.session.document_snapshot();
        assert!(rig.session.add_channel_from_file(&file, None).is_err());
        assert_eq!(rig.session.document_snapshot(), before);
    }
}
