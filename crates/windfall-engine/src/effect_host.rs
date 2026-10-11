//! Two things the host owes effects that can ring forever or that keep very
//! large histories: sums of tails that cannot overflow, and an admission of
//! their memory before anything is prepared. Both are for the echo bank and
//! the sixteen-band frequency delay; see docs/DELAY-FAMILY.md.

use windfall_dsp::echo_bank::{PreparationBudget, PreparationError};
use windfall_project::EffectKind;

use crate::plan::Plan;
use crate::project_preparation::ProjectPreparationError;

/// The most echo history one plan may keep, in bytes: 1 GiB. An echo bank
/// keeps 77 MB at 48 kHz and 614 MB at 384 kHz, so this is thirteen banks at
/// the usual rate and one at the highest. Nothing is shortened to fit: a
/// plan that needs more is refused, and what was playing carries on.
pub(crate) const ECHO_HISTORY_LIMIT: usize = 1 << 30;

/// Room for the fixed staging headers a preparation holds while it swaps its
/// histories in, which count toward its peak and not toward what it keeps.
const STAGING_HEADROOM: usize = 64 * 1024;

/// How long a chain can ring: the sum of its effects' tails. An effect whose
/// feedback sustains reports `usize::MAX`, and two of those, or one and any
/// other tail, must still add up to "without end" and not wrap round to a
/// short time.
pub(crate) fn total(tails: impl IntoIterator<Item = usize>) -> usize {
    tails.into_iter().fold(0, usize::saturating_add)
}

/// The frame by which sound that went in at `from` is over, `ring` frames of
/// tail and `held` frames of delay later. It is `u64::MAX`, never, for a
/// tail without end.
pub(crate) fn horizon(from: u64, ring: usize, held: u64) -> u64 {
    let ring = u64::try_from(ring).unwrap_or(u64::MAX);
    from.saturating_add(ring).saturating_add(held)
}

/// The echo history a plan was admitted for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HistoryAdmission {
    sample_rate: f32,
    /// What the plan's effects keep between them. At most the limit.
    pub reserved: usize,
}

impl HistoryAdmission {
    /// Reserves the history of every effect the plan runs against
    /// [`ECHO_HISTORY_LIMIT`], before any of them is prepared. Effects that
    /// are only there to fade out are not charged: their memory is already
    /// held and is on its way back.
    pub fn of(plan: &Plan, sample_rate: u32) -> Result<Self, ProjectPreparationError> {
        let kinds = plan
            .tracks
            .iter()
            .flat_map(|track| &track.effects)
            .filter(|effect| !effect.leaving)
            .map(|effect| effect.params.kind());
        Self::of_kinds(kinds, sample_rate, ECHO_HISTORY_LIMIT)
    }

    pub fn of_kinds(
        kinds: impl IntoIterator<Item = EffectKind>,
        sample_rate: u32,
        limit: usize,
    ) -> Result<Self, ProjectPreparationError> {
        let sample_rate = sample_rate.max(1) as f32;
        let mut reserved = 0_usize;
        for kind in kinds {
            let Some(bytes) = kind.history_bytes(sample_rate) else {
                continue;
            };
            // A size that cannot be counted is reported as the largest
            // number there is, and no sum with it stays under any limit.
            reserved = reserved
                .checked_add(bytes)
                .ok_or(ProjectPreparationError::SizeOverflow)?;
            if reserved > limit {
                return Err(ProjectPreparationError::MemoryLimit {
                    required: reserved,
                    limit,
                });
            }
        }
        Ok(Self {
            sample_rate,
            reserved,
        })
    }

    /// The limits one new effect is prepared within: the share of the
    /// reservation that is its own. Kinds that keep no large history get a
    /// budget nothing checks.
    pub fn budget(&self, kind: EffectKind) -> PreparationBudget {
        match kind.history_bytes(self.sample_rate) {
            Some(bytes) => {
                debug_assert!(bytes <= self.reserved, "an effect outside the admitted plan");
                PreparationBudget {
                    retained_bytes: bytes,
                    peak_bytes: bytes.saturating_add(STAGING_HEADROOM),
                }
            }
            None => PreparationBudget::UNLIMITED,
        }
    }
}

