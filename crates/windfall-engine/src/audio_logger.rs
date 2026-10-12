//! The audio logger: a bounded background history of what the engine plays
//! and of chosen mixer tracks after their effects.
//!
//! Three threads take part, and only the first is realtime:
//!
//! - The audio callback owns a [`LoggerTap`]. For each logged source it
//!   copies the block it just made into a preallocated single-producer,
//!   single-consumer queue. A full queue drops the frames that do not fit and
//!   counts them. Nothing else happens there: no allocation, free, lock, wait
//!   or IO, whether the logger is being enabled, disabled, or is overflowing.
//! - A worker thread owned by the [`AudioLogger`] drains the queues into the
//!   histories every few milliseconds.
//! - Whoever asks for [`AudioLogger::stats`] or [`AudioLogger::tail`] shares
//!   one mutex with that worker. The audio callback never touches it.
//!
//! A tap reaches the processor as a message and comes back as garbage, like
//! every other heap object the audio thread is handed, so installing,
//! replacing and removing one frees nothing on the audio thread.
//!
//! # Bounds
//!
//! A history holds `seconds` of audio at the stream's rate and never more
//! than [`MAX_SOURCE_BYTES`]; a long history at a high rate is shortened to
//! fit, and [`SourceStats::capacity_frames`] says what it came to. The
//! oldest audio is overwritten. Each source also has a transfer queue of
//! [`QUEUE_SECONDS`] seconds. At most one master and
//! [`AUDIO_LOGGER_MAX_TRACKS`] track sources exist per logger.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::SampleFormat;
use rtrb::{Consumer, Producer, RingBuffer};
use windfall_ipc::{
    AUDIO_LOGGER_MAX_SECONDS, AUDIO_LOGGER_MAX_TRACKS, AUDIO_LOGGER_MIN_SECONDS, AudioLoggerSource,
    RecordingSource,
};
use windfall_project::{EffectId, TrackId};

use crate::controller::Controller;

/// One stereo frame: left, then right.
type Frame = [f32; 2];

/// The most memory one history may hold, whatever length was asked for.
pub const MAX_SOURCE_BYTES: u64 = 256 * 1024 * 1024;

/// Seconds of audio a transfer queue holds before the audio thread starts
/// dropping frames. The worker empties it every few milliseconds; a history
/// being copied out for a clip can keep it waiting for a fraction of this.
pub const QUEUE_SECONDS: u32 = 2;

/// How long the worker sleeps between two looks at the queues.
const WORKER_INTERVAL: Duration = Duration::from_millis(5);

const BYTES_PER_FRAME: u64 = std::mem::size_of::<Frame>() as u64;

/// A fixed-capacity stereo history. Once full, new audio overwrites the
/// oldest. Control side only: it allocates when made and never again.
pub struct HistoryRing {
    /// Grows up to `capacity` inside memory reserved in [`HistoryRing::new`],
    /// so pages are only touched as audio arrives.
    frames: Vec<Frame>,
    capacity: usize,
    /// Where the next frame goes once the history is full.
    next: usize,
    sample_rate: u32,
}

impl HistoryRing {
    /// How many frames a history of `seconds` at `sample_rate` holds: the
    /// length asked for, cut to [`MAX_SOURCE_BYTES`].
    pub fn capacity_for(sample_rate: u32, seconds: u32) -> usize {
        let wanted = u64::from(sample_rate) * u64::from(seconds);
        wanted.min(MAX_SOURCE_BYTES / BYTES_PER_FRAME) as usize
    }

