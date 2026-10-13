//! Public instrument-to-sample acceptance; no device or vendor fixtures.
use windfall_engine::{Processor, preset_render as preset, render_reporting};
use windfall_ipc::{PlayMode, TransportPatch};
use windfall_project::*;

fn bytes(kind: InstrumentKind) -> Vec<u8> {
    Preset::of_plugin("Own preset", PluginPreset { tables: None,
        state: PluginPresetState::Instrument { params: kind.default_params(), asset: None },
    }).to_json().unwrap().into_bytes()
}
fn speech() -> Vec<u8> {
    let mut doc = Document::new(Project::new("Own speech"));
    let channel = ChannelId(doc.dispatch(Command::AddChannel { name: None, sample: None,
        instrument: Some(InstrumentKind::TextVoice), index: None, mixer_track: None }, None).unwrap().created[0]);
    doc.dispatch(Command::SetSampleInstrumentAsset { channel, expected: None,
        asset: Some(SampleInstrumentAsset::Speech { text: "Hello windfall".into(), pitch: 48.0,
            duration: 0.7, version: 1, source: None }) }, None).unwrap();
    let (body, asset) = ChannelPreset::capture(doc.project(), channel).unwrap();
    assert!(asset.is_none());
    Preset::of_channel("Spoken phrase", body, None).to_json().unwrap().into_bytes()
}
fn equality(bytes: &[u8], rate: u32) {
    let controls = preset::PresetRenderOptions { length_secs: 0.4, tail_secs: 0.5, ..Default::default() };
    let prepared = preset::prepare(bytes, controls, rate, &mut || true).unwrap();
    let pcm = preset::render(bytes, controls, rate, &mut || true).unwrap();
    assert!(pcm.samples().iter().any(|x| x.abs() > 1e-5));
    // Publish while detached; dropping an attached consumer before publication
    // deliberately refuses that stale stream epoch in the production protocol.
    let (processor, controller) = Processor::new(rate);
    controller.suspend_stream(|| drop(processor)).unwrap();
    controller.set_project(&prepared.project, &prepared.pool);
    let mut live = Processor::reopen(&controller, rate);
    controller.set_transport(TransportPatch { mode: Some(PlayMode::Song), loop_song: Some(false), ..Default::default() });
    controller.play();
    let latency = controller.latency_frames() as usize;
    let frames = pcm.frames();
    let mut output = vec![0.0; (latency + frames) * 2];
    for block in output.chunks_mut(127 * 2) { live.process(block); }
    assert_eq!(&output[latency * 2..], pcm.samples(), "live/offline note and complete release PCM must be exact after PDC");
    for block_frames in [7, 127, 1024] {
        let rendered = render_reporting(&prepared.project, &prepared.pool,
            &windfall_engine::RenderOptions { block_frames, ..prepared.options.clone() }, &mut |_| true);
        assert!(rendered.timeline_error.is_none());
        assert!(rendered.sampler_error.is_none());
        assert!(rendered.plugin_error.is_none());
        assert_eq!(rendered.audio.samples(), pcm.samples(), "note and release partition equality");
    }
}

