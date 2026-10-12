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
        EncoderSettings::WavPack { depth: windfall_codec::WavPackDepth::Int24 },
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
        (44100, 1, 4410)
    );
    assert!(
        info.source_warnings
            .iter()
            .any(|w| w.contains("approximately"))
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
    let imported = rig.project();
    assert!(rig.has_audio(sample));
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
    rig.session.project_new().unwrap();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(rig.project(), imported);
    assert!(rig.has_audio(sample));
    assert_eq!(fs::read(&source).unwrap(), DRUMSYNTH);
    let pcm = windfall_codec::decode_file(&source).unwrap();
    for frame in [0, 1, 100, 2205, 4409] {
        let expected = 0.5
            * (1.0 - frame as f64 / 4410.0)
            * (std::f64::consts::TAU * 440.0 * (frame + 1) as f64 / 44100.0).sin();
        assert!((f64::from(pcm.samples()[frame]) - expected).abs() < 1e-6);
    }
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
        b"[General]\nVersion=DrumSynth v2.0\n[Overtones]\nOn=1\n".as_slice(),
        b"[General]\nVersion=DrumSynth v2.0\n[Tone]\nOn=1\nF1=NaN\n".as_slice(),
    ] {
        let file = rig.file("unsupported.ds");
        fs::write(&file, bytes).unwrap();
        let before = rig.session.document_snapshot();
        assert!(rig.session.add_channel_from_file(&file, None).is_err());
        assert_eq!(rig.session.document_snapshot(), before);
    }
}
