//! A bank of sine partials: the oscillator five of the six instruments use.
//!
//! A spectrum is a list of partials, each with a frequency as a ratio of
//! the note's pitch, a level and a starting phase. The bank plays one sine
//! per partial and adds them up. Nothing about it assumes the ratios are
//! whole numbers, which is what lets the same bank play a harmonic stack
//! and a struck metal bar.

use crate::blocks::CONTROL_PERIOD;
use crate::blocks::math::clean;

use super::engine::{MAX_PARTIALS, Tone, sine_turns};

/// Smallest and largest frequency ratio a partial may ask for. A ratio
/// outside this is damage, not a sound.
const MIN_RATIO: f32 = 1.0e-3;
const MAX_RATIO: f32 = 1.0e4;

/// Largest level a partial may ask for.
const MAX_GAIN: f32 = 4.0;

/// Phase step per sample where a partial starts to fade out, and where it
/// is gone. Half a cycle per sample is the Nyquist frequency; a partial
/// taken right up to it would alias instead of ringing, so the top of the
/// range is given over to a fade rather than a cliff, which keeps a pitch
/// change from clicking as partials come and go.
const FADE_FROM: f32 = 0.44;
const FADE_TO: f32 = 0.49;

/// The partials an additive instrument wants sounded.
///
/// An instrument fills one of these from its parameters with
/// [`Spectrum::push`], which is also where a value that could not be played
/// is forced into range, so no instrument has to guard its own arithmetic.
/// Whatever it does not fill is silent, and the bank plays the whole width
/// either way: a partial the instrument has stopped asking for fades out
/// instead of being cut off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Spectrum {
    /// How many partials the instrument filled. The bank does not read
    /// this; it is where the next [`Spectrum::push`] goes.
    filled: usize,
    ratios: [f32; MAX_PARTIALS],
    gains: [f32; MAX_PARTIALS],
    phases: [f32; MAX_PARTIALS],
}

impl Default for Spectrum {
    fn default() -> Self {
        Self::SILENT
    }
}

impl Spectrum {
    pub(super) const SILENT: Self = Self {
        filled: 0,
        ratios: [1.0; MAX_PARTIALS],
        gains: [0.0; MAX_PARTIALS],
        phases: [0.0; MAX_PARTIALS],
    };

    /// Adds one partial at `ratio` times the note's pitch, `gain` loud,
    /// starting `phase` of a cycle in. Anything that is not a number, or is
    /// outside what can be played, is forced into range. Partials past
    /// [`MAX_PARTIALS`] are dropped.
    pub(super) fn push(&mut self, ratio: f32, gain: f32, phase: f32) {
        if self.filled == MAX_PARTIALS {
            return;
        }
        self.ratios[self.filled] = clean(ratio, MIN_RATIO, MAX_RATIO, 1.0);
        self.gains[self.filled] = clean(gain, -MAX_GAIN, MAX_GAIN, 0.0);
        self.phases[self.filled] = clean(phase, 0.0, 1.0, 0.0).fract();
        self.filled += 1;
    }

    /// The frequency ratios of the partials that were asked for.
    #[cfg(test)]
    pub(super) fn ratios(&self) -> &[f32] {
        &self.ratios[..self.filled]
    }

    /// The levels of the partials that were asked for.
    #[cfg(test)]
    pub(super) fn gains(&self) -> &[f32] {
        &self.gains[..self.filled]
    }
}

/// A spectrum as a voice reads it: the levels the control period starts
/// from, together with the change each one makes per sample.
#[derive(Clone, Copy)]
pub(super) struct PartialSetup {
    ratios: [f32; MAX_PARTIALS],
    gains: [f32; MAX_PARTIALS],
    steps: [f32; MAX_PARTIALS],
    phases: [f32; MAX_PARTIALS],
}

/// A voice's bank of sine partials. All it remembers is where each one has
/// got to in its cycle.
#[derive(Clone, Copy)]
pub(super) struct PartialTone {
    phases: [f32; MAX_PARTIALS],
}

impl Tone for PartialTone {
    type Target = Spectrum;
    type Setup = PartialSetup;

