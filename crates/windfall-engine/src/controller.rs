//! The control side of the engine: everything other threads may ask of the
//! audio thread.

use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard};

use rtrb::{Consumer, Producer, PushError, RingBuffer};
use windfall_core::AudioBuffer;
use windfall_ipc::{
    AutomatedValue, GainReduction, PlayMode, RealtimeFrame, TransportPatch, TransportState,
};
use windfall_project::{AutomationId, ChannelId, MAX_GAIN, MAX_KEY, PatternId, Project};

use crate::message::{GARBAGE_CAPACITY, Garbage, MESSAGE_CAPACITY, Message};
use crate::plan::{Plan, compile};
use crate::pool::SamplePool;
use crate::processor::Processor;
use crate::shared::Shared;
use crate::state::{Ledger, PlanState};

#[path = "capture_control.rs"]
pub(crate) mod capture_control;
#[path = "project_preparation.rs"]
pub(crate) mod preparation;
pub use preparation::{
    PreparationSnapshot, PreparedPublication, ProjectPreparationError, ProjectPublicationIntent,
    ProjectPublicationLease, ProjectRetirement, PublicationRefusal,
};

/// An immutable compiled project and prepared sampler pool. Attached native
/// readiness additionally requires a captured engine preparation snapshot.
pub struct PreparedProject {
    plan: Plan,
    sampler_pool: SamplePool,
}

impl PreparedProject {
    pub fn sampler_pool(&self) -> &SamplePool {
        &self.sampler_pool
    }
    fn scratch_asset_refused(&self) -> bool {
        self.plan.tracks.iter().flat_map(|track| &track.effects).any(|effect| {
            matches!(effect.params, windfall_project::EffectParams::Scratch(p) if p.source_id != 0 && p.runtime_source.is_none() && self.sampler_pool.get(windfall_project::SampleId(p.source_id)).is_some())
        })
    }
}

/// Sends requests to the audio thread and reads back what it publishes.
///
/// Clones share one engine. A controller outlives device changes. When the
/// engine replaces its stream because the audio settings changed, the
/// controller hands the project, transport settings and playhead to the new
/// stream's processor, and playback that was running carries on from the
/// playhead. When a stream is lost and none could be opened in its place,
/// playback stops and the playhead returns to where playback started, as
/// it does on [`Controller::stop`].
///
/// Calls never wait for the audio thread. A request takes effect at the
/// start of the next audio buffer, in the order the requests were made.
/// While no stream is open, requests that only make sound (playing, notes,
/// previews) are dropped, and the project, transport settings and playhead
/// are remembered for the next stream.
#[derive(Clone)]
pub struct Controller {
    inner: Arc<Inner>,
}

struct Inner {
    shared: Arc<Shared>,
    state: Mutex<State>,
    sampler_error: Mutex<Option<crate::sampler_processing::SamplerPreparationError>>,
    preparation_error: Mutex<Option<ProjectPreparationError>>,
}

struct State {
    region: Option<windfall_project::TickRange>,
    link: Option<Link>,
    /// The plan most recently compiled, kept to hand to a new processor.
    plan: Arc<Plan>,
    /// What the attached processor holds once it has taken `plan`: which
    /// effects and instruments, how long its delay lines are, its meters.
    /// `None` while no processor is attached.
    hosted: Option<Arc<Ledger>>,
    plan_generation: u64,
    stream_generation: u64,
    stream_phase: preparation::StreamPhase,
    /// The transport as last requested. `playing` here is what was asked
    /// for; the audio thread has the final say once it has caught up.
    transport: TransportState,
    /// Counts play and stop requests, so the audio thread's published
    /// state can be matched to the request it answers.
    sequence: u32,
    output_gain: f32,
    /// Messages that did not fit in the queue, oldest first.
    backlog: VecDeque<Message>,
    /// The stream that went away was playing, and the one that takes its
    /// place carries on. [`Controller::suspend`] decides this and
    /// [`Controller::detach`] calls it off.
    resume: bool,
}

/// The queues to and from the processor currently attached.
struct Link {
    messages: Producer<Message>,
    garbage: Consumer<Garbage>,
    spectrum: Option<crate::spectrum::SpectrumReader>,
    /// The stream's selectable analyzer captures. `None` when the process
    /// had no tap slot left for them.
    captures: Option<capture_control::SharedCaptures>,
}

/// A reading of the transport for control-side timing: see
/// [`Controller::playhead`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playhead {
    pub playing: bool,
    /// The tick being heard: within the pattern in pattern mode, within the
    /// song in song mode.
    pub tick: f64,
    /// How fast the tick moves here, tempo automation included.
    pub ticks_per_second: f64,
    pub mode: PlayMode,
    /// The pattern that plays in pattern mode.
    pub pattern: PatternId,
    /// The length that pattern loops at, when it is in the plan.
    pub pattern_ticks: Option<u32>,
    pub loop_song: bool,
}

/// Measurements the device callback takes while a stream runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamStats {
    /// Frames the device has asked for since the stream started.
    pub frames: u64,
    /// Dropouts since the stream started: the engine took longer to fill a
    /// buffer than the buffer lasts, the driver reported an underrun, or the
    /// device went without audio because it was served too late.
    pub xruns: u32,
    /// Running average of the share of each buffer's duration the engine
    /// spent filling it, 0 to 1.
    pub cpu: f32,
    /// The highest single-buffer share since the last call.
    pub cpu_peak: f32,
}

