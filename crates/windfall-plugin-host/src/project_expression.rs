//! Shared frame-clock project-law/expression ramps. All storage is Copy or
//! prepared on control; neither the desktop nor helper allocates while ticking.
//!
//! A hosted plugin returns one summed output, so each of a note's pan and
//! volume has exactly one of two routes. Where the plugin offers the property
//! per note, the plugin applies it. Where it does not, the channel applies it
//! to the whole output after the plugin, following the most recent sounding
//! note. [`NoteRamp::split`] divides one note's project-law gain between the
//! two so that their product is that gain whatever the routes are, and stays
//! that gain while a route changes.
use crate::{NoteExpressionKind as K, PluginNoteExpression};
use windfall_dsp::{NotePan, ProjectPanLaw};

pub const NEUTRAL: PluginNoteExpression = PluginNoteExpression {
    values: [1.0, 0.5, 0.0, 0.5, 0.5, 0.5, 0.5],
    mask: 0,
};

// Explicit protocol numbers, independent of Rust enum representation.
pub fn encode_law(law: Option<ProjectPanLaw>) -> u32 {
    match law {
        None => 0,
        Some(ProjectPanLaw::Balance) => 1,
        Some(ProjectPanLaw::ConstantPower) => 2,
        Some(ProjectPanLaw::Compromise) => 3,
        Some(ProjectPanLaw::Linear) => 4,
    }
}
pub fn decode_law(
    code: u32,
) -> Result<Option<ProjectPanLaw>, crate::bridge::protocol::ProtocolError> {
    Ok(match code {
        0 => None,
        1 => Some(ProjectPanLaw::Balance),
        2 => Some(ProjectPanLaw::ConstantPower),
        3 => Some(ProjectPanLaw::Compromise),
        4 => Some(ProjectPanLaw::Linear),
        _ => return Err(crate::bridge::protocol::ProtocolError::Configuration),
    })
}

/// Who applies each per-note property: `true` is the plugin, per note;
/// `false` is the channel, after the plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Routes {
    pub pan: bool,
    pub volume: bool,
}
impl Routes {
    pub const NATIVE: Self = Self {
        pan: true,
        volume: true,
    };
    pub const CHANNEL: Self = Self {
        pan: false,
        volume: false,
    };
    /// What a plugin with or without note instances offers on a channel
    /// whose expression mask is `mask`.
    pub fn of(instances: bool, mask: u32) -> Self {
        Self {
            pan: instances && mask & K::Pan.mask() != 0,
            volume: instances && mask & K::Volume.mask() != 0,
        }
    }
    /// A property stays with the plugin only where both offer it. One plugin
    /// has one output, so a property the channel must apply for any sounding
    /// note is applied by the channel for all of them.
    pub fn and(self, other: Self) -> Self {
        Self {
            pan: self.pan && other.pan,
            volume: self.volume && other.volume,
        }
    }
    fn weights(self) -> [f32; 2] {
        [
            f32::from(u8::from(self.pan)),
            f32::from(u8::from(self.volume)),
        ]
    }
}

/// Frames a route change or a change of leading note takes: five
/// milliseconds, the length of every other ramp here.
pub fn fade_frames(rate: f32) -> u32 {
    (rate * 0.005).round().max(1.0) as u32
}

