//! Scoped, plan-owned realtime jobs. Only the control side creates/joins lanes.
//! Jobs borrow disjoint prepared DSP, and the barrier ALWAYS harvests every
//! completion before returning. A deadline miss reports an xrun and returns the
//! completed block late; it never steals, restarts or destroys an in-flight node.
use crate::shared::Shared;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub(crate) const LANES: usize = 8;

/// Explicit node identity; high bit distinguishes channels from mixer tracks.
#[derive(Clone, Copy)]
pub(crate) struct Task {
    context: *mut (),
    execute: unsafe fn(*mut ()),
    node: u64,
}
// SAFETY: construction and run are unsafe and require exclusive, Send DSP
// contexts retained until the scoped completion barrier. No pointer escapes it.
unsafe impl Send for Task {}
impl Task {
    pub const EMPTY: Self = Self {
        context: std::ptr::null_mut(),
        execute: empty,
        node: 0,
    };
    /// The context must be Send, exclusive to this task and alive until run ends.
    pub unsafe fn new(context: *mut (), execute: unsafe fn(*mut ()), node: u64) -> Self {
        Self {
            context,
            execute,
            node,
        }
    }
    fn process(self, deadline: Instant) -> Completion {
        // SAFETY: run retains each exclusive context until this completion.
        unsafe { (self.execute)(self.context) };
        Completion {
            node: self.node,
            late: Instant::now() > deadline,
        }
    }
}
unsafe fn empty(_: *mut ()) {}
#[derive(Clone, Copy)]
struct Submission {
    task: Task,
    deadline: Instant,
}
#[derive(Clone, Copy)]
struct Completion {
    node: u64,
    late: bool,
}
struct Lane {
    input: rtrb::Producer<Submission>,
    output: rtrb::Consumer<Completion>,
    thread: JoinHandle<()>,
    _ready: Arc<AtomicBool>,
}

