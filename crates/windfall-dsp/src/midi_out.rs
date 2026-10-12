//! A channel that plays external MIDI hardware instead of making sound.
//!
//! [`MidiOut`] is an instrument whose output is silence and whose notes and
//! settings become MIDI messages. It never performs IO. Messages collect in
//! a fixed buffer inside the instrument; the host stamps them with the frame
//! they belong to and takes them out after each block, and sends them from a
//! thread that may block. A host that never takes them, as an offline render
//! does not, loses nothing but the messages: the buffer drops what does not
//! fit and counts it.
//!
//! What is sent:
//!
//! - Notes, on the chosen MIDI channel. Velocity 0 to 1 becomes 1 to 127.
//!   A key that is struck again is released first, so every note-on has its
//!   note-off. A note is released on the channel it was started on.
//! - Bank select (controllers 0 and 32) and program change, when a program
//!   is chosen and whenever it or the bank changes.
//! - The pitch wheel, fourteen bits, whenever it moves.
//! - Eight assignable controllers. Each sends its value when it is switched
//!   on, and whenever its number or value changes, which is how automation
//!   of a value becomes a controller stream.
//! - All notes off (controller 123) when the host stops every note.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::instrument::Instrument;
use crate::param::{ParamSet, param_set};

/// How many assignable controllers a MIDI output channel has.
pub const MIDI_OUT_CONTROLLERS: usize = 8;
/// How many messages wait inside the instrument between two blocks.
pub const MIDI_OUT_PENDING: usize = 512;
/// Dedicated project-owned XInput route; never an app hardware preference slot.
pub const PROJECT_FEEDBACK_PORT: u8 = 8;
/// The frame of a message the host has not stamped yet.
const UNSTAMPED: u64 = u64::MAX;

/// One MIDI message from a [`MidiOut`], with the frame it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiOutEvent {
    /// The host's frame count at the block the message was made for.
    pub frame: u64,
    /// The message. Only the first `len` bytes count.
    pub bytes: [u8; 3],
    pub len: u8,
    /// Hardware slot 0..7 (0 is the main port), or 8 for project XInput.
    pub output_port: u8,
    /// Hosted instrument channel ID; zero selects hardware.
    pub plugin_destination: u32,
}

impl MidiOutEvent {
    /// The bytes of the message.
    pub fn message(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len.min(3))]
    }
}

/// One assignable controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MidiOutController {
    /// The controller number, 0 to 127, or -1 for none.
    pub number: i32,
    /// The value sent, 0 to 127.
    pub value: i32,
}

impl Default for MidiOutController {
    fn default() -> Self {
        Self {
            number: -1,
            value: 0,
        }
    }
}

/// Settings of a MIDI output channel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MidiOutParams {
    /// The MIDI channel, 1 to 16.
    pub channel: i32,
    /// The program, 1 to 128, or 0 to send none.
    pub program: i32,
    /// Bank select, most significant part, 0 to 127.
    pub bank_msb: i32,
    /// Bank select, least significant part, 0 to 127.
    pub bank_lsb: i32,
    /// The pitch wheel, -1 to 1, with 0 the center.
    pub pitch_bend: f32,
    pub controllers: [MidiOutController; MIDI_OUT_CONTROLLERS],
    #[serde(skip_serializing_if = "is_zero")]
    #[ts(as = "Option<i32>", optional)]
    pub output_port: i32,
    #[serde(skip_serializing_if = "is_zero")]
    #[ts(as = "Option<i32>", optional)]
    pub plugin_destination: i32,
    #[serde(skip_serializing_if = "is_false")]
    #[ts(as = "Option<bool>", optional)]
    pub feedback_enabled: bool,
    #[ts(as = "Option<i32>", optional)]
    pub feedback_controller: i32,
    /// 0 low-frequency motor, 1 high-frequency motor, 2 both.
    #[ts(as = "Option<i32>", optional)]
    pub feedback_motor: i32,
    #[ts(as = "Option<f32>", optional)]
    pub feedback_strength: f32,
    /// Strength = gain * velocity^curve, evaluated once on note-on.
    #[ts(as = "Option<f32>", optional)]
    pub feedback_curve: f32,
}

fn is_false(value: &bool) -> bool { !*value }

fn is_zero(value: &i32) -> bool {
    *value == 0
}

