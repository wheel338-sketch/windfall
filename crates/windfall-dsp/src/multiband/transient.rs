use super::params::{TransientListen, TransientShaperParams, TransientSplitParams};
use crate::balance::{Controls, audio, rate};
use crate::blocks::envelope::Ballistics;
use crate::blocks::math::db_to_gain_exp;
use crate::effect::Effect;
use crate::param::ParamSet;

struct TransientDetector {
    fast: Ballistics,
    slow: Ballistics,
}
impl Default for TransientDetector {
    fn default() -> Self {
        let mut detector = Self {
            fast: Ballistics::new(0.0),
            slow: Ballistics::new(0.0),
        };
        detector.prepare(48_000.0);
        detector
    }
}
impl TransientDetector {
    fn prepare(&mut self, sample_rate: f32) {
        let rate = rate(sample_rate).max(8.0);
        self.fast.set_times(0.15, 5.0, rate);
        self.slow.set_times(15.0, 120.0, rate);
        self.reset();
    }
    fn reset(&mut self) {
        self.fast.snap(0.0);
        self.slow.snap(0.0);
    }
    fn tick(&mut self, input: [f32; 2], sensitivity: f32) -> f32 {
        let peak = input[0].abs().max(input[1].abs());
        let fast = self.fast.tick(peak);
        // Follow the fast envelope rather than the rectified carrier. This
        // makes a steady tone converge to zero contrast instead of mistaking
        // differing peak-follower duty cycles for repeated attacks.
        let slow = self.slow.tick(fast);
        (((fast - slow).max(0.0) / fast.max(1.0e-6)) * sensitivity).clamp(0.0, 1.0)
    }
}

/// Fast/slow stereo-linked envelope contrast controls attack and body gain.
pub struct TransientShaper {
    detector: TransientDetector,
    controls: Controls<3>,
    /// How much of the attack part and of the sustain part is let out.
    listen: Controls<2>,
}
impl Default for TransientShaper {
    fn default() -> Self {
        Self {
            detector: TransientDetector::default(),
            controls: Controls::new([0.0, 0.0, 1.0]),
            listen: Controls::new([1.0, 1.0]),
        }
    }
}
impl TransientShaper {
    /// One shaped frame and the share of it, 0 to 1, that is attack.
    fn shape(&mut self, input: [f32; 2]) -> ([f32; 2], f32) {
        let input = input.map(audio);
        let [attack, sustain, sensitivity] = self.controls.tick();
        let transient = self.detector.tick(input, sensitivity);
        let gain = db_to_gain_exp(attack * transient + sustain * (1.0 - transient));
        (input.map(|x| x * gain), transient)
    }

    /// Separate-output seam for a host that routes the two parts apart:
    /// `[attack, sustain]`, each a stereo frame of the shaped sound. They
    /// add up to what `process` puts out with both parts let out. The
    /// `listen` setting is not applied here; call this or `process`, not
    /// both, for any one frame.
    pub fn split_frame(&mut self, input: [f32; 2]) -> [[f32; 2]; 2] {
        let (shaped, transient) = self.shape(input);
        let _ = self.listen.tick();
        let attack = shaped.map(|x| x * transient);
        [
            attack.map(audio),
            std::array::from_fn(|c| audio(shaped[c] - attack[c])),
        ]
    }
}
impl Effect for TransientShaper {
    type Params = TransientShaperParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.detector.prepare(sample_rate);
        self.controls.prepare(sample_rate);
        self.listen.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.detector.reset();
        self.controls.reset();
        self.listen.reset();
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.controls
            .set([18.0 * p.attack, 18.0 * p.sustain, p.sensitivity]);
        self.listen.set(match p.listen {
            TransientListen::Both => [1.0, 1.0],
            TransientListen::Attack => [1.0, 0.0],
            TransientListen::Sustain => [0.0, 1.0],
        });
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let (shaped, transient) = self.shape([*l, *r]);
            let [attack, sustain] = self.listen.tick();
            // With both parts let out the shares add up to one by
            // definition, and the shaped frame goes out as it is.
            let share = if attack == 1.0 && sustain == 1.0 {
                1.0
            } else {
                attack * transient + sustain * (1.0 - transient)
            };
            *l = audio(shaped[0] * share);
            *r = audio(shaped[1] * share);
        }
    }
}

/// Complementary time-varying separation, with independent output gains.
/// This is the `fx-transmitter` behavior, not a frequency exciter.
pub struct TransientSplit {
    detector: TransientDetector,
    controls: Controls<3>,
}
impl Default for TransientSplit {
    fn default() -> Self {
        Self {
            detector: TransientDetector::default(),
            controls: Controls::new([1.0; 3]),
        }
    }
}
impl TransientSplit {
    /// Separate-output seam: [transient, sustain], each a stereo frame.
    /// At unity gains these sum to the input within floating-point tolerance.
    pub fn split_frame(&mut self, input: [f32; 2]) -> [[f32; 2]; 2] {
        let input = input.map(audio);
        let [tg, sg, sensitivity] = self.controls.tick();
        let mask = self.detector.tick(input, sensitivity);
        let transient = input.map(|x| x * mask);
        [
            transient.map(|x| audio(x * tg)),
            std::array::from_fn(|c| audio((input[c] - transient[c]) * sg)),
        ]
    }

    /// Block form of the separate-output seam, for a host that routes the
    /// parts to other tracks. A part flagged in `routed` (transient first,
    /// then sustain) leaves the main signal and is written to its own
    /// buffer; a part that is not routed stays in the main signal and its
    /// buffer is silent. With neither routed this is exactly `process`.
    /// Does not allocate.
    pub fn process_routed(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        routed: [bool; 2],
        transient: &mut [[f32; 2]],
        sustain: &mut [[f32; 2]],
    ) {
        let frames = left.iter_mut().zip(right.iter_mut());
        for ((l, r), (attack_out, body_out)) in frames.zip(transient.iter_mut().zip(sustain)) {
            let [attack, body] = self.split_frame([*l, *r]);
            let kept = |part: [f32; 2], away: bool| if away { [0.0; 2] } else { part };
            let (main_attack, main_body) = (kept(attack, routed[0]), kept(body, routed[1]));
            *attack_out = kept(attack, !routed[0]);
            *body_out = kept(body, !routed[1]);
            *l = audio(main_attack[0] + main_body[0]);
            *r = audio(main_attack[1] + main_body[1]);
        }
    }
}
impl Effect for TransientSplit {
    type Params = TransientSplitParams;
    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.detector.prepare(sample_rate);
        self.controls.prepare(sample_rate);
    }
    fn reset(&mut self) {
        self.detector.reset();
        self.controls.reset();
    }
    fn set_params(&mut self, params: &Self::Params) {
        let p = params.sanitized();
        self.controls.set([
            db_to_gain_exp(p.transient_gain_db),
            db_to_gain_exp(p.sustain_gain_db),
            p.sensitivity,
        ]);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right) {
            let [attack, sustain] = self.split_frame([*l, *r]);
            *l = audio(attack[0] + sustain[0]);
            *r = audio(attack[1] + sustain[1]);
        }
    }
}