pub(crate) struct Workers {
    lanes: Vec<Lane>,
    stop: Arc<AtomicBool>,
    active_batch: Arc<AtomicBool>,
    allocations: Arc<AtomicUsize>,
    #[cfg(test)]
    jobs: Arc<AtomicUsize>,
    pub deadline: Instant,
}
impl Workers {
    pub fn new(count: usize) -> std::io::Result<Self> {
        if !(1..=LANES).contains(&count) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Audio threads must be 1 through 8",
            ));
        }
        let mut pool = Self {
            lanes: Vec::with_capacity(count - 1),
            stop: Arc::new(AtomicBool::new(false)),
            active_batch: Arc::new(AtomicBool::new(false)),
            allocations: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            jobs: Arc::new(AtomicUsize::new(0)),
            deadline: Instant::now(),
        };
        for index in 1..count {
            let (input, mut receive) = rtrb::RingBuffer::<Submission>::new(1);
            let (mut send, output) = rtrb::RingBuffer::<Completion>::new(1);
            let stop = pool.stop.clone();
            let active_batch = pool.active_batch.clone();
            let ready = Arc::new(AtomicBool::new(false));
            let worker_ready = ready.clone();
            let allocations = pool.allocations.clone();
            #[cfg(test)]
            let jobs = pool.jobs.clone();
            let thread = std::thread::Builder::new()
                .name(format!("windfall-dsp-{index}"))
                .spawn(move || {
                    let idle = WorkerIdle::new();
                    // Let the scheduler spread lanes across physical cores.
                    // Consecutive logical CPU IDs can be SMT siblings; pinning
                    // the first lanes there makes them compete with each other.
                    worker_ready.store(true, Ordering::Release);
                    while !stop.load(Ordering::Acquire) {
                        let mut received = receive.pop().ok();
                        // Stay awake across short gaps between stages, then park.
                        // The published queue value persists through every timed
                        // sleep: no callback wake or non-realtime lock is needed.
                        for _ in 0..10_000 {
                            if received.is_some() || stop.load(Ordering::Acquire) {
                                break;
                            }
                            received = receive.pop().ok();
                            std::hint::spin_loop();
                        }
                        let Some(job) = received else {
                            // A lane can finish before the coordinator or a
                            // sibling. Keep polling while their borrowed batch
                            // is outstanding: a timed park can otherwise delay
                            // the next chunk by the OS timer quantum. Idle
                            // playback still parks; the callback never wakes
                            // a thread through an OS synchronization primitive.
                            if !active_batch.load(Ordering::Acquire) {
                                idle.wait();
                            }
                            continue;
                        };
                        #[cfg(test)]
                        let mut completion = None;
                        #[cfg(test)]
                        allocations.fetch_add(
                            crate::test_alloc::allocator_calls(|| {
                                completion = Some(job.task.process(job.deadline));
                            }),
                            Ordering::Relaxed,
                        );
                        #[cfg(test)]
                        jobs.fetch_add(1, Ordering::Relaxed);
                        #[cfg(test)]
                        let result = completion.expect("DSP job completed");
                        #[cfg(not(test))]
                        let result = {
                            let _ = &allocations;
                            job.task.process(job.deadline)
                        };
                        // One job per lane, harvested before reuse: capacity is free.
                        if send.push(result).is_err() {
                            unreachable!("scoped completion slot occupied");
                        }
                    }
                })?;
            while !ready.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            pool.lanes.push(Lane {
                input,
                output,
                thread,
                _ready: ready,
            });
        }
        Ok(pool)
    }
    pub fn begin(&mut self, start: Instant, frames: usize, rate: u32) {
        self.deadline = start + Duration::from_secs_f64(frames as f64 / f64::from(rate.max(1)));
    }
    pub fn count(&self) -> usize {
        self.lanes.len() + 1
    }
    #[cfg(test)]
    pub fn executed_jobs(&self) -> usize {
        self.jobs.load(Ordering::Relaxed)
    }
    #[cfg(test)]
    pub fn lifetime(&self) -> std::sync::Weak<AtomicBool> {
        Arc::downgrade(&self.stop)
    }
    #[cfg(test)]
    pub fn allocator_calls(&self) -> usize {
        self.allocations.load(Ordering::Relaxed)
    }
    pub fn record_node(&mut self, node: u64, shared: &Shared) {
        self.report(
            Completion {
                node,
                late: Instant::now() > self.deadline,
            },
            shared,
        );
    }
    fn report(&mut self, completion: Completion, shared: &Shared) {
        if completion.late {
            shared.processing_overruns.fetch_add(1, Ordering::Relaxed);
            shared
                .last_processing_overrun
                .store(completion.node + 1, Ordering::Release);
            if !shared
                .processing_xrun_recorded
                .swap(true, Ordering::Relaxed)
            {
                shared.record_xrun();
            }
        }
    }
    /// Safety: tasks have distinct exclusive contexts. None may be accessed,
    /// moved or dropped until this function returns, INCLUDING on an overrun.
    pub unsafe fn run(&mut self, tasks: &[Task], shared: &Shared) {
        self.active_batch
            .store(tasks.len() > 1 && !self.lanes.is_empty(), Ordering::Release);
        for chunk in tasks.chunks(self.count()) {
            for (lane, task) in self.lanes.iter_mut().zip(chunk.iter().skip(1)) {
                let submission = Submission {
                    task: *task,
                    deadline: self.deadline,
                };
                if lane.input.push(submission).is_err() {
                    unreachable!("scoped input slot occupied");
                }
            }
            if let Some(task) = chunk.first() {
                let completion = task.process(self.deadline);
                self.report(completion, shared);
            }
            // Bounded hand-off: capacity-one queues, at most seven outstanding
            // owners. Poll in fixed-size rounds, then continue the late-output
            // barrier without ever reclaiming an unfinished DSP context.
            for index in 0..chunk.len().saturating_sub(1) {
                let completion = loop {
                    if let Ok(result) = self.lanes[index].output.pop() {
                        break result;
                    }
                    for _ in 0..256 {
                        std::hint::spin_loop();
                    }
                };
                self.report(completion, shared);
            }
        }
        self.active_batch.store(false, Ordering::Release);
    }
}

/// Only the worker waits. Windows' ordinary timed park can round 25 us up to
/// a scheduler tick; a realtime callback then waits for a sleeping DSP lane.
/// A private high-resolution timer avoids changing system timer resolution.
#[cfg(windows)]
struct WorkerIdle {
    timer: *mut std::ffi::c_void,
    priority: *mut std::ffi::c_void,
}

#[cfg(windows)]
#[link(name = "avrt")]
unsafe extern "system" {
    fn AvSetMmThreadCharacteristicsW(task: *const u16, index: *mut u32) -> *mut std::ffi::c_void;
    fn AvSetMmThreadPriority(handle: *mut std::ffi::c_void, priority: i32) -> i32;
    fn AvRevertMmThreadCharacteristics(handle: *mut std::ffi::c_void) -> i32;
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateWaitableTimerExW(
        attributes: *const std::ffi::c_void,
        name: *const u16,
        flags: u32,
        access: u32,
    ) -> *mut std::ffi::c_void;
    fn SetWaitableTimerEx(
        timer: *mut std::ffi::c_void,
        due: *const i64,
        period: i32,
        completion: *const std::ffi::c_void,
        argument: *const std::ffi::c_void,
        context: *const std::ffi::c_void,
        tolerance: u32,
    ) -> i32;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, milliseconds: u32) -> u32;
    fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
}

