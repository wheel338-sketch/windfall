//! From FL Studio's own instruments to Windfall's.
//!
//! The three-oscillator synth FL Studio writes as "3x Osc" becomes
//! Windfall's subtractive synth, with its numeric settings carried over.
//! The names in `DEFAULT_INSTRUMENTS` select registered instruments with
//! sanitized defaults; their patch settings are not decoded. Other names
//! remain silent placeholders. The mappings and limits are documented in
//! `docs/integration/seams/flp-instruments.md`.
//!
//! The order of the 3x Osc's numbers comes from DawVert
//! `data_main/datadef/fl_studio.ddef`, and what they mean from DawVert
//! `plugins/plugconv/lmms__n_flstudio.py`, which turns the same state into
//! LMMS's triple oscillator. The states of three projects saved by FL
//! Studio 9 fit: 92 bytes, with the two mix levels where a project that
//! "turns oscillator 3 down" has a zero.

use windfall_dsp::{InstrumentKind, InstrumentParams, ParamSet, SynthParams, VoiceMode, Waveform};

use crate::model::{Channel, EnvelopeLfo};
use crate::plugin::Numbers;
use crate::units::envelope_ms;

/// The internal name of the instrument whose state this module decodes.
const THREE_OSC: &str = "3x osc";

const PATCH_SETTINGS_NOT_DECODED: &str =
    "its patch settings were not decoded, so it starts from Windfall's defaults";

/// Import keys only: these are FL Studio's internal names, not Windfall names.
/// Each kind is registered and is the stand-in documented in the DSP seams.
const DEFAULT_INSTRUMENTS: &[(&str, InstrumentKind)] = &[
    ("Fruity DX10", InstrumentKind::FourOp),
    ("Sytrus", InstrumentKind::MatrixFm),
    ("Toxic Biohazard", InstrumentKind::RingHybrid),
    ("Harmless", InstrumentKind::HarmonicFilter),
    ("Morphine", InstrumentKind::SnapshotMorph),
    ("Ogun", InstrumentKind::MetalVoice),
    ("Harmor", InstrumentKind::SpectralResynth),
    ("Autogun", InstrumentKind::NumberVoice),
    ("BeepMap", InstrumentKind::ColorScan),
    ("Plucked!", InstrumentKind::Pluck),
    ("BooBass", InstrumentKind::FingerBass),
    ("Sakura", InstrumentKind::AcousticString),
    ("Fruity Kick", InstrumentKind::Kick),
    ("BassDrum", InstrumentKind::LayerKick),
    ("Drumaxx", InstrumentKind::DrumRack),
    ("Drumpad", InstrumentKind::Membrane),
    ("Fruity DrumSynth Live", InstrumentKind::DrumVoice),
    ("Fruity Slicer", InstrumentKind::SliceMap),
    ("Slicex", InstrumentKind::SliceDeck),
    ("Fruity Granulizer", InstrumentKind::GrainCloud),
    ("Wave Traveller", InstrumentKind::WaveRide),
    ("Transistor Bass", InstrumentKind::AcidLine),
    ("SimSynth", InstrumentKind::TripleOsc),
    ("Poizone", InstrumentKind::TripleOsc),
    ("Sawer", InstrumentKind::WaveLane),
    ("FLEX", InstrumentKind::MacroVoice),
    ("GMS", InstrumentKind::MacroVoice),
    ("Speech Synthesizer", InstrumentKind::SpeechVoice),
];

/// An FL Studio instrument as a Windfall instrument.
#[derive(Debug, Clone)]
pub(crate) struct Translated {
    pub(crate) params: InstrumentParams,
    /// What did not carry over, in words that end a sentence about the
    /// instrument.
    pub(crate) notes: Vec<&'static str>,
}

/// Translates the instrument on a channel, or `None` when Windfall has
/// nothing that stands for it.
pub(crate) fn translate(channel: &Channel) -> Option<Translated> {
    let plugin = channel.plugin.as_ref()?;
    let name = plugin.internal_name.trim();
    if name.eq_ignore_ascii_case(THREE_OSC) {
        return Some(three_osc(Numbers(&plugin.state), channel));
    }
    let (_, kind) = DEFAULT_INSTRUMENTS
        .iter()
        .find(|(source, _)| source.eq_ignore_ascii_case(name))?;
    Some(Translated {
        params: kind.default_params().sanitized(),
        notes: vec![PATCH_SETTINGS_NOT_DECODED],
    })
}

