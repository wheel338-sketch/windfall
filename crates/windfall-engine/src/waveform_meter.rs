//! Bounded observational waveform history. Audio writes; the control side snapshots.
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use windfall_ipc::MixerWaveform;
use windfall_project::TrackId;

pub(crate) const POINTS: usize = 64;
pub(crate) const MAX_VISIBLE: usize = 128;

pub(crate) struct WaveSlot {
    pub requested: AtomicU32,
    version: AtomicU32,
    owner: AtomicU32,
    epoch: AtomicU64,
    serial: AtomicU64,
    head: AtomicU32,
    count: AtomicU32,
    sample_rate: AtomicU32,
    bucket_frames: AtomicU32,
    points: [AtomicU32; POINTS * 4],
}

impl WaveSlot {
    pub fn new() -> Self {
        Self {
            requested: AtomicU32::new(u32::MAX),
            version: AtomicU32::new(0),
            owner: AtomicU32::new(u32::MAX),
            epoch: AtomicU64::new(0),
            serial: AtomicU64::new(0),
            head: AtomicU32::new(0),
            count: AtomicU32::new(0),
            sample_rate: AtomicU32::new(0),
            bucket_frames: AtomicU32::new(0),
            points: [const { AtomicU32::new(0) }; POINTS * 4],
        }
    }

    pub fn snapshot(&self, id: TrackId, epoch: u64) -> Option<MixerWaveform> {
        if self.requested.load(Ordering::Acquire) != id.0 {
            return None;
        }
        for _ in 0..4 {
            let version = self.version.load(Ordering::Acquire);
            if version & 1 != 0 {
                std::hint::spin_loop();
                continue;
            }
            if self.owner.load(Ordering::Relaxed) != id.0
                || self.epoch.load(Ordering::Relaxed) != epoch
            {
                return None;
            }
            let count = (self.count.load(Ordering::Relaxed) as usize).min(POINTS);
            if count == 0 {
                return None;
            }
            let head = (self.head.load(Ordering::Relaxed) as usize) % POINTS;
            let points = (0..count)
                .map(|index| {
                    let at = (head + POINTS - count + index) % POINTS * 4;
                    std::array::from_fn(|side| {
                        f32::from_bits(self.points[at + side].load(Ordering::Relaxed))
                    })
                })
                .collect();
            let value = MixerWaveform {
                track: id,
                epoch,
                serial: self.serial.load(Ordering::Relaxed),
                sample_rate: self.sample_rate.load(Ordering::Relaxed),
                bucket_frames: self.bucket_frames.load(Ordering::Relaxed),
                points,
            };
            std::sync::atomic::fence(Ordering::Acquire);
            if self.version.load(Ordering::Acquire) == version {
                return Some(value);
            }
        }
        None
    }
}

/// One preallocated accumulator for each mixer allocation seat.
pub(crate) struct WaveAccumulator {
    owner: u32,
    epoch: u64,
    sample_rate: u32,
    next_frame: u64,
    fill: u32,
    bucket: [f32; 4],
    head: usize,
    count: usize,
    serial: u64,
}

impl WaveAccumulator {
    pub fn new() -> Self {
        Self {
            owner: u32::MAX,
            epoch: 0,
            sample_rate: 0,
            next_frame: 0,
            fill: 0,
            bucket: [
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::INFINITY,
                f32::NEG_INFINITY,
            ],
            head: 0,
            count: 0,
            serial: 0,
        }
    }

    pub fn capture(
        &mut self,
        slot: &WaveSlot,
        id: TrackId,
        epoch: u64,
        base: u64,
        sample_rate: u32,
        frames: &[[f32; 2]],
    ) {
        if slot.requested.load(Ordering::Acquire) != id.0 {
            self.owner = u32::MAX;
            return;
        }
        if frames.is_empty() {
            return;
        }
        let reset = self.owner != id.0
            || self.epoch != epoch
            || self.sample_rate != sample_rate
            || self.next_frame != base;
        if reset {
            *self = Self::new();
            self.owner = id.0;
            self.epoch = epoch;
            self.sample_rate = sample_rate;
        }
        let bucket_frames = (sample_rate / 200).max(1); // 5 ms at the actual rate.
        slot.version.fetch_add(1, Ordering::AcqRel);
        for frame in frames {
            for (side, sample) in frame.iter().enumerate() {
                let value = if sample.is_finite() {
                    sample.clamp(-8.0, 8.0)
                } else {
                    0.0
                };
                self.bucket[side * 2] = self.bucket[side * 2].min(value);
                self.bucket[side * 2 + 1] = self.bucket[side * 2 + 1].max(value);
            }
            self.fill += 1;
            if self.fill >= bucket_frames {
                for (side, value) in self.bucket.into_iter().enumerate() {
                    slot.points[self.head * 4 + side].store(value.to_bits(), Ordering::Relaxed);
                }
                self.head = (self.head + 1) % POINTS;
                self.count = (self.count + 1).min(POINTS);
                self.serial = self.serial.wrapping_add(1);
                self.fill = 0;
                self.bucket = [
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                ];
            }
        }
        self.next_frame = base.saturating_add(frames.len() as u64);
        slot.owner.store(id.0, Ordering::Relaxed);
        slot.epoch.store(epoch, Ordering::Relaxed);
        slot.serial.store(self.serial, Ordering::Relaxed);
        slot.head.store(self.head as u32, Ordering::Relaxed);
        slot.count.store(self.count as u32, Ordering::Relaxed);
        slot.sample_rate.store(sample_rate, Ordering::Relaxed);
        slot.bucket_frames.store(bucket_frames, Ordering::Relaxed);
        slot.version.fetch_add(1, Ordering::Release);
    }
}
