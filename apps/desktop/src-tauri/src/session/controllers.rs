//! Controllers: hardware controls linked to the project, learning those
//! links, recording what is played and what is moved, and the rolling log.
//!
//! # Threads and locks
//!
//! The MIDI worker calls [`Session::controller_message`],
//! [`Session::controller_played`] and [`Session::controller_idle`]. Each
//! takes the hub's lock for a moment and never touches the document, so a
//! slow edit cannot delay a note. What a message asks for is left in a
//! bounded queue.
//!
//! [`Session::controllers_pump`] empties that queue. It runs on the thread
//! [`Session::spawn_controllers`] starts, never on the MIDI worker or the
//! audio thread, and it is the only place a controller changes the project:
//! through the same checked commands as every other edit, so a movement
//! undoes, saves and renders offline like one made with the mouse.
//!
//! The hub's lock is a leaf: it may be taken while another lock is held,
//! the document's included, and nothing else is ever taken while it is.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use windfall_engine::Playhead;
use windfall_engine::midi_hardware::clock::{ClockIn, ClockInEvent};
use windfall_engine::midi_hardware::{InputMessage, MidiMessage};
use windfall_ipc::{
    AutomationRecordState, ControllerLearn, ControllerSettings, ControllerState,
    MidiHardwareSettings, MidiPort, NoteRecordState, PlayMode, TransportPatch,
};
use windfall_project::controller_links::{
    LinkState, MAX_CONTROLLER_LINKS, Motion, SLOT_BANK_SIZE, SourceInput, builtin_profiles,
    command_targets, links_problem, target_command,
};
use windfall_project::{
    AutomationId, AutomationPoint, AutomationTarget, ChannelId, ClipContent, ClipInit, Command,
    ControlAction, ControlMap, ControlSource, ControllerLink, ControllerProfile, DispatchResult,
    LinkTarget, MAX_SONG_TICKS, MidiControl, NoteInit, PatternId, PlaylistTrackId, Project,
    SettingsPatch, TICKS_PER_STEP,
};

use super::Session;
use super::controller_capture::{
    AutomationLane, AutomationTake, NoteTake, ScoreLog, TakenNote, TransportClock, merge_points,
    note_init, recorded_points,
};
use crate::sync::lock;

/// Controller messages that can wait for the pump.
const QUEUE: usize = 512;
/// Movements of one control closer together than this are one undo step.
const GESTURE_GAP: Duration = Duration::from_millis(600);
/// How long the pump sleeps with nothing to do, and while a glide runs.
const IDLE_WAIT: Duration = Duration::from_millis(50);
const BUSY_WAIT: Duration = Duration::from_millis(10);
/// Controls one pass of linking may hold.
const MAX_LEARN_TARGETS: usize = 64;
/// The furthest the bank commands move positions.
const MAX_BANK: u32 = 120;
/// User profile files read at most.
const MAX_USER_PROFILES: usize = 64;
/// Gesture ids of controller edits start here, far above the ids windows make.
const GESTURE_BASE: u64 = 1 << 62;
/// The gesture that follows an external clock's tempo.
const CLOCK_GESTURE: u64 = GESTURE_BASE - 1;

/// One message from a hardware control, as the pump takes it.
#[derive(Debug, Clone, PartialEq)]
struct Incoming {
    source: ControlSource,
    input: SourceInput,
    /// A key going down, or any movement of a continuous control: what a
    /// pass of linking takes as "this is the control I mean".
    learnable: bool,
}

struct Learn {
    targets: Vec<LinkTarget>,
    linked: usize,
    global: bool,
    multi: bool,
    gesture: u64,
    /// Hardware controls this pass has already given a target.
    seen: Vec<ControlSource>,
}

/// Everything about controllers that is not part of the project.
pub(super) struct Hub {
    settings: ControllerSettings,
    user_profiles: Vec<ControllerProfile>,
    /// The links of the profiles assigned to the open inputs.
    profile_links: Vec<ControllerLink>,
    feedback: Vec<windfall_project::controller_links::ProfileFeedback>,
    last_feedback: Vec<Vec<u8>>,
    /// Port names of the inputs, by their place in the settings.
    device_names: Vec<String>,
    follow_clock: bool,
    /// The source of every link in force, for the MIDI worker to tell a
    /// linked control from any other without the document.
    sources: Vec<ControlSource>,
    /// The document the sources were read from: generation and revision.
    stamp: Option<(u64, u64)>,
    generation: u64,
    queue: VecDeque<Incoming>,
    dropped: u32,
    states: HashMap<(ControlSource, LinkTarget), LinkState>,
    script_bindings: Vec<windfall_project::scripting::ControllerScript>,
    learn: Option<Learn>,
    bank: u32,
    last_input: Option<ControlSource>,
    last_tweaked: Option<AutomationTarget>,
    waiting: Vec<AutomationTarget>,
    gesture: Option<(u64, Instant)>,
    gestures: u64,
    last_pump: Option<Instant>,
    clock: TransportClock,
    /// Where the playhead last was while playing.
    last_tick: f64,
    /// Playback stopped since the pump last looked.
    stopped: bool,
    note_armed: bool,
    notes: NoteTake,
    automation_armed: bool,
    automation: AutomationTake,
    recording_gesture: Option<u64>,
    automation_destination: Option<super::pattern_movement::Destination>,
    log: ScoreLog,
    log_file: PathBuf,
    log_identity: String,
    log_dirty: bool,
    saved_log: Option<(String, ScoreLog)>,
    clock_in: ClockIn,
    clock_events: VecDeque<ClockInEvent>,
    expression: HashMap<(u8, u8), windfall_project::NoteExpression>,
    note_sources: HashMap<u8, (u8, u8)>,
    input_filter: Option<u8>,
    error: Option<String>,
}

impl Hub {
    pub(super) fn new(settings: ControllerSettings, profiles_dir: &Path) -> Self {
        Self {
            settings,
            user_profiles: load_profiles(profiles_dir),
            profile_links: Vec::new(),
            feedback: Vec::new(),
            last_feedback: Vec::new(),
            device_names: Vec::new(),
            follow_clock: false,
            sources: Vec::new(),
            stamp: None,
            generation: 0,
            queue: VecDeque::with_capacity(QUEUE),
            dropped: 0,
            states: HashMap::new(),
            script_bindings: Vec::new(),
            learn: None,
            bank: 0,
            last_input: None,
            last_tweaked: None,
            waiting: Vec::new(),
            gesture: None,
            gestures: 0,
            last_pump: None,
            clock: TransportClock::default(),
            last_tick: 0.0,
            stopped: false,
            note_armed: false,
            notes: NoteTake::default(),
            automation_armed: false,
            automation: AutomationTake::default(),
            recording_gesture: None,
            automation_destination: None,
            log: ScoreLog::default(),
            log_file: profiles_dir
                .parent()
                .unwrap_or(profiles_dir)
                .join("score-log.json"),
            log_identity: String::new(),
            log_dirty: false,
            saved_log: ScoreLog::load(
                &profiles_dir
                    .parent()
                    .unwrap_or(profiles_dir)
                    .join("score-log.json"),
            ),
            clock_in: ClockIn::default(),
            clock_events: VecDeque::new(),
            expression: HashMap::new(),
            note_sources: HashMap::new(),
            input_filter: None,
            error: None,
        }
    }

    fn profiles(&self) -> Vec<ControllerProfile> {
        let mut profiles = builtin_profiles();
        profiles.extend(self.user_profiles.iter().cloned());
        profiles
    }

    fn record_movement(
        &mut self,
        project: &Project,
        place: super::controller_capture::Position,
        target: AutomationTarget,
        before: f32,
        value: f32,
    ) {
        if place.mode == PlayMode::Pattern && target == AutomationTarget::Tempo {
            self.error = Some("Record tempo movement in song mode.".into());
            return;
        }
        if self.automation_destination.is_none() {
            self.automation_destination =
                super::pattern_movement::Destination::capture(project, place);
        }
        if !self
            .automation_destination
            .as_ref()
            .is_some_and(|destination| destination.matches(place))
        {
            self.error =
                Some("Recording destination changed. Stop and start a new automation pass.".into());
            return;
        }
        self.automation.record_in(
            (place.mode == PlayMode::Pattern).then_some(place.pattern),
            target,
            before,
            place.tick,
            value,
        );
    }