/// FL Studio's oscillator shapes, in the order of the numbers it stores:
/// sine, triangle, square, saw, rounded saw, noise, and a sample the user
/// loaded.
fn waveform(shape: i32) -> (Waveform, bool) {
    match shape {
        0 => (Waveform::Sine, true),
        1 => (Waveform::Triangle, true),
        2 => (Waveform::Square, true),
        3 => (Waveform::Saw, true),
        5 => (Waveform::WhiteNoise, true),
        // The rounded saw and the user's own wave have no twin.
        _ => (Waveform::Saw, false),
    }
}

fn three_osc(numbers: Numbers<'_>, channel: &Channel) -> Translated {
    let mut params = SynthParams::default();
    let mut notes = vec![
        "it is another synth, so it will not sound the same: the shape, tuning, level and pan of the three oscillators were carried over",
    ];
    let mut odd_shape = false;
    let mut detuned = false;
    // Oscillators 1 and 2 have seven numbers each, after the version, and
    // oscillator 3 has six: it has no mix level of its own.
    for (index, oscillator) in params.oscillators.iter_mut().enumerate() {
        let field = |offset: usize| numbers.get(1 + index * 7 + offset);
        if let Some(pan) = field(0) {
            oscillator.pan = pan as f32 / 64.0;
        }
        if let Some(shape) = field(1) {
            let (waveform, twin) = waveform(shape);
            oscillator.waveform = waveform;
            odd_shape |= !twin;
        }
        if let Some(coarse) = field(2) {
            oscillator.coarse = coarse.clamp(-36, 36);
        }
        if let Some(fine) = field(3) {
            oscillator.fine_cents = fine as f32;
        }
        detuned |= field(5).is_some_and(|detune| detune != 0);
    }
    // Two knobs share the level among three oscillators: the first takes
    // its share for oscillator 2 from oscillator 1, and the second takes
    // its share for oscillator 3 from both.
    let mix = |index: usize| {
        numbers
            .get(index)
            .map(|raw| (f64::from(raw) / 128.0).clamp(0.0, 1.0))
    };
    if let (Some(second), Some(third)) = (mix(7), mix(14)) {
        params.oscillators[0].level = ((1.0 - second) * (1.0 - third)) as f32;
        params.oscillators[1].level = (second * (1.0 - third)) as f32;
        params.oscillators[2].level = third as f32;
    }
    if odd_shape {
        notes.push("an oscillator shape Windfall does not have plays as a saw");
    }
    if detuned {
        notes.push("the stereo detune of its oscillators was not carried over");
    }
    if numbers.byte(91).is_some_and(|on| on != 0) {
        notes.push("oscillator 3 no longer modulates the level of the others");
    }

    match channel
        .volume_envelope()
        .filter(|envelope| envelope.enabled)
    {
        Some(envelope) => {
            amp_envelope(&mut params, envelope);
            notes.push("its volume envelope was converted by a rule of thumb");
        }
        // Without an envelope FL Studio holds a note at full level for as
        // long as it lasts.
        None => {
            params.amp_envelope.attack_ms = 0.0;
            params.amp_envelope.sustain = 1.0;
            params.amp_envelope.release_ms = 20.0;
        }
    }
    if let Some(polyphony) = channel.polyphony {
        if polyphony.flags & 1 != 0 {
            params.voice_mode = VoiceMode::Mono;
        }
        if let Ok(max @ 1..=32) = u8::try_from(polyphony.max) {
            params.polyphony = max;
        }
    }
    Translated {
        params: InstrumentParams::SubtractiveSynth(params.sanitized()),
        notes,
    }
}

fn amp_envelope(params: &mut SynthParams, envelope: &EnvelopeLfo) {
    params.amp_envelope.attack_ms = envelope_ms(envelope.attack);
    params.amp_envelope.decay_ms = envelope_ms(envelope.decay);
    params.amp_envelope.sustain = (envelope.sustain.min(128) as f32) / 128.0;
    params.amp_envelope.release_ms = envelope_ms(envelope.release);
}

#[cfg(test)]
mod tests {
    use crate::model::{Plugin, Polyphony};

    use super::*;

    /// The state of a three-oscillator synth from each oscillator's pan,
    /// shape, coarse and fine tuning, the two mix levels, and the last
    /// four bytes.
    fn state(oscillators: [(i32, i32, i32, i32); 3], mix: (i32, i32), flags: [u8; 4]) -> Vec<u8> {
        let mut values = vec![12];
        for (index, (pan, shape, coarse, fine)) in oscillators.into_iter().enumerate() {
            values.extend([pan, shape, coarse, fine, 0, 0]);
            match index {
                0 => values.push(mix.0),
                1 => values.push(mix.1),
                _ => {}
            }
        }
        values.push(0);
        let mut bytes: Vec<u8> = values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        bytes.extend(flags);
        bytes
    }

