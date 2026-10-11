//! Channel, plugin and mixer-track presets: the file envelope, loading as
//! one undo step, what the destination keeps, automation, refusals and
//! reading files from other builds.

use serde_json::{Value, json};
use windfall_project::preset::{
    MAX_PRESET_ASSETS, PRESET_SCHEMA, PRESET_VERSION, ParamRename, PresetPluginIdentity,
};
use windfall_project::*;

fn created(doc: &mut Document, command: Command) -> u32 {
    doc.dispatch(command, None).unwrap().created[0]
}

fn sample(doc: &mut Document, name: &str) -> SampleId {
    SampleId(created(
        doc,
        Command::AddSample {
            name: name.into(),
            path: SamplePath::External(format!("C:/sounds/{name}.wav")),
        },
    ))
}

fn sampler(doc: &mut Document, name: &str, sample: SampleId) -> ChannelId {
    ChannelId(created(
        doc,
        Command::AddChannel {
            name: Some(name.into()),
            sample: Some(sample),
            instrument: None,
            index: None,
            mixer_track: None,
        },
    ))
}

fn synth(doc: &mut Document, name: &str, kind: InstrumentKind) -> ChannelId {
    ChannelId(created(
        doc,
        Command::AddChannel {
            name: Some(name.into()),
            sample: None,
            instrument: Some(kind),
            index: None,
            mixer_track: None,
        },
    ))
}

fn effect(doc: &mut Document, track: TrackId, kind: EffectKind) -> EffectId {
    EffectId(created(
        doc,
        Command::AddEffect {
            track,
            kind,
            index: None,
        },
    ))
}

fn automate(doc: &mut Document, target: AutomationTarget) -> AutomationId {
    let id = AutomationId(created(
        doc,
        Command::AddAutomation {
            name: None,
            target,
            points: None,
        },
    ));
    let track = PlaylistTrackId(created(
        doc,
        Command::AddPlaylistTrack {
            name: None,
            index: None,
        },
    ));
    doc.dispatch(
        Command::AddClips {
            clips: vec![ClipInit {
                track,
                start: 0,
                length: Some(3840),
                offset: None,
                muted: None,
                content: ClipContent::Automation { automation: id },
            }],
        },
        None,
    )
    .unwrap();
    id
}

fn binding(id: &str, state: &[u8], parameter_ids: &[u32]) -> PluginBinding {
    PluginBinding {
        sidechain_input: None,
        auxiliary_inputs: Vec::new(),
        target: PluginTarget::Instrument {
            channel: ChannelId(999),
        },
        format: "clap".into(),
        path: format!("missing/{id}.clap"),
        id: id.into(),
        name: format!("{id} plugin"),
        state: state.to_vec(),
        parameters: parameter_ids
            .iter()
            .map(|&id| PluginParameter {
                id,
                name: format!("Parameter {id}"),
                min: 0.0,
                max: 10.0,
                value: 2.5,
                stepped: false,
                read_only: false,
                automatable: true,
            })
            .collect(),
    }
}

fn asset() -> PresetAsset {
    PresetAsset {
        id: 7,
        name: "Kick".into(),
        path: "Deep kick audio/Kick.wav".into(),
        bytes: 123_456,
    }
}

fn channel_of(doc: &Document, id: ChannelId) -> Channel {
    doc.project().channel(id).unwrap().clone()
}

