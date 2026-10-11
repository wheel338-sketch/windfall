//! Per-channel expression mapping: what modulation X, modulation Y and
//! release drive in the built-in voices.

use windfall_core::TICKS_PER_STEP;
use windfall_dsp::{NoteArticulation, NoteExpression};
use windfall_engine::RenderOptions;
use windfall_project::{
    ChannelId, Envelope, NoteCurveParameter, NoteCurvePoint, NoteExpressionCurve, NoteId,
};

use crate::support::{Rig, plain_synth, sine};

const RATE: u32 = 48_000;

fn curve(
    note: NoteId,
    parameter: NoteCurveParameter,
    values: &[(f32, f32)],
) -> NoteExpressionCurve {
    NoteExpressionCurve {
        note,
        parameter,
        points: values
            .iter()
            .map(|&(position, value)| NoteCurvePoint {
                position,
                value,
                curve: 0.25,
                hold: false,
            })
            .collect(),
    }
}

/// A sampler with an envelope and a synth, both playing notes that use every
/// stored expression field, curves on each parameter, a portamento, a slide
/// and the channel filter/pitch/pan envelopes.
fn expressive() -> (Rig, ChannelId, ChannelId) {
    let mut rig = Rig::new();
    let sampler = rig.channel(sine(RATE, 330.0, 1.5));
    rig.sampler_mut(sampler).envelope = Some(Envelope {
        attack_ms: 2.0,
        decay_ms: 60.0,
        sustain: 0.7,
        release_ms: 80.0,
    });
    let mut params = plain_synth();
    params.filter.cutoff_hz = 1_800.0;
    params.amp_envelope.release_ms = 40.0;
    let synth = rig.synth_on(params, windfall_project::TrackId::MASTER);
    let mut curved = Vec::new();
    for (index, channel) in [sampler, synth].into_iter().enumerate() {
        let shift = index as u32 * 7;
        let note = rig.note(channel, shift, TICKS_PER_STEP * 3);
        note.key = 57;
        note.pan = -0.4;
        note.expression = NoteExpression {
            release: 0.8,
            fine_pitch_cents: 35.0,
            modulation_x: 0.2,
            modulation_y: 0.9,
            ..NoteExpression::NEUTRAL
        };
        curved.push(note.id);
        let note = rig.note(channel, TICKS_PER_STEP * 2 + shift, TICKS_PER_STEP * 3);
        note.key = 64;
        note.velocity = 0.6;
        note.expression = NoteExpression {
            release: 0.1,
            modulation_x: 0.95,
            modulation_y: 0.05,
            articulation: NoteArticulation::Portamento,
            glide_ticks: 180,
            color_group: Some(3),
            ..NoteExpression::NEUTRAL
        };
        curved.push(note.id);
        let note = rig.note(channel, TICKS_PER_STEP * 4, TICKS_PER_STEP * 2);
        note.key = 60;
        note.expression = NoteExpression {
            articulation: NoteArticulation::Slide,
            color_group: Some(3),
            ..NoteExpression::NEUTRAL
        };
        let note = rig.note(channel, TICKS_PER_STEP * 7, TICKS_PER_STEP);
        note.key = 72;
        note.pan = 0.5;
        curved.push(note.id);
    }
    let pattern = rig.first_pattern();
    let curves = &mut rig.pattern_mut(pattern).note_curves;
    for pair in curved.chunks(3) {
        curves.push(curve(
            pair[0],
            NoteCurveParameter::ModulationX,
            &[(0.0, 0.1), (0.5, 0.9), (1.0, 0.3)],
        ));
        curves.push(curve(
            pair[0],
            NoteCurveParameter::FinePitchCents,
            &[(0.0, -50.0), (1.0, 120.0)],
        ));
        curves.push(curve(
            pair[1],
            NoteCurveParameter::ModulationY,
            &[(0.0, 0.9), (1.0, 0.2)],
        ));
        curves.push(curve(
            pair[1],
            NoteCurveParameter::Pan,
            &[(0.0, -1.0), (1.0, 1.0)],
        ));
        curves.push(curve(
            pair[2],
            NoteCurveParameter::Release,
            &[(0.0, 0.2), (1.0, 0.9)],
        ));
        curves.push(curve(
            pair[2],
            NoteCurveParameter::ModulationX,
            &[(0.0, 0.5), (0.4, 0.0), (1.0, 1.0)],
        ));
    }
    for channel in [sampler, synth] {
        let envelopes = &mut rig.channel_mut(channel).voice.envelopes;
        envelopes.filter.enabled = true;
        envelopes.filter.depth = -2.0;
        envelopes.pan.enabled = true;
        envelopes.pan.depth = 0.3;
    }
    rig.project.check().unwrap();
    (rig, sampler, synth)
}

fn fingerprint(samples: &[f32]) -> u64 {
    samples.iter().fold(0xcbf2_9ce4_8422_2325, |hash, sample| {
        (hash ^ u64::from(sample.to_bits())).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn offline(rig: &Rig) -> Vec<f32> {
    windfall_engine::render(
        &rig.project,
        &rig.pool,
        &RenderOptions {
            sample_rate: RATE,
            pattern_loops: 1,
            tail_secs: 0.5,
            auto_tail: false,
            ..RenderOptions::default()
        },
        &mut |_| true,
    )
    .samples()
    .to_vec()
}

/// One second of live playback: the whole pattern at 120 BPM and its tail.
fn live(rig: &Rig, block: usize) -> Vec<f32> {
    rig.play(RATE, RATE as usize * 5 / 2, block)
}

#[test]
fn print_the_reference_fingerprints() {
    let (rig, _, _) = expressive();
    let live = live(&rig, 64);
    println!(
        "BLOCKS 37={:#018x} 512={:#018x}",
        fingerprint(&self::live(&rig, 37)),
        fingerprint(&self::live(&rig, 512))
    );
    assert!(crate::support::peak(&live) > 0.05, "the fixture must sound");
    println!(
        "REFERENCE live={:#018x} offline={:#018x}",
        fingerprint(&live),
        fingerprint(&offline(&rig))
    );
}