    fn channel(state: Vec<u8>) -> Channel {
        Channel {
            plugin: Some(Plugin {
                internal_name: "3x Osc".to_owned(),
                generator: Some(true),
                state,
            }),
            ..Channel::default()
        }
    }

    fn synth(channel: &Channel) -> (SynthParams, Vec<&'static str>) {
        let translated = translate(channel).expect("a synth");
        let params = match translated.params {
            InstrumentParams::SubtractiveSynth(params) => params,
            _ => panic!("expected the subtractive synth"),
        };
        (params, translated.notes)
    }

    #[test]
    fn a_state_is_92_bytes_as_fl_studio_writes_it() {
        assert_eq!(state([(0, 0, 0, 0); 3], (64, 32), [0; 4]).len(), 92);
    }

    #[test]
    fn the_oscillators_carry_shape_tuning_pan_and_level() {
        // The default patch as FL Studio 9 saves it has mix levels of 64
        // and 32, and its second oscillator an octave down.
        let (params, notes) = synth(&channel(state(
            [(0, 0, 0, 0), (-64, 2, -12, 50), (32, 3, 24, -100)],
            (64, 32),
            [0; 4],
        )));
        let [first, second, third] = params.oscillators;
        assert_eq!(first.waveform, Waveform::Sine);
        assert_eq!(second.waveform, Waveform::Square);
        assert_eq!(third.waveform, Waveform::Saw);
        assert_eq!(
            (second.coarse, second.fine_cents, second.pan),
            (-12, 50.0, -1.0)
        );
        assert_eq!(
            (third.coarse, third.fine_cents, third.pan),
            (24, -100.0, 0.5)
        );
        assert_eq!(
            (first.level, second.level, third.level),
            (0.375, 0.375, 0.25)
        );
        assert_eq!(notes.len(), 1);
    }

    #[test]
    fn turning_the_second_mix_knob_down_silences_oscillator_3() {
        let (params, _) = synth(&channel(state([(0, 0, 0, 0); 3], (64, 0), [0; 4])));
        assert_eq!(params.oscillators[2].level, 0.0);
        assert_eq!(params.oscillators[0].level, 0.5);
        assert_eq!(params.oscillators[1].level, 0.5);
    }

    #[test]
    fn shapes_without_a_twin_play_as_a_saw_and_say_so() {
        let (params, notes) = synth(&channel(state(
            [(0, 4, 0, 0), (0, 5, 0, 0), (0, 6, 0, 0)],
            (64, 32),
            [0, 0, 0, 1],
        )));
        assert_eq!(params.oscillators[0].waveform, Waveform::Saw);
        assert_eq!(params.oscillators[1].waveform, Waveform::WhiteNoise);
        assert_eq!(params.oscillators[2].waveform, Waveform::Saw);
        assert!(notes.iter().any(|note| note.contains("plays as a saw")));
        assert!(
            notes
                .iter()
                .any(|note| note.contains("oscillator 3 no longer modulates"))
        );
    }

    #[test]
    fn without_an_envelope_a_note_holds_at_full_level() {
        let (params, _) = synth(&channel(state([(0, 0, 0, 0); 3], (64, 32), [0; 4])));
        assert_eq!(params.amp_envelope.sustain, 1.0);
        assert_eq!(params.amp_envelope.attack_ms, 0.0);
    }

    #[test]
    fn the_channel_envelope_and_polyphony_shape_the_synth() {
        let mut channel = channel(state([(0, 0, 0, 0); 3], (64, 32), [0; 4]));
        channel.envelopes = vec![
            EnvelopeLfo::default(),
            EnvelopeLfo {
                enabled: true,
                attack: 20_000,
                decay: 30_000,
                sustain: 64,
                release: 20_000,
                ..EnvelopeLfo::default()
            },
        ];
        channel.polyphony = Some(Polyphony {
            max: 4,
            slide: 0,
            flags: 1,
        });
        let (params, notes) = synth(&channel);
        assert_eq!(params.amp_envelope.sustain, 0.5);
        assert!(params.amp_envelope.attack_ms > 0.0);
        assert!(params.amp_envelope.decay_ms > params.amp_envelope.attack_ms);
        assert_eq!(params.voice_mode, VoiceMode::Mono);
        assert_eq!(params.polyphony, 4);
        assert!(notes.iter().any(|note| note.contains("rule of thumb")));
    }

