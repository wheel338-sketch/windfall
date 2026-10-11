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
}