    fn silent() -> Self {
        Self {
            phases: [0.0; MAX_PARTIALS],
        }
    }

    fn settled(target: &Spectrum) -> PartialSetup {
        PartialSetup {
            ratios: target.ratios,
            gains: target.gains,
            steps: [0.0; MAX_PARTIALS],
            phases: target.phases,
        }
    }

    fn restart(&mut self, setup: &PartialSetup) {
        self.phases = setup.phases;
    }

    fn glide(setup: &mut PartialSetup, target: &Spectrum, remaining: u32) {
        let span = CONTROL_PERIOD as f32;
        for index in 0..MAX_PARTIALS {
            // Where the period that has just ended left the level.
            let now = setup.gains[index] + setup.steps[index] * span;
            setup.gains[index] = now;
            setup.steps[index] = if remaining > 0 {
                (target.gains[index] - now) / (remaining as f32 * span)
            } else {
                0.0
            };
            // A ratio is a retuning rather than a level, so it takes effect
            // at the period boundary. A partial on its way out keeps the
            // pitch it had, so that dropping one does not slide it.
            if target.gains[index] != 0.0 {
                setup.ratios[index] = target.ratios[index];
            }
        }
        setup.phases = target.phases;
    }

    fn render(&mut self, setup: &PartialSetup, increment: f32, offset: usize, out: &mut [f32]) {
        let frames = out.len();
        for index in 0..MAX_PARTIALS {
            let step_per_sample = increment * setup.ratios[index];
            let fade = nyquist_fade(step_per_sample);
            let level_step = setup.steps[index] * fade;
            let mut level = (setup.gains[index] + setup.steps[index] * offset as f32) * fade;
            if level == 0.0 && level_step == 0.0 {
                // A silent partial still moves on, so that it is in the
                // right place in its cycle if its level comes back up.
                self.phases[index] =
                    (self.phases[index] + step_per_sample * frames as f32).rem_euclid(1.0);
                continue;
            }
            // Every partial the fade has left audible steps less than a
            // cycle per sample, so one subtraction always brings the phase
            // back round.
            let mut phase = self.phases[index];
            #[cfg(target_arch = "x86_64")]
            let remainder = {
                let (groups, remainder) = out.as_chunks_mut::<4>();
                for samples in groups {
                    let mut phases = [0.0; 4];
                    let mut levels = [0.0; 4];
                    // Preserve the original f32 recurrences, including wrap
                    // and level rounding, before evaluating four independent
                    // output positions in SIMD. Partial reduction order stays.
                    for lane in 0..4 {
                        phases[lane] = phase;
                        levels[lane] = level;
                        level += level_step;
                        phase += step_per_sample;
                        if phase >= 1.0 {
                            phase -= 1.0;
                        }
                    }
                    add_four(samples, &phases, &levels);
                }
                remainder
            };
            #[cfg(not(target_arch = "x86_64"))]
            let remainder = &mut *out;
            for sample in remainder {
                *sample += sine_turns(phase) * level;
                level += level_step;
                phase += step_per_sample;
                if phase >= 1.0 {
                    phase -= 1.0;
                }
            }
            self.phases[index] = phase;
        }
    }
}

