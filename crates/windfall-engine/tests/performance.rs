use windfall_engine::{
    Processor, SamplePool,
    performance::{CapturedLaunch, LaunchRequest, render_capture},
};
use windfall_project::*;

fn project() -> Project {
    let mut doc = Document::new(Project::new("Performance"));
    let channel = ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(InstrumentKind::SubtractiveSynth),
                index: None,
                mixer_track: None,
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    doc.dispatch(
        Command::AddNotes {
            pattern: doc.project().patterns[0].id,
            channel,
            notes: vec![NoteInit {
                start: 0,
                length: 960,
                key: 60,
                velocity: Some(0.8),
                pan: None,
                expression: None,
            }],
        },
        None,
    )
    .unwrap();
    doc.project().clone()
}
fn launch(pattern: PatternId, q: LaunchQuantization) -> LaunchRequest {
    LaunchRequest {
        track: 0,
        pattern: Some(pattern),
        quantization: q,
    }
}

#[test]
fn offline_capture_is_bit_identical_across_partitions_and_does_not_play_song() {
    let project = project();
    let log = [
        CapturedLaunch {
            frame: 0,
            request: launch(project.patterns[0].id, LaunchQuantization::Immediate),
        },
        CapturedLaunch {
            frame: 10000,
            request: LaunchRequest {
                track: 0,
                pattern: None,
                quantization: LaunchQuantization::Immediate,
            },
        },
    ];
    let a = render_capture(&project, &SamplePool::new(), 48000, 30000, 7, &log).unwrap();
    let b = render_capture(&project, &SamplePool::new(), 48000, 30000, 480, &log).unwrap();
    assert_eq!(a.samples(), b.samples());
    assert!(a.samples()[..18000].iter().any(|x| x.abs() > 0.001));
    assert!(a.samples()[40000..].iter().all(|x| x.abs() < 0.0001));
}

#[test]
fn retrigger_stop_and_tempo_plan_exchange_release_every_performance_note() {
    let mut project = project();
    let (_, controller) = Processor::new(48000);
    controller.set_project(&project, &SamplePool::new());
    let mut processor = Processor::reopen(&controller, 48000);
    controller
        .performance_launch(launch(
            project.patterns[0].id,
            LaunchQuantization::Immediate,
        ))
        .unwrap();
    processor.process(&mut vec![0.0; 12000]);
    assert!(!controller.transport().playing);
    assert_eq!(
        controller.performance_state().tracks[0].active,
        Some(project.patterns[0].id)
    );
    controller
        .performance_launch(launch(
            project.patterns[0].id,
            LaunchQuantization::Immediate,
        ))
        .unwrap();
    project.settings.tempo_bpm = 73.0;
    project.settings.time_signature = TimeSignature {
        numerator: 7,
        denominator: 8,
    };
    controller.set_project(&project, &SamplePool::new());
    processor.process(&mut vec![0.0; 128]);
    controller.performance_stop_all();
    let mut tail = vec![0.0; 96000];
    processor.process(&mut tail);
    assert!(
        controller
            .performance_state()
            .tracks
            .iter()
            .all(|s| s.active.is_none())
    );
    assert!(tail[90000..].iter().all(|x| x.abs() < 0.0001));
    assert_eq!(controller.frame().voices, 0);
}

#[test]
fn capture_rejects_unsorted_events_missing_targets_and_unbounded_duration() {
    let project = project();
    let request = launch(PatternId(u32::MAX), LaunchQuantization::Bar);
    assert!(
        render_capture(
            &project,
            &SamplePool::new(),
            48000,
            500,
            64,
            &[CapturedLaunch { frame: 0, request }]
        )
        .is_err()
    );
    assert!(render_capture(&project, &SamplePool::new(), 48000, 48000 * 61, 64, &[]).is_err());
    let request = launch(project.patterns[0].id, LaunchQuantization::Immediate);
    assert!(
        render_capture(
            &project,
            &SamplePool::new(),
            48000,
            500,
            64,
            &[
                CapturedLaunch { frame: 2, request },
                CapturedLaunch { frame: 1, request },
            ]
        )
        .is_err()
    );
    assert!(
        render_capture(
            &project,
            &SamplePool::new(),
            48000,
            500,
            64,
            &[CapturedLaunch {
                frame: 500,
                request
            },]
        )
        .is_err()
    );
}

#[test]
fn delayed_launch_and_same_frame_stop_retrigger_capture_are_partition_independent() {
    let project = project();
    let request = launch(project.patterns[0].id, LaunchQuantization::Immediate);
    let stop = LaunchRequest {
        pattern: None,
        ..request
    };
    let log = [
        CapturedLaunch {
            frame: 1531,
            request,
        },
        CapturedLaunch {
            frame: 11003,
            request: stop,
        },
        CapturedLaunch {
            frame: 11003,
            request,
        },
        CapturedLaunch {
            frame: 13000,
            request: stop,
        },
    ];
    let a = render_capture(&project, &SamplePool::new(), 48000, 30000, 127, &log).unwrap();
    let b = render_capture(&project, &SamplePool::new(), 48000, 30000, 4096, &log).unwrap();
    assert_eq!(a.samples(), b.samples());
    assert!(a.samples()[..1531 * 2].iter().all(|sample| *sample == 0.0));
    assert!(
        a.samples()[4000..20000]
            .iter()
            .any(|sample| sample.abs() > 0.001)
    );
    assert!(
        a.samples()[50000..]
            .iter()
            .all(|sample| sample.abs() < 0.0001)
    );
}