impl Default for MidiOutParams {
    fn default() -> Self {
        Self {
            channel: 1,
            program: 0,
            bank_msb: 0,
            bank_lsb: 0,
            pitch_bend: 0.0,
            controllers: [MidiOutController::default(); MIDI_OUT_CONTROLLERS],
            output_port: 0,
            plugin_destination: 0,
            feedback_enabled: false, feedback_controller: 0, feedback_motor: 0, feedback_strength: 1.0, feedback_curve: 1.0,
        }
    }
}

param_set!(MidiOutParams, "MIDI output", {
    int [channel] "channel" "MIDI channel" { None, 1, 16, 1 }
    int [program] "program" "Program" { None, 0, 128, 0 }
    int [bank_msb] "bankMsb" "Bank (coarse)" { None, 0, 127, 0 }
    int [bank_lsb] "bankLsb" "Bank (fine)" { None, 0, 127, 0 }
    float [pitch_bend] "pitchBend" "Pitch wheel" { None, Linear, -1.0, 1.0, 0.0 }
    int [controllers[0].number] "controllers.0.number" "Controller 1 number" { None, -1, 127, -1 }
    int [controllers[0].value] "controllers.0.value" "Controller 1" { None, 0, 127, 0 }
    int [controllers[1].number] "controllers.1.number" "Controller 2 number" { None, -1, 127, -1 }
    int [controllers[1].value] "controllers.1.value" "Controller 2" { None, 0, 127, 0 }
    int [controllers[2].number] "controllers.2.number" "Controller 3 number" { None, -1, 127, -1 }
    int [controllers[2].value] "controllers.2.value" "Controller 3" { None, 0, 127, 0 }
    int [controllers[3].number] "controllers.3.number" "Controller 4 number" { None, -1, 127, -1 }
    int [controllers[3].value] "controllers.3.value" "Controller 4" { None, 0, 127, 0 }
    int [controllers[4].number] "controllers.4.number" "Controller 5 number" { None, -1, 127, -1 }
    int [controllers[4].value] "controllers.4.value" "Controller 5" { None, 0, 127, 0 }
    int [controllers[5].number] "controllers.5.number" "Controller 6 number" { None, -1, 127, -1 }
    int [controllers[5].value] "controllers.5.value" "Controller 6" { None, 0, 127, 0 }
    int [controllers[6].number] "controllers.6.number" "Controller 7 number" { None, -1, 127, -1 }
    int [controllers[6].value] "controllers.6.value" "Controller 7" { None, 0, 127, 0 }
    int [controllers[7].number] "controllers.7.number" "Controller 8 number" { None, -1, 127, -1 }
    int [controllers[7].value] "controllers.7.value" "Controller 8" { None, 0, 127, 0 }
    toggle [feedback_enabled] "feedbackEnabled" "Project controller feedback" { false }
    int [feedback_controller] "feedbackController" "Controller index" { None, 0, 3, 0 }
    int [feedback_motor] "feedbackMotor" "Motor (low, high, both)" { None, 0, 2, 0 }
    float [feedback_strength] "feedbackStrength" "Maximum motor strength" { Fraction, Linear, 0.0, 1.0, 1.0 }
    float [feedback_curve] "feedbackCurve" "Velocity strength curve" { None, Linear, 0.25, 4.0, 1.0 }
});

/// The instrument. See the module documentation.
#[derive(Debug, Clone)]
pub struct MidiOut {
    params: MidiOutParams,
    /// What the hardware was last told, or `None` before the first message
    /// about settings since `prepare` or `reset`.
    sent: Option<MidiOutParams>,
    /// For each key, the MIDI channel (1 to 16) its note was started on, or
    /// 0 while the key is up.
    held: [u8; 128],
    pending: [MidiOutEvent; MIDI_OUT_PENDING],
    len: usize,
    dropped: u32,
    /// The frame the host last stamped with.
    last_frame: u64,
}

impl Default for MidiOut {
    fn default() -> Self {
        Self {
            params: MidiOutParams::default(),
            sent: None,
            held: [0; 128],
            pending: [MidiOutEvent {
                frame: UNSTAMPED,
                bytes: [0; 3],
                len: 0,
                output_port: 0,
                plugin_destination: 0,
            }; MIDI_OUT_PENDING],
            len: 0,
            dropped: 0,
            last_frame: 0,
        }
    }
}

impl MidiOut {
    /// Transfer fixed MIDI state into a prepared replacement while retaining its
    /// new settings. Destination changes balance old ownership in sync().
    pub fn adopt(&mut self, previous: &Self) {
        let params = self.params;
        *self = previous.clone();
        self.set_params(&params);
    }

