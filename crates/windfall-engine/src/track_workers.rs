//! Owned post-slot track processing and an immutable, order-preserving graph
//! schedule. The production mixer remains serial until its complete routing and
//! retirement paths can transfer this ownership end to end.
//!
//! Construct/destroy on the control thread. Submission and harvest only move
//! preallocated values through capacity-one SPSC queues: no borrowed mixer state,
//! task closures, spawning, allocation, locks, OS wakes or blocking waits. A
//! missed budget latches fallback for future jobs, but does NOT reclaim a job
//! still owned by a worker. Its eventual result must be harvested before state
//! replacement. This distinction prevents timeout fallback from racing DSP.

use std::ops::Range;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;
use windfall_dsp::TrackProcessor;

pub const TRACK_BLOCK_FRAMES: usize = crate::mixer::MAX_BLOCK;
/// At most eight total audio threads, including the calling audio thread.
pub const MAX_WORKERS: usize = 7;
/// A polling budget is bounded in queue probes, not an OS scheduling guarantee.
pub const MAX_POLL_ROUNDS: usize = 256;

/// All mutable DSP and scratch for one track. The caller retains it when a
/// submission is refused; successful submission transfers exclusive ownership.
pub struct TrackJob {
    epoch: u64,
    track: usize,
    processor: TrackProcessor,
    left: [f32; TRACK_BLOCK_FRAMES],
    right: [f32; TRACK_BLOCK_FRAMES],
    frames: usize,
}

impl TrackJob {
    /// Processor preparation (including EQ allocations) is control-side only.
    pub fn new(epoch: u64, track: usize, processor: TrackProcessor) -> Self {
        Self {
            epoch,
            track,
            processor,
            left: [0.0; TRACK_BLOCK_FRAMES],
            right: [0.0; TRACK_BLOCK_FRAMES],
            frames: 0,
        }
    }
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn track(&self) -> usize {
        self.track
    }
    pub fn processor_mut(&mut self) -> &mut TrackProcessor {
        &mut self.processor
    }
    /// Rejects malformed blocks without altering processor or scratch state.
    pub fn load(&mut self, left: &[f32], right: &[f32]) -> bool {
        if left.len() != right.len() || left.len() > TRACK_BLOCK_FRAMES {
            return false;
        }
        self.frames = left.len();
        self.left[..self.frames].copy_from_slice(left);
        self.right[..self.frames].copy_from_slice(right);
        true
    }
    pub fn left(&self) -> &[f32] {
        &self.left[..self.frames]
    }
    pub fn right(&self) -> &[f32] {
        &self.right[..self.frames]
    }
    /// Identical operation for serial fallback and a worker; no reassociation.
    pub fn process(&mut self) {
        self.processor.process(
            &mut self.left[..self.frames],
            &mut self.right[..self.frames],
        );
    }
}

struct Lane {
    input: rtrb::Producer<TrackJob>,
    output: rtrb::Consumer<TrackJob>,
    occupied: bool,
    thread: JoinHandle<()>,
    #[cfg(test)]
    allocations: Arc<std::sync::atomic::AtomicUsize>,
}

pub struct TrackWorkers {
    lanes: Vec<Lane>,
    stop: Arc<AtomicBool>,
    fallback: bool,
}

impl TrackWorkers {
    /// Precreates all workers and queues. A count of one uses the caller only.
    /// Spawn failures tear down already-created workers on this control thread.
    pub fn new(thread_count: usize) -> std::io::Result<Self> {
        if !(1..=MAX_WORKERS + 1).contains(&thread_count) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "audio thread count must be 1..=8",
            ));
        }
        let mut pool = Self {
            lanes: Vec::with_capacity(thread_count - 1),
            stop: Arc::new(AtomicBool::new(false)),
            fallback: false,
        };
        for index in 0..thread_count - 1 {
            let (input, mut receive) = rtrb::RingBuffer::<TrackJob>::new(1);
            let (mut send, output) = rtrb::RingBuffer::<TrackJob>::new(1);
            let stop = pool.stop.clone();
            #[cfg(test)]
            let allocations = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            #[cfg(test)]
            let counter = allocations.clone();
            let thread = std::thread::Builder::new()
                .name(format!("windfall-track-{index}"))
                .spawn(move || {
                    while !stop.load(Ordering::Acquire) {
                        let Ok(mut job) = receive.pop() else {
                            // The caller never wakes or parks. Idle latency is an
                            // external OS performance gate, not a realtime promise.
                            std::thread::park_timeout(Duration::from_micros(50));
                            continue;
                        };
                        #[cfg(test)]
                        counter.fetch_add(
                            crate::test_alloc::allocator_calls(|| job.process()),
                            Ordering::Relaxed,
                        );
                        #[cfg(not(test))]
                        job.process();
                        // Capacity one and one outstanding job per lane make this
                        // full branch unreachable under the public API. Retain the
                        // owner nevertheless until shutdown instead of dropping DSP.
                        loop {
                            match send.push(job) {
                                Ok(()) => break,
                                Err(rtrb::PushError::Full(returned)) => job = returned,
                            }
                            if stop.load(Ordering::Acquire) {
                                return;
                            }
                            std::hint::spin_loop();
                        }
                    }
                })?;
            pool.lanes.push(Lane {
                input,
                output,
                occupied: false,
                thread,
                #[cfg(test)]
                allocations,
            });
        }
        Ok(pool)
    }

    pub fn worker_count(&self) -> usize {
        self.lanes.len()
    }
    pub fn pending(&self) -> usize {
        self.lanes.iter().filter(|lane| lane.occupied).count()
    }
    pub fn is_fallback(&self) -> bool {
        self.fallback || self.lanes.is_empty()
    }

    /// Refusal returns the complete owner for serial processing. Even if a
    /// worker finished, no second job enters until the first result is harvested.
    // Boxing the large refused owner would introduce a callback allocation.
    #[allow(clippy::result_large_err)]
    pub fn try_submit(&mut self, lane: usize, job: TrackJob) -> Result<(), TrackJob> {
        if self.fallback {
            return Err(job);
        }
        let Some(lane) = self.lanes.get_mut(lane) else {
            return Err(job);
        };
        if lane.occupied {
            return Err(job);
        }
        match lane.input.push(job) {
            Ok(()) => {
                lane.occupied = true;
                Ok(())
            }
            Err(rtrb::PushError::Full(job)) => Err(job),
        }
    }

    pub fn try_take(&mut self, lane: usize) -> Option<TrackJob> {
        let lane = self.lanes.get_mut(lane)?;
        if !lane.occupied {
            return None;
        }
        let job = lane.output.pop().ok()?;
        lane.occupied = false;
        Some(job)
    }

    /// At most 7 * 256 queue probes, with no clock/OS call. Completed owners
    /// enter indexed slots; consumers must reduce in plan order, never arrival
    /// order. Existing slots are left untouched. Returns unharvested owners.
    pub fn poll_budget(
        &mut self,
        rounds: usize,
        completed: &mut [Option<TrackJob>; MAX_WORKERS],
    ) -> usize {
        for _ in 0..rounds.min(MAX_POLL_ROUNDS) {
            for (index, slot) in completed.iter_mut().enumerate().take(self.lanes.len()) {
                if slot.is_none() {
                    *slot = self.try_take(index);
                }
            }
            if self.pending() == 0 {
                return 0;
            }
            std::hint::spin_loop();
        }
        let pending = self.pending();
        if pending != 0 {
            self.fallback = true;
        }
        pending
    }

    /// A control-side recovery decision after every outstanding owner returned.
    pub fn resume(&mut self) -> bool {
        if self.pending() != 0 {
            return false;
        }
        self.fallback = false;
        true
    }
}