    /// Reads which inputs are open and what profiles they have.
    fn set_devices(&mut self, settings: &MidiHardwareSettings, ports: &[MidiPort]) {
        let profiles = self.profiles();
        self.device_names.clear();
        self.profile_links.clear();
        self.feedback.clear();
        self.last_feedback.clear();
        for (id, profile) in settings.inputs() {
            let name = ports
                .iter()
                .find(|port| port.id == id)
                .map_or_else(|| id.to_owned(), |port| port.name.clone());
            if let Some(profile) =
                profile.and_then(|wanted| profiles.iter().find(|profile| profile.id == wanted))
            {
                self.profile_links.extend(profile.links(Some(&name)));
                self.feedback.extend(profile.feedback.iter().cloned());
            }
            self.device_names.push(name);
        }
        self.expression.clear();
        self.note_sources.clear();
        self.input_filter = settings.input_channel;
        self.follow_clock = settings.follow_clock;
        self.stamp = None;
    }

    /// Brings what depends on the document up to date: a new document
    /// forgets everything that named the old one, and an edit refreshes the
    /// sources the MIDI worker matches against.
    fn sync(&mut self, generation: u64, revision: u64, project: &Project) {
        if self.generation != generation {
            self.generation = generation;
            self.learn = None;
            self.states.clear();
            self.waiting.clear();
            self.last_tweaked = None;
            self.expression.clear();
            self.note_sources.clear();
            self.notes = NoteTake::default();
            self.automation = AutomationTake::default();
            self.recording_gesture = None;
            self.automation_destination = None;
            self.log.clear();
            self.log_dirty = true;
            self.bank = 0;
            self.last_feedback.clear();
            self.stamp = None;
            self.queue.clear();
        }
        if self.stamp != Some((generation, revision)) {
            if self.script_bindings != project.settings.scripts.controllers {
                self.states.clear();
                self.waiting.clear();
                self.script_bindings = project.settings.scripts.controllers.clone();
            }
            let identity = serde_json::to_string(&(
                project.settings.name.as_str(),
                project
                    .channels
                    .iter()
                    .map(|channel| (channel.id, channel.name.as_str()))
                    .collect::<Vec<_>>(),
            ))
            .unwrap_or_default();
            if self
                .saved_log
                .as_ref()
                .is_some_and(|(saved, _)| *saved == identity)
            {
                self.log = self.saved_log.take().unwrap().1;
                self.log_dirty = false;
            }
            self.log_identity = identity;
            self.stamp = Some((generation, revision));
            self.sources.clear();
            let links = project
                .settings
                .controller_links
                .iter()
                .chain(&self.settings.links)
                .chain(&self.profile_links);
            for link in links {
                if !self.sources.contains(&link.source) {
                    self.sources.push(link.source.clone());
                }
            }
            for binding in &project.settings.scripts.controllers {
                if !self.sources.contains(&binding.source) {
                    self.sources.push(binding.source.clone());
                }
            }
        }
    }

    /// The id that groups a run of controller movements into one undo step.
    fn gesture_at(&mut self, now: Instant) -> u64 {
        let id = match self.gesture {
            Some((id, last)) if now.saturating_duration_since(last) <= GESTURE_GAP => id,
            _ => {
                self.gestures += 1;
                GESTURE_BASE + self.gestures
            }
        };
        self.gesture = Some((id, now));
        id
    }

    fn fresh_gesture(&mut self) -> u64 {
        self.gestures += 1;
        GESTURE_BASE + self.gestures
    }

    fn script_failed(&mut self, error: String) {
        let bindings = &self.script_bindings;
        self.states.retain(|(source, _), _| !bindings.iter().any(|binding| binding.source == *source));
        self.waiting.clear();
        self.error = Some(error);
    }

    fn push(&mut self, incoming: Incoming) {
        if self.queue.len() >= QUEUE {
            self.queue.pop_front();
            self.dropped = self.dropped.saturating_add(1);
        }
        self.queue.push_back(incoming);
    }
}

/// True when a link from `link` takes a message that came as `from`.
pub(super) fn source_matches(link: &ControlSource, from: &ControlSource) -> bool {
    match (link, from) {
        (
            ControlSource::Midi {
                device,
                channel,
                control,
            },
            ControlSource::Midi {
                device: from_device,
                channel: from_channel,
                control: from_control,
            },
        ) => {
            control == from_control
                && (channel.is_none() || channel == from_channel)
                && (device.is_none() || device == from_device)
        }
        _ => false,
    }
}

/// Reads the user's controller profiles: one JSON file each. A file that is
/// malformed, or takes the id of another profile, is skipped and logged.
fn load_profiles(dir: &Path) -> Vec<ControllerProfile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();
    let mut profiles: Vec<ControllerProfile> = Vec::new();
    let taken = builtin_profiles();
    for file in files.into_iter().take(MAX_USER_PROFILES) {
        let read = std::fs::read_to_string(&file)
            .map_err(|error| error.to_string())
            .and_then(|json| {
                serde_json::from_str::<ControllerProfile>(&json).map_err(|error| error.to_string())
            })
            .and_then(|profile| match profile.problem() {
                Some(problem) => Err(problem),
                None => Ok(profile),
            });
        match read {
            Ok(profile)
                if taken
                    .iter()
                    .chain(&profiles)
                    .all(|other| other.id != profile.id) =>
            {
                profiles.push(profile);
            }
            Ok(profile) => log::warn!(
                "controller profile {}: the id {} is already taken",
                file.display(),
                profile.id
            ),
            Err(error) => log::warn!("controller profile {}: {error}", file.display()),
        }
    }
    profiles
}

/// What the pump worked out under the locks, to carry out once they are
/// released.
#[derive(Default)]
pub(super) struct Plan {
    pub(super) generation: u64,
    pub(super) revision: u64,
    pub(super) script_events: Vec<(ControlSource, SourceInput, u8)>,
    pub(super) script_bank: u32,
    script_overflow: bool,
    /// The targets to move, each with the value it had, the value it gets
    /// (both 0 to 1) and the link that moves it.
    writes: Vec<Write>,
    commands: Vec<Command>,
    actions: Vec<ControlAction>,
    learned: Vec<(ControllerLink, bool, u64)>,
    clock: Vec<ClockInEvent>,
    stopped: bool,
    gesture: u64,
}

pub(super) type ScriptMotion = (ControlSource, LinkTarget, ControlMap, SourceInput);

struct Write {
    target: AutomationTarget,
    before: f32,
    value: f32,
    link: (ControlSource, LinkTarget),
}

impl Session {
    /// Starts the thread that carries out what controllers ask for. It stops
    /// when the last handle to the session is dropped.
    pub fn spawn_controllers(&self) -> io::Result<JoinHandle<()>> {
        let session = self.downgrade();
        thread::Builder::new()
            .name("windfall-controllers".to_owned())
            .spawn(move || {
                loop {
                    let Some(session) = session.upgrade() else {
                        break;
                    };
                    session.controllers_wait();
                    session.controllers_pump(Instant::now());
                }
            })
    }

    /// Sleeps until a controller message arrives or the wait runs out.
    fn controllers_wait(&self) {
        let hub = lock(&self.inner.controllers);
        let busy = !hub.queue.is_empty()
            || hub.stopped
            || !hub.clock_events.is_empty()
            || hub.states.values().any(LinkState::gliding);
        if !hub.queue.is_empty() || hub.stopped {
            return;
        }
        let wait = if busy { BUSY_WAIT } else { IDLE_WAIT };
        let _ = self
            .inner
            .controllers_wake
            .wait_timeout(hub, wait)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }

    /// Reads the devices the MIDI settings name into the hub.
    pub(super) fn controllers_devices(&self, settings: &MidiHardwareSettings, ports: &[MidiPort]) {
        lock(&self.inner.controllers).set_devices(settings, ports);
    }

