//! Bounded six-operator phase modulation. Feedback/cyclic edges use the previous
//! substep, independent of graph traversal order. No heap graph is copied to audio.
use super::{
    core,
    runtime::{self, Context, Tone},
};
use crate::{EnvelopeParams, Waveform};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum OperatorWave {
    Sine,
    Triangle,
    Saw,
    Square,
}
impl OperatorWave {
    fn waveform(self) -> Waveform {
        match self {
            Self::Sine => Waveform::Sine,
            Self::Triangle => Waveform::Triangle,
            Self::Saw => Waveform::Saw,
            Self::Square => Waveform::Square,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct MatrixOperator {
    pub ratio: f32,
    pub detune_cents: f32,
    pub level: f32,
    pub feedback: f32,
    pub key_scale: f32,
    pub waveform: OperatorWave,
    pub envelope: EnvelopeParams,
}
impl Default for MatrixOperator {
    fn default() -> Self {
        Self {
            ratio: 1.0,
            detune_cents: 0.0,
            level: 0.5,
            feedback: 0.0,
            key_scale: 0.0,
            waveform: OperatorWave::Sine,
            envelope: EnvelopeParams::default(),
        }
    }
}
pub(super) fn configure(operators: &[MatrixOperator; 6], tone: &mut Tone, rate: f32) {
    tone.matrix_graph = MatrixGraph::default();
    tone.matrix_cache.fill(MatrixCache::default());
    for (op, env) in operators.iter().zip(&mut tone.operators) {
        runtime::envelope(env, op.envelope, rate);
    }
}
#[derive(Clone, Copy)]
pub(super) struct MatrixCache {
    key: [f32; 3],
    increments: [f32; 6],
    scales: [f32; 6],
    gains: [f32; 6],
}
impl Default for MatrixCache {
    fn default() -> Self {
        Self {
            key: [f32::NAN; 3],
            increments: [0.0; 6],
            scales: [0.0; 6],
            gains: [0.0; 6],
        }
    }
}
/// Immutable route metadata is shared by every unison lane of this tone.
#[derive(Default)]
pub(super) struct MatrixGraph {
    avx: bool,
    graph_ready: bool,
    ratio_offsets: [f32; 6],
    detune_offsets: [f32; 6],
    requested: [f32; 6],
    highest_source: [u8; 6],
    fm_masks: [u32; 6],
    fm_aliases: [u8; 6],
    fm_columns: [[f32; 8]; 6],
    dense_fm: bool,
    ring_masks: [u32; 6],
    ring_aliases: [u8; 6],
    ring_columns: [[f32; 8]; 6],
    dense_ring: bool,
    levels: [f32; 8],
    feedback: [f32; 8],
    sine_only: bool,
    ring_free: bool,
}
#[allow(clippy::too_many_arguments)] // One fixed graph and one unison lane, no callback graph object.
#[inline(always)]
pub(super) fn matrix(
    t: &mut Tone,
    c: &Context,
    ops: &[MatrixOperator; 6],
    fm: &[[f32; 6]; 6],
    ring: &[[f32; 6]; 6],
    u: usize,
    pitch: f32,
    lfo_index: f32,
) -> [f32; 6] {
    let key = [pitch, c.rate, f32::from(c.key)];
    if !t.matrix_graph.graph_ready {
        let cache = &mut t.matrix_graph;
        cache.graph_ready = true;
        cache.avx = prepare_vector_features();
        cache.ratio_offsets = std::array::from_fn(|i| 12.0 * ops[i].ratio.log2());
        cache.detune_offsets = std::array::from_fn(|i| ops[i].detune_cents / 100.0);
        cache.sine_only = ops.iter().all(|op| op.waveform == OperatorWave::Sine);
        cache.ring_free = ring.iter().flatten().all(|depth| *depth == 0.0);
        for (to, op) in ops.iter().enumerate() {
            let mut requested = op.feedback * op.level * 4.0 + lfo_index;
            let mut highest = None;
            for (source, depth) in fm[to].iter().enumerate() {
                if *depth > 0.0 {
                    if highest.is_none_or(|previous: usize| {
                        cache.ratio_offsets[source] > cache.ratio_offsets[previous]
                    }) {
                        highest = Some(source);
                    }
                    requested += depth * ops[source].level * 4.0;
                }
            }
            cache.highest_source[to] = highest.map_or(6, |source| source as u8);
            cache.requested[to] = requested;
            cache.fm_masks[to] = fm[to]
                .iter()
                .enumerate()
                .fold(0, |bits, (i, depth)| bits | (u32::from(*depth != 0.0) << i));
            cache.fm_aliases[to] = (0..to).find(|&source| fm[source] == fm[to]).unwrap_or(to) as u8;
            cache.ring_masks[to] = ring[to]
                .iter()
                .enumerate()
                .fold(0, |bits, (i, depth)| bits | (u32::from(*depth != 0.0) << i));
            cache.ring_aliases[to] = (0..to)
                .find(|&source| ring[source] == ring[to])
                .unwrap_or(to) as u8;
        }
        cache.dense_fm = fm.iter().flatten().all(|depth| *depth > 0.0);
        cache.dense_ring = cache.ring_masks == [0x3f; 6];
        cache.fm_columns = route_columns(fm);
        cache.ring_columns = route_columns(ring);
        cache.levels = std::array::from_fn(|i| if i < 6 { ops[i].level } else { 0.0 });
        cache.feedback = std::array::from_fn(|i| if i < 6 { ops[i].feedback } else { 0.0 });
    }
    if t.matrix_cache[u].key != key {
        let cache = &mut t.matrix_cache[u];
        let graph = &t.matrix_graph;
        if cache.key[2] != key[2] {
            for (to, op) in ops.iter().enumerate() {
                cache.gains[to] = (op.key_scale * (c.key as f32 - 60.0) / 12.0)
                    .exp2()
                    .min(4.0);
            }
        }
        cache.key = key;
        cache.increments =
            prepared_increments(pitch, &graph.ratio_offsets, &graph.detune_offsets, c.rate);
        let mut sources = [0.0_f32; 6];
        let mut computed = 0_u32;
        // Every dense row reaches all six sources, even when its depths differ.
        // Their frequency maximum is shared; each carrier's depth budget isn't.
        let dense_highest = graph.dense_fm.then(|| {
            core::increment(
                pitch + graph.ratio_offsets[usize::from(graph.highest_source[0])],
                c.rate,
            ) * c.rate
        });
        if let Some(highest) = dense_highest {
            cache.scales = dense_scales(&cache.increments, &graph.requested, highest, c.rate);
        } else {
            for to in 0..6 {
                let inc = cache.increments[to];
                let mut highest = inc * c.rate;
                let source = usize::from(graph.highest_source[to]);
                if source < 6 {
                    // Increment is monotone in pitch, including both clamps. The
                    // largest routed ratio gives the exact same maximum as the
                    // original six-source scan. Every actual FM edge stays active.
                    if computed & (1 << source) == 0 {
                        sources[source] =
                            core::increment(pitch + graph.ratio_offsets[source], c.rate);
                        computed |= 1 << source;
                    }
                    highest = highest.max(sources[source] * c.rate);
                }
                let budget = ((c.rate * 0.24 - inc * c.rate) / highest.max(1.0) - 1.0).max(0.0);
                cache.scales[to] = if graph.requested[to] > 0.0 {
                    (budget / graph.requested[to]).min(1.0)
                } else {
                    1.0
                };
            }
        }
    }
    render(t, c, ops, fm, ring, u, lfo_index)
}
#[inline(always)]
fn render(
    t: &mut Tone,
    c: &Context,
    ops: &[MatrixOperator; 6],
    fm: &[[f32; 6]; 6],
    ring: &[[f32; 6]; 6],
    u: usize,
    lfo_index: f32,
) -> [f32; 6] {
    let previous = t.previous[u];
    let mut now = [0.0; 6];
    let cache = &t.matrix_cache[u];
    let graph = &t.matrix_graph;
    if graph.sine_only && graph.dense_ring && graph.dense_fm {
        let phases = prepared_phases(&mut t.oscillators[u], &cache.increments);
        let output = dense_sine_graph(
            &phases,
            &previous,
            graph,
            cache,
            &t.levels,
            lfo_index * c.lfo,
        );
        t.previous[u] = output;
        return output;
    }
    // Equal route rows have the exact same ordered dot product of the same
    // previous substep. Share that result without changing any edge's depth,
    // its feedback contribution, or accumulation order. Reconfiguration
    // invalidates aliases along with all other graph coefficients.
    let mut routed = [0.0; 6];
    if graph.dense_fm {
        routed = dense_routes(&graph.fm_columns, &previous, None);
    } else {
        for to in 0..6 {
            let source = usize::from(graph.fm_aliases[to]);
            routed[to] = if source < to {
                routed[source]
            } else {
                route_sum(&fm[to], &previous, graph.fm_masks[to])
            };
        }
    }
    let ring_factors = if !graph.ring_free && !(graph.dense_ring && graph.sine_only) {
        let mut factors = [[1.0; 6]; 6];
        for to in 0..6 {
            if usize::from(graph.ring_aliases[to]) == to {
                for (source, &depth) in ring[to].iter().enumerate() {
                    factors[to][source] = 1.0 - depth + depth * previous[source];
                }
            }
        }
        Some(factors)
    } else {
        None
    };
    if graph.sine_only {
        // Phase modulation depends only on the previous substep. Stage those
        // independent phases, then evaluate the six carriers as a SIMD-friendly
        // polynomial block. Two padded lanes are scratch, never extra voices.
        let mut phases = prepared_phases(&mut t.oscillators[u], &cache.increments);
        modulate_phases(
            &mut phases,
            &graph.feedback,
            &routed,
            &previous,
            &cache.scales,
            lfo_index * c.lfo,
        );
        wrapped_sines(&mut phases);
        if graph.dense_ring {
            let samples = dense_routes(&graph.ring_columns, &previous, Some(&phases));
            phases[..6].copy_from_slice(&samples);
        }
        if !graph.ring_free && !graph.dense_ring {
            for (to, phase) in phases.iter_mut().enumerate().take(6) {
                *phase = ring_product(
                    *phase,
                    &ring_factors.as_ref().expect("sparse ring factors prepared")
                        [usize::from(graph.ring_aliases[to])],
                    graph.ring_masks[to],
                );
            }
        }
        now = operator_gains(&phases, &graph.levels, &t.levels, &cache.gains);
        t.previous[u] = now;
        return now;
    }
    for (to, op) in ops.iter().enumerate() {
        let inc = cache.increments[to];
        let phase = t.oscillators[u][to].phase();
        let modulation =
            (routed[to] + op.feedback * previous[to] + lfo_index * c.lfo) * cache.scales[to];
        // Keep phase modulation finite while preserving all small-signal routes.
        let moved = phase + modulation / std::f32::consts::TAU;
        let moved = moved - moved.floor();
        let raw = if op.waveform == OperatorWave::Sine {
            crate::blocks::oscillator::sine_fast(moved)
        } else {
            crate::blocks::oscillator::tonal_sample(op.waveform.waveform(), moved, inc, 0.5)
        };
        t.oscillators[u][to].advance_bounded(inc);
        let sample = if graph.ring_free {
            raw
        } else {
            ring_product(
                raw,
                &ring_factors
                    .as_ref()
                    .expect("non-sine ring factors prepared")[usize::from(graph.ring_aliases[to])],
                graph.ring_masks[to],
            )
        };
        now[to] = core::guard(sample * op.level * t.levels[to] * cache.gains[to]);
    }
    t.previous[u] = now;
    now
}

fn route_columns(rows: &[[f32; 6]; 6]) -> [[f32; 8]; 6] {
    std::array::from_fn(|source| {
        std::array::from_fn(|to| if to < 6 { rows[to][source] } else { 0.0 })
    })
}
pub(super) fn prepare_vector_features() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

#[inline(always)]
fn prepared_increments(pitch: f32, ratios: &[f32; 6], detunes: &[f32; 6], rate: f32) -> [f32; 6] {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;
        macro_rules! hz {
            ($i:expr) => {
                crate::blocks::math::key_to_hz(
                    (pitch + ratios[$i] + detunes[$i]).clamp(-36.0, 160.0),
                )
            };
        }
        let frequencies = [hz!(0), hz!(1), hz!(2), hz!(3), hz!(4), hz!(5), 0.0, 0.0];
        let mut output = [0.0; 8];
        // The same scalar frequency mapping; division/clipping of independent
        // carriers is SIMD. Preserve a NaN's scalar clamp behavior too.
        unsafe {
            macro_rules! lane {
                ($offset:expr) => {{
                    let value = _mm_div_ps(
                        _mm_loadu_ps(frequencies.as_ptr().add($offset)),
                        _mm_set1_ps(rate),
                    );
                    let clipped = _mm_max_ps(
                        _mm_min_ps(value, _mm_set1_ps(crate::blocks::oscillator::MAX_INCREMENT)),
                        _mm_set1_ps(1.0e-7),
                    );
                    let nan = _mm_cmpunord_ps(value, value);
                    _mm_storeu_ps(
                        output.as_mut_ptr().add($offset),
                        _mm_or_ps(_mm_and_ps(nan, value), _mm_andnot_ps(nan, clipped)),
                    );
                }};
            }
            lane!(0);
            lane!(4);
        }
        output[..6].try_into().expect("six fixed frequency lanes")
    }
    #[cfg(not(target_arch = "x86_64"))]
    std::array::from_fn(|i| core::increment(pitch + ratios[i] + detunes[i], rate))
}

#[inline(always)]
fn prepared_phases(
    oscillators: &mut [crate::blocks::oscillator::Oscillator; 6],
    increments: &[f32; 6],
) -> [f32; 8] {
    let mut phases = [0.0; 8];

    phases[0] = oscillators[0].phase();
    oscillators[0].advance_bounded(increments[0]);

    phases[1] = oscillators[1].phase();
    oscillators[1].advance_bounded(increments[1]);

    phases[2] = oscillators[2].phase();
    oscillators[2].advance_bounded(increments[2]);

    phases[3] = oscillators[3].phase();
    oscillators[3].advance_bounded(increments[3]);

    phases[4] = oscillators[4].phase();
    oscillators[4].advance_bounded(increments[4]);

    phases[5] = oscillators[5].phase();
    oscillators[5].advance_bounded(increments[5]);

    phases
}
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn wrapped_sine_register_avx(moved: std::arch::x86_64::__m256) -> std::arch::x86_64::__m256 {
    use std::arch::x86_64::*;
    // SSE2 is baseline on x86_64; bounded matrix phases fit signed conversion.
    let integral = _mm256_cvtepi32_ps(_mm256_cvttps_epi32(moved));
    let correction = _mm256_and_ps(
        _mm256_cmp_ps::<{ _CMP_LT_OQ }>(moved, integral),
        _mm256_set1_ps(1.0),
    );
    let floor = _mm256_sub_ps(integral, correction);
    let phase = _mm256_andnot_ps(
        _mm256_cmp_ps::<{ _CMP_EQ_OQ }>(moved, _mm256_setzero_ps()),
        _mm256_sub_ps(moved, floor),
    );
    let first_half = _mm256_cmp_ps::<{ _CMP_LT_OQ }>(phase, _mm256_set1_ps(0.5));
    let sign = _mm256_or_ps(
        _mm256_and_ps(first_half, _mm256_set1_ps(1.0)),
        _mm256_andnot_ps(first_half, _mm256_set1_ps(-1.0)),
    );
    let quarter = _mm256_or_ps(
        _mm256_and_ps(first_half, phase),
        _mm256_andnot_ps(first_half, _mm256_sub_ps(phase, _mm256_set1_ps(0.5))),
    );
    let reflected = _mm256_cmp_ps::<{ _CMP_GT_OQ }>(quarter, _mm256_set1_ps(0.25));
    let quarter = _mm256_or_ps(
        _mm256_and_ps(reflected, _mm256_sub_ps(_mm256_set1_ps(0.5), quarter)),
        _mm256_andnot_ps(reflected, quarter),
    );
    let y = _mm256_mul_ps(_mm256_set1_ps(std::f32::consts::TAU), quarter);
    let square = _mm256_mul_ps(y, y);
    let polynomial = _mm256_add_ps(
        _mm256_set1_ps(1.0),
        _mm256_mul_ps(
            square,
            _mm256_add_ps(
                _mm256_set1_ps(-0.166_656_8),
                _mm256_mul_ps(
                    square,
                    _mm256_add_ps(
                        _mm256_set1_ps(0.008_312_37),
                        _mm256_mul_ps(square, _mm256_set1_ps(-0.000_184_92)),
                    ),
                ),
            ),
        ),
    );
    _mm256_mul_ps(_mm256_mul_ps(sign, y), polynomial)
}
#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn dense_sine_graph_avx(
    phases: &[f32; 8],
    previous: &[f32; 6],
    graph: &MatrixGraph,
    cache: &MatrixCache,
    envelopes: &[f32; 6],
    lfo: f32,
) -> [f32; 6] {
    use std::arch::x86_64::*;
    // Runtime feature admission precedes this call. Masked loads access only
    // the six actual values; two disabled padding lanes never dereference.
    unsafe {
        let mask = _mm256_setr_epi32(-1, -1, -1, -1, -1, -1, 0, 0);
        let load_six = |values: &[f32; 6]| _mm256_maskload_ps(values.as_ptr(), mask);
        let mut routed = _mm256_set1_ps(-0.0);
        for (source, previous_value) in previous.iter().enumerate() {
            let depth = _mm256_loadu_ps(graph.fm_columns[source].as_ptr().add(0));
            routed = _mm256_add_ps(
                routed,
                _mm256_mul_ps(depth, _mm256_set1_ps(*previous_value)),
            );
        }
        let feedback = _mm256_loadu_ps(graph.feedback.as_ptr().add(0));
        let modulation = _mm256_mul_ps(
            _mm256_add_ps(
                _mm256_add_ps(routed, _mm256_mul_ps(feedback, load_six(previous))),
                _mm256_set1_ps(lfo),
            ),
            load_six(&cache.scales),
        );
        let moved = _mm256_add_ps(
            _mm256_loadu_ps(phases.as_ptr().add(0)),
            _mm256_div_ps(modulation, _mm256_set1_ps(std::f32::consts::TAU)),
        );
        let mut sample = wrapped_sine_register_avx(moved);
        {
            let depth = _mm256_loadu_ps(graph.ring_columns[0].as_ptr().add(0));
            let factor = _mm256_add_ps(
                _mm256_sub_ps(_mm256_set1_ps(1.0), depth),
                _mm256_mul_ps(depth, _mm256_set1_ps(previous[0])),
            );
            sample = _mm256_mul_ps(sample, factor);
        }
        {
            let depth = _mm256_loadu_ps(graph.ring_columns[1].as_ptr().add(0));
            let factor = _mm256_add_ps(
                _mm256_sub_ps(_mm256_set1_ps(1.0), depth),
                _mm256_mul_ps(depth, _mm256_set1_ps(previous[1])),
            );
            sample = _mm256_mul_ps(sample, factor);
        }
        {
            let depth = _mm256_loadu_ps(graph.ring_columns[2].as_ptr().add(0));
            let factor = _mm256_add_ps(
                _mm256_sub_ps(_mm256_set1_ps(1.0), depth),
                _mm256_mul_ps(depth, _mm256_set1_ps(previous[2])),
            );
            sample = _mm256_mul_ps(sample, factor);
        }
        {
            let depth = _mm256_loadu_ps(graph.ring_columns[3].as_ptr().add(0));
            let factor = _mm256_add_ps(
                _mm256_sub_ps(_mm256_set1_ps(1.0), depth),
                _mm256_mul_ps(depth, _mm256_set1_ps(previous[3])),
            );
            sample = _mm256_mul_ps(sample, factor);
        }
        {
            let depth = _mm256_loadu_ps(graph.ring_columns[4].as_ptr().add(0));
            let factor = _mm256_add_ps(
                _mm256_sub_ps(_mm256_set1_ps(1.0), depth),
                _mm256_mul_ps(depth, _mm256_set1_ps(previous[4])),
            );
            sample = _mm256_mul_ps(sample, factor);
        }
        {
            let depth = _mm256_loadu_ps(graph.ring_columns[5].as_ptr().add(0));
            let factor = _mm256_add_ps(
                _mm256_sub_ps(_mm256_set1_ps(1.0), depth),
                _mm256_mul_ps(depth, _mm256_set1_ps(previous[5])),
            );
            sample = _mm256_mul_ps(sample, factor);
        }
        let level = _mm256_loadu_ps(graph.levels.as_ptr().add(0));
        let value = _mm256_mul_ps(
            _mm256_mul_ps(_mm256_mul_ps(sample, level), load_six(envelopes)),
            load_six(&cache.gains),
        );
        let finite = _mm256_cmp_ps::<{ _CMP_LE_OQ }>(
            _mm256_andnot_ps(_mm256_set1_ps(-0.0), value),
            _mm256_set1_ps(f32::MAX),
        );
        let clipped = _mm256_max_ps(
            _mm256_min_ps(value, _mm256_set1_ps(4.0)),
            _mm256_set1_ps(-4.0),
        );

        let mut output = [0.0; 8];
        _mm256_storeu_ps(output.as_mut_ptr(), _mm256_and_ps(finite, clipped));
        output[..6].try_into().expect("six fixed AVX carriers")
    }
}

#[inline(always)]
fn dense_sine_graph(
    phases: &[f32; 8],
    previous: &[f32; 6],
    graph: &MatrixGraph,
    cache: &MatrixCache,
    envelopes: &[f32; 6],
    lfo: f32,
) -> [f32; 6] {
    #[cfg(target_arch = "x86_64")]
    if graph.avx {
        // SAFETY: this prepared graph records successful AVX2 feature admission.
        return unsafe { dense_sine_graph_avx(phases, previous, graph, cache, envelopes, lfo) };
    }
    let mut output = [0.0; 8];
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;
        // Keep each destination's phase, polynomial, ordered ring chain and
        // level products in registers through the entire prepared graph.
        unsafe {
            macro_rules! lane {
                ($offset:expr) => {{
                    let load_six = |values: &[f32; 6]| {
                        if $offset == 0 {
                            _mm_loadu_ps(values.as_ptr())
                        } else {
                            _mm_setr_ps(values[4], values[5], 0.0, 0.0)
                        }
                    };
                    let mut routed = _mm_set1_ps(-0.0);
                    for source in 0..6 {
                        let depth = _mm_loadu_ps(graph.fm_columns[source].as_ptr().add($offset));
                        routed =
                            _mm_add_ps(routed, _mm_mul_ps(depth, _mm_set1_ps(previous[source])));
                    }
                    let feedback = _mm_loadu_ps(graph.feedback.as_ptr().add($offset));
                    let modulation = _mm_mul_ps(
                        _mm_add_ps(
                            _mm_add_ps(routed, _mm_mul_ps(feedback, load_six(previous))),
                            _mm_set1_ps(lfo),
                        ),
                        load_six(&cache.scales),
                    );
                    let moved = _mm_add_ps(
                        _mm_loadu_ps(phases.as_ptr().add($offset)),
                        _mm_div_ps(modulation, _mm_set1_ps(std::f32::consts::TAU)),
                    );
                    let mut sample = wrapped_sine_register(moved);
                    {
                        let depth = _mm_loadu_ps(graph.ring_columns[0].as_ptr().add($offset));
                        let factor = _mm_add_ps(
                            _mm_sub_ps(_mm_set1_ps(1.0), depth),
                            _mm_mul_ps(depth, _mm_set1_ps(previous[0])),
                        );
                        sample = _mm_mul_ps(sample, factor);
                    }
                    {
                        let depth = _mm_loadu_ps(graph.ring_columns[1].as_ptr().add($offset));
                        let factor = _mm_add_ps(
                            _mm_sub_ps(_mm_set1_ps(1.0), depth),
                            _mm_mul_ps(depth, _mm_set1_ps(previous[1])),
                        );
                        sample = _mm_mul_ps(sample, factor);
                    }
                    {
                        let depth = _mm_loadu_ps(graph.ring_columns[2].as_ptr().add($offset));
                        let factor = _mm_add_ps(
                            _mm_sub_ps(_mm_set1_ps(1.0), depth),
                            _mm_mul_ps(depth, _mm_set1_ps(previous[2])),
                        );
                        sample = _mm_mul_ps(sample, factor);
                    }
                    {
                        let depth = _mm_loadu_ps(graph.ring_columns[3].as_ptr().add($offset));
                        let factor = _mm_add_ps(
                            _mm_sub_ps(_mm_set1_ps(1.0), depth),
                            _mm_mul_ps(depth, _mm_set1_ps(previous[3])),
                        );
                        sample = _mm_mul_ps(sample, factor);
                    }
                    {
                        let depth = _mm_loadu_ps(graph.ring_columns[4].as_ptr().add($offset));
                        let factor = _mm_add_ps(
                            _mm_sub_ps(_mm_set1_ps(1.0), depth),
                            _mm_mul_ps(depth, _mm_set1_ps(previous[4])),
                        );
                        sample = _mm_mul_ps(sample, factor);
                    }
                    {
                        let depth = _mm_loadu_ps(graph.ring_columns[5].as_ptr().add($offset));
                        let factor = _mm_add_ps(
                            _mm_sub_ps(_mm_set1_ps(1.0), depth),
                            _mm_mul_ps(depth, _mm_set1_ps(previous[5])),
                        );
                        sample = _mm_mul_ps(sample, factor);
                    }
                    let level = _mm_loadu_ps(graph.levels.as_ptr().add($offset));
                    let value = _mm_mul_ps(
                        _mm_mul_ps(_mm_mul_ps(sample, level), load_six(envelopes)),
                        load_six(&cache.gains),
                    );
                    let finite = _mm_cmple_ps(
                        _mm_andnot_ps(_mm_set1_ps(-0.0), value),
                        _mm_set1_ps(f32::MAX),
                    );
                    let clipped =
                        _mm_max_ps(_mm_min_ps(value, _mm_set1_ps(4.0)), _mm_set1_ps(-4.0));
                    _mm_storeu_ps(
                        output.as_mut_ptr().add($offset),
                        _mm_and_ps(finite, clipped),
                    );
                }};
            }
            lane!(0);
            lane!(4);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        let routed = dense_routes(&graph.fm_columns, previous, None);
        let mut phases = *phases;
        modulate_phases(
            &mut phases,
            &graph.feedback,
            &routed,
            previous,
            &cache.scales,
            lfo,
        );
        wrapped_sines(&mut phases);
        let samples = dense_routes(&graph.ring_columns, previous, Some(&phases));
        phases[..6].copy_from_slice(&samples);
        output[..6].copy_from_slice(&operator_gains(
            &phases,
            &graph.levels,
            envelopes,
            &cache.gains,
        ));
    }
    output[..6].try_into().expect("six fixed dense carriers")
}

#[inline(always)]
fn dense_scales(increments: &[f32; 6], requested: &[f32; 6], source: f32, rate: f32) -> [f32; 6] {
    let mut output = [0.0; 8];
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;
        // Six independent carrier budgets; source maximum may be shared but
        // neither unequal depths nor carrier increments are reassociated.
        unsafe {
            macro_rules! lane {
                ($offset:expr) => {{
                    let load_six = |values: &[f32; 6]| {
                        if $offset == 0 {
                            _mm_loadu_ps(values.as_ptr())
                        } else {
                            _mm_setr_ps(values[4], values[5], 0.0, 0.0)
                        }
                    };
                    let carrier = _mm_mul_ps(load_six(increments), _mm_set1_ps(rate));
                    let highest =
                        _mm_max_ps(_mm_max_ps(carrier, _mm_set1_ps(source)), _mm_set1_ps(1.0));
                    let budget = _mm_max_ps(
                        _mm_sub_ps(
                            _mm_div_ps(_mm_sub_ps(_mm_set1_ps(rate * 0.24), carrier), highest),
                            _mm_set1_ps(1.0),
                        ),
                        _mm_setzero_ps(),
                    );
                    let requested = load_six(requested);
                    let active = _mm_cmpgt_ps(requested, _mm_setzero_ps());
                    let scale = _mm_min_ps(_mm_div_ps(budget, requested), _mm_set1_ps(1.0));
                    let value = _mm_or_ps(
                        _mm_and_ps(active, scale),
                        _mm_andnot_ps(active, _mm_set1_ps(1.0)),
                    );
                    _mm_storeu_ps(output.as_mut_ptr().add($offset), value);
                }};
            }
            lane!(0);
            lane!(4);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    for to in 0..6 {
        let carrier = increments[to] * rate;
        let highest = carrier.max(source);
        let budget = ((rate * 0.24 - carrier) / highest.max(1.0) - 1.0).max(0.0);
        output[to] = if requested[to] > 0.0 {
            (budget / requested[to]).min(1.0)
        } else {
            1.0
        };
    }
    output[..6].try_into().expect("six fixed carrier budgets")
}

#[inline(always)]
fn modulate_phases(
    phases: &mut [f32; 8],
    feedback: &[f32; 8],
    routed: &[f32; 6],
    previous: &[f32; 6],
    scales: &[f32; 6],
    lfo: f32,
) {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;
        // Prefix loads contain four values; the last two have explicit padding.
        // All additions, scaling and division retain their scalar order.
        unsafe {
            macro_rules! lane {
                ($offset:expr) => {{
                    let offset = $offset;
                    let load_six = |values: &[f32; 6]| {
                        if offset == 0 {
                            _mm_loadu_ps(values.as_ptr())
                        } else {
                            _mm_setr_ps(values[4], values[5], 0.0, 0.0)
                        }
                    };
                    let route = load_six(routed);
                    let before = load_six(previous);
                    let scale = load_six(scales);
                    let feedback = _mm_loadu_ps(feedback.as_ptr().add(offset));
                    let modulation = _mm_mul_ps(
                        _mm_add_ps(
                            _mm_add_ps(route, _mm_mul_ps(feedback, before)),
                            _mm_set1_ps(lfo),
                        ),
                        scale,
                    );
                    let phase = _mm_loadu_ps(phases.as_ptr().add(offset));
                    let moved = _mm_add_ps(
                        phase,
                        _mm_div_ps(modulation, _mm_set1_ps(std::f32::consts::TAU)),
                    );
                    _mm_storeu_ps(phases.as_mut_ptr().add(offset), moved);
                }};
            }
            lane!(0);
            lane!(4);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    for to in 0..6 {
        let modulation = (routed[to] + feedback[to] * previous[to] + lfo) * scales[to];
        phases[to] += modulation / std::f32::consts::TAU;
    }
}

#[inline(always)]
fn operator_gains(
    samples: &[f32; 8],
    levels: &[f32; 8],
    envelopes: &[f32; 6],
    gains: &[f32; 6],
) -> [f32; 6] {
    let mut output = [0.0; 8];
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;
        // All loads are in-bounds. The final two lanes have explicit padding;
        // gain ordering and finite/clipping behavior match core::guard exactly.
        unsafe {
            macro_rules! lane {
                ($offset:expr) => {{
                    let offset = $offset;
                    let envelope = if offset == 0 {
                        _mm_loadu_ps(envelopes.as_ptr())
                    } else {
                        _mm_setr_ps(envelopes[4], envelopes[5], 0.0, 0.0)
                    };
                    let gain = if offset == 0 {
                        _mm_loadu_ps(gains.as_ptr())
                    } else {
                        _mm_setr_ps(gains[4], gains[5], 0.0, 0.0)
                    };
                    let sample = _mm_loadu_ps(samples.as_ptr().add(offset));
                    let level = _mm_loadu_ps(levels.as_ptr().add(offset));
                    let value = _mm_mul_ps(_mm_mul_ps(_mm_mul_ps(sample, level), envelope), gain);
                    let finite = _mm_cmple_ps(
                        _mm_andnot_ps(_mm_set1_ps(-0.0), value),
                        _mm_set1_ps(f32::MAX),
                    );
                    let clipped =
                        _mm_max_ps(_mm_min_ps(value, _mm_set1_ps(4.0)), _mm_set1_ps(-4.0));
                    _mm_storeu_ps(output.as_mut_ptr().add(offset), _mm_and_ps(finite, clipped));
                }};
            }
            lane!(0);
            lane!(4);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    for to in 0..6 {
        output[to] = core::guard(samples[to] * levels[to] * envelopes[to] * gains[to]);
    }
    output[..6].try_into().expect("six fixed operator lanes")
}

#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn wrapped_sine_register(moved: std::arch::x86_64::__m128) -> std::arch::x86_64::__m128 {
    use std::arch::x86_64::*;
    // SSE2 is baseline on x86_64; bounded matrix phases fit signed conversion.
    unsafe {
        let integral = _mm_cvtepi32_ps(_mm_cvttps_epi32(moved));
        let correction = _mm_and_ps(_mm_cmplt_ps(moved, integral), _mm_set1_ps(1.0));
        let floor = _mm_sub_ps(integral, correction);
        let phase = _mm_andnot_ps(
            _mm_cmpeq_ps(moved, _mm_setzero_ps()),
            _mm_sub_ps(moved, floor),
        );
        let first_half = _mm_cmplt_ps(phase, _mm_set1_ps(0.5));
        let sign = _mm_or_ps(
            _mm_and_ps(first_half, _mm_set1_ps(1.0)),
            _mm_andnot_ps(first_half, _mm_set1_ps(-1.0)),
        );
        let quarter = _mm_or_ps(
            _mm_and_ps(first_half, phase),
            _mm_andnot_ps(first_half, _mm_sub_ps(phase, _mm_set1_ps(0.5))),
        );
        let reflected = _mm_cmpgt_ps(quarter, _mm_set1_ps(0.25));
        let quarter = _mm_or_ps(
            _mm_and_ps(reflected, _mm_sub_ps(_mm_set1_ps(0.5), quarter)),
            _mm_andnot_ps(reflected, quarter),
        );
        let y = _mm_mul_ps(_mm_set1_ps(std::f32::consts::TAU), quarter);
        let square = _mm_mul_ps(y, y);
        let polynomial = _mm_add_ps(
            _mm_set1_ps(1.0),
            _mm_mul_ps(
                square,
                _mm_add_ps(
                    _mm_set1_ps(-0.166_656_8),
                    _mm_mul_ps(
                        square,
                        _mm_add_ps(
                            _mm_set1_ps(0.008_312_37),
                            _mm_mul_ps(square, _mm_set1_ps(-0.000_184_92)),
                        ),
                    ),
                ),
            ),
        );
        _mm_mul_ps(_mm_mul_ps(sign, y), polynomial)
    }
}
#[inline(always)]
fn wrapped_sines(phases: &mut [f32; 8]) {
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;
        // Matrix phases are bounded by sanitized route depths, LFO and guarded
        // previous outputs, well inside signed conversion range. SSE2 computes
        // floor via truncation and a negative-fraction correction, then the
        // existing sine polynomial in exactly its original arithmetic order.
        unsafe {
            macro_rules! lane {
                ($offset:expr) => {{
                    let offset = $offset;
                    let moved = _mm_loadu_ps(phases.as_ptr().add(offset));
                    let sine = wrapped_sine_register(moved);
                    _mm_storeu_ps(phases.as_mut_ptr().add(offset), sine);
                }};
            }
            lane!(0);
            lane!(4);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    for phase in phases {
        *phase = crate::blocks::oscillator::sine_fast(*phase - phase.floor());
    }
}

/// Independent destination lanes; each lane retains the original six-source
/// addition/multiplication order. Separate mul/add instructions never fuse.
#[inline(always)]
fn dense_routes(
    columns: &[[f32; 8]; 6],
    previous: &[f32; 6],
    carriers: Option<&[f32; 8]>,
) -> [f32; 6] {
    let mut lanes = carriers.copied().unwrap_or([-0.0; 8]);
    #[cfg(target_arch = "x86_64")]
    {
        use std::arch::x86_64::*;
        // SSE2 is mandatory on x86_64. Every unaligned load/store accesses four
        // lanes within an eight-lane array; the two padding lanes are discarded.
        unsafe {
            macro_rules! lane {
                ($offset:expr) => {{
                    let offset = $offset;
                    let mut values = _mm_loadu_ps(lanes.as_ptr().add(offset));
                    {
                        let depth = _mm_loadu_ps(columns[0].as_ptr().add(offset));
                        let product = _mm_mul_ps(depth, _mm_set1_ps(previous[0]));
                        values = if carriers.is_some() {
                            let factor = _mm_add_ps(_mm_sub_ps(_mm_set1_ps(1.0), depth), product);
                            _mm_mul_ps(values, factor)
                        } else {
                            _mm_add_ps(values, product)
                        };
                    }
                    {
                        let depth = _mm_loadu_ps(columns[1].as_ptr().add(offset));
                        let product = _mm_mul_ps(depth, _mm_set1_ps(previous[1]));
                        values = if carriers.is_some() {
                            let factor = _mm_add_ps(_mm_sub_ps(_mm_set1_ps(1.0), depth), product);
                            _mm_mul_ps(values, factor)
                        } else {
                            _mm_add_ps(values, product)
                        };
                    }
                    {
                        let depth = _mm_loadu_ps(columns[2].as_ptr().add(offset));
                        let product = _mm_mul_ps(depth, _mm_set1_ps(previous[2]));
                        values = if carriers.is_some() {
                            let factor = _mm_add_ps(_mm_sub_ps(_mm_set1_ps(1.0), depth), product);
                            _mm_mul_ps(values, factor)
                        } else {
                            _mm_add_ps(values, product)
                        };
                    }
                    {
                        let depth = _mm_loadu_ps(columns[3].as_ptr().add(offset));
                        let product = _mm_mul_ps(depth, _mm_set1_ps(previous[3]));
                        values = if carriers.is_some() {
                            let factor = _mm_add_ps(_mm_sub_ps(_mm_set1_ps(1.0), depth), product);
                            _mm_mul_ps(values, factor)
                        } else {
                            _mm_add_ps(values, product)
                        };
                    }
                    {
                        let depth = _mm_loadu_ps(columns[4].as_ptr().add(offset));
                        let product = _mm_mul_ps(depth, _mm_set1_ps(previous[4]));
                        values = if carriers.is_some() {
                            let factor = _mm_add_ps(_mm_sub_ps(_mm_set1_ps(1.0), depth), product);
                            _mm_mul_ps(values, factor)
                        } else {
                            _mm_add_ps(values, product)
                        };
                    }
                    {
                        let depth = _mm_loadu_ps(columns[5].as_ptr().add(offset));
                        let product = _mm_mul_ps(depth, _mm_set1_ps(previous[5]));
                        values = if carriers.is_some() {
                            let factor = _mm_add_ps(_mm_sub_ps(_mm_set1_ps(1.0), depth), product);
                            _mm_mul_ps(values, factor)
                        } else {
                            _mm_add_ps(values, product)
                        };
                    }
                    _mm_storeu_ps(lanes.as_mut_ptr().add(offset), values);
                }};
            }
            lane!(0);
            lane!(4);
        }
    }
    #[cfg(not(target_arch = "x86_64"))]
    for source in 0..6 {
        for (to, value) in lanes.iter_mut().enumerate() {
            let depth = columns[source][to];
            let product = depth * previous[source];
            if carriers.is_some() {
                *value *= 1.0 - depth + product;
            } else {
                *value += product;
            }
        }
    }
    lanes[..6].try_into().expect("six fixed destination lanes")
}

#[inline(always)]
fn ring_product(mut sample: f32, factors: &[f32; 6], mut mask: u32) -> f32 {
    if mask == 0x3f {
        for &factor in factors {
            // Depth 1 is a true ring product, depth 0 leaves the carrier intact.
            sample *= factor;
        }
    } else {
        while mask != 0 {
            let source = mask.trailing_zeros() as usize;
            sample *= factors[source];
            mask &= mask - 1;
        }
    }
    sample
}

#[inline(always)]
fn route_sum(depths: &[f32; 6], previous: &[f32; 6], mut mask: u32) -> f32 {
    if mask == 0x3f {
        depths
            .iter()
            .zip(previous)
            .map(|(depth, source)| depth * source)
            .sum()
    } else {
        let mut sum = 0.0;
        while mask != 0 {
            let source = mask.trailing_zeros() as usize;
            sum += depths[source] * previous[source];
            mask &= mask - 1;
        }
        sum
    }
}

#[cfg(test)]
mod metadata_tests {
    use super::*;
    #[test]
    fn fused_dense_sine_graph_matches_ordered_stages_bits() {
        let graph = MatrixGraph {
            fm_columns: route_columns(&std::array::from_fn(|to| {
                std::array::from_fn(|source| 0.17 + (to * 6 + source) as f32 * 0.023)
            })),
            feedback: std::array::from_fn(|i| 6.0 - i as f32 * 0.273),
            levels: std::array::from_fn(|i| 0.91 - i as f32 * 0.071),
            ring_columns: route_columns(&std::array::from_fn(|to| {
                std::array::from_fn(|source| 0.1 + (to * 6 + source) as f32 * 0.019)
            })),
            ..Default::default()
        };
        let cache = MatrixCache {
            scales: std::array::from_fn(|i| 0.77 - i as f32 * 0.113),
            gains: std::array::from_fn(|i| 3.71 - i as f32 * 0.171),
            ..Default::default()
        };
        for frame in 0..2048 {
            let phases = std::array::from_fn(|i| (frame * 13 + i * 17) as f32 * 0.0017);
            let previous = std::array::from_fn(|i| ((frame * 7 + i * 23) as f32 * 0.071).cos());
            let routed = dense_routes(&graph.fm_columns, &previous, None);
            let envelopes = std::array::from_fn(|i| 0.97 - i as f32 * 0.133);
            let lfo = (frame as f32 * 0.013).sin();
            let mut expected = phases;
            modulate_phases(
                &mut expected,
                &graph.feedback,
                &routed,
                &previous,
                &cache.scales,
                lfo,
            );
            wrapped_sines(&mut expected);
            let samples = dense_routes(&graph.ring_columns, &previous, Some(&expected));
            expected[..6].copy_from_slice(&samples);
            let expected = operator_gains(&expected, &graph.levels, &envelopes, &cache.gains);
            let actual = dense_sine_graph(&phases, &previous, &graph, &cache, &envelopes, lfo);
            assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
            #[cfg(target_arch = "x86_64")]
            if prepare_vector_features() {
                // SAFETY: this test has just checked the AVX2 admission predicate.
                let avx = unsafe {
                    dense_sine_graph_avx(&phases, &previous, &graph, &cache, &envelopes, lfo)
                };
                assert_eq!(avx.map(f32::to_bits), expected.map(f32::to_bits));
            }
        }
    }
    #[test]
    fn phase_modulation_lanes_match_scalar_order_bits() {
        for frame in 0..1024 {
            let mut phases = std::array::from_fn(|lane| (frame * 13 + lane * 17) as f32 * 0.0017);
            let feedback = std::array::from_fn(|lane| 6.0 - lane as f32 * 0.371);
            let routed =
                std::array::from_fn(|lane| ((frame * 23 + lane * 7) as f32 * 0.137).sin() * 7.13);
            let previous = std::array::from_fn(|lane| 0.97 - lane as f32 * 0.133);
            let scales = std::array::from_fn(|lane| 0.71 - lane as f32 * 0.117);
            let lfo = (frame as f32 * 0.013).sin();
            let expected = std::array::from_fn::<_, 6, _>(|to| {
                let modulation = (routed[to] + feedback[to] * previous[to] + lfo) * scales[to];
                phases[to] + modulation / std::f32::consts::TAU
            });
            modulate_phases(&mut phases, &feedback, &routed, &previous, &scales, lfo);
            assert_eq!(
                phases[..6]
                    .iter()
                    .copied()
                    .map(f32::to_bits)
                    .collect::<Vec<_>>(),
                expected.map(f32::to_bits)
            );
        }
    }
    #[test]
    fn operator_gain_lanes_match_scalar_order_and_guard_bits() {
        for frame in 0..1024 {
            let samples = std::array::from_fn(|lane| match frame {
                0 => -0.0,
                1 => f32::NAN,
                2 => f32::INFINITY,
                3 => f32::NEG_INFINITY,
                _ => ((frame * 13 + lane * 23) as f32 * 0.371).sin() * 7.13,
            });
            let levels = std::array::from_fn(|lane| 1.0 - lane as f32 * 0.077);
            let envelopes = std::array::from_fn(|lane| 0.97 - lane as f32 * 0.133);
            let gains = std::array::from_fn(|lane| 3.71 - lane as f32 * 0.173);
            let expected = std::array::from_fn::<_, 6, _>(|to| {
                core::guard(samples[to] * levels[to] * envelopes[to] * gains[to])
            });
            assert_eq!(
                operator_gains(&samples, &levels, &envelopes, &gains).map(f32::to_bits),
                expected.map(f32::to_bits)
            );
        }
    }
    #[test]
    fn wrapped_sine_lanes_match_scalar_floor_and_polynomial_bits() {
        for frame in 0..16384 {
            let mut phases = std::array::from_fn(|lane| {
                if frame == 0 {
                    -0.0
                } else {
                    (frame * 31 + lane * 17) as f32 / 128.0 - 2048.0
                }
            });
            let expected =
                phases.map(|phase| crate::blocks::oscillator::sine_fast(phase - phase.floor()));
            wrapped_sines(&mut phases);
            assert_eq!(phases.map(f32::to_bits), expected.map(f32::to_bits));
        }
    }
    #[test]
    fn dense_route_and_ring_lanes_match_ordered_scalar_bits() {
        let rows = std::array::from_fn(|to| {
            std::array::from_fn(|source| 0.01 + (to * 6 + source) as f32 * 0.023)
        });
        let columns = route_columns(&rows);
        for previous in [[0.0; 6], [-0.0; 6], [0.0, -0.0, 0.0, -0.0, 0.0, -0.0]] {
            let sums = dense_routes(&columns, &previous, None);
            for to in 0..6 {
                assert_eq!(
                    sums[to].to_bits(),
                    route_sum(&rows[to], &previous, 0x3f).to_bits()
                );
            }
        }
        for frame in 0..1024 {
            let previous = std::array::from_fn(|source| {
                ((frame * 17 + source * 13) as f32 * 0.071).sin() * 1.317
            });
            let carriers = std::array::from_fn(|lane| (frame as f32 * 0.137 + lane as f32).cos());
            let sums = dense_routes(&columns, &previous, None);
            let products = dense_routes(&columns, &previous, Some(&carriers));
            for to in 0..6 {
                assert_eq!(
                    sums[to].to_bits(),
                    route_sum(&rows[to], &previous, 0x3f).to_bits()
                );
                let factors = std::array::from_fn(|source| {
                    1.0 - rows[to][source] + rows[to][source] * previous[source]
                });
                assert_eq!(
                    products[to].to_bits(),
                    ring_product(carriers[to], &factors, 0x3f).to_bits()
                );
            }
        }
    }
    #[test]
    fn moving_pitch_metadata_matches_original_ordered_carson_scan_bits() {
        let ops = std::array::from_fn(|i| MatrixOperator {
            ratio: 0.73 + i as f32 * 1.317,
            detune_cents: -47.0 + i as f32 * 17.3,
            level: 1.0 - i as f32 * 0.061,
            feedback: 6.0 - i as f32 * 0.271,
            key_scale: -1.7 + i as f32 * 0.61,
            ..Default::default()
        });
        for rate in [16000.0, 96000.0, 384000.0] {
            for sparse in [false, true] {
                let fm = std::array::from_fn(|to| {
                    std::array::from_fn(|source| {
                        if sparse && (to + source) % 3 == 0 {
                            0.0
                        } else {
                            0.1 + (to * 6 + source) as f32 * 0.17
                        }
                    })
                });
                let ring = std::array::from_fn(|to| {
                    std::array::from_fn(|source| 0.1 + (to * 6 + source) as f32 * 0.01)
                });
                let mut tone = Tone::default();
                tone.levels = [1.0; 6];
                for frame in 0..257 {
                    let pitch = -70.0 + frame as f32 * 1.17;
                    let context = Context {
                        operator_graph: None,
                        pitch,
                        key: (frame % 128) as u8,
                        held: true,
                        slot: 0,
                        lfo: 0.3,
                        cutoff: 1300.0,
                        rate,
                        bpm: 120.0,
                        assets: None,
                    };
                    matrix(&mut tone, &context, &ops, &fm, &ring, 0, pitch, 1.37);
                    for to in 0..6 {
                        let inc = core::increment(
                            pitch + 12.0 * ops[to].ratio.log2() + ops[to].detune_cents / 100.0,
                            rate,
                        );
                        let mut highest = inc * rate;
                        let mut requested = ops[to].feedback * ops[to].level * 4.0 + 1.37;
                        for source in 0..6 {
                            if fm[to][source] > 0.0 {
                                highest = highest.max(
                                    core::increment(pitch + 12.0 * ops[source].ratio.log2(), rate)
                                        * rate,
                                );
                                requested += fm[to][source] * ops[source].level * 4.0;
                            }
                        }
                        let budget = ((rate * 0.24 - inc * rate) / highest.max(1.0) - 1.0).max(0.0);
                        let scale = if requested > 0.0 {
                            (budget / requested).min(1.0)
                        } else {
                            1.0
                        };
                        assert_eq!(tone.matrix_cache[0].increments[to].to_bits(), inc.to_bits());
                        assert_eq!(tone.matrix_cache[0].scales[to].to_bits(), scale.to_bits());
                    }
                }
            }
        }
    }
}