#[derive(Clone, Copy)]
pub struct NoteRamp {
    pan: NotePan,
    from: [f64; 7],
    from_mask: u32,
    target: PluginNoteExpression,
    step: [f64; 7],
    elapsed: u32,
    duration: u32,
    law: Option<ProjectPanLaw>,
    routes: Routes,
    /// Pan, volume: 1 where the plugin applies the property, 0 where the
    /// channel does, and in between while a route changes.
    weight: [f32; 2],
    weight_step: [f32; 2],
    weight_left: u32,
}
impl NoteRamp {
    pub const EMPTY: Self = Self {
        pan: NotePan::CENTER,
        from: NEUTRAL.values,
        from_mask: 0,
        target: NEUTRAL,
        step: [0.0; 7],
        elapsed: 0,
        duration: 0,
        law: None,
        routes: Routes::CHANNEL,
        weight: [0.0; 2],
        weight_step: [0.0; 2],
        weight_left: 0,
    };
    pub fn new(controls: PluginNoteExpression, law: Option<ProjectPanLaw>, routes: Routes) -> Self {
        Self {
            pan: NotePan::new(
                (controls.values[1] * 2.0 - 1.0) as f32,
                law.unwrap_or_default(),
            ),
            from: controls.values,
            target: controls,
            law,
            routes,
            weight: routes.weights(),
            ..Self::EMPTY
        }
    }
    fn raw(&self) -> [f64; 7] {
        if self.elapsed >= self.duration {
            self.target.values
        } else {
            std::array::from_fn(|i| self.from[i] + self.step[i] * f64::from(self.elapsed))
        }
    }
    pub fn retarget(
        &mut self,
        controls: PluginNoteExpression,
        law: Option<ProjectPanLaw>,
        routes: Routes,
        rate: f32,
    ) {
        if law.is_none() {
            *self = Self::new(controls, law, routes);
            return;
        }
        if self.target != controls {
            self.from_mask = self.current().mask;
            self.from = self.raw();
            self.target = controls;
            self.elapsed = 0;
            self.duration = (rate * 0.005).round().max(1.0) as u32;
            self.step = std::array::from_fn(|i| {
                (controls.values[i] - self.from[i]) / f64::from(self.duration)
            });
        }
        self.pan.set_position(
            (controls.values[1] * 2.0 - 1.0) as f32,
            law.unwrap_or_default(),
            rate,
        );
        self.law = law;
        self.route(routes, fade_frames(rate));
    }
    /// Moves each property to its new route over `frames`. The values, their
    /// targets and the time they have left are untouched: only who applies
    /// them changes, and [`NoteRamp::split`] keeps their product throughout.
    pub fn route(&mut self, routes: Routes, frames: u32) {
        if routes == self.routes {
            return;
        }
        self.routes = routes;
        let to = routes.weights();
        if self.law.is_none() || frames == 0 {
            self.weight = to;
            self.weight_left = 0;
            return;
        }
        self.weight_left = frames;
        self.weight_step = std::array::from_fn(|i| (to[i] - self.weight[i]) / frames as f32);
    }
    pub fn routes(&self) -> Routes {
        self.routes
    }
    /// What the plugin offers for this note now. A property it does not
    /// offer cannot be shared with it for even one frame: the channel takes
    /// all of it at once, which is exactly what the plugin stopped doing.
    pub fn limit(&mut self, offered: Routes) {
        for (index, offered) in [offered.pan, offered.volume].into_iter().enumerate() {
            if !offered {
                self.weight[index] = 0.0;
                self.weight_step[index] = 0.0;
            }
        }
        self.routes = self.routes.and(offered);
    }
    /// The controls the plugin receives and the gains the channel applies
    /// after it, for this frame.
    ///
    /// With the project law's gains `(l, r)` for the note's pan and its
    /// volume `v`, let `b = r / (l + r)` and `s = (l + r) / 2`. A plugin that
    /// pans by `(1 - p, p)` and scales by its volume then needs `p = b` and
    /// volume `v s` to produce `v (l, r)` on its own. With pan weight `wp`
    /// and volume weight `wv` the plugin is sent `p = 1/2 + wp (b - 1/2)` and
    /// volume `1 + wv (v s - 1)`, and the channel makes up exactly the rest:
    /// `(v s / volume) ((1 - b) / (1 - p), b / p)`. At weight 1 the channel
    /// gain is 1; at weight 0 the plugin receives the neutral value.
    pub fn split(&self) -> (PluginNoteExpression, (f32, f32)) {
        let mask = self.target.mask
            | if self.elapsed < self.duration {
                self.from_mask
            } else {
                0
            };
        let mut controls = PluginNoteExpression {
            values: self.raw(),
            mask,
        };
        if self.law.is_none() {
            return (controls, (1.0, 1.0));
        }
        let (left, right) = self.pan.gains();
        let sum = f64::from(left) + f64::from(right);
        let balance = if sum > 0.0 {
            f64::from(right) / sum
        } else {
            0.5
        };
        let level = controls.values[0] * sum * 0.5;
        let [pan_weight, volume_weight] = self.weight.map(f64::from);
        let native_pan = if pan_weight >= 1.0 {
            balance
        } else {
            0.5 + pan_weight * (balance - 0.5)
        };
        let native_volume = if volume_weight >= 1.0 {
            level
        } else {
            1.0 + volume_weight * (level - 1.0)
        };
        // Where the plugin's own factor is zero the product already is.
        let rest = |whole: f64, part: f64| if part > 0.0 { whole / part } else { 1.0 };
        let made_up = rest(level, native_volume);
        let host = (
            (made_up * rest(1.0 - balance, 1.0 - native_pan)) as f32,
            (made_up * rest(balance, native_pan)) as f32,
        );
        controls.values[1] = native_pan;
        controls.values[0] = native_volume;
        // A property on its way to or from the plugin is still sent, so the
        // plugin reaches the neutral value before the channel takes over.
        for (kind, weight) in [(K::Pan, pan_weight), (K::Volume, volume_weight)] {
            if weight > 0.0 {
                controls.mask |= kind.mask();
            } else {
                controls.mask &= !kind.mask();
            }
        }
        (controls, host)
    }
    pub fn current(&self) -> PluginNoteExpression {
        self.split().0
    }
    /// The gains the channel applies after the plugin for this note.
    pub fn host(&self) -> (f32, f32) {
        self.split().1
    }
    pub fn tick(&mut self) -> PluginNoteExpression {
        let controls = self.current();
        self.advance();
        controls
    }
    fn advance(&mut self) {
        self.pan.tick();
        self.elapsed = self.elapsed.saturating_add(1).min(self.duration);
        if self.weight_left > 0 {
            self.weight_left -= 1;
            self.weight = if self.weight_left == 0 {
                self.routes.weights()
            } else {
                std::array::from_fn(|i| self.weight[i] + self.weight_step[i])
            };
        }
    }
    /// As [`NoteRamp::tick`], with the channel's share of the same frame.
    pub fn tick_split(&mut self) -> (PluginNoteExpression, (f32, f32)) {
        let split = self.split();
        self.advance();
        split
    }
    pub fn active(&self) -> bool {
        !self.pan.settled() || self.elapsed < self.duration || self.weight_left > 0
    }
    /// The value each control is moving to, and the frames it still has.
    pub fn target(&self) -> (PluginNoteExpression, u32) {
        (self.target, self.duration - self.elapsed.min(self.duration))
    }
}