    /// The tempo of the clock an input is sending, while one is.
    pub(super) fn external_bpm(&self) -> Option<f32> {
        lock(&self.inner.controllers)
            .clock_in
            .bpm()
            .map(|bpm| bpm as f32)
    }

    /// Every accepted hardware message, on the MIDI worker. True takes the
    /// message away from note audition.
    pub(super) fn controller_message(&self, input: InputMessage) -> bool {
        let mut hub = lock(&self.inner.controllers);
        if let Some((channel, parameter, value)) =
            super::capture_expression::movement(input.message)
            && hub.input_filter.is_none_or(|filter| filter == channel + 1)
        {
            let source = (input.device, channel);
            super::capture_expression::set(
                hub.expression.entry(source).or_default(),
                parameter,
                value,
            );
            let offset = f64::from(hub.settings.record_offset_ms) / 1000.0;
            if let Some(place) = hub.clock.position_at(input.at, offset) {
                hub.notes.expression(source, parameter, place.tick, value);
            }
        }
        if let MidiMessage::NoteOn { channel, key, .. } = input.message {
            hub.note_sources.insert(key, (input.device, channel));
        }
        let (channel, control, value, learnable) = match input.message {
            MidiMessage::Clock | MidiMessage::Start | MidiMessage::Continue | MidiMessage::Stop => {
                if let Some(event) = hub.clock_in.message(input.message, input.at)
                    && hub.follow_clock
                    && hub.clock_events.len() < 32
                {
                    hub.clock_events.push_back(event);
                    self.inner.controllers_wake.notify_one();
                }
                return false;
            }
            MidiMessage::ProgramChange { .. } => return false,
            MidiMessage::NoteOn { channel, key, .. } => (
                channel,
                MidiControl::Note { key },
                SourceInput::button(true),
                true,
            ),
            MidiMessage::NoteOff { channel, key } => (
                channel,
                MidiControl::Note { key },
                SourceInput::button(false),
                false,
            ),
            MidiMessage::Controller {
                channel,
                number,
                value,
            } => (
                channel,
                MidiControl::Cc { number },
                SourceInput::seven_bit(value),
                true,
            ),
            MidiMessage::PitchBend { channel, value } => (
                channel,
                MidiControl::PitchBend,
                SourceInput::fourteen_bit(value),
                true,
            ),
            MidiMessage::ChannelPressure { channel, value } => (
                channel,
                MidiControl::ChannelPressure,
                SourceInput::seven_bit(value),
                true,
            ),
        };
        let source = ControlSource::Midi {
            device: hub.device_names.get(usize::from(input.device)).cloned(),
            channel: Some(channel + 1),
            control,
        };
        let is_note = matches!(control, MidiControl::Note { .. });
        let linked = hub.sources.iter().any(|link| source_matches(link, &source));
        // While links are being made, a key that goes down is a candidate
        // and its release goes with it. Controls move nothing meanwhile.
        let learning = hub.learn.is_some();
        if !linked && !learning {
            if !is_note {
                hub.last_input = Some(source);
            }
            return false;
        }
        hub.push(Incoming {
            source,
            input: value,
            learnable,
        });
        self.inner.controllers_wake.notify_one();
        true
    }

    /// A note that sounds: from hardware, the typing keyboard or the touch
    /// controllers. `velocity` is `None` when the key comes up. The note is
    /// logged always and recorded while note recording is armed and
    /// playback runs.
    pub(super) fn controller_played(
        &self,
        channel: ChannelId,
        key: u8,
        velocity: Option<f32>,
        at: Instant,
    ) {
        self.controller_played_from(channel, key, velocity, at, None);
    }

    fn controller_played_from(
        &self,
        channel: ChannelId,
        key: u8,
        velocity: Option<f32>,
        at: Instant,
        source: Option<(u8, u8)>,
    ) {
        let mut hub = lock(&self.inner.controllers);
        hub.saved_log = None;
        let offset = f64::from(hub.settings.record_offset_ms) / 1000.0;
        let position = hub.clock.position_at(at, offset);
        match velocity {
            Some(velocity) => {
                hub.log
                    .note_on(channel, key, velocity, at, position.map(|place| place.tick));
                if let Some(place) = position.filter(|_| hub.note_armed) {
                    let initial = source
                        .and_then(|source| hub.expression.get(&source).copied())
                        .unwrap_or_default();
                    hub.notes.note_on_expressed(
                        channel,
                        key,
                        velocity,
                        place,
                        super::capture_expression::ExpressionCapture {
                            initial,
                            source,
                            samples: Vec::new(),
                        },
                    );
                }
            }
            None => {
                hub.log.note_off(channel, key, at);
                hub.log_dirty = true;
                if let Some(place) = position {
                    hub.notes.note_off(channel, key, place);
                }
            }
        }
    }

    /// A hardware note for the audition destination, with its capture time.
    pub(super) fn hardware_note_at(&self, epoch: u64, key: u8, velocity: u8, at: Instant) -> bool {
        let channel = self.state().midi_target;
        let delivered = self.hardware_note(epoch, key, velocity);
        if delivered && let Some(channel) = channel {
            let velocity = (velocity > 0).then(|| f32::from(velocity) / 127.0);
            let source = lock(&self.inner.controllers)
                .note_sources
                .get(&key)
                .copied();
            self.controller_played_from(channel, key, velocity, at, source);
        }
        delivered
    }

    /// Once a turn of the MIDI worker: follows the playhead, and wakes the
    /// pump when playback stops or a glide has somewhere to go.
    pub(super) fn controller_idle(&self, now: Instant, playhead: &Playhead) {
        let mut hub = lock(&self.inner.controllers);
        if let Some(place) = hub.clock.position_at(now, 0.0) {
            hub.last_tick = place.tick;
        }
        let mut capture_head = *playhead;
        if self.controller().count_in_remaining() > 0 {
            capture_head.playing = false;
        }
        if hub.clock.observe(&capture_head, now) {
            let end = hub.last_tick;
            hub.notes.close_all(end);
            hub.stopped = true;
            self.inner.controllers_wake.notify_one();
        }
        hub.clock_in.idle(now);
        hub.log.prune(now);
    }

    /// Carries out everything controllers have asked for since the last
    /// call. Never call it from the MIDI worker or the audio thread.
    pub fn controllers_pump(&self, now: Instant) {
        let mut plan = self.controllers_plan(now);
        let ordinary_actions = plan.actions.len();
        let learned = !plan.learned.is_empty();
        for (link, global, gesture) in std::mem::take(&mut plan.learned) {
            self.controller_link_add(link, global, gesture);
        }
        if learned {
            // So the very next message from a newly linked control is taken.
            let state = self.state();
            lock(&self.inner.controllers).sync(
                state.generation,
                state.document.revision(),
                state.document.project(),
            );
        }
        let feedback = if plan.script_overflow {
            plan.commands.clear();
            plan.actions.truncate(ordinary_actions);
            lock(&self.inner.controllers).script_failed("controller script event budget exceeded; batch refused".into());
            Vec::new()
        } else if !plan.script_events.is_empty() {
            match self.controller_scripts(&mut plan, now) {
                Ok(feedback) => feedback,
                Err(error) => {
                    plan.commands.clear();
                    plan.actions.truncate(ordinary_actions);
                    lock(&self.inner.controllers).script_failed(error);
                    Vec::new()
                }
            }
        } else { Vec::new() };
        let mut committed = plan.commands.is_empty();
        let mut applied_revision = plan.revision;
        if !plan.commands.is_empty() {
            // The batch binds revision and generation under the document lock.
            let command = if plan.commands.len() == 1 {
                plan.commands.remove(0)
            } else {
                Command::Batch {
                    label: None,
                    commands: std::mem::take(&mut plan.commands),
                }
            };
            match self.dispatch_controller_batch(command, Some(plan.gesture), plan.generation, Some(plan.revision)) {
                Ok(result) => { self.controllers_wrote(&plan, now); committed = true; applied_revision = result.patch.revision; },
                Err(error) => lock(&self.inner.controllers).error = Some(error),
            }
        }
        if committed && let Err(error) = self.controller_script_feedback(feedback, plan.generation, applied_revision) {
            lock(&self.inner.controllers).error = Some(format!("Controller movement applied; MIDI feedback failed: {error}"));
        }
        if !committed { plan.actions.truncate(ordinary_actions); }
        for action in std::mem::take(&mut plan.actions) {
            self.controller_action(action);
        }
        for event in std::mem::take(&mut plan.clock) {
            self.follow_clock(event);
        }
        if plan.stopped {
            self.commit_note_take();
            self.commit_automation_take();
        }
        self.surface_feedback();
        self.flush_score_log();
    }