#[cfg(windows)]
impl WorkerIdle {
    fn new() -> Self {
        let task = [80_u16, 114, 111, 32, 65, 117, 100, 105, 111, 0]; // Pro Audio
        let mut index = 0;
        // SAFETY: static task name and live index pointer; registration is
        // owned and reverted on this worker, before it exits.
        let priority = unsafe { AvSetMmThreadCharacteristicsW(task.as_ptr(), &mut index) };
        if !priority.is_null() {
            // SAFETY: live MMCSS registration for this thread; HIGH = 1.
            unsafe {
                AvSetMmThreadPriority(priority, 1);
            }
        }
        // SAFETY: no name/security descriptor; this worker exclusively owns
        // the resulting auto-reset timer until Drop on the same thread.
        Self {
            timer: unsafe {
                CreateWaitableTimerExW(std::ptr::null(), std::ptr::null(), 2, 0x1f0003)
            },
            priority,
        }
    }

    fn wait(&self) {
        let due = -250_i64; // relative 25 us, in 100 ns units
        // SAFETY: owned live handle, valid due time, no APC or wake context.
        if !self.timer.is_null()
            && unsafe {
                SetWaitableTimerEx(
                    self.timer,
                    &due,
                    0,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    0,
                )
            } != 0
        {
            // A finite bound also makes retirement safe after a timer failure.
            // SAFETY: owned live handle; no pointer is retained by this call.
            unsafe {
                WaitForSingleObject(self.timer, 1);
            }
        } else {
            // Unsupported timer: never fall back to a millisecond-scale sleep
            // that strands a borrowed DSP batch. Yield without a timed park.
            std::thread::yield_now();
        }
    }
}

#[cfg(windows)]
impl Drop for WorkerIdle {
    fn drop(&mut self) {
        if !self.timer.is_null() {
            // SAFETY: this worker exclusively owns the timer, with no waits
            // outstanding; Drop runs on the same worker after its last use.
            unsafe {
                CloseHandle(self.timer);
            }
        }
        if !self.priority.is_null() {
            // SAFETY: live registration belongs to this worker; no other
            // thread uses it, and no processing job is outstanding here.
            unsafe {
                AvRevertMmThreadCharacteristics(self.priority);
            }
        }
    }
}

#[cfg(not(windows))]
struct WorkerIdle;

#[cfg(not(windows))]
impl WorkerIdle {
    fn new() -> Self {
        Self
    }
    fn wait(&self) {
        std::thread::park_timeout(Duration::from_micros(25));
    }
}
impl Drop for Workers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        for lane in self.lanes.drain(..) {
            lane.thread.thread().unpark();
            let _ = lane.thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scoped_dispatch_retains_late_nodes_and_reports_one_xrun_without_allocating() {
        unsafe fn fill(context: *mut ()) {
            // SAFETY: test owns this separate scalar until the barrier returns.
            unsafe {
                *context.cast::<u64>() = 77;
            }
        }
        let mut workers = Workers::new(4).unwrap();
        let shared = Shared::new();
        let mut values = [0u64; 8];
        let mut tasks = [Task::EMPTY; 8];
        for (index, value) in values.iter_mut().enumerate() {
            // SAFETY: distinct scalars retained throughout run.
            tasks[index] = unsafe { Task::new(value as *mut u64 as *mut (), fill, index as u64) };
        }
        // Deterministically late before any job starts; no timing-dependent assertion.
        workers.begin(Instant::now() - Duration::from_secs(1), 1, 48_000);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| {
                // SAFETY: all contexts remain stationary until the full barrier.
                unsafe {
                    workers.run(&tasks, &shared);
                }
            }),
            0
        );
        assert_eq!(values, [77; 8]);
        assert_eq!(workers.allocator_calls(), 0);
        assert_eq!(shared.xruns(), 1);
        assert_eq!(shared.processing_overruns.load(Ordering::Relaxed), 8);
        assert_eq!(shared.last_processing_overrun.load(Ordering::Acquire), 8);
        shared.record_buffer(1, Duration::from_secs(1), 48_000);
        assert_eq!(
            shared.xruns(),
            1,
            "device timing must not double-count a node overrun"
        );
        let lifetime = workers.lifetime();
        drop(workers);
        assert!(
            lifetime.upgrade().is_none(),
            "all worker references leave before control destruction ends"
        );
    }
    #[test]
    fn prepared_parallel_dsp_and_immutable_plans_are_send_and_sync() {
        fn send<T: Send>() {}
        fn sync<T: Sync>() {}
        send::<crate::rack::EffectUnit>();
        send::<crate::rack::InstrumentUnit>();
        send::<windfall_dsp::TrackProcessor>();
        send::<crate::mixer::TrackScratch>();
        send::<crate::state::DelaySlot>();
        sync::<crate::plan::Plan>();
    }
}
