//! Values the audio thread publishes for the control side to read, all
//! through atomics.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use windfall_project::MAX_MIXER_TRACKS;

/// How much of each new measurement goes into the running CPU load average.
const LOAD_SMOOTHING: f32 = 0.05;

/// Most automations whose values are published at once. A project that has
/// more of them at work reports the first so many, in the order of the
/// project's automations.
pub(crate) const MAX_REPORTED_AUTOMATIONS: usize = 128;

/// Times a reader tries to get the published automation values in one
/// piece before it gives up for this time.
const READ_ATTEMPTS: u32 = 4;

/// Messages of MIDI output channels that can wait for the MIDI worker. A
/// power of two, so a position that wraps still names the same slot.
pub(crate) const MIDI_OUT_CAPACITY: usize = 2048;
/// The frame of a waiting MIDI message is kept in this many bits.
const MIDI_OUT_FRAME_BITS: u32 = 38;

pub(crate) struct Shared {
    pub waveform_epoch: AtomicU64,
    pub waveforms: Box<[crate::waveform_meter::WaveSlot]>,
    /// Current utility source. MAX means disabled, as for offline processors.
    pub current_track: AtomicU32,
    pub recording_clock: crate::recording_clock::RecordingClock,
    pub count_in_remaining: AtomicU32,
    pub navigation_overflows: AtomicU32,
    /// Notes whose echoes did not fit a channel's fixed echo queue and
    /// played dry instead.
    pub rejected_echoes: AtomicU32,
    pub rejected_control_notes: AtomicU32,
    pub rejected_rack_events: AtomicU32,
    /// Invalidates queued hardware notes and silences their voices without queue space.
    pub hardware_epoch: AtomicU64,
    /// The transport sequence number of the last play or stop the audio
    /// thread handled, shifted left one bit, with the playing flag in the
    /// lowest bit. Publishing both in one value lets the control side tell
    /// an answer to its latest request from an older state.
    transport: AtomicU64,
    /// Playhead in ticks, as `f64` bits.
    tick: AtomicU64,
    pub position_frame: AtomicU64,
    /// Where stopping returns the playhead to, in ticks, as `f64` bits.
    start: AtomicU64,
    voices: AtomicU32,
    /// Audio clips playing, and audio clips that would be had there been a
    /// slot for them.
    audio_clips: AtomicU32,
    dropped_clips: AtomicU32,
    /// Peak level per mixer track and side since the control side last
    /// looked, as `f32` bits: left, right, left, right.
    meters: [AtomicU32; MAX_MIXER_TRACKS * 2],
    /// Running average of processing time over buffer duration, `f32` bits.
    load: AtomicU32,
    /// Highest single-buffer load since the control side last looked.
    load_peak: AtomicU32,
    xruns: AtomicU32,
    /// Frames the device has asked for since the stream started.
    frames: AtomicU64,
    /// Size of the most recent device buffer in frames.
    buffer_frames: AtomicU32,
    /// Counts the changes to the automation values below, twice each: it
    /// is odd while the audio thread is writing them, and a reader that
    /// finds it changed has read a mixture.
    automated_version: AtomicU32,
    automated_count: AtomicU32,
    /// An automation's id in the high half and its value, as `f32` bits,
    /// in the low half.
    automated: [AtomicU64; MAX_REPORTED_AUTOMATIONS],
    /// Messages of MIDI output channels on their way to the MIDI worker:
    /// one writer, the audio thread, and one reader. Each slot holds the
    /// three bytes, their count and the frame: see [`Shared::push_midi_out`].
    midi_out: Box<[AtomicU64]>,
    midi_out_ports: Box<[AtomicU32]>,
    midi_out_write: AtomicU32,
    midi_out_read: AtomicU32,
    midi_out_dropped: AtomicU32,
}

