//! The parameter structs and their descriptions must agree with each
//! other, with their serde names and with their documented ranges.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use windfall_dsp::blocks::noise::Rng;
use windfall_dsp::{
    AnyEffect, AnyInstrument, BalanceParams, ChannelMuteParams, ChorusParams, CompressorParams,
    DcBlockParams, DelayMode, DelayParams, DetectorMode, DistortionParams, EffectKind,
    EffectParams, EqParams, FlangerParams, InstrumentKind, InstrumentParams, LimiterParams,
    NoteDivision, ParamInfo, ParamKind, ParamScale, ParamSet, ParamUnit, PhaserParams,
    PolarityParams, ReverbParams, SoftClipperParams, StereoMatrixParams, SynthParams, Waveform,
};

use crate::support::random_params;

/// The value at a dotted path such as `"oscillators.0.level"`.
fn at_path<'a>(root: &'a Value, path: &str) -> &'a Value {
    path.split('.').fold(root, |value, part| {
        let next = match part.parse::<usize>() {
            Ok(index) => value.get(index),
            Err(_) => value.get(part),
        };
        next.unwrap_or_else(|| panic!("nothing at `{part}` of `{path}`"))
    })
}

/// What a control's number looks like in JSON.
fn expected_json(info: &ParamInfo, value: f32) -> Value {
    match info.kind {
        ParamKind::Float => json!(value),
        ParamKind::Integer => json!(value as i64),
        ParamKind::Toggle => json!(value >= 0.5),
        ParamKind::Choice => json!(info.choices[value as usize].value),
    }
}

fn assert_json_matches(info: &ParamInfo, json: &Value, value: f32) {
    let found = at_path(json, info.id);
    let expected = expected_json(info, value);
    let same = match (found.as_f64(), expected.as_f64()) {
        (Some(found), Some(expected)) => {
            (found - expected).abs() <= 1.0e-6 * expected.abs().max(1.0)
        }
        _ => *found == expected,
    };
    assert!(
        same,
        "`{}` is {found} in JSON, expected {expected}",
        info.id
    );
}