/// Four independent time positions, with the scalar sine's exact arithmetic.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
fn add_four(out: &mut [f32], phases: &[f32; 4], levels: &[f32; 4]) {
    use super::engine::{SINE_C3, SINE_C5, SINE_C7};
    use std::arch::x86_64::*;
    // SAFETY: SSE2 is baseline on x86_64. Inputs contain four initialized
    // values and the caller supplies exactly four writable output positions.
    unsafe {
        let half = _mm_set1_ps(0.5);
        let phase = _mm_loadu_ps(phases.as_ptr());
        let first = _mm_cmplt_ps(phase, half);
        let mut quarter = _mm_or_ps(
            _mm_and_ps(first, phase),
            _mm_andnot_ps(first, _mm_sub_ps(phase, half)),
        );
        let sign = _mm_or_ps(
            _mm_and_ps(first, _mm_set1_ps(1.0)),
            _mm_andnot_ps(first, _mm_set1_ps(-1.0)),
        );
        let mirror = _mm_cmpgt_ps(quarter, _mm_set1_ps(0.25));
        quarter = _mm_or_ps(
            _mm_and_ps(mirror, _mm_sub_ps(half, quarter)),
            _mm_andnot_ps(mirror, quarter),
        );
        let y = _mm_mul_ps(_mm_set1_ps(std::f32::consts::TAU), quarter);
        let square = _mm_mul_ps(y, y);
        let inner = _mm_add_ps(
            _mm_set1_ps(SINE_C5),
            _mm_mul_ps(square, _mm_set1_ps(SINE_C7)),
        );
        let inner = _mm_add_ps(_mm_set1_ps(SINE_C3), _mm_mul_ps(square, inner));
        let polynomial = _mm_add_ps(_mm_set1_ps(1.0), _mm_mul_ps(square, inner));
        let sine = _mm_mul_ps(_mm_mul_ps(sign, y), polynomial);
        let sample = _mm_mul_ps(sine, _mm_loadu_ps(levels.as_ptr()));
        _mm_storeu_ps(
            out.as_mut_ptr(),
            _mm_add_ps(_mm_loadu_ps(out.as_ptr()), sample),
        );
    }
}

#[cfg(all(test, target_arch = "x86_64"))]
mod vector_tests {
    use super::*;
    #[test]
    fn packed_time_positions_preserve_scalar_polynomial_and_addition_bits() {
        let cases = [
            0.0, -0.0, 0.001, 0.24999999, 0.25, 0.49999997, 0.5, 0.75, 0.99999994, 1.0,
        ];
        for start in 0..cases.len() {
            let phases = std::array::from_fn(|lane| cases[(start + lane) % cases.len()]);
            let levels = [0.7, -0.3, 0.0, 1.4];
            let mut actual = [-0.0, 0.3, -0.1, 0.0];
            let mut expected = actual;
            for lane in 0..4 {
                expected[lane] += sine_turns(phases[lane]) * levels[lane];
            }
            add_four(&mut actual, &phases, &levels);
            assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
        }
    }
    #[test]
    fn packed_partial_render_preserves_ordered_pcm_and_phase_bits_across_partitions() {
        let setup = PartialSetup {
            ratios: std::array::from_fn(|i| 1.0 + i as f32 * 0.61),
            gains: std::array::from_fn(|i| 0.4 + i as f32 * 0.03),
            steps: std::array::from_fn(|i| (i as f32 % 7.0 - 3.0) * 0.001),
            phases: std::array::from_fn(|i| i as f32 * 0.013),
        };
        let mut actual = PartialTone {
            phases: setup.phases,
        };
        let mut reference = actual;
        for frames in [1, 7, 64, 3, 128] {
            let increment = 277.0 / 48000.0;
            let mut found = vec![0.1; frames];
            let mut expected = found.clone();
            for index in 0..MAX_PARTIALS {
                let step = increment * setup.ratios[index];
                let fade = nyquist_fade(step);
                let delta = setup.steps[index] * fade;
                let mut level = setup.gains[index] * fade;
                let mut phase = reference.phases[index];
                for sample in &mut expected {
                    *sample += sine_turns(phase) * level;
                    level += delta;
                    phase += step;
                    if phase >= 1.0 {
                        phase -= 1.0;
                    }
                }
                reference.phases[index] = phase;
            }
            actual.render(&setup, increment, 0, &mut found);
            assert_eq!(
                found.iter().map(|x| x.to_bits()).collect::<Vec<_>>(),
                expected.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
            );
            assert_eq!(
                actual.phases.map(f32::to_bits),
                reference.phases.map(f32::to_bits)
            );
        }
    }
}

/// How much of a partial taking `increment` cycles per sample is left: all
/// of it below [`FADE_FROM`], none of it above [`FADE_TO`], and a smooth
/// step between.
#[inline]
fn nyquist_fade(increment: f32) -> f32 {
    if !increment.is_finite() || increment >= FADE_TO {
        return 0.0;
    }
    if increment <= FADE_FROM {
        return 1.0;
    }
    let left = (FADE_TO - increment) / (FADE_TO - FADE_FROM);
    left * left * (3.0 - 2.0 * left)
}
