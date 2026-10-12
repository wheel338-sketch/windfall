use crate::{AnalysisError, Result, add};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

#[derive(Clone, Default, Debug)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Cooperative control-side work accounting. A checkpoint charges at least
/// one unit; callers must checkpoint between bounded chunks. This is not a
/// process sandbox or a way to preempt an uncooperative native adapter/IO call.
pub struct Work {
    cancel: CancelToken,
    deadline: Instant,
    maximum: u64,
    completed: u64,
    memory_limit: u64,
    retained_memory: u64,
    peak_memory: u64,
    pub(crate) progress: Option<Arc<dyn Fn(u64) -> Result<()> + Send + Sync>>,
}

impl Work {
    pub fn new(cancel: CancelToken, maximum: u64, timeout: Duration) -> Result<Self> {
        if maximum == 0 || maximum > u64::MAX - 32 || timeout.is_zero() {
            return Err(AnalysisError::Invalid("work/deadline bounds"));
        }
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(AnalysisError::Exhausted)?;
        Ok(Self {
            cancel,
            deadline,
            maximum,
            completed: 0,
            memory_limit: crate::classical_memory::CLASSICAL_MEMORY_BYTES,
            retained_memory: 0,
            peak_memory: 0,
            progress: None,
        })
    }
    pub(crate) fn with_deadline(cancel: CancelToken, maximum: u64, deadline: Instant) -> Self {
        Self {
            cancel,
            deadline,
            maximum,
            completed: 0,
            memory_limit: crate::classical_memory::CLASSICAL_MEMORY_BYTES,
            retained_memory: 0,
            peak_memory: 0,
            progress: None,
        }
    }
    pub fn check(&self) -> Result<()> {
        if self.cancel.is_cancelled() {
            return Err(AnalysisError::Cancelled);
        }
        if Instant::now() >= self.deadline {
            return Err(AnalysisError::Deadline);
        }
        Ok(())
    }
    pub fn checkpoint(&mut self, units: u64) -> Result<()> {
        self.check()?;
        let next = add(self.completed, units.max(1))?;
        if next > self.maximum {
            return Err(AnalysisError::Budget("work units"));
        }
        self.completed = next;
        if let Some(progress) = &self.progress {
            progress(next)?;
        }
        Ok(())
    }
    pub fn completed(&self) -> u64 {
        self.completed
    }
    /// Configure the aggregate quota before classical work. `retained` covers
    /// caller-owned previews/results/source allocations throughout the call.
    pub fn with_memory_limit(mut self, limit: u64, retained: u64) -> Result<Self> {
        if retained > limit || limit == 0 {
            return Err(AnalysisError::Budget("classical retained memory"));
        }
        self.memory_limit = limit;
        self.retained_memory = retained;
        self.peak_memory = retained;
        Ok(self)
    }
    pub fn require_memory(&mut self, additional_peak: u64) -> Result<()> {
        self.check()?;
        let peak = add(self.retained_memory, additional_peak)?;
        if peak > self.memory_limit {
            return Err(AnalysisError::Budget("classical aggregate memory"));
        }
        self.peak_memory = self.peak_memory.max(peak);
        Ok(())
    }
    pub fn peak_memory_bytes(&self) -> u64 {
        self.peak_memory
    }
    pub(crate) fn set_completed(&mut self, completed: u64) {
        self.completed = completed;
    }
    #[cfg(test)]
    pub(crate) fn expire_at_checkpoint(&mut self) {
        self.deadline = Instant::now();
    }
}