/// The channel's side of the routes: one pair of gains for the plugin's
/// whole output. It follows the most recently started sounding note, moves
/// to a new leader over the fade, holds its last value while nothing sounds,
/// and fades the whole output in and out around a plugin restart.
#[derive(Clone, Copy)]
pub struct ChannelStage {
    gain: [f32; 2],
    leader: u64,
    left: u32,
    frames: u32,
    fresh: bool,
    presence: f32,
    presence_to: f32,
    presence_left: u32,
}
impl ChannelStage {
    pub fn new(rate: f32) -> Self {
        Self {
            gain: [1.0; 2],
            leader: 0,
            left: 0,
            frames: fade_frames(rate),
            fresh: true,
            presence: 1.0,
            presence_to: 1.0,
            presence_left: 0,
        }
    }
    pub fn reset(&mut self) {
        *self = Self {
            frames: self.frames,
            ..Self::new(48_000.0)
        };
    }
    /// One frame. `leader` names the most recent sounding note (0 when none
    /// sounds) and `target` is that note's channel share.
    pub fn tick(&mut self, leader: u64, target: (f32, f32)) -> [f32; 2] {
        if leader != 0 {
            let target = [target.0, target.1];
            if self.fresh {
                self.fresh = false;
                self.leader = leader;
                self.gain = target;
            } else if leader != self.leader {
                self.leader = leader;
                self.left = self.frames;
            }
            if self.left > 0 {
                for (gain, to) in self.gain.iter_mut().zip(target) {
                    *gain += (to - *gain) / self.left as f32;
                }
                self.left -= 1;
            } else {
                self.gain = target;
            }
        }
        if self.presence_left > 0 {
            self.presence += (self.presence_to - self.presence) / self.presence_left as f32;
            self.presence_left -= 1;
            if self.presence_left == 0 {
                self.presence = self.presence_to;
            }
        }
        [self.gain[0] * self.presence, self.gain[1] * self.presence]
    }
    /// True when ticking with this leader starts no move to a new one.
    pub fn follows(&self, leader: u64) -> bool {
        leader == 0 || (!self.fresh && leader == self.leader)
    }
    /// True while a tick can still change the gains without a new leader
    /// or a moving note.
    pub fn moving(&self) -> bool {
        self.left > 0 || self.presence_left > 0
    }
    /// True when the stage multiplies by something other than one.
    pub fn engaged(&self) -> bool {
        self.gain != [1.0; 2] || self.presence != 1.0 || self.moving()
    }
    pub fn gains(&self) -> [f32; 2] {
        [self.gain[0] * self.presence, self.gain[1] * self.presence]
    }
    /// Fades the whole output to silence, before the plugin goes away.
    pub fn depart(&mut self) {
        self.presence_to = 0.0;
        self.presence_left = self.frames;
    }
    /// Starts the whole output from silence and fades it in, after the
    /// plugin comes back or a replacement takes over.
    pub fn arrive(&mut self) {
        self.presence = 0.0;
        self.presence_to = 1.0;
        self.presence_left = self.frames;
    }
    pub fn departed(&self) -> bool {
        self.presence_to == 0.0 && self.presence_left == 0
    }
    pub fn departing(&self) -> bool {
        self.presence_to == 0.0
    }
    /// Brings the output back after a departure that did not happen.
    pub fn resume(&mut self) {
        self.presence_to = 1.0;
        self.presence_left = self.frames;
    }
    /// Carries the gains and the leader over to the stage of a replacement,
    /// so it continues from where this one stopped.
    pub fn inherit(&mut self, before: &Self) {
        self.gain = before.gain;
        self.leader = before.leader;
        self.left = before.left;
        self.fresh = before.fresh;
    }
}