    /// Messages that did not fit the buffer since the instrument was made.
    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    /// Gives every message made since the last call the frame `frame`. The
    /// host calls this at the start of each block it processes, with that
    /// block's first frame.
    pub fn stamp(&mut self, frame: u64) {
        self.last_frame = frame;
        for event in &mut self.pending[..self.len] {
            if event.frame == UNSTAMPED {
                event.frame = frame;
            }
        }
    }

    /// Hands over every waiting message, oldest first, and empties the
    /// buffer. A message made after the last stamp carries that stamp's
    /// frame, so messages never go back in time.
    pub fn drain(&mut self, out: &mut dyn FnMut(MidiOutEvent)) {
        let last = self.last_frame;
        for event in &mut self.pending[..self.len] {
            if event.frame == UNSTAMPED {
                event.frame = last;
            }
            out(*event);
        }
        self.len = 0;
    }

    fn push(&mut self, bytes: [u8; 3], len: u8) {
        if self.len == MIDI_OUT_PENDING {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        self.pending[self.len] = MidiOutEvent {
            frame: UNSTAMPED,
            bytes,
            len,
            output_port: if self.params.feedback_enabled { PROJECT_FEEDBACK_PORT } else { self.params.output_port as u8 },
            plugin_destination: if self.params.feedback_enabled { 0 } else { self.params.plugin_destination as u32 },
        };
        self.len += 1;
    }

    fn routed_channel(params: &MidiOutParams) -> i32 {
        if params.feedback_enabled { params.feedback_controller * 3 + params.feedback_motor + 1 } else { params.channel.clamp(1,16) }
    }
    fn status(kind: u8, channel: i32) -> u8 {
        kind | (channel.clamp(1, 16) as u8 - 1)
    }

    fn release(&mut self, key: u8) {
        let channel = self.held[usize::from(key)];
        if channel != 0 {
            self.held[usize::from(key)] = 0;
            self.push([Self::status(0x80, i32::from(channel)), key, 0], 3);
        }
    }

    fn release_all(&mut self) {
        for key in 0..128_u8 {
            self.release(key);
        }
    }

    /// Sends whatever differs between what the hardware was told and the
    /// settings now.
    fn sync(&mut self) {
        let now = self.params;
        let before = self.sent;
        let channel = Self::routed_channel(&now);
        let moved_channel = before.is_some_and(|before| Self::routed_channel(&before) != channel);
        let moved_route = before.is_some_and(|before| {
            before.output_port != now.output_port
                || before.plugin_destination != now.plugin_destination
                || before.feedback_enabled != now.feedback_enabled
                || before.feedback_controller != now.feedback_controller
                || before.feedback_motor != now.feedback_motor
                || before.feedback_strength != now.feedback_strength
                || before.feedback_curve != now.feedback_curve
        });
        if moved_channel || moved_route {
            // Notes still sounding belong to the channel they started on.
            // Release using the old destination before telling the new one anything.
            if let Some(before) = before {
                self.params = before;
            }
            self.release_all();
            self.params = now;
        }
        let fresh = before.is_none() || moved_channel || moved_route;
        let changed = |pick: fn(&MidiOutParams) -> i32| {
            fresh || before.is_some_and(|before| pick(&before) != pick(&now))
        };
        let bank = changed(|params| params.bank_msb) || changed(|params| params.bank_lsb);
        if now.program > 0 && (bank || changed(|params| params.program)) {
            self.push([Self::status(0xb0, channel), 0, now.bank_msb as u8], 3);
            self.push([Self::status(0xb0, channel), 32, now.bank_lsb as u8], 3);
            self.push([Self::status(0xc0, channel), (now.program - 1) as u8, 0], 2);
        }
        let wheel = |params: &MidiOutParams| {
            (8192.0 + f64::from(params.pitch_bend) * 8191.0)
                .round()
                .clamp(0.0, 16_383.0) as u16
        };
        let bend = wheel(&now);
        let bend_before = before
            .filter(|_| !moved_channel && !moved_route)
            .map(|before| wheel(&before));
        if bend_before.map_or(bend != 8192, |before| before != bend) {
            self.push(
                [
                    Self::status(0xe0, channel),
                    (bend & 0x7f) as u8,
                    (bend >> 7) as u8,
                ],
                3,
            );
        }
        for index in 0..MIDI_OUT_CONTROLLERS {
            let controller = now.controllers[index];
            if controller.number < 0 {
                continue;
            }
            let same =
                !fresh && before.is_some_and(|before| before.controllers[index] == controller);
            if !same {
                self.push(
                    [
                        Self::status(0xb0, channel),
                        controller.number as u8,
                        controller.value as u8,
                    ],
                    3,
                );
            }
        }
        self.sent = Some(now);
    }
}

impl Instrument for MidiOut {
    type Params = MidiOutParams;

