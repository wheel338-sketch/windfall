//! Per-voice project panning. Gains are evaluated only on note/control events;
//! the audio loop performs two multiplies and a bounded linear interpolation.
use windfall_core::PanLaw;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NotePan {
    pub position: f32,
    from: (f32, f32),
    target: (f32, f32),
    step: (f32, f32),
    elapsed: u32,
    duration: u32,
}
impl Default for NotePan {
    fn default() -> Self {
        Self::new(0.0, PanLaw::Balance)
    }
}
impl NotePan {
    pub const CENTER: Self = Self {
        position: 0.0,
        from: (1.0, 1.0),
        target: (1.0, 1.0),
        step: (0.0, 0.0),
        elapsed: 0,
        duration: 0,
    };
    pub fn new(position: f32, law: PanLaw) -> Self {
        let position = if position.is_finite() {
            position.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let gains = law.gains(position);
        Self {
            position,
            from: gains,
            target: gains,
            step: (0.0, 0.0),
            elapsed: 0,
            duration: 0,
        }
    }
    #[inline]
    pub fn gains(&self) -> (f32, f32) {
        if self.elapsed >= self.duration {
            return self.target;
        }
        let fraction = self.elapsed as f32;
        (
            self.from.0 + self.step.0 * fraction,
            self.from.1 + self.step.1 * fraction,
        )
    }
    pub fn retarget(&mut self, law: PanLaw, sample_rate: f32) {
        let target = law.gains(self.position);
        if target == self.target {
            return;
        }
        self.from = self.gains();
        self.target = target;
        self.elapsed = 0;
        self.duration = (sample_rate * 0.005).round().max(1.0) as u32;
        self.step = (
            (target.0 - self.from.0) / self.duration as f32,
            (target.1 - self.from.1) / self.duration as f32,
        );
    }
    pub fn set_position(&mut self, position: f32, law: PanLaw, sample_rate: f32) {
        self.position = if position.is_finite() {
            position.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        self.retarget(law, sample_rate);
    }
    pub fn settled(&self) -> bool {
        self.elapsed >= self.duration
    }
    /// The whole ramp as explicit words, for a process that takes the note
    /// over mid-ramp. The layout is part of the plugin bridge protocol.
    pub fn state(&self) -> [u32; Self::STATE_WORDS] {
        [
            self.position.to_bits(),
            self.from.0.to_bits(),
            self.from.1.to_bits(),
            self.target.0.to_bits(),
            self.target.1.to_bits(),
            self.step.0.to_bits(),
            self.step.1.to_bits(),
            self.elapsed,
            self.duration,
        ]
    }
    pub const STATE_WORDS: usize = 9;
    /// A ramp from [`NotePan::state`]. `None` unless every value is finite,
    /// the position is in range and the ramp is no longer than `longest`.
    pub fn from_state(words: [u32; Self::STATE_WORDS], longest: u32) -> Option<Self> {
        let value = |index: usize| f32::from_bits(words[index]);
        let pan = Self {
            position: value(0),
            from: (value(1), value(2)),
            target: (value(3), value(4)),
            step: (value(5), value(6)),
            elapsed: words[7],
            duration: words[8],
        };
        let finite = (0..7).all(|index| value(index).is_finite());
        (finite
            && (-1.0..=1.0).contains(&pan.position)
            && pan.elapsed <= pan.duration
            && pan.duration <= longest)
            .then_some(pan)
    }
    #[inline]
    pub fn tick(&mut self) -> (f32, f32) {
        if self.elapsed >= self.duration {
            return self.target;
        }
        let gains = self.gains();
        self.elapsed = self.elapsed.saturating_add(1).min(self.duration);
        gains
    }
}