    /// Reserves the whole history. Fails, instead of aborting, when the
    /// memory is not there.
    pub fn new(sample_rate: u32, seconds: u32) -> Result<Self, String> {
        let capacity = Self::capacity_for(sample_rate, seconds);
        if capacity == 0 {
            return Err("The audio logger needs a sample rate and a history length.".into());
        }
        let mut frames = Vec::new();
        frames.try_reserve_exact(capacity).map_err(|_| {
            format!(
                "Not enough memory for {} MB of audio history. Choose a shorter history.",
                capacity as u64 * BYTES_PER_FRAME / (1024 * 1024)
            )
        })?;
        Ok(Self {
            frames,
            capacity,
            next: 0,
            sample_rate,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frames held right now.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Frames held at most.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// The memory the history is allowed, all of it reserved up front.
    pub fn memory_bytes(&self) -> u64 {
        self.capacity as u64 * BYTES_PER_FRAME
    }

    pub fn clear(&mut self) {
        self.frames.clear();
        self.next = 0;
    }

    /// Appends frames, overwriting the oldest once full. A sample that is
    /// not a number is kept as silence: a history ends up in a file.
    pub fn push(&mut self, audio: &[Frame]) {
        for frame in audio {
            self.push_frame(*frame);
        }
    }

    /// The same for interleaved stereo. A stray last sample is ignored.
    pub fn push_interleaved(&mut self, samples: &[f32]) {
        for frame in samples.as_chunks::<2>().0.iter() {
            self.push_frame([frame[0], frame[1]]);
        }
    }

    #[inline]
    fn push_frame(&mut self, frame: Frame) {
        let frame = frame.map(|sample| if sample.is_finite() { sample } else { 0.0 });
        if self.frames.len() < self.capacity {
            // Inside the reservation: this never reallocates.
            self.frames.push(frame);
        } else {
            self.frames[self.next] = frame;
        }
        self.next = (self.next + 1) % self.capacity;
    }

    /// The newest `frames` frames, or all of them, oldest first and
    /// interleaved. Fewer when the history holds fewer.
    pub fn tail(&self, frames: Option<u64>) -> Result<Vec<f32>, String> {
        let held = self.frames.len();
        let count = frames.map_or(held, |frames| frames.min(held as u64) as usize);
        let mut out = Vec::new();
        out.try_reserve_exact(count * 2)
            .map_err(|_| "Not enough memory to copy the audio history.".to_owned())?;
        if count == 0 {
            return Ok(out);
        }
        // Until the history is full the newest frame is the last one; after
        // that it sits just before `next`.
        let end = if held < self.capacity {
            held
        } else {
            self.next
        };
        let start = (end + held - count) % held;
        let first = (held - start).min(count);
        for frame in &self.frames[start..start + first] {
            out.extend_from_slice(frame);
        }
        for frame in &self.frames[..count - first] {
            out.extend_from_slice(frame);
        }
        Ok(out)
    }
}

/// What the audio thread publishes about one source.
#[derive(Default)]
struct Counters {
    dropped: AtomicU64,
}

/// The audio thread's end of one source.
struct Lane {
    queue: Producer<Frame>,
    counters: Arc<Counters>,
}

impl Lane {
    /// Copies as much of the block as the queue has room for and counts the
    /// rest as dropped. Never waits.
    #[inline]
    fn push(&mut self, audio: &[Frame]) {
        let room = self.queue.slots().min(audio.len());
        if room > 0
            && let Ok(chunk) = self.queue.write_chunk_uninit(room)
        {
            chunk.fill_from_iter(audio[..room].iter().copied());
        }
        if room < audio.len() {
            self.counters
                .dropped
                .fetch_add((audio.len() - room) as u64, Ordering::Relaxed);
        }
    }
}

/// Everything the audio thread holds of a logger. Built on the control side,
/// handed over whole, and sent back whole to be dropped there.
pub(crate) struct LoggerTap {
    master: Option<Lane>,
    tracks: Box<[(TrackId, Lane)]>,
    effects: Box<[(EffectId, Lane)]>,
}

impl LoggerTap {
    #[inline]
    pub(crate) fn effect(&mut self, id: EffectId, left: &[f32], right: &[f32]) {
        for (logged, lane) in &mut self.effects {
            if *logged != id {
                continue;
            }
            let frames = left.len().min(right.len());
            let room = lane.queue.slots().min(frames);
            if room > 0
                && let Ok(chunk) = lane.queue.write_chunk_uninit(room)
            {
                chunk.fill_from_iter(
                    left[..room]
                        .iter()
                        .zip(&right[..room])
                        .map(|(a, b)| [*a, *b]),
                );
            }
            if room < frames {
                lane.counters
                    .dropped
                    .fetch_add((frames - room) as u64, Ordering::Relaxed);
            }
        }
    }
    /// The final mix of one block, as it leaves the engine.
    #[inline]
    pub(crate) fn master(&mut self, out: &[Frame]) {
        if let Some(lane) = &mut self.master {
            lane.push(out);
        }
    }

    /// One block of a mixer track after its effects. Tracks are told apart
    /// by id, so a logged track is followed wherever a new plan puts it and
    /// simply stops arriving when the project no longer has it.
    #[inline]
    pub(crate) fn track(&mut self, id: TrackId, audio: &[Frame]) {
        for (logged, lane) in &mut self.tracks {
            if *logged == id {
                lane.push(audio);
            }
        }
    }
}

/// The control side's end of one source.
struct Source {
    source: AudioLoggerSource,
    queue: Consumer<Frame>,
    queue_frames: usize,
    counters: Arc<Counters>,
    history: HistoryRing,
}

impl Source {
    fn drain(&mut self) {
        let waiting = self.queue.slots();
        if waiting == 0 {
            return;
        }
        if let Ok(chunk) = self.queue.read_chunk(waiting) {
            let (first, second) = chunk.as_slices();
            self.history.push(first);
            self.history.push(second);
            chunk.commit_all();
        }
    }

    fn memory_bytes(&self) -> u64 {
        self.history.memory_bytes() + self.queue_frames as u64 * BYTES_PER_FRAME
    }
}

struct Core {
    sources: Mutex<Vec<Source>>,
    stop: AtomicBool,
    sample_rate: u32,
}

impl Core {
    fn sources(&self) -> MutexGuard<'_, Vec<Source>> {
        // A history stays usable if a thread panicked while holding it.
        self.sources
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn pump(&self) {
        for source in self.sources().iter_mut() {
            source.drain();
        }
    }
}

/// What a logger is to keep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoggerSpec {
    /// History length, [`AUDIO_LOGGER_MIN_SECONDS`] to
    /// [`AUDIO_LOGGER_MAX_SECONDS`].
    pub seconds: u32,
    /// Log the final stereo mix.
    pub master: bool,
    /// Mixer tracks to log after their effects, at most
    /// [`AUDIO_LOGGER_MAX_TRACKS`], each once.
    pub tracks: Vec<TrackId>,
    pub effects: Vec<EffectId>,
    /// Start the worker that drains the queues. Without it nothing reaches a
    /// history until [`AudioLogger::pump`] is called, which is how tests
    /// make the queues overflow on purpose.
    pub worker: bool,
}

/// How much one source holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceStats {
    pub source: AudioLoggerSource,
    pub sample_rate: u32,
    pub capacity_frames: u64,
    pub available_frames: u64,
    /// Frames the audio thread could not queue because the worker was behind.
    pub dropped_frames: u64,
    /// The history and the transfer queue together.
    pub memory_bytes: u64,
}

/// Audio copied out of a history: interleaved stereo, oldest frame first.
#[derive(Debug, Clone, PartialEq)]
pub struct LoggedAudio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

/// The control side of a running logger: its histories and the worker that
/// fills them. Dropping it stops the worker and frees the histories; the tap
/// the audio thread holds is taken back by
/// [`Controller::stop_audio_logger`] or replaced by the next
/// [`Controller::start_audio_logger`], and comes home as garbage.
pub struct AudioLogger {
    core: Arc<Core>,
    worker: Option<JoinHandle<()>>,
}

impl AudioLogger {
    /// The rate every history of this logger was captured at.
    pub fn sample_rate(&self) -> u32 {
        self.core.sample_rate
    }