/// Every check that holds for any parameter struct.
fn check<P: ParamSet + Serialize + DeserializeOwned>() {
    let infos = P::descriptors();
    let name = P::NAME;
    assert!(!infos.is_empty(), "{name} has no controls");

    // The table is well formed.
    for (index, info) in infos.iter().enumerate() {
        let id = info.id;
        assert!(!info.name.is_empty() && !id.is_empty());
        assert!(info.min < info.max, "{name} `{id}`: empty range");
        assert!(
            (info.min..=info.max).contains(&info.default),
            "{name} `{id}`: default outside its range"
        );
        assert_eq!(P::index_of(id), Some(index), "{name} `{id}`: duplicate id");
        match info.kind {
            ParamKind::Choice => {
                assert_eq!(info.choices.len() as f32, info.max + 1.0);
                assert_eq!(info.min, 0.0);
                for choice in info.choices {
                    assert!(!choice.value.is_empty() && !choice.label.is_empty());
                }
            }
            ParamKind::Toggle => assert_eq!((info.min, info.max), (0.0, 1.0)),
            ParamKind::Float | ParamKind::Integer => assert!(info.choices.is_empty()),
        }
        if info.kind != ParamKind::Float {
            assert_eq!(info.scale, ParamScale::Linear);
        }
        if info.scale == ParamScale::Logarithmic {
            assert!(
                info.min > 0.0,
                "{name} `{id}`: a log scale cannot reach zero"
            );
        }
        if matches!(info.kind, ParamKind::Toggle | ParamKind::Choice) {
            assert_eq!(info.unit, ParamUnit::None);
        }
    }
    assert_eq!(P::index_of("no such control"), None);

    // The table's defaults are the struct's defaults, and every id is where
    // the value really is in the JSON.
    let defaults = P::default();
    let json = serde_json::to_value(defaults).unwrap();
    for (index, info) in infos.iter().enumerate() {
        let value = defaults.get(index).unwrap();
        assert!(
            (value - info.default).abs() <= 1.0e-6,
            "{name} `{}`: the table says {} but the default is {value}",
            info.id,
            info.default
        );
        assert_json_matches(info, &json, value);
    }
    assert_eq!(defaults.get(infos.len()), None);
    assert_eq!(
        defaults.sanitized(),
        defaults,
        "{name}: defaults are not clean"
    );

    // Every control can be set across its range, lands in the JSON under
    // its id, and survives a round trip through JSON.
    for (index, info) in infos.iter().enumerate() {
        let steps = match info.kind {
            ParamKind::Choice | ParamKind::Integer => (info.max - info.min) as usize,
            ParamKind::Toggle => 1,
            ParamKind::Float => 4,
        };
        for step in 0..=steps {
            let value = info.min + (info.max - info.min) * step as f32 / steps as f32;
            let mut params = P::default();
            assert!(params.set(index, value));
            let read = params.get(index).unwrap();
            assert!(
                (read - value).abs() <= 1.0e-5 * value.abs().max(1.0),
                "{name} `{}`: set {value}, read {read}",
                info.id
            );
            let json = serde_json::to_value(params).unwrap();
            assert_json_matches(info, &json, read);
            let back: P = serde_json::from_value(json).unwrap();
            assert_eq!(back, params);
            assert_eq!(params.sanitized(), params);
        }

        // Values out of range are brought in. Values that are not numbers
        // fall back to the default, or leave a choice where it was.
        let mut params = P::default();
        params.set(index, info.max + 1.0e6);
        assert_eq!(params.get(index), Some(info.max), "{name} `{}`", info.id);
        params.set(index, info.min - 1.0e6);
        assert_eq!(params.get(index), Some(info.min), "{name} `{}`", info.id);
        params.set(index, f32::NAN);
        let after = params.get(index).unwrap();
        assert!((info.min..=info.max).contains(&after));
        params.set(index, f32::INFINITY);
        assert!(params.get(index).unwrap().is_finite());
    }
    let mut params = P::default();
    assert!(!params.set(infos.len(), 1.0));
    assert_eq!(params, P::default());

    // An empty object is the defaults, so files written before a field
    // existed still load.
    let empty: P = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty, P::default());

    // Approaching a target arrives exactly and reports it.
    let mut rng = Rng::new(42);
    for _ in 0..20 {
        let target: P = random_params(&mut rng);
        assert_eq!(target.sanitized(), target);
        let mut current: P = random_params(&mut rng);
        let mut steps = 0;
        while current.approach(&target, 0.2) {
            steps += 1;
            assert!(steps < 500, "{name}: approach never arrives");
            for (index, info) in infos.iter().enumerate() {
                let value = current.get(index).unwrap();
                assert!((info.min..=info.max).contains(&value));
            }
        }
        assert_eq!(current, target);
        assert!(!current.approach(&target, 0.2));
    }
}

#[test]
fn every_parameter_struct_agrees_with_its_description() {
    check::<windfall_dsp::LofiParams>();
    check::<EqParams>();
    check::<CompressorParams>();
    check::<LimiterParams>();
    check::<ReverbParams>();
    check::<DelayParams>();
    check::<BalanceParams>();
    check::<DcBlockParams>();
    check::<ChannelMuteParams>();
    check::<PolarityParams>();
    check::<StereoMatrixParams>();
    check::<SoftClipperParams>();
    check::<DistortionParams>();
    check::<ChorusParams>();
    check::<FlangerParams>();
    check::<PhaserParams>();
    check::<SynthParams>();
    check::<windfall_dsp::GrooveSynthParams>();
    check::<windfall_dsp::PocketSynthParams>();
    check::<windfall_dsp::VintageSawParams>();
    check::<windfall_dsp::CompactSynthParams>();
    check::<windfall_dsp::DualPolyParams>();
    check::<windfall_dsp::VintagePolyParams>();
}