#[test]
fn compact_and_subtractive_presets_equal_live_engine_pcm_at_project_rates() {
    for kind in [InstrumentKind::CompactSynth, InstrumentKind::SubtractiveSynth] {
        for rate in [44_100, 48_000, 96_000] { equality(&bytes(kind), rate); }
    }
}
#[test]
fn phoneme_and_text_speech_presets_equal_live_engine_pcm() {
    equality(&bytes(InstrumentKind::SpeechVoice), 48_000);
    equality(&speech(), 48_000);
}
#[test]
fn every_accepted_synth_kind_has_finite_deterministic_measured_pcm() {
    let mut kinds = std::collections::HashSet::new();
    for &kind in preset::SYNTH_KINDS {
        assert!(kinds.insert(kind), "duplicate capability {kind:?}");
        let source = bytes(kind);
        let controls = preset::PresetRenderOptions { length_secs: 0.1, tail_secs: 0.1, ..Default::default() };
        let pcm = preset::render(&source, controls, 24_000, &mut || true).unwrap();
        assert_eq!(pcm.sample_rate(), 24_000);
        assert_eq!(pcm.channels(), 2);
        assert!(pcm.frames() >= 2400);
        assert!(pcm.samples().iter().all(|x| x.is_finite()));
        assert!(pcm.samples().iter().any(|x| x.abs() > 1e-7), "{kind:?} produces real audio");
        assert_eq!(pcm.samples(), preset::render(&source, controls, 24_000, &mut || true).unwrap().samples());
    }
}
#[test]
fn mono_note_velocity_and_release_controls_measure_actual_pcm() {
    let source = bytes(InstrumentKind::CompactSynth);
    let controls = preset::PresetRenderOptions { length_secs: 0.25, tail_secs: 0.2, ..Default::default() };
    let stereo = preset::render(&source, controls, 48_000, &mut || true).unwrap();
    let mono = preset::render(&source, preset::PresetRenderOptions { stereo: false, ..controls }, 48_000, &mut || true).unwrap();
    assert_eq!(mono.channels(), 1);
    assert_eq!(mono.frames(), stereo.frames());
    assert_eq!(mono.samples(), stereo.samples().chunks_exact(2).map(|s| (s[0] + s[1]) * 0.5).collect::<Vec<_>>());
    assert!(stereo.samples()[24000..].iter().any(|s| s.abs() > 1e-7), "release tail must ring beyond note-off");
    let changed = preset::render(&source, preset::PresetRenderOptions { note: 72, velocity: 0.3, ..controls }, 48_000, &mut || true).unwrap();
    assert_ne!(changed.samples(), stereo.samples());
    let silent = preset::render(&source, preset::PresetRenderOptions { velocity: 0.0, ..controls }, 48_000, &mut || true).unwrap();
    assert!(silent.samples().iter().all(|s| *s == 0.0));
}
#[test]
fn cancelled_hostile_oversized_and_unsupported_presets_never_return_partial_pcm() {
    let source = bytes(InstrumentKind::CompactSynth);
    let mut checkpoints = 0;
    assert!(preset::render(&source, Default::default(), 48_000, &mut || {
        checkpoints += 1; checkpoints < 5
    }).unwrap_err().contains("cancelled"));
    for hostile in [vec![b' '; preset::MAX_PRESET_BYTES + 1], b"{broken".to_vec(), bytes(InstrumentKind::MidiOut)] {
        assert!(preset::render(&hostile, Default::default(), 48_000, &mut || true).is_err());
    }
    for controls in [
        preset::PresetRenderOptions { length_secs: f32::NAN, ..Default::default() },
        preset::PresetRenderOptions { tail_secs: f32::INFINITY, ..Default::default() },
        preset::PresetRenderOptions { note: 128, ..Default::default() },
        preset::PresetRenderOptions { length_secs: 60.0, tail_secs: 30.0, ..Default::default() },
    ] { assert!(preset::render(&source, controls, 192_000, &mut || true).is_err()); }
    let mut empty = InstrumentKind::SpeechVoice.default_params();
    if let windfall_dsp::InstrumentParams::SpeechVoice(voice) = &mut empty { voice.phrase.length = 0; }
    let preset = Preset::of_plugin("Empty phrase", PluginPreset { tables: None,
        state: PluginPresetState::Instrument { params: empty, asset: None } }).to_json().unwrap();
    assert!(preset::render(preset.as_bytes(), Default::default(), 48_000, &mut || true).unwrap_err().contains("empty"));
    let speech = String::from_utf8(speech()).unwrap().replace("Hello windfall", "  ");
    assert!(preset::render(speech.as_bytes(), Default::default(), 48_000, &mut || true).unwrap_err().contains("empty"));
}
