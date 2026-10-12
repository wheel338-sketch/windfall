//! Native Session publication, matching the mounted WAV-import payload.
//! Uses the production session/engine and a fake device; no physical audio IO.
use super::{Rig, rms};
use windfall_project::{Command, ConvolverImpulse, EffectId, EffectKind, TrackId};

fn impulse(gain: f32) -> ConvolverImpulse {
    ConvolverImpulse {
        name: "Own IR.wav".into(),
        sample_rate: 48_000,
        group_delay: 0,
        eq_gains: None,
        frames: vec![[gain, gain], [gain * 0.5, gain * 0.5]],
    }
}

fn publish(rig: &Rig, effect: EffectId, impulse: Option<ConvolverImpulse>) -> Result<(), String> {
    rig.session
        .dispatch(
            Command::SetConvolverImpulse {
                track: TrackId::MASTER,
                effect,
                impulse,
            },
            None,
        )
        .map(|_| ())
}

fn offline(rig: &Rig) -> Vec<f32> {
    let (project, pool) = {
        let state = rig.session.state();
        (state.document.project().clone(), state.pool.clone())
    };
    let rendered = windfall_engine::render_reporting(
        &project,
        &pool,
        &windfall_engine::RenderOptions {
            block_frames: 173,
            ..Default::default()
        },
        &mut |_| true,
    );
    assert!(
        rendered.plugin_error.is_none(),
        "{:?}",
        rendered.plugin_error
    );
    assert!(rendered.sampler_error.is_none());
    rendered.audio.samples().to_vec()
}

#[test]
fn save_snapshot_retains_drawn_assets_across_project_replacement_and_collection() {
    use windfall_dsp::asset::{CurvePoint, ShapeRef};
    let mut rig = Rig::new();
    let effect = EffectId(
        rig.session
            .dispatch(
                Command::AddEffect {
                    track: TrackId::MASTER,
                    kind: EffectKind::Waveshaper,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    let shape = ShapeRef::new(&[
        CurvePoint::new(0.0, 0.091371),
        CurvePoint::new(1.0, 0.913719),
    ])
    .unwrap();
    rig.session
        .dispatch(
            Command::SetEffectParams {
                track: TrackId::MASTER,
                effect,
                params: windfall_dsp::EffectParams::Waveshaper(
                    windfall_dsp::drive::WaveshaperParams {
                        shape,
                        ..Default::default()
                    },
                ),
            },
            None,
        )
        .unwrap();
    rig.run(4096);
    let captured = rig.project();
    let path = rig.file("pinned-drawn-snapshot.windfall");
    let hold = rig.session.hold("save:write");
    let writer = rig
        .session
        .background(move |session| session.project_save(Some(&path)));
    hold.wait();
    rig.session.project_new().unwrap();
    rig.run(4096);
    rig.session.realtime_tick();
    assert!(
        shape.asset().is_some(),
        "the blocked save still owns its curve"
    );
    hold.release();
    let path = writer.join().unwrap().unwrap();
    let reopened = windfall_project::file::load(path).unwrap();
    assert_eq!(
        reopened, captured,
        "a save must preserve the captured project even when the live project is replaced"
    );
}

#[test]
fn session_ir_replacement_undo_reopen_and_expanded_rate_refusal_preserve_document_and_audio() {
    let mut rig = Rig::new();
    let (channel, _) = super::effects::add_synth(&rig);
    let effect = EffectId(
        rig.session
            .dispatch(
                Command::AddEffect {
                    track: TrackId::MASTER,
                    kind: EffectKind::Convolver,
                    index: None,
                },
                None,
            )
            .unwrap()
            .created[0],
    );
    rig.run(1024);
    let dry = offline(&rig);
    assert!(rms(&dry[..24_000]) > 0.001);
    rig.session.audition_note_on(channel, 48, 0.8);
    rig.run(12_000);
    let mut previous_level = 0.0;
    for iteration in 0..12 {
        let asset = impulse(0.2 + iteration as f32 * 0.025);
        let before = rig.session.document_snapshot();
        publish(&rig, effect, Some(asset.clone())).unwrap();
        let after = rig.session.document_snapshot();
        assert_eq!(after.history.cursor, before.history.cursor + 1);
        assert_eq!(
            after.project.mixer.tracks[0].effects[0].impulse,
            Some(asset.clone())
        );
        let live = rig.run(8192); // Adopt the owner and finish its rack fade.
        assert!(live.iter().all(|v| v.is_finite()));
        let level = rms(&live[4096..]);
        assert!(
            level > previous_level * 1.01 && level > 0.001,
            "replacement {iteration} did not publish its larger IR gain: {level} after {previous_level}"
        );
        previous_level = level;
        rig.session.realtime_tick();
        let wet = offline(&rig);
        assert!(wet.iter().all(|v| v.is_finite()));
        assert_ne!(&wet[..24_000], &dry[..24_000]);
        rig.session.undo().unwrap();
        rig.run(2048);
        rig.session.realtime_tick();
        assert_eq!(rig.project(), before.project);
        rig.session.redo().unwrap();
        rig.run(2048);
        rig.session.realtime_tick();
        assert_eq!(rig.project(), after.project);
        assert_eq!(offline(&rig), wet);
    }

    rig.session.audition_note_off(channel, 48);
    rig.run(4800);

    let saved = rig.project();
    let reference = offline(&rig);
    let path = rig
        .session
        .project_save(Some(&rig.file("native-convolver")))
        .unwrap();
    let mut rig = rig.restart();
    assert_eq!(rig.session.project_open(&path).unwrap().project, saved);
    rig.run(2048);
    rig.session.realtime_tick();
    assert_eq!(offline(&rig), reference);

    // The project limit accepts this source-rate IR; the actual 48 kHz host
    // expansion refuses it. Dispatch must fail BEFORE document/history commit.
    rig.session.transport_seek(0.0);
    rig.session.transport_play().unwrap();
    assert!(rms(&rig.run(12_000)) > 0.001);
    let snapshot = rig.session.document_snapshot();
    let latency = rig.session.controller().latency_frames();
    let bad = ConvolverImpulse {
        name: "Original oversized-at-host-rate IR".into(),
        sample_rate: 8000,
        group_delay: 0,
        eq_gains: None,
        frames: vec![[0.0; 2]; 65_536],
    };
    let error = publish(&rig, effect, Some(bad)).unwrap_err();
    assert!(!error.is_empty());
    assert_eq!(rig.session.document_snapshot(), snapshot);
    assert_eq!(rig.session.controller().latency_frames(), latency);
    assert_eq!(offline(&rig), reference);
    assert!(
        rms(&rig.run(4000)) > 0.001,
        "the admitted owner keeps sounding"
    );
    rig.session.transport_stop();
    rig.run(4800);
    publish(&rig, effect, None).unwrap();
    rig.run(2048);
    rig.session.realtime_tick();
    assert!(rig.project().mixer.tracks[0].effects[0].impulse.is_none());
    rig.session.undo().unwrap();
    rig.run(2048);
    rig.session.realtime_tick();
    assert_eq!(rig.project(), saved);
    assert_eq!(offline(&rig), reference);
}
