//! Independent manually edited score is the reference, not another VM run.
use crate::support::{Rig, sine};
use windfall_engine::{RenderOptions, render};
use windfall_ipc::PlayMode;
use windfall_project::scripting::{
    ChannelScript, ScriptAsset, ScriptKind, ScriptLibrary, ScriptReference,
};

#[test]
fn channel_script_matches_manual_score_live_offline_irregular_blocks_and_saved_reopen() {
    let mut rig = Rig::new();
    let channel = rig.channel(sine(48_000, 220.0, 0.15));
    rig.note(channel, 240, 240).key = 60;
    rig.note(channel, 720, 240).key = 64;
    let track = rig.playlist_track();
    rig.clip(track, rig.first_pattern(), 0, 1920);
    let mut reference = rig.project.clone();
    for lane in &mut reference.patterns[0].lanes {
        for note in &mut lane.notes {
            note.key += 12;
            note.velocity *= 0.5;
            note.pan = -0.25;
        }
    }
    let script = ScriptReference {
        id: "octave".into(),
        revision: 1,
    };
    rig.project.settings.scripts = ScriptLibrary {
        assets: vec![ScriptAsset {
            reference: script.clone(),
            language_version: 1,
            kind: ScriptKind::NoteTransform,
            name: "Octave".into(),
            source: "set key key 12 +\nset velocity velocity 0.5 *\nset pan -0.25\nemit".into(),
        }],
        channels: vec![ChannelScript {
            channel,
            script,
            parameters: vec![],
        }],
        controllers: vec![],
    };
    rig.project.check().unwrap();
    let options = RenderOptions {
        mode: PlayMode::Song,
        tail_secs: 0.05,
        block_frames: 7,
        ..Default::default()
    };
    let actual = render(&rig.project, &rig.pool, &options, &mut |_| true);
    let expected = render(&reference, &rig.pool, &options, &mut |_| true);
    assert!(actual.samples().iter().any(|s| s.abs() > 0.001));
    assert_eq!(actual.samples(), expected.samples());
    assert_eq!(
        actual.samples(),
        rig.play_song(48_000, actual.frames(), 251)
    );
    let (mut processor, controller) = rig.processor(48_000);
    controller.play();
    let mut output = [0.0; 502];
    for _ in 0..64 {
        assert_eq!(
            crate::realtime::allocator_calls(|| processor.process(&mut output)),
            0
        );
        controller.frame();
    }
    let json = windfall_project::file::to_json(&rig.project).unwrap();
    rig.project = windfall_project::file::from_json(&json).unwrap();
    let other = RenderOptions {
        block_frames: 1024,
        ..options
    };
    assert_eq!(
        actual.samples(),
        render(&rig.project, &rig.pool, &other, &mut |_| true).samples()
    );
}

#[test]
fn scripted_short_long_and_duplicated_durations_match_manual_score_live_and_offline() {
    for rack in [false, true] {
    for (scale, copies, window, effective_length) in [(0.5, 1, 960, 960), (2.0, 1, 780, 960), (2.0, 3, 960, 1440)] {
        let mut rig = Rig::new();
        let params = crate::support::plain_synth();
        let channel = rig.synth_on(params, windfall_project::TrackId::MASTER);
        if rack {
            use windfall_project::rack::*;
            let node = |id, processor| RackNode { id, name: format!("Node {id}"), position: [0.0; 2], muted: false, solo: false, processor };
            let wire = |from, port, to, input| RackEdge { from: RackEndpoint { node: from, port },
                to: RackEndpoint { node: to, port: input }, gain: 1.0, parameter: None };
            rig.project.settings.modular_racks.push(ChannelRack { channel, graph: RackGraph { next_id: 4,
                nodes: vec![node(1, RackProcessor::NoteInput), node(2, RackProcessor::Instrument {
                    params: windfall_dsp::InstrumentParams::SubtractiveSynth(params) }), node(3, RackProcessor::AudioOutput)],
                edges: vec![wire(1, NOTE_OUT, 2, NOTE_IN), wire(2, AUDIO_OUT, 3, AUDIO_IN)] } });
        }
        rig.project.patterns[0].length_steps = 4;
        rig.note(channel, 720, 120).key = 60;
        let track = rig.playlist_track();
        rig.clip(track, rig.first_pattern(), 0, window);
        let mut reference = rig.project.clone();
        let original = reference.patterns[0].lanes[0].notes[0].clone();
        reference.patterns[0].lanes[0].notes.clear();
        for index in 0..copies {
            let mut note = original.clone();
            note.id = windfall_project::NoteId(reference.next_id);
            reference.next_id += 1;
            note.start += index * 240;
            note.key += index as u8 * 7;
            note.length = (note.length as f64 * scale) as u32;
            reference.patterns[0].lanes[0].notes.push(note);
        }
        reference.patterns[0].length_steps = effective_length / 240;
        reference.playlist.clips[0].length = effective_length;
        let script = ScriptReference { id: "duration-fixture".into(), revision: 1 };
        rig.project.settings.scripts = ScriptLibrary {
            assets: vec![ScriptAsset { reference: script.clone(), language_version: 1,
                kind: ScriptKind::NoteTransform, name: "Duration fixture".into(),
                source: format!("set length length {scale} *\ncopies {copies} 7 240\nemit") }],
            channels: vec![ChannelScript { channel, script, parameters: vec![] }], controllers: vec![],
        };
        rig.project.check().unwrap();
        let options = RenderOptions { mode: PlayMode::Song, tail_secs: 0.05, block_frames: 7, ..Default::default() };
        let actual = render(&rig.project, &rig.pool, &options, &mut |_| true);
        let expected = render(&reference, &rig.pool, &options, &mut |_| true);
        assert!(actual.samples().iter().any(|s| s.abs() > 0.001));
        assert_eq!(actual.samples(), expected.samples(), "scale={scale} copies={copies}");
        assert_eq!(actual.samples(), rig.play_song(48_000, actual.frames(), 251));
        let (mut processor, controller) = rig.song_processor(48_000);
        controller.play();
        let mut block = [0.0; 502];
        for _ in 0..actual.frames().div_ceil(251) {
            assert_eq!(crate::realtime::allocator_calls(|| processor.process(&mut block)), 0);
            controller.frame();
        }
        let json = windfall_project::file::to_json(&rig.project).unwrap();
        rig.project = windfall_project::file::from_json(&json).unwrap();
        assert_eq!(actual.samples(), render(&rig.project, &rig.pool, &RenderOptions { block_frames: 1024, ..options }, &mut |_| true).samples());
    }
    }
}