    fn flush_score_log(&self) {
        let saved = {
            let mut hub = lock(&self.inner.controllers);
            if !hub.log_dirty || hub.saved_log.is_some() {
                return;
            }
            hub.log_dirty = false;
            (
                hub.log_file.clone(),
                hub.log.saved(hub.log_identity.clone(), Instant::now()),
            )
        };
        let outcome = saved.1.and_then(|bytes| {
            windfall_project::file::write_atomic(&saved.0, &bytes)
                .map_err(|error| error.to_string())
        });
        if let Err(error) = outcome {
            let mut hub = lock(&self.inner.controllers);
            hub.error = Some(format!("The note log could not be saved: {error}"));
            hub.log_dirty = true;
        }
    }

    fn surface_feedback(&self) {
        use windfall_project::controller_links::FeedbackSource;
        let transport = self.transport_state();
        let automated = self.controller().controller_automation_values();
        let messages = {
            let state = self.state();
            let project = state.document.project();
            let hub = lock(&self.inner.controllers);
            let messages: Vec<_> = hub
                .feedback
                .iter()
                .map(|feedback| {
                    let (value, text) = match feedback.source {
                        FeedbackSource::Slot { slot } => {
                            let value =
                                slot.resolve(project, hub.bank)
                                    .and_then(|target| {
                                        let audible = automated.iter().find_map(|(id, value)| {
                                            let matches = project.automations.iter().any(|lane| {
                                                lane.id.0 == *id && lane.target == target
                                            }) || project
                                                .patterns
                                                .iter()
                                                .flat_map(|pattern| &pattern.events)
                                                .any(|lane| {
                                                    lane.id.0 == *id && lane.target == target
                                                });
                                            matches.then_some(*value)
                                        });
                                        project
                                            .automation_range(&target)
                                            .zip(project.automation_stored_value(&target))
                                            .map(|(range, stored)| {
                                                audible.unwrap_or_else(|| range.normalized(stored))
                                            })
                                    })
                                    .unwrap_or(0.0);
                            (value, "")
                        }
                        FeedbackSource::Playing => (f32::from(u8::from(transport.playing)), ""),
                        FeedbackSource::Stopped => (f32::from(u8::from(!transport.playing)), ""),
                        FeedbackSource::NoteRecordArmed => {
                            (f32::from(u8::from(hub.note_armed)), "")
                        }
                        FeedbackSource::AutomationRecordArmed => {
                            (f32::from(u8::from(hub.automation_armed)), "")
                        }
                        FeedbackSource::MixerName { index } => (
                            0.0,
                            project
                                .mixer
                                .tracks
                                .iter()
                                .filter(|track| {
                                    track.id != windfall_project::TrackId::MASTER && !track.current
                                })
                                .nth((index + hub.bank) as usize)
                                .map_or("", |track| track.name.as_str()),
                        ),
                    };
                    feedback.encode(value, text)
                })
                .filter(|bytes| !bytes.is_empty())
                .collect();
            if messages == hub.last_feedback {
                return;
            }
            messages
        };
        if messages.is_empty() {
            return;
        }
        if let Ok(runtime) = self.midi_runtime() {
            let outcome = messages
                .chunks(256)
                .try_for_each(|chunk| runtime.surface_feedback(chunk.to_vec()));
            match outcome {
                Ok(()) => lock(&self.inner.controllers).last_feedback = messages,
                Err(error) => lock(&self.inner.controllers).error = Some(error),
            }
        }
    }

    /// The first half of a pump: under the document lock and the hub's,
    /// works out what the queued messages and running glides move.
    fn controllers_plan(&self, now: Instant) -> Plan {
        let state = self.state();
        let project = state.document.project();
        let mut guard = lock(&self.inner.controllers);
        let hub = &mut *guard;
        hub.sync(state.generation, state.document.revision(), project);
        let mut plan = Plan {
            generation: state.generation,
            revision: state.document.revision(),
            script_bank: hub.bank,
            stopped: std::mem::take(&mut hub.stopped),
            clock: hub.clock_events.drain(..).collect(),
            ..Plan::default()
        };
        let elapsed = hub.last_pump.map_or(0.0, |last| {
            now.saturating_duration_since(last).as_secs_f32()
        });
        hub.last_pump = Some(now);
        let pickup = hub.settings.pickup;
        let bank = hub.bank;
        let mut links: Vec<ControllerLink> = project
            .settings
            .controller_links
            .iter()
            .chain(&hub.settings.links)
            .chain(&hub.profile_links)
            .cloned()
            .collect();
        // Script outputs enter these same states after interpretation. Include
        // them here so glides and pickup indicators continue on idle pumps.
        for binding in &project.settings.scripts.controllers {
            for target in binding.target.into_iter().chain(std::iter::once(LinkTarget::Param { target: AutomationTarget::Tempo })) {
                let link = ControllerLink { source: binding.source.clone(), target, map: binding.map.clone() };
                links.retain(|old| old.source != link.source || old.target != link.target);
                links.push(link);
            }
        }
        hub.waiting.clear();
        let events: Vec<Incoming> = hub.queue.drain(..).collect();
        if !events.is_empty() {
            hub.error = None;
        }
        // What a target is to be given, so later messages in the same turn
        // carry on from it instead of from the stored value.
        let resolve = |target: &LinkTarget| -> Option<AutomationTarget> {
            match target {
                LinkTarget::Param { target } => Some(*target),
                LinkTarget::Slot { slot } => slot.resolve(project, bank),
                LinkTarget::Action { .. } => None,
            }
        };
        let current = |writes: &[Write], target: &AutomationTarget| -> Option<f32> {
            if let Some(write) = writes.iter().rev().find(|write| write.target == *target) {
                return Some(write.value);
            }
            let range = project.automation_range(target)?;
            Some(range.normalized(project.automation_stored_value(target)?))
        };
        let put = |writes: &mut Vec<Write>,
                   target: AutomationTarget,
                   value: f32,
                   link: &ControllerLink| {
            let Some(stored) = project
                .automation_range(&target)
                .zip(project.automation_stored_value(&target))
                .map(|(range, value)| range.normalized(value))
            else {
                return;
            };
            let key = (link.source.clone(), link.target);
            match writes.iter_mut().find(|write| write.target == target) {
                Some(write) => {
                    write.value = value;
                    write.link = key;
                }
                None => writes.push(Write {
                    target,
                    before: stored,
                    value,
                    link: key,
                }),
            }
        };

        for event in events {
            hub.last_input = Some(event.source.clone());
            if let Some(learn) = &mut hub.learn {
                let free = learn.linked < learn.targets.len();
                if event.learnable && free && !learn.seen.contains(&event.source) {
                    let target = learn.targets[learn.linked];
                    learn.linked += 1;
                    learn.seen.push(event.source.clone());
                    plan.learned.push((
                        ControllerLink {
                            source: event.source.clone(),
                            target,
                            map: ControlMap::default(),
                        },
                        learn.global,
                        learn.gesture,
                    ));
                    if !learn.multi {
                        hub.learn = None;
                    }
                }
                continue;
            }
            if project
                .settings
                .scripts
                .controllers
                .iter()
                .any(|binding| source_matches(&binding.source, &event.source))
            {
                if plan.script_events.len() < 32 {
                    let number = match &event.source {
                        ControlSource::Midi {
                            control: MidiControl::Cc { number } | MidiControl::Note { key: number },
                            ..
                        } => *number,
                        _ => 0,
                    };
                    plan.script_events
                        .push((event.source.clone(), event.input, number));
                } else {
                    plan.script_overflow = true;
                    hub.dropped = hub.dropped.saturating_add(1);
                    hub.error = Some("controller script event budget exceeded".into());
                }
                continue;
            }
            for link in links
                .iter()
                .filter(|link| source_matches(&link.source, &event.source))
            {
                let key = (link.source.clone(), link.target);
                let link_state = hub.states.entry(key).or_default();
                if let LinkTarget::Action { action } = link.target {
                    if link_state.pressed(event.input) {
                        plan.actions.push(action);
                    }
                    continue;
                }
                let Some(target) = resolve(&link.target) else {
                    continue;
                };
                let (Some(value), Ok(map)) = (current(&plan.writes, &target), link.map.compile())
                else {
                    continue;
                };
                match link_state.input(&map, event.input, value, pickup) {
                    Motion::Write(value) => put(&mut plan.writes, target, value, link),
                    Motion::Held => {
                        if !hub.waiting.contains(&target) {
                            hub.waiting.push(target);
                        }
                    }
                    Motion::None => {}
                }
            }
        }
        // Glides move on by the time since the last turn.
        if elapsed > 0.0 {
            for link in &links {
                let key = (link.source.clone(), link.target);
                let Some(link_state) = hub.states.get_mut(&key).filter(|held| held.gliding())
                else {
                    continue;
                };
                let (Some(target), Ok(map)) = (resolve(&link.target), link.map.compile()) else {
                    link_state.reset();
                    continue;
                };
                if let Motion::Write(value) = link_state.advance(&map, elapsed) {
                    put(&mut plan.writes, target, value, link);
                }
            }
        }
        for write in &plan.writes {
            let Some(range) = project.automation_range(&write.target) else {
                continue;
            };
            if let Some(command) = target_command(project, &write.target, range.value(write.value))
            {
                plan.commands.push(command);
            }
        }
        // Held pickup state persists across idle polls; only crossing/removal releases it.
        hub.waiting = links
            .iter()
            .filter_map(|link| {
                let key = (link.source.clone(), link.target);
                hub.states
                    .get(&key)
                    .filter(|state| state.waiting())
                    .and_then(|_| resolve(&link.target))
            })
            .collect();
        if !plan.commands.is_empty() {
            plan.gesture = if hub.automation_armed && hub.clock.playing() {
                if hub.recording_gesture.is_none() {
                    hub.recording_gesture = Some(hub.fresh_gesture());
                }
                hub.recording_gesture.unwrap()
            } else {
                hub.gesture_at(now)
            };
        }
        plan
    }