/// Delays the channel's share by the plugin's own latency. What the plugin
/// is sent now reaches its output that many frames later, and the two halves
/// of [`NoteRamp::split`] only multiply to the right gain on the same frame.
/// Allocated on a control thread; pushing never allocates.
pub struct ShareDelay {
    ring: Box<[(u64, (f32, f32))]>,
    at: usize,
    latency: usize,
    last: (u64, (f32, f32)),
    /// Frames until everything still in flight equals `last`.
    pending: usize,
}
impl ShareDelay {
    /// Room for up to `capacity` frames of latency.
    pub fn new(capacity: usize) -> Self {
        Self {
            ring: vec![(0, (1.0, 1.0)); capacity + 1].into_boxed_slice(),
            at: 0,
            latency: 0,
            last: (0, (1.0, 1.0)),
            pending: 0,
        }
    }
    pub fn set_latency(&mut self, frames: usize) {
        let frames = frames.min(self.ring.len() - 1);
        if frames != self.latency {
            self.latency = frames;
            self.ring.fill(self.last);
            self.pending = 0;
        }
    }
    /// True while a frame pushed earlier has yet to come out changed.
    pub fn pending(&self) -> bool {
        self.pending > 0
    }
    /// Whether pushing `share` now would differ from the last push.
    pub fn changes(&self, share: (u64, (f32, f32))) -> bool {
        self.latency > 0 && share != self.last
    }
    /// One frame in, the frame from `latency` frames ago out.
    pub fn push(&mut self, share: (u64, (f32, f32))) -> (u64, (f32, f32)) {
        if self.latency == 0 {
            self.last = share;
            return share;
        }
        if self.last.0 == 0 && share.0 != 0 {
            // The first note after silence: nothing it could be late for.
            self.prime(share);
        }
        if self.pending == 0 && share != self.last {
            // Nothing was pushed while the share held still: what would
            // have been is the value it held.
            self.ring.fill(self.last);
        }
        if share != self.last {
            self.last = share;
            self.pending = self.latency;
        } else {
            self.pending = self.pending.saturating_sub(1);
        }
        let len = self.ring.len();
        self.ring[self.at] = share;
        let out = self.ring[(self.at + len - self.latency) % len];
        self.at = (self.at + 1) % len;
        out
    }
    pub fn reset(&mut self) {
        self.prime((0, (1.0, 1.0)));
    }
    /// Starts as if `share` had been pushed for as long as the delay is: a
    /// unit that takes over a sounding note continues from the share that
    /// note had, instead of from none.
    pub fn prime(&mut self, share: (u64, (f32, f32))) {
        self.last = share;
        self.ring.fill(share);
        self.pending = 0;
    }
}

