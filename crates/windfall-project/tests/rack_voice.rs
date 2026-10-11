//! The channel filter, per-target envelopes and LFOs, and layer channels
//! through checked commands, history and project files.

use windfall_project::*;

fn add_channel(document: &mut Document, name: &str) -> ChannelId {
    ChannelId(
        document
            .dispatch(
                Command::AddChannel {
                    name: Some(name.into()),
                    sample: None,
                    instrument: None,
                    index: None,
                    mixer_track: Some(TrackId::MASTER),
                },
                None,
            )
            .unwrap()
            .created[0],
    )
}

fn voice(document: &Document, id: ChannelId) -> ChannelVoiceSettings {
    document.project().channel(id).unwrap().voice
}

fn layer(children: &[ChannelId]) -> ChannelVoiceSettings {
    ChannelVoiceSettings {
        layer: LayerSettings {
            enabled: true,
            children: LayerChildren::new(children.iter().copied()),
        },
        ..Default::default()
    }
}

fn set(
    document: &mut Document,
    id: ChannelId,
    settings: ChannelVoiceSettings,
) -> Result<Applied, CommandError> {
    document.dispatch(Command::SetChannelVoiceSettings { id, settings }, None)
}

fn reload(document: &Document) -> Project {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("rack.windfall");
    file::save(document.project(), &path).unwrap();
    file::load(&path).unwrap()
}

#[test]
fn filter_envelope_and_lfo_settings_are_sanitized_undoable_and_saved() {
    let mut document = Document::new(Project::new("Modulation"));
    let id = add_channel(&mut document, "Pad");
    let mut settings = ChannelVoiceSettings::default();
    settings.envelopes.filter_settings = VoiceFilter {
        enabled: true,
        mode: VoiceFilterMode::BandPass,
        cutoff_hz: 1.0e9,
        resonance: -4.0,
    };
    settings.envelopes.resonance = ModulationEnvelope {
        enabled: true,
        attack_ms: -1.0,
        decay_ms: 0.0,
        sustain: 2.0,
        release_ms: f32::NAN,
        depth: 7.0,
    };
    settings.envelopes.lfos.volume = TargetLfo {
        enabled: true,
        shape: LfoShape::Square,
        rate_hz: 1000.0,
        depth: -3.0,
    };
    settings.envelopes.lfos.cutoff.rate_hz = f32::INFINITY;
    settings.envelopes.lfo.target = LfoTarget::Resonance;

    let before = document.history().cursor;
    let applied = set(&mut document, id, settings).unwrap();
    assert!(applied.touched.channels);
    let stored = voice(&document, id);
    assert_eq!(stored, settings.sanitized());
    let filter = stored.envelopes.filter_settings;
    assert_eq!(
        (
            filter.enabled,
            filter.mode,
            filter.cutoff_hz,
            filter.resonance
        ),
        (true, VoiceFilterMode::BandPass, MAX_VOICE_CUTOFF_HZ, 0.0)
    );
    let resonance = stored.envelopes.resonance;
    assert_eq!(
        (
            resonance.attack_ms,
            resonance.decay_ms,
            resonance.sustain,
            resonance.depth
        ),
        (0.0, 1.0, 1.0, 1.0)
    );
    assert_eq!(
        resonance.release_ms,
        ModulationEnvelope::default().release_ms
    );
    assert_eq!(stored.envelopes.lfos.volume.rate_hz, 30.0);
    assert_eq!(stored.envelopes.lfos.volume.depth, -1.0);
    assert_eq!(
        stored.envelopes.lfos.cutoff.rate_hz,
        TargetLfo::default().rate_hz
    );
    assert_eq!(stored.envelopes.lfo.target, LfoTarget::Resonance);
    document.project().check().unwrap();

    // One undo step; save and reopen in both states.
    assert_eq!(reload(&document).channel(id).unwrap().voice, stored);
    document.undo().unwrap();
    assert_eq!(document.history().cursor, before, "one undo step");
    assert_eq!(voice(&document, id), ChannelVoiceSettings::default());
    assert_eq!(
        reload(&document).channel(id).unwrap().voice,
        ChannelVoiceSettings::default()
    );
    document.redo().unwrap();
    assert_eq!(voice(&document, id), stored);
}

