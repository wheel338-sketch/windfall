//! Preparation that can be refused, for the effects whose histories are
//! large enough that a host has to budget them: the echo bank and the
//! sixteen-band frequency delay. See docs/DELAY-FAMILY.md.
//!
//! Every other effect prepares as it always did and cannot be refused.

use super::{AnyEffect, EffectKind, EffectSlot};
use crate::blocks::delay_line::DelayLine;
use crate::blocks::tap_crossfade::TapCrossfade;
use crate::echo_bank::{
    EchoBank, PreparationBudget, PreparationError, PreparationRefusal, PreparationStatus,
};
use crate::frequency_delay::FrequencyDelay;

impl EffectKind {
    /// The bytes a prepared effect of this kind keeps at `sample_rate`, for
    /// the kinds a host has to admit before preparing them, and `None` for
    /// every other kind. The figure is the checked payload the fallible
    /// preparation asks for; it is `usize::MAX` where that cannot be
    /// counted, which no budget admits.
    pub fn history_bytes(self, sample_rate: f32) -> Option<usize> {
        match self {
            EffectKind::EchoBank => Some(EchoBank::preparation_bytes(sample_rate)),
            EffectKind::FrequencyDelay => Some(FrequencyDelay::preparation_bytes(sample_rate)),
            _ => None,
        }
    }
}

impl AnyEffect {
    /// [`AnyEffect::prepare`] within `budget`. An effect with large
    /// histories reserves all of them before it changes anything, so a
    /// refusal leaves it exactly as it was. Every other effect ignores the
    /// budget and always succeeds. Control side only.
    pub fn try_prepare(
        &mut self,
        sample_rate: f32,
        max_block: usize,
        budget: PreparationBudget,
    ) -> Result<(), PreparationError> {
        if let AnyEffect::EchoBank(effect) = self {
            return effect.try_prepare(sample_rate, max_block, budget);
        }
        if let AnyEffect::FrequencyDelay(effect) = self {
            return effect.try_prepare(sample_rate, max_block, budget);
        }
        self.prepare(sample_rate, max_block);
        Ok(())
    }

    /// Whether the effect is prepared and what its last preparation was
    /// refused for, for the kinds that can be refused.
    pub fn preparation_status(&self) -> Option<PreparationStatus> {
        match self {
            AnyEffect::EchoBank(effect) => Some(effect.preparation_status()),
            AnyEffect::FrequencyDelay(effect) => Some(effect.preparation_status()),
            _ => None,
        }
    }

    /// The refusal a preparation left behind, if it left one. This is how a
    /// caller of the infallible [`AnyEffect::prepare`] finds out.
    pub fn preparation_refusal(&self) -> Option<PreparationRefusal> {
        self.preparation_status()
            .and_then(|status| status.last_refusal)
    }
}

