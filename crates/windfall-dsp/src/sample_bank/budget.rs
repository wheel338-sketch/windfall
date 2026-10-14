//! Control-side reservations include unpublished jobs and retained generations.
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub const SAMPLE_BANK_TOTAL_BYTES: usize = 256 * 1024 * 1024;

#[derive(Debug)]
pub struct BankBudget {
    limit: usize,
    used: AtomicUsize,
}
impl Default for BankBudget {
    fn default() -> Self {
        Self::new(SAMPLE_BANK_TOTAL_BYTES)
    }
}
impl BankBudget {
    pub fn shared() -> Arc<Self> {
        static BUDGET: std::sync::OnceLock<Arc<BankBudget>> = std::sync::OnceLock::new();
        BUDGET.get_or_init(|| Arc::new(Self::default())).clone()
    }
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.min(SAMPLE_BANK_TOTAL_BYTES),
            used: AtomicUsize::new(0),
        }
    }
    pub fn remaining(&self) -> usize {
        self.limit.saturating_sub(self.used())
    }
    pub fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
    pub fn reserve(self: &Arc<Self>, bytes: usize) -> Result<BankReservation, String> {
        let mut used = self.used();
        loop {
            if bytes > self.limit.saturating_sub(used) {
                return Err(windfall_core::notice!(
                    "dsp.sampleBankMemory",
                    bytes = bytes,
                    used = used,
                    limit = self.limit
                )
                .to_string());
            }
            match self.used.compare_exchange_weak(
                used,
                used + bytes,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(BankReservation {
                        budget: self.clone(),
                        bytes,
                    });
                }
                Err(now) => used = now,
            }
        }
    }
}

#[derive(Debug)]
pub struct BankReservation {
    budget: Arc<BankBudget>,
    bytes: usize,
}
impl BankReservation {
    /// Release decoder workspace after its PCM has reached its final capacity.
    /// Control side only; may never increase an already admitted reservation.
    pub fn shrink(&mut self, bytes: usize) -> Result<(), String> {
        if bytes > self.bytes {
            return Err(
                "Sample-bank memory refused: decoder capacity exceeds its admitted workspace"
                    .into(),
            );
        }
        self.budget
            .used
            .fetch_sub(self.bytes - bytes, Ordering::AcqRel);
        self.bytes = bytes;
        Ok(())
    }
}
impl Drop for BankReservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
