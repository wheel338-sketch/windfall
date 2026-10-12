//! Two things the host owes effects that can ring forever or that keep very
//! large histories: sums of tails that cannot overflow, and an admission of
//! their memory before anything is prepared. Both are for the echo bank and
//! the sixteen-band frequency delay; see docs/DELAY-FAMILY.md.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use windfall_dsp::echo_bank::{PreparationBudget, PreparationError};
use windfall_project::EffectKind;

use crate::plan::Plan;
use crate::project_preparation::ProjectPreparationError;

/// The most echo history one controller may keep, in bytes: 1 GiB. An echo bank
/// keeps 77 MB at 48 kHz and 614 MB at 384 kHz, so this is thirteen banks at
/// the usual rate and one at the highest. Nothing is shortened to fit: a
/// plan that needs more is refused, and what was playing carries on.
pub(crate) const ECHO_HISTORY_LIMIT: usize = 1 << 30;

/// Room for the fixed staging headers a preparation holds while it swaps its
/// histories in, which count toward its peak and not toward what it keeps.
const STAGING_HEADROOM: usize = 64 * 1024;

/// Shared across every publication/attachment of a controller, including
/// speculative candidates and states waiting in the retirement queue.
/// Only preparation and control-side owner destruction change the count.
#[derive(Debug)]
pub(crate) struct HistoryPool {
    occupied: AtomicUsize,
    retained_limit: usize,
    peak_limit: usize,
}

impl Default for HistoryPool {
    fn default() -> Self {
        Self::with_limits(ECHO_HISTORY_LIMIT, ECHO_HISTORY_LIMIT + STAGING_HEADROOM)
    }
}

impl HistoryPool {
    #[cfg(test)]
    pub fn occupied(&self) -> usize {
        self.occupied.load(Ordering::Acquire)
    }

    pub fn with_limits(retained_limit: usize, peak_limit: usize) -> Self {
        Self {
            occupied: AtomicUsize::new(0),
            retained_limit,
            peak_limit,
        }
    }

    /// Reserves ALL new payload before constructing any replacement effect.
    /// Concurrent preparations also charge their fixed staging headroom.
    /// Existing staging is conservatively counted in the retained check.
    pub fn reserve(
        self: &Arc<Self>,
        bytes: usize,
    ) -> Result<HistoryReservation, ProjectPreparationError> {
        // Reused owners require no new memory, even while another worker holds
        // transient staging above the retained ceiling.
        if bytes == 0 {
            return Ok(HistoryReservation {
                pool: self.clone(),
                charge: 0,
                payload: 0,
            });
        }
        let charge = bytes
            .checked_add(STAGING_HEADROOM)
            .ok_or(ProjectPreparationError::SizeOverflow)?;
        let mut occupied = self.occupied.load(Ordering::Acquire);
        loop {
            let retained = occupied
                .checked_add(bytes)
                .ok_or(ProjectPreparationError::SizeOverflow)?;
            if retained > self.retained_limit {
                return Err(ProjectPreparationError::MemoryLimit {
                    required: retained,
                    limit: self.retained_limit,
                });
            }
            let peak = occupied
                .checked_add(charge)
                .ok_or(ProjectPreparationError::SizeOverflow)?;
            if peak > self.peak_limit {
                return Err(ProjectPreparationError::MemoryLimit {
                    required: peak,
                    limit: self.peak_limit,
                });
            }
            match self.occupied.compare_exchange_weak(
                occupied,
                peak,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(HistoryReservation {
                        pool: self.clone(),
                        charge,
                        payload: bytes,
                    });
                }
                Err(current) => occupied = current,
            }
        }
    }
}

/// Attached to the concrete prepared EffectUnit, never to a plan/ledger clone.
/// Moving an owner during callback adoption moves its charge with it. Its Drop
/// runs only when that owner is destroyed on the control side, after its PCM
/// storage is freed. An unbuilt batch returns its payload and staging on failure.
pub(crate) struct HistoryReservation {
    pool: Arc<HistoryPool>,
    charge: usize,
    payload: usize,
}