#[test]
fn files_from_before_the_filter_and_layers_load_with_neutral_defaults() {
    let mut document = Document::new(Project::new("Legacy"));
    let id = add_channel(&mut document, "Old");
    let mut settings = ChannelVoiceSettings::default();
    settings.envelopes.pitch.enabled = true;
    settings.envelopes.pitch.depth = 700.0;
    settings.envelopes.lfo.enabled = true;
    settings.envelopes.lfo.target = LfoTarget::Pan;
    set(&mut document, id, settings).unwrap();

    // What an older build wrote: no resonance envelope, per-target LFOs,
    // filter settings or layer. Settings left at their defaults are not
    // written either, so an old file saves back with the same keys.
    let mut legacy = serde_json::to_value(document.project()).unwrap();
    let envelopes = legacy["channels"][0]["voice"]["envelopes"]
        .as_object()
        .unwrap();
    for key in ["resonance", "lfos", "filterSettings"] {
        assert!(
            !envelopes.contains_key(key),
            "{key} is not written by default"
        );
    }
    assert!(
        !legacy["channels"][0]["voice"]
            .as_object()
            .unwrap()
            .contains_key("layer")
    );
    let loaded = file::from_json(&legacy.to_string()).unwrap();
    loaded.check().unwrap();
    assert_eq!(loaded.channel(id).unwrap().voice, settings);
    let envelopes = loaded.channel(id).unwrap().voice.envelopes;
    assert_eq!(envelopes.resonance, ModulationEnvelope::default());
    assert_eq!(envelopes.lfos, ChannelTargetLfos::default());
    assert_eq!(envelopes.filter_settings, VoiceFilter::default());
    assert!(!envelopes.filter_settings.enabled);
    assert!(!envelopes.lfos.any_enabled());
    assert_eq!(
        loaded.channel(id).unwrap().voice.layer,
        LayerSettings::default()
    );

    // Out-of-range values in a file are repaired on load, like the rest of
    // the voice tools.
    legacy["channels"][0]["voice"]["envelopes"]["filterSettings"] =
        serde_json::json!({"enabled": true, "cutoffHz": 5.0, "resonance": 9.0});
    let repaired = file::from_json(&legacy.to_string()).unwrap();
    repaired.check().unwrap();
    let filter = repaired
        .channel(id)
        .unwrap()
        .voice
        .envelopes
        .filter_settings;
    assert_eq!(
        (filter.cutoff_hz, filter.resonance),
        (MIN_VOICE_CUTOFF_HZ, 1.0)
    );
}

#[test]
fn a_layer_is_set_checked_undone_and_saved_as_one_edit() {
    let mut document = Document::new(Project::new("Layers"));
    let bass = add_channel(&mut document, "Bass");
    let lead = add_channel(&mut document, "Lead");
    let top = add_channel(&mut document, "Layer");

    let before = document.history().cursor;
    set(&mut document, top, layer(&[bass, lead])).unwrap();
    assert_eq!(
        voice(&document, top)
            .layer
            .children
            .iter()
            .collect::<Vec<_>>(),
        [bass, lead]
    );
    document.project().check().unwrap();
    assert_eq!(
        layer::targets(&document.project().channels, 2),
        [0, 1],
        "the layer resolves to its two children"
    );

    // Refused edits change neither the project nor the history.
    let snapshot = document.project().clone();
    let history = document.history();
    for (channel, bad) in [
        (top, layer(&[top])),
        (top, layer(&[bass, ChannelId(9999)])),
        // bass -> top would close the loop top -> bass -> top.
        (bass, layer(&[top])),
        (ChannelId(9999), layer(&[bass])),
    ] {
        assert!(set(&mut document, channel, bad).is_err());
        assert_eq!(document.project(), &snapshot);
        assert_eq!(document.history(), history);
    }

    // Saved as a plain list of ids and loaded back.
    let json = serde_json::to_value(document.project()).unwrap();
    assert_eq!(
        json["channels"][2]["voice"]["layer"],
        serde_json::json!({"enabled": true, "children": [bass.0, lead.0]})
    );
    assert_eq!(
        reload(&document).channel(top).unwrap().voice,
        layer(&[bass, lead])
    );

    document.undo().unwrap();
    assert_eq!(document.history().cursor, before, "one undo step");
    assert_eq!(voice(&document, top), ChannelVoiceSettings::default());
    document.redo().unwrap();
    assert_eq!(voice(&document, top), layer(&[bass, lead]));
}

