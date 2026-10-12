//! Native publication counterparts of mounted WASM rack gestures/tempo fit.
use super::{Rig, SAMPLE_RATE, rms};
use windfall_core::AudioBuffer;
use windfall_project::{
    ChannelId, ClipStretchQuality, Command, MixerTrackPatch, ModulationEnvelope, SamplerKeyRange,
    SamplerPatch, SamplerStretch, TrackId, VoiceFilter,
};

fn fixture(rig: &Rig) -> ChannelId {
    let path = rig.file("rack-two-seconds.wav");
    let pcm = (0..96000)
        .flat_map(|i| {
            let s = 0.4 * (std::f32::consts::TAU * i as f32 * 2000.0 / 48000.0).sin();
            [s, s]
        })
        .collect();
    windfall_codec::write_wav(
        &path,
        &AudioBuffer::from_interleaved(SAMPLE_RATE, 2, pcm),
        windfall_codec::WavSampleFormat::Float32,
    )
    .unwrap();
    rig.session.add_channel_from_file(&path, None).unwrap();
    let channel = rig.project().channels.last().unwrap().id;
    let project = rig.project();
    let track = project.channel(channel).unwrap().mixer_track;
    for id in [track, TrackId::MASTER] {
        rig.session
            .dispatch(
                Command::UpdateMixerTrack {
                    id,
                    patch: MixerTrackPatch {
                        volume: Some(1.0),
                        ..Default::default()
                    },
                },
                None,
            )
            .unwrap();
    }
    channel
}
fn heard(rig: &mut Rig, channel: ChannelId, frames: usize) -> Vec<f32> {
    rig.session.transport_stop();
    rig.run(1024);
    rig.session.audition_note_on(channel, 60, 1.0);
    rig.run(frames)
}

#[test]
fn envelope_and_target_lfo_gestures_publish_audible_native_changes_as_one_history_step() {
    let mut rig = Rig::new();
    let channel = fixture(&rig);
    let mut settings = rig.project().channel(channel).unwrap().voice;
    settings.envelopes.filter_settings = VoiceFilter {
        enabled: true,
        cutoff_hz: 300.0,
        ..Default::default()
    };
    settings.envelopes.filter = ModulationEnvelope {
        enabled: true,
        attack_ms: 0.0,
        decay_ms: 1.0,
        sustain: 1.0,
        release_ms: 1.0,
        depth: 4.0,
    };
    rig.session
        .dispatch(
            Command::SetChannelVoiceSettings {
                id: channel,
                settings,
            },
            None,
        )
        .unwrap();
    let dry = heard(&mut rig, channel, 12000);
    let before = rig.session.document_snapshot();
    for attack_ms in [50.0, 100.0, 150.0, 200.0] {
        settings.envelopes.filter.attack_ms = attack_ms;
        rig.session
            .dispatch(
                Command::SetChannelVoiceSettings {
                    id: channel,
                    settings,
                },
                Some(87001),
            )
            .unwrap();
    }
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    let slow = heard(&mut rig, channel, 12000);
    assert!(
        rms(&slow[1000..4000]) < rms(&dry[1000..4000]) * 0.6,
        "native filter attack is audible"
    );
    rig.session.undo().unwrap();
    assert_eq!(heard(&mut rig, channel, 12000), dry);
    rig.session.redo().unwrap();
    assert_eq!(heard(&mut rig, channel, 12000), slow);
    settings.envelopes.filter.attack_ms = 0.0;
    settings.envelopes.lfos.volume.enabled = true;
    settings.envelopes.lfos.volume.rate_hz = 10.0;
    settings.envelopes.lfos.volume.depth = 0.0;
    rig.session
        .dispatch(
            Command::SetChannelVoiceSettings {
                id: channel,
                settings,
            },
            None,
        )
        .unwrap();
    let base = heard(&mut rig, channel, 12000);
    let before = rig.session.document_snapshot();
    for depth in [0.2, 0.4, 0.6, 0.8] {
        settings.envelopes.lfos.volume.depth = depth;
        rig.session
            .dispatch(
                Command::SetChannelVoiceSettings {
                    id: channel,
                    settings,
                },
                Some(87002),
            )
            .unwrap();
    }
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    let tremolo = heard(&mut rig, channel, 12000);
    assert!(rms(&tremolo[2000..18000]) < rms(&base[2000..18000]) * 0.9);
    let mut block = [0.0; 512];
    assert_eq!(
        crate::test_alloc::allocator_calls(|| rig.processor.process(&mut block)),
        0
    );
    let saved = rig.file("rack-gestures.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    rig.session.project_new().unwrap();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(heard(&mut rig, channel, 12000), tremolo);
}

#[test]
fn tempo_fit_prepares_the_requested_native_duration_and_restores_it_after_history_and_reopen() {
    let mut rig = Rig::new();
    let channel = fixture(&rig);
    let source = rig
        .project()
        .channel(channel)
        .unwrap()
        .source
        .sample()
        .unwrap();
    let source_secs = rig
        .session
        .state()
        .pool
        .get(source)
        .unwrap()
        .duration_secs();
    let fitted_seconds = 2.0 * 60.0 / rig.project().settings.tempo_bpm;
    let ratio = fitted_seconds / source_secs;
    assert_eq!(ratio, 0.5);
    let before = rig.session.document_snapshot();
    let request = rig.session.sampler_preparation_begin().unwrap();
    rig.session
        .prepare_sampler_command(
            Command::UpdateSampler {
                id: channel,
                patch: SamplerPatch {
                    stretch: Some(SamplerStretch::Spectral {
                        ratio,
                        quality: ClipStretchQuality::Fast,
                        formants: false,
                        range: SamplerKeyRange {
                            first: 60,
                            last: 60,
                        },
                    }),
                    ..Default::default()
                },
            },
            request,
        )
        .unwrap();
    assert_eq!(
        rig.session.document_snapshot().history.cursor,
        before.history.cursor + 1
    );
    let fitted = heard(&mut rig, channel, 60000);
    let last = fitted
        .as_chunks::<2>()
        .0
        .iter()
        .rposition(|s| s[0].abs() > 1e-4)
        .unwrap();
    let expected = (fitted_seconds * f64::from(SAMPLE_RATE)).round() as usize;
    assert!(
        last >= expected - 512 && last < expected + 512,
        "prepared end {last}, independently fitted {expected}"
    );
    assert!(fitted[expected * 2 + 1024..].iter().all(|s| *s == 0.0));
    rig.session.undo().unwrap();
    assert!(
        rms(&heard(&mut rig, channel, 60000)[100000..]) > 0.01,
        "original two seconds restored"
    );
    rig.session.redo().unwrap();
    assert_eq!(heard(&mut rig, channel, 60000), fitted);
    let saved = rig.file("tempo-fit.windfall");
    rig.session.project_save(Some(&saved)).unwrap();
    rig.session.project_new().unwrap();
    rig.session.project_open(&saved).unwrap();
    assert_eq!(heard(&mut rig, channel, 60000), fitted);
}
