//! The whole audio path with no device attached.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use rtrb::{Consumer, Producer};
use windfall_core::AudioBuffer;
use windfall_project::TrackId;

use crate::automation::{self, GRID, Look, Place};
use crate::channel_voice::{ArpeggiatorMode, NoteEvent, NoteEventKind, admit_note};
use crate::channel_voice_runtime::{Generator, Owner};
use crate::clips::ClipPlayer;
use crate::controller::Controller;
use crate::message::{GARBAGE_HEADROOM, Garbage, Message, retire};
use crate::mixer::{Frame, MAX_BLOCK, Mixer};
use crate::plan::Plan;
use crate::ramp::Ramp;
use crate::sequencer::{Cursor, Fire, Sequencer, Triggers};
use crate::shared::Shared;
use crate::state::{Fades, PlanState};
use crate::stems::Taps;
use crate::voice::{Note, Origin, Stretch, VoicePool};
use windfall_dsp::NoteInstanceId;

/// Turns the current plan into audio: sequencer, sampler voices,
/// instruments, effects and mixer.
///
/// The device callback, the offline renderer and the tests all drive the
/// same `process`, so an export is the same audio as playback.
///
/// `process` is safe to call from a realtime thread. It never allocates,
/// frees, locks or blocks: every buffer is allocated in [`Processor::new`],
/// and whatever arrives from the [`Controller`] owning heap memory is sent
/// back to it to be dropped.
pub struct Processor {
    spectrum: Option<crate::spectrum::SpectrumTap>,
    /// The selectable analyzer captures of a device stream. `None` offline.
    captures: Option<crate::capture::AudioCaptures>,
    sample_rate: u32,
    /// Frames processed so far. It is the engine's clock.
    frame: u64,
    plan: Arc<Plan>,
    /// The moving values that belong to `plan`. Always built for it.
    state: Box<PlanState>,
    sequencer: Sequencer,
    ignored_pause: Option<u32>,
    voices: VoicePool,
    /// The audio clips of the playlist that are sounding.
    clips: ClipPlayer,
    mixer: Mixer,
    triggers: Triggers,
    metronome: crate::metronome::Metronome,
    count_in: Option<crate::metronome::CountIn>,
    input_monitors: Box<[Box<crate::recording_monitor::MonitorReader>]>,
    disk_taps: Box<[crate::recording_disk::DiskWriter]>,
    /// The audio logger's end of its queues. `None` while nothing is logged,
    /// and always in an offline render.
    audio_logger: Option<Box<crate::audio_logger::LoggerTap>>,
    hardware_output: Option<crate::hardware_output::HardwareOutput>,
    /// Gain applied after the master track and its meter.
    output_gain: Ramp,
    fades: Fades,
    /// Sequence number of the last play or stop request handled.
    transport_sequence: u32,
    hardware_epoch: u64,
    /// Automated values were published the last time there were any to
    /// publish, so their going away has to be published once as well.
    reported: bool,
    /// Playback has begun or been moved since the automation was last
    /// looked at.
    landed: bool,
    /// The place in the song at which a song that has stopped keeps its
    /// automation, for as long as it is ringing out. See
    /// [`Processor::automate`].
    hold: Option<Place>,
    /// The offline renderer is what plays: nobody will use the transport
    /// again, so a hold lasts for good.
    hold_for_good: bool,
    /// The tracks a stem render listens to, each on its own. `None`
    /// whenever anything else runs the processor.
    taps: Option<Taps>,
    messages: Consumer<Message>,
    garbage: Producer<Garbage>,
    shared: Arc<Shared>,
}

impl Processor {
    /// Creates a processor running at `sample_rate` and the controller that
    /// drives it. The device layer does the same when it opens a stream;
    /// call this directly to run the engine without a device.
    pub fn new(sample_rate: u32) -> (Processor, Controller) {
        let controller = Controller::new();
        let processor = controller.attach(sample_rate);
        (processor, controller)
    }

    /// Gives `controller` a new processor running at `sample_rate`, as the
    /// device layer does when it reopens a stream, for running the engine
    /// with no device. The project, the transport settings and the playhead
    /// carry over; what belonged to the stream before, such as analyzer
    /// captures and their tickets, does not. Drop the controller's previous
    /// processor first: this panics while that one is still alive.
    pub fn reopen(controller: &Controller, sample_rate: u32) -> Processor {
        controller.attach(sample_rate)
    }

