//! Worker-owned preparation and borrowed, infallible publication admission.
//!
//! A lease owns no native units. Its caller keeps both the ready token and an
//! outer retirement slot alive until all document/recording guards are gone.

#[cfg(test)]
#[path = "project_preparation/echo_history_tests.rs"]
mod echo_history_tests;

use super::*;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FactoryStamp {
    provider: u64,
    revision: u64,
}

impl FactoryStamp {
    pub fn capture(factory: &Option<Arc<dyn crate::plugins::PluginFactory>>) -> Option<Self> {
        factory.as_ref().map(|factory| Self {
            provider: factory.provider_identity(),
            revision: factory.revision(),
        })
    }
    pub fn identity(self) -> u64 {
        self.provider ^ self.revision.rotate_left(17)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamPhase {
    Detached,
    Starting,
    Running,
    Closing,
}

#[derive(Debug, Clone, Copy)]
pub enum ProjectPublicationIntent {
    Edit,
    Replace { transport: TransportPatch },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectPreparationError {
    Rack(String),
    Sampler(crate::sampler_processing::SamplerPreparationError),
    Native {
        target: windfall_project::PluginTarget,
        reason: String,
    },
    Unsupported(&'static str),
    IdentityChanged,
    StreamTransitioning,
    UnresolvedProgress,
    SizeOverflow,
    MemoryLimit {
        required: usize,
        limit: usize,
    },
    Publication(PublicationRefusal),
    AttachmentRetries {
        attempts: u8,
        reason: Box<ProjectPreparationError>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationRefusal {
    WrongController,
    StalePlan,
    StaleStream,
    StaleFactory,
    UnresolvedProgress,
    QueueFull,
    GenerationExhausted,
}

impl fmt::Display for ProjectPreparationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rack(reason) => write!(formatter, "Rack preparation failed: {reason}"),
            Self::Sampler(error) => write!(formatter, "{error}"),
            Self::Native { target, reason } => write!(
                formatter,
                "Native plugin preparation failed for {target:?}: {reason}"
            ),
            Self::Unsupported(reason) => formatter.write_str(reason),
            Self::MemoryLimit { required, limit } => write!(
                formatter,
                "This project needs {} MB of prepared audio memory and the limit is {} MB. Nothing was changed.",
                required.div_ceil(1 << 20),
                limit >> 20
            ),
            _ => write!(formatter, "Project preparation refused: {self:?}"),
        }
    }
}
impl std::error::Error for ProjectPreparationError {}
impl fmt::Display for PublicationRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}
impl std::error::Error for PublicationRefusal {}
impl From<crate::plugins::NativePreparationError> for ProjectPreparationError {
    fn from(error: crate::plugins::NativePreparationError) -> Self {
        Self::Native {
            target: error.target,
            reason: error.reason,
        }
    }
}

/// Cheap immutable engine metadata. Capturing never drains a queue or prepares
/// a processor, and no unique native owner is temporarily held under its guard.
#[derive(Clone)]
pub struct PreparationSnapshot {
    audio_threads: usize,
    owner: Arc<Inner>,
    plan: Arc<Plan>,
    ledger: Option<Arc<Ledger>>,
    plan_generation: u64,
    stream_generation: u64,
    phase: StreamPhase,
    factory: Option<FactoryStamp>,
}

/// Whole-state ready units, still unselected. Refusal borrows this token so its
/// last references remain with the caller, outside all admission guards.
pub struct PreparedPublication {
    snapshot: Option<PreparationSnapshot>,
    plan: Option<Arc<Plan>>,
    ledger: Option<Arc<Ledger>>,
    pool: SamplePool,
    intent: ProjectPublicationIntent,
    messages: [Option<Message>; 4],
    count: usize,
    transport: Option<TransportState>,
    sequence: u32,
    retirement: Option<ProjectRetirement>,
}

/// Must be allocated before entering document/recording guards, and dropped
/// after leaving them. It holds a bounded current-ring drain, entire displaced
/// endpoints/backlog, and the captured/displaced immutable metadata handles.
pub struct ProjectRetirement {
    garbage: Vec<Garbage>,
    old_plan: Option<Arc<Plan>>,
    old_ledger: Option<Arc<Ledger>>,
    snapshot: Option<PreparationSnapshot>,
    old_link: Option<Link>,
    backlog: Option<VecDeque<Message>>,
    old_error: Option<ProjectPreparationError>,
    old_sampler_error: Option<crate::sampler_processing::SamplerPreparationError>,
}

impl Default for ProjectRetirement {
    fn default() -> Self {
        Self {
            garbage: Vec::with_capacity(GARBAGE_CAPACITY),
            old_plan: None,
            old_ledger: None,
            snapshot: None,
            old_link: None,
            backlog: None,
            old_error: None,
            old_sampler_error: None,
        }
    }
}

impl ProjectRetirement {
    /// Release retired owners only after all caller/controller guards are gone.
    /// Retains the bounded ring-drain capacity for the next control-side pass.
    pub fn clear(&mut self) {
        self.garbage.clear();
        self.old_link = None;
        self.backlog = None;
        self.old_plan = None;
        self.old_ledger = None;
        self.snapshot = None;
        self.old_error = None;
        self.old_sampler_error = None;
    }
}

pub struct ProjectPublicationLease<'a> {
    controller: &'a Controller,
    state: Option<MutexGuard<'a, State>>,
    prepared: &'a mut PreparedPublication,
}

impl PreparationSnapshot {
    /// Retry may recapture a newer plan/source ledger only within the same
    /// controller, stream epoch, rate and captured provider/revision universe.
    /// This compares immutable metadata; it neither locks nor retires owners.
    pub fn same_environment(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.owner, &other.owner)
            && self.stream_generation == other.stream_generation
            && self.phase == other.phase
            && self.factory == other.factory
            && self.ledger.as_ref().map(|ledger| ledger.sample_rate)
                == other.ledger.as_ref().map(|ledger| ledger.sample_rate)
    }

    pub(super) fn is_attached(&self) -> bool {
        self.ledger.is_some()
    }
    pub fn prepare(
        self,
        project: &Project,
        pool: &SamplePool,
        intent: ProjectPublicationIntent,
    ) -> Result<PreparedPublication, ProjectPreparationError> {
        let controller = Controller {
            inner: self.owner.clone(),
        };
        for channel in &project.channels {
            if let Some(settings) = &channel.sample_playback {
                crate::sample_playback::prepare(settings, pool)
                    .map_err(ProjectPreparationError::Unsupported)?;
            }
        }
        let result = Controller::try_prepare_project(project, pool)
            .map_err(ProjectPreparationError::Sampler)
            .and_then(|prepared| self.prepare_compiled(prepared, intent));
        if let Err(error) = &result {
            if let ProjectPreparationError::Sampler(error) = error {
                *controller
                    .inner
                    .sampler_error
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
            }
            controller.latch_preparation_error(error.clone());
        }
        result
    }

