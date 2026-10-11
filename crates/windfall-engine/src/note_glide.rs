//! A pitch transition measured on the sequencer's musical clock.

#[derive(Debug, Clone, Copy)]
pub(crate) struct NoteGlide {
    pub start: f64,
    pub end: f64,
    pub from: f64,
    pub to: f64,
}

impl NoteGlide {
    pub fn new(start: f64, end: f64, from: f64, to: f64) -> Self {
        Self {
            start,
            end: end.max(start),
            from,
            to,
        }
    }

    pub fn at(self, tick: f64) -> f64 {
        let fraction = if self.end <= self.start {
            1.0
        } else {
            ((tick - self.start) / (self.end - self.start)).clamp(0.0, 1.0)
        };
        self.from + (self.to - self.from) * fraction
    }

    pub fn shift(&mut self, ticks: f64) {
        self.start += ticks;
        self.end += ticks;
    }
    pub fn move_clock(&mut self, moved: impl Fn(f64) -> f64) {
        self.start = moved(self.start);
        self.end = moved(self.end);
    }
}