    fn prepare(&mut self, _sample_rate: f32, _max_block: usize) {
        // Nothing an unprepared instrument was told has reached hardware.
        self.len = 0;
        self.held = [0; 128];
        self.sent = None;
        self.last_frame = 0;
    }

    fn reset(&mut self) {
        self.release_all();
        self.sent = None;
    }

    fn set_params(&mut self, params: &MidiOutParams) {
        self.params = params.sanitized();
        self.params.output_port = self.params.output_port.clamp(0, 7);
        self.params.plugin_destination = self.params.plugin_destination.max(-1);
        self.sync();
    }

    fn note_on(&mut self, key: u8, velocity: f32) {
        let key = key.min(127);
        if velocity.is_nan() || velocity <= 0.0 {
            self.release(key);
            return;
        }
        if self.sent.is_none() {
            self.sync();
        }
        self.release(key);
        let channel = Self::routed_channel(&self.params);
        let velocity = if self.params.feedback_enabled { velocity.min(1.0).powf(self.params.feedback_curve) * self.params.feedback_strength } else { velocity.min(1.0) };
        if velocity <= 0.0 { return; }
        let velocity = (velocity * 127.0).round().clamp(1.0, 127.0) as u8;
        self.held[usize::from(key)] = channel as u8;
        self.push([Self::status(0x90, channel), key, velocity], 3);
    }

    fn note_off(&mut self, key: u8) {
        self.release(key.min(127));
    }