/// The `type` a file gives a processor's settings.
fn tag(params: impl serde::Serialize) -> String {
    serde_json::to_value(params).unwrap()["type"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Dispatches a command that must be refused and checks that nothing moved.
fn refused(doc: &mut Document, command: Command) -> String {
    let before = doc.project().clone();
    let history = doc.history().entries.len();
    let error = doc.dispatch(command, None).unwrap_err();
    assert_eq!(*doc.project(), before);
    assert_eq!(doc.history().entries.len(), history);
    error.to_string()
}

#[test]
fn a_sampler_channel_preset_carries_its_audio_and_loads_as_one_undo_step() {
    let mut doc = Document::new(Project::new("Presets"));
    let kick = sample(&mut doc, "Kick");
    let source = sampler(&mut doc, "Deep kick", kick);
    doc.dispatch(
        Command::UpdateChannel {
            id: source,
            patch: ChannelPatch {
                volume: Some(0.41),
                pan: Some(-0.25),
                color: Some(0x12_34_56),
                timing: Some(ChannelTiming {
                    swing_mix: 0.5,
                    gate_ticks: 120,
                    shift_ticks: -7,
                }),
                ..ChannelPatch::default()
            },
        },
        None,
    )
    .unwrap();
    doc.dispatch(
        Command::UpdateSampler {
            id: source,
            patch: SamplerPatch {
                tune: Some(-5.0),
                root_key: Some(48),
                reverse: Some(true),
                ..SamplerPatch::default()
            },
        },
        None,
    )
    .unwrap();

    let (body, audio) = ChannelPreset::capture(doc.project(), source).unwrap();
    assert_eq!(audio.as_ref().map(|sample| sample.id), Some(kick));
    // Nothing in the body is a project id.
    let ChannelSource::Sampler(stored) = &body.source else {
        panic!("{body:?}")
    };
    assert_eq!(stored.sample, None);
    assert_eq!(stored.tune, -5.0);

    let preset = Preset::of_channel("Deep kick", body, Some(asset()));
    assert_eq!(preset.schema, PRESET_SCHEMA);
    assert_eq!(preset.version, PRESET_VERSION);
    assert_eq!(preset.kind, PresetKind::Channel);
    assert_eq!(preset.channel.as_ref().unwrap().sample_asset, Some(7));
    assert_eq!(preset.plugin_identity(), None);
    let text = preset.to_json().unwrap();
    let (read, notes) = Preset::from_json(text.as_bytes()).unwrap();
    assert_eq!(read, preset);
    assert_eq!(notes, Vec::<String>::new());
    assert_eq!(read.assets, [asset()]);

    // The destination is another channel, in a group, muted, on its own
    // mixer track, with a sample of its own.
    let snare = sample(&mut doc, "Snare");
    let destination = sampler(&mut doc, "Snare", snare);
    doc.dispatch(
        Command::UpdateChannel {
            id: destination,
            patch: ChannelPatch {
                group: Some("Drums".into()),
                muted: Some(true),
                ..ChannelPatch::default()
            },
        },
        None,
    )
    .unwrap();
    let expected = channel_of(&doc, destination);
    let before = doc.project().clone();
    let history = doc.history().entries.len();

    // What the shell does: the audio becomes a project sample and the
    // preset loads onto the channel, in one step.
    let applied = doc
        .dispatch(
            Command::Batch {
                label: Some("Load channel preset".into()),
                commands: vec![
                    Command::AddSample {
                        name: "Kick".into(),
                        path: SamplePath::External("C:/presets/Deep kick audio/Kick.wav".into()),
                    },
                    Command::ApplyChannelPreset {
                        id: destination,
                        expected: expected.clone(),
                        preset: read.channel.clone().unwrap(),
                        sample: Some(SampleId(before.next_id)),
                        audio: None,
                        name_color: false,
                    },
                ],
            },
            None,
        )
        .unwrap();
    assert_eq!(applied.label, "Load channel preset");
    assert_eq!(doc.history().entries.len(), history + 1);
    let loaded = channel_of(&doc, destination);
    let captured = channel_of(&doc, source);
    assert_eq!(loaded.volume, captured.volume);
    assert_eq!(loaded.pan, captured.pan);
    assert_eq!(loaded.timing, captured.timing);
    assert_eq!(loaded.voice, captured.voice);
    let (ChannelSource::Sampler(now), ChannelSource::Sampler(then)) =
        (&loaded.source, &captured.source)
    else {
        panic!("{loaded:?}")
    };
    assert_eq!(now.sample, Some(SampleId(before.next_id)));
    assert_eq!(
        SamplerSettings {
            sample: then.sample,
            ..now.clone()
        },
        *then
    );
    // The destination keeps its place in the project.
    assert_eq!(loaded.id, expected.id);
    assert_eq!(loaded.name, "Snare");
    assert_eq!(loaded.color, expected.color);
    assert_eq!(loaded.group, "Drums");
    assert!(loaded.muted);
    assert_eq!(loaded.solo, expected.solo);
    assert_eq!(loaded.mixer_track, expected.mixer_track);
    doc.project().check().unwrap();

    let after = doc.project().clone();
    assert_eq!(
        file::from_json(&file::to_json(&after).unwrap()).unwrap(),
        after
    );
    doc.undo().unwrap();
    let mut restored = before.clone();
    restored.next_id = after.next_id;
    assert_eq!(*doc.project(), restored);
    doc.redo().unwrap();
    assert_eq!(*doc.project(), after);
}

#[test]
fn a_presets_audio_file_joins_the_pool_in_the_same_step_and_is_not_added_twice() {
    let mut doc = Document::new(Project::new("Audio"));
    let kick = sample(&mut doc, "Kick");
    let source = sampler(&mut doc, "Kick", kick);
    let (body, _) = ChannelPreset::capture(doc.project(), source).unwrap();
    let first = synth(&mut doc, "First", InstrumentKind::SubtractiveSynth);
    let second = synth(&mut doc, "Second", InstrumentKind::SubtractiveSynth);
    let audio = PresetAudio {
        name: "Preset kick".into(),
        path: SamplePath::External("C:/presets/Kick audio/Kick.wav".into()),
    };

    let before = doc.project().clone();
    let history = doc.history().entries.len();
    let applied = doc
        .dispatch(
            Command::ApplyChannelPreset {
                id: first,
                expected: channel_of(&doc, first),
                preset: body.clone(),
                sample: None,
                audio: Some(audio.clone()),
                name_color: false,
            },
            None,
        )
        .unwrap();
    // One step that made one sample, and says which.
    assert_eq!(doc.history().entries.len(), history + 1);
    assert_eq!(applied.created, [before.next_id]);
    let added = SampleId(before.next_id);
    assert_eq!(doc.project().sample(added).unwrap().path, audio.path);
    assert_eq!(doc.project().sample(added).unwrap().name, "Preset kick");
    assert_eq!(channel_of(&doc, first).source.sample(), Some(added));
    assert_eq!(doc.project().samples.len(), before.samples.len() + 1);
    doc.project().check().unwrap();

    // The same file again: the sample the project has is the one used.
    let samples = doc.project().samples.len();
    let applied = doc
        .dispatch(
            Command::ApplyChannelPreset {
                id: second,
                expected: channel_of(&doc, second),
                preset: body,
                sample: None,
                audio: Some(audio),
                name_color: false,
            },
            None,
        )
        .unwrap();
    assert_eq!(applied.created, [added.0]);
    assert_eq!(doc.project().samples.len(), samples);
    assert_eq!(channel_of(&doc, second).source.sample(), Some(added));

    // Undoing the first load takes its sample out again with it.
    doc.undo().unwrap();
    doc.undo().unwrap();
    let mut restored = before.clone();
    restored.next_id = doc.project().next_id;
    assert_eq!(*doc.project(), restored);
}

#[test]
fn name_and_color_are_restored_only_when_asked() {
    let mut doc = Document::new(Project::new("Names"));
    let source = synth(&mut doc, "Glass pad", InstrumentKind::FourOp);
    doc.dispatch(
        Command::UpdateChannel {
            id: source,
            patch: ChannelPatch {
                color: Some(0xAB_CD_EF),
                ..ChannelPatch::default()
            },
        },
        None,
    )
    .unwrap();
    let (body, audio) = ChannelPreset::capture(doc.project(), source).unwrap();
    assert!(audio.is_none());
    assert_eq!(body.sample_asset, None);
    let destination = synth(&mut doc, "Lead", InstrumentKind::SubtractiveSynth);

    for name_color in [false, true] {
        let expected = channel_of(&doc, destination);
        doc.dispatch(
            Command::ApplyChannelPreset {
                id: destination,
                expected: expected.clone(),
                preset: body.clone(),
                sample: None,
                audio: None,
                name_color,
            },
            None,
        )
        .unwrap();
        let loaded = channel_of(&doc, destination);
        assert_eq!(loaded.source, channel_of(&doc, source).source);
        if name_color {
            assert_eq!(loaded.name, "Glass pad");
            assert_eq!(loaded.color, 0xAB_CD_EF);
        } else {
            assert_eq!(loaded.name, expected.name);
            assert_eq!(loaded.color, expected.color);
        }
        doc.project().check().unwrap();
        doc.undo().unwrap();
        assert_eq!(channel_of(&doc, destination), expected);
    }
}

#[test]
fn automation_survives_the_same_instrument_and_goes_with_another() {
    let mut doc = Document::new(Project::new("Automation"));
    let same = synth(&mut doc, "Same kind", InstrumentKind::SubtractiveSynth);
    let other = synth(&mut doc, "Other kind", InstrumentKind::FourOp);
    doc.dispatch(
        Command::SetInstrumentParam {
            channel: same,
            param: 0,
            value: 0.123,
        },
        None,
    )
    .ok();
    let (same_kind, _) = ChannelPreset::capture(doc.project(), same).unwrap();
    let (other_kind, _) = ChannelPreset::capture(doc.project(), other).unwrap();

    let destination = synth(&mut doc, "Lead", InstrumentKind::SubtractiveSynth);
    let setting = automate(
        &mut doc,
        AutomationTarget::InstrumentParam {
            channel: destination,
            param: 0,
        },
    );
    let volume = automate(
        &mut doc,
        AutomationTarget::ChannelVolume {
            channel: destination,
        },
    );
    let clips = doc.project().playlist.clips.len();

    // The same instrument: index 0 is still the same setting.
    let expected = channel_of(&doc, destination);
    doc.dispatch(
        Command::ApplyChannelPreset {
            id: destination,
            expected,
            preset: same_kind,
            sample: None,
            audio: None,
            name_color: false,
        },
        None,
    )
    .unwrap();
    assert!(doc.project().automation(setting).is_some());
    assert!(doc.project().automation(volume).is_some());
    assert_eq!(doc.project().playlist.clips.len(), clips);

    // Another instrument: index 0 is some other setting now, so the curve
    // that moved it goes, with its clip. The channel's own volume stays.
    let before = doc.project().clone();
    let expected = channel_of(&doc, destination);
    doc.dispatch(
        Command::ApplyChannelPreset {
            id: destination,
            expected,
            preset: other_kind,
            sample: None,
            audio: None,
            name_color: false,
        },
        None,
    )
    .unwrap();
    assert!(doc.project().automation(setting).is_none());
    assert!(doc.project().automation(volume).is_some());
    assert_eq!(doc.project().playlist.clips.len(), clips - 1);
    assert!(
        doc.project()
            .playlist
            .clips
            .iter()
            .all(|clip| clip.content.automation() != Some(setting))
    );
    doc.project().check().unwrap();
    doc.undo().unwrap();
    assert_eq!(*doc.project(), before);
}

#[test]
fn a_hosted_instrument_keeps_its_opaque_state_and_follows_the_destination() {
    let state: Vec<u8> = (0..=255).cycle().take(4096).collect();
    let mut doc = Document::new(Project::new("Hosted"));
    let source = ChannelId(created(
        &mut doc,
        Command::AddPluginInstrument {
            plugin: binding("org.example.synth", &state, &[43, 44]),
        },
    ));
    let (body, audio) = ChannelPreset::capture(doc.project(), source).unwrap();
    assert!(audio.is_none());
    let stored = body.plugin.as_ref().unwrap();
    // A placeholder, not the channel it was taken from.
    assert_eq!(
        stored.target,
        PluginTarget::Instrument {
            channel: ChannelId(0)
        }
    );
    assert_eq!(stored.state, state);

    let preset = Preset::of_channel("Hosted lead", body, None);
    assert_eq!(
        preset.plugin_identity(),
        Some(PresetPluginIdentity {
            format: "clap".into(),
            id: "org.example.synth".into(),
            instrument: true,
        })
    );
    let (read, notes) = Preset::from_json(preset.to_json().unwrap().as_bytes()).unwrap();
    assert_eq!(read, preset);
    assert!(notes.is_empty());

    // Onto a built-in synth that has automation of one of its settings.
    let destination = synth(&mut doc, "Lead", InstrumentKind::SubtractiveSynth);
    let builtin_curve = automate(
        &mut doc,
        AutomationTarget::InstrumentParam {
            channel: destination,
            param: 0,
        },
    );
    let before = doc.project().clone();
    let expected = channel_of(&doc, destination);
    doc.dispatch(
        Command::ApplyChannelPreset {
            id: destination,
            expected,
            preset: read.channel.clone().unwrap(),
            sample: None,
            audio: None,
            name_color: false,
        },
        None,
    )
    .unwrap();
    let target = PluginTarget::Instrument {
        channel: destination,
    };
    let bound = doc.project().plugin(target).unwrap();
    assert_eq!(bound.state, state);
    assert_eq!(bound.id, "org.example.synth");
    assert_eq!(bound.parameters.len(), 2);
    // The source still has its own binding: two instances, two targets.
    assert_eq!(doc.project().plugins.len(), 2);
    assert!(doc.project().automation(builtin_curve).is_none());
    doc.project().check().unwrap();
    let after = doc.project().clone();
    assert_eq!(
        file::from_json(&file::to_json(&after).unwrap()).unwrap(),
        after
    );
    doc.undo().unwrap();
    assert_eq!(*doc.project(), before);
    doc.redo().unwrap();

    // The same plugin again, with another parameter at index 1: the curve
    // on index 0 still moves parameter 43 and the one on index 1 goes.
    let kept = automate(
        &mut doc,
        AutomationTarget::InstrumentParam {
            channel: destination,
            param: 0,
        },
    );
    let stale = automate(
        &mut doc,
        AutomationTarget::InstrumentParam {
            channel: destination,
            param: 1,
        },
    );
    let mut update = read.channel.clone().unwrap();
    update.plugin = Some(PluginBinding {
        target: PluginTarget::Instrument {
            channel: ChannelId(0),
        },
        ..binding("org.example.synth", b"second state", &[43, 99])
    });
    let expected = channel_of(&doc, destination);
    doc.dispatch(
        Command::ApplyChannelPreset {
            id: destination,
            expected,
            preset: update,
            sample: None,
            audio: None,
            name_color: false,
        },
        None,
    )
    .unwrap();
    assert!(doc.project().automation(kept).is_some());
    assert!(doc.project().automation(stale).is_none());
    assert_eq!(doc.project().plugin(target).unwrap().state, b"second state");
    doc.project().check().unwrap();

    // A sampler preset takes the plugin away again.
    let kick = sample(&mut doc, "Kick");
    let drum = sampler(&mut doc, "Kick", kick);
    let (drum_body, _) = ChannelPreset::capture(doc.project(), drum).unwrap();
    let expected = channel_of(&doc, destination);
    doc.dispatch(
        Command::ApplyChannelPreset {
            id: destination,
            expected,
            preset: drum_body,
            sample: Some(kick),
            audio: None,
            name_color: false,
        },
        None,
    )
    .unwrap();
    assert!(doc.project().plugin(target).is_none());
    assert!(doc.project().automation(kept).is_none());
    assert_eq!(channel_of(&doc, destination).source.sample(), Some(kick));
    doc.project().check().unwrap();
}

#[test]
fn a_plugin_preset_replaces_one_effect_and_leaves_its_slot_alone() {
    let mut doc = Document::new(Project::new("Effects"));
    let source = effect(&mut doc, TrackId::MASTER, EffectKind::Limiter);
    doc.dispatch(
        Command::SetEffectParam {
            track: TrackId::MASTER,
            effect: source,
            param: 0,
            value: -3.0,
        },
        None,
    )
    .unwrap();
    let limiter = PluginPreset::capture(
        doc.project(),
        PluginTarget::Effect { effect: source },
    )
    .unwrap();
    let PluginPresetState::Effect { params } = &limiter.state else {
        panic!("{limiter:?}")
    };
    assert_eq!(params.kind(), EffectKind::Limiter);
    let preset = Preset::of_plugin("Loud", limiter.clone());
    assert_eq!(
        preset.plugin_identity(),
        Some(PresetPluginIdentity {
            format: "builtin".into(),
            id: tag(EffectKind::Limiter.default_params()),
            instrument: false,
        })
    );
    let (read, notes) = Preset::from_json(preset.to_json().unwrap().as_bytes()).unwrap();
    assert_eq!(read, preset);
    assert!(notes.is_empty());

    // The destination: a delay at half mix, bypassed, with automation of a
    // setting and of its mix.
    let destination = effect(&mut doc, TrackId::MASTER, EffectKind::Delay);
    doc.dispatch(
        Command::UpdateEffect {
            track: TrackId::MASTER,
            effect: destination,
            patch: EffectSlotPatch {
                enabled: Some(false),
                mix: Some(0.5),
            },
        },
        None,
    )
    .unwrap();
    let setting = automate(
        &mut doc,
        AutomationTarget::EffectParam {
            track: TrackId::MASTER,
            effect: destination,
            param: 0,
        },
    );
    let mix = automate(
        &mut doc,
        AutomationTarget::EffectMix {
            track: TrackId::MASTER,
            effect: destination,
        },
    );
    let target = PluginTarget::Effect {
        effect: destination,
    };
    let before = doc.project().clone();
    let history = doc.history().entries.len();
    let expected = PluginPreset::capture(doc.project(), target).unwrap().state;
    let applied = doc
        .dispatch(
            Command::ApplyPluginPreset {
                target,
                expected,
                preset: read.plugin.clone().unwrap(),
            },
            None,
        )
        .unwrap();
    assert_eq!(applied.label, "Load plugin preset");
    assert_eq!(doc.history().entries.len(), history + 1);
    let slot = doc.project().mixer.track(TrackId::MASTER).unwrap().effects[1].clone();
    assert_eq!(slot.id, destination);
    assert!(!slot.enabled);
    assert_eq!(slot.mix, 0.5);
    assert_eq!(slot.params, *params);
    assert!(doc.project().automation(setting).is_none());
    assert!(doc.project().automation(mix).is_some());
    doc.project().check().unwrap();
    doc.undo().unwrap();
    assert_eq!(*doc.project(), before);
    doc.redo().unwrap();

    // Onto the same kind: the automation of its settings stays.
    let kept = automate(
        &mut doc,
        AutomationTarget::EffectParam {
            track: TrackId::MASTER,
            effect: destination,
            param: 0,
        },
    );
    let expected = PluginPreset::capture(doc.project(), target).unwrap().state;
    doc.dispatch(
        Command::ApplyPluginPreset {
            target,
            expected,
            preset: limiter.clone(),
        },
        None,
    )
    .unwrap();
    assert!(doc.project().automation(kept).is_some());

    // A hosted effect onto the slot, and a built-in back over it.
    let mut hosted = binding("org.example.verb", b"reverb state", &[1]);
    hosted.target = PluginTarget::Effect {
        effect: EffectId(0),
    };
    let expected = PluginPreset::capture(doc.project(), target).unwrap().state;
    doc.dispatch(
        Command::ApplyPluginPreset {
            target,
            expected,
            preset: PluginPreset {
                state: PluginPresetState::Hosted {
                    instrument: false,
                    binding: hosted,
                },
            },
        },
        None,
    )
    .unwrap();
    assert_eq!(doc.project().plugin(target).unwrap().state, b"reverb state");
    assert!(doc.project().automation(kept).is_none());
    let captured = PluginPreset::capture(doc.project(), target).unwrap();
    let PluginPresetState::Hosted { binding: stored, .. } = &captured.state else {
        panic!("{captured:?}")
    };
    assert_eq!(
        stored.target,
        PluginTarget::Effect {
            effect: EffectId(0)
        }
    );
    doc.project().check().unwrap();
    doc.dispatch(
        Command::ApplyPluginPreset {
            target,
            expected: captured.state,
            preset: limiter,
        },
        None,
    )
    .unwrap();
    assert!(doc.project().plugin(target).is_none());
    doc.project().check().unwrap();
}

#[test]
fn a_stale_destination_a_wrong_role_and_invalid_bodies_are_refused_whole() {
    let mut doc = Document::new(Project::new("Refusals"));
    let kick = sample(&mut doc, "Kick");
    let drum = sampler(&mut doc, "Kick", kick);
    let lead = synth(&mut doc, "Lead", InstrumentKind::SubtractiveSynth);
    let limiter = effect(&mut doc, TrackId::MASTER, EffectKind::Limiter);
    let (drum_body, _) = ChannelPreset::capture(doc.project(), drum).unwrap();
    let (lead_body, _) = ChannelPreset::capture(doc.project(), lead).unwrap();

    // The channel is not the one the preset was chosen for any more.
    let mut seen = channel_of(&doc, lead);
    seen.volume = 0.111;
    let message = refused(
        &mut doc,
        Command::ApplyChannelPreset {
            id: lead,
            expected: seen,
            preset: drum_body.clone(),
            sample: Some(kick),
            audio: None,
            name_color: false,
        },
    );
    assert!(message.contains("changed"), "{message}");

    let expected = channel_of(&doc, lead);
    let channel = |preset: ChannelPreset, sample: Option<SampleId>| Command::ApplyChannelPreset {
        id: lead,
        expected: expected.clone(),
        preset,
        sample,
        audio: None,
        name_color: true,
    };
    // A sample the project does not have, and a sample for an instrument.
    refused(&mut doc, channel(drum_body.clone(), Some(SampleId(9_999))));
    refused(&mut doc, channel(lead_body.clone(), Some(kick)));
    // Audio as a sample and as a file at once, a file for an instrument,
    // and a file whose path a project could not store.
    let file = |path: &str| {
        Some(PresetAudio {
            name: "Kick".into(),
            path: SamplePath::Project(path.into()),
        })
    };
    for (preset, sample, audio) in [
        (drum_body.clone(), Some(kick), file("presets/Kick.wav")),
        (lead_body.clone(), None, file("presets/Kick.wav")),
        (drum_body.clone(), None, file("../outside/Kick.wav")),
    ] {
        refused(
            &mut doc,
            Command::ApplyChannelPreset {
                id: lead,
                expected: expected.clone(),
                preset,
                sample,
                audio,
                name_color: false,
            },
        );
    }
    // A channel that does not exist.
    refused(
        &mut doc,
        Command::ApplyChannelPreset {
            id: ChannelId(9_999),
            expected: expected.clone(),
            preset: lead_body.clone(),
            sample: None,
            audio: None,
            name_color: false,
        },
    );

    let mut invalid = Vec::new();
    let mut body = drum_body.clone();
    body.volume = f32::NAN;
    invalid.push(body);
    let mut body = drum_body.clone();
    body.pan = 1.5;
    invalid.push(body);
    let mut body = drum_body.clone();
    body.color = 0x1_00_00_00;
    invalid.push(body);
    let mut body = drum_body.clone();
    body.name = "bad\nname".into();
    invalid.push(body);
    // A project id inside a body.
    let mut body = drum_body.clone();
    let ChannelSource::Sampler(settings) = &mut body.source else {
        panic!()
    };
    settings.sample = Some(kick);
    invalid.push(body);
    // Sampler settings a project file would not load with.
    let mut body = drum_body.clone();
    let ChannelSource::Sampler(settings) = &mut body.source else {
        panic!()
    };
    settings.root_key = 200;
    invalid.push(body);
    // A plugin on a sampler, and audio on an instrument.
    let mut body = drum_body.clone();
    body.plugin = Some(PluginBinding {
        target: PluginTarget::Instrument {
            channel: ChannelId(0),
        },
        ..binding("org.example.synth", b"", &[])
    });
    invalid.push(body);
    let mut body = lead_body.clone();
    body.sample_asset = Some(1);
    invalid.push(body);
    // A binding that names a project object, and one with no identifier.
    let mut body = lead_body.clone();
    body.plugin = Some(binding("org.example.synth", b"", &[]));
    invalid.push(body);
    let mut body = lead_body.clone();
    body.plugin = Some(PluginBinding {
        target: PluginTarget::Instrument {
            channel: ChannelId(0),
        },
        id: String::new(),
        ..binding("org.example.synth", b"", &[])
    });
    invalid.push(body);
    for body in invalid {
        assert!(body.validate(body.sample_asset).is_err(), "{body:?}");
        refused(&mut doc, channel(body, None));
    }

    // Plugin presets: wrong role, stale destination, a sampler target.
    let effect_target = PluginTarget::Effect { effect: limiter };
    let lead_target = PluginTarget::Instrument { channel: lead };
    let effect_state = PluginPreset::capture(doc.project(), effect_target).unwrap();
    let lead_state = PluginPreset::capture(doc.project(), lead_target).unwrap();
    let message = refused(
        &mut doc,
        Command::ApplyPluginPreset {
            target: effect_target,
            expected: effect_state.state.clone(),
            preset: lead_state.clone(),
        },
    );
    assert!(message.contains("instrument preset"), "{message}");
    let message = refused(
        &mut doc,
        Command::ApplyPluginPreset {
            target: lead_target,
            expected: lead_state.state.clone(),
            preset: effect_state.clone(),
        },
    );
    assert!(message.contains("effect preset"), "{message}");
    refused(
        &mut doc,
        Command::ApplyPluginPreset {
            target: effect_target,
            expected: PluginPresetState::Effect {
                params: EffectKind::Delay.default_params(),
            },
            preset: effect_state.clone(),
        },
    );
    assert!(
        PluginPreset::capture(doc.project(), PluginTarget::Instrument { channel: drum })
            .unwrap_err()
            .contains("channel preset")
    );
    refused(
        &mut doc,
        Command::ApplyPluginPreset {
            target: PluginTarget::Instrument { channel: drum },
            expected: lead_state.state.clone(),
            preset: lead_state,
        },
    );
    refused(
        &mut doc,
        Command::ApplyPluginPreset {
            target: PluginTarget::Effect {
                effect: EffectId(9_999),
            },
            expected: effect_state.state.clone(),
            preset: effect_state,
        },
    );
}

#[test]
fn a_hosted_destination_is_recognised_by_its_plugin_not_by_its_changing_state() {
    let mut doc = Document::new(Project::new("Live state"));
    let channel = ChannelId(created(
        &mut doc,
        Command::AddPluginInstrument {
            plugin: binding("org.example.synth", b"first", &[43]),
        },
    ));
    let target = PluginTarget::Instrument { channel };
    let seen = PluginPreset::capture(doc.project(), target).unwrap();
    // The plugin reports a new state while the preset dialog is open.
    doc.dispatch(
        Command::SetPluginState {
            target,
            state: b"moved on".to_vec(),
        },
        None,
    )
    .unwrap();
    let preset = PluginPreset {
        state: PluginPresetState::Hosted {
            instrument: true,
            binding: PluginBinding {
                target: PluginTarget::Instrument {
                    channel: ChannelId(0),
                },
                ..binding("org.example.synth", b"from the preset", &[43])
            },
        },
    };
    doc.dispatch(
        Command::ApplyPluginPreset {
            target,
            expected: seen.state.clone(),
            preset: preset.clone(),
        },
        None,
    )
    .unwrap();
    assert_eq!(
        doc.project().plugin(target).unwrap().state,
        b"from the preset"
    );

    // Another plugin in its place is another destination.
    let PluginPresetState::Hosted { binding: mut other, .. } = seen.state else {
        panic!()
    };
    other.id = "org.example.other".into();
    refused(
        &mut doc,
        Command::ApplyPluginPreset {
            target,
            expected: PluginPresetState::Hosted {
                instrument: true,
                binding: other,
            },
            preset,
        },
    );
}

fn channel_file() -> Value {
    let mut doc = Document::new(Project::new("File"));
    let kick = sample(&mut doc, "Kick");
    let drum = sampler(&mut doc, "Kick", kick);
    let (body, _) = ChannelPreset::capture(doc.project(), drum).unwrap();
    serde_json::from_str(
        &Preset::of_channel("Kick", body, Some(asset()))
            .to_json()
            .unwrap(),
    )
    .unwrap()
}

fn read(value: &Value) -> Result<(Preset, Vec<String>), String> {
    Preset::from_json(serde_json::to_string(value).unwrap().as_bytes())
}

#[test]
fn the_envelope_refuses_other_schemas_newer_versions_and_unsafe_or_repeated_assets() {
    let file = channel_file();
    read(&file).unwrap();

    let mut other = file.clone();
    other["schema"] = json!("someone.else");
    assert!(read(&other).unwrap_err().contains("not a Windfall preset"));
    assert!(Preset::from_json(b"[1, 2, 3]").is_err());
    assert!(Preset::from_json(b"{}").unwrap_err().contains("not a Windfall preset"));
    assert!(Preset::from_json(b"not json").is_err());

    // A newer version is named, whatever else the file holds.
    let mut newer = file.clone();
    newer["version"] = json!(PRESET_VERSION + 1);
    newer["kind"] = json!("somethingNew");
    newer["somethingNew"] = json!({"anything": true});
    let message = read(&newer).unwrap_err();
    assert!(message.contains("newer Windfall"), "{message}");
    assert!(message.contains(&(PRESET_VERSION + 1).to_string()), "{message}");
    let mut unversioned = file.clone();
    unversioned["version"] = json!(0);
    assert!(read(&unversioned).is_err());

    for path in [
        "../Kick.wav",
        "audio/../../Kick.wav",
        "/Kick.wav",
        "C:/sounds/Kick.wav",
        "audio\\Kick.wav",
        "",
        "audio//Kick.wav",
    ] {
        let mut unsafe_path = file.clone();
        unsafe_path["assets"][0]["path"] = json!(path);
        assert!(read(&unsafe_path).is_err(), "{path:?}");
    }

    // Two assets with one id, two with one path, and more than a preset lists.
    let mut repeated = file.clone();
    let first = repeated["assets"][0].clone();
    repeated["assets"].as_array_mut().unwrap().push(first);
    assert!(read(&repeated).unwrap_err().contains("twice"));
    let mut same_path = file.clone();
    let mut second = same_path["assets"][0].clone();
    second["id"] = json!(8);
    second["path"] = json!("deep KICK audio/kick.WAV");
    same_path["assets"].as_array_mut().unwrap().push(second);
    assert!(read(&same_path).unwrap_err().contains("twice"));
    let mut many = file.clone();
    let assets: Vec<Value> = (0..=MAX_PRESET_ASSETS)
        .map(|index| json!({"id": index, "name": "A", "path": format!("a/{index}.wav"), "bytes": 1}))
        .collect();
    many["assets"] = json!(assets);
    assert!(read(&many).is_err());

    // The body has to be the one the kind names, and the audio one it plays.
    let mut wrong_kind = file.clone();
    wrong_kind["kind"] = json!("plugin");
    assert!(read(&wrong_kind).is_err());
    let mut no_body = file.clone();
    no_body.as_object_mut().unwrap().remove("channel");
    assert!(read(&no_body).is_err());
    let mut unlisted = file.clone();
    unlisted["channel"]["sampleAsset"] = json!(99);
    assert!(read(&unlisted).is_err());
    let mut unused = file.clone();
    unused["channel"].as_object_mut().unwrap().remove("sampleAsset");
    assert!(read(&unused).is_err());
    let mut long_name = file.clone();
    long_name["name"] = json!("n".repeat(257));
    assert!(read(&long_name).is_err());
}

#[test]
fn settings_from_another_build_are_renamed_dropped_defaulted_and_clamped_with_a_report() {
    let mut doc = Document::new(Project::new("Migration"));
    let limiter = effect(&mut doc, TrackId::MASTER, EffectKind::Limiter);
    let preset = Preset::of_plugin(
        "Loud",
        PluginPreset::capture(doc.project(), PluginTarget::Effect { effect: limiter }).unwrap(),
    );
    let file: Value = serde_json::from_str(&preset.to_json().unwrap()).unwrap();
    let kind = tag(EffectKind::Limiter.default_params());
    let params = file["plugin"]["state"]["params"].as_object().unwrap().clone();
    // The first two numeric settings the limiter has in this build.
    let numeric: Vec<String> = params
        .iter()
        .filter(|(key, value)| key.as_str() != "type" && value.is_number())
        .map(|(key, _)| key.clone())
        .collect();
    let (renamed, missing) = (numeric[0].clone(), numeric[1].clone());
    let renames = [ParamRename {
        processor: Box::leak(kind.clone().into_boxed_str()),
        from: "nameBeforeTheRename",
        to: Box::leak(renamed.clone().into_boxed_str()),
    }];

    // An older build's file: one setting under its old name, one it never
    // had, and one this build no longer has.
    let mut old = file.clone();
    let state = old["plugin"]["state"]["params"].as_object_mut().unwrap();
    let default = state.remove(&renamed).unwrap();
    // A value that is not the default, so keeping it can be told from
    // falling back. It is inside the range of both leading limiter settings.
    let value = json!(-1.5);
    assert_ne!(default, value);
    state.insert("nameBeforeTheRename".into(), value.clone());
    state.remove(&missing);
    state.insert("settingThatWasRemoved".into(), json!(0.25));
    let text = serde_json::to_string(&old).unwrap();

    let (read_back, notes) = Preset::from_json_with(text.as_bytes(), &renames).unwrap();
    // Settings are 32-bit; compare them as that, not as JSON's 64-bit text.
    let single = |value: &Value| value.as_f64().unwrap() as f32;
    let read_params =
        serde_json::to_value(&read_back.plugin.as_ref().unwrap().state).unwrap()["params"].clone();
    assert_eq!(
        single(&read_params[&renamed]),
        single(&value),
        "the renamed value was kept"
    );
    assert_eq!(
        single(&read_params[&missing]),
        single(&params[&missing]),
        "the default"
    );
    assert!(read_params.get("settingThatWasRemoved").is_none());
    assert!(read_params.get("nameBeforeTheRename").is_none());
    assert_eq!(notes.len(), 3, "{notes:?}");
    assert!(notes.iter().any(|note| note.contains("nameBeforeTheRename")));
    assert!(notes.iter().any(|note| note.contains("settingThatWasRemoved")));
    assert!(notes.iter().any(|note| note.contains(&missing)));
    // What was read loads like any preset.
    let target = PluginTarget::Effect { effect: limiter };
    let expected = PluginPreset::capture(doc.project(), target).unwrap().state;
    doc.dispatch(
        Command::ApplyPluginPreset {
            target,
            expected,
            preset: read_back.plugin.unwrap(),
        },
        None,
    )
    .unwrap();

    // Without the rename table the old name is only an unknown setting:
    // the value is lost, and the report says so.
    let (plain, notes) = Preset::from_json(text.as_bytes()).unwrap();
    let plain_params =
        serde_json::to_value(&plain.plugin.as_ref().unwrap().state).unwrap()["params"].clone();
    assert_eq!(single(&plain_params[&renamed]), single(&params[&renamed]));
    assert!(notes.iter().any(|note| note.contains("nameBeforeTheRename")));

    // A value far outside its range is brought into it, and reported.
    let mut wild = file.clone();
    wild["plugin"]["state"]["params"][&renamed] = json!(1.0e30);
    let (clamped, notes) = read(&wild).unwrap();
    assert!(notes.iter().any(|note| note.contains("ranges")), "{notes:?}");
    clamped.validate().unwrap();

    // A processor this build does not have is refused by name.
    let mut unknown = file.clone();
    unknown["plugin"]["state"]["params"]["type"] = json!("effectFromTheFuture");
    let message = read(&unknown).unwrap_err();
    assert!(message.contains("effectFromTheFuture"), "{message}");

    // The same holds for an instrument inside a channel preset.
    let lead = synth(&mut doc, "Lead", InstrumentKind::SubtractiveSynth);
    let (body, _) = ChannelPreset::capture(doc.project(), lead).unwrap();
    let mut channel: Value =
        serde_json::from_str(&Preset::of_channel("Lead", body, None).to_json().unwrap()).unwrap();
    channel["channel"]["source"]["params"]["settingThatWasRemoved"] = json!(1);
    let (_, notes) = read(&channel).unwrap();
    assert_eq!(notes.len(), 1, "{notes:?}");
    channel["channel"]["source"]["params"]["type"] = json!("synthFromTheFuture");
    assert!(read(&channel).unwrap_err().contains("synthFromTheFuture"));
}

#[test]
fn a_mixer_track_preset_travels_in_the_same_envelope_unchanged() {
    let mut doc = Document::new(Project::new("Mixer"));
    let source = TrackId(created(
        &mut doc,
        Command::AddMixerTrack {
            name: Some("Drum bus".into()),
        },
    ));
    effect(&mut doc, source, EffectKind::Limiter);
    let destination = TrackId(created(
        &mut doc,
        Command::AddMixerTrack {
            name: Some("Bass".into()),
        },
    ));
    let body = MixerTrackPreset::capture(doc.project(), source).unwrap();
    let preset = Preset::of_mixer_track(body.clone());
    assert_eq!(preset.kind, PresetKind::MixerTrack);
    assert_eq!(preset.name, "Drum bus");
    assert_eq!(preset.plugin_identity(), None);
    let (read_back, notes) = Preset::from_json(preset.to_json().unwrap().as_bytes()).unwrap();
    assert!(notes.is_empty());
    assert_eq!(read_back.mixer_track.as_ref(), Some(&body));

    // The mixer's own command loads it.
    let expected = doc.project().mixer.track(destination).unwrap().clone();
    doc.dispatch(
        Command::ApplyMixerTrackPreset {
            id: destination,
            expected,
            preset: read_back.mixer_track.unwrap(),
            name_color: false,
        },
        None,
    )
    .unwrap();
    let loaded = doc.project().mixer.track(destination).unwrap();
    assert_eq!(loaded.effects.len(), 1);
    assert_eq!(loaded.effects[0].params, body.effects[0].params);
    doc.project().check().unwrap();

    // A mixer body the mixer itself would refuse is refused in the envelope.
    let mut invalid = body;
    invalid.version = 2;
    assert!(Preset::of_mixer_track(invalid).validate().is_err());
}
