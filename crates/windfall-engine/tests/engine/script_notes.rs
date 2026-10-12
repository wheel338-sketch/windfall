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