    fn all_notes_off(&mut self) {
        let any = self.held.iter().any(|channel| *channel != 0);
        self.release_all();
        if any {
            self.push([Self::status(0xb0, Self::routed_channel(&self.params)), 123, 0], 3);
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        left.fill(0.0);
        right.fill(0.0);
    }

    fn active_voices(&self) -> usize {
        self.held.iter().filter(|channel| **channel != 0).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(instrument: &mut MidiOut) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        instrument.drain(&mut |event| out.push(event.message().to_vec()));
        out
    }

    fn ready(params: MidiOutParams) -> MidiOut {
        let mut instrument = MidiOut::default();
        instrument.prepare(48_000.0, 256);
        instrument.set_params(&params);
        instrument
    }

    #[test]
    fn destination_change_and_prepared_adoption_balance_the_old_route() {
        let mut previous = ready(MidiOutParams {
            output_port: 2,
            program: 12,
            ..Default::default()
        });
        taken(&mut previous);
        previous.note_on(60, 1.0);
        taken(&mut previous);
        let mut next = ready(MidiOutParams {
            plugin_destination: 42,
            program: 12,
            ..Default::default()
        });
        next.adopt(&previous);
        let mut events = Vec::new();
        next.drain(&mut |event| events.push(event));
        assert_eq!(
            (
                events[0].bytes,
                events[0].output_port,
                events[0].plugin_destination
            ),
            ([0x80, 60, 0], 2, 0)
        );
        assert!(
            events[1..]
                .iter()
                .all(|event| event.plugin_destination == 42)
        );
        assert_eq!(next.active_voices(), 0);
    }

    #[test]
    fn notes_become_balanced_messages_on_the_chosen_channel() {
        let mut out = ready(MidiOutParams {
            channel: 10,
            ..MidiOutParams::default()
        });
        assert!(taken(&mut out).is_empty(), "defaults send nothing");
        out.note_on(36, 1.0);
        out.note_on(38, 0.5);
        out.note_on(40, 0.001);
        assert_eq!(out.active_voices(), 3);
        out.note_off(36);
        out.note_off(36);
        out.note_on(38, 0.0);
        assert_eq!(
            taken(&mut out),
            [
                vec![0x99, 36, 127],
                vec![0x99, 38, 64],
                vec![0x99, 40, 1],
                vec![0x89, 36, 0],
                vec![0x89, 38, 0],
            ]
        );
        // Striking a held key again releases it first.
        out.note_on(40, 0.8);
        assert_eq!(taken(&mut out), [vec![0x89, 40, 0], vec![0x99, 40, 102]]);
        assert_eq!(out.active_voices(), 1);
        // Keys and velocities outside their range are brought into it.
        out.note_on(200, 9.0);
        out.note_on(1, f32::NAN);
        assert_eq!(taken(&mut out), [vec![0x99, 127, 127]]);
    }

    #[test]
    fn stopping_every_note_releases_each_and_sends_all_notes_off_once() {
        let mut out = ready(MidiOutParams::default());
        out.all_notes_off();
        assert!(taken(&mut out).is_empty(), "nothing held, nothing sent");
        out.note_on(60, 1.0);
        out.note_on(64, 1.0);
        taken(&mut out);
        out.all_notes_off();
        assert_eq!(
            taken(&mut out),
            [vec![0x80, 60, 0], vec![0x80, 64, 0], vec![0xb0, 123, 0]]
        );
        assert_eq!(out.active_voices(), 0);
        out.note_on(60, 1.0);
        taken(&mut out);
        out.reset();
        assert_eq!(taken(&mut out), [vec![0x80, 60, 0]]);
    }

    #[test]
    fn program_and_bank_are_sent_when_chosen_and_when_they_change() {
        let mut params = MidiOutParams {
            channel: 2,
            program: 5,
            bank_msb: 1,
            bank_lsb: 3,
            ..MidiOutParams::default()
        };
        let mut out = ready(params);
        let select = |program: u8, msb: u8, lsb: u8| {
            vec![vec![0xb1, 0, msb], vec![0xb1, 32, lsb], vec![0xc1, program]]
        };
        assert_eq!(taken(&mut out), select(4, 1, 3));
        out.set_params(&params);
        assert!(taken(&mut out).is_empty(), "the same settings send nothing");
        params.program = 128;
        out.set_params(&params);
        assert_eq!(taken(&mut out), select(127, 1, 3));
        params.bank_lsb = 9;
        out.set_params(&params);
        assert_eq!(taken(&mut out), select(127, 1, 9));
        // No program: the bank alone is not sent.
        params.program = 0;
        params.bank_msb = 7;
        out.set_params(&params);
        assert!(taken(&mut out).is_empty());
    }

    #[test]
    fn a_controller_value_change_is_one_message_and_off_is_silent() {
        let mut params = MidiOutParams::default();
        params.controllers[0] = MidiOutController {
            number: 74,
            value: 10,
        };
        params.controllers[3].value = 99;
        let mut out = ready(params);
        assert_eq!(taken(&mut out), [vec![0xb0, 74, 10]]);
        for value in [11, 12, 12, 127] {
            params.controllers[0].value = value;
            out.set_params(&params);
        }
        assert_eq!(
            taken(&mut out),
            [vec![0xb0, 74, 11], vec![0xb0, 74, 12], vec![0xb0, 74, 127]]
        );
        // Switching a controller on sends where it stands.
        params.controllers[3].number = 1;
        out.set_params(&params);
        assert_eq!(taken(&mut out), [vec![0xb0, 1, 99]]);
        params.controllers[3].number = -1;
        params.controllers[3].value = 5;
        out.set_params(&params);
        assert!(taken(&mut out).is_empty());
    }

    #[test]
    fn the_pitch_wheel_sends_fourteen_bits_around_the_center() {
        let mut params = MidiOutParams::default();
        let mut out = ready(params);
        for (bend, lsb, msb) in [(1.0, 0x7f, 0x7f), (-1.0, 1, 0), (0.0, 0, 0x40)] {
            params.pitch_bend = bend;
            out.set_params(&params);
            assert_eq!(taken(&mut out), [vec![0xe0, lsb, msb]], "{bend}");
        }
        out.set_params(&params);
        assert!(taken(&mut out).is_empty());
    }

    #[test]
    fn changing_the_channel_releases_notes_where_they_started() {
        let mut params = MidiOutParams {
            program: 1,
            ..MidiOutParams::default()
        };
        params.controllers[0] = MidiOutController {
            number: 7,
            value: 100,
        };
        let mut out = ready(params);
        out.note_on(60, 1.0);
        taken(&mut out);
        params.channel = 5;
        out.set_params(&params);
        // The held note ends on channel 1; the settings are told to channel 5.
        assert_eq!(
            taken(&mut out),
            [
                vec![0x80, 60, 0],
                vec![0xb4, 0, 0],
                vec![0xb4, 32, 0],
                vec![0xc4, 0],
                vec![0xb4, 7, 100],
            ]
        );
        out.note_on(60, 1.0);
        out.note_off(60);
        assert_eq!(taken(&mut out), [vec![0x94, 60, 127], vec![0x84, 60, 0]]);
    }

    #[test]
    fn messages_carry_the_frame_of_the_block_they_were_made_for() {
        let mut out = ready(MidiOutParams::default());
        out.note_on(60, 1.0);
        out.stamp(480);
        out.note_off(60);
        out.stamp(960);
        // A second stamp does not move a message that already has a frame.
        out.stamp(1_440);
        let mut frames = Vec::new();
        out.drain(&mut |event| frames.push((event.frame, event.bytes[0])));
        assert_eq!(frames, [(480, 0x90), (960, 0x80)]);
        // Made after the last stamp: it carries that stamp's frame.
        out.note_on(61, 1.0);
        out.drain(&mut |event| frames.push((event.frame, event.bytes[0])));
        assert_eq!(frames.last(), Some(&(1_440, 0x90)));
    }

    #[test]
    fn the_output_is_silence_and_a_full_buffer_drops_and_counts() {
        let mut out = ready(MidiOutParams::default());
        let (mut left, mut right) = ([1.0_f32; 64], [1.0_f32; 64]);
        out.note_on(60, 1.0);
        out.process(&mut left, &mut right);
        assert!(left.iter().chain(&right).all(|sample| *sample == 0.0));
        assert_eq!((out.latency_samples(), out.tail_samples()), (0, 0));
        taken(&mut out);
        for _ in 0..MIDI_OUT_PENDING {
            out.note_on(61, 1.0);
        }
        // Each strike after the first is a release and a start.
        assert_eq!(out.dropped(), MIDI_OUT_PENDING as u32 - 1);
        assert_eq!(taken(&mut out).len(), MIDI_OUT_PENDING);
        out.note_on(62, 1.0);
        assert_eq!(taken(&mut out), [vec![0x90, 62, 127]]);
    }

    #[test]
    fn settings_out_of_range_are_brought_into_it() {
        let wild = MidiOutParams {
            channel: 99,
            program: -4,
            bank_msb: 500,
            bank_lsb: -1,
            pitch_bend: f32::NAN,
            controllers: [MidiOutController {
                number: 900,
                value: -3,
            }; MIDI_OUT_CONTROLLERS],
            ..Default::default()
        };
        let clean = wild.sanitized();
        assert_eq!((clean.channel, clean.program), (16, 0));
        assert_eq!((clean.bank_msb, clean.bank_lsb), (127, 0));
        assert_eq!(clean.pitch_bend, 0.0);
        assert_eq!(
            clean.controllers[7],
            MidiOutController {
                number: 127,
                value: 0
            }
        );
        let mut out = ready(wild);
        out.note_on(60, 1.0);
        let sent = taken(&mut out);
        assert!(sent.iter().all(|message| message[0] & 0x0f == 15));
        assert!(
            sent.iter()
                .all(|message| message[1..].iter().all(|byte| *byte < 128))
        );
    }
    #[test]
    fn project_feedback_encodes_controller_motor_and_curve_and_balances_route_changes() {
        let mut out=ready(MidiOutParams { feedback_enabled:true,feedback_controller:2,feedback_motor:1,feedback_strength:0.5,feedback_curve:2.0,..Default::default() });
        let _=taken(&mut out);out.note_on(60,0.5);
        let mut events=Vec::new();out.drain(&mut |event|events.push(event));
        assert_eq!(events[0].message(),&[0x97,60,16]);assert_eq!(events[0].output_port,PROJECT_FEEDBACK_PORT);assert_eq!(events[0].plugin_destination,0);
        let mut params=out.params;params.feedback_controller=3;params.feedback_motor=2;out.set_params(&params);
        events.clear();out.drain(&mut |event|events.push(event));
        assert_eq!(events[0].message(),&[0x87,60,0]);assert_eq!(events[0].output_port,PROJECT_FEEDBACK_PORT);
        out.note_on(61,1.0);events.clear();out.drain(&mut |event|events.push(event));assert_eq!(events[0].message(),&[0x9b,61,64]);
        out.note_off(61);assert_eq!(taken(&mut out),vec![vec![0x8b,61,0]]);
    }

}