    /// A processor that starts out on `plan`, with `state` built for it at
    /// this sample rate, and with the output at `output_gain`. All of it is
    /// simply there from the first frame: nothing has sounded yet, so there
    /// is nothing to glide or fade in from.
    pub(crate) fn with_queues(
        sample_rate: u32,
        plan: Arc<Plan>,
        state: Box<PlanState>,
        output_gain: f32,
        messages: Consumer<Message>,
        garbage: Producer<Garbage>,
        shared: Arc<Shared>,
    ) -> Self {
        let sample_rate = sample_rate.max(1);
        Self {
            spectrum: None,
            captures: None,
            sample_rate,
            frame: 0,
            state,
            sequencer: Sequencer::new(sample_rate, &plan),
            ignored_pause: None,
            plan,
            voices: VoicePool::new(sample_rate),
            clips: ClipPlayer::new(sample_rate),
            mixer: Mixer::new(),
            triggers: Triggers::new(),
            metronome: crate::metronome::Metronome::new(),
            count_in: None,
            input_monitors: Vec::new().into_boxed_slice(),
            disk_taps: Vec::new().into_boxed_slice(),
            audio_logger: None,
            hardware_output: None,
            output_gain: Ramp::at_rest(output_gain),
            fades: Fades::at(sample_rate),
            transport_sequence: 0,
            hardware_epoch: shared.hardware_epoch.load(Ordering::Acquire),
            reported: false,
            landed: false,
            hold: None,
            hold_for_good: false,
            taps: None,
            messages,
            garbage,
            shared,
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Installed during off-audio attachment, before processing can begin.
    pub(crate) fn set_spectrum(&mut self, spectrum: crate::spectrum::SpectrumTap) {
        self.spectrum = Some(spectrum);
    }

    /// Installed during off-audio attachment, before processing can begin.
    pub(crate) fn set_captures(&mut self, captures: crate::capture::AudioCaptures) {
        self.captures = Some(captures);
    }

    pub(crate) fn output_frame(&self) -> u64 {
        self.frame
    }
    /// Configure off the callback, before the device starts processing.
    pub(crate) fn set_output_channels(&mut self, channels: usize) {
        self.hardware_output = Some(crate::hardware_output::HardwareOutput::new(channels));
    }
    pub(crate) fn hardware_samples(&self) -> &[f32] {
        self.hardware_output
            .as_ref()
            .map_or(&[], |output| output.samples())
    }

    /// Fills `out`, interleaved stereo, with the next `out.len() / 2` frames.
    ///
    /// Requests from the controller take effect at the start of the call.
    /// The audio does not depend on how the output is divided into calls,
    /// only on which frame each request arrives at.
    pub fn process(&mut self, out: &mut [f32]) {
        if let Some(captures) = &mut self.captures {
            captures.boundary();
        }
        self.plugin_control_boundary();
        self.voices.sweep(&self.plan, &mut self.garbage);
        self.clips.sweep(&self.plan, &mut self.garbage);
        self.handle_messages();
        if let Some(output) = &mut self.hardware_output {
            output.rewind();
        }
        if let Some(taps) = &mut self.taps {
            taps.rewind();
        }
        let (frames, stray) = out.as_chunks_mut::<2>();
        stray.fill(0.0);
        let mut rest = frames;
        let mut navigation_left = crate::timeline::MAX_NAVIGATION_TRANSITIONS;
        while !rest.is_empty() {
            self.finish_count_in();
            self.apply_navigation(&mut navigation_left);
            let most = self.next_block(rest.len());
            let most = self.count_in.as_ref().map_or(most, |count| {
                most.min(count.end().saturating_sub(self.frame).max(1) as usize)
            });
            let next = self.navigation_boundary().map_or(most, |(frame, _, _)| {
                most.min(frame.saturating_sub(self.frame).max(1) as usize)
            });
            let (block, later) = rest.split_at_mut(next);
            self.process_block(block);
            rest = later;
        }
        self.finish_count_in();
        self.apply_navigation(&mut navigation_left);
        self.publish_automation();
        self.collect_midi_out();
        self.shared.publish_transport(
            self.transport_sequence,
            self.sequencer.playing() || self.count_in.is_some(),
        );
        // The playhead is where the sound now leaving the engine was in the
        // song, which is behind what is being worked out by the latency of
        // the instruments and effects.
        self.shared.publish_position(
            self.sequencer
                .tick_heard(&self.plan, self.frame, self.state.latency),
            self.sequencer.start(),
            self.voices.active() + self.state.instrument_voices(),
        );
        let missing = self.clips.missing(self.sequencer.clock(), self.frame);
        self.shared
            .publish_clips(self.clips.playing() as u32, missing as u32);
    }

    /// Has every `process` from now on copy what leaves each of these
    /// tracks, past its fader, to where [`Processor::taps`] shows it. A
    /// call may then ask for no more frames than the taps have room for.
    pub(crate) fn listen(&mut self, taps: Taps) {
        self.taps = Some(taps);
    }

    /// What the last `process` copied out of the tracks listened to.
    pub(crate) fn taps(&self) -> Option<&Taps> {
        self.taps.as_ref()
    }

    /// Where a track is in the plan, and by how many frames what leaves it
    /// lags the notes that make it.
    pub(crate) fn track_place(&self, id: TrackId) -> Option<(usize, usize)> {
        let index = self.plan.track_ids.get(id.0)?;
        Some((index, self.state.behind[index]))
    }

    /// Audio clips that were to start and did not, because as many as can
    /// play at once were playing already. The offline renderer reports it.
    pub(crate) fn clips_left_out(&self) -> u32 {
        self.clips.refused()
    }

    /// True when nothing is sounding and nothing can sound again until a
    /// note starts: every voice and audio clip has ended, every instrument
    /// has finished, and every effect and delay has had silence coming in
    /// for as long as it can hold on to sound. The offline renderer asks
    /// this to know when a tail is over.
    pub(crate) fn settled(&self) -> bool {
        self.voices.active() == 0
            && self.clips.active() == 0
            && self.state.settled(&self.plan, self.frame)
    }

    fn handle_messages(&mut self) {
        self.sync_hardware_epoch();
        // Handling a message can retire values, and those must have
        // somewhere to go.
        let mut handled = false;
        while self.garbage.slots() >= GARBAGE_HEADROOM {
            let Ok(message) = self.messages.pop() else {
                break;
            };
            self.handle(message);
            handled = true;
        }
        // The transport may have started, stopped or moved, and a new plan
        // may have other curves: the automation is looked at right away,
        // not at the next step of its grid.
        if handled {
            self.automate();
        }
    }

    fn sync_hardware_epoch(&mut self) {
        let epoch = self.shared.hardware_epoch.load(Ordering::Acquire);
        if epoch != self.hardware_epoch {
            self.cut_echoes_of(Origin::Hardware);
            self.voices.fade_origin(Origin::Hardware);
            for unit in self.state.instrument_units() {
                unit.silence_hardware();
            }
            self.hardware_epoch = epoch;
        }
    }

    /// How many frames to process next, out of `left` to do. While
    /// automation is at work this is up to the next step of its grid, and
    /// the automation is looked at when a step has come. The grid is
    /// counted in frames of the engine's clock, so where it falls does not
    /// depend on how the output is divided into calls.
    fn next_block(&mut self, left: usize) -> usize {
        let most = left.min(MAX_BLOCK);
        let at_work = self.state.engaged > 0 || self.state.returning > 0;
        let in_pattern = self.plan.pattern_lanes
            && self.sequencer.playing()
            && self.sequencer.mode() == windfall_ipc::PlayMode::Pattern;
        let automating =
            !self.plan.lanes.is_empty() && (self.sequencer.in_song() || in_pattern || at_work);
        if !automating {
            return most;
        }
        let into = self.frame % GRID;
        if into == 0 {
            self.automate();
        }
        most.min((GRID - into) as usize)
    }

    /// Looks at the automation: sets every automated target on its way to
    /// the value it is to have at the next step of the grid, and tells the
    /// effects and instruments the tempo the song has there.
    ///
    /// While the song plays, that is the value of the place in the song.
    /// A song that has stopped, by itself at its end or because it was
    /// told to, leaves its targets where they were for as long as anything
    /// is still sounding: a tail must not be heard through a fader that has
    /// gone back to its stored value. The hold ends when everything has
    /// rung out, or sooner when the transport is used or something is
    /// played by hand, and the targets then return to their stored values.
    fn automate(&mut self) {
        if self.plan.lanes.is_empty() {
            self.hold = None;
            return;
        }
        let now = self.frame;
        let playing = self.sequencer.playing();
        if self.hold.is_some() && (playing || (!self.hold_for_good && self.settled())) {
            self.hold = None;
        }
        let landed = std::mem::take(&mut self.landed);
        let until = (now / GRID + 1) * GRID;
        let ahead = self.place_at(until);
        let look = Look {
            now,
            until,
            tick: ahead.or(self.hold),
            landed: self.place_at(now).filter(|_| landed),
            // With the song playing on, what starts next must find the
            // target where it belongs. With the transport stopped there is
            // only what still rings to hear the change.
            back: if playing {
                self.fades.ramp
            } else {
                self.fades.restore
            },
        };
        let tick = look.tick;
        automation::step(&self.plan, &mut self.state, look);
        if self.state.engaged == 0 {
            self.hold = None;
        }
        let tempo = match (tick, &self.plan.tempo_map) {
            (Some(Place::Song(tick)), Some(map)) => map.tempo_at(tick),
            _ => self.plan.tempo_bpm,
        };
        self.state.tell_tempo(tempo);
    }

    /// The place automation is read at for frame `frame`: in the song while
    /// the playlist plays, in the pattern while a pattern plays by itself
    /// and some pattern has event lanes.
    fn place_at(&self, frame: u64) -> Option<Place> {
        if let Some(tick) = self.sequencer.song_tick_at(&self.plan, frame) {
            return Some(Place::Song(tick));
        }
        if !self.plan.pattern_lanes {
            return None;
        }
        self.sequencer
            .pattern_tick_at(&self.plan, frame)
            .map(|(index, tick)| Place::Pattern { index, tick })
    }

    /// Publishes the value of every automation that has its target in
    /// hand, for the UI to show the control moving.
    fn publish_automation(&mut self) {
        if self.state.engaged == 0 && !self.reported {
            return;
        }
        let lanes = self.state.lanes.iter().filter(|lane| lane.engaged);
        self.shared
            .publish_automated(lanes.map(|lane| (lane.automation, lane.value)));
        self.reported = self.state.engaged > 0;
    }

    fn handle(&mut self, message: Message) {
        if matches!(
            &message,
            Message::SetPlan { .. }
                | Message::Play { .. }
                | Message::Stop { .. }
                | Message::Seek(_)
                | Message::SetTransport { .. }
                | Message::CountIn { .. }
        ) && let Some(spectrum) = &mut self.spectrum
        {
            spectrum.reset();
        }
        if let Some(captures) = &mut self.captures
            && matches!(
                &message,
                Message::SetPlan { .. }
                    | Message::Play { .. }
                    | Message::Stop { .. }
                    | Message::Seek(_)
                    | Message::SetTransport { .. }
                    | Message::CountIn { .. }
            )
        {
            captures.reset();
        }
        // A stale hardware note must not even release a stopped song's
        // automation hold. Discard it before any transport/tail side effects.
        if let Message::HardwareNote { epoch, .. } = &message {
            self.sync_hardware_epoch();
            if *epoch != self.hardware_epoch {
                return;
            }
        }
        let now = self.frame;
        // Where the song is, in case this is what stops it.
        let song_at = self.sequencer.song_tick_at(&self.plan, now);
        // Using the transport, or playing something by hand, ends the wait
        // for a stopped song to ring out: what comes next is to be heard
        // at the stored values.
        let lets_go = match &message {
            Message::Play { .. }
            | Message::CountIn { .. }
            | Message::Stop { .. }
            | Message::Seek(_)
            | Message::NoteOn { .. }
            | Message::HardwareNote {
                velocity: 1..=127, ..
            }
            | Message::Preview(_) => true,
            Message::SetTransport { mode, .. } => *mode != self.sequencer.mode(),
            Message::SetPlan { .. }
            | Message::SetRegion(_)
            | Message::NoteOff { .. }
            | Message::HardwareNote { .. }
            | Message::StopPreview
            | Message::SetInputMonitors(_)
            | Message::SetDiskTaps(_)
            | Message::SetAudioLogger(_)
            | Message::SetOutputGain(_) => false,
        };
        if lets_go {
            self.hold = None;
        }
        match message {
            Message::CountIn {
                sequence,
                bars,
                capture,
            } => {
                self.sequencer.set_recording(capture.is_some());
                self.transport_sequence = sequence;
                self.hold_for_good = false;
                self.sequencer.stop();
                for channel in 0..self.state.channel_voice.len() {
                    self.reset_channel_voice(channel);
                }
                self.end_sequenced();
                let tick = self.sequencer.tick(&self.plan, now);
                let song = self.sequencer.mode() == windfall_ipc::PlayMode::Song;
                let (signature, meters) = if song {
                    (self.plan.signature, &self.plan.meters)
                } else {
                    self.sequencer
                        .pattern(&self.plan)
                        .map_or((self.plan.signature, &self.plan.meters), |pattern| {
                            (pattern.signature, &pattern.meters)
                        })
                };
                let signature = meters
                    .as_ref()
                    .ok()
                    .and_then(|meters| {
                        meters
                            .iter()
                            .rev()
                            .find(|segment| f64::from(segment.start_tick()) <= tick)
                    })
                    .map_or(signature, |segment| segment.signature());
                let tempo = if song {
                    self.plan
                        .tempo_map
                        .as_ref()
                        .map_or(self.plan.tempo_bpm, |map| map.tempo_at(tick))
                } else {
                    self.plan.tempo_bpm
                };
                let beats = u32::from(signature.numerator) * u32::from(bars);
                let frames_per_beat = f64::from(self.sample_rate) * 60.0 * 4.0
                    / (tempo * f64::from(signature.denominator));
                let count = crate::metronome::CountIn {
                    start: now,
                    frames_per_beat,
                    beats,
                    beats_per_bar: u32::from(signature.numerator),
                    emitted: 0,
                };
                if let Some(ticket) = capture {
                    self.shared
                        .recording_clock
                        .schedule(ticket, count.end().saturating_add(self.state.latency));
                }
                self.count_in = Some(count);
                self.shared
                    .count_in_remaining
                    .store(beats, Ordering::Release);
            }
            Message::SetPlan { plan, state } => self.adopt(plan, state),
            Message::Play {
                sequence,
                passes,
                from,
            } => {
                self.sequencer.set_recording(false);
                self.transport_sequence = sequence;
                self.cancel_count_in();
                self.hold_for_good = passes.is_some();
                self.sequencer.play(&self.plan, now, passes, from);
            }
            Message::Stop { sequence } => {
                self.sequencer.set_recording(false);
                self.cancel_count_in();
                self.transport_sequence = sequence;
                self.sequencer.stop();
                self.clips.release_all();
                // Stop silences every note, so no echo of one may follow it.
                for origin in [Origin::Sequenced, Origin::Live, Origin::Hardware] {
                    self.cut_echoes_of(origin);
                }
                self.voices.fade_origin(Origin::Sequenced);
                self.voices.fade_origin(Origin::Live);
                self.voices.fade_origin(Origin::Hardware);
                for unit in self.state.instrument_units() {
                    unit.silence();
                }
            }
            Message::Seek(tick) => {
                self.cancel_count_in();
                self.ignored_pause = None;
                if self.sequencer.seek(tick, &self.plan, now) {
                    self.end_sequenced();
                }
            }
            Message::SetRegion(region) => {
                if region != self.sequencer.region() {
                    self.ignored_pause = None;
                }
                if self.sequencer.set_region(region, &self.plan, now) {
                    self.end_sequenced();
                }
            }
            Message::SetTransport {
                mode,
                pattern,
                loop_song,
                metronome,
            } => {
                self.metronome.configure(metronome);
                self.sequencer.set_metronome(metronome.enabled);
                if self
                    .sequencer
                    .set_transport(mode, pattern, loop_song, &self.plan, now)
                {
                    self.end_sequenced();
                }
            }
            Message::NoteOn {
                channel,
                key,
                velocity,
            } => {
                if let Some(channel) = self.plan.channel(channel) {
                    let note = Note {
                        channel,
                        source: None,
                        key,
                        velocity,
                        pan: 0.0,
                        end: f64::INFINITY,
                        origin: Origin::Live,
                        expression: Default::default(),
                    };
                    self.start(note, now);
                }
            }
            Message::NoteOff { channel, key } => {
                if let Some(index) = self.plan.channel(channel) {
                    self.release_input(index, key, Origin::Live);
                }
            }
            Message::HardwareNote {
                epoch,
                channel,
                key,
                velocity,
            } => {
                self.sync_hardware_epoch();
                if epoch == self.hardware_epoch {
                    if velocity == 0 {
                        if let Some(index) = self.plan.channel(channel) {
                            self.release_input(index, key, Origin::Hardware);
                        }
                    } else if let Some(channel) = self.plan.channel(channel) {
                        self.start(
                            Note {
                                channel,
                                source: None,
                                key,
                                velocity: f32::from(velocity) / 127.0,
                                pan: 0.0,
                                end: f64::INFINITY,
                                origin: Origin::Hardware,
                                expression: Default::default(),
                            },
                            now,
                        );
                    }
                }
            }
            Message::Preview(sample) => self.preview(sample),
            Message::StopPreview => self.voices.fade_origin(Origin::Preview),
            Message::SetOutputGain(gain) => self.set_output_gain(gain),
            Message::SetInputMonitors(monitors) => {
                let old = std::mem::replace(&mut self.input_monitors, monitors);
                retire(&mut self.garbage, Garbage::InputMonitors(old));
            }
            Message::SetDiskTaps(taps) => {
                let old = std::mem::replace(&mut self.disk_taps, taps);
                retire(&mut self.garbage, Garbage::DiskTaps(old));
            }
            Message::SetAudioLogger(tap) => {
                if let Some(old) = std::mem::replace(&mut self.audio_logger, tap) {
                    retire(&mut self.garbage, Garbage::AudioLogger(old));
                }
            }
        }
        // Whatever moved the playhead moved the clock under the notes that
        // are still sounding.
        let shift = self.sequencer.take_clock_shift();
        if shift != 0.0 {
            self.voices.shift_ends(shift);
            for runtime in &mut self.state.channel_voice {
                runtime.move_ends(&|end| end + shift);
            }
            for unit in self.state.instrument_units() {
                unit.shift_ends(shift);
            }
        }
        if self.sequencer.take_jump() {
            self.landed = true;
            self.clips.release_all();
            self.chase_clips();
        }
        if let Some(tick) = song_at {
            self.hold_if_stopped(tick);
        }
    }

    /// Keeps the automation at `tick` of the song, where the song was a
    /// moment ago, if the transport has stopped since and a target is in
    /// automation's hands. [`Processor::automate`] ends the hold.
    fn hold_if_stopped(&mut self, tick: f64) {
        if !self.sequencer.playing() && self.state.engaged > 0 {
            self.hold = Some(Place::Song(tick));
        }
    }

    fn navigation_boundary(&self) -> Option<(u64, u32, crate::timeline::NavigationAction)> {
        use crate::timeline::NavigationAction;
        if !self.sequencer.in_song() {
            return None;
        }
        let region = self.sequencer.region();
        let mut next = region.map(|r| {
            (
                self.sequencer.song_frame(&self.plan, r.end).max(self.frame),
                r.end,
                if self.sequencer.song_looping() {
                    NavigationAction::Jump(r.start)
                } else {
                    NavigationAction::Pause
                },
            )
        });
        if self.sequencer.navigation_enabled()
            && self.shared.recording_clock.active_ticket().is_none()
        {
            for point in &self.plan.navigation {
                if self.ignored_pause == Some(point.tick)
                    || (point.looping && (region.is_some() || !self.sequencer.song_looping()))
                    || region.is_some_and(|r| point.tick < r.start || point.tick >= r.end)
                {
                    continue;
                }
                let Some(frame) = self
                    .sequencer
                    .navigation_frame(&self.plan, point.tick, self.frame)
                else {
                    continue;
                };
                if next.is_none_or(|(next_frame, _, _)| frame < next_frame) {
                    next = Some((frame, point.tick, point.action));
                }
            }
        }
        next
    }

    fn apply_navigation(&mut self, remaining: &mut u32) {
        use crate::timeline::NavigationAction;
        while let Some((frame, tick, action)) = self.navigation_boundary() {
            if frame > self.frame {
                break;
            }
            if *remaining == 0 {
                self.shared
                    .navigation_overflows
                    .fetch_add(1, Ordering::Relaxed);
                let tick = self.sequencer.raw_song_tick(&self.plan, self.frame);
                self.sequencer.pause_at(tick);
                self.end_sequenced();
                self.clips.release_all();
                self.hold_if_stopped(tick);
                break;
            }
            *remaining -= 1;
            self.end_sequenced();
            self.clips.release_all();
            match action {
                NavigationAction::Pause => {
                    self.ignored_pause = Some(tick);
                    self.sequencer.pause_at(f64::from(tick));
                    self.hold_if_stopped(f64::from(tick));
                }
                NavigationAction::Jump(destination) => {
                    self.ignored_pause = None;
                    let destination = self
                        .sequencer
                        .region()
                        .map_or(destination, |r| destination.min(r.end));
                    let recording_wrap = self
                        .sequencer
                        .region()
                        .is_some_and(|r| tick == r.end && destination == r.start)
                        && self.sequencer.wrap_recording_region(&self.plan);
                    if !recording_wrap {
                        self.sequencer
                            .seek(f64::from(destination), &self.plan, self.frame);
                    }
                    let shift = self.sequencer.take_clock_shift();
                    self.voices.shift_ends(shift);
                    for runtime in &mut self.state.channel_voice {
                        runtime.move_ends(&|end| end + shift);
                    }
                    for unit in self.state.instrument_units() {
                        unit.shift_ends(shift);
                    }
                    self.sequencer.take_jump();
                    self.chase_clips();
                    self.hold = None;
                    self.hold_if_stopped(f64::from(destination));
                    self.landed = true;
                }
            }
            // Restore automation immediately at a navigation destination,
            // using the existing 64-frame ramp contract and no new DSP API.
            self.automate();
        }
    }

    /// Starts the audio clips the playhead is inside of and that are not
    /// sounding, part way through: playback has begun or been moved there,
    /// or a new plan has put a clip there.
    fn chase_clips(&mut self) {
        if !self.sequencer.in_song() {
            return;
        }
        let now = self.frame;
        self.clips.chase(
            &self.plan,
            &mut self.garbage,
            self.sequencer.clock(),
            self.sequencer.pass_start(),
            self.sequencer.tick(&self.plan, now),
            now,
        );
    }

    /// Starts a note on frame `now`: on its channel's instrument if it has
    /// one, and as a sampler voice otherwise, including per-voice expression.
    fn start(&mut self, note: Note, now: u64) {
        // A layer is silent itself and plays the note on each channel it
        // resolves to. The plan holds that bounded list, so this is a fixed
        // amount of work and nested layers were followed on the control side.
        if let Some(targets) = self.plan.channels[note.channel].layer.as_ref() {
            if !self.plan.channels[note.channel].audible {
                return;
            }
            for index in 0..targets.len() {
                let Some(layer) = self.plan.channels[note.channel].layer.as_ref() else {
                    break;
                };
                let channel = layer[index];
                self.start_on(Note { channel, ..note }, now);
            }
            return;
        }
        self.start_on(note, now);
    }

    /// Starts a note on a channel that makes sound.
    fn start_on(&mut self, note: Note, now: u64) {
        let channel = note.channel;
        if self.state.channel_voice[channel].settings.arpeggiator.mode != ArpeggiatorMode::Off
            && note.expression.articulation != windfall_dsp::NoteArticulation::Slide
        {
            self.state.channel_voice[channel].chord_on(note);
            return;
        }
        if note.expression.articulation == windfall_dsp::NoteArticulation::Slide {
            // A slide bends sounding notes and has no echo of its own.
        } else if !note.end.is_finite() {
            // A note held by hand has no length yet: its echoes start now
            // and are let go when the key is.
            if matches!(note.origin, Origin::Live | Origin::Hardware) {
                let bpm = self.state.tempo();
                let rejected = self.state.channel_voice[channel].rejected_echoes;
                self.state.channel_voice[channel].schedule_held_echo(
                    note,
                    f64::from(self.sample_rate),
                    bpm,
                );
                self.report_rejected_echoes(channel, rejected);
            }
        } else {
            let duration = self
                .sequencer
                .clock()
                .frame_of(note.end)
                .saturating_sub(now)
                .max(1) as usize;
            let bpm = self.state.tempo();
            let rejected = self.state.channel_voice[channel].rejected_echoes;
            self.state.channel_voice[channel].schedule_echo(
                note,
                duration,
                f64::from(self.sample_rate),
                bpm,
            );
            self.report_rejected_echoes(channel, rejected);
        }
        self.start_output(note, now, None);
    }

    /// Ends the echoes of every note from `origin`: the ones sounding are
    /// released and the ones still queued are discarded.
    fn cut_echoes_of(&mut self, origin: Origin) {
        for channel in 0..self.state.channel_voice.len() {
            for slot in 0..self.state.channel_voice[channel].owners.len() {
                if let Some(owner) = self.state.channel_voice[channel].owners[slot]
                    && matches!(owner.generated, Some((Generator::Echo, _)))
                    && owner.note.origin == origin
                {
                    self.release_instance(channel, owner.instance);
                    if owner.admitted {
                        self.state.channel_voice[channel]
                            .held
                            .note_off(owner.note.key);
                    }
                    self.state.channel_voice[channel].owners[slot] = None;
                }
            }
            self.state.channel_voice[channel].cancel_echoes_of(origin);
        }
    }

    /// Publishes echo requests refused since `before` to the control side.
    fn report_rejected_echoes(&self, channel: usize, before: u64) {
        let refused = self.state.channel_voice[channel]
            .rejected_echoes
            .saturating_sub(before);
        if refused > 0 {
            self.shared
                .rejected_echoes
                .fetch_add(refused.min(u64::from(u32::MAX)) as u32, Ordering::Relaxed);
        }
    }

    fn release_instance(&mut self, channel: usize, id: NoteInstanceId) {
        if let Some(unit) = self.state.instrument(channel) {
            unit.release_instance(id);
        } else {
            self.voices
                .release_instance(self.plan.channels[channel].id, id);
        }
    }

    fn release_input(&mut self, channel: usize, key: u8, origin: Origin) {
        // A layer's keys were played on the channels it resolves to.
        if let Some(targets) = self.plan.channels[channel].layer.as_ref() {
            for index in 0..targets.len() {
                let Some(layer) = self.plan.channels[channel].layer.as_ref() else {
                    break;
                };
                let child = layer[index];
                self.release_input_on(child, key, origin);
            }
            return;
        }
        self.release_input_on(channel, key, origin);
    }

    fn release_input_on(&mut self, channel: usize, key: u8, origin: Origin) {
        self.state.channel_voice[channel].chord_off(key, origin);
        self.state.channel_voice[channel].release_held_echo(key, origin);
        for slot in 0..self.state.channel_voice[channel].owners.len() {
            if let Some(owner) = self.state.channel_voice[channel].owners[slot]
                && owner.generated.is_none()
                && owner.note.key == key
                && owner.note.origin == origin
            {
                if let Some(unit) = self.state.instrument(channel) {
                    unit.release_instance(owner.instance);
                } else {
                    self.voices
                        .key_up_instance(self.plan.channels[channel].id, owner.instance);
                }
                self.state.channel_voice[channel].owners[slot] = None;
                self.state.channel_voice[channel].held.note_off(key);
            }
        }
    }

    fn reset_channel_voice(&mut self, channel: usize) {
        for slot in 0..self.state.channel_voice[channel].owners.len() {
            if let Some(owner) = self.state.channel_voice[channel].owners[slot] {
                self.release_instance(channel, owner.instance);
            }
        }
        self.voices
            .reset_channel_modulation(self.plan.channels[channel].id);
        if let Some(unit) = self.state.instrument(channel) {
            unit.reset_channel_modulation();
        }
        self.state.channel_voice[channel].reset();
    }

    fn start_output(&mut self, mut note: Note, now: u64, generated: Option<(Generator, u64)>) {
        let clock = self.sequencer.clock();
        let at = clock.tick_at(now);
        if let Some(source) = note.source.filter(|source| source.active) {
            (note.pan, note.expression) = source.controls(&self.plan, at);
        }
        let glide_ticks = f64::from(note.expression.glide_ticks.clamp(1, 245760));
        let glide_end = if self.sequencer.in_song() {
            let pass = self.sequencer.pass_start();
            pass + self.plan.warp(self.plan.unwarp(at - pass) + glide_ticks)
        } else {
            at + glide_ticks
        };
        if note.expression.articulation == windfall_dsp::NoteArticulation::Slide {
            if let Some(unit) = self.state.instrument(note.channel) {
                unit.slide_notes(note.key, note.expression, at, note.end, note.origin);
            } else {
                self.voices.slide_notes(
                    self.plan.channels[note.channel].id,
                    note.key,
                    note.expression,
                    at,
                    note.end,
                    note.origin,
                );
            }
            return;
        }
        let channel = note.channel;
        self.state.channel_voice[channel].expire(clock, now);
        let settings = self.state.channel_voice[channel].settings.polyphony;
        // An echo sounds alongside the note it repeats, so it is not entered
        // in the held table, where a second note of the same key replaces
        // the first. A one-voice channel stays one voice: there an echo
        // takes the note's place like any other note.
        let alongside = matches!(generated, Some((Generator::Echo, _)))
            && !settings.mono_legato
            && settings.max_voices > 1;
        // The voice limit counts echoes too, and echoes give way first: the
        // oldest one is released for any new note, and a new echo is left
        // out when only played notes are sounding.
        let limit = usize::from(settings.max_voices);
        loop {
            let runtime = &self.state.channel_voice[channel];
            let sounding = runtime.owners.iter().flatten().count();
            if sounding < limit && sounding < runtime.owners.len() {
                break;
            }
            let Some(slot) = runtime.oldest_echo() else {
                break;
            };
            if let Some(owner) = self.state.channel_voice[channel].owners[slot].take() {
                self.release_instance(channel, owner.instance);
            }
        }
        if alongside
            && self.state.channel_voice[channel]
                .owners
                .iter()
                .flatten()
                .count()
                >= limit
        {
            return;
        }
        let decision = if alongside {
            crate::channel_voice::PolyphonyDecision::alongside()
        } else {
            admit_note(
                &mut self.state.channel_voice[channel].held,
                note.key,
                settings,
            )
        };
        let mono = if !decision.retrigger {
            self.state.channel_voice[channel]
                .owners
                .iter()
                .flatten()
                .next()
                .copied()
        } else {
            None
        };
        for &key in decision.released_keys() {
            for slot in 0..self.state.channel_voice[channel].owners.len() {
                if let Some(owner) = self.state.channel_voice[channel].owners[slot]
                    && owner.note.key == key
                    && owner.admitted
                {
                    if mono.is_none() {
                        self.release_instance(channel, owner.instance);
                    }
                    self.state.channel_voice[channel].owners[slot] = None;
                }
            }
        }
        let envelopes = self.state.channel_voice[channel].envelopes;
        let coefficient = self.state.channel_voice[channel].glide;
        let instance = if let Some(owner) = mono {
            let moved = if let Some(unit) = self.state.instrument(channel) {
                unit.retarget_instance(owner.instance, note, at, glide_end)
            } else {
                self.voices
                    .retarget_instance(&self.plan, owner.instance, note, at, glide_end)
            };
            moved.then_some(owner.instance)
        } else {
            None
        };
        let instance = instance.or_else(|| match self.state.instrument(channel) {
            Some(unit) => unit.start_note_source(
                note.key,
                note.velocity,
                note.pan,
                note.expression,
                note.end,
                note.origin,
                at,
                glide_end,
                note.source,
            ),
            None => self.voices.start(
                &self.plan,
                &mut self.garbage,
                note,
                now,
                clock,
                glide_end,
                envelopes,
                coefficient,
            ),
        });
        if let Some(instance) = instance {
            if let Some(slot) = self.state.channel_voice[channel]
                .owners
                .iter_mut()
                .find(|slot| slot.is_none())
            {
                *slot = Some(Owner {
                    note,
                    instance,
                    generated,
                    admitted: !alongside,
                });
            }
        } else if !alongside {
            self.state.channel_voice[channel].held.note_off(note.key);
        }
    }

    /// Ends sequenced notes and flushes the channel voice tools after a jump.
    fn end_sequenced(&mut self) {
        if let Some(spectrum) = &mut self.spectrum {
            spectrum.reset();
        }
        if let Some(captures) = &mut self.captures {
            captures.reset();
        }
        for channel in 0..self.state.channel_voice.len() {
            for slot in 0..self.state.channel_voice[channel].owners.len() {
                if let Some(owner) = self.state.channel_voice[channel].owners[slot]
                    && (owner.note.origin == Origin::Sequenced || owner.generated.is_some())
                {
                    self.release_instance(channel, owner.instance);
                    self.state.channel_voice[channel].owners[slot] = None;
                }
            }
            self.state.channel_voice[channel].end_sequenced();
        }
        self.voices.fade_origin(Origin::Sequenced);
        for unit in self.state.instrument_units() {
            unit.end_sequenced();
        }
    }

    /// Switches to a new plan between two blocks. Sounding voices carry on,
    /// effects and instruments move across with all they remember, and
    /// whatever sound is passing through changes gently. Voices already
    /// fading out step aside first where they can, so they hold no gain
    /// back from changing at once.
    fn adopt(&mut self, plan: Arc<Plan>, mut state: Box<PlanState>) {
        let now = self.frame;
        for channel in 0..self.plan.channels.len() {
            let old = &self.plan.channels[channel];
            if plan.channel(old.id).is_none_or(|index| {
                plan.channels[index].voice != old.voice
                    || plan.channels[index].instrument.map(|params| params.kind())
                        != old.instrument.map(|params| params.kind())
            }) {
                self.reset_channel_voice(channel);
            }
        }
        self.voices.rebind(&plan, &self.plan, &self.state, now);
        self.clips.rebind(&plan, now, self.fades.ramp);
        let sounding = self.voices.destinations();
        state.take_over(
            &plan,
            &self.plan,
            &mut self.state,
            sounding.chain(self.clips.destinations()),
            now,
            self.fades,
        );
        if self.sequencer.set_plan(&plan, &self.plan, now) {
            // The tempo map changed under the song: every clock tick now
            // means another place in it, the ends of sounding notes too.
            let origin = self.sequencer.pass_start();
            let moved = |end: f64| Sequencer::moved(&plan, &self.plan, origin, end);
            self.voices.move_ends(moved);
            for runtime in &mut state.channel_voice {
                runtime.move_ends(&moved);
            }
            for unit in state.instrument_units() {
                unit.move_ends(moved);
            }
        }
        let old_plan = std::mem::replace(&mut self.plan, plan);
        for unit in state.instrument_units() {
            unit.refresh_curve_sources(&self.plan);
        }
        let old_state = std::mem::replace(&mut self.state, state);
        retire(&mut self.garbage, Garbage::Plan(old_plan));
        retire(&mut self.garbage, Garbage::State(old_state));
        // A song that got shorter can have moved the playhead back into
        // it. Either way a clip may now lie under the playhead that was
        // not there, or not like this, a moment ago.
        if self.sequencer.take_jump() {
            self.landed = true;
            self.clips.release_all();
        }
        self.chase_clips();
    }

    fn preview(&mut self, sample: AudioBuffer) {
        let unplayed = self
            .voices
            .preview(&self.plan, &mut self.garbage, sample, self.frame);
        if let Some(sample) = unplayed {
            retire(&mut self.garbage, Garbage::Sample(sample));
        }
    }

    /// Glides the output to a new gain, or sets it outright while nothing
    /// is sounding and a glide would only reach into whatever starts next.
    fn set_output_gain(&mut self, gain: f32) {
        if self.settled() {
            self.output_gain = Ramp::at_rest(gain);
        } else {
            self.output_gain.retarget(gain, self.frame, self.fades.ramp);
        }
    }

    fn plugin_control_boundary(&mut self) {
        for chain in &mut self.state.chains {
            for unit in chain.iter_mut().flatten() {
                unit.plugin_control_boundary();
            }
        }
        for seat in self.state.instruments.iter_mut().flatten() {
            if let Some(unit) = &mut seat.unit {
                unit.plugin_control_boundary();
            }
        }
    }

    /// Processes up to [`MAX_BLOCK`] frames.
    /// Moves what the MIDI output channels made in this buffer to the queue
    /// the MIDI worker reads. Fixed storage on both sides; nothing is sent
    /// from here.
    fn collect_midi_out(&mut self) {
        let shared = &self.shared;
        for seat in self.state.instruments.iter_mut().flatten() {
            if let Some(unit) = &mut seat.unit {
                unit.drain_midi(&mut |event| {
                    shared.push_midi_out(event.frame, event.bytes, event.len);
                });
            }
        }
    }

    fn process_block(&mut self, out: &mut [Frame]) {
        // Failed preparations are refused on the control side. Keep this
        // internal invariant fail-closed without manufacturing native anchors.
        let in_song = self.sequencer.mode() == windfall_ipc::PlayMode::Song;
        let (base_signature, selected_meters) = if in_song {
            (self.plan.signature, &self.plan.meters)
        } else {
            self.sequencer
                .pattern(&self.plan)
                .map_or((self.plan.signature, &self.plan.meters), |pattern| {
                    (pattern.signature, &pattern.meters)
                })
        };
        let Ok(meters) = selected_meters else {
            out.fill([0.0; 2]);
            return;
        };
        let tick = self.sequencer.tick(&self.plan, self.frame);
        let warped = if in_song {
            self.plan
                .tempo_map
                .as_ref()
                .map_or(tick, |map| map.warp(tick))
        } else {
            tick
        };
        let segment = meters
            [..meters.partition_point(|segment| f64::from(segment.start_tick()) <= tick)]
            .last();
        let signature = segment.map_or(base_signature, |segment| segment.signature());
        let transport = crate::plugins::PluginTransport {
            playing: self.sequencer.playing(),
            tempo_bpm: self.state.tempo(),
            position_beats: tick / windfall_core::PPQ as f64,
            position_seconds: warped * 60.0 / (self.plan.tempo_bpm * windfall_core::PPQ as f64),
            numerator: signature.numerator as u16,
            denominator: signature.denominator as u16,
            meter_anchor: segment.map(|segment| crate::plugins::MeterAnchor {
                bar_origin_beats: f64::from(segment.start_tick()) / windfall_core::PPQ as f64,
                bar_origin_index: segment.bar_origin_index(),
            }),
        };
        self.plugin_control_boundary();
        for (index, chain) in self.state.chains.iter_mut().enumerate() {
            for (place, unit) in chain.iter_mut().enumerate() {
                if !self.plan.tracks[index].effects[place].leaving
                    && let Some(unit) = unit
                {
                    unit.plugin_transport(transport);
                }
            }
        }
        for (index, seat) in self.state.instruments.iter_mut().enumerate() {
            if !self.plan.channels[index].leaving
                && let Some(seat) = seat
                && let Some(unit) = &mut seat.unit
            {
                unit.plugin_transport(transport);
            }
        }
        let frames = out.len();
        let base = self.frame;
        let end = base + frames as u64;
        self.mixer.clear(self.plan.tracks.len(), frames);
        for monitor in &mut self.input_monitors {
            if let Some(track) = self.plan.track_ids.get(monitor.settings.track.0) {
                monitor.render(&mut self.mixer.track_mut(track)[..frames]);
            }
        }
        self.metronome.clear_block();

        // Voices and instruments are rendered up to each note's frame
        // before the note starts, so everything a note-on does, cutting
        // other voices included, happens on its exact frame. The notes
        // arrive in rounds
        // of as many as the trigger buffer holds, in playing order, so a
        // block with more notes than that plays every one of them just the
        // same.
        let mut rendered = 0;
        let mut cursor = Cursor::at(base);
        let in_song = self.sequencer.in_song();
        loop {
            let next = self
                .sequencer
                .collect(&self.plan, cursor, end, &mut self.triggers);
            for index in 0..self.triggers.len() {
                let trigger = self.triggers.get(index);
                let offset = (trigger.frame - base) as usize;
                self.render_voices(base, rendered, offset);
                rendered = offset;
                match trigger.what {
                    Fire::Click { accent } => self.metronome.trigger(accent, self.sample_rate),
                    Fire::Note {
                        source,
                        channel,
                        key,
                        velocity,
                        pan,
                        end,
                        expression,
                    } => {
                        let note = Note {
                            source: Some(source),
                            channel,
                            key,
                            velocity,
                            pan,
                            end,
                            origin: Origin::Sequenced,
                            expression,
                        };
                        self.start(note, trigger.frame);
                    }
                    Fire::Clip { clip, origin } => {
                        let garbage = &mut self.garbage;
                        self.clips
                            .start(&self.plan, garbage, clip, origin, 0.0, trigger.frame);
                    }
                }
            }
            let Some(next) = next else {
                break;
            };
            self.sequencer.advance(&self.plan, cursor.frame, next.frame);
            cursor = next;
        }
        self.sequencer.advance(&self.plan, cursor.frame, end);
        if in_song {
            // A song that ran out in this block left off at its end.
            self.hold_if_stopped(f64::from(self.plan.song_end));
        }
        self.render_voices(base, rendered, frames);

        self.mixer.mix(
            &self.plan,
            &mut self.state,
            &self.shared,
            base,
            out,
            crate::mixer::CaptureSinks {
                disk: &mut self.disk_taps,
                logger: &mut self.audio_logger,
            },
        );
        if let Some(captures) = &mut self.captures {
            captures.publish_tracks(
                &self.plan,
                &self.state,
                &self.mixer,
                base,
                self.sample_rate,
                frames,
            );
        }
        if let Some(taps) = &mut self.taps {
            taps.collect(&mut self.mixer, frames);
        }
        self.mixer
            .publish_waveforms(&self.plan, &self.shared, base, self.sample_rate, out);
        self.metronome.mix(out);
        // A capture of the mix output sees it after the gain and before the
        // scrub; the device gets the same samples either way.
        match &mut self.captures {
            Some(captures) if captures.wants_mix() => captures.finish_output(
                &self.output_gain,
                base,
                self.sample_rate,
                self.state.latency,
                out,
            ),
            _ => self.finish_output(base, out),
        }
        if let Some(logger) = &mut self.audio_logger {
            logger.master(out);
        }
        if let Some(spectrum) = &mut self.spectrum {
            spectrum.publish(
                out,
                base,
                self.sample_rate,
                self.state.latency,
                Arc::as_ptr(&self.plan) as usize,
            );
        }
        self.collect_hardware(base, out);
        self.frame = end;
    }

    fn collect_hardware(&mut self, base: u64, out: &[Frame]) {
        let Some(output) = &mut self.hardware_output else {
            return;
        };
        let first = output.position();
        let master =
            self.plan.tracks[0]
                .external_output
                .unwrap_or(windfall_project::ExternalOutputRoute {
                    left: 0,
                    right: (output.channels() > 1).then_some(1),
                    exclusive: false,
                });
        for (offset, frame) in out.iter().enumerate() {
            output.add(master, first + offset, *frame);
        }
        for (index, track) in self.plan.tracks.iter().enumerate().skip(1) {
            let Some(route) = track.external_output else {
                continue;
            };
            let frames = out.len();
            output.scratch[..frames].copy_from_slice(&self.mixer.track_mut(index)[..frames]);
            if let Some(line) = &mut self.state.external[index].line {
                line.process(&mut output.scratch[..frames]);
            }
            for offset in 0..frames {
                let mut frame = output.scratch[offset];
                let gain = self.output_gain.at(base + offset as u64);
                frame[0] *= gain;
                frame[1] *= gain;
                output.add(route, first + offset, frame);
            }
        }
        output.advance(out.len());
    }

    fn generated_event(
        &mut self,
        channel: usize,
        generator: Generator,
        event: NoteEvent,
        now: u64,
    ) {
        if event.kind == NoteEventKind::Off {
            for slot in 0..self.state.channel_voice[channel].owners.len() {
                if let Some(owner) = self.state.channel_voice[channel].owners[slot]
                    && owner.generated == Some((generator, event.id))
                {
                    self.release_instance(channel, owner.instance);
                    if owner.admitted {
                        self.state.channel_voice[channel]
                            .held
                            .note_off(owner.note.key);
                    }
                    self.state.channel_voice[channel].owners[slot] = None;
                }
            }
            if generator == Generator::Echo {
                self.state.channel_voice[channel].forget_echo(event.id);
            }
        } else if let Some(source) = self.state.channel_voice[channel].source(generator, event) {
            let note = Note {
                key: event.note.key,
                velocity: event.note.velocity,
                end: f64::INFINITY,
                ..source
            };
            self.start_output(note, now, Some((generator, event.id)));
        }
    }

    fn render_voices(&mut self, base: u64, from: usize, to: usize) {
        if self.state.channel_voice.iter().all(|runtime| {
            runtime.settings.arpeggiator.mode == ArpeggiatorMode::Off
                && runtime.echo.pending_events() == 0
        }) {
            self.render_voice_audio(base, from, to);
            return;
        }
        // One-frame subblocks make source chord changes and generated releases exact.
        // Both processor scratch batches were reserved by PlanState::try_build.
        for frame in from..to {
            let now = base + frame as u64;
            let clock = self.sequencer.clock();
            let bpm = self.state.tempo();
            for channel in 0..self.state.channel_voice.len() {
                let rejected = self.state.channel_voice[channel].rejected_echoes;
                let runtime = &mut self.state.channel_voice[channel];
                runtime.expire(clock, now);
                runtime.arp.process_block(
                    f64::from(self.sample_rate),
                    bpm,
                    1,
                    &mut runtime.arp_events,
                );
                for index in 0..runtime.arp_events.as_slice().len() {
                    let event = runtime.arp_events.as_slice()[index];
                    if event.kind == NoteEventKind::On
                        && let Some(source) = runtime.source(Generator::Arp, event)
                    {
                        let duration = runtime.arp.gate_frames();
                        runtime.schedule_echo(
                            Note {
                                key: event.note.key,
                                velocity: event.note.velocity,
                                ..source
                            },
                            duration,
                            f64::from(self.sample_rate),
                            bpm,
                        );
                    }
                }
                runtime.echo.process_block(1, &mut runtime.echo_events);
                self.report_rejected_echoes(channel, rejected);
                // Merge chronologically, off before on for the shared frame.
                for kind in [NoteEventKind::Off, NoteEventKind::On] {
                    for generator in [Generator::Arp, Generator::Echo] {
                        let len = match generator {
                            Generator::Arp => self.state.channel_voice[channel]
                                .arp_events
                                .as_slice()
                                .len(),
                            Generator::Echo => self.state.channel_voice[channel]
                                .echo_events
                                .as_slice()
                                .len(),
                        };
                        for index in 0..len {
                            let event = match generator {
                                Generator::Arp => {
                                    self.state.channel_voice[channel].arp_events.as_slice()[index]
                                }
                                Generator::Echo => {
                                    self.state.channel_voice[channel].echo_events.as_slice()[index]
                                }
                            };
                            if event.kind == kind {
                                self.generated_event(channel, generator, event, now);
                            }
                        }
                    }
                }
            }
            self.render_voice_audio(base, frame, frame + 1);
        }
    }

    fn render_voice_audio(&mut self, base: u64, from: usize, to: usize) {
        if from < to {
            if let Some(count) = &mut self.count_in {
                self.metronome.render_count_in(
                    count,
                    &self.shared,
                    base,
                    from,
                    to,
                    self.sample_rate,
                );
            } else {
                self.metronome.render(from, to);
            }
            let clock = self.sequencer.clock();
            self.voices.render(
                &self.plan,
                &self.state,
                clock,
                &mut self.garbage,
                &mut self.mixer,
                Stretch {
                    base,
                    frames: from..to,
                },
            );
            self.state
                .render_instrument_curves(&self.plan, clock, base, from..to);
            self.clips.render(
                &self.plan,
                clock,
                &mut self.garbage,
                &mut self.mixer,
                Stretch {
                    base,
                    frames: from..to,
                },
            );
        }
    }

    fn cancel_count_in(&mut self) {
        self.count_in = None;
        self.shared.count_in_remaining.store(0, Ordering::Release);
    }

    fn finish_count_in(&mut self) {
        if self
            .count_in
            .as_ref()
            .is_some_and(|count| self.frame >= count.end())
        {
            self.cancel_count_in();
            self.sequencer.play(&self.plan, self.frame, None, None);
        }
    }

    /// Applies the output gain and keeps anything that is not a number away
    /// from the device.
    fn finish_output(&self, base: u64, out: &mut [Frame]) {
        let steady = self.output_gain.settled(base);
        let mut gain = self.output_gain.at(base);
        for (offset, frame) in out.iter_mut().enumerate() {
            if !steady {
                gain = self.output_gain.at(base + offset as u64);
            }
            for sample in frame {
                *sample = if sample.is_finite() {
                    *sample * gain
                } else {
                    0.0
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windfall_project::ChannelId;

    #[test]
    fn stale_hardware_notes_preserve_stopped_automation_hold() {
        let (mut processor, controller) = Processor::new(48_000);
        processor.hold = Some(Place::Song(960.0));
        let epoch = controller.hardware_epoch();
        controller.panic_hardware();

        processor.handle(Message::HardwareNote {
            epoch,
            channel: ChannelId(123),
            key: 60,
            velocity: 127,
        });

        assert_eq!(processor.hold, Some(Place::Song(960.0)));
    }
}