/// What a refused effect preparation is to the project: the plan is not
/// published and the reason reaches the user.
pub(crate) fn refusal(error: PreparationError) -> ProjectPreparationError {
    match error {
        PreparationError::CapacityOverflow { .. } => ProjectPreparationError::SizeOverflow,
        PreparationError::RetainedBudgetExceeded {
            required_bytes,
            budget_bytes,
        }
        | PreparationError::PeakBudgetExceeded {
            required_bytes,
            budget_bytes,
        } => ProjectPreparationError::MemoryLimit {
            required: required_bytes,
            limit: budget_bytes,
        },
        PreparationError::ReservationFailed { .. } => ProjectPreparationError::Unsupported(
            "There is not enough memory for an echo effect's history; the project keeps playing as it was.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails_without_end_add_up_to_without_end() {
        assert_eq!(total([]), 0);
        assert_eq!(total([480, 0, 96]), 576);
        assert_eq!(total([usize::MAX]), usize::MAX);
        assert_eq!(total([usize::MAX, 1]), usize::MAX);
        assert_eq!(total([7, usize::MAX, usize::MAX, 48_000]), usize::MAX);
    }

    #[test]
    fn a_horizon_never_wraps_round_to_an_early_frame() {
        assert_eq!(horizon(100, 480, 32), 612);
        assert_eq!(horizon(100, usize::MAX, 0), u64::MAX);
        assert_eq!(horizon(u64::MAX - 1, 480, 32), u64::MAX);
        assert_eq!(horizon(1, usize::MAX, u64::MAX), u64::MAX);
        // A track with such a tail has not rung out at any frame there is.
        assert!(horizon(1, total([usize::MAX, 9_600]), 0) > u64::MAX - 1);
    }

    #[test]
    fn a_plan_is_admitted_up_to_the_limit_and_refused_past_it() {
        let rate = 48_000;
        let bank = EffectKind::EchoBank.history_bytes(rate as f32).unwrap();
        let bands = EffectKind::FrequencyDelay
            .history_bytes(rate as f32)
            .unwrap();
        // Effects without large histories cost nothing.
        let free = [EffectKind::Delay, EffectKind::Reverb, EffectKind::Room];
        assert_eq!(
            HistoryAdmission::of_kinds(free, rate, 0).map(|admitted| admitted.reserved),
            Ok(0)
        );
        let pair = [EffectKind::EchoBank, EffectKind::Eq, EffectKind::FrequencyDelay];
        let admitted = HistoryAdmission::of_kinds(pair, rate, bank + bands).unwrap();
        assert_eq!(admitted.reserved, bank + bands);
        assert_eq!(
            HistoryAdmission::of_kinds(pair, rate, bank + bands - 1),
            Err(ProjectPreparationError::MemoryLimit {
                required: bank + bands,
                limit: bank + bands - 1,
            })
        );
        // Thirteen banks fit the real limit at 48 kHz and fourteen do not.
        let banks = |count| std::iter::repeat_n(EffectKind::EchoBank, count);
        assert!(HistoryAdmission::of_kinds(banks(13), rate, ECHO_HISTORY_LIMIT).is_ok());
        assert_eq!(
            HistoryAdmission::of_kinds(banks(14), rate, ECHO_HISTORY_LIMIT),
            Err(ProjectPreparationError::MemoryLimit {
                required: 14 * bank,
                limit: ECHO_HISTORY_LIMIT,
            })
        );
        // The highest rate is not clamped to make two banks fit.
        assert!(HistoryAdmission::of_kinds(banks(1), 384_000, ECHO_HISTORY_LIMIT).is_ok());
        assert!(HistoryAdmission::of_kinds(banks(2), 384_000, ECHO_HISTORY_LIMIT).is_err());
    }

    #[test]
    fn each_new_effect_is_prepared_within_its_own_share() {
        let admitted =
            HistoryAdmission::of_kinds([EffectKind::EchoBank], 48_000, ECHO_HISTORY_LIMIT).unwrap();
        let bank = EffectKind::EchoBank.history_bytes(48_000.0).unwrap();
        let budget = admitted.budget(EffectKind::EchoBank);
        assert_eq!(budget.retained_bytes, bank);
        assert_eq!(budget.peak_bytes, bank + STAGING_HEADROOM);
        assert_eq!(admitted.budget(EffectKind::Delay), PreparationBudget::UNLIMITED);
    }

    #[test]
    fn a_refused_preparation_becomes_a_project_refusal() {
        assert_eq!(
            refusal(PreparationError::RetainedBudgetExceeded {
                required_bytes: 9,
                budget_bytes: 8,
            }),
            ProjectPreparationError::MemoryLimit {
                required: 9,
                limit: 8,
            }
        );
        assert_eq!(
            refusal(PreparationError::CapacityOverflow {
                maximum_delay_samples: 1,
                history_count: 16,
            }),
            ProjectPreparationError::SizeOverflow
        );
        assert!(matches!(
            refusal(PreparationError::ReservationFailed {
                history_index: 3,
                requested_bytes: 64,
            }),
            ProjectPreparationError::Unsupported(_)
        ));
    }
}