#[test]
fn deleting_a_child_takes_it_out_of_its_layers_in_the_same_undo_step() {
    let mut document = Document::new(Project::new("Layers"));
    let bass = add_channel(&mut document, "Bass");
    let lead = add_channel(&mut document, "Lead");
    let top = add_channel(&mut document, "Layer");
    let outer = add_channel(&mut document, "Outer");
    set(&mut document, top, layer(&[bass, lead])).unwrap();
    set(&mut document, outer, layer(&[top, lead])).unwrap();
    let full = document.project().clone();
    let cursor = document.history().cursor;

    document
        .dispatch(Command::RemoveChannel { id: lead }, None)
        .unwrap();
    document.project().check().unwrap();
    assert!(document.project().channel(lead).is_none());
    assert_eq!(
        voice(&document, top)
            .layer
            .children
            .iter()
            .collect::<Vec<_>>(),
        [bass]
    );
    assert_eq!(
        voice(&document, outer)
            .layer
            .children
            .iter()
            .collect::<Vec<_>>(),
        [top]
    );
    assert_eq!(reload(&document), *document.project());

    document.undo().unwrap();
    assert_eq!(
        document.project(),
        &full,
        "one undo restores the child everywhere"
    );
    assert_eq!(document.history().cursor, cursor);
    document.redo().unwrap();
    assert!(document.project().channel(lead).is_none());
    document.project().check().unwrap();

    // Deleting a layer leaves its children alone.
    document
        .dispatch(Command::RemoveChannel { id: top }, None)
        .unwrap();
    document.project().check().unwrap();
    assert!(document.project().channel(bass).is_some());
    assert!(voice(&document, outer).layer.children.is_empty());
}

#[test]
fn a_duplicated_layer_keeps_its_children_and_a_new_layer_is_one_batch() {
    let mut document = Document::new(Project::new("Layers"));
    let bass = add_channel(&mut document, "Bass");
    let lead = add_channel(&mut document, "Lead");

    // What the rack's "Add layer channel" sends: the new channel's id is the
    // project's next id because no mixer track is made for it.
    let predicted = ChannelId(document.project().next_id);
    let before = document.history().cursor;
    let applied = document
        .dispatch(
            Command::Batch {
                label: Some("Add layer channel".into()),
                commands: vec![
                    Command::AddChannel {
                        name: Some("Layer".into()),
                        sample: None,
                        instrument: None,
                        index: None,
                        mixer_track: Some(TrackId::MASTER),
                    },
                    Command::SetChannelVoiceSettings {
                        id: predicted,
                        settings: layer(&[bass, lead]),
                    },
                ],
            },
            None,
        )
        .unwrap();
    assert_eq!(applied.created, [predicted.0]);
    assert_eq!(voice(&document, predicted), layer(&[bass, lead]));
    let tracks = document.project().mixer.tracks.len();

    let copy = ChannelId(
        document
            .dispatch(Command::DuplicateChannel { id: predicted }, None)
            .unwrap()
            .created[0],
    );
    assert_eq!(voice(&document, copy), layer(&[bass, lead]));
    assert_eq!(document.project().mixer.tracks.len(), tracks);
    document.project().check().unwrap();

    document.undo().unwrap();
    document.undo().unwrap();
    assert_eq!(
        document.history().cursor,
        before,
        "the layer was one undo step"
    );
    assert!(document.project().channel(predicted).is_none());
}

#[test]
fn damaged_layer_lists_in_a_file_are_refused() {
    let mut document = Document::new(Project::new("Layers"));
    let bass = add_channel(&mut document, "Bass");
    let top = add_channel(&mut document, "Layer");
    set(&mut document, top, layer(&[bass])).unwrap();
    let good = serde_json::to_value(document.project()).unwrap();
    assert!(file::from_json(&good.to_string()).is_ok());

    for children in [
        serde_json::json!([top.0]),
        serde_json::json!([4242]),
        serde_json::json!((0..17).collect::<Vec<u32>>()),
    ] {
        let mut bad = good.clone();
        bad["channels"][1]["voice"]["layer"]["children"] = children;
        assert!(file::from_json(&bad.to_string()).is_err());
    }
}