    /// Moves what the audio thread queued into the histories. The worker
    /// does this by itself; asking for stats or audio does it first too.
    pub fn pump(&self) {
        self.core.pump();
    }

    /// True once the audio thread no longer holds this logger's tap: its
    /// stream was closed or replaced, or the tap was taken away.
    pub fn detached(&self) -> bool {
        self.core
            .sources()
            .iter()
            .any(|source| source.queue.is_abandoned())
    }

    pub fn stats(&self) -> Vec<SourceStats> {
        let mut sources = self.core.sources();
        sources
            .iter_mut()
            .map(|source| {
                source.drain();
                SourceStats {
                    source: source.source,
                    sample_rate: source.history.sample_rate(),
                    capacity_frames: source.history.capacity() as u64,
                    available_frames: source.history.len() as u64,
                    dropped_frames: source.counters.dropped.load(Ordering::Relaxed),
                    memory_bytes: source.memory_bytes(),
                }
            })
            .collect()
    }

    /// The newest `frames` frames of a source, or all it has. The logger
    /// keeps running; the copy holds the history for as long as it takes.
    pub fn tail(
        &self,
        source: AudioLoggerSource,
        frames: Option<u64>,
    ) -> Result<LoggedAudio, String> {
        let mut sources = self.core.sources();
        let found = sources
            .iter_mut()
            .find(|held| held.source == source)
            .ok_or("The audio logger is not logging that source.")?;
        found.drain();
        Ok(LoggedAudio {
            sample_rate: found.history.sample_rate(),
            samples: found.history.tail(frames)?,
        })
    }
}

impl Drop for AudioLogger {
    fn drop(&mut self) {
        self.core.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

fn queue_frames(sample_rate: u32) -> usize {
    (sample_rate as usize)
        .saturating_mul(QUEUE_SECONDS as usize)
        .max(8_192)
}

fn check(spec: &LoggerSpec) -> Result<(), String> {
    if spec.effects.len() > AUDIO_LOGGER_MAX_TRACKS
        || spec
            .effects
            .iter()
            .enumerate()
            .any(|(i, effect)| spec.effects[..i].contains(effect))
    {
        return Err("Choose at most four different effect slots to record.".into());
    }
    if !(AUDIO_LOGGER_MIN_SECONDS..=AUDIO_LOGGER_MAX_SECONDS).contains(&spec.seconds) {
        return Err(format!(
            "The audio logger keeps {AUDIO_LOGGER_MIN_SECONDS} to {AUDIO_LOGGER_MAX_SECONDS} seconds."
        ));
    }
    if spec.tracks.len() > AUDIO_LOGGER_MAX_TRACKS {
        return Err(format!(
            "The audio logger follows at most {AUDIO_LOGGER_MAX_TRACKS} mixer tracks."
        ));
    }
    let repeated = spec
        .tracks
        .iter()
        .enumerate()
        .any(|(index, track)| spec.tracks[..index].contains(track));
    if repeated {
        return Err("A mixer track can be logged only once.".into());
    }
    if !spec.master && spec.tracks.is_empty() && spec.effects.is_empty() {
        return Err("Choose the master output or a mixer track to log.".into());
    }
    Ok(())
}

impl Controller {
    /// The playhead in ticks, as last published by the audio thread. Unlike
    /// [`Controller::frame`] this reads no meters, so it resets none.
    pub fn playhead_tick(&self) -> f64 {
        self.shared().tick()
    }

    /// The sample rate of the stream the logger would tap, if one is open.
    pub fn audio_logger_rate(&self) -> Option<u32> {
        self.attached_stream().map(|(_, rate)| rate)
    }

    /// Starts logging on the stream that is open now, in place of whatever
    /// logger it had. Allocates the histories and queues here, on the
    /// caller's thread; the audio thread only swaps one pointer for another
    /// at the start of its next buffer.
    ///
    /// Histories start empty and at the stream's rate. Fails with no stream
    /// open, and when the stream is replaced while this runs.
    pub fn start_audio_logger(&self, spec: &LoggerSpec) -> Result<AudioLogger, String> {
        check(spec)?;
        let (stream, sample_rate) = self
            .attached_stream()
            .ok_or("Open an audio output before the audio logger can run.")?;
        let named = spec
            .master
            .then_some(AudioLoggerSource::Master)
            .into_iter()
            .chain(
                spec.tracks
                    .iter()
                    .map(|&track| AudioLoggerSource::Track { track }),
            )
            .chain(
                spec.effects
                    .iter()
                    .map(|&effect| AudioLoggerSource::Effect { effect }),
            );
        let mut sources = Vec::new();
        let mut master = None;
        let mut tracks = Vec::new();
        let mut effects = Vec::new();
        for source in named {
            let frames = queue_frames(sample_rate);
            let (producer, consumer) = RingBuffer::new(frames);
            let counters = Arc::new(Counters::default());
            let lane = Lane {
                queue: producer,
                counters: counters.clone(),
            };
            match source {
                AudioLoggerSource::Track { track } => tracks.push((track, lane)),
                AudioLoggerSource::Effect { effect } => effects.push((effect, lane)),
                _ => master = Some(lane),
            }
            sources.push(Source {
                source,
                queue: consumer,
                queue_frames: frames,
                counters,
                history: HistoryRing::new(sample_rate, spec.seconds)?,
            });
        }
        let tap = Box::new(LoggerTap {
            master,
            tracks: tracks.into_boxed_slice(),
            effects: effects.into_boxed_slice(),
        });
        self.set_audio_logger_tap(Some(stream), Some(tap))?;
        let core = Arc::new(Core {
            sources: Mutex::new(sources),
            stop: AtomicBool::new(false),
            sample_rate,
        });
        let worker = if spec.worker {
            let core = core.clone();
            let worker = std::thread::Builder::new()
                .name("windfall-audio-logger".into())
                .spawn(move || {
                    while !core.stop.load(Ordering::Acquire) {
                        core.pump();
                        std::thread::park_timeout(WORKER_INTERVAL);
                    }
                })
                .map_err(|error| {
                    // The tap must not outlive histories nobody drains.
                    self.stop_audio_logger();
                    format!("Could not start the audio logger worker: {error}")
                })?;
            Some(worker)
        } else {
            None
        };
        Ok(AudioLogger { core, worker })
    }

    /// Gives a logger whose stream went away new queues on the stream that
    /// is open now, keeping its histories. Only for a stream at the rate the
    /// histories were captured at; anything else needs a fresh logger.
    pub fn reattach_audio_logger(&self, logger: &AudioLogger) -> Result<(), String> {
        let (stream, sample_rate) = self
            .attached_stream()
            .ok_or("Open an audio output before the audio logger can run.")?;
        if sample_rate != logger.sample_rate() {
            return Err("The audio output now runs at another sample rate.".into());
        }
        let mut master = None;
        let mut tracks = Vec::new();
        let mut effects = Vec::new();
        let mut queues = Vec::new();
        {
            let sources = logger.core.sources();
            for source in sources.iter() {
                let (producer, consumer) = RingBuffer::new(source.queue_frames);
                let lane = Lane {
                    queue: producer,
                    counters: source.counters.clone(),
                };
                match source.source {
                    AudioLoggerSource::Track { track } => tracks.push((track, lane)),
                    AudioLoggerSource::Effect { effect } => effects.push((effect, lane)),
                    _ => master = Some(lane),
                }
                queues.push(consumer);
            }
        }
        let tap = Box::new(LoggerTap {
            master,
            tracks: tracks.into_boxed_slice(),
            effects: effects.into_boxed_slice(),
        });
        // The old queues are emptied into the histories before they go, and
        // the new ones are in place before the audio thread can fill them.
        let mut replaced = Vec::with_capacity(queues.len());
        {
            let mut sources = logger.core.sources();
            for (source, queue) in sources.iter_mut().zip(queues) {
                source.drain();
                replaced.push(std::mem::replace(&mut source.queue, queue));
            }
        }
        drop(replaced);
        self.set_audio_logger_tap(Some(stream), Some(tap))
    }

    /// Takes the logger's tap away from the audio thread. The audio thread
    /// sends it back to be dropped on the control side.
    pub fn stop_audio_logger(&self) {
        let _ = self.set_audio_logger_tap(None, None);
    }
}

/// The rate nearest `preferred` that any of `ranges`, each a lowest and a
/// highest rate, supports. `preferred` itself where it is supported.
pub fn nearest_rate(preferred: u32, ranges: &[(u32, u32)]) -> Option<u32> {
    ranges
        .iter()
        .filter(|(low, high)| low <= high)
        .map(|&(low, high)| preferred.clamp(low, high))
        .min_by_key(|rate| (rate.abs_diff(preferred), *rate))
}

/// The rate a hardware input is logged at: `preferred`, which is the output
/// rate, where the device supports it on the chosen channels, and otherwise
/// the nearest rate it does support. Reads the device's capabilities and
/// opens no stream; [`crate::recording::Capture::start`] then opens it at
/// the rate returned.
pub fn input_rate(source: &RecordingSource, preferred: u32) -> Result<u32, String> {
    let id = cpal::available_hosts()
        .into_iter()
        .find(|id| id.name() == source.host)
        .ok_or("Input host is unavailable.")?;
    let host = cpal::host_from_id(id).map_err(|error| error.to_string())?;
    // The same lookup recording uses, so the loopback of an output device
    // can be logged like any other input.
    let endpoint = crate::recording_endpoint::find_input(&host, &source.device)?
        .ok_or("Input device is unavailable.")?;
    let highest = source.left.max(source.right.unwrap_or(source.left));
    let ranges: Vec<(u32, u32)> = crate::recording_endpoint::input_configs(&endpoint)?
        .into_iter()
        .filter(|config| {
            config.channels() > highest
                && matches!(
                    config.sample_format(),
                    SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16 | SampleFormat::I32
                )
        })
        .map(|config| (config.min_sample_rate(), config.max_sample_rate()))
        .collect();
    nearest_rate(preferred, &ranges)
        .ok_or_else(|| "The input does not offer the selected channels.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::processor::Processor;
    use crate::test_alloc::allocator_calls;

    fn frames(range: std::ops::Range<usize>) -> Vec<Frame> {
        range.map(|index| [index as f32, -(index as f32)]).collect()
    }

    fn interleaved(range: std::ops::Range<usize>) -> Vec<f32> {
        frames(range).into_iter().flatten().collect()
    }

    #[test]
    fn ring_keeps_the_newest_frames_in_order_across_many_wraps() {
        let mut ring = HistoryRing::new(10, 30).unwrap();
        assert_eq!(ring.capacity(), 300);
        assert!(ring.is_empty());
        assert_eq!(ring.tail(None).unwrap(), Vec::<f32>::new());

        ring.push(&frames(0..120));
        assert_eq!(ring.len(), 120);
        assert_eq!(ring.tail(None).unwrap(), interleaved(0..120));
        assert_eq!(ring.tail(Some(7)).unwrap(), interleaved(113..120));

        // Uneven pieces, so the write position lands everywhere.
        let mut written = 120;
        for piece in [1, 179, 7, 300, 299, 301, 64, 1_000, 3] {
            ring.push(&frames(written..written + piece));
            written += piece;
            assert_eq!(ring.len(), 300.min(written));
            let oldest = written.saturating_sub(300);
            assert_eq!(ring.tail(None).unwrap(), interleaved(oldest..written));
            assert_eq!(
                ring.tail(Some(11)).unwrap(),
                interleaved(written - 11..written)
            );
            assert_eq!(
                ring.tail(Some(10_000)).unwrap(),
                interleaved(oldest..written),
                "asking for more than there is gives all there is"
            );
        }
        ring.clear();
        assert!(ring.is_empty());
        ring.push_interleaved(&[0.5, -0.5, 0.25, -0.25, 9.0]);
        assert_eq!(ring.tail(None).unwrap(), [0.5, -0.5, 0.25, -0.25]);
    }

    #[test]
    fn ring_memory_is_bounded_and_reserved_once() {
        // Ten minutes at 48 kHz fits; at 192 kHz it is cut to the bound.
        assert_eq!(HistoryRing::capacity_for(48_000, 600), 28_800_000);
        let cut = HistoryRing::capacity_for(192_000, 600);
        assert_eq!(cut as u64, MAX_SOURCE_BYTES / 8);
        assert!((cut as u64) < 192_000 * 600);
        assert!(HistoryRing::new(0, 30).is_err());
        assert!(HistoryRing::new(48_000, 0).is_err());

        let mut ring = HistoryRing::new(1_000, 30).unwrap();
        assert_eq!(ring.memory_bytes(), 30_000 * 8);
        let block = frames(0..977);
        let calls = allocator_calls(|| {
            for _ in 0..100 {
                ring.push(&block);
            }
        });
        assert_eq!(calls, 0, "a history never grows past its reservation");
        assert_eq!(ring.len(), 30_000);
        assert_eq!(ring.capacity(), 30_000);
    }

    #[test]
    fn ring_stores_silence_for_samples_that_are_not_numbers() {
        let mut ring = HistoryRing::new(10, 30).unwrap();
        ring.push(&[[f32::NAN, 0.5], [f32::INFINITY, f32::NEG_INFINITY]]);
        assert_eq!(ring.tail(None).unwrap(), [0.0, 0.5, 0.0, 0.0]);
    }

    #[test]
    fn nearest_rate_prefers_the_output_rate_and_falls_back_to_the_closest() {
        assert_eq!(nearest_rate(48_000, &[]), None);
        assert_eq!(nearest_rate(48_000, &[(44_100, 44_100)]), Some(44_100));
        assert_eq!(
            nearest_rate(48_000, &[(44_100, 44_100), (48_000, 48_000)]),
            Some(48_000)
        );
        assert_eq!(nearest_rate(48_000, &[(8_000, 96_000)]), Some(48_000));
        assert_eq!(
            nearest_rate(88_200, &[(44_100, 48_000), (96_000, 192_000)]),
            Some(96_000)
        );
        // Equally near: the lower rate, every time.
        assert_eq!(
            nearest_rate(46_050, &[(48_000, 48_000), (44_100, 44_100)]),
            Some(44_100)
        );
        assert_eq!(nearest_rate(48_000, &[(50_000, 40_000)]), None);
    }

    #[test]
    fn specs_outside_the_bounds_are_refused_before_anything_is_built() {
        let (_processor, controller) = Processor::new(48_000);
        let spec = |seconds, master, tracks: &[u32]| LoggerSpec {
            seconds,
            master,
            tracks: tracks.iter().map(|&id| TrackId(id)).collect(),
            effects: Vec::new(),
            worker: false,
        };
        for bad in [
            spec(29, true, &[]),
            spec(601, true, &[]),
            spec(60, false, &[]),
            spec(60, true, &[1, 2, 3, 4, 5]),
            spec(60, true, &[3, 3]),
        ] {
            assert!(controller.start_audio_logger(&bad).is_err(), "{bad:?}");
        }
        assert!(
            controller
                .start_audio_logger(&spec(60, true, &[1, 2, 3, 4]))
                .is_ok()
        );
    }

    #[test]
    fn the_tap_never_allocates_when_installed_running_overflowing_or_removed() {
        let (mut processor, controller) = Processor::new(48_000);
        let mut out = vec![0.0_f32; 2 * 4_096];
        processor.process(&mut out);
        let spec = LoggerSpec {
            seconds: 30,
            master: true,
            tracks: vec![TrackId::MASTER, TrackId(77)],
            effects: Vec::new(),
            worker: false,
        };
        let logger = controller.start_audio_logger(&spec).unwrap();
        // Installing: the message is taken and the tap starts copying.
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        // Running, then overflowing: nothing drains the two-second queue.
        let calls = allocator_calls(|| {
            for _ in 0..40 {
                processor.process(&mut out);
            }
        });
        assert_eq!(calls, 0);
        let stats = logger.stats();
        assert_eq!(stats[0].source, AudioLoggerSource::Master);
        assert_eq!(stats[0].dropped_frames, 41 * 4_096 - 96_000);
        // Replacing retires the old tap, and removing retires the new one:
        // both leave the audio thread as garbage, not as frees.
        let second = controller.start_audio_logger(&spec).unwrap();
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        assert!(!logger.detached(), "the old tap waits in the garbage queue");
        controller.stop_audio_logger();
        assert_eq!(allocator_calls(|| processor.process(&mut out)), 0);
        let mut retirement = crate::ProjectRetirement::default();
        controller.take_retired(&mut retirement);
        assert!(!logger.detached() && !second.detached());
        retirement.clear();
        assert!(logger.detached() && second.detached());
    }

    fn master_only() -> LoggerSpec {
        LoggerSpec {
            seconds: 30,
            master: true,
            tracks: Vec::new(),
            effects: Vec::new(),
            worker: false,
        }
    }

    fn run(processor: &mut Processor, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0_f32; frames * 2];
        for block in out.chunks_mut(512) {
            processor.process(block);
        }
        out
    }

    #[test]
    fn a_stream_at_another_rate_needs_a_new_logger_with_an_empty_history() {
        let (mut processor, controller) = Processor::new(48_000);
        assert_eq!(controller.audio_logger_rate(), Some(48_000));
        let logger = controller.start_audio_logger(&master_only()).unwrap();
        run(&mut processor, 4_800);
        assert_eq!(logger.stats()[0].available_frames, 4_800);
        assert!(!logger.detached());

        // The device is reconfigured: the stream and its processor go, and
        // with them the tap. Nothing of the old logger is left installed.
        controller.suspend();
        drop(processor);
        assert!(logger.detached());
        assert_eq!(controller.audio_logger_rate(), None);
        assert!(controller.start_audio_logger(&master_only()).is_err());
        assert!(controller.reattach_audio_logger(&logger).is_err());

        let mut processor = controller.attach(44_100);
        assert_eq!(controller.audio_logger_rate(), Some(44_100));
        // Audio at one rate cannot continue a history at another.
        assert!(controller.reattach_audio_logger(&logger).is_err());
        let fresh = controller.start_audio_logger(&master_only()).unwrap();
        drop(logger);
        assert_eq!(fresh.sample_rate(), 44_100);
        let stats = fresh.stats()[0];
        assert_eq!(stats.available_frames, 0);
        assert_eq!(stats.sample_rate, 44_100);
        assert_eq!(stats.capacity_frames, 44_100 * 30);
        run(&mut processor, 441);
        assert_eq!(fresh.stats()[0].available_frames, 441);
        assert!(!fresh.detached());
    }

    #[test]
    fn a_stream_that_comes_back_at_the_same_rate_continues_the_history() {
        let (mut processor, controller) = Processor::new(48_000);
        let logger = controller.start_audio_logger(&master_only()).unwrap();
        controller.set_output_gain(0.0);
        run(&mut processor, 1_000);
        // Still in the queue when the stream is lost: it must not be lost.
        controller.suspend();
        drop(processor);
        assert!(logger.detached());

        let mut processor = controller.attach(48_000);
        controller.reattach_audio_logger(&logger).unwrap();
        assert!(!logger.detached());
        assert_eq!(logger.stats()[0].available_frames, 1_000);
        run(&mut processor, 2_000);
        let stats = logger.stats()[0];
        assert_eq!(stats.available_frames, 3_000);
        assert_eq!(stats.dropped_frames, 0);
        assert_eq!(
            logger
                .tail(AudioLoggerSource::Master, None)
                .unwrap()
                .samples
                .len(),
            6_000
        );
    }

    #[test]
    fn a_tap_for_a_stream_that_was_replaced_is_refused_and_dropped_here() {
        let (processor, controller) = Processor::new(48_000);
        let (stream, _) = controller.attached_stream().unwrap();
        controller.suspend();
        drop(processor);
        let _processor = controller.attach(48_000);
        let (producer, consumer) = RingBuffer::new(8);
        let tap = Box::new(LoggerTap {
            master: Some(Lane {
                queue: producer,
                counters: Arc::new(Counters::default()),
            }),
            tracks: Vec::new().into_boxed_slice(),
            effects: Vec::new().into_boxed_slice(),
        });
        assert!(
            controller
                .set_audio_logger_tap(Some(stream), Some(tap))
                .is_err()
        );
        assert!(consumer.is_abandoned());
        // Taking a tap away needs no stream to name.
        assert!(controller.set_audio_logger_tap(None, None).is_ok());
    }
}
