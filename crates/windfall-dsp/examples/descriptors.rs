//! Prints what the UI needs to know about every effect and instrument as
//! JSON: its name, its parameter table and its default settings.
//!
//! `scripts/gen-bindings.sh` writes the output to
//! `apps/desktop/src/bindings/descriptors.json`, next to the generated
//! TypeScript types. The file also carries reference curves so the UI's own
//! drawing code for the equalizer and compressor displays can be tested
//! against the real processors.

use serde_json::{Value, json};
use windfall_dsp::{
    CompressorParams, EffectKind, EffectParams, EqParams, InstrumentKind, ParamSet, TrackParams,
};

const FIXTURE_SAMPLE_RATE: f32 = 48_000.0;

fn main() {
    use windfall_dsp::rack_note::RackNoteModule;
    let note_modules: serde_json::Map<String, Value> = [
        RackNoteModule::ColorRouter {
            params: Default::default(),
        },
        RackNoteModule::Envelope {
            params: Default::default(),
        },
        RackNoteModule::PropertyScaler {
            params: Default::default(),
        },
        RackNoteModule::KeySplitter {
            params: Default::default(),
            preserve_key: true,
        },
        RackNoteModule::KeyMapper {
            params: Default::default(),
            chord: [0; 4],
            voices: 1,
        },
        RackNoteModule::Sequencer {
            params: Default::default(),
        },
    ]
    .into_iter()
    .map(|module| {
        let params: Vec<Value> = (0..)
            .map_while(|i| {
                module.parameter_info(i).map(|info| {
                    let mut value = json!(info);
                    let prefix = match module {
                        RackNoteModule::KeySplitter { .. } if i >= 7 => "",
                        RackNoteModule::KeyMapper { .. } if i >= 128 => "",
                        RackNoteModule::Sequencer { .. } if i >= 4 => "params.grid.",
                        _ => "params.",
                    };
                    value["id"] = json!(format!("{prefix}{}", info.id));
                    value
                })
            })
            .collect();
        (
            module.kind_id().into(),
            json!({ "name": module.kind_id(), "params": params, "defaults": module }),
        )
    })
    .collect();
    let effects: serde_json::Map<String, Value> = EffectKind::ALL
        .iter()
        .map(|kind| {
            (
                key(kind),
                json!({
                    "name": kind.name(),
                    "params": kind.descriptors(),
                    "defaults": kind.default_params(),
                }),
            )
        })
        .collect();

    let instruments: serde_json::Map<String, Value> = InstrumentKind::ALL
        .iter()
        .map(|kind| {
            (
                key(kind),
                json!({
                    "name": kind.name(),
                    "params": kind.descriptors(),
                    "defaults": kind.default_params(),
                }),
            )
        })
        .collect();

    // Only the effects with an output a mixer can route to another track.
    let effect_outputs: serde_json::Map<String, Value> = EffectKind::ALL
        .iter()
        .filter(|kind| !kind.aux_outputs().is_empty())
        .map(|kind| (key(kind), json!(kind.aux_outputs())))
        .collect();

    let output = json!({
        "effects": effects,
        "instruments": instruments,
        "track": { "name": "Track EQ and stereo", "params": TrackParams::descriptors(), "defaults": TrackParams::default() },
        "effectOutputs": effect_outputs,
        "noteModules": note_modules,
        "fixtures": {
            "eqResponse": eq_fixture(),
            "compressorCurve": compressor_fixture(),
        },
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&output).expect("descriptors serialize")
    );
}

/// The JSON name of a kind, which is also its `type` tag in saved settings.
fn key<T: serde::Serialize>(kind: &T) -> String {
    match serde_json::to_value(kind).expect("a kind serializes") {
        Value::String(name) => name,
        other => panic!("a kind serializes to a string, not {other}"),
    }
}

/// Equalizer settings spread over the parameter ranges, each with the curve
/// the processor reports for it.
fn eq_fixture() -> Value {
    let frequencies: Vec<f32> = (0..48)
        .map(|step| 20.0 * (1000.0_f32).powf(step as f32 / 47.0))
        .collect();
    let descriptors = EffectKind::Eq.descriptors();

    let mut cases = Vec::new();
    for seed in 0..6u32 {
        let mut params = EffectKind::Eq.default_params();
        if seed > 0 {
            for (index, info) in descriptors.iter().enumerate() {
                // A fixed spread of positions inside each range, different
                // for every case and every parameter.
                let position = ((seed * 37 + index as u32 * 53) % 101) as f32 / 100.0;
                params.set(index, info.min + (info.max - info.min) * position);
            }
        }
        let EffectParams::Eq(eq) = params.sanitized() else {
            unreachable!("the equalizer's defaults are equalizer settings");
        };
        cases.push(json!({
            "params": eq,
            "gainsDb": response(&eq, &frequencies),
        }));
    }

    json!({
        "sampleRate": FIXTURE_SAMPLE_RATE,
        "frequenciesHz": frequencies,
        "cases": cases,
    })
}

fn response(params: &EqParams, frequencies: &[f32]) -> Vec<f32> {
    let mut gains = vec![0.0; frequencies.len()];
    params.magnitude_response_db(FIXTURE_SAMPLE_RATE, frequencies, &mut gains);
    gains
}

/// Compressor settings with the gain the processor applies at a range of
/// input levels, and the makeup gain in effect.
fn compressor_fixture() -> Value {
    let levels: Vec<f32> = (0..25).map(|step| -72.0 + step as f32 * 3.0).collect();
    let settings = [
        CompressorParams::default(),
        CompressorParams {
            threshold_db: -30.0,
            ratio: 2.0,
            knee_db: 0.0,
            ..CompressorParams::default()
        },
        CompressorParams {
            threshold_db: -12.0,
            ratio: 100.0,
            knee_db: 12.0,
            auto_makeup: true,
            ..CompressorParams::default()
        },
        CompressorParams {
            threshold_db: -24.0,
            ratio: 8.0,
            knee_db: 24.0,
            makeup_db: 6.0,
            ..CompressorParams::default()
        },
    ];

    let cases: Vec<Value> = settings
        .iter()
        .map(|params| {
            let gains: Vec<f32> = levels
                .iter()
                .map(|level| params.static_gain_db(*level))
                .collect();
            json!({
                "params": params,
                "gainsDb": gains,
                "totalMakeupDb": params.total_makeup_db(),
            })
        })
        .collect();

    json!({ "levelsDb": levels, "cases": cases })
}
