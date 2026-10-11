//! Calibrated acceptance fixtures. Written, not executed by implementation worker.
use std::time::Duration;
use windfall_analysis::{CancelToken, Work, classical};
use windfall_core::AudioBuffer;
use windfall_project::{
    AudioDenoiseSettings, AudioEditRecipe, AudioNoteOverride, AudioWarpMarker, Clip, ClipContent,
    ClipId, PlaylistTrackId, SampleId, TrackId,
};

fn work() -> Work {
    Work::new(
        CancelToken::default(),
        100_000_000_000,
        Duration::from_secs(120),
    )
    .unwrap()
}
fn signal(hz: &[f64], frames: usize) -> AudioBuffer {
    AudioBuffer::from_interleaved(
        16000,
        1,
        (0..frames)
            .map(|i| {
                hz.iter()
                    .map(|f| (std::f64::consts::TAU * *f * i as f64 / 16000.0).sin() * 0.2)
                    .sum::<f64>() as f32
            })
            .collect(),
    )
}
fn recipe(audio: &AudioBuffer) -> AudioEditRecipe {
    AudioEditRecipe {
        source_clip: Clip {
            id: ClipId(8),
            track: PlaylistTrackId(7),
            start: 0,
            length: 1920,
            offset: 0,
            muted: false,
            content: ClipContent::Audio {
                sample: SampleId(6),
                mixer_track: TrackId::MASTER,
                output: Default::default(),
                normalize: false,
                gain: 1.0,
                pan: 0.0,
                fade_in: 0,
                fade_out: 0,
                reverse: false,
                pitch: 0.0,
                stretch: Default::default(),
            },
        },
        analysis: classical::detect(audio, false, &mut work()).unwrap(),
        overrides: vec![],
        markers: vec![
            AudioWarpMarker {
                source_frame: 0,
                target_frame: 0,
            },
            AudioWarpMarker {
                source_frame: audio.frames() as u32,
                target_frame: audio.frames() as u32,
            },
        ],
        denoise: None,
    }
}
#[test]
fn calibrated_note_pitch_duration_and_neutral_render() {
    let audio = signal(&[440.0], 16000);
    let mut r = recipe(&audio);
    assert_eq!(r.analysis.notes.len(), 1);
    assert!((r.analysis.notes[0].midi_pitch - 69.0).abs() < 0.05);
    let neutral = classical::render(&audio, &r, &mut work()).unwrap();
    assert_eq!(neutral.samples(), audio.samples());
    let n = &r.analysis.notes[0];
    r.overrides.push(AudioNoteOverride {
        note: 0,
        midi_pitch: n.midi_pitch + 12.0,
        start_frame: n.start_frame,
        end_frame: n.end_frame,
    });
    let output = classical::render(&audio, &r, &mut work()).unwrap();
    let estimates = classical::detect(&output, false, &mut work()).unwrap();
    let voiced: Vec<_> = estimates
        .notes
        .iter()
        .filter(|n| n.end_frame - n.start_frame > 4000)
        .collect();
    assert!(!voiced.is_empty());
    assert!((voiced[0].midi_pitch - 81.0).abs() < 0.15);
    r.overrides.clear();
    r.markers[1].target_frame = 24000;
    assert_eq!(
        classical::render(&audio, &r, &mut work()).unwrap().frames(),
        24000
    );
}
#[test]
fn repeated_and_held_notes_silence_and_polyphonic_chord() {
    let mut pcm = signal(&[440.0], 32000).samples().to_vec();
    pcm[14000..18000].fill(0.0);
    let audio = AudioBuffer::from_interleaved(16000, 1, pcm);
    let data = classical::detect(&audio, false, &mut work()).unwrap();
    assert_eq!(data.notes.len(), 2);
    assert!(data.notes[0].end_frame < 18000);
    assert!(data.notes[1].start_frame >= 17500);
    for note in &data.notes {
        assert!((note.midi_pitch - 69.0).abs() < 0.1);
        assert!(note.rms > 0.1);
    }
    let held = classical::detect(&signal(&[440.0], 32000), false, &mut work()).unwrap();
    assert_eq!(held.notes.len(), 1);
    let chord = classical::detect(
        &signal(&[261.625565, 329.627557, 391.995436], 32000),
        true,
        &mut work(),
    )
    .unwrap();
    for key in [60.0, 64.0, 67.0] {
        assert!(
            chord
                .notes
                .iter()
                .any(|n| (n.midi_pitch - key).abs() < 0.1 && n.end_frame - n.start_frame > 24000)
        );
    }
    let silent = AudioBuffer::from_interleaved(16000, 1, vec![0.0; 16000]);
    assert!(
        classical::detect(&silent, false, &mut work())
            .unwrap()
            .notes
            .is_empty()
    );
    assert!(
        classical::detect(&silent, true, &mut work())
            .unwrap()
            .notes
            .is_empty()
    );
}
#[test]
fn glide_vibrato_and_noise_keep_bounded_estimates() {
    for vibrato in [false, true] {
        let mut phase = 0.0_f64;
        let audio = AudioBuffer::from_interleaved(
            16000,
            1,
            (0..32000)
                .map(|i| {
                    let t = i as f64 / 16000.0;
                    let hz = if vibrato {
                        440.0 * 2.0_f64.powf(0.3 * (std::f64::consts::TAU * 5.0 * t).sin() / 12.0)
                    } else {
                        220.0 + 110.0 * t
                    };
                    phase += std::f64::consts::TAU * hz / 16000.0;
                    (phase.sin() * 0.2) as f32
                })
                .collect(),
        );
        let data = classical::detect(&audio, false, &mut work()).unwrap();
        assert!(!data.notes.is_empty());
        assert!(
            data.notes
                .iter()
                .all(|n| (55.0..71.0).contains(&n.midi_pitch))
        );
    }
    let mut seed = 777_u32;
    let noise = AudioBuffer::from_interleaved(
        16000,
        1,
        (0..16000)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed as f64 / u32::MAX as f64 * 2.0 - 1.0) as f32 * 0.1
            })
            .collect(),
    );
    assert!(
        classical::detect(&noise, false, &mut work())
            .unwrap()
            .notes
            .is_empty()
    );
}
#[test]
fn warp_order_bounds_overrides_and_cancel_are_explicit() {
    let audio = signal(&[440.0], 16000);
    let mut r = recipe(&audio);
    r.markers.insert(
        1,
        AudioWarpMarker {
            source_frame: 8000,
            target_frame: 17000,
        },
    );
    assert!(r.check().is_err());
    r.markers[1].target_frame = 10;
    assert!(r.check().is_err());
    let token = CancelToken::default();
    token.cancel();
    let mut cancelled = Work::new(token, 100_000, Duration::from_secs(10)).unwrap();
    assert!(matches!(
        classical::detect(&audio, false, &mut cancelled),
        Err(windfall_analysis::AnalysisError::Cancelled)
    ));
}
#[test]
fn learned_profile_improves_independent_snr_and_dry_is_exact() {
    let mut seed = 42_u32;
    let clean: Vec<f32> = (0..48000)
        .map(|i| {
            if i < 8000 {
                0.0
            } else {
                (0.25 * (std::f64::consts::TAU * 440.0 * i as f64 / 16000.0).sin()) as f32
            }
        })
        .collect();
    let noisy: Vec<f32> = clean
        .iter()
        .map(|x| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *x + ((seed as f64 / u32::MAX as f64) * 2.0 - 1.0) as f32 * 0.09
        })
        .collect();
    let audio = AudioBuffer::from_interleaved(16000, 1, noisy.clone());
    let settings = AudioDenoiseSettings {
        profile_start: 0,
        profile_end: 8000,
        reduction_db: 24.0,
        wet: 1.0,
    };
    let out = classical::denoise(&audio, settings, &mut work()).unwrap();
    let errors = |values: &[f32]| {
        (12000..44000)
            .map(|i| f64::from(values[i] - clean[i]).powi(2))
            .sum::<f64>()
    };
    let improvement = 10.0 * (errors(&noisy) / errors(out.samples())).log10();
    assert!(improvement > 5.0, "SNR improvement {improvement} dB");
    let rms = |values: &[f32]| {
        (values[12000..44000]
            .iter()
            .map(|v| f64::from(*v).powi(2))
            .sum::<f64>()
            / 32000.0)
            .sqrt()
    };
    assert!((20.0 * (rms(out.samples()) / rms(&clean)).log10()).abs() < 1.5);
    assert_eq!(
        classical::denoise(
            &audio,
            AudioDenoiseSettings {
                wet: 0.0,
                ..settings
            },
            &mut work()
        )
        .unwrap()
        .samples(),
        noisy
    );
    assert_eq!(audio.samples(), noisy);
}