impl HistoryReservation {
    /// Split without changing the global count. Called only during preparation.
    pub fn take(&mut self, bytes: usize) -> Result<Self, ProjectPreparationError> {
        if bytes > self.payload {
            return Err(ProjectPreparationError::Unsupported(
                "Echo owner reuse changed after memory admission; the project keeps playing as it was.",
            ));
        }
        self.payload -= bytes;
        self.charge -= bytes;
        Ok(Self {
            pool: self.pool.clone(),
            charge: bytes,
            payload: bytes,
        })
    }
}

impl Drop for HistoryReservation {
    fn drop(&mut self) {
        let before = self.pool.occupied.fetch_sub(self.charge, Ordering::AcqRel);
        debug_assert!(before >= self.charge, "echo history reservation underflow");
    }
}

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
    /// are only there to fade out are included. The shared pool separately
    /// charges actual owners until retirement, even if a plan omits them.
    pub fn of(plan: &Plan, sample_rate: u32) -> Result<Self, ProjectPreparationError> {
        let kinds = plan
            .tracks
            .iter()
            .flat_map(|track| &track.effects)
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
                debug_assert!(
                    bytes <= self.reserved,
                    "an effect outside the admitted plan"
                );
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
    fn shared_history_charges_installed_fading_and_retiring_owners_until_drop() {
        let pool = Arc::new(HistoryPool::with_limits(100, 100 + STAGING_HEADROOM));
        let mut first = pool.reserve(60).unwrap();
        let installed = first.take(60).unwrap();
        drop(first); // staging ends, the concrete owner's payload remains
        assert_eq!(pool.occupied(), 60);
        let mut replacement = pool.reserve(40).unwrap();
        let incoming = replacement.take(40).unwrap();
        drop(replacement);
        let fading = installed; // callback adoption only moves ownership
        assert_eq!(pool.occupied(), 100);
        let retirement = vec![fading]; // control-side carrier has not dropped it
        assert!(matches!(
            pool.reserve(1),
            Err(ProjectPreparationError::MemoryLimit {
                required: 101,
                limit: 100
            })
        ));
        assert_eq!(pool.occupied(), 100);
        drop(retirement);
        assert_eq!(pool.occupied(), 40);
        assert!(pool.reserve(60).is_ok());
        drop(incoming);
        assert_eq!(pool.occupied(), 0);
    }

    #[test]
    fn shared_history_peak_refusal_and_cancelled_preparation_return_every_charge() {
        let pool = Arc::new(HistoryPool::with_limits(100, 100 + STAGING_HEADROOM - 1));
        assert!(matches!(pool.reserve(100),
            Err(ProjectPreparationError::MemoryLimit { required, limit })
                if required == 100 + STAGING_HEADROOM && limit == 100 + STAGING_HEADROOM - 1));
        assert_eq!(pool.occupied(), 0);
        let candidate = pool.reserve(99).unwrap();
        assert_eq!(pool.occupied(), 99 + STAGING_HEADROOM);
        // A second speculative build cannot ignore the first candidate's staging.
        assert!(pool.reserve(1).is_err());
        drop(candidate);
        assert_eq!(pool.occupied(), 0);
        assert!(pool.reserve(99).is_ok());
        assert!(matches!(
            pool.reserve(usize::MAX),
            Err(ProjectPreparationError::SizeOverflow)
        ));
        assert_eq!(pool.occupied(), 0);
    }

    #[test]
    fn leaving_plan_definitions_are_included_in_history_admission() {
        let mut plan = Plan::empty();
        let mut doc = windfall_project::Document::new(windfall_project::Project::new("leaving"));
        doc.dispatch(
            windfall_project::Command::AddEffect {
                track: windfall_project::TrackId::MASTER,
                kind: EffectKind::EchoBank,
                index: None,
            },
            None,
        )
        .unwrap();
        plan.tracks = crate::plan::compile(doc.project(), &crate::SamplePool::new()).tracks;
        plan.tracks[0].effects[0].leaving = true;
        assert_eq!(
            HistoryAdmission::of(&plan, 8_000).unwrap().reserved,
            EffectKind::EchoBank.history_bytes(8_000.0).unwrap()
        );
    }

    #[test]
    fn prepared_history_owner_reuse_is_charged_once_and_failed_build_releases_staging() {
        let rate = 8_000;
        for kind in [EffectKind::EchoBank, EffectKind::FrequencyDelay] {
            let bytes = kind.history_bytes(rate as f32).unwrap();
            let pool = Arc::new(HistoryPool::with_limits(bytes, bytes + STAGING_HEADROOM));
            let mut doc = windfall_project::Document::new(windfall_project::Project::new("reuse"));
            doc.dispatch(
                windfall_project::Command::AddEffect {
                    track: windfall_project::TrackId::MASTER,
                    kind,
                    index: None,
                },
                None,
            )
            .unwrap();
            let mut plan = crate::plan::compile(doc.project(), &crate::SamplePool::new());
            plan.echo_history = pool.clone();
            let (state, ledger) = crate::state::PlanState::try_build(&plan, rate, None).unwrap();
            assert_eq!(pool.occupied(), bytes);
            // Same concrete generation/kind, no new effect owner is constructed.
            let (reusing, _) =
                crate::state::PlanState::try_build(&plan, rate, Some(&ledger)).unwrap();
            assert_eq!(pool.occupied(), bytes);
            drop(reusing);
            let fresh = plan.for_attachment();
            assert!(
                matches!(crate::state::PlanState::try_build(&fresh, rate, None),
                Err(ProjectPreparationError::MemoryLimit { required, limit })
                    if required == 2 * bytes && limit == bytes)
            );
            assert_eq!(pool.occupied(), bytes);
            drop(state);
            // Metadata/ledger clones do not keep a destroyed owner charged.
            assert_eq!(pool.occupied(), 0);
            let (retry, _) = crate::state::PlanState::try_build(&fresh, rate, None).unwrap();
            assert_eq!(pool.occupied(), bytes);
            drop(retry);
            assert_eq!(pool.occupied(), 0);
        }
    }

    #[test]
    fn a_different_rate_candidate_cannot_escape_the_installed_history_pool() {
        let old_rate = 8_000;
        let new_rate = 16_000;
        let kind = EffectKind::FrequencyDelay;
        let old_bytes = kind.history_bytes(old_rate as f32).unwrap();
        let new_bytes = kind.history_bytes(new_rate as f32).unwrap();
        let limit = old_bytes + new_bytes - 1;
        let pool = Arc::new(HistoryPool::with_limits(limit, limit + STAGING_HEADROOM));
        let mut doc =
            windfall_project::Document::new(windfall_project::Project::new("clock change"));
        doc.dispatch(
            windfall_project::Command::AddEffect {
                track: windfall_project::TrackId::MASTER,
                kind,
                index: None,
            },
            None,
        )
        .unwrap();
        let mut plan = crate::plan::compile(doc.project(), &crate::SamplePool::new());
        plan.echo_history = pool.clone();
        let (state, ledger) = crate::state::PlanState::try_build(&plan, old_rate, None).unwrap();
        // A newly compiled plan has a fresh pool, but its old-rate ledger still
        // identifies the live controller's shared admission.
        let candidate = crate::plan::compile(doc.project(), &crate::SamplePool::new());
        assert!(
            matches!(crate::state::PlanState::try_build(&candidate, new_rate, Some(&ledger)),
            Err(ProjectPreparationError::MemoryLimit { required, limit: bound })
                if required == old_bytes + new_bytes && bound == limit)
        );
        assert_eq!(pool.occupied(), old_bytes);
        drop(state);
        assert_eq!(pool.occupied(), 0);
    }

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
        let pair = [
            EffectKind::EchoBank,
            EffectKind::Eq,
            EffectKind::FrequencyDelay,
        ];
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
        assert_eq!(
            admitted.budget(EffectKind::Delay),
            PreparationBudget::UNLIMITED
        );
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
