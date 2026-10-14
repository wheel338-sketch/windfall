//! A delay line that can be read at whole or fractional delays.

/// A ring of past samples.
///
/// Delays count pushes: a delay of 1 is the sample pushed last. To delay a
/// signal by `d` samples, read `tap(d)` and then push the new input.
///
/// A delay longer than [`DelayLine::max_delay`] wraps around and returns
/// the wrong sample instead of panicking, so callers clamp their delays.
#[derive(Debug, Clone)]
pub struct DelayLine {
    buffer: Box<[f32]>,
    mask: usize,
    /// Where the next push goes.
    write: usize,
}

/// Samples past the longest delay that the cubic read needs.
const GUARD: usize = 4;

impl Default for DelayLine {
    /// A line too short to be useful, which holds no memory worth
    /// mentioning. Processors hold one of these until they are prepared.
    fn default() -> Self {
        Self::new(0)
    }
}

impl DelayLine {
    /// Allocates a line that can delay by up to `max_delay` samples.
    pub fn new(max_delay: usize) -> Self {
        let length = (max_delay + GUARD).next_power_of_two();
        Self {
            buffer: vec![0.0; length].into_boxed_slice(),
            mask: length - 1,
            write: 0,
        }
    }

    /// The longest delay that reads back correctly.
    pub fn max_delay(&self) -> usize {
        self.buffer.len() - GUARD
    }

    /// How many samples the ring holds: the longest delay and its guard.
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Silences the line without freeing or allocating.
    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }

    #[inline]
    pub fn push(&mut self, sample: f32) {
        self.buffer[self.write] = sample;
        self.write = (self.write + 1) & self.mask;
    }

    /// The sample pushed `delay` pushes ago. `delay` is at least 1.
    #[inline]
    pub fn tap(&self, delay: usize) -> f32 {
        self.buffer[self.write.wrapping_sub(delay) & self.mask]
    }

    /// Reads at a fractional delay of at least 1 by drawing a straight line
    /// between the two nearest samples. Cheap, but it dulls the top octave
    /// when the fraction is near one half, so keep it out of long feedback
    /// loops.
    #[inline]
    pub fn tap_linear(&self, delay: f32) -> f32 {
        let whole = delay as usize;
        let fraction = delay - whole as f32;
        let near = self.tap(whole);
        let far = self.tap(whole + 1);
        near + (far - near) * fraction
    }

    /// Reads at a fractional delay of at least 2 through a cubic that passes
    /// through the four nearest samples (a Catmull-Rom spline). It loses far
    /// less treble than [`DelayLine::tap_linear`] and has no kinks, which
    /// makes it the one to use for a delay that is being modulated.
    #[inline]
    pub fn tap_cubic(&self, delay: f32) -> f32 {
        let whole = delay as usize;
        let t = delay - whole as f32;
        let y0 = self.tap(whole.wrapping_sub(1));
        let y1 = self.tap(whole);
        let y2 = self.tap(whole + 1);
        let y3 = self.tap(whole + 2);
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * t + c2) * t + c1) * t + y1
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::TAU;

    use super::*;

    #[test]
    fn whole_delays_return_the_exact_sample() {
        let mut line = DelayLine::new(100);
        assert!(line.max_delay() >= 100);
        for n in 0..1_000 {
            let input = n as f32;
            for delay in [1, 2, 37, 100] {
                if n >= delay {
                    assert_eq!(line.tap(delay), (n - delay) as f32);
                }
            }
            line.push(input);
        }
    }

    #[test]
    fn fractional_reads_agree_with_whole_ones_on_whole_delays() {
        let mut line = DelayLine::new(64);
        for n in 0..200 {
            line.push(((n * 7) % 13) as f32);
        }
        for delay in [2_usize, 5, 40] {
            assert_eq!(line.tap_linear(delay as f32), line.tap(delay));
            assert_eq!(line.tap_cubic(delay as f32), line.tap(delay));
        }
    }

    #[test]
    fn linear_read_is_exact_on_a_ramp() {
        let mut line = DelayLine::new(64);
        for n in 0..100 {
            line.push(n as f32);
        }
        // The last sample pushed is 99, so a delay of 10.25 reads 89.75.
        assert!((line.tap_linear(10.25) - 89.75).abs() < 1e-4);
        assert!((line.tap_cubic(10.25) - 89.75).abs() < 1e-4);
    }

    /// Worst error when reading a sine at a fractional delay.
    fn sine_error(frequency: f32, cubic: bool) -> f32 {
        let rate = 48_000.0;
        let mut line = DelayLine::new(64);
        let mut worst = 0.0_f32;
        for n in 0..2_000 {
            let delay = 20.37;
            if n > 100 {
                let expected = (TAU * frequency * (n as f32 - delay) / rate).sin();
                let read = if cubic {
                    line.tap_cubic(delay)
                } else {
                    line.tap_linear(delay)
                };
                worst = worst.max((read - expected).abs());
            }
            line.push((TAU * frequency * n as f32 / rate).sin());
        }
        worst
    }

    #[test]
    fn cubic_read_is_more_accurate_than_linear() {
        assert!(sine_error(1_000.0, false) < 2.0e-3);
        assert!(sine_error(1_000.0, true) < 1.0e-4);
        let (linear, cubic) = (sine_error(6_000.0, false), sine_error(6_000.0, true));
        assert!(cubic < linear * 0.5, "{cubic} against {linear}");
    }

    #[test]
    fn clearing_silences_the_line() {
        let mut line = DelayLine::new(16);
        for _ in 0..40 {
            line.push(1.0);
        }
        line.clear();
        for delay in 1..=16 {
            assert_eq!(line.tap(delay), 0.0);
        }
    }

    #[test]
    fn an_unprepared_line_reads_without_panicking() {
        let mut line = DelayLine::default();
        line.push(1.0);
        let _ = line.tap(5_000);
        let _ = line.tap_cubic(9_000.5);
    }
}