impl Controller {
    pub fn performance_stream_stamp(&self) -> Option<(u64, u32)> {
        let state = self.lock();
        state.link.as_ref()?;
        Some((state.stream_generation, state.hosted.as_ref()?.sample_rate))
    }
    pub fn performance_record_begin(&self, capture: Arc<crate::performance_recording::PerformanceRecording>) -> Result<(), String> {
        let mut state = self.lock();
        state.maintain();
        if !state.backlog.is_empty() { return Err("Performance recording queue is full".into()); }
        state.link.as_mut().ok_or("Performance recording needs an active audio stream")?.messages
            .push(Message::PerformanceRecordBegin(capture)).map_err(|_| "Performance recording queue is full".into())
    }
    pub fn performance_record_end(&self) { self.lock().send(Message::PerformanceRecordEnd); }
    pub fn performance_replay(&self, audio: Option<AudioBuffer>) -> Result<(), String> {
        if audio.as_ref().is_some_and(|audio| audio.channels() != 2 || audio.frames() == 0 || audio.samples().iter().any(|sample| !sample.is_finite())) {
            return Err("Performance replay needs finite stereo PCM".into());
        }
        let mut state = self.lock();
        state.maintain();
        if !state.backlog.is_empty() { return Err("Performance replay queue is full".into()); }
        state.link.as_mut().ok_or("Performance replay needs an active audio stream")?.messages
            .push(Message::PerformanceReplay(audio)).map_err(|_| "Performance replay queue is full".into())
    }
    /// Admits a bounded performance request without changing song transport.
    pub fn performance_launch(
        &self,
        request: crate::performance::LaunchRequest,
    ) -> Result<(), &'static str> {
        let mut state = self.lock();
        if state.link.is_none() {
            return Err("Performance launching needs an active audio stream");
        }
        if usize::from(request.track) >= crate::performance::TRACKS
            || request
                .pattern
                .is_some_and(|id| state.plan.pattern_ids.get(id.0).is_none())
        {
            return Err("Performance track or pattern no longer exists");
        }
        // No unbounded control-side backlog for launch traffic. Refuse new
        // requests observably, preserving every earlier request and release.
        state.maintain();
        if !state.backlog.is_empty() {
            return Err("Performance launch queue is full");
        }
        let Some(link) = &mut state.link else {
            return Err("Audio stream changed");
        };
        link.messages
            .push(Message::Performance(request))
            .map_err(|_| "Performance launch queue is full")
    }
    pub fn performance_scene(
        &self,
        requests: [Option<crate::performance::LaunchRequest>; crate::performance::TRACKS],
    ) -> Result<(), &'static str> {
        let mut state = self.lock();
        state.maintain();
        if state.link.is_none() {
            return Err("Performance launching needs an active audio stream");
        }
        if requests.iter().flatten().any(|request| {
            usize::from(request.track) >= crate::performance::TRACKS
                || request
                    .pattern
                    .is_some_and(|id| state.plan.pattern_ids.get(id.0).is_none())
        }) {
            return Err("A scene pattern no longer exists");
        }
        if !state.backlog.is_empty() {
            return Err("Performance launch queue is full");
        }
        state
            .link
            .as_mut()
            .ok_or("Audio stream changed")?
            .messages
            .push(Message::PerformanceScene(requests))
            .map_err(|_| "Performance launch queue is full")
    }
    pub fn performance_stop_all(&self) {
        self.lock().send(Message::PerformanceStopAll);
    }
    pub fn performance_state(&self) -> windfall_ipc::PerformanceState {
        let state = self.lock();
        let shared = &self.inner.shared;
        windfall_ipc::PerformanceState {
            available: state.link.is_some(),
            tick: f64::from_bits(shared.performance_tick.load(Ordering::Acquire)),
            dropped_notes: shared
                .performance_dropped
                .load(Ordering::Acquire)
                .to_string(),
            tracks: shared
                .performance_slots
                .iter()
                .map(|slot| {
                    let value = slot.load(Ordering::Acquire);
                    let active = value as u32;
                    let pending = (value >> 32) as u32;
                    windfall_ipc::PerformanceTrackState {
                        active: (active != 0).then_some(PatternId(active)),
                        queued: (pending != 0 && pending != u32::MAX).then_some(PatternId(pending)),
                        queued_stop: pending == u32::MAX,
                    }
                })
                .collect(),
        }
    }

    /// Selects a post-fader utility source without rebuilding the musical plan.
    /// Interest is installed off audio; unrequested tracks perform no history work.
    pub fn set_waveform_tracks(&self, ids: &[windfall_project::TrackId]) {
        let ids = &ids[..ids.len().min(crate::waveform_meter::MAX_VISIBLE)];
        let state = self.lock();
        for (index, slot) in self.inner.shared.waveforms.iter().enumerate() {
            let id = state
                .plan
                .tracks
                .get(index)
                .filter(|track| ids.contains(&track.id))
                .map_or(u32::MAX, |track| track.id.0);
            slot.requested.store(id, Ordering::Release);
        }
    }

    pub fn clear_waveform_history(&self) {
        self.inner
            .shared
            .waveform_epoch
            .fetch_add(1, Ordering::AcqRel);
        for slot in self.inner.shared.waveforms.iter() {
            slot.requested.store(u32::MAX, Ordering::Release);
        }
    }

    pub fn set_current_track(&self, track: Option<windfall_project::TrackId>) {
        self.inner
            .shared
            .current_track
            .store(track.map_or(u32::MAX, |track| track.0), Ordering::Release);
    }
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                shared: Arc::new(Shared::new()),
                sampler_error: Mutex::new(None),
                preparation_error: Mutex::new(None),
                state: Mutex::new(State {
                    region: None,
                    link: None,
                    plan: Arc::new(Plan::empty()),
                    hosted: None,
                    plan_generation: 0,
                    stream_generation: 0,
                    stream_phase: preparation::StreamPhase::Detached,
                    transport: TransportState {
                        playing: false,
                        mode: PlayMode::Pattern,
                        pattern: PatternId(0),
                        loop_song: false,
                        metronome: None,
                        count_in_bars: None,
                        count_in_remaining: None,
                    },
                    sequence: 0,
                    output_gain: 1.0,
                    backlog: VecDeque::new(),
                    resume: false,
                }),
            }),
        }
    }

    /// Makes the engine play this project. Call it again after every edit:
    /// sounding notes carry on, and gain, pan and mute changes glide on
    /// whatever those notes are heard through. A channel or mixer track
    /// that nothing is sounding through takes its new values at once, and
    /// notes that are already fading out, as after [`Controller::stop`], do
    /// not count where only faders lie between them and the output: they
    /// finish at the levels they had. So the first project, and one that
    /// replaces another, play at their own levels from the first frame.
    ///
    /// Effects and instruments are told apart by their ids. One the engine
    /// already runs keeps running across the edit with everything it
    /// remembers, such as a reverb's tail, and is only told what changed
    /// about it; it glides to new settings. One that is new is built and
    /// prepared here, on the caller's thread, which allocates and can take
    /// a moment. An effect that joins or leaves a track sound is passing
    /// through fades in or out over 5 ms, and an instrument whose channel
    /// is gone stops with a short fade.
    ///
    /// Samples missing from `pool` leave their channels silent. If the
    /// transport's pattern is not in the project, the transport moves to the
    /// project's first pattern.
    /// Call outside document/recording guards: preparation and retirement run
    /// on this caller's thread. Guarded document edits use borrowed publication.
    pub fn set_project(&self, project: &Project, pool: &SamplePool) {
        // T1's invalid-meter precheck precedes even bounded provider metadata
        // reads, so a refused convenience edit leaves native owners untouched.
        let result = Self::try_prepare_project(project, pool)
            .map_err(ProjectPreparationError::Sampler)
            .and_then(|prepared| {
                self.preparation_snapshot()
                    .prepare_compiled(prepared, ProjectPublicationIntent::Edit)
            });
        self.publish_convenience(result);
    }

    fn publish_convenience(&self, result: Result<PreparedPublication, ProjectPreparationError>) {
        match result {
            Ok(mut prepared) => {
                let mut retirement = None;
                let result = match self.publication(&mut prepared) {
                    Ok(lease) => {
                        retirement = Some(lease.install());
                        Ok(())
                    }
                    Err(reason) => Err(ProjectPreparationError::Publication(reason)),
                };
                if let Err(error) = result {
                    self.latch_preparation_error(error);
                } else {
                    *self
                        .inner
                        .sampler_error
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = None;
                    *self
                        .inner
                        .preparation_error
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = None;
                }
                if let Some(retirement) = &mut retirement {
                    self.take_retired(retirement);
                }
                // Both the candidate and retired native owners drop after the
                // publication guard has gone. Production Session adoption of
                // the borrowed interface is the separate stage2 integration.
            }
            Err(error) => {
                if let ProjectPreparationError::Sampler(sampler) = &error {
                    *self
                        .inner
                        .sampler_error
                        .lock()
                        .unwrap_or_else(|error| error.into_inner()) = Some(sampler.clone());
                }
                self.latch_preparation_error(error);
            }
        }
    }

    /// A refused convenience publication keeps the old plan and exposes its reason.
    pub fn sampler_preparation_error(
        &self,
    ) -> Option<crate::sampler_processing::SamplerPreparationError> {
        self.inner
            .sampler_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub fn sampler_key_supported(&self, channel: ChannelId, key: u8) -> bool {
        let state = self.lock();
        state.plan.channel(channel).is_some_and(|index| {
            let sampler = &state.plan.channels[index].sampler;
            !sampler.spectral
                || sampler
                    .bank
                    .as_ref()
                    .is_some_and(|bank| bank.at(key).is_some())
        })
    }

    /// Compiles and renders spectral clips on the caller's worker/control thread.
    pub fn prepare_project(
        project: &Project,
        pool: &SamplePool,
    ) -> Result<PreparedProject, crate::sampler_processing::SamplerPreparationError> {
        Self::try_prepare_project(project, pool)
    }

    /// Fallible bounded preparation. Must run outside document/audio locks.
    pub fn try_prepare_project(
        project: &Project,
        pool: &SamplePool,
    ) -> Result<PreparedProject, crate::sampler_processing::SamplerPreparationError> {
        windfall_project::timeline::MeterMap::checked(
            project.settings.time_signature,
            &project.playlist.timeline.meters,
        )
        .map_err(|_| {
            crate::sampler_processing::SamplerPreparationError::Unsupported(
                "Invalid song meter map; project preparation refused.",
            )
        })?;
        let pool = pool.prepare_samplers(project, &mut || true, &mut |_, _, _| {})?;
        let prepared = Self::compile_prepared_project(project, &pool);
        if prepared.scratch_asset_refused() {
            return Err(
                crate::sampler_processing::SamplerPreparationError::Unsupported(
                    "The scratch source exceeds the effect asset memory budget; restart to release retained sources.",
                ),
            );
        }
        Ok(prepared)
    }

    /// Compile already prepared sampler banks; never performs sampler DSP.
    pub fn compile_prepared_project(project: &Project, pool: &SamplePool) -> PreparedProject {
        PreparedProject {
            plan: compile(project, pool),
            sampler_pool: pool.clone(),
        }
    }

    /// Installs a precompiled snapshot. The caller must verify it still matches the project.
    pub fn set_prepared_project(&self, project: &Project, prepared: PreparedProject) {
        if prepared.scratch_asset_refused() {
            self.latch_preparation_error(ProjectPreparationError::Unsupported("The scratch source exceeds the effect asset memory budget; restart to release retained sources."));
            return;
        }
        if self.refuse_invalid_meters(&prepared.plan) {
            return;
        }
        let snapshot = self.preparation_snapshot();
        if snapshot.is_attached() {
            self.latch_preparation_error(ProjectPreparationError::Unsupported(
                "Attached project publication requires an engine readiness token.",
            ));
            return;
        }
        let _ = project;
        self.publish_convenience(
            snapshot.prepare_compiled(prepared, ProjectPublicationIntent::Edit),
        );
    }

    pub(crate) fn set_plan(&self, plan: Plan) {
        if self.refuse_invalid_meters(&plan) {
            return;
        }
        let prepared = PreparedProject {
            plan,
            sampler_pool: SamplePool::new(),
        };
        self.publish_convenience(
            self.preparation_snapshot()
                .prepare_compiled(prepared, ProjectPublicationIntent::Edit),
        );
    }

    /// A failed immutable snapshot never replaces the installed project.
    /// Called only by the control-side publication paths, before State.
    fn refuse_invalid_meters(&self, plan: &Plan) -> bool {
        if plan.meters.is_ok() {
            return false;
        }
        *self
            .inner
            .sampler_error
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(
            crate::sampler_processing::SamplerPreparationError::Unsupported(
                "Invalid song meter map; project preparation refused.",
            ),
        );
        true
    }

    /// Frames by which the project's instruments and effects delay the
    /// output of the stream that is open, at its sample rate. Zero with no
    /// stream. Every path through the mixer is delayed to match the slowest
    /// one, so this one figure holds for the whole mix.
    pub fn latency_frames(&self) -> u32 {
        let state = self.lock();
        state.hosted.as_ref().map_or(0, |hosted| hosted.latency)
    }

    /// Starts playback from the playhead. Does nothing while playing.
    /// Source notes refused by the fixed 128-note table of an internal controller.
    pub fn rejected_control_notes(&self) -> u32 {
        self.inner
            .shared
            .rejected_control_notes
            .load(Ordering::Relaxed)
    }

    pub fn play(&self) {
        self.start(None);
    }

    /// A recording pre-roll on the output clock, then playback at the playhead.
    pub fn play_count_in(&self, bars: u8) {
        let mut state = self.lock();
        if state.link.is_none() {
            return;
        }
        state.transport.playing = true;
        state.sequence = state.sequence.wrapping_add(1);
        let sequence = state.sequence;
        self.inner
            .shared
            .count_in_remaining
            .store(1, Ordering::Release);
        state.send(Message::CountIn {
            sequence,
            bars: bars.min(8),
            capture: self.inner.shared.recording_clock.active_ticket(),
        });
    }

    pub fn recording_clock(&self) -> crate::recording_clock::RecordingClock {
        self.inner.shared.recording_clock.clone()
    }
    pub fn start_input_monitor(
        &self,
        settings: windfall_ipc::RecordingMonitorSettings,
        rate: u32,
    ) -> Result<crate::recording_monitor::MonitorWriter, String> {
        self.start_input_monitors(vec![settings], rate)
            .map(|mut writers| writers.remove(0))
    }
    pub fn start_input_monitors(
        &self,
        settings: Vec<windfall_ipc::RecordingMonitorSettings>,
        rate: u32,
    ) -> Result<Vec<crate::recording_monitor::MonitorWriter>, String> {
        if settings.len() > windfall_project::MAX_MIXER_TRACKS || rate == 0 {
            return Err("Monitor routes exceed the mixer capacity.".into());
        }
        let mut state = self.lock();
        if state.link.is_none() {
            return Err("Open an output and choose an existing monitor mixer track.".into());
        }
        for setting in &settings {
            if !setting.gain.is_finite()
                || !(0.0..=1.0).contains(&setting.gain)
                || !(5..=100).contains(&setting.buffer_ms)
                || state.plan.track_ids.get(setting.track.0).is_none()
            {
                return Err(
                    "Choose an existing monitor track, gain 0–1 and buffering 5–100 ms.".into(),
                );
            }
        }
        let (writers, readers): (Vec<_>, Vec<_>) = settings
            .into_iter()
            .map(|setting| crate::recording_monitor::channel(setting, rate))
            .unzip();
        state.send(Message::SetInputMonitors(readers.into_boxed_slice()));
        Ok(writers)
    }
    pub fn stop_input_monitor(&self) {
        self.lock()
            .send(Message::SetInputMonitors(Vec::new().into_boxed_slice()));
    }
    pub fn start_disk_taps(
        &self,
        taps: Vec<(windfall_ipc::RecordingMixerTap, f64)>,
        gate: crate::recording_clock::CaptureGate,
        rate: u32,
    ) -> Result<Vec<crate::recording_disk::DiskReader>, String> {
        let mut state = self.lock();
        if state.link.is_none() || rate == 0 || taps.len() > windfall_project::MAX_MIXER_TRACKS {
            return Err("Open an output before mixer disk recording.".into());
        }
        for (tap, offset) in &taps {
            if tap.mode == windfall_project::MixerRecordMode::Input
                || state.plan.track_ids.get(tap.track.0).is_none()
                || !offset.is_finite()
                || offset.abs() > 1000.0
            {
                return Err("Choose a mixer track, processed recording source and an offset within 1000 ms.".into());
            }
        }
        let (readers, writers): (Vec<_>, Vec<_>) = taps
            .into_iter()
            .map(|(tap, offset)| crate::recording_disk::channel(tap, gate.clone(), rate, offset))
            .unzip();
        state.send(Message::SetDiskTaps(writers.into_boxed_slice()));
        Ok(readers)
    }
    pub fn stop_disk_taps(&self) {
        self.lock()
            .send(Message::SetDiskTaps(Vec::new().into_boxed_slice()));
    }

    /// The stream attached now: a number that changes whenever another takes
    /// its place, and its sample rate. See `audio_logger.rs`.
    pub(crate) fn attached_stream(&self) -> Option<(u64, u32)> {
        let state = self.lock();
        state.link.as_ref()?;
        let rate = state.hosted.as_ref()?.sample_rate;
        Some((state.stream_generation, rate))
    }

    /// Hands the audio thread the audio logger's tap, or takes its tap away.
    /// A tap built for `stream` is refused once another stream is attached.
    pub(crate) fn set_audio_logger_tap(
        &self,
        stream: Option<u64>,
        tap: Option<Box<crate::audio_logger::LoggerTap>>,
    ) -> Result<(), String> {
        let refused = {
            let mut state = self.lock();
            let current = state.link.is_some()
                && stream.is_none_or(|stream| stream == state.stream_generation);
            if current {
                state.send(Message::SetAudioLogger(tap));
                return Ok(());
            }
            tap
        };
        // Dropped here, with the guard released: nothing was installed.
        drop(refused);
        match stream {
            Some(_) => Err("The audio output changed while the audio logger was starting.".into()),
            None => Ok(()),
        }
    }

    /// Exact nominal sample span through the compiled song tempo map.
    pub fn song_range_frames(
        &self,
        range: windfall_project::TickRange,
        rate: u32,
    ) -> Result<f64, String> {
        range.check()?;
        let state = self.lock();
        let ticks = state.plan.warp(f64::from(range.end)) - state.plan.warp(f64::from(range.start));
        Ok(ticks * windfall_core::samples_per_tick(state.plan.tempo_bpm, f64::from(rate)))
    }
    pub fn song_tick_after_seconds(&self, start: u32, seconds: f64) -> f64 {
        let state = self.lock();
        let ticks = seconds.max(0.0) * state.plan.tempo_bpm * f64::from(windfall_core::PPQ) / 60.0;
        state.plan.unwarp(state.plan.warp(f64::from(start)) + ticks)
    }

    pub fn count_in_remaining(&self) -> u32 {
        self.inner.shared.count_in_remaining.load(Ordering::Acquire)
    }

    /// Temporarily gives a recorded gesture ownership over an existing curve.
    /// It does not mutate the saved document and is never used by offline rendering.
    pub fn automation_touch(&self, target: windfall_project::AutomationTarget, value: Option<f32>) {
        if value.is_none_or(f32::is_finite) {
            self.lock().send(Message::AutomationTouch { target, value });
        }
    }

    /// Plays the pattern or song `passes` times, then stops without cutting
    /// off what is still sounding. The offline renderer uses this.
    pub(crate) fn play_passes(&self, passes: u32) {
        self.start(Some(passes));
    }

    fn start(&self, passes: Option<u32>) {
        let mut state = self.lock();
        if state.link.is_none() || self.playing(&state) {
            return;
        }
        state.transport.playing = true;
        state.sequence = state.sequence.wrapping_add(1);
        let sequence = state.sequence;
        state.send(Message::Play {
            sequence,
            passes,
            from: None,
        });
    }

    /// Stops playback, fades out every note, and returns the playhead to
    /// where playback last started. What automation was moving stays where
    /// the song had it until everything has rung out. Stopping again
    /// returns it to the stored values right away, over 100 ms.
    pub fn stop(&self) {
        self.inner.shared.recording_clock.close_active();
        let mut state = self.lock();
        self.panic_hardware();
        state.transport.playing = false;
        state.sequence = state.sequence.wrapping_add(1);
        let sequence = state.sequence;
        state.send(Message::Stop { sequence });
        if state.link.is_none() {
            // No audio thread is there to act on this, so the playhead is
            // returned here, and a stream on its way does not start playing.
            state.resume = false;
            let shared = &self.inner.shared;
            shared.set_tick(shared.start());
        }
    }

    /// Moves the playhead to `tick`. While playing, sequenced notes fade out
    /// and playback carries on from there.
    pub fn seek(&self, tick: f64) {
        self.inner.shared.recording_clock.close_active();
        let tick = if tick.is_finite() { tick.max(0.0) } else { 0.0 };
        let mut state = self.lock();
        let tick = if state.transport.mode == PlayMode::Song {
            state
                .region
                .map_or(tick, |r| tick.clamp(f64::from(r.start), f64::from(r.end)))
        } else {
            tick
        };
        // Shown at once, and kept for the next stream when none is open.
        self.inner.shared.set_tick(tick);
        self.inner.shared.set_start(tick);
        state.send(Message::Seek(tick));
    }

    /// Changes the play mode, the pattern that plays in pattern mode, or
    /// song looping. Changing the mode moves the playhead to the start.
    pub fn set_transport(&self, patch: TransportPatch) {
        let mut state = self.lock();
        if let Some(mode) = patch.mode {
            if mode != state.transport.mode {
                self.inner.shared.set_tick(0.0);
                self.inner.shared.set_start(0.0);
            }
            state.transport.mode = mode;
        }
        if let Some(pattern) = patch.pattern {
            state.transport.pattern = pattern;
        }
        if let Some(loop_song) = patch.loop_song {
            state.transport.loop_song = loop_song;
        }
        if let Some(mut metronome) = patch.metronome {
            metronome.gain = if metronome.gain.is_finite() {
                metronome.gain.clamp(0.0, 1.0)
            } else {
                0.25
            };
            state.transport.metronome = Some(metronome);
        }
        if let Some(bars) = patch.count_in_bars {
            state.transport.count_in_bars = Some(bars.min(8));
        }
        state.send_transport();
    }

    pub fn transport(&self) -> TransportState {
        let mut state = self.lock();
        state.maintain();
        TransportState {
            playing: self.playing(&state),
            count_in_remaining: Some(self.count_in_remaining()),
            ..state.transport
        }
    }

    /// Where playback is and how fast it moves, for timing live input and
    /// MIDI clock on the control side. The tick is the one last published by
    /// the audio thread, so it steps once a buffer; callers that need finer
    /// time carry on from it at `ticks_per_second`.
    pub fn playhead(&self) -> Playhead {
        let mut state = self.lock();
        state.maintain();
        let tick = self.inner.shared.tick();
        let mode = state.transport.mode;
        let stored = state.plan.tempo_bpm * f64::from(windfall_core::PPQ) / 60.0;
        // Tempo automation bends the song's clock; a pattern plays at the
        // stored tempo.
        let ticks_per_second = if mode == PlayMode::Song {
            const SPAN: f64 = 0.01;
            let ahead = state.plan.unwarp(state.plan.warp(tick) + stored * SPAN);
            let rate = (ahead - tick) / SPAN;
            if rate.is_finite() && rate > 0.0 {
                rate
            } else {
                stored
            }
        } else {
            stored
        };
        let pattern = state.transport.pattern;
        Playhead {
            playing: self.playing(&state),
            tick,
            ticks_per_second,
            mode,
            pattern,
            pattern_ticks: state
                .plan
                .pattern_ids
                .get(pattern.0)
                .map(|index| state.plan.patterns[index].length),
            loop_song: state.transport.loop_song,
        }
    }

    /// Control-side surface feedback reads normalized audible automation only,
    /// without requesting analysis snapshots or taking the engine control lock.
    pub fn controller_automation_values(&self) -> Vec<(u32, f32)> {
        let mut values = Vec::new();
        self.inner.shared.automated(&mut values);
        values
    }

    /// Frame corresponding to the published playhead in the DAC clock.
    pub fn playhead_output_frame(&self) -> u64 {
        self.inner.shared.position_frame.load(Ordering::Acquire)
    }

    /// Takes the messages MIDI output channels have queued, oldest first,
    /// each with the engine frame it belongs to. One thread at a time may
    /// call this: it is the MIDI worker's. Never blocks the audio thread.
    pub fn drain_midi_out(&self, mut out: impl FnMut(u64, &[u8])) {
        self.drain_midi_out_from(|frame, bytes, _| out(frame, bytes));
    }

    pub fn drain_midi_out_from(&self, mut out: impl FnMut(u64, &[u8], u8)) {
        for _ in 0..crate::shared::MIDI_OUT_CAPACITY {
            let Some((frame, bytes, len, port)) = self.inner.shared.pop_midi_out_from() else {
                break;
            };
            out(frame, &bytes[..usize::from(len.min(3))], port);
        }
    }

    /// MIDI output messages lost because the worker did not keep up.
    pub fn midi_out_dropped(&self) -> u32 {
        self.inner.shared.midi_out_dropped()
    }

    /// Session-only song region, preserved across stream replacement.
    pub fn set_timeline_region(
        &self,
        region: Option<windfall_project::TickRange>,
    ) -> Result<(), String> {
        if let Some(range) = region {
            range.check()?;
        }
        let mut state = self.lock();
        state.region = region;
        state.send(Message::SetRegion(region));
        Ok(())
    }

    pub fn timeline_region(&self) -> Option<windfall_project::TickRange> {
        self.lock().region
    }

    pub fn navigation_overflows(&self) -> u32 {
        self.inner
            .shared
            .navigation_overflows
            .load(Ordering::Relaxed)
    }

    /// Notes that played without their echoes since the engine started,
    /// because a channel's fixed echo queue or its table of held notes was
    /// full. The dry note always sounds.
    pub fn rejected_echoes(&self) -> u32 {
        self.inner.shared.rejected_echoes.load(Ordering::Relaxed)
    }

    /// Plays a note on a channel right away, as from the UI keyboard.
    /// `velocity` runs from 0 to 1.
    pub fn note_on(&self, channel: ChannelId, key: u8, velocity: f32) {
        let velocity = if velocity.is_finite() {
            velocity.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.lock().send(Message::NoteOn {
            channel,
            key: key.min(MAX_KEY),
            velocity,
        });
    }

    /// Ends a note started with [`Controller::note_on`]. A sampler with no
    /// envelope plays its sample to the end regardless.
    pub fn note_off(&self, channel: ChannelId, key: u8) {
        self.lock().send(Message::NoteOff {
            channel,
            key: key.min(MAX_KEY),
        });
    }

    /// Hardware input uses this epoch both at capture and at audio delivery.
    pub fn hardware_epoch(&self) -> u64 {
        self.inner.shared.hardware_epoch.load(Ordering::Acquire)
    }

    /// Queue-independent panic. The next audio buffer fades hardware voices and
    /// discards old hardware messages, even when its message/garbage queues are full.
    pub fn panic_hardware(&self) {
        self.inner
            .shared
            .hardware_epoch
            .fetch_add(1, Ordering::AcqRel);
    }

    /// Control-worker only. Never grows the general control backlog for MIDI.
    /// False means unavailable/full/stale; the MIDI worker must panic on failure.
    pub fn hardware_note(&self, epoch: u64, channel: ChannelId, key: u8, velocity: u8) -> bool {
        let mut state = self.lock();
        state.maintain();
        if epoch != self.hardware_epoch()
            || key > MAX_KEY
            || velocity > 127
            || !state.backlog.is_empty()
            || state.plan.channel(channel).is_none()
        {
            return false;
        }
        state.link.as_mut().is_some_and(|link| {
            link.messages
                .push(Message::HardwareNote {
                    epoch,
                    channel,
                    key,
                    velocity,
                })
                .is_ok()
        })
    }

    /// Plays a sample once, straight into the master track, whether or not
    /// the transport is running. A new preview replaces the one playing.
    /// Previews play at [`PREVIEW_GAIN_DB`](crate::PREVIEW_GAIN_DB), so one
    /// heard over a full mix does not clip the master.
    pub fn preview(&self, sample: AudioBuffer) {
        self.lock().send(Message::Preview(sample));
    }

    pub fn stop_preview(&self) {
        self.lock().send(Message::StopPreview);
    }

    /// Sets a gain applied at the very end of the chain, after the master
    /// track and its meter. At zero the engine still does all its work and
    /// the meters still move, but nothing is heard.
    pub fn set_output_gain(&self, gain: f32) {
        let gain = if gain.is_finite() {
            gain.clamp(0.0, MAX_GAIN)
        } else {
            0.0
        };
        let mut state = self.lock();
        state.output_gain = gain;
        state.send(Message::SetOutputGain(gain));
    }

    /// The playhead, meters and load for the UI. Reading the meters resets
    /// them, so each frame reports the peaks since the frame before.
    ///
    /// The playhead is where the sound now leaving the engine is in the
    /// song: the latency of instruments and effects is taken off it, so it
    /// is no further ahead of what is heard than the device's own buffer
    /// puts it.
    pub fn frame(&self) -> RealtimeFrame {
        let mut state = self.lock();
        state.maintain();
        let shared = &self.inner.shared;
        let plan = Arc::as_ptr(&state.plan) as usize;
        let (spectrum, spectrogram, correlation) = state
            .link
            .as_mut()
            .and_then(|link| link.spectrum.as_mut())
            .and_then(|reader| reader.snapshot(plan))
            .map_or_else(
                || (Vec::new(), Vec::new(), None),
                |snapshot| {
                    (
                        crate::spectrum::summarize(snapshot),
                        crate::spectrum::summarize_history(snapshot),
                        frame_correlation(snapshot.stereo.correlation),
                    )
                },
            );
        let meters = state
            .hosted
            .as_ref()
            .map_or(&[][..], |hosted| &hosted.meters);
        let mut automated = Vec::new();
        if state.link.is_some() {
            shared.automated(&mut automated);
        }
        RealtimeFrame {
            rejected_echoes: Some(shared.rejected_echoes.load(Ordering::Relaxed)),
            media_clock: state
                .link
                .as_ref()
                .and_then(|_| state.hosted.as_ref())
                .map(|hosted| windfall_ipc::MediaClock {
                    epoch: shared
                        .presentation_epoch
                        .load(Ordering::Acquire)
                        .to_string(),
                    output_frame: shared
                        .presentation_frame
                        .load(Ordering::Acquire)
                        .to_string(),
                    sample_rate: hosted.sample_rate,
                    pdc_frames: state.hosted.as_ref().map_or(0, |ledger| ledger.latency),
                    device_latency_frames: None,
                    playing: shared.transport().1,
                    mode: state.transport.mode,
                    tick: shared.tick(),
                }),
            active_note_poses: if state.link.is_some() {
                shared
                    .note_poses
                    .iter()
                    .filter_map(|slot| {
                        let packed = slot.load(Ordering::Acquire);
                        (packed != 0).then_some(windfall_ipc::ActiveNotePose {
                            channel: ChannelId(packed as u32),
                            key: ((packed >> 32) & 127) as u8,
                            velocity: ((packed >> 40) & 127) as f32 / 127.0,
                        })
                    })
                    .collect()
            } else {
                Vec::new()
            },
            correlation,
            spectrum,
            spectrogram,
            waveforms: if state.link.is_some() {
                let epoch = shared.waveform_epoch.load(Ordering::Acquire);
                state
                    .plan
                    .tracks
                    .iter()
                    .enumerate()
                    .filter_map(|(index, track)| shared.waveforms[index].snapshot(track.id, epoch))
                    .take(crate::waveform_meter::MAX_VISIBLE)
                    .collect()
            } else {
                Vec::new()
            },
            playing: self.playing(&state),
            tick: shared.tick(),
            meters: (0..state.plan.tracks.len() * 2)
                .map(|index| shared.take_meter(index))
                .collect(),
            cpu: shared.load().clamp(0.0, 1.0),
            xruns: shared.xruns(),
            voices: shared.voices(),
            audio_clips: shared.audio_clips(),
            dropped_clips: shared.dropped_clips(),
            rejected_rack_events: Some(shared.rejected_rack_events.load(Ordering::Relaxed))
                .filter(|count| *count != 0),
            gain_reductions: meters
                .iter()
                .map(|(effect, meter)| GainReduction {
                    effect: *effect,
                    db: meter.take_db(),
                })
                .collect(),
            automated: automated
                .into_iter()
                .map(|(automation, value)| AutomatedValue {
                    automation: AutomationId(automation),
                    value,
                })
                .collect(),
        }
    }

    /// What the device callback has measured. Reading resets `cpu_peak`.
    pub fn stream_stats(&self) -> StreamStats {
        let shared = &self.inner.shared;
        StreamStats {
            frames: shared.frames(),
            xruns: shared.xruns(),
            cpu: shared.load(),
            cpu_peak: shared.take_load_peak(),
        }
    }

    pub(crate) fn shared(&self) -> &Arc<Shared> {
        &self.inner.shared
    }

    /// Creates the processor for a new stream and makes it the one this
    /// controller talks to. It starts on the current project and transport
    /// settings, with stop returning to where it did before. After
    /// [`Controller::suspend`] found the transport playing, it carries on
    /// from the playhead; otherwise it starts out stopped.
    ///
    /// May be called again before any audio was processed, as when a stream
    /// could not be built and another kind is tried: the processor made
    /// last is the one that counts.
    pub(crate) fn attach(&self, sample_rate: u32) -> Processor {
        self.inner.shared.recording_clock.reset_output();
        self.inner
            .shared
            .presentation_epoch
            .fetch_add(1, Ordering::AcqRel);
        for slot in &self.inner.shared.performance_slots {
            slot.store(0, Ordering::Release);
        }
        for slot in &self.inner.shared.note_poses {
            slot.store(0, Ordering::Release);
        }
        let (processor, _, retirement) = self
            .try_attach(
                sample_rate,
                crate::project_preparation::AttachmentMode::Independent,
            )
            .expect("independent processor preparation failed");
        drop(retirement);
        processor
    }

    #[cfg(test)]
    pub(crate) fn suspend(&self) {
        self.inner.shared.recording_clock.reset_output();
        let mut state = self.lock();
        self.panic_hardware();
        state.resume = self.playing(&state) && self.count_in_remaining() == 0;
        if self.count_in_remaining() > 0 {
            state.transport.playing = false;
        }
        self.inner
            .shared
            .count_in_remaining
            .store(0, Ordering::Release);
        state.link = None;
        state.hosted = None;
        state.backlog.clear();
        let shared = &self.inner.shared;
        shared.publish_position(shared.tick(), shared.start(), 0);
        shared.publish_clips(0, 0);
    }

    pub(crate) fn detach(&self) {
        self.inner.shared.recording_clock.reset_output();
        self.inner
            .shared
            .count_in_remaining
            .store(0, Ordering::Release);
        // A failed stream build can leave an admitted attachment in Starting.
        // Close through the same generation fence as a running stream so a
        // later reopen can prepare, and retire owners outside the state guard.
        if let Ok(closing) = self.begin_close(false) {
            self.finish_close(closing);
        }
    }

    /// Whether the transport is playing: the audio thread's word once it
    /// has handled the latest play or stop, and the request until then.
    fn playing(&self, state: &State) -> bool {
        let (sequence, playing) = self.inner.shared.transport();
        if sequence == state.sequence {
            playing
        } else {
            state.transport.playing
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        // The state stays usable if a thread panicked while holding it.
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl State {
    /// Queues a message for the audio thread. Without a stream it is
    /// dropped: what a later stream needs is kept in this struct instead.
    fn send(&mut self, message: Message) {
        self.maintain();
        let Some(link) = &mut self.link else {
            return;
        };
        if !self.backlog.is_empty() {
            self.backlog.push_back(message);
        } else if let Err(PushError::Full(message)) = link.messages.push(message) {
            self.backlog.push_back(message);
        }
    }

    fn send_transport(&mut self) {
        self.send(Message::SetRegion(self.region));
        self.send(Message::SetTransport {
            mode: self.transport.mode,
            pattern: self.transport.pattern,
            loop_song: self.transport.loop_song,
            metronome: self.transport.metronome.unwrap_or_default(),
        });
    }

    /// Moves waiting messages
    /// into the queue while there is room. Retirement is explicit: queries
    /// under an external document guard must never free native owners.
    fn maintain(&mut self) {
        let Some(link) = &mut self.link else {
            return;
        };
        for _ in 0..MESSAGE_CAPACITY {
            let Some(message) = self.backlog.pop_front() else {
                break;
            };
            if let Err(PushError::Full(message)) = link.messages.push(message) {
                self.backlog.push_front(message);
                break;
            }
        }
    }
}

/// Copies existing analyzer evidence on the control-side frame path only.
fn frame_correlation(correlation: Option<f64>) -> Option<f32> {
    correlation
        .filter(|value| value.is_finite() && (-1.0..=1.0).contains(value))
        .map(|value| value as f32)
}

#[cfg(test)]
mod tests {
    use windfall_core::TICKS_PER_STEP;
    use windfall_project::{
        Channel, ChannelSource, Lane, Note, NoteId, SampleId, SamplerSettings, TrackId,
    };

    use super::*;

    #[test]
    fn phase_meter_publication_preserves_absence_zero_and_endpoints() {
        assert_eq!(frame_correlation(None), None);
        for value in [-1.0, 0.0, 0.25, 1.0] {
            assert_eq!(frame_correlation(Some(value)), Some(value as f32));
        }
    }

    #[test]
    fn phase_meter_publication_rejects_nonfinite_and_out_of_range_values() {
        for value in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -1.0 - f64::EPSILON,
            1.0 + f64::EPSILON,
            f64::MAX,
        ] {
            assert_eq!(frame_correlation(Some(value)), None);
        }
    }

    /// A project and pool with one channel that plays a single full-scale
    /// frame on each of `steps`.
    fn clicks(steps: &[u32]) -> (Project, SamplePool) {
        let mut project = Project::new("clicks");
        let mut pool = SamplePool::new();
        pool.insert(
            SampleId(900),
            AudioBuffer::from_interleaved(48_000, 1, vec![1.0]),
        );
        project.channels.push(Channel {
            sample_playback: None,
            id: ChannelId(901),
            name: String::new(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            group: String::new(),
            voice: Default::default(),
            expression_map: Default::default(),
            sample_instrument: None,
            timing: windfall_project::ChannelTiming::default(),
            mixer_track: TrackId::MASTER,
            source: ChannelSource::Sampler(SamplerSettings {
                sample: Some(SampleId(900)),
                ..SamplerSettings::default()
            }),
        });
        project.patterns[0].lanes.push(Lane {
            channel: ChannelId(901),
            notes: steps
                .iter()
                .map(|step| Note {
                    id: NoteId(1_000 + step),
                    start: step * TICKS_PER_STEP,
                    length: TICKS_PER_STEP,
                    key: 60,
                    velocity: 1.0,
                    pan: 0.0,
                    expression: Default::default(),
                })
                .collect(),
        });
        (project, pool)
    }

    /// Processes `frames` frames and returns the frames that were not
    /// silent.
    fn run(processor: &mut Processor, frames: usize) -> Vec<usize> {
        let mut out = vec![0.0; frames * 2];
        for block in out.chunks_mut(256) {
            processor.process(block);
        }
        (0..frames).filter(|frame| out[frame * 2] != 0.0).collect()
    }

    #[derive(Debug, Default)]
    struct R6Native {
        revision: std::sync::atomic::AtomicU64,
        creates: std::sync::atomic::AtomicUsize,
        prepared_revision: [std::sync::atomic::AtomicU64; 8],
        processes: [std::sync::atomic::AtomicUsize; 8],
        drops: [std::sync::atomic::AtomicUsize; 8],
    }
    #[derive(Debug)]
    struct R6Factory(Arc<R6Native>);
    impl crate::plugins::PluginFactory for R6Factory {
        fn revision(&self) -> u64 {
            self.0.revision.load(Ordering::Relaxed)
        }
        fn effect(
            &self,
            _: &windfall_project::PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn crate::plugins::HostedEffect>, String> {
            let owner = self.0.creates.fetch_add(1, Ordering::Relaxed);
            self.0.prepared_revision[owner].store(self.revision(), Ordering::Relaxed);
            Ok(Box::new(R6Delay {
                ring: [[0.0; 2]; 32],
                write: 0,
                owner,
                stats: self.0.clone(),
            }))
        }
        fn instrument(
            &self,
            _: &windfall_project::PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn crate::plugins::HostedInstrument>, String> {
            Err("effect fixture".into())
        }
    }
    struct R6Delay {
        ring: [[f32; 2]; 32],
        write: usize,
        owner: usize,
        stats: Arc<R6Native>,
    }
    impl Drop for R6Delay {
        fn drop(&mut self) {
            self.stats.drops[self.owner].fetch_add(1, Ordering::Relaxed);
        }
    }
    impl crate::plugins::HostedEffect for R6Delay {
        fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
            self.stats.processes[self.owner].fetch_add(1, Ordering::Relaxed);
            for (left, right) in left.iter_mut().zip(right) {
                let old = self.ring[self.write];
                self.ring[self.write] = [*left, *right];
                self.write = (self.write + 1) % 32;
                [*left, *right] = old;
            }
        }
        fn set_param(&mut self, _: u32, _: f32) {}
        fn set_tempo(&mut self, _: f32) {}
        fn latency(&self) -> usize {
            32
        }
        fn tail(&self) -> usize {
            32
        }
    }
    fn r6_fixture(cancel: bool) -> (Project, SamplePool, Arc<R6Native>) {
        use windfall_project::{EffectId, EffectParams, EffectSlot, PluginBinding, PluginTarget};
        let (mut project, mut pool) = clicks(&[0]);
        let tone: Vec<f32> = (0..48_000)
            .map(|n| (std::f64::consts::TAU * n as f64 * 173.0 / 48_000.0).sin() as f32 * 0.5)
            .collect();
        pool.insert(
            SampleId(900),
            AudioBuffer::from_interleaved(48_000, 1, tone.clone()),
        );
        let mut first = project.mixer.tracks[0].clone();
        first.id = TrackId(11);
        first.output = Some(TrackId::MASTER);
        first.effects = vec![
            EffectSlot {
                impulse: None,
                id: EffectId(20),
                enabled: true,
                mix: 1.0,
                params: windfall_project::EffectKind::Balance.default_params(),
            },
            EffectSlot {
                impulse: None,
                id: EffectId(21),
                enabled: true,
                mix: 1.0,
                params: EffectParams::StereoMatrix(windfall_dsp::StereoMatrixParams {
                    left_delay_ms: 1.0,
                    right_delay_ms: 1.0,
                    ..Default::default()
                }),
            },
        ];
        project.mixer.tracks.push(first);
        project.channels[0].mixer_track = TrackId(11);
        project.channels[0].volume = 0.5;
        if cancel {
            let mut second = project.mixer.tracks[0].clone();
            second.id = TrackId(12);
            second.output = Some(TrackId::MASTER);
            project.mixer.tracks.push(second);
            pool.insert(
                SampleId(901),
                AudioBuffer::from_interleaved(
                    48_000,
                    1,
                    tone.iter().map(|sample| -*sample).collect(),
                ),
            );
            let mut channel = project.channels[0].clone();
            channel.id = ChannelId(902);
            channel.mixer_track = TrackId(12);
            channel.source = ChannelSource::Sampler(SamplerSettings {
                sample: Some(SampleId(901)),
                ..Default::default()
            });
            project.channels.push(channel);
            let mut lane = project.patterns[0].lanes[0].clone();
            lane.channel = ChannelId(902);
            lane.notes[0].id = NoteId(1100);
            project.patterns[0].lanes.push(lane);
        }
        project.plugins.push(PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            wrapper: Default::default(),
            target: PluginTarget::Effect {
                effect: EffectId(20),
            },
            format: "clap".into(),
            path: "r6-detached-delay.clap".into(),
            id: "r6-delay".into(),
            name: "Prepared delay".into(),
            state: vec![],
            parameters: vec![],
        });
        let stats = Arc::new(R6Native::default());
        pool.set_plugin_factory(Arc::new(R6Factory(stats.clone())));
        (project, pool, stats)
    }
    fn r6_collect(controller: &Controller) {
        controller.frame();
        let mut retirement = ProjectRetirement::default();
        controller.take_retired(&mut retirement);
    }

    fn r6_publish_ready(controller: &Controller, prepared: PreparedProject) {
        let mut ready = controller
            .preparation_snapshot()
            .prepare_compiled(prepared, ProjectPublicationIntent::Edit)
            .unwrap();
        let mut retirement = controller.publication(&mut ready).unwrap().install();
        controller.take_retired(&mut retirement);
    }

    fn r6_audio(processor: &mut Processor, frames: usize, block: usize) -> Vec<f32> {
        let mut audio = vec![0.0; frames * 2];
        for out in audio.chunks_mut(block * 2) {
            assert_eq!(
                crate::test_alloc::allocator_calls(|| processor.process(out)),
                0
            );
        }
        audio
    }
    fn r6_detached_precompiled_adoption(cancel: bool, revise_before_attach: bool) {
        let (mut project, pool, stats) = r6_fixture(cancel);
        let controller = Controller::new();
        let prepared = Controller::prepare_project(&project, &pool)
            .expect("the detached fixture prepares successfully");
        stats.revision.store(1, Ordering::Relaxed);
        controller.set_prepared_project(&project, prepared);
        if revise_before_attach {
            stats.revision.store(2, Ordering::Relaxed);
        }
        let mut processor = controller.attach(48_000);
        controller.play();
        let prime = r6_audio(&mut processor, 4092, 137);
        if cancel {
            assert!(prime.iter().all(|sample| sample.abs() < 1e-6));
        }
        assert_eq!(controller.latency_frames(), 80);
        assert_eq!(stats.creates.load(Ordering::Relaxed), 1);
        assert_eq!(
            stats.prepared_revision[0].load(Ordering::Relaxed),
            if revise_before_attach { 2 } else { 1 }
        );
        let mut audio = prime[prime.len() - 2..].to_vec();
        let before = stats.processes[0].load(Ordering::Relaxed);
        controller.set_project(&project, &pool);
        let adopted = r6_audio(&mut processor, 1000, 1);
        if cancel {
            let residual = adopted.iter().copied().map(f32::abs).fold(0.0, f32::max);
            println!("detached prepared adoption residual {residual}");
            assert!(
                residual < 1e-6,
                "detached prepared adoption residual {residual}"
            );
        }
        audio.extend(adopted);
        if !cancel {
            let step = audio
                .as_chunks::<2>()
                .0
                .windows(2)
                .map(|frames| (frames[1][0] - frames[0][0]).abs())
                .fold(0.0, f32::max);
            println!("detached prepared adoption solo step {step}");
            assert!(step < 0.04, "detached prepared adoption solo step {step}");
        }
        assert!(
            stats.processes[0].load(Ordering::Relaxed) > before,
            "running native owner stopped processing"
        );
        r6_collect(&controller);
        assert_eq!(stats.creates.load(Ordering::Relaxed), 1);
        assert_eq!(stats.drops[0].load(Ordering::Relaxed), 0);
        for block in [7, 137, 1] {
            let before = stats.processes[0].load(Ordering::Relaxed);
            if block == 137 {
                r6_publish_ready(
                    &controller,
                    Controller::prepare_project(&project, &pool)
                        .expect("the detached fixture prepares successfully"),
                );
            } else {
                controller.set_project(&project, &pool);
            }
            let continuation = r6_audio(&mut processor, 137, block);
            if cancel {
                assert!(continuation.iter().all(|sample| sample.abs() < 1e-6));
            }
            assert!(stats.processes[0].load(Ordering::Relaxed) > before);
            assert_eq!(controller.latency_frames(), 80);
            r6_collect(&controller);
            assert_eq!(stats.creates.load(Ordering::Relaxed), 1);
            assert_eq!(stats.drops[0].load(Ordering::Relaxed), 0);
        }
        project.mixer.tracks[1].effects.remove(0);
        project.plugins.clear();
        controller.set_project(&project, &pool);
        let removal = r6_audio(&mut processor, 1000, 29);
        if cancel {
            assert!(removal.iter().all(|sample| sample.abs() < 1e-6));
        }
        controller.set_project(&project, &pool);
        r6_audio(&mut processor, 137, 137);
        r6_collect(&controller);
        assert_eq!(stats.drops[0].load(Ordering::Relaxed), 1);
        assert_eq!(controller.latency_frames(), 48);
    }
    #[test]
    fn utility_r6_detached_precompiled_adoption_keeps_native_cancellation() {
        r6_detached_precompiled_adoption(true, false);
    }
    #[test]
    fn utility_r6_detached_precompiled_adoption_keeps_native_continuity_and_owner() {
        r6_detached_precompiled_adoption(false, false);
    }

    #[test]
    fn utility_r6_revision_after_detached_install_uses_actual_preparation() {
        r6_detached_precompiled_adoption(true, true);
    }

    #[test]
    fn utility_r6_attachment_retry_and_reopen_freeze_native_identity() {
        let (project, pool, stats) = r6_fixture(true);
        let controller = Controller::new();
        controller.set_project(&project, &pool);
        stats.revision.store(1, Ordering::Relaxed);
        let abandoned = controller.attach(48_000);
        assert_eq!(stats.prepared_revision[0].load(Ordering::Relaxed), 1);
        drop(abandoned);
        assert_eq!(stats.processes[0].load(Ordering::Relaxed), 0);
        assert_eq!(stats.drops[0].load(Ordering::Relaxed), 1);

        stats.revision.store(2, Ordering::Relaxed);
        let mut processor = controller.attach(48_000);
        controller.play();
        let prime = r6_audio(&mut processor, 4092, 29);
        assert!(prime.iter().all(|sample| sample.abs() < 1e-6));
        assert_eq!(stats.prepared_revision[1].load(Ordering::Relaxed), 2);
        let before = stats.processes[1].load(Ordering::Relaxed);
        controller.set_project(&project, &pool);
        let adopted = r6_audio(&mut processor, 1000, 7);
        assert!(adopted.iter().all(|sample| sample.abs() < 1e-6));
        assert!(stats.processes[1].load(Ordering::Relaxed) > before);
        r6_collect(&controller);
        assert_eq!(controller.latency_frames(), 80);
        assert_eq!(stats.creates.load(Ordering::Relaxed), 2);
        assert_eq!(stats.drops[1].load(Ordering::Relaxed), 0);

        controller.suspend();
        drop(processor);
        assert_eq!(stats.drops[1].load(Ordering::Relaxed), 1);
        stats.revision.store(3, Ordering::Relaxed);
        let mut processor = controller.attach(48_000);
        let resumed = r6_audio(&mut processor, 4092, 137);
        assert!(resumed.iter().all(|sample| sample.abs() < 1e-6));
        assert_eq!(stats.prepared_revision[2].load(Ordering::Relaxed), 3);
        let before = stats.processes[2].load(Ordering::Relaxed);
        r6_publish_ready(
            &controller,
            Controller::prepare_project(&project, &pool)
                .expect("the detached fixture prepares successfully"),
        );
        let adopted = r6_audio(&mut processor, 1000, 1);
        assert!(adopted.iter().all(|sample| sample.abs() < 1e-6));
        assert!(stats.processes[2].load(Ordering::Relaxed) > before);
        r6_collect(&controller);
        assert_eq!(controller.latency_frames(), 80);
        assert_eq!(stats.creates.load(Ordering::Relaxed), 3);
        assert_eq!(stats.drops[2].load(Ordering::Relaxed), 0);
        controller.detach();
        drop(processor);
        assert_eq!(stats.drops[2].load(Ordering::Relaxed), 1);
    }

    /// A controller playing from tick 240, with its playhead at tick 604 as
    /// the stream is about to go away.
    fn playing_at_604() -> (Processor, Controller) {
        let (mut processor, controller) = Processor::new(48_000);
        let (project, pool) = clicks(&[2, 3, 4]);
        controller.set_project(&project, &pool);
        controller.seek(240.0);
        controller.play();
        // 9100 frames at 25 frames a tick are 364 ticks.
        assert_eq!(run(&mut processor, 9_100), [6_000]);
        assert_eq!(controller.frame().tick, 604.0);
        (processor, controller)
    }

    #[test]
    fn new_settings_keep_playing_and_keep_the_place_stop_returns_to() {
        let (processor, controller) = playing_at_604();
        controller.suspend();
        drop(processor);
        assert!(controller.transport().playing);
        assert_eq!(controller.frame().tick, 604.0);

        // The new stream runs at another rate: 22.96875 frames a tick.
        let mut processor = controller.attach(44_100);
        assert!(controller.transport().playing);
        // Playback picks up at tick 604. Step 3, at tick 720, comes 116
        // ticks later, and step 2, which is behind, does not play again.
        assert_eq!(run(&mut processor, 4_410), [2_665]);
        let frame = controller.frame();
        assert!(frame.playing);
        assert!((frame.tick - (604.0 + 192.0)).abs() < 1e-9);

        controller.stop();
        run(&mut processor, 64);
        assert!(!controller.transport().playing);
        assert_eq!(controller.frame().tick, 240.0);
        controller.play();
        run(&mut processor, 64);
        controller.stop();
        run(&mut processor, 64);
        assert_eq!(controller.frame().tick, 240.0);
    }

    #[test]
    fn a_stream_built_twice_before_it_runs_still_carries_on() {
        let (processor, controller) = playing_at_604();
        controller.suspend();
        drop(processor);
        // The first kind of stream could not be built, so its processor is
        // thrown away unused and another is made.
        drop(controller.attach(96_000));
        let mut processor = controller.attach(48_000);
        assert_eq!(run(&mut processor, 4_800), [116 * 25]);
        assert!(controller.frame().playing);
        controller.stop();
        run(&mut processor, 64);
        assert_eq!(controller.frame().tick, 240.0);
    }

    #[test]
    fn settings_that_cannot_be_opened_stop_playback_at_its_start() {
        let (processor, controller) = playing_at_604();
        controller.suspend();
        drop(processor);
        drop(controller.attach(48_000));
        controller.detach();
        assert!(!controller.transport().playing);
        let frame = controller.frame();
        assert!(!frame.playing);
        assert_eq!(frame.tick, 240.0);

        // A stream that opens later starts out stopped, at the same place.
        let mut processor = controller.attach(48_000);
        assert!(run(&mut processor, 4_800).is_empty());
        assert!(!controller.transport().playing);
        assert_eq!(controller.frame().tick, 240.0);
        controller.play();
        assert_eq!(run(&mut processor, 6_100), [6_000]);
    }

    #[test]
    fn a_lost_stream_stops_playback_at_its_start() {
        let (processor, controller) = playing_at_604();
        controller.detach();
        drop(processor);
        assert!(!controller.transport().playing);
        assert_eq!(controller.frame().tick, 240.0);
        assert_eq!(controller.frame().voices, 0);

        let mut processor = controller.attach(48_000);
        run(&mut processor, 64);
        assert!(!controller.frame().playing);
        assert_eq!(controller.frame().tick, 240.0);
    }

    #[test]
    fn stopping_between_two_streams_is_not_undone_by_the_second() {
        let (processor, controller) = playing_at_604();
        controller.suspend();
        drop(processor);
        controller.stop();
        assert!(!controller.transport().playing);
        assert_eq!(controller.frame().tick, 240.0);

        let mut processor = controller.attach(48_000);
        assert!(run(&mut processor, 4_800).is_empty());
        assert!(!controller.frame().playing);
        assert_eq!(controller.frame().tick, 240.0);
    }

    #[test]
    fn a_stopped_transport_stays_where_it_is_across_streams() {
        let (mut processor, controller) = Processor::new(48_000);
        let (project, pool) = clicks(&[0]);
        controller.set_project(&project, &pool);
        controller.seek(960.0);
        run(&mut processor, 64);
        controller.suspend();
        drop(processor);
        // A seek made while no stream is open is kept as well.
        controller.seek(480.0);

        let mut processor = controller.attach(44_100);
        run(&mut processor, 64);
        assert!(!controller.transport().playing);
        assert_eq!(controller.frame().tick, 480.0);
        controller.play();
        run(&mut processor, 64);
        controller.stop();
        run(&mut processor, 64);
        assert_eq!(controller.frame().tick, 480.0);
    }

    #[test]
    fn a_new_stream_does_not_report_the_automation_of_the_last_one() {
        use windfall_project::{
            Automation, AutomationId, AutomationPoint, AutomationTarget, Clip, ClipContent, ClipId,
            PlaylistTrack, PlaylistTrackId,
        };

        // A song of one bar in which a curve holds the master's fader.
        let (mut project, pool) = clicks(&[0]);
        let pattern = project.patterns[0].id;
        project.playlist.tracks.push(PlaylistTrack {
            id: PlaylistTrackId(950),
            name: String::new(),
            muted: false,
            solo: false,
            color: 0,
            height: 0,
        });
        project.automations.push(Automation {
            id: AutomationId(960),
            name: String::new(),
            color: 0,
            target: AutomationTarget::TrackVolume {
                track: TrackId::MASTER,
            },
            points: vec![AutomationPoint {
                tick: 0,
                value: 0.5,
                curve: 0.0,
                hold: false,
            }],
        });
        let contents = [
            ClipContent::Pattern { pattern },
            ClipContent::Automation {
                automation: AutomationId(960),
            },
        ];
        for (index, content) in contents.into_iter().enumerate() {
            project.playlist.clips.push(Clip {
                id: ClipId(970 + index as u32),
                track: PlaylistTrackId(950),
                start: 0,
                length: 3_840,
                offset: 0,
                muted: false,
                content,
            });
        }
        let (mut processor, controller) = Processor::new(48_000);
        controller.set_project(&project, &pool);
        controller.set_transport(TransportPatch {
            mode: Some(PlayMode::Song),
            ..TransportPatch::default()
        });
        controller.play();
        run(&mut processor, 4_800);
        let automated = controller.frame().automated;
        assert_eq!(automated.len(), 1);
        assert_eq!(automated[0].automation, AutomationId(960));
        assert_eq!(automated[0].value, 0.5);

        // The stream is lost. With none open, nothing is reported, and the
        // one that opens later starts out stopped, with nothing automated.
        controller.detach();
        drop(processor);
        assert!(controller.frame().automated.is_empty());
        let mut processor = controller.attach(48_000);
        assert!(controller.frame().automated.is_empty());
        run(&mut processor, 480);
        assert!(controller.frame().automated.is_empty());
        controller.play();
        run(&mut processor, 480);
        assert_eq!(controller.frame().automated.len(), 1);
    }

    #[test]
    fn a_new_stream_starts_on_the_project_and_the_output_gain() {
        let (processor, controller) = Processor::new(48_000);
        let (mut project, pool) = clicks(&[0]);
        project.mixer.tracks[0].volume = 0.5;
        controller.set_project(&project, &pool);
        controller.set_output_gain(0.5);
        controller.suspend();
        drop(processor);

        // Nothing has sounded on the new stream, so neither the master
        // fader nor the output gain has anything to glide in from.
        let mut processor = controller.attach(48_000);
        controller.play();
        let mut out = [0.0; 64];
        processor.process(&mut out);
        assert_eq!(out[..2], [0.25, 0.25]);
    }

    #[test]
    fn timeline_invalid_meter_snapshots_are_refused_before_publication_and_native_ownership() {
        use crate::plugins::{HostedEffect, HostedInstrument, PluginFactory};
        use crate::sampler_processing::SamplerPreparationError;
        use std::sync::atomic::AtomicUsize;
        use windfall_project::timeline::MeterMapError;
        use windfall_project::{
            EffectId, EffectParams, EffectSlot, MeterChange, MeterChangeId, PluginBinding,
            PluginTarget, TickRange, TimeSignature,
        };

        #[derive(Debug, Default)]
        struct Counts {
            snapshots: AtomicUsize,
            preparations: AtomicUsize,
            process: AtomicUsize,
            drops: AtomicUsize,
        }
        struct Unit(Arc<Counts>);
        impl Drop for Unit {
            fn drop(&mut self) {
                self.0.drops.fetch_add(1, Ordering::Relaxed);
            }
        }
        impl HostedEffect for Unit {
            fn process(&mut self, _: &mut [f32], _: &mut [f32]) {
                self.0.process.fetch_add(1, Ordering::Relaxed);
            }
            fn set_param(&mut self, _: u32, _: f32) {}
            fn set_tempo(&mut self, _: f32) {}
            fn latency(&self) -> usize {
                0
            }
            fn tail(&self) -> usize {
                0
            }
        }
        #[derive(Debug)]
        struct Factory(Arc<Counts>);
        impl PluginFactory for Factory {
            fn provider_identity(&self) -> u64 {
                self.0.snapshots.fetch_add(1, Ordering::Relaxed);
                42
            }
            fn effect(
                &self,
                _: &PluginBinding,
                _: u32,
                _: usize,
            ) -> Result<Box<dyn HostedEffect>, String> {
                self.0.preparations.fetch_add(1, Ordering::Relaxed);
                Ok(Box::new(Unit(self.0.clone())))
            }
            fn instrument(
                &self,
                _: &PluginBinding,
                _: u32,
                _: usize,
            ) -> Result<Box<dyn HostedInstrument>, String> {
                panic!("this project has no hosted instruments")
            }
        }
        let counts = Arc::new(Counts::default());
        let (mut project, mut pool) = clicks(&[0]);
        pool.insert(
            SampleId(900),
            AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 24_000]),
        );
        pool.set_plugin_factory(Arc::new(Factory(counts.clone())));
        project.mixer.tracks[0].effects.push(EffectSlot {
            impulse: None,
            id: EffectId(950),
            enabled: true,
            mix: 1.0,
            params: EffectParams::Limiter(Default::default()),
        });
        project.plugins.push(PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            wrapper: Default::default(),
            target: PluginTarget::Effect {
                effect: EffectId(950),
            },
            format: "clap".into(),
            path: "prepared-owner-probe".into(),
            id: "0".into(),
            name: "probe".into(),
            state: Vec::new(),
            parameters: Vec::new(),
        });
        let (mut processor, controller) = Processor::new(48_000);
        controller.set_project(&project, &pool);
        controller
            .set_timeline_region(Some(TickRange {
                start: 17,
                end: 839,
            }))
            .unwrap();
        controller.play();
        processor.process(&mut [0.0; 2]);
        let before_plan = controller.lock().plan.clone();
        let before_transport = controller.transport();
        let before_tick = controller.frame().tick;
        let before_region = controller.timeline_region();
        let before_counts = (
            counts.snapshots.load(Ordering::Relaxed),
            counts.preparations.load(Ordering::Relaxed),
            counts.process.load(Ordering::Relaxed),
        );
        assert!(before_counts.0 > 0 && before_counts.1 > 0 && before_counts.2 > 0);

        let mut invalid = project.clone();
        invalid.patterns[0].id = PatternId(123_456);
        invalid.settings.tempo_bpm = 300.0;
        let error = SamplerPreparationError::Unsupported(
            "Invalid song meter map; project preparation refused.",
        );
        for cause in [
            MeterMapError::Numerator(0),
            MeterMapError::Denominator(0),
            MeterMapError::UnorderedOrOutOfBounds,
        ] {
            invalid.settings.time_signature = TimeSignature {
                numerator: 4,
                denominator: 4,
            };
            invalid.playlist.timeline.meters.clear();
            match cause {
                MeterMapError::Numerator(_) => invalid.settings.time_signature.numerator = 0,
                MeterMapError::Denominator(_) => invalid.settings.time_signature.denominator = 0,
                _ => {
                    invalid.playlist.timeline.meters = vec![
                        MeterChange {
                            id: MeterChangeId(100_001),
                            tick: 4001,
                            signature: TimeSignature {
                                numerator: 7,
                                denominator: 8,
                            },
                        },
                        MeterChange {
                            id: MeterChangeId(100_002),
                            tick: 4001,
                            signature: TimeSignature {
                                numerator: 3,
                                denominator: 4,
                            },
                        },
                    ]
                }
            }
            assert!(
                matches!(Controller::try_prepare_project(&invalid, &pool), Err(ref refused) if *refused == error)
            );
            let prepared = Controller::compile_prepared_project(&invalid, &pool);
            assert!(matches!(prepared.plan.meters, Err(actual) if actual == cause));
            controller.set_prepared_project(&invalid, prepared);
            assert_eq!(controller.sampler_preparation_error(), Some(error.clone()));
            controller.set_plan(compile(&invalid, &pool));
            controller.set_project(&invalid, &pool);
            assert_eq!(controller.sampler_preparation_error(), Some(error.clone()));
            assert!(Arc::ptr_eq(&before_plan, &controller.lock().plan));
            assert_eq!(controller.transport(), before_transport);
            assert_eq!(controller.frame().tick, before_tick);
            assert_eq!(controller.timeline_region(), before_region);
            assert_eq!(
                (
                    counts.snapshots.load(Ordering::Relaxed),
                    counts.preparations.load(Ordering::Relaxed),
                    counts.process.load(Ordering::Relaxed)
                ),
                before_counts
            );
            assert_eq!(counts.drops.load(Ordering::Relaxed), 0);
        }
        let mut out = [0.0; 2];
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        assert_eq!(out, [0.25; 2]);
        assert!(controller.transport().playing);
        assert_eq!(counts.drops.load(Ordering::Relaxed), 0);
        project.settings.time_signature = TimeSignature {
            numerator: 3,
            denominator: 4,
        };
        controller.set_project(&project, &pool);
        assert_eq!(controller.sampler_preparation_error(), None);
        assert!(!Arc::ptr_eq(&before_plan, &controller.lock().plan));
        assert_eq!(controller.transport(), before_transport);
        assert_eq!(controller.timeline_region(), before_region);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        assert_eq!(out, [0.25; 2]);
        assert_eq!(counts.preparations.load(Ordering::Relaxed), before_counts.1);
        assert_eq!(counts.drops.load(Ordering::Relaxed), 0);
    }
}