    pub(crate) fn prepare_compiled(
        self,
        prepared: PreparedProject,
        intent: ProjectPublicationIntent,
    ) -> Result<PreparedPublication, ProjectPreparationError> {
        if matches!(self.phase, StreamPhase::Starting | StreamPhase::Closing) {
            return Err(ProjectPreparationError::StreamTransitioning);
        }
        if prepared.scratch_asset_refused() {
            return Err(ProjectPreparationError::Unsupported(
                "The scratch source exceeds the effect asset memory budget; release unused project history or sources before retrying.",
            ));
        }
        let mut plan = prepared.plan;
        plan.audio_threads = self.audio_threads;
        // Also retain the budget while detached: an old processor/retirement
        // carrier can still own histories when its replacement attaches.
        plan.echo_history = self.plan.echo_history.clone();
        if plan.meters.is_err() {
            return Err(ProjectPreparationError::Unsupported(
                "Invalid song meter map; project preparation refused.",
            ));
        }
        // Freeze intended provider metadata before any constructors. Old actual
        // native identities are read exclusively from the captured Ledger.
        plan.snapshot_native_owners();
        let factory = plan.factory_stamp;
        let mut messages = std::array::from_fn(|_| None);
        let ledger = if let Some(held) = self.ledger.as_deref() {
            plan.reserve_departures(&self.plan, held)?;
            let (state, ledger) = PlanState::try_build(&plan, held.sample_rate, Some(held))?;
            let plan = Arc::new(plan);
            messages[0] = Some(Message::SetPlan {
                plan: plan.clone(),
                state: Box::new(state),
            });
            (plan, Some(Arc::new(ledger)))
        } else {
            (Arc::new(plan), None)
        };
        if factory != FactoryStamp::capture(&ledger.0.plugin_factory)
            || self.factory != FactoryStamp::capture(&self.plan.plugin_factory)
        {
            return Err(ProjectPreparationError::IdentityChanged);
        }
        Ok(PreparedPublication {
            snapshot: Some(self),
            plan: Some(ledger.0),
            ledger: ledger.1,
            pool: prepared.sampler_pool,
            intent,
            messages,
            count: 0,
            transport: None,
            sequence: 0,
            retirement: Some(ProjectRetirement::default()),
        })
    }
}

impl PreparedPublication {
    pub fn sampler_pool(&self) -> &SamplePool {
        &self.pool
    }
}

impl Controller {
    pub fn preparation_snapshot(&self) -> PreparationSnapshot {
        let state = self.lock();
        PreparationSnapshot {
            audio_threads: state.audio_threads,
            owner: self.inner.clone(),
            plan: state.plan.clone(),
            ledger: state.hosted.clone(),
            plan_generation: state.plan_generation,
            stream_generation: state.stream_generation,
            phase: state.stream_phase,
            factory: FactoryStamp::capture(&state.plan.plugin_factory),
        }
    }

    pub fn publication<'a>(
        &'a self,
        prepared: &'a mut PreparedPublication,
    ) -> Result<ProjectPublicationLease<'a>, PublicationRefusal> {
        let snapshot = prepared
            .snapshot
            .as_ref()
            .ok_or(PublicationRefusal::StalePlan)?;
        if !Arc::ptr_eq(&snapshot.owner, &self.inner) {
            return Err(PublicationRefusal::WrongController);
        }
        let state = self.lock();
        if state.stream_generation != snapshot.stream_generation
            || state.stream_phase != snapshot.phase
            || matches!(
                state.stream_phase,
                StreamPhase::Starting | StreamPhase::Closing
            )
            || state
                .link
                .as_ref()
                .is_some_and(|link| link.messages.is_abandoned())
        {
            return Err(PublicationRefusal::StaleStream);
        }
        let plan = prepared
            .plan
            .as_ref()
            .ok_or(PublicationRefusal::StalePlan)?;
        if snapshot.factory != FactoryStamp::capture(&state.plan.plugin_factory)
            || plan.factory_stamp != FactoryStamp::capture(&plan.plugin_factory)
        {
            return Err(PublicationRefusal::StaleFactory);
        }
        if state.plan_generation != snapshot.plan_generation
            || !Arc::ptr_eq(&state.plan, &snapshot.plan)
        {
            return Err(PublicationRefusal::StalePlan);
        }
        if state.plan_generation == u64::MAX {
            return Err(PublicationRefusal::GenerationExhausted);
        }
        let mut transport = state.transport;
        let sequence = match prepared.intent {
            ProjectPublicationIntent::Edit => {
                if plan.pattern_ids.get(transport.pattern.0).is_none()
                    && let Some(first) = plan.patterns.first()
                {
                    transport.pattern = first.id;
                }
                state.sequence
            }
            ProjectPublicationIntent::Replace { transport: patch } => {
                transport.playing = false;
                transport.mode = patch.mode.unwrap_or(transport.mode);
                transport.pattern = patch.pattern.unwrap_or(transport.pattern);
                if plan.pattern_ids.get(transport.pattern.0).is_none()
                    && let Some(first) = plan.patterns.first()
                {
                    transport.pattern = first.id;
                }
                transport.loop_song = patch.loop_song.unwrap_or(transport.loop_song);
                state.sequence.wrapping_add(1)
            }
        };
        let count = if state.link.is_none() {
            0
        } else {
            match prepared.intent {
                ProjectPublicationIntent::Edit => {
                    1 + usize::from(transport.pattern != state.transport.pattern)
                }
                ProjectPublicationIntent::Replace { .. } => 4,
            }
        };
        if !state.backlog.is_empty()
            || state
                .link
                .as_ref()
                .is_some_and(|link| link.messages.slots() < count)
        {
            return Err(PublicationRefusal::QueueFull);
        }
        if prepared.retirement.is_none() || prepared.transport.is_some() {
            return Err(PublicationRefusal::StalePlan);
        }
        if count > 0 {
            let set_plan = prepared.messages[0]
                .take()
                .ok_or(PublicationRefusal::StalePlan)?;
            match prepared.intent {
                ProjectPublicationIntent::Edit => {
                    prepared.messages[0] = Some(set_plan);
                    if count == 2 {
                        prepared.messages[1] = Some(transport_message(transport));
                    }
                }
                ProjectPublicationIntent::Replace { .. } => {
                    prepared.messages[0] = Some(Message::Stop { sequence });
                    prepared.messages[1] = Some(set_plan);
                    prepared.messages[2] = Some(transport_message(transport));
                    prepared.messages[3] = Some(Message::Seek(0.0));
                }
            }
        }
        prepared.transport = Some(transport);
        prepared.sequence = sequence;
        prepared.count = count;
        Ok(ProjectPublicationLease {
            controller: self,
            state: Some(state),
            prepared,
        })
    }

    /// Capture only into caller-owned preallocated storage. No destruction and
    /// no producer-following drain loop can run under this guard.
    pub fn take_retired(&self, retirement: &mut ProjectRetirement) {
        let mut state = self.lock();
        if let Some(link) = &mut state.link {
            let available = retirement.garbage.capacity() - retirement.garbage.len();
            for _ in 0..available.min(GARBAGE_CAPACITY) {
                let Ok(item) = link.garbage.pop() else { break };
                retirement.garbage.push(item);
            }
        }
        state.maintain();
    }

    pub fn project_preparation_error(&self) -> Option<ProjectPreparationError> {
        self.inner
            .preparation_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub(crate) fn latch_preparation_error(&self, error: ProjectPreparationError) {
        *self
            .inner
            .preparation_error
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(error);
    }
}

fn transport_message(transport: TransportState) -> Message {
    Message::SetTransport {
        mode: transport.mode,
        pattern: transport.pattern,
        loop_song: transport.loop_song,
        metronome: transport.metronome.unwrap_or_default(),
    }
}

impl ProjectPublicationLease<'_> {
    /// Sole producer ownership plus checked space makes these serial pushes
    /// infallible. The consumer can only increase space; it may interleave.
    pub fn install(mut self) -> ProjectRetirement {
        {
            let prepared = &mut self.prepared;
            let state = self.state.as_mut().expect("admitted guard");
            // Keep retirement in the outer borrowed token for the entire guard
            // lifetime. A panic cannot drop a unique local carrier first.
            let retired = prepared
                .retirement
                .as_mut()
                .expect("admitted retirement carrier");
            retired.old_plan = Some(std::mem::replace(
                &mut state.plan,
                prepared.plan.take().expect("admitted plan"),
            ));
            retired.old_ledger = std::mem::replace(&mut state.hosted, prepared.ledger.take());
            retired.snapshot = prepared.snapshot.take();
            retired.old_error = self
                .controller
                .inner
                .preparation_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            retired.old_sampler_error = self
                .controller
                .inner
                .sampler_error
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            state.plan_generation += 1;
            state.transport = prepared.transport.take().expect("admitted transport");
            state.sequence = prepared.sequence;
            if matches!(prepared.intent, ProjectPublicationIntent::Replace { .. }) {
                self.controller.panic_hardware();
                state.resume = false;
                self.controller.inner.shared.set_tick(0.0);
                self.controller.inner.shared.set_start(0.0);
                if state.link.is_none() {
                    self.controller
                        .inner
                        .shared
                        .publish_transport(state.sequence, false);
                }
            }
            if let Some(link) = &mut state.link {
                for index in 0..prepared.count {
                    let message = prepared.messages[index]
                        .take()
                        .expect("initialized publication prefix");
                    if let Err(PushError::Full(message)) = link.messages.push(message) {
                        std::mem::forget(message);
                        unreachable!("exclusive publication reservation lost");
                    }
                }
            }
        }
        drop(self.state.take());
        self.prepared
            .retirement
            .take()
            .expect("installed retirement carrier")
    }
}