    /// After a controller edit landed: tells each link what its target now
    /// holds, and records the movement when automation recording is on.
    fn controllers_wrote(&self, plan: &Plan, now: Instant) {
        let state = self.state();
        if state.generation != plan.generation {
            return;
        }
        let project = state.document.project();
        let mut guard = lock(&self.inner.controllers);
        let hub = &mut *guard;
        let place = hub
            .clock
            .position_at(now, 0.0)
            .filter(|_| hub.automation_armed);
        for write in &plan.writes {
            let Some(stored) = project
                .automation_range(&write.target)
                .zip(project.automation_stored_value(&write.target))
                .map(|(range, value)| range.normalized(value))
            else {
                continue;
            };
            if let Some(link_state) = hub.states.get_mut(&write.link) {
                link_state.wrote(stored);
            }
            if let Some(place) = place {
                hub.record_movement(project, place, write.target, write.before, stored);
            }
        }
        drop(guard);
        drop(state);
        self.recording_touch_refresh();
    }

    pub(super) fn controllers_script_motion(
        &self,
        plan: &mut Plan,
        motions: Vec<ScriptMotion>,
        now: Instant,
    ) -> Result<(), String> {
        let state = self.state();
        if state.generation != plan.generation || state.document.revision() != plan.revision {
            return Err("controller script output became stale".into());
        }
        let project = state.document.project();
        let mut guard = lock(&self.inner.controllers);
        let hub = &mut *guard;
        let mut states = hub.states.clone();
        for (source, target, map, input) in motions {
            let compiled = map.compile()?;
            let link_state = states.entry((source.clone(), target)).or_default();
            if let LinkTarget::Action { action } = target {
                if link_state.pressed(SourceInput { value: compiled.shape(input.value), raw: input.raw }) {
                    plan.actions.push(action);
                }
                continue;
            }
            let resolved = match target {
                LinkTarget::Param { target } => Some(target),
                LinkTarget::Slot { slot } => slot.resolve(project, plan.script_bank),
                LinkTarget::Action { .. } => None,
            }.ok_or("controller script target is unavailable")?;
            let range = project.automation_range(&resolved).ok_or("controller script target is unavailable")?;
            let before = range.normalized(project.automation_stored_value(&resolved).ok_or("controller script target is unavailable")?);
            let value = plan.writes.iter().rev().find(|w| w.target == resolved).map_or(before, |w| w.value);
            match link_state.input(&compiled, input, value, hub.settings.pickup) {
                Motion::Write(value) => {
                    if let Some(write) = plan.writes.iter_mut().find(|w| w.target == resolved) {
                        write.value = value;
                        write.link = (source, target);
                    } else {
                        plan.writes.push(Write { target: resolved, before, value, link: (source, target) });
                    }
                }
                Motion::Held => { if !hub.waiting.contains(&resolved) { hub.waiting.push(resolved); } },
                Motion::None => {},
            }
        }
        plan.commands.clear();
        for write in &plan.writes {
            let range = project.automation_range(&write.target).ok_or("controller script target is unavailable")?;
            plan.commands.push(target_command(project, &write.target, range.value(write.value))
                .ok_or("controller script target has no checked command")?);
        }
        if plan.commands.len() > 128 || plan.actions.len() > 128 { return Err("controller output budget exceeded".into()); }
        hub.states = states;
        if !plan.commands.is_empty() {
            plan.gesture = if hub.automation_armed && hub.clock.playing() {
                if hub.recording_gesture.is_none() { hub.recording_gesture = Some(hub.fresh_gesture()); }
                hub.recording_gesture.unwrap()
            } else { hub.gesture_at(now) };
        }
        Ok(())
    }

    /// Adds one link made by a pass of linking: to the project as an edit,
    /// or to the app's settings.
    fn controller_link_add(&self, link: ControllerLink, global: bool, gesture: u64) {
        let outcome = if global {
            let mut settings = lock(&self.inner.controllers).settings.clone();
            settings
                .links
                .retain(|other| other.target != link.target && other.source != link.source);
            settings.links.push(link);
            self.controller_settings_set(settings).map(|_| ())
        } else {
            let mut links = self
                .state()
                .document
                .project()
                .settings
                .controller_links
                .clone();
            // Linking a control again replaces the link it had.
            links.retain(|other| other.target != link.target);
            links.push(link);
            self.dispatch_quiet(Command::SetControllerLinks { links }, Some(gesture))
                .map(|_| ())
        };
        if let Err(error) = outcome {
            let mut hub = lock(&self.inner.controllers);
            hub.error = Some(error);
            hub.learn = None;
        }
    }