#[test]
fn sanitizing_replaces_everything_that_is_not_a_number() {
    let compressor = CompressorParams {
        threshold_db: f32::NAN,
        ratio: f32::INFINITY,
        attack_ms: -5.0,
        release_ms: f32::NEG_INFINITY,
        knee_db: 1.0e9,
        makeup_db: f32::NAN,
        mix: 7.0,
        ..CompressorParams::default()
    }
    .sanitized();
    assert_eq!(compressor.threshold_db, -18.0);
    assert_eq!(compressor.ratio, 4.0);
    assert_eq!(compressor.attack_ms, 0.05);
    assert_eq!(compressor.release_ms, 120.0);
    assert_eq!(compressor.knee_db, 24.0);
    assert_eq!(compressor.makeup_db, 0.0);
    assert_eq!(compressor.mix, 1.0);

    let mut eq = EqParams::default();
    eq.peak2.frequency_hz = f32::NAN;
    eq.peak2.q = 0.0;
    eq.low_shelf.gain_db = 300.0;
    eq.high_cut.frequency_hz = -1.0;
    eq.output_gain_db = f32::INFINITY;
    let eq = eq.sanitized();
    assert_eq!(eq.peak2.frequency_hz, 1_000.0);
    assert_eq!(eq.peak2.q, 0.1);
    assert_eq!(eq.low_shelf.gain_db, 24.0);
    assert_eq!(eq.high_cut.frequency_hz, 20.0);
    assert_eq!(eq.output_gain_db, 0.0);

    let limiter = LimiterParams {
        ceiling_db: 12.0,
        input_gain_db: f32::NAN,
        release_ms: 0.0,
        lookahead_ms: 1.0e6,
        ..LimiterParams::default()
    }
    .sanitized();
    assert_eq!(limiter.ceiling_db, 0.0);
    assert_eq!(limiter.input_gain_db, 0.0);
    assert_eq!(limiter.release_ms, 1.0);
    assert_eq!(limiter.lookahead_ms, 20.0);

    let reverb = ReverbParams {
        decay_s: f32::NAN,
        size: 2.0,
        mix: -1.0,
        low_cut_hz: 0.0,
        ..ReverbParams::default()
    }
    .sanitized();
    assert_eq!(reverb.decay_s, 1.8);
    assert_eq!(reverb.size, 1.0);
    assert_eq!(reverb.mix, 0.0);
    assert_eq!(reverb.low_cut_hz, 20.0);

    let delay = DelayParams {
        feedback: 5.0,
        time_ms: f32::NAN,
        stereo_offset_ms: 1.0e4,
        ..DelayParams::default()
    }
    .sanitized();
    assert_eq!(delay.feedback, 0.95);
    assert_eq!(delay.time_ms, 250.0);
    assert_eq!(delay.stereo_offset_ms, 50.0);

    let mut synth = SynthParams::default();
    synth.oscillators[1].coarse = 900;
    synth.oscillators[2].level = f32::NAN;
    synth.unison_voices = 200;
    synth.polyphony = 0;
    synth.filter.cutoff_hz = f32::NAN;
    synth.gain = 50.0;
    synth.lfos[1].rate_hz = 0.0;
    let synth = synth.sanitized();
    assert_eq!(synth.oscillators[1].coarse, 36);
    assert_eq!(synth.oscillators[2].level, 0.0);
    assert_eq!(synth.unison_voices, 7);
    assert_eq!(synth.polyphony, 1);
    assert_eq!(synth.filter.cutoff_hz, 20_000.0);
    assert_eq!(synth.gain, 2.0);
    assert_eq!(synth.lfos[1].rate_hz, 0.01);
}

#[test]
fn effect_params_carry_their_kind_as_a_tag() {
    for kind in EffectKind::ALL {
        let params = kind.default_params();
        assert_eq!(params.kind(), kind);
        assert_eq!(params.sanitized(), params);
        let json = serde_json::to_value(params).unwrap();
        let tag = serde_json::to_value(kind).unwrap();
        assert_eq!(json["type"], tag);
        let back: EffectParams = serde_json::from_value(json).unwrap();
        assert_eq!(back, params);
        // Only the tag is needed; the rest falls back to the defaults.
        let bare: EffectParams = serde_json::from_value(json!({ "type": tag })).unwrap();
        assert_eq!(bare, params);

        assert_eq!(kind.descriptors().len(), {
            let mut count = 0;
            while params.get(count).is_some() {
                count += 1;
            }
            count
        });
        assert!(!kind.name().is_empty());
        assert_eq!(AnyEffect::new(&params).kind(), kind);
    }
    let json = json!({ "type": "compressor", "thresholdDb": -30.0, "detector": "rms" });
    let params: EffectParams = serde_json::from_value(json).unwrap();
    let EffectParams::Compressor(compressor) = params else {
        panic!("wrong kind");
    };
    assert_eq!(compressor.threshold_db, -30.0);
    assert_eq!(compressor.detector, DetectorMode::Rms);
    assert_eq!(compressor.ratio, 4.0);
}