impl Drop for ProjectPublicationLease<'_> {
    fn drop(&mut self) {
        if self.prepared.snapshot.is_some() {
            self.prepared.transport = None;
            if matches!(
                self.prepared.intent,
                ProjectPublicationIntent::Replace { .. }
            ) {
                self.prepared.messages[0] = self.prepared.messages[1].take();
            }
            for message in &mut self.prepared.messages[1..] {
                *message = None;
            }
            self.prepared.count = 0;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachmentMode {
    Device,
    Independent,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AttachmentAdmission {
    generation: u64,
}

pub(crate) struct ClosingStream {
    generation: u64,
    resume: bool,
    retirement: ProjectRetirement,
}

impl Controller {
    pub(crate) fn try_attach(
        &self,
        rate: u32,
        mode: AttachmentMode,
    ) -> Result<(Processor, AttachmentAdmission, ProjectRetirement), ProjectPreparationError> {
        let snapshot = self.preparation_snapshot();
        if matches!(snapshot.phase, StreamPhase::Starting | StreamPhase::Closing) {
            return Err(ProjectPreparationError::StreamTransitioning);
        }
        // The previous consumer must be destroyed before its control endpoints
        // can be displaced. An abandoned consumer permits a fresh retry.
        if self
            .lock()
            .link
            .as_ref()
            .is_some_and(|link| !link.messages.is_abandoned())
        {
            return Err(ProjectPreparationError::Publication(
                PublicationRefusal::StaleStream,
            ));
        }
        let mut plan = snapshot.plan.for_attachment();
        plan.audio_threads = snapshot.audio_threads;
        let plan = Arc::new(plan);
        if plan.meters.is_err() {
            return Err(ProjectPreparationError::Unsupported(
                "Invalid song meter map; project preparation refused.",
            ));
        }
        let factory = plan.factory_stamp;
        let (plan_state, ledger) = PlanState::try_build(&plan, rate, None)?;
        let ledger = Arc::new(ledger);
        let (message_tx, message_rx) = RingBuffer::new(MESSAGE_CAPACITY);
        let (garbage_tx, garbage_rx) = RingBuffer::new(GARBAGE_CAPACITY);
        let gain = self.lock().output_gain;
        let mut processor = Processor::with_queues(
            rate,
            plan.clone(),
            Box::new(plan_state),
            gain,
            message_rx,
            garbage_tx,
            self.inner.shared.clone(),
        );
        // All analyzer allocation is outside the controller/document guards.
        let spectrum =
            crate::spectrum::prepare(snapshot.plan_generation, rate).map(|(tap, reader)| {
                processor.set_spectrum(tap);
                reader
            });
        let captures = super::capture_control::attach(rate).map(|(audio, control)| {
            processor.set_captures(audio);
            control
        });
        let mut link = Some(Link {
            messages: message_tx,
            garbage: garbage_rx,
            spectrum,
            captures,
        });
        let mut retirement = ProjectRetirement::default();
        let admission;
        {
            let mut state = self.lock();
            if state
                .link
                .as_ref()
                .is_some_and(|link| !link.messages.is_abandoned())
            {
                return Err(ProjectPreparationError::Publication(
                    PublicationRefusal::StaleStream,
                ));
            }
            if snapshot.plan_generation != state.plan_generation
                || !Arc::ptr_eq(&snapshot.plan, &state.plan)
            {
                return Err(ProjectPreparationError::Publication(
                    PublicationRefusal::StalePlan,
                ));
            }
            if snapshot.stream_generation != state.stream_generation
                || snapshot.phase != state.stream_phase
            {
                return Err(ProjectPreparationError::Publication(
                    PublicationRefusal::StaleStream,
                ));
            }
            if snapshot.factory != FactoryStamp::capture(&state.plan.plugin_factory)
                || factory != FactoryStamp::capture(&plan.plugin_factory)
            {
                return Err(ProjectPreparationError::IdentityChanged);
            }
            // Leave room for a failed CPAL setup to close its admitted stream.
            if state.stream_generation > u64::MAX - 2 || state.plan_generation == u64::MAX {
                return Err(ProjectPreparationError::Publication(
                    PublicationRefusal::GenerationExhausted,
                ));
            }
            retirement.old_link = std::mem::replace(&mut state.link, link.take());
            retirement.backlog = Some(std::mem::take(&mut state.backlog));
            retirement.old_plan = Some(std::mem::replace(&mut state.plan, plan));
            retirement.old_ledger = state.hosted.replace(ledger);
            state.plan_generation += 1;
            state.stream_generation += 1;
            state.stream_phase = if mode == AttachmentMode::Device {
                StreamPhase::Starting
            } else {
                StreamPhase::Running
            };
            admission = AttachmentAdmission {
                generation: state.stream_generation,
            };
            self.panic_hardware();
            self.inner.shared.publish_automated(std::iter::empty());
            // Brand-new queues have room for this fixed numeric startup prefix.
            state.send_transport();
            let output_gain = state.output_gain;
            state.send(Message::SetOutputGain(output_gain));
            state.send(Message::Seek(self.inner.shared.start()));
            if state.resume {
                state.transport.playing = true;
                state.sequence = state.sequence.wrapping_add(1);
                let sequence = state.sequence;
                state.send(Message::Play {
                    sequence,
                    passes: None,
                    from: Some(self.inner.shared.tick()),
                });
            } else {
                state.transport.playing = false;
                self.inner.shared.publish_transport(state.sequence, false);
            }
        }
        retirement.snapshot = Some(snapshot);
        Ok((processor, admission, retirement))
    }

    pub(crate) fn complete_attachment(
        &self,
        admission: AttachmentAdmission,
    ) -> Result<(), PublicationRefusal> {
        let mut state = self.lock();
        if state.stream_generation != admission.generation
            || state.stream_phase != StreamPhase::Starting
        {
            return Err(PublicationRefusal::StaleStream);
        }
        state.stream_phase = StreamPhase::Running;
        Ok(())
    }

    /// Fence project admission before the device/processor's last references
    /// are destroyed. The existing endpoint is retained until finish_close.
    pub(crate) fn begin_close(&self, resume: bool) -> Result<ClosingStream, PublicationRefusal> {
        let retirement = ProjectRetirement::default();
        let generation;
        {
            let mut state = self.lock();
            generation = state
                .stream_generation
                .checked_add(1)
                .ok_or(PublicationRefusal::GenerationExhausted)?;
            state.stream_generation = generation;
            state.stream_phase = StreamPhase::Closing;
        }
        Ok(ClosingStream {
            generation,
            resume,
            retirement,
        })
    }

    pub(crate) fn finish_close(&self, mut closing: ClosingStream) {
        {
            let mut state = self.lock();
            assert_eq!(
                state.stream_generation, closing.generation,
                "serialized stream lifecycle"
            );
            assert_eq!(
                state.stream_phase,
                StreamPhase::Closing,
                "stream close admission"
            );
            state.resume = closing.resume && self.playing(&state);
            closing.retirement.old_link = state.link.take();
            closing.retirement.old_ledger = state.hosted.take();
            closing.retirement.backlog = Some(std::mem::take(&mut state.backlog));
            state.stream_phase = StreamPhase::Detached;
            self.panic_hardware();
            let shared = &self.inner.shared;
            if closing.resume {
                shared.publish_position(shared.tick(), shared.start(), 0);
            } else {
                state.transport.playing = false;
                shared.publish_transport(state.sequence, false);
                shared.publish_position(shared.start(), shared.start(), 0);
            }
            shared.publish_clips(0, 0);
        }
        // Whole endpoint/backlog ownership drops only after the guard, and in
        // production only after CPAL has stopped/dropped its Feeder.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::{HostedEffect, HostedInstrument, PluginFactory, PluginTransport};
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize};
    use std::sync::{Condvar, Weak};
    use windfall_project::{Command, Document, EffectId, PluginBinding, PluginTarget, TrackId};

    #[derive(Debug, Default)]
    struct Evidence {
        revision: AtomicU64,
        made: AtomicUsize,
        dropped: AtomicUsize,
        selected: AtomicUsize,
        fail: AtomicBool,
        latency: AtomicUsize,
        recording: Mutex<()>,
        block: Mutex<(bool, bool)>,
        barrier: Condvar,
        processed: Mutex<Vec<Arc<AtomicUsize>>>,
    }

    #[derive(Debug)]
    struct Factory {
        evidence: Arc<Evidence>,
        controller: Weak<Inner>,
        external: Arc<Mutex<()>>,
        staged: Option<u64>,
    }

    struct Unit {
        evidence: Arc<Evidence>,
        controller: Weak<Inner>,
        external: Arc<Mutex<()>>,
        ring: Vec<[f32; 2]>,
        cursor: usize,
        processed: Arc<AtomicUsize>,
    }
    impl HostedEffect for Unit {
        fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
            self.processed.fetch_add(left.len(), Ordering::Relaxed);
            for (left, right) in left.iter_mut().zip(right) {
                let before = self.ring[self.cursor];
                self.ring[self.cursor] = [*left, *right];
                [*left, *right] = before;
                self.cursor = (self.cursor + 1) % self.ring.len();
            }
        }
        fn set_param(&mut self, _: u32, _: f32) {}
        fn set_tempo(&mut self, _: f32) {}
        fn transport(&mut self, _: PluginTransport) {
            self.evidence.selected.fetch_add(1, Ordering::Relaxed);
        }
        fn latency(&self) -> usize {
            self.ring.len()
        }
        fn tail(&self) -> usize {
            0
        }
    }
    impl Drop for Unit {
        fn drop(&mut self) {
            assert!(
                self.external.try_lock().is_ok(),
                "external document/recording guard at native Drop"
            );
            assert!(
                self.evidence.recording.try_lock().is_ok(),
                "recording guard at native Drop"
            );
            if let Some(controller) = self.controller.upgrade() {
                assert!(
                    !matches!(
                        controller.state.try_lock(),
                        Err(std::sync::TryLockError::WouldBlock)
                    ),
                    "controller guard at native Drop"
                );
            }
            self.evidence.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    impl PluginFactory for Factory {
        fn provider_identity(&self) -> u64 {
            Arc::as_ptr(&self.evidence) as usize as u64
        }
        fn revision(&self) -> u64 {
            self.staged
                .unwrap_or_else(|| self.evidence.revision.load(Ordering::Relaxed))
        }
        fn effect(
            &self,
            _: &PluginBinding,
            rate: u32,
            block: usize,
        ) -> Result<Box<dyn HostedEffect>, String> {
            assert_eq!(rate, 48_000);
            assert_eq!(block, 256);
            self.evidence.made.fetch_add(1, Ordering::Relaxed);
            {
                let mut state = self.evidence.block.lock().unwrap();
                if state.0 {
                    state.1 = true;
                    self.evidence.barrier.notify_all();
                    while state.0 {
                        state = self.evidence.barrier.wait(state).unwrap();
                    }
                }
            }
            if self.evidence.fail.load(Ordering::Relaxed) {
                return Err("actual constructor refused".into());
            }
            let latency = self.evidence.latency.load(Ordering::Relaxed).max(37);
            let processed = Arc::new(AtomicUsize::new(0));
            self.evidence
                .processed
                .lock()
                .unwrap()
                .push(processed.clone());
            Ok(Box::new(Unit {
                evidence: self.evidence.clone(),
                controller: self.controller.clone(),
                external: self.external.clone(),
                ring: vec![[0.0; 2]; latency],
                cursor: 0,
                processed,
            }))
        }
        fn instrument(
            &self,
            _: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedInstrument>, String> {
            Err("fixture has effects only".into())
        }
    }

    fn fixture(
        count: usize,
    ) -> (
        Processor,
        Controller,
        Project,
        SamplePool,
        Arc<Evidence>,
        Arc<Mutex<()>>,
    ) {
        let (processor, controller) = Processor::new(48_000);
        let external = Arc::new(Mutex::new(()));
        let evidence = Arc::new(Evidence::default());
        let factory = Arc::new(Factory {
            evidence: evidence.clone(),
            controller: Arc::downgrade(&controller.inner),
            external: external.clone(),
            staged: None,
        });
        let mut pool = SamplePool::new();
        pool.set_plugin_factory(factory);
        let mut document = Document::new(Project::new("P1 native readiness"));
        for _ in 0..count {
            document
                .dispatch(
                    Command::AddPluginEffect {
                        track: TrackId(0),
                        plugin: PluginBinding {
                            target: PluginTarget::Effect {
                                effect: EffectId(0),
                            },
                            format: "clap".into(),
                            path: "p1-test.clap".into(),
                            id: "p1".into(),
                            name: "P1".into(),
                            state: vec![],
                            parameters: vec![],
                            sidechain_input: None,
                            auxiliary_inputs: Vec::new(),
                            capabilities: Default::default(),
                            wrapper: Default::default(),
                        },
                    },
                    None,
                )
                .unwrap();
        }
        (
            processor,
            controller,
            document.project().clone(),
            pool,
            evidence,
            external,
        )
    }

    fn ready(controller: &Controller, project: &Project, pool: &SamplePool) -> PreparedPublication {
        controller
            .preparation_snapshot()
            .prepare(project, pool, ProjectPublicationIntent::Edit)
            .unwrap()
    }

    fn sounding(project: &mut Project, pool: &mut SamplePool, controller: &Controller) {
        use windfall_project::{Channel, ChannelSource, SampleId, SamplerSettings};
        let channel = ChannelId(project.next_id);
        project.next_id += 1;
        let sample = SampleId(project.next_id);
        project.next_id += 1;
        pool.insert(
            sample,
            AudioBuffer::from_interleaved(48_000, 1, vec![1.0; 16_000]),
        );
        project.channels.push(Channel {
            sample_playback: None,
            id: channel,
            name: String::new(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            mixer_track: TrackId(0),
            group: String::new(),
            timing: Default::default(),
            voice: Default::default(),
            expression_map: Default::default(),
            sample_instrument: None,
            source: ChannelSource::Sampler(SamplerSettings {
                sample: Some(sample),
                ..Default::default()
            }),
        });
        controller.set_project(project, pool);
        controller.note_on(channel, 60, 1.0);
    }

    fn audio(processor: &mut Processor, frames: usize) {
        for _ in 0..frames {
            assert_eq!(
                crate::test_alloc::allocator_calls(|| processor.process(&mut [0.0; 2])),
                0
            );
        }
    }

    #[test]
    fn p1_p20_g30_successive_pending_publications_keep_or_refuse_third_before_construction() {
        let (mut processor, controller, mut project, mut pool, evidence, _) = fixture(10);
        sounding(&mut project, &mut pool, &controller);
        audio(&mut processor, 1000);
        let mut removed = project.clone();
        removed.mixer.tracks[0].effects.clear();
        removed.plugins.clear();
        controller.set_project(&removed, &pool);
        audio(&mut processor, 80);
        controller.set_project(&project, &pool);
        audio(&mut processor, 1);
        let mut removed_ready = ready(&controller, &removed, &pool);
        install(&controller, &mut removed_ready);
        let mut restored_ready = ready(&controller, &project, &pool);
        let plan = restored_ready.plan.as_ref().unwrap();
        assert_eq!(plan.tracks[0].effects.len(), 20);
        let generations: std::collections::HashSet<_> = plan.tracks[0]
            .effects
            .iter()
            .flat_map(|row| {
                let mut generations = Vec::new();
                if let Some(reservation) = &row.departure {
                    generations.extend(
                        reservation
                            .sources
                            .iter()
                            .flatten()
                            .map(|source| source.life.generation),
                    );
                } else {
                    generations.push(row.life.generation);
                }
                generations
            })
            .collect();
        assert_eq!(generations.len(), 30);
        for index in 0..10 {
            assert!(!plan.tracks[0].effects[plan.effect_places[index].1].leaving);
        }
        assert_eq!(evidence.made.load(Ordering::Relaxed), 30);
        install(&controller, &mut restored_ready);
        // Paired same-id revisions supersede the unheard current owner; they
        // do not add it to the already-reserved departure universe.
        for _ in 0..2 {
            evidence.revision.fetch_add(1, Ordering::Relaxed);
            let mut revision = ready(&controller, &project, &pool);
            assert_eq!(revision.plan.as_ref().unwrap().tracks[0].effects.len(), 20);
            install(&controller, &mut revision);
        }
        assert_eq!(evidence.made.load(Ordering::Relaxed), 50);
        let held = ready(&controller, &project, &pool);
        assert_eq!(
            evidence.made.load(Ordering::Relaxed),
            50,
            "retained pending owner is never speculatively rebuilt"
        );
        drop(held);
        let before = controller.lock().plan.clone();
        assert!(matches!(
            controller.preparation_snapshot().prepare(
                &removed,
                &pool,
                ProjectPublicationIntent::Edit
            ),
            Err(ProjectPreparationError::UnresolvedProgress)
        ));
        assert!(Arc::ptr_eq(&before, &controller.lock().plan));
        assert_eq!(evidence.made.load(Ordering::Relaxed), 50);
        audio(&mut processor, 1000);
        let retry = ready(&controller, &removed, &pool);
        assert!(retry.plan.as_ref().unwrap().tracks[0].effects.len() <= 20);
    }

    #[test]
    fn p1_replace_reserves_full_prefix_and_binds_current_transport_in_serial_order() {
        for space in [0, 3, 4] {
            let controller = Controller::new();
            let empty = Arc::new(Plan::empty());
            let (_, ledger) = PlanState::try_build(&empty, 48_000, None).unwrap();
            let (mut producer, mut consumer) = RingBuffer::new(MESSAGE_CAPACITY);
            let (_garbage, garbage) = RingBuffer::new(GARBAGE_CAPACITY);
            for _ in 0..MESSAGE_CAPACITY - space {
                producer.push(Message::Seek(9.0)).ok().unwrap();
            }
            {
                let mut state = controller.lock();
                state.hosted = Some(Arc::new(ledger));
                state.link = Some(Link {
                    messages: producer,
                    garbage,
                    spectrum: None,
                    captures: None,
                });
                state.stream_phase = StreamPhase::Running;
                state.sequence = u32::MAX;
                state.transport.loop_song = true;
            }
            let mut candidate = controller
                .preparation_snapshot()
                .prepare(
                    &Project::new("replacement"),
                    &SamplePool::new(),
                    ProjectPublicationIntent::Replace {
                        transport: TransportPatch::default(),
                    },
                )
                .unwrap();
            let original = controller.lock().plan.clone();
            let mut retirement = None;
            if space < 4 {
                assert!(matches!(
                    controller.publication(&mut candidate),
                    Err(PublicationRefusal::QueueFull)
                ));
                assert!(Arc::ptr_eq(&original, &controller.lock().plan));
                assert_eq!(controller.lock().sequence, u32::MAX);
            } else {
                let lease = controller.publication(&mut candidate).unwrap();
                assert_eq!(
                    crate::test_alloc::allocator_calls(|| retirement = Some(lease.install())),
                    0
                );
                for _ in 0..MESSAGE_CAPACITY - space {
                    assert!(matches!(consumer.pop(), Ok(Message::Seek(9.0))));
                }
                assert!(matches!(consumer.pop(), Ok(Message::Stop { sequence: 0 })));
                let plan_message = consumer.pop().unwrap();
                assert!(matches!(plan_message, Message::SetPlan { .. }));
                assert!(matches!(
                    consumer.pop(),
                    Ok(Message::SetTransport {
                        loop_song: true,
                        ..
                    })
                ));
                assert!(matches!(consumer.pop(), Ok(Message::Seek(0.0))));
                // The test consumes each push in serial order; group delivery
                // in one callback is neither required nor asserted.
                drop(plan_message);
            }
            drop(retirement);
        }
    }

    #[test]
    fn p1_adoption_after_acceptance_chooses_actual_outgoing_remainder_or_first_heard_successor() {
        use crate::state::{Fades, Heard};
        for advanced in [100, 250, 300] {
            let (_unused, controller, project, pool, evidence, _) = fixture(1);
            let original = Arc::new(compile(&project, &pool));
            let (mut original_state, original_ledger) =
                PlanState::try_build(&original, 48_000, None).unwrap();
            let mut left = [1.0];
            let mut right = [1.0];
            let mut dry_left = [0.0];
            let mut dry_right = [0.0];
            original_state.chains[0][0].as_mut().unwrap().process(
                &mut left,
                &mut right,
                &mut dry_left,
                &mut dry_right,
            );
            let mut removed_project = project.clone();
            removed_project.mixer.tracks[0].effects.clear();
            removed_project.plugins.clear();
            let mut removed = compile(&removed_project, &pool);
            removed
                .reserve_departures(&original, &original_ledger)
                .unwrap();
            let removed = Arc::new(removed);
            let (mut removed_state, removed_ledger) =
                PlanState::try_build(&removed, 48_000, Some(&original_ledger)).unwrap();
            removed_state.take_over(
                &removed,
                &original,
                &mut original_state,
                [Heard::Track(0)].into_iter(),
                0,
                Fades::at(48_000),
            );
            let mut restored = compile(&project, &pool);
            restored
                .reserve_departures(&removed, &removed_ledger)
                .unwrap();
            let restored = Arc::new(restored);
            let (mut actual, ledger) =
                PlanState::try_build(&restored, 48_000, Some(&removed_ledger)).unwrap();
            actual.take_over(
                &restored,
                &removed,
                &mut removed_state,
                [Heard::Track(0)].into_iter(),
                0,
                Fades::at(48_000),
            );
            let outgoing = actual.chains[0][0].as_ref().unwrap().generation();
            let fresh = actual.chains[0][1].as_ref().unwrap().generation();
            let (messages, mut receiver) = RingBuffer::new(MESSAGE_CAPACITY);
            let (_garbage, garbage) = RingBuffer::new(GARBAGE_CAPACITY);
            let displaced;
            {
                let mut state = controller.lock();
                displaced = state.link.replace(Link {
                    messages,
                    garbage,
                    spectrum: None,
                    captures: None,
                });
                state.plan = restored.clone();
                state.hosted = Some(Arc::new(ledger));
            }
            drop(displaced);
            let mut ready = ready(&controller, &removed_project, &pool);
            let mut retirement = Some(controller.publication(&mut ready).unwrap().install());
            let Message::SetPlan { plan, mut state } = receiver.pop().unwrap() else {
                panic!("ready SetPlan")
            };
            // Model a callback delayed by existing garbage backpressure: the
            // admitted message stays owned while the actual predecessor's
            // single-frame DSP advances. No callback transaction is introduced.
            for _ in 0..advanced {
                left[0] = 1.0;
                right[0] = 1.0;
                assert_eq!(
                    crate::test_alloc::allocator_calls(|| {
                        for unit in actual.chains[0].iter_mut().flatten() {
                            unit.process(&mut left, &mut right, &mut dry_left, &mut dry_right);
                        }
                    }),
                    0
                );
            }
            assert_eq!(
                crate::test_alloc::allocator_calls(|| state.take_over(
                    &plan,
                    &restored,
                    &mut actual,
                    [Heard::Track(0)].into_iter(),
                    advanced,
                    Fades::at(48_000)
                )),
                0
            );
            if advanced == 100 {
                let selected = state.chains[0][0].as_ref().unwrap();
                assert_eq!(selected.generation(), outgoing);
                assert_eq!(
                    selected.removal_remaining(),
                    140,
                    "old removal never restarts"
                );
            } else if advanced == 300 {
                let selected = state.chains[0][0].as_ref().unwrap();
                assert_eq!(selected.generation(), fresh);
                assert_eq!(selected.removal_remaining(), 240);
            } else {
                assert!(
                    state.chains[0][0].is_none(),
                    "completed outgoing and unheard successor select no source"
                );
                assert!(
                    !state.shaped[0],
                    "inactive row/bank never shapes a scalar path"
                );
                let following = controller
                    .preparation_snapshot()
                    .prepare(&removed_project, &pool, ProjectPublicationIntent::Edit)
                    .unwrap();
                assert!(
                    following.plan.as_ref().unwrap().tracks[0]
                        .effects
                        .is_empty(),
                    "AdoptedNone is not provisional native evidence on the next edit"
                );
            }
            assert_eq!(
                evidence.made.load(Ordering::Relaxed),
                2,
                "alternatives never construct native owners"
            );
            assert_eq!(
                evidence.selected.load(Ordering::Relaxed),
                0,
                "adoption never selects native capture"
            );
            if let Some(retirement) = &mut retirement {
                controller.take_retired(retirement);
            }
        }
    }

    fn install(controller: &Controller, prepared: &mut PreparedPublication) {
        let mut retirement = controller.publication(prepared).unwrap().install();
        controller.take_retired(&mut retirement);
    }

    #[test]
    fn p1_blocked_native_constructor_leaves_control_and_external_guards_available() {
        let (mut processor, controller, project, pool, evidence, external) = fixture(1);
        evidence.block.lock().unwrap().0 = true;
        let snapshot = controller.preparation_snapshot();
        let worker = std::thread::spawn(move || {
            snapshot.prepare(&project, &pool, ProjectPublicationIntent::Edit)
        });
        {
            let mut state = evidence.block.lock().unwrap();
            while !state.1 {
                state = evidence.barrier.wait(state).unwrap();
            }
        }
        controller.frame();
        controller.transport();
        controller.seek(144.0);
        controller.play();
        controller.stop();
        assert!(controller.inner.state.try_lock().is_ok());
        assert!(external.try_lock().is_ok());
        evidence.block.lock().unwrap().0 = false;
        evidence.barrier.notify_all();
        let mut prepared = worker.join().unwrap().unwrap();
        assert_eq!(evidence.selected.load(Ordering::Relaxed), 0);
        let mut retirement = None;
        {
            let _document = external.lock().unwrap();
            let lease = controller.publication(&mut prepared).unwrap();
            assert_eq!(
                crate::test_alloc::allocator_calls(|| retirement = Some(lease.install())),
                0
            );
            controller.frame();
            assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
        }
        drop(retirement);
        drop(prepared);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut [0.0; 2])),
            0
        );
        assert!(evidence.selected.load(Ordering::Relaxed) > 0);
    }

    #[test]
    fn p1_stale_refusal_is_borrowed_and_native_drop_is_after_both_guards() {
        let (_processor, controller, project, pool, evidence, external) = fixture(1);
        let mut candidate = ready(&controller, &project, &pool);
        controller.set_project(&Project::new("replacement"), &SamplePool::new());
        {
            let _document = external.lock().unwrap();
            assert!(matches!(
                controller.publication(&mut candidate),
                Err(PublicationRefusal::StalePlan)
            ));
            assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
        }
        drop(candidate);
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn p1_stalled_plan_stream_rate_and_factory_changes_refuse_before_publication() {
        for action in 0..4 {
            let (processor, controller, project, pool, evidence, external) = fixture(1);
            evidence.block.lock().unwrap().0 = true;
            let snapshot = controller.preparation_snapshot();
            let worker = std::thread::spawn(move || {
                snapshot.prepare(&project, &pool, ProjectPublicationIntent::Edit)
            });
            {
                let mut blocked = evidence.block.lock().unwrap();
                while !blocked.1 {
                    blocked = evidence.barrier.wait(blocked).unwrap();
                }
            }
            let mut processor = Some(processor);
            let mut reopened = None;
            match action {
                0 => controller.set_project(&Project::new("new/open/edit"), &SamplePool::new()),
                1 | 2 => {
                    let closing = controller.begin_close(false).unwrap();
                    drop(processor.take());
                    controller.finish_close(closing);
                    let (new, admission, retired) = controller
                        .try_attach(
                            if action == 1 { 48_000 } else { 44_100 },
                            AttachmentMode::Device,
                        )
                        .unwrap();
                    drop(retired);
                    controller.complete_attachment(admission).unwrap();
                    reopened = Some(new);
                }
                _ => {
                    evidence.revision.fetch_add(1, Ordering::Relaxed);
                }
            }
            let accepted = controller.lock().plan.clone();
            let transport = controller.transport();
            evidence.block.lock().unwrap().0 = false;
            evidence.barrier.notify_all();
            match worker.join().unwrap() {
                Ok(mut candidate) => {
                    {
                        let _recording = evidence.recording.lock().unwrap();
                        let _document = external.lock().unwrap();
                        assert!(matches!(
                            controller.publication(&mut candidate),
                            Err(PublicationRefusal::StalePlan | PublicationRefusal::StaleStream)
                        ));
                        assert!(Arc::ptr_eq(&accepted, &controller.lock().plan));
                        assert_eq!(transport, controller.transport());
                        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
                    }
                    drop(candidate);
                }
                Err(error) => assert_eq!(error, ProjectPreparationError::IdentityChanged),
            }
            assert_eq!(evidence.made.load(Ordering::Relaxed), 1);
            assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
            drop(reopened);
            drop(processor);
        }
    }

    #[test]
    fn p1_live_attachment_refuses_before_construction_and_abandoned_retry_retires_after_shutdown() {
        let (processor, controller, project, pool, evidence, _) = fixture(1);
        controller.set_project(&project, &pool);
        let made = evidence.made.load(Ordering::Relaxed);
        let plan = controller.lock().plan.clone();
        let generation = controller.lock().stream_generation;
        assert!(matches!(
            controller.try_attach(48_000, AttachmentMode::Independent),
            Err(ProjectPreparationError::Publication(
                PublicationRefusal::StaleStream
            ))
        ));
        assert_eq!(evidence.made.load(Ordering::Relaxed), made);
        assert_eq!(controller.lock().stream_generation, generation);
        assert!(Arc::ptr_eq(&plan, &controller.lock().plan));
        drop(processor);
        // Queued native-bearing SetPlan still belongs to the retained endpoints.
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
        let (replacement, _, retirement) = controller
            .try_attach(48_000, AttachmentMode::Independent)
            .unwrap();
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
        drop(retirement);
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
        let closing = controller.begin_close(false).unwrap();
        drop(replacement);
        // Processor-owned units may retire at its control-side destruction;
        // its control endpoints remain retained until that destruction ends.
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 2);
        assert!(
            controller
                .lock()
                .link
                .as_ref()
                .unwrap()
                .messages
                .is_abandoned()
        );
        controller.finish_close(closing);
        assert!(controller.lock().link.is_none());
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn p1_attachment_revalidates_live_consumer_after_stalled_construction() {
        let (processor, controller, project, pool, evidence, _) = fixture(1);
        drop(processor);
        controller.detach();
        controller.set_project(&project, &pool);
        evidence.block.lock().unwrap().0 = true;
        let pending = controller.clone();
        let worker =
            std::thread::spawn(move || pending.try_attach(48_000, AttachmentMode::Independent));
        {
            let mut block = evidence.block.lock().unwrap();
            while !block.1 {
                block = evidence.barrier.wait(block).unwrap();
            }
        }
        controller.set_project(&Project::new("winning attachment"), &SamplePool::new());
        let (winner, _, retirement) = controller
            .try_attach(48_000, AttachmentMode::Independent)
            .unwrap();
        drop(retirement);
        let generation = controller.lock().stream_generation;
        evidence.block.lock().unwrap().0 = false;
        evidence.barrier.notify_all();
        assert!(matches!(
            worker.join().unwrap(),
            Err(ProjectPreparationError::Publication(
                PublicationRefusal::StaleStream
            ))
        ));
        assert_eq!(controller.lock().stream_generation, generation);
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
        let closing = controller.begin_close(false).unwrap();
        drop(winner);
        controller.finish_close(closing);
    }

    #[test]
    fn p1_restore_then_move_before_departure_completion_switches_old_track_immediately() {
        let (mut processor, controller, mut project, mut pool, evidence, _) = fixture(1);
        let mut second = project.mixer.tracks[0].clone();
        second.id = TrackId(project.next_id);
        project.next_id += 1;
        second.effects.clear();
        second.output = Some(TrackId(0));
        project.mixer.tracks.push(second);
        sounding(&mut project, &mut pool, &controller);
        project.channels[0].mixer_track = project.mixer.tracks[1].id;
        controller.set_project(&project, &pool);
        audio(&mut processor, 1000);
        let mut removed = project.clone();
        removed.mixer.tracks[0].effects.clear();
        removed.plugins.clear();
        controller.set_project(&removed, &pool);
        audio(&mut processor, 30);
        controller.set_project(&project, &pool);
        audio(&mut processor, 1);
        let restored = controller.lock().plan.tracks[0].effects[1].life.clone();
        assert!(!restored.heard());
        let outgoing = controller.lock().plan.tracks[0].effects[0].life.clone();
        assert!(outgoing.heard(), "departure remains unfinished at the move");
        let old = evidence.processed.lock().unwrap()[0].clone();
        let active = evidence.processed.lock().unwrap()[1].clone();
        let before = old.load(Ordering::Relaxed);
        let mut moved = project;
        let effect = moved.mixer.tracks[0].effects.remove(0);
        moved.mixer.tracks[1].effects.push(effect);
        let mut candidate = ready(&controller, &moved, &pool);
        assert!(
            candidate.plan.as_ref().unwrap().tracks[0]
                .effects
                .is_empty()
        );
        let row = &candidate.plan.as_ref().unwrap().tracks[1].effects[0];
        assert!(Arc::ptr_eq(&row.life, &restored));
        assert!(row.after_departure.is_none());
        assert_eq!(evidence.made.load(Ordering::Relaxed), 2);
        install(&controller, &mut candidate);
        audio(&mut processor, 128);
        assert_eq!(old.load(Ordering::Relaxed), before);
        assert!(active.load(Ordering::Relaxed) > 0);
        assert!(restored.heard());
    }

    #[test]
    fn p1_native_constructor_error_is_distinct_from_unavailable_and_sampler() {
        let (_processor, controller, project, pool, evidence, _) = fixture(1);
        evidence.fail.store(true, Ordering::Relaxed);
        assert!(matches!(
            controller.preparation_snapshot().prepare(
                &project,
                &pool,
                ProjectPublicationIntent::Edit
            ),
            Err(ProjectPreparationError::Native { .. })
        ));
        let unavailable = ready(&controller, &project, &SamplePool::new());
        let ledger = unavailable.ledger.as_ref().unwrap();
        assert_eq!(
            ledger.native_identity(project.mixer.tracks[0].effects[0].id),
            None
        );
        assert_eq!(evidence.selected.load(Ordering::Relaxed), 0);
        let mut invalid = project.clone();
        invalid.settings.time_signature.numerator = 0;
        assert!(matches!(
            controller.preparation_snapshot().prepare(
                &invalid,
                &pool,
                ProjectPublicationIntent::Edit
            ),
            Err(ProjectPreparationError::Sampler(_))
        ));
    }

    #[test]
    fn p1_same_provider_revision_different_factory_arcs_reuse_actual_owner_and_generation() {
        let (mut processor, controller, project, mut pool, evidence, external) = fixture(1);
        let mut first = ready(&controller, &project, &pool);
        install(&controller, &mut first);
        processor.process(&mut [0.0; 2]);
        let life = controller.lock().plan.tracks[0].effects[0].life.clone();
        pool.set_plugin_factory(Arc::new(Factory {
            evidence: evidence.clone(),
            controller: Arc::downgrade(&controller.inner),
            external,
            staged: None,
        }));
        let mut second = ready(&controller, &project, &pool);
        assert!(Arc::ptr_eq(
            &life,
            &second.plan.as_ref().unwrap().tracks[0].effects[0].life
        ));
        assert_eq!(evidence.made.load(Ordering::Relaxed), 1);
        install(&controller, &mut second);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut [0.0; 2])),
            0
        );
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn p1_staged_revision_compares_actual_ledger_and_preserves_old_installed_factory_stamp() {
        let (mut processor, controller, project, mut pool, evidence, external) = fixture(1);
        let mut first = ready(&controller, &project, &pool);
        install(&controller, &mut first);
        audio(&mut processor, 1);
        let installed = controller.lock().hosted.clone().unwrap();
        let old = controller.lock().plan.tracks[0].effects[0].life.clone();
        let effect = project.mixer.tracks[0].effects[0].id;
        let before = installed.native_identity(effect).unwrap();
        pool.set_plugin_factory(Arc::new(Factory {
            evidence: evidence.clone(),
            controller: Arc::downgrade(&controller.inner),
            external,
            staged: Some(7),
        }));
        let mut next = ready(&controller, &project, &pool);
        assert!(!Arc::ptr_eq(
            &old,
            &next.plan.as_ref().unwrap().tracks[0].effects[0].life
        ));
        assert_ne!(
            next.ledger.as_ref().unwrap().native_identity(effect),
            Some(before)
        );
        assert_eq!(evidence.made.load(Ordering::Relaxed), 2);
        assert_eq!(
            evidence.revision.load(Ordering::Relaxed),
            0,
            "preparation never publishes an installed revision"
        );
        install(&controller, &mut next);
        let same = ready(&controller, &project, &pool);
        assert_eq!(evidence.made.load(Ordering::Relaxed), 2);
        assert!(Arc::ptr_eq(
            &controller.lock().plan.tracks[0].effects[0].life,
            &same.plan.as_ref().unwrap().tracks[0].effects[0].life
        ));
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut [0.0; 2])),
            0
        );
    }

    #[test]
    fn p1_dense_conditional_routes_refuse_visible_memory_limit_before_native_construction() {
        let (mut processor, controller, mut project, mut pool, evidence, _) = fixture(10);
        sounding(&mut project, &mut pool, &controller);
        audio(&mut processor, 1000);
        let mut requested = project.clone();
        requested.mixer.tracks[0].effects.clear();
        requested.plugins.clear();
        for _ in 1..windfall_project::MAX_MIXER_TRACKS {
            let mut track = requested.mixer.tracks[0].clone();
            track.id = TrackId(requested.next_id);
            requested.next_id += 1;
            track.output = Some(TrackId(0));
            requested.mixer.tracks.push(track);
        }
        let mut slot = project.mixer.tracks[0].effects[0].clone();
        slot.id = EffectId(requested.next_id);
        requested.next_id += 1;
        let mut binding = project.plugins[0].clone();
        binding.target = PluginTarget::Effect { effect: slot.id };
        requested.mixer.tracks[1].effects.push(slot);
        requested.plugins.push(binding);
        let before = controller.lock().plan.clone();
        assert!(
            matches!(controller.preparation_snapshot().prepare(&requested, &pool, ProjectPublicationIntent::Edit),
            Err(ProjectPreparationError::MemoryLimit { required, limit }) if required > limit && limit == 256 * 1024 * 1024)
        );
        assert!(Arc::ptr_eq(&before, &controller.lock().plan));
        assert_eq!(
            evidence.made.load(Ordering::Relaxed),
            10,
            "determinable route budget refuses before the fresh constructor"
        );
    }

    #[test]
    fn p1_negotiated_native_route_beyond_host_compensation_refuses_without_latency_clamp() {
        let (_processor, controller, mut project, pool, evidence, _) = fixture(1);
        let mut track = project.mixer.tracks[0].clone();
        track.id = TrackId(project.next_id);
        project.next_id += 1;
        track.output = Some(TrackId(0));
        project.mixer.tracks[0].effects.clear();
        project.mixer.tracks.push(track);
        evidence.latency.store(48_001, Ordering::Relaxed);
        let before = controller.lock().plan.clone();
        assert!(matches!(
            controller.preparation_snapshot().prepare(
                &project,
                &pool,
                ProjectPublicationIntent::Edit
            ),
            Err(ProjectPreparationError::Unsupported(
                "Prepared native graph exceeds the one-second host compensation bound."
            ))
        ));
        assert!(Arc::ptr_eq(&before, &controller.lock().plan));
        assert_eq!(evidence.made.load(Ordering::Relaxed), 1);
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn p1_cancelled_replace_lease_preserves_owned_candidate_and_late_transport() {
        let (_processor, controller, project, pool, evidence, external) = fixture(1);
        let mut candidate = controller
            .preparation_snapshot()
            .prepare(
                &project,
                &pool,
                ProjectPublicationIntent::Replace {
                    transport: TransportPatch::default(),
                },
            )
            .unwrap();
        let original = controller.lock().plan.clone();
        {
            let _document = external.lock().unwrap();
            drop(controller.publication(&mut candidate).unwrap());
            assert!(Arc::ptr_eq(&original, &controller.lock().plan));
            assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
        }
        controller.set_transport(TransportPatch {
            loop_song: Some(true),
            ..Default::default()
        });
        install(&controller, &mut candidate);
        assert!(controller.transport().loop_song);
        assert!(!controller.transport().playing);
        assert_eq!(controller.frame().tick, 0.0);
    }

    #[test]
    fn p1_edit_late_pattern_removal_and_generation_exhaustion_refuse_before_mutation() {
        let (mut processor, controller) = Processor::new(48_000);
        let mut document = Document::new(Project::new("late edit"));
        document
            .dispatch(Command::AddPattern { name: None }, None)
            .unwrap();
        let requested = document.project().patterns[1].id;
        controller.set_project(document.project(), &SamplePool::new());
        audio(&mut processor, 1);
        document
            .dispatch(Command::RemovePattern { id: requested }, None)
            .unwrap();
        let mut candidate = controller
            .preparation_snapshot()
            .prepare(
                document.project(),
                &SamplePool::new(),
                ProjectPublicationIntent::Edit,
            )
            .unwrap();
        controller.set_transport(TransportPatch {
            pattern: Some(requested),
            ..Default::default()
        });
        controller.play();
        controller.seek(720.0);
        controller.stop();
        controller.play();
        let late = controller.lock().transport;
        let sequence = controller.lock().sequence;
        let tick = controller.frame().tick;
        let lease = controller.publication(&mut candidate).unwrap();
        assert_eq!(lease.prepared.count, 2);
        assert_eq!(
            lease.prepared.transport.unwrap().pattern,
            document.project().patterns[0].id
        );
        assert_eq!(lease.prepared.transport.unwrap().playing, late.playing);
        assert_eq!(lease.prepared.sequence, sequence);
        let retired = lease.install();
        drop(retired);
        assert_eq!(
            controller.frame().tick,
            tick,
            "ordinary edit never replaces a late seek"
        );
        audio(&mut processor, 1);
        assert!(controller.frame().playing);
        let before = controller.lock().plan.clone();
        {
            let mut state = controller.lock();
            state.plan_generation = u64::MAX;
        }
        let mut exhausted = controller
            .preparation_snapshot()
            .prepare(
                document.project(),
                &SamplePool::new(),
                ProjectPublicationIntent::Edit,
            )
            .unwrap();
        assert!(matches!(
            controller.publication(&mut exhausted),
            Err(PublicationRefusal::GenerationExhausted)
        ));
        assert!(Arc::ptr_eq(&before, &controller.lock().plan));
        {
            controller.lock().stream_generation = u64::MAX;
        }
        assert!(matches!(
            controller.begin_close(false),
            Err(PublicationRefusal::GenerationExhausted)
        ));
        assert_eq!(controller.lock().stream_phase, StreamPhase::Running);
    }

    #[test]
    fn p1_borrowed_lease_unwind_poison_and_full_retirement_never_destroy_under_guards() {
        use std::panic::{AssertUnwindSafe, catch_unwind};
        let (_processor, controller, project, pool, evidence, external) = fixture(1);
        let mut candidate = ready(&controller, &project, &pool);
        let before = controller.lock().plan.clone();
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let _recording = evidence.recording.lock().unwrap();
                let _document = external.lock().unwrap();
                let _lease = controller.publication(&mut candidate).unwrap();
                panic!("document dispatch panicked before install");
            }))
            .is_err()
        );
        assert!(Arc::ptr_eq(&before, &controller.lock().plan));
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
        external.clear_poison();
        evidence.recording.clear_poison();
        // A poisoned admission mutex remains usable; cancellation restored the
        // outer token, rather than destroying its initialized native message.
        let retired = controller.publication(&mut candidate).unwrap().install();
        drop(retired);
        drop(candidate);

        let plan = Arc::new(compile(&project, &pool));
        let (state, _) = PlanState::try_build(&plan, 48_000, None).unwrap();
        let (mut garbage_tx, garbage_rx) = RingBuffer::new(GARBAGE_CAPACITY);
        garbage_tx
            .push(Garbage::State(Box::new(state)))
            .ok()
            .unwrap();
        for _ in 1..GARBAGE_CAPACITY {
            garbage_tx.push(Garbage::Plan(plan.clone())).ok().unwrap();
        }
        let displaced;
        {
            let mut guard = controller.lock();
            displaced = std::mem::replace(&mut guard.link.as_mut().unwrap().garbage, garbage_rx);
        }
        drop(displaced);
        let mut retired = ProjectRetirement::default();
        {
            let _recording = evidence.recording.lock().unwrap();
            let _document = external.lock().unwrap();
            assert_eq!(
                crate::test_alloc::allocator_calls(|| controller.take_retired(&mut retired)),
                0
            );
            assert_eq!(retired.garbage.len(), GARBAGE_CAPACITY);
            assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
        }
        let (state, _) = PlanState::try_build(&plan, 48_000, None).unwrap();
        garbage_tx
            .push(Garbage::State(Box::new(state)))
            .ok()
            .unwrap();
        let (state, _) = PlanState::try_build(&plan, 48_000, None).unwrap();
        let mut backlog = VecDeque::with_capacity(1);
        backlog.push_back(Message::SetPlan {
            plan: plan.clone(),
            state: Box::new(state),
        });
        {
            let _recording = evidence.recording.lock().unwrap();
            let _document = external.lock().unwrap();
            let snapshot = controller.preparation_snapshot();
            drop(snapshot);
            assert_eq!(
                crate::test_alloc::allocator_calls(|| controller.take_retired(&mut retired)),
                0
            );
            assert_eq!(retired.garbage.len(), GARBAGE_CAPACITY);
            assert_eq!(
                garbage_tx.slots(),
                GARBAGE_CAPACITY - 1,
                "a full outer carrier does not drain or drop another owner"
            );
            assert_eq!(evidence.dropped.load(Ordering::Relaxed), 0);
        }
        drop(retired);
        assert_eq!(evidence.dropped.load(Ordering::Relaxed), 1);
        {
            controller.lock().backlog = backlog;
        }
        let closing = controller.begin_close(false).unwrap();
        drop(_processor);
        drop(garbage_tx);
        controller.finish_close(closing);
        assert_eq!(evidence.made.load(Ordering::Relaxed), 4);
        assert_eq!(
            evidence.dropped.load(Ordering::Relaxed),
            4,
            "queued SetPlan, entire backlog and ring endpoints retire after guards"
        );
    }
}