impl EffectSlot {
    /// [`EffectSlot::prepare`] within `budget`. On a refusal the slot is
    /// left as it was and must not be handed to the audio thread: drop it
    /// on the control side and keep what was playing.
    pub fn try_prepare(
        &mut self,
        sample_rate: f32,
        max_block: usize,
        budget: PreparationBudget,
    ) -> Result<(), PreparationError> {
        let max_block = max_block.max(1);
        let sample_rate = sample_rate.max(1.0);
        self.effect.try_prepare(sample_rate, max_block, budget)?;
        // From here on this is `EffectSlot::prepare`, which the effect's own
        // preparation was the only fallible part of. Keep the two in step.
        self.sample_rate = sample_rate;
        let latency = self.effect.max_latency_samples(self.sample_rate);
        self.dry_left = DelayLine::new(latency);
        self.dry_right = DelayLine::new(latency);
        self.dry_fade = TapCrossfade::new(latency, self.effect.latency_samples());
        self.scratch_left = vec![0.0; max_block].into_boxed_slice();
        self.scratch_right = vec![0.0; max_block].into_boxed_slice();
        self.dry_aligned = vec![false; max_block].into_boxed_slice();
        self.reset();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::EffectParams;
    use crate::echo_bank::EchoBankParams;

    const RATE: f32 = 8_000.0;

    fn slot(kind: EffectKind) -> EffectSlot {
        EffectSlot::new(AnyEffect::new(&kind.default_params()))
    }

    fn exact(kind: EffectKind) -> PreparationBudget {
        let bytes = kind.history_bytes(RATE).expect("an admitted kind");
        PreparationBudget {
            retained_bytes: bytes,
            // The staging headers of the largest kind are 1280 bytes.
            peak_bytes: bytes + 4_096,
        }
    }

    #[test]
    fn only_the_two_large_delay_kinds_ask_for_admission() {
        for kind in EffectKind::ALL {
            let wanted = matches!(kind, EffectKind::EchoBank | EffectKind::FrequencyDelay);
            assert_eq!(kind.history_bytes(RATE).is_some(), wanted, "{kind:?}");
            let effect = AnyEffect::new(&kind.default_params());
            assert_eq!(effect.preparation_status().is_some(), wanted, "{kind:?}");
            assert_eq!(effect.preparation_refusal(), None, "{kind:?}");
        }
        // Twenty-five seconds of sixteen histories against one second of
        // thirty-two: the bank is far the larger.
        let bank = EffectKind::EchoBank.history_bytes(RATE).unwrap();
        let bands = EffectKind::FrequencyDelay.history_bytes(RATE).unwrap();
        assert!(bank > 16 * 25 * 8_000 * 4 && bank < 16 * 26 * 8_000 * 4);
        assert!(bands > 32 * 8_000 * 4 && bands < 33 * 8_000 * 4 + 16_384);
    }

    #[test]
    fn a_slot_prepares_within_exactly_what_its_kind_asks_for() {
        for kind in [EffectKind::EchoBank, EffectKind::FrequencyDelay] {
            let mut slot = slot(kind);
            assert_eq!(slot.try_prepare(RATE, 64, exact(kind)), Ok(()));
            let status = slot.effect().preparation_status().unwrap();
            assert!(status.is_prepared);
            assert_eq!(status.last_refusal, None);
            // A prepared slot processes: its scratch space is there.
            let (mut left, mut right) = ([0.25_f32; 64], [-0.25_f32; 64]);
            slot.process(&mut left, &mut right);
            assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
        }
    }

    #[test]
    fn one_byte_too_few_is_refused_and_named() {
        for kind in [EffectKind::EchoBank, EffectKind::FrequencyDelay] {
            let mut budget = exact(kind);
            budget.retained_bytes -= 1;
            let mut slot = slot(kind);
            let refused = slot.try_prepare(RATE, 64, budget);
            assert_eq!(
                refused,
                Err(PreparationError::RetainedBudgetExceeded {
                    required_bytes: budget.retained_bytes + 1,
                    budget_bytes: budget.retained_bytes,
                }),
                "{kind:?}"
            );
            let status = slot.effect().preparation_status().unwrap();
            assert!(!status.is_prepared);
            assert_eq!(status.last_refusal.map(|refusal| refusal.error), refused.err());
            assert_eq!(slot.effect().preparation_refusal().unwrap().sample_rate, RATE);

            let mut peak = exact(kind);
            peak.peak_bytes = peak.retained_bytes;
            let mut slot = self::slot(kind);
            assert!(matches!(
                slot.try_prepare(RATE, 64, peak),
                Err(PreparationError::PeakBudgetExceeded { budget_bytes, .. })
                    if budget_bytes == peak.retained_bytes
            ));
        }
    }

    #[test]
    fn a_refused_replacement_leaves_a_prepared_slot_sounding_as_it_did() {
        let mut params = EchoBankParams::default();
        params.units[0].sync = false;
        params.units[0].time_ms = 5.0;
        let params = EffectParams::EchoBank(params);
        let build = || {
            let mut slot = EffectSlot::new(AnyEffect::new(&params));
            slot.try_prepare(RATE, 64, PreparationBudget::UNLIMITED)
                .expect("an unlimited budget admits");
            slot.set_params(&params);
            slot
        };
        let (mut kept, mut untouched) = (build(), build());
        let input: Vec<f32> = (0..512).map(|n| ((n * 37) % 101) as f32 / 101.0 - 0.5).collect();
        let run = |slot: &mut EffectSlot| {
            let (mut left, mut right) = (input.clone(), input.clone());
            for (left, right) in left.chunks_mut(64).zip(right.chunks_mut(64)) {
                slot.process(left, right);
            }
            (left, right)
        };
        assert_eq!(run(&mut kept), run(&mut untouched));
        let none = PreparationBudget {
            retained_bytes: 0,
            peak_bytes: 0,
        };
        assert!(kept.try_prepare(RATE, 64, none).is_err());
        // The refusal is on record and the audio carries on unchanged.
        assert!(kept.effect().preparation_refusal().is_some());
        assert!(kept.effect().preparation_status().unwrap().is_prepared);
        assert_eq!(run(&mut kept), run(&mut untouched));
    }

    #[test]
    fn every_other_kind_ignores_the_budget_and_matches_plain_preparation() {
        let none = PreparationBudget {
            retained_bytes: 0,
            peak_bytes: 0,
        };
        for kind in [EffectKind::Delay, EffectKind::Limiter, EffectKind::Room] {
            let params = kind.default_params();
            let mut tried = EffectSlot::new(AnyEffect::new(&params));
            assert_eq!(tried.try_prepare(RATE, 32, none), Ok(()));
            let mut plain = EffectSlot::new(AnyEffect::new(&params));
            plain.prepare(RATE, 32);
            assert_eq!(tried.latency_samples(), plain.latency_samples());
            assert_eq!(tried.tail_samples(), plain.tail_samples());
            let input: Vec<f32> = (0..96).map(|n| (n as f32 * 0.37).sin() * 0.5).collect();
            let (mut left, mut right) = (input.clone(), input.clone());
            let (mut plain_left, mut plain_right) = (input.clone(), input);
            tried.process(&mut left, &mut right);
            plain.process(&mut plain_left, &mut plain_right);
            assert_eq!(left, plain_left, "{kind:?}");
            assert_eq!(right, plain_right, "{kind:?}");
        }
    }
}