#[test]
fn effect_params_address_controls_by_index() {
    let mut params = EffectKind::Delay.default_params();
    let feedback = DelayParams::index_of("feedback").unwrap();
    let mode = DelayParams::index_of("mode").unwrap();
    let division = DelayParams::index_of("division").unwrap();
    assert!(params.set(feedback, 0.5));
    assert!(params.set(mode, 1.0));
    assert!(params.set(division, 5.0));
    assert!(!params.set(999, 0.0));
    let EffectParams::Delay(delay) = params else {
        panic!("wrong kind");
    };
    assert_eq!(delay.feedback, 0.5);
    assert_eq!(delay.mode, DelayMode::PingPong);
    assert_eq!(delay.division, NoteDivision::Quarter);
    assert_eq!(params.get(feedback), Some(0.5));
}

#[test]
fn instrument_params_carry_their_kind_as_a_tag() {
    for kind in InstrumentKind::ALL {
        let params = kind.default_params();
        assert_eq!(params.kind(), kind);
        let json = serde_json::to_value(params).unwrap();
        assert_eq!(json["type"], serde_json::to_value(kind).unwrap());
        let back: InstrumentParams = serde_json::from_value(json).unwrap();
        assert_eq!(back, params);
        assert!(!kind.name().is_empty() && !kind.descriptors().is_empty());
        assert_eq!(AnyInstrument::new(&params).kind(), kind);
    }
    let mut params = InstrumentKind::SubtractiveSynth.default_params();
    let waveform = SynthParams::index_of("oscillators.1.waveform").unwrap();
    assert!(params.set(waveform, 4.0));
    let synth = match params {
        InstrumentParams::SubtractiveSynth(synth) => synth,
        _ => panic!("expected the subtractive synth"),
    };
    assert_eq!(synth.oscillators[1].waveform, Waveform::Pulse);
    assert_eq!(params.sanitized(), params);
}

#[test]
fn names_are_plain_descriptions() {
    let names: Vec<&str> = EffectKind::ALL
        .iter()
        .map(|kind| kind.name())
        .chain(InstrumentKind::ALL.iter().map(|kind| kind.name()))
        .collect();
    assert_eq!(
        names,
        [
            "Parametric EQ",
            "Compressor",
            "Limiter",
            "Reverb",
            "Delay",
            "Balance",
            "DC blocker",
            "Channel mute",
            "Polarity",
            "Stereo matrix",
            "Soft clipper",
            "Drive distortion",
            "Fast lowpass",
            "Selectable filter",
            "Bass shelf",
            "Lo-fi reduction",
            "Chorus",
            "Flanger",
            "Phaser",
            "Band Split",
            "Multiband Compressor",
            "Multiband Maximizer",
            "Transient Shaper",
            "Transient Split",
            "One Knob",
            "Bass Harmonics",
            "Exciter",
            "Waveshaper",
            "Overdrive",
            "Guitar Rack",
            "Drive Chain",
            "Vintage Chorus",
            "Hyper Chorus",
            "Vintage Phaser",
            "Stacked Flanger",
            "Band Delay",
            "Room",
            "Spreader",
            "Stereo Enhancer",
            "Volume Gate",
            "Time Transport",
            "Scratch",
            "Performance Rack",
            "Convolver",
            "Frequency Shifter",
            "Pitch Shift",
            "Pitch Correct",
            "Vocoder",
            "Echo bank",
            "Frequency delay",
            "Seven Band",
            "Morph EQ",
            "Filter Bank",
            "XY Pad",
            "XYZ Pad",
            "Pan Motion",
            "Envelope Follower",
            "Note Envelope",
            "Lush Space",
            "Tuner",
            "Stage Stack",
            "Control Surface",
            "Send Tap",
            "Subtractive synth",
            "Four Operator",
            "Matrix FM",
            "Ring Hybrid",
            "Harmonic stack",
            "Partial morph",
            "Inharmonic",
            "Resynth",
            "Seed patch",
            "Scan synth",
            "Pluck",
            "Finger Bass",
            "Acoustic String",
            "Membrane",
            "Drum Rack",
            "Kick",
            "Drum Voice",
            "Slice Map",
            "Slice Deck",
            "Grain Cloud",
            "Wave Ride",
            "Sequenced resonant bass",
            "Three oscillator synth",
            "Wavetable synth",
            "Macro synth",
            "Speech Voice",
            "MIDI output",
            "Vintage poly",
            "Dual vintage poly",
            "Compact synth",
            "Vintage saw",
            "Pocket synth",
            "Groove synth",
        ]
    );
}
