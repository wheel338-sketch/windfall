//! Shared frame-clock project-law/expression ramps. All storage is Copy or
//! prepared on control; neither the desktop nor helper allocates while ticking.
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
pub fn decode_law(code: u32) -> Result<Option<ProjectPanLaw>, crate::bridge::protocol::ProtocolError> {
    Ok(match code {
        0 => None,
        1 => Some(ProjectPanLaw::Balance),
        2 => Some(ProjectPanLaw::ConstantPower),
        3 => Some(ProjectPanLaw::Compromise),
        4 => Some(ProjectPanLaw::Linear),
        _ => return Err(crate::bridge::protocol::ProtocolError::Configuration),
    })
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
    native_law: bool,
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
        native_law: false,
    };
    pub fn new(controls: PluginNoteExpression, law: Option<ProjectPanLaw>, native_law: bool) -> Self {
        Self {
            pan: NotePan::new((controls.values[1] * 2.0 - 1.0) as f32, law.unwrap_or_default()),
            from: controls.values,
            target: controls,
            law,
            native_law,
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
    pub fn retarget(&mut self, controls: PluginNoteExpression, law: Option<ProjectPanLaw>, native_law: bool, rate: f32) {
        if law.is_none() {
            *self = Self::new(controls, law, native_law);
            return;
        }
        if self.target != controls {
            self.from_mask = self.current().mask;
            self.from = self.raw();
            self.target = controls;
            self.elapsed = 0;
            self.duration = (rate * 0.005).round().max(1.0) as u32;
            self.step = std::array::from_fn(|i| (controls.values[i] - self.from[i]) / f64::from(self.duration));
        }
        self.pan.set_position((controls.values[1] * 2.0 - 1.0) as f32, law.unwrap_or_default(), rate);
        self.law = law;
        self.native_law = native_law;
    }
    pub fn current(&self) -> PluginNoteExpression {
        let mask = self.target.mask | if self.elapsed < self.duration { self.from_mask } else { 0 };
        let mut controls = PluginNoteExpression { values: self.raw(), mask };
        if self.law.is_some() && self.native_law {
            let (left, right) = self.pan.gains();
            let sum = left + right;
            controls.values[1] = if sum > 0.0 { f64::from(right / sum) } else { 0.5 };
            controls.values[0] *= f64::from(sum * 0.5);
            controls.mask |= K::Pan.mask() | K::Volume.mask();
        }
        controls
    }
    pub fn tick(&mut self) -> PluginNoteExpression {
        let controls = self.current();
        self.pan.tick();
        self.elapsed = self.elapsed.saturating_add(1).min(self.duration);
        controls
    }
    pub fn active(&self) -> bool {
        !self.pan.settled() || self.elapsed < self.duration
    }
}

pub fn native_law(instances: bool, mask: u32) -> bool {
    instances && mask & (K::Pan.mask() | K::Volume.mask()) == (K::Pan.mask() | K::Volume.mask())
}

#[derive(Clone, Copy)]
struct Owner {
    id: u32,
    key: u8,
    channel: u8,
    target: PluginNoteExpression,
    sent: PluginNoteExpression,
    ramp: NoteRamp,
    fresh: bool,
    dirty: bool,
}
impl Owner {
    const EMPTY: Self = Self { id: 0, key: 0, channel: 0, target: NEUTRAL, sent: NEUTRAL, ramp: NoteRamp::EMPTY, fresh: false, dirty: false };
}
/// Helper-side ownership, allocated once before Ready. Incoming expressions are
/// collected together at an event frame, then one complete target is ramped.
pub struct Bank {
    owners: Box<[Owner]>,
    law: Option<ProjectPanLaw>,
    rate: f32,
    overflow: bool,
}
impl Bank {
    pub fn new(rate: f32) -> Self {
        Self { owners: vec![Owner::EMPTY; crate::MAX_NOTE_INSTANCES].into_boxed_slice(), law: None, rate, overflow: false }
    }
    pub fn reset(&mut self) { self.owners.fill(Owner::EMPTY); self.overflow = false; }
    pub fn set_law(&mut self, law: Option<ProjectPanLaw>) {
        if self.law != law {
            self.law = law;
            for owner in self.owners.iter_mut().filter(|owner| owner.id != 0) { owner.dirty = true; }
        }
    }
    /// Returns true for expressions consumed by the ramp bank. Starts/releases
    /// still reach the native processor through its reserved release path.
    pub fn observe(&mut self, event: crate::HostEvent) -> bool {
        use crate::HostEvent as E;
        match event {
            E::NoteOnInstance { id, key, channel, .. } => {
                let slot = self.owners.iter().position(|owner| owner.id == id)
                    .or_else(|| self.owners.iter().position(|owner| owner.id == 0));
                if let Some(slot) = slot { self.owners[slot] = Owner { id, key, channel, fresh: true, dirty: true, ..Owner::EMPTY }; }
                else { self.overflow = true; }
            }
            E::NoteExpression { id, kind, value, .. } => {
                if let Some(owner) = self.owners.iter_mut().find(|owner| owner.id == id && id != 0) {
                    owner.target.values[kind as usize] = value;
                    owner.target.mask |= kind.mask();
                    owner.dirty = true;
                    return true;
                }
            }
            E::NoteOffInstance { id, .. } => {
                if let Some(owner) = self.owners.iter_mut().find(|owner| owner.id == id) { *owner = Owner::EMPTY; }
            }
            E::AllNotesOff { .. } => self.reset(),
            _ => {}
        }
        false
    }
    pub fn active(&self) -> bool {
        self.owners.iter().any(|owner| owner.id != 0 && (owner.dirty || owner.ramp.active()))
    }
    pub fn frame(&mut self, processor: &mut crate::PluginProcessor) -> bool {
        let instances = processor.supports_note_instances();
        let mut accepted = !self.overflow;
        for owner in self.owners.iter_mut().filter(|owner| owner.id != 0) {
            let mask = processor.note_expression_mask(owner.channel);
            let law = native_law(instances, mask);
            if owner.fresh {
                owner.ramp = NoteRamp::new(owner.target, self.law, law);
                owner.fresh = false;
                owner.dirty = false;
            } else if owner.dirty {
                owner.ramp.retarget(owner.target, self.law, law, self.rate);
                owner.dirty = false;
            }
            let controls = owner.ramp.tick();
            for (index, kind) in [K::Volume, K::Pan, K::Tuning, K::Vibrato, K::Expression, K::Brightness, K::Pressure].into_iter().enumerate() {
                if mask & controls.mask & kind.mask() != 0 {
                    if owner.sent.mask & kind.mask() == 0 || owner.sent.values[index] != controls.values[index] {
                        let event = crate::HostEvent::NoteExpression { time: 0, id: owner.id, key: owner.key, channel: owner.channel, kind, value: controls.values[index] };
                        if processor.push_recovery_event(event) {
                            owner.sent.values[index] = controls.values[index];
                            owner.sent.mask |= kind.mask();
                        } else { accepted = false; }
                    }
                } else { owner.sent.mask &= !kind.mask(); }
            }
        }
        accepted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_law_and_expression_ramps_follow_the_same_240_frame_clock() {
        for law in [ProjectPanLaw::Balance, ProjectPanLaw::ConstantPower, ProjectPanLaw::Compromise, ProjectPanLaw::Linear] {
            let initial = PluginNoteExpression { values: [1.0, 0.825, 0.0, 0.5, 0.5, 0.5, 0.5], mask: 0x7f };
            let mut ramp = NoteRamp::new(initial, Some(law), true);
            let from = law.gains(0.65);
            let to = ProjectPanLaw::Linear.gains(0.65);
            ramp.retarget(initial, Some(ProjectPanLaw::Linear), true, 48_000.0);
            for frame in 0..=240 {
                let controls = ramp.tick();
                let fraction = frame as f32 / 240.0;
                let gains = (from.0 + (to.0 - from.0) * fraction, from.1 + (to.1 - from.1) * fraction);
                let left = controls.values[0] * (1.0 - controls.values[1]) * 2.0;
                let right = controls.values[0] * controls.values[1] * 2.0;
                assert!((left - f64::from(gains.0)).abs() < 2e-6);
                assert!((right - f64::from(gains.1)).abs() < 2e-6);
            }
            let changed = PluginNoteExpression { values: [0.25, 0.825, 0.0, 0.5, 0.1, 0.2, 0.3], ..initial };
            ramp.retarget(changed, Some(ProjectPanLaw::Linear), true, 48_000.0);
            for frame in 0..=240 {
                let controls = ramp.tick();
                let expected = 1.0 + (0.25 - 1.0) * frame as f64 / 240.0;
                assert!((controls.values[0] - expected).abs() < 2e-6);
            }
            assert!(!ramp.active());
        }
    }
}
