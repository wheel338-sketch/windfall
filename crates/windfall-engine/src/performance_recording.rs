//! Prepared performance PCM and callback-admitted transcript. One callback
//! writes atomic slots; control snapshots after the completion release.
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use windfall_core::AudioBuffer;
use windfall_project::{LaunchQuantization, PatternId, PerformanceLogEvent};
const MAX_EVENTS: usize = 8192;
struct LogSlot { frame: AtomicU32, request: AtomicU64 }
pub struct PerformanceRecording {
    rate: u32,
    audio: Box<[AtomicU32]>,
    events: Box<[LogSlot]>,
    start: AtomicU64,
    tick: AtomicU64,
    frames: AtomicU32,
    count: AtomicU32,
    // 0 waiting, 1 writing, 2 complete, 3 capacity/clock failure.
    phase: AtomicU32,
}
pub struct RecordedPerformance {
    pub audio: AudioBuffer,
    pub initial_tick: f64,
    pub events: Vec<PerformanceLogEvent>,
}
impl PerformanceRecording {
    pub fn prepare(rate: u32, seconds: u32) -> Result<std::sync::Arc<Self>, String> {
        if !(8000..=192000).contains(&rate) || !(1..=60).contains(&seconds) {
            return Err("Performance recording supports 8–192 kHz and 1–60 seconds".into());
        }
        let count = rate as usize * seconds as usize * 2;
        let mut audio = Vec::new();
        audio.try_reserve_exact(count).map_err(|_| "Not enough memory to prepare the performance capture")?;
        audio.resize_with(count, || AtomicU32::new(0));
        Ok(std::sync::Arc::new(Self {
            rate,
            audio: audio.into_boxed_slice(),
            events: (0..MAX_EVENTS).map(|_| LogSlot { frame: AtomicU32::new(0), request: AtomicU64::new(0) }).collect::<Vec<_>>().into_boxed_slice(),
            start: AtomicU64::new(0), tick: AtomicU64::new(0), frames: AtomicU32::new(0), count: AtomicU32::new(0), phase: AtomicU32::new(0),
        }))
    }
    pub fn state(&self) -> windfall_ipc::PerformanceRecordingState {
        let phase = self.phase.load(Ordering::Acquire);
        windfall_ipc::PerformanceRecordingState {
            active: phase < 2, finished: phase == 2,
            frames: self.frames.load(Ordering::Acquire), sample_rate: self.rate,
            error: (phase == 3).then(|| "Performance capture exceeded its duration/event budget or the audio clock changed. Cancel this take.".into()),
        }
    }
    pub fn snapshot(&self) -> Result<RecordedPerformance, String> {
        let state = self.state();
        if let Some(error) = state.error { return Err(error); }
        if !state.finished { return Err("Wait for the audio callback to finish the performance capture".into()); }
        let samples = self.audio[..state.frames as usize * 2].iter().map(|sample| f32::from_bits(sample.load(Ordering::Relaxed))).collect();
        let events = self.events[..self.count.load(Ordering::Relaxed) as usize].iter().map(|slot| {
            let word = slot.request.load(Ordering::Relaxed);
            PerformanceLogEvent {
                frame: slot.frame.load(Ordering::Relaxed), track: word as u8,
                pattern: ((word >> 16) as u32 != 0).then_some(PatternId((word >> 16) as u32)),
                quantization: match (word >> 8) & 255 { 0 => LaunchQuantization::Immediate, 1 => LaunchQuantization::Beat, _ => LaunchQuantization::Bar },
            }
        }).collect();
        Ok(RecordedPerformance { audio: AudioBuffer::from_interleaved(self.rate, 2, samples), initial_tick: f64::from_bits(self.tick.load(Ordering::Relaxed)), events })
    }
    pub(crate) fn begin(&self, frame: u64, tick: f64, rate: u32) {
        self.start.store(frame, Ordering::Relaxed);
        self.tick.store(tick.to_bits(), Ordering::Relaxed);
        let _ = self.phase.compare_exchange(0, if rate == self.rate { 1 } else { 3 }, Ordering::Release, Ordering::Relaxed);
    }
    pub(crate) fn launch(&self, frame: u64, request: crate::performance::LaunchRequest) {
        if self.phase.load(Ordering::Relaxed) != 1 { return; }
        let index = self.count.load(Ordering::Relaxed) as usize;
        let relative = frame.saturating_sub(self.start.load(Ordering::Relaxed));
        if index == MAX_EVENTS || relative >= self.audio.len() as u64 / 2 {
            self.phase.store(3, Ordering::Release); return;
        }
        let q = match request.quantization { LaunchQuantization::Immediate => 0_u64, LaunchQuantization::Beat => 1, LaunchQuantization::Bar => 2 };
        self.events[index].frame.store(relative as u32, Ordering::Relaxed);
        self.events[index].request.store(u64::from(request.track) | (q << 8) | (u64::from(request.pattern.map_or(0, |id| id.0)) << 16), Ordering::Relaxed);
        self.count.store(index as u32 + 1, Ordering::Release);
    }
    pub(crate) fn copy(&self, frame: u64, rate: u32, audio: &[[f32; 2]]) {
        if self.phase.load(Ordering::Relaxed) != 1 { return; }
        let first = frame.saturating_sub(self.start.load(Ordering::Relaxed)) as usize;
        if rate != self.rate || first != self.frames.load(Ordering::Relaxed) as usize || first.saturating_add(audio.len()) > self.audio.len() / 2 {
            self.phase.store(3, Ordering::Release); return;
        }
        for (index, pair) in audio.iter().enumerate() {
            for (side, sample) in pair.iter().enumerate() {
                if !sample.is_finite() { self.phase.store(3, Ordering::Release); return; }
                self.audio[(first + index) * 2 + side].store(sample.to_bits(), Ordering::Relaxed);
            }
        }
        self.frames.store((first + audio.len()) as u32, Ordering::Release);
    }
    pub(crate) fn finish(&self) {
        let _ = self.phase.compare_exchange(1, 2, Ordering::Release, Ordering::Relaxed);
    }
    pub fn invalidate(&self) { self.phase.store(3, Ordering::Release); }
}