impl Shared {
    pub fn new() -> Self {
        Self {
            waveform_epoch: AtomicU64::new(1),
            waveforms: (0..MAX_MIXER_TRACKS)
                .map(|_| crate::waveform_meter::WaveSlot::new())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            current_track: AtomicU32::new(u32::MAX),
            recording_clock: crate::recording_clock::RecordingClock::new(),
            count_in_remaining: AtomicU32::new(0),
            navigation_overflows: AtomicU32::new(0),
            rejected_echoes: AtomicU32::new(0),
            rejected_control_notes: AtomicU32::new(0),
            rejected_rack_events: AtomicU32::new(0),
            hardware_epoch: AtomicU64::new(0),
            transport: AtomicU64::new(0),
            tick: AtomicU64::new(0.0_f64.to_bits()),
            position_frame: AtomicU64::new(0),
            start: AtomicU64::new(0.0_f64.to_bits()),
            voices: AtomicU32::new(0),
            audio_clips: AtomicU32::new(0),
            dropped_clips: AtomicU32::new(0),
            meters: [const { AtomicU32::new(0) }; MAX_MIXER_TRACKS * 2],
            load: AtomicU32::new(0),
            load_peak: AtomicU32::new(0),
            xruns: AtomicU32::new(0),
            frames: AtomicU64::new(0),
            buffer_frames: AtomicU32::new(0),
            automated_version: AtomicU32::new(0),
            automated_count: AtomicU32::new(0),
            automated: [const { AtomicU64::new(0) }; MAX_REPORTED_AUTOMATIONS],
            midi_out: (0..MIDI_OUT_CAPACITY)
                .map(|_| AtomicU64::new(0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            midi_out_ports: (0..MIDI_OUT_CAPACITY)
                .map(|_| AtomicU32::new(0))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            midi_out_write: AtomicU32::new(0),
            midi_out_read: AtomicU32::new(0),
            midi_out_dropped: AtomicU32::new(0),
        }
    }

    /// Queues one message of a MIDI output channel for the MIDI worker.
    /// Only the audio thread calls this. It stores to preallocated slots and
    /// never waits; a full queue drops the message and counts it.
    #[cfg(test)]
    pub fn push_midi_out(&self, frame: u64, bytes: [u8; 3], len: u8) {
        self.push_midi_out_to(frame, bytes, len, 0);
    }

    pub fn push_midi_out_to(&self, frame: u64, bytes: [u8; 3], len: u8, port: u8) {
        let write = self.midi_out_write.load(Ordering::Relaxed);
        let read = self.midi_out_read.load(Ordering::Acquire);
        if write.wrapping_sub(read) as usize >= MIDI_OUT_CAPACITY {
            self.midi_out_dropped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let frame = frame & ((1 << MIDI_OUT_FRAME_BITS) - 1);
        let packed = u64::from(bytes[0])
            | u64::from(bytes[1]) << 8
            | u64::from(bytes[2]) << 16
            | u64::from(len.min(3)) << 24
            | frame << 26;
        self.midi_out[write as usize % MIDI_OUT_CAPACITY].store(packed, Ordering::Relaxed);
        self.midi_out_ports[write as usize % MIDI_OUT_CAPACITY]
            .store(u32::from(port.min(7)), Ordering::Relaxed);
        self.midi_out_write
            .store(write.wrapping_add(1), Ordering::Release);
    }

    /// Takes the oldest waiting MIDI output message: its frame, bytes and
    /// byte count. Only one thread at a time may call this.
    pub fn pop_midi_out_from(&self) -> Option<(u64, [u8; 3], u8, u8)> {
        let read = self.midi_out_read.load(Ordering::Relaxed);
        if read == self.midi_out_write.load(Ordering::Acquire) {
            return None;
        }
        let packed = self.midi_out[read as usize % MIDI_OUT_CAPACITY].load(Ordering::Relaxed);
        let port =
            self.midi_out_ports[read as usize % MIDI_OUT_CAPACITY].load(Ordering::Relaxed) as u8;
        self.midi_out_read
            .store(read.wrapping_add(1), Ordering::Release);
        Some((
            packed >> 26,
            [packed as u8, (packed >> 8) as u8, (packed >> 16) as u8],
            ((packed >> 24) & 3) as u8,
            port,
        ))
    }

    pub fn refused_midi_out(&self) {
        self.midi_out_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// MIDI output messages lost to a full queue since the engine started.
    pub fn midi_out_dropped(&self) -> u32 {
        self.midi_out_dropped.load(Ordering::Relaxed)
    }

    /// Publishes the automations at work as their id and their value, 0
    /// to 1. Only the audio thread calls this.
    pub fn publish_automated(&self, values: impl Iterator<Item = (u32, f32)>) {
        self.automated_version.fetch_add(1, Ordering::AcqRel);
        let mut count = 0;
        for (slot, (id, value)) in self.automated.iter().zip(values) {
            let packed = u64::from(id) << 32 | u64::from(value.to_bits());
            slot.store(packed, Ordering::Relaxed);
            count += 1;
        }
        self.automated_count.store(count, Ordering::Relaxed);
        self.automated_version.fetch_add(1, Ordering::Release);
    }

    /// Reads what [`publish_automated`](Self::publish_automated) last
    /// published into `out`. Should the audio thread be writing at every
    /// attempt, `out` is left empty for this once.
    pub fn automated(&self, out: &mut Vec<(u32, f32)>) {
        for _ in 0..READ_ATTEMPTS {
            out.clear();
            let version = self.automated_version.load(Ordering::Acquire);
            if version % 2 == 1 {
                std::hint::spin_loop();
                continue;
            }
            let count = self.automated_count.load(Ordering::Relaxed) as usize;
            for slot in &self.automated[..count.min(MAX_REPORTED_AUTOMATIONS)] {
                let packed = slot.load(Ordering::Relaxed);
                out.push(((packed >> 32) as u32, f32::from_bits(packed as u32)));
            }
            if self.automated_version.load(Ordering::Acquire) == version {
                return;
            }
        }
        out.clear();
    }

    pub fn publish_transport(&self, sequence: u32, playing: bool) {
        self.transport.store(
            u64::from(sequence) << 1 | u64::from(playing),
            Ordering::Release,
        );
    }

    /// The sequence number of the last transport request handled, and
    /// whether the transport was playing after it.
    pub fn transport(&self) -> (u32, bool) {
        let value = self.transport.load(Ordering::Acquire);
        ((value >> 1) as u32, value & 1 == 1)
    }

    pub fn publish_position(&self, tick: f64, start: f64, voices: u32) {
        self.tick.store(tick.to_bits(), Ordering::Relaxed);
        self.start.store(start.to_bits(), Ordering::Relaxed);
        self.voices.store(voices, Ordering::Relaxed);
    }

    pub fn set_tick(&self, tick: f64) {
        self.tick.store(tick.to_bits(), Ordering::Relaxed);
    }

    pub fn tick(&self) -> f64 {
        f64::from_bits(self.tick.load(Ordering::Relaxed))
    }

    pub fn set_start(&self, tick: f64) {
        self.start.store(tick.to_bits(), Ordering::Relaxed);
    }

    pub fn start(&self) -> f64 {
        f64::from_bits(self.start.load(Ordering::Relaxed))
    }

    pub fn voices(&self) -> u32 {
        self.voices.load(Ordering::Relaxed)
    }

    /// Publishes how many audio clips are playing and how many are left
    /// out for lack of a slot.
    pub fn publish_clips(&self, playing: u32, dropped: u32) {
        self.audio_clips.store(playing, Ordering::Relaxed);
        self.dropped_clips.store(dropped, Ordering::Relaxed);
    }

    pub fn audio_clips(&self) -> u32 {
        self.audio_clips.load(Ordering::Relaxed)
    }

    pub fn dropped_clips(&self) -> u32 {
        self.dropped_clips.load(Ordering::Relaxed)
    }

    /// Raises a track's meter to these peaks if they are higher.
    pub fn raise_meter(&self, track: usize, left: f32, right: f32) {
        for (side, peak) in [left, right].into_iter().enumerate() {
            // Levels are never negative, and for such floats the bit
            // patterns sort the same way the values do. A level that is not
            // a number would stick, so it is left out.
            if peak > 0.0 && peak.is_finite() {
                self.meters[track * 2 + side].fetch_max(peak.to_bits(), Ordering::Relaxed);
            }
        }
    }

    /// Reads one meter value and resets it.
    pub fn take_meter(&self, index: usize) -> f32 {
        f32::from_bits(self.meters[index].swap(0, Ordering::Relaxed))
    }

    /// Clears the stream measurements when a device stream starts.
    pub fn begin_stream(&self) {
        self.waveform_epoch.fetch_add(1, Ordering::AcqRel);
        self.load.store(0, Ordering::Relaxed);
        self.load_peak.store(0, Ordering::Relaxed);
        self.xruns.store(0, Ordering::Relaxed);
        self.frames.store(0, Ordering::Relaxed);
        self.buffer_frames.store(0, Ordering::Relaxed);
    }

    /// Records one device buffer: how many frames it held and how long the
    /// engine took to fill it. Taking longer than the buffer lasts is an
    /// xrun.
    pub fn record_buffer(&self, frames: usize, elapsed: Duration, sample_rate: u32) {
        if frames == 0 {
            return;
        }
        let duration = frames as f64 / f64::from(sample_rate);
        let load = (elapsed.as_secs_f64() / duration) as f32;
        let average = f32::from_bits(self.load.load(Ordering::Relaxed));
        let average = average + (load - average) * LOAD_SMOOTHING;
        self.load.store(average.to_bits(), Ordering::Relaxed);
        self.load_peak.fetch_max(load.to_bits(), Ordering::Relaxed);
        if load > 1.0 {
            self.record_xrun();
        }
        self.frames.fetch_add(frames as u64, Ordering::Relaxed);
        self.buffer_frames.store(frames as u32, Ordering::Relaxed);
    }

    pub fn record_xrun(&self) {
        self.xruns.fetch_add(1, Ordering::Relaxed);
    }

    pub fn load(&self) -> f32 {
        f32::from_bits(self.load.load(Ordering::Relaxed))
    }

    /// Reads the highest single-buffer load and resets it.
    pub fn take_load_peak(&self) -> f32 {
        f32::from_bits(self.load_peak.swap(0, Ordering::Relaxed))
    }

    pub fn xruns(&self) -> u32 {
        self.xruns.load(Ordering::Relaxed)
    }

    pub fn frames(&self) -> u64 {
        self.frames.load(Ordering::Relaxed)
    }

    pub fn buffer_frames(&self) -> u32 {
        self.buffer_frames.load(Ordering::Relaxed)
    }
}
