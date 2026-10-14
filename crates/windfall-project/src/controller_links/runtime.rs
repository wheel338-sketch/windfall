//! What a link does with a source value: shaping, relative steps, pickup
//! and smoothing. Pure and clock free, so every consumer drives it with its
//! own time and every case can be tested exactly.

use super::formula::Formula;
use super::{ControlMap, MAX_SMOOTHING_MS, PickupMode, SourceMode};

/// How near the hardware has to come to the target's value for pickup to
/// take hold without crossing it: a little over one step of a seven-bit
/// controller.
pub const PICKUP_WINDOW: f32 = 1.5 / 127.0;
/// How far the target may sit from what a link last gave it before the
/// link takes it to have been moved by something else.
const EXTERNAL_CHANGE: f32 = 1e-4;
/// A glide this close to its goal lands on it.
const SETTLED: f32 = 1e-4;

/// A [`ControlMap`] ready to run: its formula parsed, its numbers checked.
/// `Copy` and allocation free.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompiledMap {
    pub min: f32,
    pub max: f32,
    pub formula: Option<Formula>,
    pub smoothing_seconds: f32,
    pub pickup: PickupMode,
    pub mode: SourceMode,
}

impl ControlMap {
    /// Checks the map and parses its formula. The message of a failure can
    /// be shown to the user.
    pub fn compile(&self) -> Result<CompiledMap, String> {
        let unit = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
        if !unit(self.min) || !unit(self.max) {
            return Err("A controller link's range runs from 0 to 1.".to_owned());
        }
        if !self.smoothing_ms.is_finite() || !(0.0..=MAX_SMOOTHING_MS).contains(&self.smoothing_ms)
        {
            return Err(windfall_core::notice!(
                "controllers.links.smoothing",
                max = MAX_SMOOTHING_MS
            )
            .to_string());
        }
        let formula = match self.formula.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(text) => Some(Formula::parse(text)?),
        };
        Ok(CompiledMap {
            min: self.min,
            max: self.max,
            formula,
            smoothing_seconds: self.smoothing_ms / 1000.0,
            pickup: self.pickup,
            mode: self.mode,
        })
    }
}

impl CompiledMap {
    /// The target value, 0 to 1, for the source value `x`, 0 to 1: the
    /// formula first, then the range. Never allocates.
    pub fn shape(&self, x: f32) -> f32 {
        let x = if x.is_finite() {
            x.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let shaped = self.formula.map_or(x, |formula| formula.eval(x));
        self.min + (self.max - self.min) * shaped
    }

    fn bounds(&self) -> (f32, f32) {
        (self.min.min(self.max), self.min.max(self.max))
    }
}

/// One message from a source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceInput {
    /// The position, 0 to 1. A key or pad gives 1 while down and 0 when up.
    pub value: f32,
    /// The seven data bits as they came, which is what an endless knob
    /// encodes its steps in.
    pub raw: u8,
}

impl SourceInput {
    /// A seven-bit controller value.
    pub fn seven_bit(raw: u8) -> Self {
        let raw = raw.min(127);
        Self {
            value: f32::from(raw) / 127.0,
            raw,
        }
    }

    /// A fourteen-bit value such as the pitch wheel's.
    pub fn fourteen_bit(raw: u16) -> Self {
        let raw = raw.min(16_383);
        Self {
            value: f32::from(raw) / 16_383.0,
            raw: (raw >> 7) as u8,
        }
    }

    /// A key or pad going down or coming up.
    pub fn button(down: bool) -> Self {
        Self {
            value: f32::from(down),
            raw: if down { 127 } else { 0 },
        }
    }
}

impl SourceMode {
    /// The steps an endless knob's message stands for. 0 for anything else.
    pub fn steps(self, raw: u8) -> i32 {
        let raw = i32::from(raw & 0x7f);
        match self {
            SourceMode::RelativeTwosComplement => {
                if raw < 64 {
                    raw
                } else {
                    raw - 128
                }
            }
            SourceMode::RelativeSignedBit => {
                if raw & 0x40 == 0 {
                    raw
                } else {
                    -(raw & 0x3f)
                }
            }
            SourceMode::RelativeBinaryOffset => raw - 64,
            SourceMode::Absolute | SourceMode::Toggle => 0,
        }
    }

    pub fn is_relative(self) -> bool {
        matches!(
            self,
            SourceMode::RelativeTwosComplement
                | SourceMode::RelativeSignedBit
                | SourceMode::RelativeBinaryOffset
        )
    }
}

/// What a link wants done after a message or a step of time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Motion {
    /// Nothing to do.
    None,
    /// Pickup is waiting: the hardware has not reached the target's value.
    Held,
    /// Give the target this value, 0 to 1, now. A glide reports its steps
    /// this way from [`LinkState::advance`].
    Write(f32),
}

