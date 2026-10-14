//! Control-side admission shared by the two delay processors.

/// Payload limits, excluding allocator bookkeeping and ordinary call-stack
/// overhead. Peak includes the live processor, both history sets and the
/// fixed staging array's headers. These limits do not guarantee allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PreparationBudget {
    pub retained_bytes: usize,
    pub peak_bytes: usize,
}
impl PreparationBudget {
    /// Compatibility policy for `Effect::prepare`; hosts should pass their
    /// actual admission limits to `try_prepare` instead.
    pub const UNLIMITED: Self = Self {
        retained_bytes: usize::MAX,
        peak_bytes: usize::MAX,
    };
}

/// Checked payload requirements for one atomic replacement at the sanitized
/// sample rate. Old histories are counted in peak, not retained, bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreparationRequirements {
    pub sample_rate: f32,
    pub retained_bytes: usize,
    pub peak_bytes: usize,
}

/// No strings, heap ownership or callback-side error destruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparationError {
    CapacityOverflow {
        maximum_delay_samples: usize,
        history_count: usize,
    },
    RetainedBudgetExceeded {
        required_bytes: usize,
        budget_bytes: usize,
    },
    PeakBudgetExceeded {
        required_bytes: usize,
        budget_bytes: usize,
    },
    /// Index is `2 * unit_or_band + channel` (left = 0, right = 1).
    /// The allocator refused this exact reservation; earlier staged buffers
    /// have already been retired off the audio thread when this is returned.
    ReservationFailed {
        history_index: usize,
        requested_bytes: usize,
    },
}
impl std::fmt::Display for PreparationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let notice = match self {
            Self::CapacityOverflow {
                maximum_delay_samples,
                history_count,
            } => windfall_core::notice!(
                "dsp.echo.capacityOverflow",
                histories = history_count,
                samples = maximum_delay_samples
            ),
            Self::RetainedBudgetExceeded {
                required_bytes,
                budget_bytes,
            } => windfall_core::notice!(
                "dsp.echo.retainedBudget",
                required = required_bytes,
                budget = budget_bytes
            ),
            Self::PeakBudgetExceeded {
                required_bytes,
                budget_bytes,
            } => windfall_core::notice!(
                "dsp.echo.peakBudget",
                required = required_bytes,
                budget = budget_bytes
            ),
            Self::ReservationFailed {
                history_index,
                requested_bytes,
            } => windfall_core::notice!(
                "dsp.echo.reservationFailed",
                history = history_index,
                requested = requested_bytes
            ),
        };
        write!(f, "{notice}")
    }
}
impl std::error::Error for PreparationError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreparationRefusal {
    /// The sanitized requested clock, which has NOT been installed.
    pub sample_rate: f32,
    pub error: PreparationError,
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreparationStatus {
    /// A refusal does not change whether a previous preparation is usable.
    pub is_prepared: bool,
    /// Latched until a successful preparation; reset/setters/process do not
    /// clear it. This also exposes refusal through legacy `Effect::prepare`.
    pub last_refusal: Option<PreparationRefusal>,
}

impl PreparationRequirements {
    pub(crate) fn checked<const N: usize>(
        sample_rate: f32,
        maximum: usize,
        inline: usize,
        old_history_bytes: usize,
    ) -> Result<Self, PreparationError> {
        let overflow = PreparationError::CapacityOverflow {
            maximum_delay_samples: maximum,
            history_count: N,
        };
        let samples = maximum.checked_add(1).ok_or(overflow)?;
        let per_history = samples.checked_mul(size_of::<f32>()).ok_or(overflow)?;
        // Vec's pointer offsets must fit isize even if usize arithmetic fits.
        if per_history > isize::MAX as usize {
            return Err(overflow);
        }
        let heap = per_history.checked_mul(N).ok_or(overflow)?;
        Self::with_heap::<N>(sample_rate, maximum, inline, old_history_bytes, heap)
    }
    pub(crate) fn with_heap<const N: usize>(
        sample_rate: f32,
        maximum: usize,
        inline: usize,
        old_history_bytes: usize,
        heap: usize,
    ) -> Result<Self, PreparationError> {
        let overflow = PreparationError::CapacityOverflow {
            maximum_delay_samples: maximum,
            history_count: N,
        };
        let retained_bytes = inline.checked_add(heap).ok_or(overflow)?;
        let headers = size_of::<super::support::History>()
            .checked_mul(N)
            .ok_or(overflow)?;
        let peak_bytes = retained_bytes
            .checked_add(old_history_bytes)
            .and_then(|n| n.checked_add(headers))
            .ok_or(overflow)?;
        Ok(Self {
            sample_rate,
            retained_bytes,
            peak_bytes,
        })
    }
    pub(crate) fn admit(self, budget: PreparationBudget) -> Result<(), PreparationError> {
        if self.retained_bytes > budget.retained_bytes {
            return Err(PreparationError::RetainedBudgetExceeded {
                required_bytes: self.retained_bytes,
                budget_bytes: budget.retained_bytes,
            });
        }
        if self.peak_bytes > budget.peak_bytes {
            return Err(PreparationError::PeakBudgetExceeded {
                required_bytes: self.peak_bytes,
                budget_bytes: budget.peak_bytes,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