    fn controller_action(&self, action: ControlAction) {
        let outcome: Result<(), String> = match action {
            ControlAction::Play => self.transport_play().map(|_| ()),
            ControlAction::Stop => {
                self.transport_stop();
                Ok(())
            }
            ControlAction::TogglePlay => self.transport_toggle().map(|_| ()),
            ControlAction::ToggleLoop => {
                let looping = self.transport_state().loop_song;
                self.transport_set(TransportPatch {
                    loop_song: Some(!looping),
                    ..TransportPatch::default()
                })
                .map(|_| ())
            }
            ControlAction::ToggleMetronome => {
                let mut metronome = self.transport_state().metronome.unwrap_or_default();
                metronome.enabled = !metronome.enabled;
                self.transport_set(TransportPatch {
                    metronome: Some(metronome),
                    ..TransportPatch::default()
                })
                .map(|_| ())
            }
            ControlAction::ToggleNoteRecord => {
                let armed = lock(&self.inner.controllers).note_armed;
                self.note_record_arm(!armed);
                Ok(())
            }
            ControlAction::ToggleAutomationRecord => {
                let armed = lock(&self.inner.controllers).automation_armed;
                self.automation_record_arm(!armed);
                Ok(())
            }
            ControlAction::Undo => {
                self.undo();
                Ok(())
            }
            ControlAction::Redo => {
                self.redo();
                Ok(())
            }
            ControlAction::PatternPrevious | ControlAction::PatternNext => {
                let patterns: Vec<PatternId> = {
                    let state = self.state();
                    let patterns = &state.document.project().patterns;
                    patterns.iter().map(|pattern| pattern.id).collect()
                };
                let now = self.transport_state().pattern;
                let at = patterns.iter().position(|id| *id == now).unwrap_or(0);
                let next = if action == ControlAction::PatternNext {
                    (at + 1).min(patterns.len().saturating_sub(1))
                } else {
                    at.saturating_sub(1)
                };
                match patterns.get(next) {
                    Some(pattern) => self
                        .transport_set(TransportPatch {
                            pattern: Some(*pattern),
                            ..TransportPatch::default()
                        })
                        .map(|_| ()),
                    None => Ok(()),
                }
            }
            ControlAction::BankPrevious | ControlAction::BankNext => {
                let mut hub = lock(&self.inner.controllers);
                hub.bank = if action == ControlAction::BankNext {
                    (hub.bank + SLOT_BANK_SIZE).min(MAX_BANK)
                } else {
                    hub.bank.saturating_sub(SLOT_BANK_SIZE)
                };
                Ok(())
            }
            ControlAction::PerformancePad { track, scene } => {
                self.performance_controller_pad(track, Some(scene))
            }
            ControlAction::PerformanceStop { track } => {
                self.performance_controller_pad(track, None)
            }
            ControlAction::PerformanceStopAll => {
                self.performance_stop_all();
                Ok(())
            }
            ControlAction::PanicMidi => {
                self.performance_stop_all();
                self.midi_hardware_panic();
                Ok(())
            }
        };
        if let Err(error) = outcome {
            lock(&self.inner.controllers).error = Some(error);
        }
    }

    /// Does what the clock of an input asks, when following is switched on.
    fn follow_clock(&self, event: ClockInEvent) {
        let outcome = match event {
            ClockInEvent::Start => {
                self.transport_seek(0.0);
                self.transport_play().map(|_| ())
            }
            ClockInEvent::Continue => self.transport_play().map(|_| ()),
            ClockInEvent::Stop => {
                self.transport_stop();
                Ok(())
            }
            ClockInEvent::Tempo(bpm) => {
                let tempo = self.state().document.project().settings.tempo_bpm;
                if (tempo - bpm).abs() < 0.05 {
                    Ok(())
                } else {
                    let patch = SettingsPatch {
                        tempo_bpm: Some(bpm),
                        ..SettingsPatch::default()
                    };
                    self.dispatch_quiet(Command::UpdateSettings { patch }, Some(CLOCK_GESTURE))
                        .map(|_| ())
                }
            }
        };
        if let Err(error) = outcome {
            lock(&self.inner.controllers).error = Some(error);
        }
    }

    /// The targets a command is about to set, each with the value it has
    /// now, 0 to 1. Call before the command runs.
    pub(super) fn controller_tweak_probe(&self, command: &Command) -> Vec<(AutomationTarget, f32)> {
        let state = self.state();
        let project = state.document.project();
        command_targets(project, command)
            .into_iter()
            .filter_map(|target| {
                let range = project.automation_range(&target)?;
                let value = project.automation_stored_value(&target)?;
                Some((target, range.normalized(value)))
            })
            .collect()
    }

    /// After a command from the UI set those targets: remembers the last
    /// one, adds them to a multi-link pass, and records the movement when
    /// automation recording is on.
    pub(super) fn controller_tweaked(&self, tweaks: &[(AutomationTarget, f32)]) {
        self.controller_tweaked_guarded(tweaks, None);
    }

    pub(super) fn controller_tweaked_guarded(
        &self,
        tweaks: &[(AutomationTarget, f32)],
        generation: Option<u64>,
    ) {
        let Some((last, _)) = tweaks.last() else {
            return;
        };
        let now = Instant::now();
        let state = self.state();
        if generation.is_some_and(|expected| state.generation != expected) {
            return;
        }
        let project = state.document.project();
        let mut guard = lock(&self.inner.controllers);
        let hub = &mut *guard;
        hub.last_tweaked = Some(*last);
        if let Some(learn) = hub
            .learn
            .as_mut()
            .filter(|learn| learn.multi && !learn.global)
        {
            for (target, _) in tweaks {
                let target = LinkTarget::Param { target: *target };
                if !learn.targets.contains(&target) && learn.targets.len() < MAX_LEARN_TARGETS {
                    learn.targets.push(target);
                }
            }
        }
        let place = hub
            .clock
            .position_at(now, 0.0)
            .filter(|_| hub.automation_armed);
        if let Some(place) = place {
            for (target, before) in tweaks {
                let stored = project
                    .automation_range(target)
                    .zip(project.automation_stored_value(target))
                    .map(|(range, value)| range.normalized(value));
                if let Some(stored) = stored.filter(|stored| stored != before) {
                    hub.record_movement(project, place, *target, *before, stored);
                }
            }
        }
        drop(guard);
        drop(state);
        self.recording_touch_refresh();
    }

    /// Recording movements, including their curve commit, share one pass history entry.
    pub(super) fn recording_edit_gesture(&self, has_tweaks: bool) -> Option<u64> {
        let mut hub = lock(&self.inner.controllers);
        if !has_tweaks || !hub.automation_armed || !hub.clock.playing() {
            return None;
        }
        if hub.recording_gesture.is_none() {
            hub.recording_gesture = Some(hub.fresh_gesture());
        }
        hub.recording_gesture
    }

    fn recording_touch_refresh(&self) {
        let targets = {
            let hub = lock(&self.inner.controllers);
            if !hub.automation_armed || !hub.clock.playing() {
                return;
            }
            hub.automation.recorded_targets()
        };
        let values: Vec<_> = {
            let state = self.state();
            targets
                .into_iter()
                .filter_map(|target| {
                    state
                        .document
                        .project()
                        .automation_stored_value(&target)
                        .map(|value| (target, value))
                })
                .collect()
        };
        for (target, value) in values {
            self.controller().automation_touch(target, Some(value));
        }
    }

    pub(super) fn controllers_note_armed(&self) -> bool {
        lock(&self.inner.controllers).note_armed
    }

    pub fn controller_state(&self) -> ControllerState {
        let generation = self.state().generation;
        let hub = lock(&self.inner.controllers);
        let now = Instant::now();
        ControllerState {
            settings: hub.settings.clone(),
            profiles: hub.profiles(),
            learn: hub.learn.as_ref().map(|learn| ControllerLearn {
                targets: learn.targets.clone(),
                linked: learn.linked as u32,
                global: learn.global,
                multi: learn.multi,
            }),
            last_input: hub.last_input.clone(),
            last_tweaked: hub.last_tweaked,
            waiting: hub.waiting.clone(),
            bank: hub.bank,
            note_record: NoteRecordState {
                armed: hub.note_armed,
                pending: hub.notes.pending(),
                dropped: hub.notes.dropped(),
            },
            automation_record: AutomationRecordState {
                armed: hub.automation_armed,
                targets: hub.automation.targets(),
                pending: hub.automation.pending(),
            },
            score_log: hub.log.state(now),
            dropped: hub.dropped,
            error: hub.error.clone(),
            generation,
        }
    }

    /// Replaces what the app remembers about controllers and saves it.
    pub fn controller_settings_set(
        &self,
        settings: ControllerSettings,
    ) -> Result<ControllerState, String> {
        settings.validate()?;
        {
            let mut hub = lock(&self.inner.controllers);
            hub.settings = settings.clone();
            hub.stamp = None;
            hub.states.clear();
        }
        self.store().update(|store| store.controllers = settings);
        Ok(self.controller_state())
    }