/// What one link remembers between messages.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LinkState {
    /// Where the hardware last was, mapped, whether or not it took hold.
    last_shaped: Option<f32>,
    picked_up: bool,
    /// What this link last gave the target, as the target then stored it.
    written: Option<f32>,
    /// Where a glide is heading.
    goal: Option<f32>,
    /// Where a glide is.
    value: f32,
    /// A button's last state, for finding the moment it is pressed.
    down: bool,
}

impl LinkState {
    /// True while pickup has this link waiting for the hardware.
    pub fn waiting(&self) -> bool {
        self.last_shaped.is_some() && !self.picked_up
    }

    /// True while a glide still has somewhere to go.
    pub fn gliding(&self) -> bool {
        self.goal.is_some()
    }

    /// True when the message is a button going from up to down: the moment
    /// an action runs. Tracks the button's state, so call it once a message.
    pub fn pressed(&mut self, input: SourceInput) -> bool {
        let down = input.value >= 0.5;
        let pressed = down && !self.down;
        self.down = down;
        pressed
    }

    /// Tells the link what the target stored for the value it was given.
    /// A stepped target rounds, so this is not always the value itself.
    pub fn wrote(&mut self, stored: f32) {
        self.written = Some(stored);
    }

    /// Takes a message. `current` is the target's value now, 0 to 1, and
    /// `pickup_default` is what [`PickupMode::Inherit`] means.
    ///
    /// A link that smooths answers [`Motion::None`] and starts a glide,
    /// which [`advance`](Self::advance) carries out.
    pub fn input(
        &mut self,
        map: &CompiledMap,
        input: SourceInput,
        current: f32,
        pickup_default: bool,
    ) -> Motion {
        let current = if current.is_finite() {
            current.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (low, high) = map.bounds();
        let goal = match map.mode {
            SourceMode::Toggle => {
                if !self.pressed(input) {
                    return Motion::None;
                }
                // Whichever end the target is nearer to, go to the other.
                let at_max = (current - map.max).abs() < (current - map.min).abs();
                self.goal = None;
                return Motion::Write(if at_max { map.min } else { map.max });
            }
            mode if mode.is_relative() => {
                let steps = mode.steps(input.raw);
                if steps == 0 {
                    return Motion::None;
                }
                // A turn carries on from where a glide is heading, so quick
                // turns add up instead of each starting from a stale value.
                let from = self.goal.unwrap_or(current);
                let step = (map.max - map.min) / 127.0;
                (from + steps as f32 * step).clamp(low, high)
            }
            _ => {
                let shaped = map.shape(input.value);
                let pickup = match map.pickup {
                    PickupMode::Inherit => pickup_default,
                    PickupMode::On => true,
                    PickupMode::Off => false,
                };
                if pickup {
                    let moved_elsewhere = self
                        .written
                        .is_none_or(|written| (current - written).abs() > EXTERNAL_CHANGE);
                    if moved_elsewhere && !self.gliding() {
                        self.picked_up = false;
                    }
                    if !self.picked_up {
                        let near = (shaped - current).abs() <= PICKUP_WINDOW;
                        let crossed = self
                            .last_shaped
                            .is_some_and(|last| (last - current) * (shaped - current) <= 0.0);
                        self.last_shaped = Some(shaped);
                        if !(near || crossed) {
                            return Motion::Held;
                        }
                        self.picked_up = true;
                    }
                } else {
                    self.picked_up = true;
                }
                self.last_shaped = Some(shaped);
                shaped
            }
        };
        if map.smoothing_seconds > 0.0 {
            if self.goal.is_none() {
                self.value = current;
            }
            self.goal = Some(goal);
            return Motion::None;
        }
        self.goal = None;
        if (goal - current).abs() <= f32::EPSILON {
            // Already there; still the link's value, so pickup keeps hold.
            self.written = Some(current);
            return Motion::None;
        }
        Motion::Write(goal)
    }

    /// Moves a glide on by `seconds`. Answers the value to give the target,
    /// or [`Motion::None`] when no glide is running.
    pub fn advance(&mut self, map: &CompiledMap, seconds: f32) -> Motion {
        let Some(goal) = self.goal else {
            return Motion::None;
        };
        if seconds.is_nan() || seconds <= 0.0 {
            return Motion::None;
        }
        let share = if map.smoothing_seconds > 0.0 {
            1.0 - (-seconds / map.smoothing_seconds).exp()
        } else {
            1.0
        };
        self.value += (goal - self.value) * share;
        if (goal - self.value).abs() <= SETTLED {
            self.value = goal;
            self.goal = None;
        }
        Motion::Write(self.value)
    }

    /// Forgets everything, as when the link's target or map changes.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
