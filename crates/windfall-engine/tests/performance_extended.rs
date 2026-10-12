use windfall_core::AudioBuffer;
use windfall_engine::{
    SamplePool,
    performance::{CapturedLaunch, LaunchRequest, render_capture},
};
use windfall_project::*;

fn synth() -> (Project, ChannelId) {
    let mut doc = Document::new(Project::new("performance tools"));
    let channel = ChannelId(
        doc.dispatch(
            Command::AddChannel {
                name: None,
                sample: None,
                instrument: Some(InstrumentKind::SubtractiveSynth),
                index: None,
                mixer_track: Some(TrackId::MASTER),
            },
            None,
        )
        .unwrap()
        .created[0],
    );
    let pattern = doc.project().patterns[0].id;
    doc.dispatch(
        Command::AddNotes {
            pattern,
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
    (doc.project().clone(), channel)
}
fn launch(project: &Project, id: PatternId, track: u8) -> CapturedLaunch {
    let _ = project;
    CapturedLaunch {
        frame: 0,
        request: LaunchRequest {
            track,
            pattern: Some(id),
            quantization: LaunchQuantization::Immediate,
        },
    }
}
fn energy(samples: &[f32]) -> f64 {
    samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum()
}

#[test]
fn performance_layer_fans_out_and_arp_echo_change_measured_audio() {
    let (mut project, channel) = synth();
    let id = project.patterns[0].id;
    let pool = SamplePool::new();
    let original = render_capture(
        &project,
        &pool,
        48000,
        36000,
        64,
        &[launch(&project, id, 0)],
    )
    .unwrap();
    let child = ChannelId(project.next_id);
    project.next_id += 1;
    let mut copy = project.channels[0].clone();
    copy.id = child;
    copy.name = "Layer child".into();
    project.channels.push(copy);
    project.channels[0].voice.layer = LayerSettings {
        enabled: true,
        children: LayerChildren::new([child]),
    };
    let layered = render_capture(
        &project,
        &pool,
        48000,
        36000,
        64,
        &[launch(&project, id, 0)],
    )
    .unwrap();
    assert_eq!(original.samples(), layered.samples());
    let source = project
        .channels
        .iter_mut()
        .find(|held| held.id == child)
        .unwrap();
    source.voice.arpeggiator.mode = ArpeggiatorMode::Up;
    source.voice.arpeggiator.range_octaves = 2;
    source.voice.echo = NoteEchoSettings {
        enabled: true,
        time: EchoTime::Milliseconds { ms: 50.0 },
        feedback: 0.5,
        pitch_semitones: 12,
        repeats: 2,
    };
    let tools = render_capture(
        &project,
        &pool,
        48000,
        36000,
        64,
        &[launch(&project, id, 0)],
    )
    .unwrap();
    let single =
        render_capture(&project, &pool, 48000, 36000, 1, &[launch(&project, id, 0)]).unwrap();
    assert_eq!(tools.samples(), single.samples());
    assert!(energy(tools.samples()) > 0.01);
    assert!(
        tools
            .samples()
            .iter()
            .zip(original.samples())
            .any(|(a, b)| (a - b).abs() > 0.001)
    );
    assert_eq!(project.patterns[0].lanes[0].channel, channel);
}

#[test]
fn performance_note_pan_curve_moves_sound_while_song_transport_is_stopped() {
    let (mut project, _) = synth();
    let note = project.patterns[0].lanes[0].notes[0].id;
    project.patterns[0].note_curves.push(NoteExpressionCurve {
        note,
        parameter: NoteCurveParameter::Pan,
        points: vec![
            NoteCurvePoint {
                position: 0.0,
                value: -1.0,
                curve: 0.0,
                hold: false,
            },
            NoteCurvePoint {
                position: 1.0,
                value: 1.0,
                curve: 0.0,
                hold: false,
            },
        ],
    });
    let id = project.patterns[0].id;
    let pcm = render_capture(
        &project,
        &SamplePool::new(),
        48000,
        24000,
        97,
        &[launch(&project, id, 0)],
    )
    .unwrap();
    let sides = |start: usize, end: usize| -> [f64; 2] {
        std::array::from_fn(|side| {
            pcm.samples()[start * 2..end * 2]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|frame| f64::from(frame[side]).powi(2))
                .sum()
        })
    };
    let early = sides(500, 2000);
    let late = sides(20000, 22000);
    assert!(early[0] > early[1] * 9.0);
    assert!(late[1] > late[0] * 9.0);
}

#[test]
fn audio_and_automation_pads_loop_route_and_restore_without_song_playback() {
    let mut project = Project::new("clip pads");
    let sample = SampleId(project.next_id);
    project.next_id += 1;
    project.samples.push(SampleAsset {
        id: sample,
        name: "constant".into(),
        path: SamplePath::External("own.wav".into()),
        processing: None,
        regions: Vec::new(),
    });
    let clip = ClipId(project.next_id);
    project.next_id += 1;
    let automation = AutomationId(project.next_id);
    project.next_id += 1;
    let auto_clip = ClipId(project.next_id);
    project.next_id += 1;
    project.automations.push(Automation {
        id: automation,
        name: "Master gain".into(),
        color: 0,
        target: AutomationTarget::TrackVolume {
            track: TrackId::MASTER,
        },
        points: vec![AutomationPoint {
            tick: 0,
            value: 0.5,
            curve: 0.0,
            hold: true,
        }],
    });
    let track = project.playlist.tracks[0].id;
    project.playlist.clips.push(Clip {
        id: clip,
        track,
        start: 96000,
        length: 960,
        offset: 0,
        muted: true,
        content: ClipContent::Audio {
            sample,
            mixer_track: TrackId::MASTER,
            output: ClipAudioOutput::Mixer,
            normalize: false,
            gain: 1.0,
            pan: 0.0,
            fade_in: 0,
            fade_out: 0,
            reverse: false,
            pitch: 0.0,
            stretch: ClipStretch::Tape,
        },
    });
    project.playlist.clips.push(Clip {
        id: auto_clip,
        track,
        start: 96000,
        length: 960,
        offset: 0,
        muted: true,
        content: ClipContent::Automation { automation },
    });
    let pattern = project.patterns[0].id;
    project.settings.stage_media.pads = vec![
        PerformancePad {
            track: 0,
            scene: 0,
            pattern,
            clip: Some(clip),
        },
        PerformancePad {
            track: 1,
            scene: 0,
            pattern,
            clip: Some(auto_clip),
        },
    ];
    let mut pool = SamplePool::new();
    pool.insert(
        sample,
        AudioBuffer::from_interleaved(48000, 2, vec![0.25; 48000]),
    );
    let mut log = vec![
        launch(&project, PatternId(clip.0), 0),
        launch(&project, PatternId(auto_clip.0), 1),
    ];
    log.push(CapturedLaunch {
        frame: 12000,
        request: LaunchRequest {
            track: 1,
            pattern: None,
            quantization: LaunchQuantization::Immediate,
        },
    });
    let pcm = render_capture(&project, &pool, 48000, 30000, 73, &log).unwrap();
    let other = render_capture(&project, &pool, 48000, 30000, 1, &log).unwrap();
    assert_eq!(pcm.samples(), other.samples());
    // Fader automation units use gain = 2 * unit^2: .5 -> .5 gain.
    assert!(
        pcm.samples()[1000..20000]
            .iter()
            .all(|sample| (*sample - 0.125).abs() < 1e-6)
    );
    assert!(
        pcm.samples()[26000..40000]
            .iter()
            .all(|sample| (*sample - 0.25).abs() < 1e-6)
    );
    assert!((pcm.samples()[50000] - 0.25).abs() < 1e-6); // second pad cycle
}