    #[test]
    fn a_damaged_state_gives_a_synth_at_its_defaults() {
        let (params, _) = synth(&channel(vec![1, 2, 3]));
        assert_eq!(params.oscillators, SynthParams::default().oscillators);
        let (params, _) = synth(&channel(state(
            [(i32::MAX, i32::MIN, i32::MAX, i32::MIN); 3],
            (i32::MAX, i32::MIN),
            [255; 4],
        )));
        assert_eq!(
            InstrumentParams::SubtractiveSynth(params),
            InstrumentParams::SubtractiveSynth(params.sanitized())
        );
    }

    #[test]
    fn registered_stand_ins_use_sanitized_defaults_without_decoding_patches() {
        let expected = [
            ("Fruity DX10", InstrumentKind::FourOp),
            ("Sytrus", InstrumentKind::MatrixFm),
            ("Toxic Biohazard", InstrumentKind::RingHybrid),
            ("Harmless", InstrumentKind::HarmonicFilter),
            ("Morphine", InstrumentKind::SnapshotMorph),
            ("Ogun", InstrumentKind::MetalVoice),
            ("Harmor", InstrumentKind::SpectralResynth),
            ("Autogun", InstrumentKind::NumberVoice),
            ("BeepMap", InstrumentKind::ColorScan),
            ("Plucked!", InstrumentKind::Pluck),
            ("BooBass", InstrumentKind::FingerBass),
            ("Sakura", InstrumentKind::AcousticString),
            ("Fruity Kick", InstrumentKind::Kick),
            ("BassDrum", InstrumentKind::LayerKick),
            ("Drumaxx", InstrumentKind::DrumRack),
            ("Drumpad", InstrumentKind::Membrane),
            ("Fruity DrumSynth Live", InstrumentKind::DrumVoice),
            ("Fruity Slicer", InstrumentKind::SliceMap),
            ("Slicex", InstrumentKind::SliceDeck),
            ("Fruity Granulizer", InstrumentKind::GrainCloud),
            ("Wave Traveller", InstrumentKind::WaveRide),
            ("Transistor Bass", InstrumentKind::AcidLine),
            ("SimSynth", InstrumentKind::TripleOsc),
            ("Poizone", InstrumentKind::TripleOsc),
            ("Sawer", InstrumentKind::WaveLane),
            ("FLEX", InstrumentKind::MacroVoice),
            ("GMS", InstrumentKind::MacroVoice),
            ("Speech Synthesizer", InstrumentKind::SpeechVoice),
        ];
        for (name, kind) in expected {
            for internal_name in [
                name.to_owned(),
                format!("  {}\t", name.to_ascii_uppercase()),
            ] {
                for state in [Vec::new(), vec![255; 92]] {
                    let mut channel = channel(state);
                    channel.plugin.as_mut().unwrap().internal_name = internal_name.clone();
                    let translated = translate(&channel).expect(name);
                    assert_eq!(translated.params.kind(), kind, "{name}");
                    assert_eq!(
                        translated.params,
                        kind.default_params().sanitized(),
                        "{name}"
                    );
                    assert_eq!(translated.notes, [PATCH_SETTINGS_NOT_DECODED], "{name}");
                }
            }
        }
    }

    #[test]
    fn three_osc_still_decodes_with_case_and_whitespace_variations() {
        let mut channel = channel(state([(0, 2, -12, 50); 3], (64, 0), [0; 4]));
        channel.plugin.as_mut().unwrap().internal_name = "  3X oSC\t".to_owned();
        let (params, _) = synth(&channel);
        assert_eq!(params.oscillators[0].waveform, Waveform::Square);
        assert_eq!(params.oscillators[0].coarse, -12);
        assert_eq!(params.oscillators[0].fine_cents, 50.0);
        assert_eq!(params.oscillators[2].level, 0.0);
    }

    #[test]
    fn names_without_a_documented_registered_stand_in_remain_placeholders() {
        for name in [
            "DirectWave",
            "SoundFont",
            "Fruity Soundfont Player",
            "FL Keys",
            "FPC",
            "Kepler",
            "MiniSynth",
            "Fruity Slicer 2",
        ] {
            let mut channel = channel(Vec::new());
            channel.plugin.as_mut().unwrap().internal_name = name.to_owned();
            assert!(translate(&channel).is_none(), "{name}");
        }
    }

    #[test]
    fn other_instruments_have_no_equivalent() {
        let mut other = channel(Vec::new());
        if let Some(plugin) = &mut other.plugin {
            plugin.internal_name = "Some Other Synth".to_owned();
        }
        assert!(translate(&other).is_none());
        assert!(translate(&Channel::default()).is_none());
    }
}
