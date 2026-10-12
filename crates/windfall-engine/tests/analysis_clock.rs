//! Independent analytic tempo truth; written, not executed.
use windfall_engine::SongClock;
use windfall_project::*;

#[test]
fn audio_seconds_follow_tempo_ramp_and_meter_labels_do_not_change_clock() {
    let mut doc = Document::new(Project::new("Clock"));
    let automation = AutomationId(
        doc.dispatch(
            Command::AddAutomation {
                name: None,
                target: AutomationTarget::Tempo,
                points: Some(vec![
                    AutomationPoint {
                        tick: 0,
                        value: (60.0 - 10.0) / 512.0,
                        curve: 0.0,
                        hold: false,
                    },
                    AutomationPoint {
                        tick: 1920,
                        value: (120.0 - 10.0) / 512.0,
                        curve: 0.0,
                        hold: false,
                    },
                ]),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let track = PlaylistTrackId(
        doc.dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    doc.dispatch(
        Command::AddClips {
            clips: vec![ClipInit {
                track,
                start: 0,
                length: Some(1920),
                offset: Some(0),
                muted: Some(false),
                content: ClipContent::Automation { automation },
            }],
        },
        None,
    )
    .unwrap();
    let clock = SongClock::new(doc.project()).unwrap();
    // Integral of 60/(PPQ*(60 + 60*t/1920)) from 0..1920.
    assert!((clock.seconds_at(1920.0).unwrap() - 2.0 * 2.0_f64.ln()).abs() < 1e-9);
    assert_eq!(clock.tick_after(0, 2.0 * 1.5_f64.ln()).unwrap(), 960);
    assert_eq!(clock.tick_after(1920, 0.5).unwrap(), 2880);
    doc.dispatch(
        Command::AddMeterChange {
            tick: 960,
            signature: TimeSignature {
                numerator: 7,
                denominator: 8,
            },
        },
        None,
    )
    .unwrap();
    let with_meter = SongClock::new(doc.project()).unwrap();
    assert_eq!(with_meter.tick_after(1920, 0.5).unwrap(), 2880);
    assert!(clock.tick_after(MAX_SONG_TICKS, 1.0).is_err());
    assert!(clock.tick_after(0, f64::NAN).is_err());
    let mut pool = windfall_engine::SamplePool::new();
    pool.insert(
        SampleId(900),
        windfall_core::AudioBuffer::from_interleaved(16000, 1, vec![0.2; 32000]),
    );
    let clip = Clip {
        id: ClipId(901),
        track,
        start: 0,
        length: 1920,
        offset: 120,
        muted: false,
        content: ClipContent::Audio {
            sample: SampleId(900),
            mixer_track: TrackId::MASTER,
            output: Default::default(),
            normalize: false,
            gain: 1.0,
            pan: 0.0,
            fade_in: 960,
            fade_out: 960,
            reverse: false,
            pitch: 0.0,
            stretch: Default::default(),
        },
    };
    let timing = windfall_engine::audio_edit::view_timing(
        &pool,
        &clip,
        doc.project().settings.tempo_bpm,
        &clock,
    )
    .unwrap();
    assert!((timing.duration_frames - 32000.0 * 2.0_f64.ln()).abs() < 1e-6);
    assert!((timing.fade_in_frames - 32000.0 * 1.5_f64.ln()).abs() < 1e-6);
    assert!((timing.fade_out_frames - 32000.0 * (4.0_f64 / 3.0).ln()).abs() < 1e-6);
    // Offset follows stored 120 BPM: 120 ticks = 1000 source frames.
    assert!((timing.offset_frames - 1000.0).abs() < 1e-9);
    let view = windfall_engine::audio_edit::render_view_timed(&pool, &clip, &timing).unwrap();
    assert_eq!(view.frames(), timing.duration_frames.ceil() as usize);
    for frame in [4000, 12000, 18000] {
        let fi = (frame as f64 / timing.fade_in_frames).clamp(0.0, 1.0);
        let fo = ((timing.duration_frames - frame as f64) / timing.fade_out_frames).clamp(0.0, 1.0);
        let expected = 0.2
            * (fi * std::f64::consts::FRAC_PI_2).sin()
            * (fo * std::f64::consts::FRAC_PI_2).sin();
        assert!((f64::from(view.samples()[frame * 2]) - expected).abs() < 1e-6);
    }
}