impl Drop for TrackWorkers {
    fn drop(&mut self) {
        // Never drop this pool from the audio callback. This is control-side
        // shutdown, where joins and destruction of unharvested DSP are allowed.
        self.stop.store(true, Ordering::Release);
        for lane in self.lanes.drain(..) {
            lane.thread.thread().unpark();
            let _ = lane.thread.join();
        }
    }
}

/// Consecutive independent ranges into an existing topological order. Keeping
/// ranges consecutive preserves serial floating-point accumulation order.
#[derive(Debug, Clone)]
pub struct GraphWaves {
    ranges: Vec<Range<usize>>,
}

impl GraphWaves {
    /// Control-side only. Supply ALL audio/send/sidechain/aux dependencies.
    /// Dynamic Current tracks and unsafe hosted units must be singleton barriers.
    /// Rejects cycles, invalid indices and duplicate/missing tracks rather than
    /// trying to invent a different routing order.
    pub fn compile(
        order: &[usize],
        tracks: usize,
        dependencies: &[(usize, usize)],
        serial: &[usize],
    ) -> Result<Self, &'static str> {
        if order.len() != tracks {
            return Err("incomplete track order");
        }
        let mut positions = vec![usize::MAX; tracks];
        for (position, &track) in order.iter().enumerate() {
            if track >= tracks || positions[track] != usize::MAX {
                return Err("invalid track order");
            }
            positions[track] = position;
        }
        for &(source, target) in dependencies {
            if source >= tracks || target >= tracks || positions[source] >= positions[target] {
                return Err("invalid dependency order");
            }
        }
        if serial.iter().any(|&track| track >= tracks) {
            return Err("invalid serial barrier");
        }
        let mut ranges = Vec::with_capacity(tracks);
        let mut start = 0;
        for (position, &track) in order.iter().enumerate() {
            if serial.contains(&track) {
                if start < position {
                    ranges.push(start..position);
                }
                ranges.push(position..position + 1);
                start = position + 1;
            } else if dependencies
                .iter()
                .any(|&(source, target)| target == track && positions[source] >= start)
            {
                ranges.push(start..position);
                start = position;
            }
        }
        if start < tracks {
            ranges.push(start..tracks);
        }
        Ok(Self { ranges })
    }
    pub fn ranges(&self) -> &[Range<usize>] {
        &self.ranges
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_dsp_allocator_is_guarded_on_the_actual_worker_thread() {
        let mut pool = TrackWorkers::new(2).unwrap();
        let mut job = TrackJob::new(
            1,
            0,
            TrackProcessor::new(48_000, TRACK_BLOCK_FRAMES, Default::default()),
        );
        assert!(job.load(&[0.25; TRACK_BLOCK_FRAMES], &[0.125; TRACK_BLOCK_FRAMES]));
        assert!(pool.try_submit(0, job).is_ok());
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let job = loop {
            if let Some(job) = pool.try_take(0) {
                break job;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(pool.lanes[0].allocations.load(Ordering::Relaxed), 0);
        assert_eq!(job.left().len(), TRACK_BLOCK_FRAMES);
    }
    #[test]
    fn malformed_blocks_do_not_change_the_owned_buffers() {
        let mut job = TrackJob::new(
            1,
            0,
            TrackProcessor::new(48_000, TRACK_BLOCK_FRAMES, Default::default()),
        );
        assert!(job.load(&[1.0; 3], &[2.0; 3]));
        assert!(!job.load(&[0.0; 2], &[0.0; 3]));
        assert!(!job.load(
            &[0.0; TRACK_BLOCK_FRAMES + 1],
            &[0.0; TRACK_BLOCK_FRAMES + 1]
        ));
        assert_eq!(job.left(), &[1.0; 3]);
        assert_eq!(job.right(), &[2.0; 3]);
    }
}