    /// Starts a pass of linking: the next hardware controls that move are
    /// given `targets`, in order. With `multi`, controls moved on screen are
    /// added to the pass as they are moved, and the pass runs until it is
    /// cancelled. With `global`, the links go to the app's settings.
    pub fn controller_learn(
        &self,
        targets: Vec<LinkTarget>,
        global: bool,
        multi: bool,
        generation: u64,
    ) -> Result<ControllerState, String> {
        if targets.len() > MAX_LEARN_TARGETS {
            return Err(format!(
                "At most {MAX_LEARN_TARGETS} controls can be linked in one pass."
            ));
        }
        if targets.is_empty() && !multi {
            return Err("Choose a control to link.".into());
        }
        {
            let state = self.state();
            if state.generation != generation {
                return Err("Project changed. Try linking again.".into());
            }
            let project = state.document.project();
            let existing = if global {
                lock(&self.inner.controllers).settings.links.len()
            } else {
                project.settings.controller_links.len()
            };
            if existing + targets.len() > MAX_CONTROLLER_LINKS {
                return Err(format!(
                    "There can be at most {MAX_CONTROLLER_LINKS} controller links."
                ));
            }
            // A link with any source will do to check the targets.
            let probe: Vec<ControllerLink> = targets
                .iter()
                .enumerate()
                .map(|(index, target)| ControllerLink {
                    source: ControlSource::Midi {
                        device: None,
                        channel: None,
                        control: MidiControl::Cc {
                            number: index as u8,
                        },
                    },
                    target: *target,
                    map: ControlMap::default(),
                })
                .collect();
            if let Some(problem) = links_problem(&probe, (!global).then_some(project)) {
                return Err(problem);
            }
            let mut hub = lock(&self.inner.controllers);
            hub.sync(state.generation, state.document.revision(), project);
            let gesture = hub.fresh_gesture();
            hub.queue.clear();
            hub.learn = Some(Learn {
                targets,
                linked: 0,
                global,
                multi,
                gesture,
                seen: Vec::new(),
            });
        }
        Ok(self.controller_state())
    }

    pub fn controller_learn_cancel(&self) -> ControllerState {
        lock(&self.inner.controllers).learn = None;
        self.controller_state()
    }

    /// Arms or disarms note recording. Disarming writes the pass so far.
    pub fn note_record_arm(&self, armed: bool) -> ControllerState {
        let was = std::mem::replace(&mut lock(&self.inner.controllers).note_armed, armed);
        if was && !armed {
            {
                let mut hub = lock(&self.inner.controllers);
                let end = hub.last_tick;
                hub.notes.close_all(end);
            }
            self.commit_note_take();
        }
        self.controller_state()
    }

    /// Arms or disarms automation recording. Disarming writes the pass so far.
    pub fn automation_record_arm(&self, armed: bool) -> ControllerState {
        let was = std::mem::replace(&mut lock(&self.inner.controllers).automation_armed, armed);
        if was && !armed {
            self.commit_automation_take();
        }
        self.controller_state()
    }

    /// Writes the notes of the pass into their patterns, as one undo step.
    fn commit_note_take(&self) {
        let (notes, quantize, generation) = {
            let mut hub = lock(&self.inner.controllers);
            (
                hub.notes.take(),
                hub.settings.record_quantize_ticks,
                hub.generation,
            )
        };
        if notes.is_empty() {
            return;
        }
        let command = {
            let state = self.state();
            if state.generation != generation {
                return;
            }
            recorded_notes_command(state.document.project(), &notes, quantize)
        };
        let Some(command) = command else {
            return;
        };
        if let Err(error) = self.dispatch_controller_capture(command, None, generation) {
            let current = self.state().generation;
            let mut hub = lock(&self.inner.controllers);
            if current == generation && hub.generation == generation {
                hub.notes.restore(notes);
            }
            hub.error = Some(error);
        }
    }

    /// Writes the movements of the pass as automation, as one undo step.
    fn commit_automation_take(&self) {
        let (lanes, gesture, destination, generation) = {
            let mut hub = lock(&self.inner.controllers);
            (
                hub.automation.take(),
                hub.recording_gesture.take(),
                hub.automation_destination.take(),
                hub.generation,
            )
        };
        if lanes.is_empty() {
            return;
        }
        let command = {
            let state = self.state();
            if state.generation != generation {
                return;
            }
            destination.as_ref().map_or_else(
                || {
                    Ok(recorded_automation_command(
                        state.document.project(),
                        &lanes,
                    ))
                },
                |destination| destination.command(state.document.project(), &lanes),
            )
        };
        for lane in &lanes {
            self.controller().automation_touch(lane.target, None);
        }
        let result = command.and_then(|command| match command {
            Some(command) => self
                .dispatch_controller_capture(command, gesture, generation)
                .map(|_| ()),
            None => Ok(()),
        });
        if let Err(error) = result {
            let current = self.state().generation;
            let mut hub = lock(&self.inner.controllers);
            if current == generation && hub.generation == generation {
                hub.automation.restore(lanes);
                hub.recording_gesture = gesture;
                hub.automation_destination = destination;
            }
            hub.error = Some(error);
        }
    }

    /// Puts what the rolling log holds into a pattern, as one undo step. The
    /// log keeps its notes, so undoing the dump loses nothing. `seconds`
    /// limits the dump to notes that began that recently.
    pub fn score_log_dump(
        &self,
        pattern: PatternId,
        seconds: Option<f32>,
        generation: u64,
    ) -> Result<DispatchResult, String> {
        let command = {
            let state = self.state();
            if state.generation != generation {
                return Err("Project changed. Try again.".into());
            }
            let project = state.document.project();
            let Some(found) = project.pattern(pattern) else {
                return Err("The pattern no longer exists.".into());
            };
            let signature = found
                .time_signature
                .unwrap_or(project.settings.time_signature);
            let window = seconds
                .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
                .map(Duration::from_secs_f32);
            let hub = lock(&self.inner.controllers);
            let dump = hub.log.dump(
                Instant::now(),
                window,
                project.settings.tempo_bpm,
                signature.ticks_per_bar(),
                hub.settings.record_quantize_ticks,
            );
            let commands: Vec<Command> = dump
                .lanes
                .into_iter()
                .filter(|(channel, notes)| project.channel(*channel).is_some() && !notes.is_empty())
                .map(|(channel, notes)| Command::AddNotes {
                    pattern,
                    channel,
                    notes,
                })
                .collect();
            if commands.is_empty() {
                return Err("Nothing has been played to put into the pattern.".into());
            }
            Command::Batch {
                label: Some("Dump note log".to_owned()),
                commands,
            }
        };
        self.dispatch_controller_capture(command, None, generation)
    }

    pub fn score_log_clear(&self) -> ControllerState {
        {
            let mut hub = lock(&self.inner.controllers);
            hub.log.clear();
            hub.saved_log = None;
            hub.log_dirty = true;
        }
        self.flush_score_log();
        self.controller_state()
    }
}