/// The captured final mix bypasses processing on replay to avoid applying
/// effects/output gain twice. Rate conversion is the only transformation.
pub(crate) struct Replay { pub audio: AudioBuffer, pub position: f64 }
impl Replay {
    pub fn render(&mut self, rate: u32, out: &mut [[f32; 2]]) -> bool {
        let step = f64::from(self.audio.sample_rate()) / f64::from(rate);
        for frame in out {
            let first = self.position.floor() as usize;
            let fraction = (self.position - first as f64) as f32;
            for (side, value) in frame.iter_mut().enumerate() {
                let a = self.audio.samples().get(first * 2 + side).copied().unwrap_or(0.0);
                let b = self.audio.samples().get((first + 1) * 2 + side).copied().unwrap_or(a);
                *value = a + (b - a) * fraction;
            }
            self.position += step;
        }
        self.position >= self.audio.frames() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_capture_preserves_exact_pcm_and_same_frame_launch_transcript_without_allocating() {
        let capture = PerformanceRecording::prepare(8000, 1).unwrap();
        let request = crate::performance::LaunchRequest { track: 3, pattern: Some(PatternId(42)), quantization: LaunchQuantization::Beat };
        let audio = [[0.25, -0.75], [0.5, -0.5]];
        assert_eq!(crate::test_alloc::allocator_calls(|| {
            capture.begin(100, 240.0, 8000);
            capture.launch(100, request);
            capture.launch(100, crate::performance::LaunchRequest { pattern: None, ..request });
            capture.copy(100, 8000, &audio);
            capture.finish();
        }), 0);
        let saved = capture.snapshot().unwrap();
        assert_eq!(saved.initial_tick, 240.0);
        assert_eq!(saved.audio.samples(), &[0.25, -0.75, 0.5, -0.5]);
        assert_eq!(saved.events.len(), 2);
        assert_eq!(saved.events[0].frame, 0);
        assert_eq!(saved.events[0].pattern, Some(PatternId(42)));
        assert_eq!(saved.events[1].pattern, None);
        let mut replay = Replay { audio: saved.audio.clone(), position: 0.0 };
        let mut heard = [[0.0; 2]; 2];
        assert_eq!(crate::test_alloc::allocator_calls(|| { assert!(replay.render(8000, &mut heard)); }), 0);
        assert_eq!(heard, audio);
    }
    #[test]
    fn capture_refuses_clock_discontinuity_and_event_overflow_without_partial_success() {
        let capture = PerformanceRecording::prepare(8000, 1).unwrap();
        capture.begin(0, 0.0, 8000);
        capture.copy(1, 8000, &[[0.0; 2]]);
        capture.finish();
        assert!(capture.snapshot().is_err());
        let capture = PerformanceRecording::prepare(8000, 1).unwrap();
        capture.begin(0, 0.0, 8000);
        for _ in 0..=MAX_EVENTS { capture.launch(0, crate::performance::LaunchRequest { track: 0, pattern: None, quantization: LaunchQuantization::Immediate }); }
        capture.finish();
        assert!(capture.snapshot().is_err());
    }
    #[test]
    fn processor_performance_record_adoption_copy_completion_and_replay_have_no_allocator_calls() {
        use windfall_project::{Command, Document, InstrumentKind, NoteInit, ChannelId};
        let mut doc = Document::new(windfall_project::Project::new("callback take"));
        let channel = ChannelId(doc.dispatch(Command::AddChannel { name: None, sample: None, instrument: Some(InstrumentKind::SubtractiveSynth), index: None, mixer_track: None }, None).unwrap().created[0]);
        let pattern = doc.project().patterns[0].id;
        doc.dispatch(Command::AddNotes { pattern, channel, notes: vec![NoteInit { start: 0, length: 960, key: 60, velocity: Some(0.8), pan: None, expression: None }] }, None).unwrap();
        let controller = crate::Controller::new();
        controller.set_project(doc.project(), &crate::SamplePool::new());
        let mut processor = controller.attach(48000);
        let capture = PerformanceRecording::prepare(48000, 1).unwrap();
        controller.performance_record_begin(capture.clone()).unwrap();
        controller.performance_launch(crate::performance::LaunchRequest { track: 0, pattern: Some(pattern), quantization: LaunchQuantization::Immediate }).unwrap();
        let mut actual = [0.0; 960];
        assert_eq!(crate::test_alloc::allocator_calls(|| processor.process(&mut actual)), 0);
        controller.performance_record_end();
        let mut ack = [0.0; 2];
        assert_eq!(crate::test_alloc::allocator_calls(|| processor.process(&mut ack)), 0);
        let saved = capture.snapshot().unwrap();
        assert_eq!(saved.audio.samples(), actual);
        assert_eq!(saved.events.len(), 1);
        controller.performance_replay(Some(saved.audio)).unwrap();
        let mut replay = [0.0; 960];
        assert_eq!(crate::test_alloc::allocator_calls(|| processor.process(&mut replay)), 0);
        assert_eq!(replay, actual);
    }
}