/// What one sounding note hands to the unit that replaces its plugin after a
/// capability change: who it is and its ramp, mid-flight.
#[derive(Clone, Copy)]
pub struct Carried {
    pub id: u64,
    pub key: u8,
    pub channel: u8,
    pub serial: u64,
    pub ramp: NoteRamp,
}
/// One plugin target's sounding notes and channel stage, written by the unit
/// that plays them and read by the unit that takes over. Both run on the
/// audio thread, one after the other, so the lock around this is only ever
/// tried, never waited for. Allocated once on a control thread.
pub struct Carry {
    owners: Box<[Carried]>,
    count: usize,
    stage: Option<ChannelStage>,
}
impl Carry {
    pub fn new() -> Self {
        Self {
            owners: vec![
                Carried {
                    id: 0,
                    key: 0,
                    channel: 0,
                    serial: 0,
                    ramp: NoteRamp::EMPTY,
                };
                crate::MAX_NOTE_INSTANCES
            ]
            .into_boxed_slice(),
            count: 0,
            stage: None,
        }
    }
    /// Replaces what is carried with the notes that sound now.
    pub fn store(&mut self, stage: &ChannelStage, owners: impl Iterator<Item = Carried>) {
        self.count = 0;
        for owner in owners.take(self.owners.len()) {
            self.owners[self.count] = owner;
            self.count += 1;
        }
        self.stage = Some(*stage);
    }
    /// The carried note with this identity, if it is the same key and channel.
    pub fn find(&self, id: u64, key: u8, channel: u8) -> Option<Carried> {
        self.owners[..self.count]
            .iter()
            .find(|owner| owner.id == id && owner.key == key && owner.channel == channel)
            .copied()
    }
    pub fn stage(&self) -> Option<ChannelStage> {
        self.stage
    }
    pub fn serial(&self) -> u64 {
        self.owners[..self.count]
            .iter()
            .map(|owner| owner.serial)
            .max()
            .unwrap_or(0)
    }
}
impl Default for Carry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
struct Owner {
    id: u32,
    key: u8,
    channel: u8,
    /// Order of arrival: the highest sounding one leads the channel stage.
    serial: u64,
    target: PluginNoteExpression,
    sent: PluginNoteExpression,
    ramp: NoteRamp,
    fresh: bool,
    dirty: bool,
    /// Held over a recovery and not yet named by its snapshot.
    stale: bool,
}
impl Owner {
    const EMPTY: Self = Self {
        id: 0,
        key: 0,
        channel: 0,
        serial: 0,
        target: NEUTRAL,
        sent: NEUTRAL,
        ramp: NoteRamp::EMPTY,
        fresh: false,
        dirty: false,
        stale: false,
    };
}
/// Helper-side ownership, allocated once before Ready. Incoming expressions are
/// collected together at an event frame, then one complete target is ramped.
pub struct Bank {
    owners: Box<[Owner]>,
    law: Option<ProjectPanLaw>,
    rate: f32,
    overflow: bool,
    serial: u64,
    recovering: bool,
    stage: ChannelStage,
    delay: ShareDelay,
    gain: [f32; 2],
}
impl Bank {
    pub fn new(rate: f32) -> Self {
        Self {
            // One second is the longest latency a hosted plugin may report.
            delay: ShareDelay::new(rate.max(1.0) as usize),
            owners: vec![Owner::EMPTY; crate::MAX_NOTE_INSTANCES].into_boxed_slice(),
            law: None,
            rate,
            overflow: false,
            serial: 0,
            recovering: false,
            stage: ChannelStage::new(rate),
            gain: [1.0; 2],
        }
    }
    pub fn reset(&mut self) {
        self.owners.fill(Owner::EMPTY);
        self.overflow = false;
        self.recovering = false;
        self.stage.reset();
        self.delay.reset();
        self.gain = [1.0; 2];
    }
    /// The plugin's latency in frames, from a control thread.
    pub fn set_latency(&mut self, frames: usize) {
        self.delay.set_latency(frames);
    }
    /// The processor was reset and the held snapshot is about to be replayed.
    /// Notes the snapshot names again keep their ramp where it was: the
    /// value, its target and the frames it has left.
    pub fn begin_recovery(&mut self) {
        self.overflow = false;
        self.recovering = true;
        for owner in self.owners.iter_mut().filter(|owner| owner.id != 0) {
            owner.stale = true;
        }
    }
    /// The snapshot has been replayed: notes it did not name have ended.
    pub fn end_recovery(&mut self) {
        self.recovering = false;
        for owner in self.owners.iter_mut().filter(|owner| owner.stale) {
            *owner = Owner::EMPTY;
        }
    }
    pub fn set_law(&mut self, law: Option<ProjectPanLaw>) {
        if self.law != law {
            self.law = law;
            for owner in self.owners.iter_mut().filter(|owner| owner.id != 0) {
                owner.dirty = true;
            }
        }
    }
    /// Returns true for expressions consumed by the ramp bank. Starts/releases
    /// still reach the native processor through its reserved release path.
    pub fn observe(&mut self, event: crate::HostEvent) -> bool {
        use crate::HostEvent as E;
        match event {
            E::NoteOnInstance {
                id, key, channel, ..
            } => {
                let held = self
                    .owners
                    .iter()
                    .position(|owner| owner.id == id && id != 0);
                if let Some(slot) = held
                    && self.recovering
                    && self.owners[slot].stale
                    && self.owners[slot].key == key
                    && self.owners[slot].channel == channel
                {
                    // The same note after a processor reset: everything is
                    // sent again, nothing about its ramp starts over.
                    let owner = &mut self.owners[slot];
                    owner.stale = false;
                    owner.sent = NEUTRAL;
                    owner.dirty = true;
                    return false;
                }
                let slot = held.or_else(|| self.owners.iter().position(|owner| owner.id == 0));
                if let Some(slot) = slot {
                    self.serial += 1;
                    self.owners[slot] = Owner {
                        id,
                        key,
                        channel,
                        serial: self.serial,
                        fresh: true,
                        dirty: true,
                        ..Owner::EMPTY
                    };
                } else {
                    self.overflow = true;
                }
            }
            E::NoteExpression {
                id, kind, value, ..
            } => {
                if let Some(owner) = self
                    .owners
                    .iter_mut()
                    .find(|owner| owner.id == id && id != 0)
                {
                    owner.target.values[kind as usize] = value;
                    owner.target.mask |= kind.mask();
                    owner.dirty = true;
                    return true;
                }
            }
            E::NoteOffInstance { id, .. } => {
                if let Some(owner) = self.owners.iter_mut().find(|owner| owner.id == id) {
                    *owner = Owner::EMPTY;
                }
            }
            E::AllNotesOff { .. } => {
                self.owners.fill(Owner::EMPTY);
                self.overflow = false;
            }
            _ => {}
        }
        false
    }
    pub fn active(&self) -> bool {
        self.stage.moving()
            || self.delay.pending()
            || self
                .owners
                .iter()
                .any(|owner| owner.id != 0 && (owner.dirty || owner.ramp.active()))
    }
    /// The gains the caller applies to everything the plugin returned for
    /// the frame [`Bank::frame`] last prepared.
    pub fn gain(&self) -> [f32; 2] {
        self.gain
    }
    pub fn frame(&mut self, processor: &mut crate::PluginProcessor) -> bool {
        let instances = processor.supports_note_instances();
        let mut accepted = !self.overflow;
        // One output, one route per property: the plugin keeps a property
        // only if it offers it for every note that sounds.
        let mut routes = Routes::NATIVE;
        for owner in self.owners.iter().filter(|owner| owner.id != 0) {
            routes = routes.and(Routes::of(
                instances,
                processor.note_expression_mask(owner.channel),
            ));
        }
        let fade = fade_frames(self.rate);
        let mut leader = (0, (1.0, 1.0));
        for owner in self.owners.iter_mut().filter(|owner| owner.id != 0) {
            let mask = processor.note_expression_mask(owner.channel);
            if owner.fresh {
                owner.ramp = NoteRamp::new(owner.target, self.law, routes);
                owner.fresh = false;
                owner.dirty = false;
            } else if owner.dirty {
                owner
                    .ramp
                    .retarget(owner.target, self.law, routes, self.rate);
                owner.dirty = false;
            } else {
                owner.ramp.route(routes, fade);
            }
            owner.ramp.limit(Routes::of(instances, mask));
            let (controls, host) = owner.ramp.tick_split();
            if owner.serial > leader.0 {
                leader = (owner.serial, host);
            }
            for (index, kind) in [
                K::Volume,
                K::Pan,
                K::Tuning,
                K::Vibrato,
                K::Expression,
                K::Brightness,
                K::Pressure,
            ]
            .into_iter()
            .enumerate()
            {
                if mask & controls.mask & kind.mask() != 0 {
                    if owner.sent.mask & kind.mask() == 0
                        || owner.sent.values[index] != controls.values[index]
                    {
                        let event = crate::HostEvent::NoteExpression {
                            time: 0,
                            id: owner.id,
                            key: owner.key,
                            channel: owner.channel,
                            kind,
                            value: controls.values[index],
                        };
                        if processor.push_recovery_event(event) {
                            owner.sent.values[index] = controls.values[index];
                            owner.sent.mask |= kind.mask();
                        } else {
                            accepted = false;
                        }
                    }
                } else {
                    owner.sent.mask &= !kind.mask();
                }
            }
        }
        self.gain = if self.law.is_some() {
            let delayed = self.delay.push(leader);
            self.stage.tick(delayed.0, delayed.1)
        } else {
            [1.0; 2]
        };
        accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAWS: [ProjectPanLaw; 4] = [
        ProjectPanLaw::Balance,
        ProjectPanLaw::ConstantPower,
        ProjectPanLaw::Compromise,
        ProjectPanLaw::Linear,
    ];
    const ROUTES: [Routes; 4] = [
        Routes {
            pan: true,
            volume: true,
        },
        Routes {
            pan: true,
            volume: false,
        },
        Routes {
            pan: false,
            volume: true,
        },
        Routes {
            pan: false,
            volume: false,
        },
    ];

    /// What comes out of a plugin that pans by `(1 - p, p)` and scales by its
    /// volume, after the channel's share, with twice that so centre is one.
    /// A property the plugin has no route for is left at its neutral value.
    fn heard(split: (PluginNoteExpression, (f32, f32)), plugin: Routes) -> (f64, f64) {
        let (controls, host) = split;
        let pan = if plugin.pan && controls.mask & K::Pan.mask() != 0 {
            controls.values[1]
        } else {
            0.5
        };
        let volume = if plugin.volume && controls.mask & K::Volume.mask() != 0 {
            controls.values[0]
        } else {
            1.0
        };
        (
            volume * 2.0 * (1.0 - pan) * f64::from(host.0),
            volume * 2.0 * pan * f64::from(host.1),
        )
    }

    #[test]
    fn project_law_and_expression_ramps_follow_the_same_240_frame_clock() {
        for law in LAWS {
            let initial = PluginNoteExpression {
                values: [1.0, 0.825, 0.0, 0.5, 0.5, 0.5, 0.5],
                mask: 0x7f,
            };
            let mut ramp = NoteRamp::new(initial, Some(law), Routes::NATIVE);
            let from = law.gains(0.65);
            let to = ProjectPanLaw::Linear.gains(0.65);
            ramp.retarget(
                initial,
                Some(ProjectPanLaw::Linear),
                Routes::NATIVE,
                48_000.0,
            );
            for frame in 0..=240 {
                assert_eq!(ramp.host(), (1.0, 1.0));
                let controls = ramp.tick();
                let fraction = frame as f32 / 240.0;
                let gains = (
                    from.0 + (to.0 - from.0) * fraction,
                    from.1 + (to.1 - from.1) * fraction,
                );
                let left = controls.values[0] * (1.0 - controls.values[1]) * 2.0;
                let right = controls.values[0] * controls.values[1] * 2.0;
                assert!((left - f64::from(gains.0)).abs() < 2e-6);
                assert!((right - f64::from(gains.1)).abs() < 2e-6);
            }
            let changed = PluginNoteExpression {
                values: [0.25, 0.825, 0.0, 0.5, 0.1, 0.2, 0.3],
                ..initial
            };
            ramp.retarget(
                changed,
                Some(ProjectPanLaw::Linear),
                Routes::NATIVE,
                48_000.0,
            );
            for frame in 0..=240 {
                let controls = ramp.tick();
                let expected = 1.0 + (0.25 - 1.0) * frame as f64 / 240.0;
                assert!((controls.values[0] - expected).abs() < 2e-6);
            }
            assert!(!ramp.active());
        }
    }

    #[test]
    fn all_four_pan_and_volume_routes_give_the_project_law_gain_exactly_once() {
        let mut worst = 0.0_f64;
        for law in LAWS {
            for routes in ROUTES {
                for (pan, volume) in [
                    (-1.0, 1.0),
                    (-0.4, 0.5),
                    (0.0, 1.0),
                    (0.65, 0.25),
                    (1.0, 0.8),
                ] {
                    let controls = PluginNoteExpression {
                        values: [
                            volume,
                            (f64::from(pan) + 1.0) / 2.0,
                            0.0,
                            0.5,
                            0.5,
                            0.5,
                            0.5,
                        ],
                        mask: 0x7f,
                    };
                    let ramp = NoteRamp::new(controls, Some(law), routes);
                    let (sent, host) = ramp.split();
                    // Each property goes one way only.
                    assert_eq!(sent.mask & K::Pan.mask() != 0, routes.pan);
                    assert_eq!(sent.mask & K::Volume.mask() != 0, routes.volume);
                    if routes == Routes::NATIVE {
                        assert_eq!(host, (1.0, 1.0));
                    }
                    let want = law.gains(pan);
                    let got = heard((sent, host), routes);
                    for (got, want) in [(got.0, want.0), (got.1, want.1)] {
                        let want = f64::from(want) * volume;
                        worst = worst.max((got - want).abs());
                        assert!(
                            (got - want).abs() < 2e-6,
                            "{law:?} {routes:?} {pan} {volume}: {got} vs {want}"
                        );
                    }
                    // The channel-level share alone, where nothing is native,
                    // is the project law's gain times the note's volume.
                    if routes == Routes::CHANNEL {
                        let want = law.gains(pan);
                        assert!((f64::from(host.0) - f64::from(want.0) * volume).abs() < 2e-6);
                        assert!((f64::from(host.1) - f64::from(want.1) * volume).abs() < 2e-6);
                        assert_eq!((sent.values[0], sent.values[1]), (1.0, 0.5));
                    }
                }
            }
        }
        println!("ROUTES four-combination worst difference {worst:.3e}");
    }

    #[test]
    fn a_route_change_mid_ramp_keeps_value_target_and_remaining_time_and_never_steps() {
        let rate = 48_000.0;
        let fade = fade_frames(rate);
        let mut worst_step = 0.0_f64;
        let mut own_step = 0.0_f64;
        let mut worst_error = 0.0_f64;
        for law in LAWS {
            for before in ROUTES {
                for after in ROUTES {
                    let start = PluginNoteExpression {
                        values: [1.0, 0.2, 0.0, 0.5, 0.5, 0.5, 0.5],
                        mask: 0x7f,
                    };
                    let end = PluginNoteExpression {
                        values: [0.3, 0.9, 0.0, 0.5, 0.5, 0.5, 0.5],
                        mask: 0x7f,
                    };
                    let mut ramp = NoteRamp::new(start, Some(law), before);
                    ramp.retarget(end, Some(law), before, rate);
                    // A twin that never changes route is the ramp itself.
                    let mut twin = ramp;
                    let mut last = None;
                    for frame in 0..600 {
                        if frame == 100 {
                            let (target, remaining) = ramp.target();
                            let value = ramp.split();
                            ramp.route(after, fade);
                            assert_eq!(ramp.target(), (target, remaining));
                            assert_eq!(remaining, 140);
                            // The frame of the switch itself is unchanged.
                            assert_eq!(ramp.split().0.values, value.0.values);
                            assert_eq!(ramp.split().1, value.1);
                        }
                        // While a weight moves the plugin follows both
                        // properties it is still being sent.
                        let got = heard(ramp.tick_split(), Routes::NATIVE);
                        let want = heard(twin.tick_split(), Routes::NATIVE);
                        for (got, want) in [(got.0, want.0), (got.1, want.1)] {
                            worst_error = worst_error.max((got - want).abs());
                            assert!(
                                (got - want).abs() < 4e-6,
                                "{law:?} {before:?}->{after:?} frame {frame}: {got} vs {want}"
                            );
                        }
                        if let Some((mine, own)) = last {
                            let mine: (f64, f64) = mine;
                            let own: (f64, f64) = own;
                            // What the switch adds to the step the ramp
                            // takes anyway on this frame.
                            let extra = f64::max(
                                ((got.0 - mine.0) - (want.0 - own.0)).abs(),
                                ((got.1 - mine.1) - (want.1 - own.1)).abs(),
                            );
                            worst_step = worst_step.max(extra);
                            own_step = own_step
                                .max((want.0 - own.0).abs())
                                .max((want.1 - own.1).abs());
                        }
                        last = Some((got, want));
                    }
                    assert!(!ramp.active());
                    assert_eq!(ramp.routes(), after);
                    let (sent, _) = ramp.split();
                    assert_eq!(sent.mask & K::Pan.mask() != 0, after.pan);
                    assert_eq!(sent.mask & K::Volume.mask() != 0, after.volume);
                }
            }
        }
        // Sample to sample, a route change adds nothing measurable to the
        // slope the ramp has anyway.
        println!(
            "ROUTES migration added_step={worst_step:.3e} ramp_step={own_step:.5} worst_error={worst_error:.3e}"
        );
        assert!(worst_step < 1e-6, "added step {worst_step}");
        assert!(own_step > 1e-3, "the ramp moves: {own_step}");
    }

    #[test]
    fn a_property_the_plugin_stops_offering_is_the_channels_from_that_frame() {
        // A plugin that loses a capability stops applying it at once, so
        // the channel takes all of it at once: no frame has it twice or
        // not at all. A property it still offers is not disturbed.
        for law in LAWS {
            for lost in [
                Routes {
                    pan: false,
                    volume: true,
                },
                Routes {
                    pan: true,
                    volume: false,
                },
                Routes::CHANNEL,
            ] {
                let controls = PluginNoteExpression {
                    values: [0.6, 0.85, 0.0, 0.5, 0.5, 0.5, 0.5],
                    mask: 0x7f,
                };
                let mut ramp = NoteRamp::new(controls, Some(law), Routes::NATIVE);
                let before = heard(ramp.tick_split(), Routes::NATIVE);
                ramp.limit(lost);
                assert_eq!(ramp.routes(), lost);
                // The plugin now applies only what it still offers.
                let after = heard(ramp.tick_split(), lost);
                assert!((after.0 - before.0).abs() < 2e-6, "{law:?} {lost:?}");
                assert!((after.1 - before.1).abs() < 2e-6, "{law:?} {lost:?}");
                assert!(!ramp.active());
            }
        }
    }

    #[test]
    fn the_channel_stage_follows_the_latest_note_without_stepping_and_fades_around_a_restart() {
        let rate = 48_000.0;
        let mut stage = ChannelStage::new(rate);
        // The first note sets the gains at once; nothing sounded before it.
        assert_eq!(stage.tick(1, (0.25, 0.75)), [0.25, 0.75]);
        assert!(!stage.moving());
        // No note: the last gains hold for the release tail.
        assert_eq!(stage.tick(0, (1.0, 1.0)), [0.25, 0.75]);
        // A newer note takes over across the fade, never in one step.
        let mut last = stage.gains();
        for frame in 0..240 {
            let now = stage.tick(2, (1.0, 0.0));
            assert!(
                (now[0] - last[0]).abs() <= 0.75 / 240.0 + 1e-6,
                "frame {frame}"
            );
            assert!(
                (now[1] - last[1]).abs() <= 0.75 / 240.0 + 1e-6,
                "frame {frame}"
            );
            last = now;
        }
        assert_eq!(last, [1.0, 0.0]);
        assert!(!stage.moving());
        // Departure reaches silence in the fade and arrival comes back from it.
        stage.depart();
        let mut level = 1.0;
        for _ in 0..240 {
            let now = stage.tick(2, (1.0, 0.0))[0];
            assert!(level - now <= 1.0 / 240.0 + 1e-6 && now <= level);
            level = now;
        }
        assert_eq!(level, 0.0);
        assert!(stage.departed());
        let mut next = ChannelStage::new(rate);
        next.inherit(&stage);
        next.arrive();
        let mut level = 0.0;
        for _ in 0..240 {
            let now = next.tick(2, (1.0, 0.0))[0];
            assert!(now - level <= 1.0 / 240.0 + 1e-6 && now >= level);
            level = now;
        }
        assert_eq!(level, 1.0);
    }
}