/// The command that writes a pass of recorded notes. Notes played in
/// pattern mode go where they were played in the pattern. Notes played in
/// song mode go into the pattern that was selected, through the clip of it
/// under the playhead; where no clip of it was playing, one is added on a
/// new playlist track to hold them. `None` when nothing can be written.
pub(super) fn recorded_notes_command(
    project: &Project,
    notes: &[TakenNote],
    quantize: u32,
) -> Option<Command> {
    let pattern_ticks = |id: PatternId| {
        project
            .pattern(id)
            .map(|pattern| pattern.length_steps.max(1).saturating_mul(TICKS_PER_STEP))
    };
    // Notes per pattern and channel, in the order they are first met.
    let mut lanes: Vec<(
        PatternId,
        ChannelId,
        Vec<NoteInit>,
        Vec<windfall_project::NoteCurveInsert>,
    )> = Vec::new();
    let mut add = |pattern: PatternId, channel: ChannelId, init: NoteInit, taken: &TakenNote| {
        let index = lanes
            .iter()
            .position(|(p, c, _, _)| *p == pattern && *c == channel)
            .unwrap_or_else(|| {
                lanes.push((pattern, channel, Vec::new(), Vec::new()));
                lanes.len() - 1
            });
        let (_, _, notes, curves) = &mut lanes[index];
        curves.extend(
            taken
                .expression
                .curves(taken.start, taken.length, notes.len() as u32),
        );
        notes.push(init);
    };
    // Song-mode notes with no clip of their pattern under them, by pattern.
    let mut homeless: Vec<(PatternId, Vec<&TakenNote>)> = Vec::new();
    for note in notes {
        let Some(length) = pattern_ticks(note.pattern) else {
            continue;
        };
        if project.channel(note.channel).is_none() {
            continue;
        }
        if note.mode == PlayMode::Pattern {
            if let Some(init) = note_init(note, 0.0, quantize, Some(length)) {
                add(note.pattern, note.channel, init, note);
            }
            continue;
        }
        let clip = project.playlist.clips.iter().find(|clip| {
            clip.content
                == (ClipContent::Pattern {
                    pattern: note.pattern,
                })
                && f64::from(clip.start) <= note.start
                && note.start < f64::from(clip.start) + f64::from(clip.length)
        });
        match clip {
            Some(clip) => {
                // Into the pattern as the clip plays it: from its offset,
                // round the pattern's length.
                let local = (note.start - f64::from(clip.start) + f64::from(clip.offset))
                    .rem_euclid(f64::from(length));
                let placed = TakenNote {
                    start: local,
                    ..note.clone()
                };
                if let Some(init) = note_init(&placed, 0.0, quantize, Some(length)) {
                    add(note.pattern, note.channel, init, note);
                }
            }
            None => match homeless
                .iter_mut()
                .find(|(pattern, _)| *pattern == note.pattern)
            {
                Some((_, held)) => held.push(note),
                None => homeless.push((note.pattern, vec![note])),
            },
        }
    }
    let mut commands = Vec::new();
    // A batch cannot pass an id from one command to the next, so the ids
    // the new tracks will get are worked out first: each track is followed
    // by its clip.
    let mut next_id = project.next_id;
    let bar = f64::from(project.settings.time_signature.ticks_per_bar().max(1));
    for (pattern, held) in &homeless {
        let first = held
            .iter()
            .map(|note| note.start)
            .fold(f64::INFINITY, f64::min);
        let last = held
            .iter()
            .map(|note| note.start + note.length)
            .fold(0.0, f64::max);
        let start = (first / bar).floor() * bar;
        let length = (((last - start) / bar).ceil().max(1.0)) * bar;
        if start + length > f64::from(MAX_SONG_TICKS) {
            continue;
        }
        commands.push(Command::AddPlaylistTrack {
            name: None,
            index: None,
        });
        commands.push(Command::AddClips {
            clips: vec![ClipInit {
                track: PlaylistTrackId(next_id),
                start: start as u32,
                length: Some(length as u32),
                offset: None,
                muted: None,
                content: ClipContent::Pattern { pattern: *pattern },
            }],
        });
        next_id = next_id.saturating_add(2);
        // The clip loops its pattern, so a note further in than the pattern
        // is long belongs to a later time round.
        let looped = pattern_ticks(*pattern);
        for note in held {
            let local = note.start - start;
            let placed = TakenNote {
                start: looped.map_or(local, |length| local.rem_euclid(f64::from(length))),
                ..(*note).clone()
            };
            if let Some(init) = note_init(&placed, 0.0, quantize, looped) {
                add(*pattern, note.channel, init, note);
            }
        }
    }
    for (pattern, channel, notes, curves) in lanes {
        commands.push(Command::AddNotesWithCurves {
            pattern,
            channel,
            notes,
            curves,
        });
    }
    if !commands
        .iter()
        .any(|command| matches!(command, Command::AddNotesWithCurves { .. }))
    {
        return None;
    }
    Some(Command::Batch {
        label: Some("Record notes".to_owned()),
        commands,
    })
}

/// The command that writes a pass of recorded movements as automation. A
/// control that an automation clip under the playhead already drives has
/// the recorded stretch written into that curve, the rest of the curve
/// staying as it was. Any other control gets a new automation that starts
/// from the value it held, and a clip on a new playlist track from the
/// start of the song. `None` when nothing can be written.
pub(super) fn recorded_automation_command(
    project: &Project,
    lanes: &[AutomationLane],
) -> Option<Command> {
    let mut commands = Vec::new();
    // As in `Session::automate`: the ids a new automation, its track and
    // its clip will get, in that order.
    let mut next_id = project.next_id;
    let song_end = project
        .playlist
        .clips
        .iter()
        .map(|clip| clip.start.saturating_add(clip.length))
        .max()
        .unwrap_or(0);
    let bar = project.settings.time_signature.ticks_per_bar().max(1);
    for lane in lanes {
        if project.automation_range(&lane.target).is_none() {
            continue;
        }
        if let Some(pattern_id) = lane.pattern {
            if lane.target == AutomationTarget::Tempo {
                continue;
            }
            let Some(pattern) = project.pattern(pattern_id) else {
                continue;
            };
            let expected = pattern
                .events
                .iter()
                .find(|event| event.target == lane.target)
                .map(|event| event.points.clone())
                .unwrap_or_default();
            let recorded = recorded_points(&lane.samples, windfall_project::MAX_AUTOMATION_POINTS);
            let base = if expected.is_empty() {
                vec![AutomationPoint {
                    tick: 0,
                    value: lane.before,
                    curve: 0.0,
                    hold: false,
                }]
            } else {
                expected.clone()
            };
            let points = merge_points(&base, &recorded, !expected.is_empty());
            if points != expected {
                commands.push(Command::SetPatternEvents {
                    pattern: pattern_id,
                    target: lane.target,
                    expected,
                    points,
                });
            }
            continue;
        }
        let Some(&(first_tick, _)) = lane.samples.first() else {
            continue;
        };
        // The clip that was driving the control where the pass began.
        let driving = project.playlist.clips.iter().find_map(|clip| {
            let automation = project.automation(clip.content.automation()?)?;
            let covers = f64::from(clip.start) <= first_tick
                && first_tick < f64::from(clip.start) + f64::from(clip.length);
            (automation.target == lane.target && !clip.muted && covers)
                .then_some((clip, automation))
        });
        match driving {
            Some((clip, automation)) => {
                let end = f64::from(clip.start) + f64::from(clip.length);
                let local: Vec<(f64, f32)> = lane
                    .samples
                    .iter()
                    .filter(|(tick, _)| *tick >= f64::from(clip.start) && *tick < end)
                    .map(|(tick, value)| {
                        (
                            tick - f64::from(clip.start) + f64::from(clip.offset),
                            *value,
                        )
                    })
                    .collect();
                let recorded = recorded_points(&local, windfall_project::MAX_AUTOMATION_POINTS);
                let points = merge_points(&automation.points, &recorded, true);
                if points != automation.points {
                    commands.push(Command::SetAutomationPoints {
                        id: automation.id,
                        points,
                    });
                }
            }
            None => {
                let recorded =
                    recorded_points(&lane.samples, windfall_project::MAX_AUTOMATION_POINTS);
                let Some(last) = recorded.last() else {
                    continue;
                };
                let start = [AutomationPoint {
                    tick: 0,
                    value: lane.before,
                    curve: 0.0,
                    hold: false,
                }];
                let points = merge_points(&start, &recorded, false);
                let length = song_end
                    .max(last.tick.saturating_add(bar))
                    .max(bar * 4)
                    .min(MAX_SONG_TICKS);
                commands.push(Command::AddAutomation {
                    name: None,
                    target: lane.target,
                    points: Some(points),
                });
                commands.push(Command::AddPlaylistTrack {
                    name: None,
                    index: None,
                });
                commands.push(Command::AddClips {
                    clips: vec![ClipInit {
                        track: PlaylistTrackId(next_id.saturating_add(1)),
                        start: 0,
                        length: Some(length),
                        offset: None,
                        muted: None,
                        content: ClipContent::Automation {
                            automation: AutomationId(next_id),
                        },
                    }],
                });
                next_id = next_id.saturating_add(3);
            }
        }
    }
    if commands.is_empty() {
        return None;
    }
    Some(Command::Batch {
        label: Some("Record automation".to_owned()),
        commands,
    })
}
