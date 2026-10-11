//! A stereo delay: echoes, optionally in time with the song.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blocks::delay_line::DelayLine;
use crate::blocks::lfo::{Lfo, LfoShape};
use crate::blocks::math::{flush, lerp, ms_to_samples};
use crate::blocks::shaper::soft_clip;
use crate::blocks::smooth::{LinearRamp, OnePole};
use crate::blocks::svf::{Svf, SvfCoeffs};
use crate::blocks::{CONTROL_PERIOD, SMOOTHING_MS};
use crate::effect::Effect;
use crate::param::{ParamSet, param_set};

/// Longest delay time, in ms. A synced time longer than this is held here.
const MAX_DELAY_MS: f32 = 4_000.0;

/// Largest offset between the channels, in ms.
const MAX_OFFSET_MS: f32 = 50.0;

/// Largest excursion of the delay-time modulation, in ms.
const MAX_MOD_DEPTH_MS: f32 = 20.0;

/// Length of the crossfade from the old delay time to a new one.
const TIME_FADE_MS: f32 = 30.0;

/// Time the feedback filters take to cover 63% of a cutoff change.
const CUTOFF_SMOOTHING_MS: f32 = 20.0;

/// Tempo assumed until the host sets one.
const DEFAULT_TEMPO_BPM: f32 = 120.0;

/// How many times harder full saturation drives the echoes into the
/// clipper.
const SATURATION_DRIVE: f32 = 3.0;

/// A level this far down counts as silence when working out the tail.
const TAIL_FLOOR: f32 = 1.0e-3;

/// How the two channels' echoes relate.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum DelayMode {
    /// Left echoes stay left and right echoes stay right.
    #[default]
    Stereo,
    /// The input is summed to mono and its echoes bounce from left to
    /// right and back.
    PingPong,
    /// Every echo is on the opposite side to the sound that made it: what
    /// comes in on the left echoes on the right and stays there, and the
    /// other way round. The dry sound keeps its place.
    Inverted,
}

/// A note length for a delay time that follows the tempo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoteDivision {
    Whole,
    HalfDotted,
    Half,
    HalfTriplet,
    QuarterDotted,
    Quarter,
    QuarterTriplet,
    EighthDotted,
    #[default]
    Eighth,
    EighthTriplet,
    SixteenthDotted,
    Sixteenth,
    SixteenthTriplet,
    ThirtySecond,
}

impl NoteDivision {
    /// Length in quarter notes.
    pub fn beats(self) -> f32 {
        match self {
            NoteDivision::Whole => 4.0,
            NoteDivision::HalfDotted => 3.0,
            NoteDivision::Half => 2.0,
            NoteDivision::HalfTriplet => 4.0 / 3.0,
            NoteDivision::QuarterDotted => 1.5,
            NoteDivision::Quarter => 1.0,
            NoteDivision::QuarterTriplet => 2.0 / 3.0,
            NoteDivision::EighthDotted => 0.75,
            NoteDivision::Eighth => 0.5,
            NoteDivision::EighthTriplet => 1.0 / 3.0,
            NoteDivision::SixteenthDotted => 0.375,
            NoteDivision::Sixteenth => 0.25,
            NoteDivision::SixteenthTriplet => 1.0 / 6.0,
            NoteDivision::ThirtySecond => 0.125,
        }
    }
}

/// Settings of the [`Delay`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct DelayParams {
    /// Follow the tempo, using `division`, instead of `time_ms`. Default
    /// on.
    pub sync: bool,
    /// Time between echoes in ms when not synced. 1 to 2000, default 250.
    pub time_ms: f32,
    /// Time between echoes as a note length when synced. Default an eighth
    /// note. Times over 4 seconds, which only very slow tempos reach, are
    /// held at 4 seconds.
    pub division: NoteDivision,
    /// Share of each echo that is fed back to make the next one. 0 gives a
    /// single echo; the maximum of 0.95 gives a very long trail that still
    /// dies away. Default 0.35.
    pub feedback: f32,
    /// Whether echoes keep to their side or bounce between the sides.
    /// Default stereo.
    pub mode: DelayMode,
    /// Makes one side's echoes later than the other's, in ms, which widens
    /// the sound. Positive values delay the right side, negative values the
    /// left. -50 to 50, default 0.
    pub stereo_offset_ms: f32,
    /// Removes lows from each repeat, in Hz, so the trail thins out as it
    /// goes. 20 to 2000, default 20.
    pub low_cut_hz: f32,
    /// Removes highs from each repeat, in Hz, so the trail gets darker as
    /// it goes, like tape. 500 to 20000, default 12000.
    pub high_cut_hz: f32,
    /// Grit added to each repeat. At 0 the echoes are clean; higher values
    /// squash and warm them more with every pass. 0 to 1, default 0.
    pub saturation: f32,
    /// Balance between the dry input (0) and the echoes (1). Default 0.3.
    /// Use 1 on a send track.
    pub mix: f32,
    /// Speed of the delay-time sine LFO, in Hz. 0 to 8, default 0.
    /// Zero holds the LFO at its current phase.
    pub mod_rate_hz: f32,
    /// Peak excursion around the delay time, in ms. 0 to 20, default 0.
    /// Zero keeps the original, unmodulated delay time.
    pub mod_depth_ms: f32,
}

impl Default for DelayParams {
    fn default() -> Self {
        Self {
            sync: true,
            time_ms: 250.0,
            division: NoteDivision::Eighth,
            feedback: 0.35,
            mode: DelayMode::Stereo,
            stereo_offset_ms: 0.0,
            low_cut_hz: 20.0,
            high_cut_hz: 12_000.0,
            saturation: 0.0,
            mix: 0.3,
            mod_rate_hz: 0.0,
            mod_depth_ms: 0.0,
        }
    }
}

param_set!(DelayParams, "Delay", {
    toggle [sync] "sync" "Sync" { true }
    float [time_ms] "timeMs" "Time" { Milliseconds, Logarithmic, 1.0, 2000.0, 250.0 }
    choice [division] "division" "Division" {
        NoteDivision, Eighth,
        [
            Whole "whole" "1/1",
            HalfDotted "halfDotted" "1/2 dotted",
            Half "half" "1/2",
            HalfTriplet "halfTriplet" "1/2 triplet",
            QuarterDotted "quarterDotted" "1/4 dotted",
            Quarter "quarter" "1/4",
            QuarterTriplet "quarterTriplet" "1/4 triplet",
            EighthDotted "eighthDotted" "1/8 dotted",
            Eighth "eighth" "1/8",
            EighthTriplet "eighthTriplet" "1/8 triplet",
            SixteenthDotted "sixteenthDotted" "1/16 dotted",
            Sixteenth "sixteenth" "1/16",
            SixteenthTriplet "sixteenthTriplet" "1/16 triplet",
            ThirtySecond "thirtySecond" "1/32"
        ]
    }
    float [feedback] "feedback" "Feedback" { Fraction, Linear, 0.0, 0.95, 0.35 }
    choice [mode] "mode" "Mode" {
        DelayMode, Stereo, [
            Stereo "stereo" "Stereo",
            PingPong "pingPong" "Ping-pong",
            Inverted "inverted" "Inverted"
        ]
    }
    float [stereo_offset_ms] "stereoOffsetMs" "Stereo offset"
        { Milliseconds, Linear, -50.0, 50.0, 0.0 }
    float [low_cut_hz] "lowCutHz" "Low cut" { Hertz, Logarithmic, 20.0, 2000.0, 20.0 }
    float [high_cut_hz] "highCutHz" "High cut" { Hertz, Logarithmic, 500.0, 20_000.0, 12_000.0 }
    float [saturation] "saturation" "Saturation" { Fraction, Linear, 0.0, 1.0, 0.0 }
    float [mix] "mix" "Mix" { Fraction, Linear, 0.0, 1.0, 0.3 }
    float [mod_rate_hz] "modRateHz" "Modulation rate" { Hertz, Linear, 0.0, 8.0, 0.0 }
    float [mod_depth_ms] "modDepthMs" "Modulation depth"
        { Milliseconds, Linear, 0.0, MAX_MOD_DEPTH_MS, 0.0 }
});

impl DelayParams {
    /// The time between echoes in ms at a tempo, before the stereo offset.
    pub fn time_ms_at(&self, tempo_bpm: f32) -> f32 {
        let time_ms = if self.sync {
            self.division.beats() * 60_000.0 / tempo_bpm.max(1.0)
        } else {
            self.time_ms
        };
        time_ms.clamp(1.0, MAX_DELAY_MS)
    }
}

/// Where one channel's echo is read from the delay line. A new delay time
/// is reached by fading from the old position to the new one, so changing
/// the base time never clicks and never bends the pitch of what is already
/// in the line. Modulation offsets both positions during a fade.
#[derive(Debug, Clone, Copy)]
struct Tap {
    current: usize,
    next: usize,
    /// The newest time asked for. It is taken up when the fade in progress
    /// ends.
    wanted: usize,
    fade_left: u32,
    fade_len: u32,
}

impl Tap {
    fn new() -> Self {
        Self {
            current: 1,
            next: 1,
            wanted: 1,
            fade_left: 0,
            fade_len: 1,
        }
    }

    fn set_target(&mut self, delay: usize, fade_len: u32, snap: bool) {
        self.wanted = delay;
        self.fade_len = fade_len.max(1);
        if snap {
            self.current = delay;
            self.next = delay;
            self.fade_left = 0;
        } else if self.fade_left == 0 && delay != self.current {
            self.next = delay;
            self.fade_left = self.fade_len;
        }
    }

    #[inline]
    fn read(&mut self, line: &DelayLine, modulation: f32) -> f32 {
        let read = |delay: usize| {
            if modulation == 0.0 {
                // Preserve the exact original samples when modulation is off.
                line.tap(delay)
            } else {
                let delay = (delay as f32 + modulation).clamp(1.0, line.max_delay().max(1) as f32);
                // Linear interpolation has unity maximum gain in the feedback loop.
                line.tap_linear(delay)
            }
        };
        if self.fade_left == 0 {
            return read(self.current);
        }
        let old = self.fade_left as f32 / self.fade_len as f32;
        let value = lerp(read(self.next), read(self.current), old);
        self.fade_left -= 1;
        if self.fade_left == 0 {
            self.current = self.next;
            if self.wanted != self.current {
                self.next = self.wanted;
                self.fade_left = self.fade_len;
            }
        }
        value
    }

    /// The longest delay in use right now.
    fn longest(&self) -> usize {
        self.current.max(self.next).max(self.wanted)
    }
}

/// Squashes an echo. At an amount of 0 it does nothing; the slope never
/// exceeds 1, so it cannot make the feedback loop run away.
#[inline]
fn saturate(input: f32, amount: f32) -> f32 {
    if amount == 0.0 {
        return input;
    }
    let drive = 1.0 + SATURATION_DRIVE * amount;
    lerp(input, soft_clip(input * drive) / drive, amount)
}

/// A stereo delay with filters and saturation in its feedback path.
///
/// The first echo is a clean copy of the input. Each later echo has been
/// through the low cut, the high cut and the saturation once more than the
/// one before, so a trail thins, darkens and warms as it fades.
///
/// With modulation off, delay times are whole numbers of samples, so an
/// echo is an exact copy with no interpolation loss. Changing the time,
/// the tempo or the offset crossfades from the old echoes to the new ones
/// over 30 ms; it does not
/// glide in pitch the way a tape delay does. Optional sine modulation
/// moves the read positions continuously with linear interpolation.
///
/// Three modes place the echoes. Stereo keeps each side's echoes on its
/// side. Ping-pong sums the input to mono and bounces it from left to
/// right. Inverted swaps the sides of every echo: a sound on the left
/// echoes on the right, and its repeats stay there.
///
/// Feedback stops at 0.95 and nothing in the loop has a gain above 1, so
/// the delay cannot run away. It adds no latency.
pub struct Delay {
    sample_rate: f32,
    params: DelayParams,
    tempo_bpm: f32,
    lines: [DelayLine; 2],
    taps: [Tap; 2],
    low_cut: [Svf; 2],
    high_cut: [Svf; 2],
    /// Cutoffs as natural logarithms of Hz, so they glide in pitch.
    low_cut_log: OnePole,
    high_cut_log: OnePole,
    low_cut_coeffs: SvfCoeffs,
    high_cut_coeffs: SvfCoeffs,
    feedback: LinearRamp,
    saturation: LinearRamp,
    mix: LinearRamp,
    mod_lfo: Lfo,
    mod_depth_samples: LinearRamp,
    /// 0 for stereo routing, 1 for ping-pong, in between while switching.
    cross: LinearRamp,
    /// 0 while each side feeds its own line, 1 for inverted stereo, where
    /// each side feeds the other's, in between while switching.
    swap: LinearRamp,
    until_control: usize,
    fresh: bool,
}

impl Default for Delay {
    fn default() -> Self {
        let params = DelayParams::default();
        let mut delay = Self {
            sample_rate: 48_000.0,
            params,
            tempo_bpm: DEFAULT_TEMPO_BPM,
            lines: [DelayLine::default(), DelayLine::default()],
            taps: [Tap::new(); 2],
            low_cut: [Svf::default(); 2],
            high_cut: [Svf::default(); 2],
            low_cut_log: OnePole::new(params.low_cut_hz.ln()),
            high_cut_log: OnePole::new(params.high_cut_hz.ln()),
            low_cut_coeffs: SvfCoeffs::new(params.low_cut_hz, 0.707, 48_000.0),
            high_cut_coeffs: SvfCoeffs::new(params.high_cut_hz, 0.707, 48_000.0),
            feedback: LinearRamp::new(0.0),
            saturation: LinearRamp::new(0.0),
            mix: LinearRamp::new(0.0),
            mod_lfo: Lfo::new(1),
            mod_depth_samples: LinearRamp::new(0.0),
            cross: LinearRamp::new(0.0),
            swap: LinearRamp::new(0.0),
            until_control: 0,
            fresh: true,
        };
        delay.apply();
        delay
    }
}

impl Delay {
    /// The left and right delay in samples for the current settings.
    fn delays(&self) -> [usize; 2] {
        let per_ms = 0.001 * self.sample_rate;
        let base = self.params.time_ms_at(self.tempo_bpm) * per_ms;
        let offset = self.params.stereo_offset_ms * per_ms;
        let longest = self.lines[0].max_delay().max(1);
        [base + (-offset).max(0.0), base + offset.max(0.0)]
            .map(|delay| (delay.round() as usize).clamp(1, longest))
    }

    fn apply(&mut self) {
        let (params, rate, snap) = (self.params, self.sample_rate, self.fresh);
        let ramp = if snap {
            0
        } else {
            ms_to_samples(SMOOTHING_MS, rate)
        };
        self.feedback.set_target(params.feedback, ramp);
        self.saturation.set_target(params.saturation, ramp);
        self.mix.set_target(params.mix, ramp);
        self.mod_depth_samples
            .set_target(params.mod_depth_ms * (0.001 * rate), ramp);
        let (cross, swap) = match params.mode {
            DelayMode::Stereo => (0.0, 0.0),
            DelayMode::PingPong => (1.0, 0.0),
            DelayMode::Inverted => (0.0, 1.0),
        };
        self.cross.set_target(cross, ramp);
        self.swap.set_target(swap, ramp);

        let fade = ms_to_samples(TIME_FADE_MS, rate);
        let delays = self.delays();
        for (tap, delay) in self.taps.iter_mut().zip(delays) {
            tap.set_target(delay, fade, snap);
        }

        self.low_cut_log.set_target(params.low_cut_hz.ln());
        self.high_cut_log.set_target(params.high_cut_hz.ln());
        if snap {
            self.low_cut_log.snap(params.low_cut_hz.ln());
            self.high_cut_log.snap(params.high_cut_hz.ln());
            self.update_filters();
        }
    }

    fn update_filters(&mut self) {
        let butterworth = std::f32::consts::FRAC_1_SQRT_2;
        self.low_cut_coeffs = SvfCoeffs::new(
            self.low_cut_log.value().exp(),
            butterworth,
            self.sample_rate,
        );
        self.high_cut_coeffs = SvfCoeffs::new(
            self.high_cut_log.value().exp(),
            butterworth,
            self.sample_rate,
        );
    }

    fn control(&mut self) {
        if !(self.low_cut_log.is_settled() && self.high_cut_log.is_settled()) {
            self.low_cut_log.tick();
            self.high_cut_log.tick();
            self.update_filters();
        }
        for filter in self.low_cut.iter_mut().chain(&mut self.high_cut) {
            filter.flush();
        }
    }

    #[inline]
    fn modulation_samples(&mut self) -> f32 {
        let wave = self
            .mod_lfo
            .tick(LfoShape::Sine, self.params.mod_rate_hz / self.sample_rate);
        wave * self.mod_depth_samples.tick()
    }

    /// Include the current and target depth while depth changes are smoothing.
    fn longest_delay(&self) -> usize {
        let depth = self
            .mod_depth_samples
            .value()
            .max(self.mod_depth_samples.target())
            .ceil() as usize;
        (self.taps[0].longest().max(self.taps[1].longest()) + depth)
            .min(self.lines[0].max_delay().max(1))
    }
}

impl Effect for Delay {
    type Params = DelayParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        self.sample_rate = sample_rate.max(1.0);
        let longest = ms_to_samples(
            MAX_DELAY_MS + MAX_OFFSET_MS + MAX_MOD_DEPTH_MS,
            self.sample_rate,
        ) as usize;
        self.lines = [DelayLine::new(longest), DelayLine::new(longest)];
        let control_rate = self.sample_rate / CONTROL_PERIOD as f32;
        self.low_cut_log.set_time(CUTOFF_SMOOTHING_MS, control_rate);
        self.high_cut_log
            .set_time(CUTOFF_SMOOTHING_MS, control_rate);
        self.reset();
    }

    fn reset(&mut self) {
        self.fresh = true;
        for line in &mut self.lines {
            line.clear();
        }
        for filter in self.low_cut.iter_mut().chain(&mut self.high_cut) {
            filter.reset();
        }
        self.until_control = 0;
        self.mod_lfo.reset();
        self.apply();
    }

    fn set_params(&mut self, params: &DelayParams) {
        self.params = params.sanitized();
        self.apply();
    }

    fn set_tempo(&mut self, bpm: f32) {
        let bpm = if bpm.is_finite() {
            bpm.clamp(10.0, 1_000.0)
        } else {
            DEFAULT_TEMPO_BPM
        };
        if bpm != self.tempo_bpm {
            self.tempo_bpm = bpm;
            self.apply();
        }
    }

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.fresh = false;
        for (left, right) in left.iter_mut().zip(right.iter_mut()) {
            if self.until_control == 0 {
                self.control();
                self.until_control = CONTROL_PERIOD;
            }
            self.until_control -= 1;

            let modulation = self.modulation_samples();
            let echo_left = self.taps[0].read(&self.lines[0], modulation);
            let echo_right = self.taps[1].read(&self.lines[1], modulation);

            let feedback = self.feedback.tick();
            let saturation = self.saturation.tick();
            let mut back = [echo_left, echo_right];
            for (channel, back) in back.iter_mut().enumerate() {
                let filtered = self.low_cut[channel].tick(&self.low_cut_coeffs, *back).high;
                let filtered = self.high_cut[channel]
                    .tick(&self.high_cut_coeffs, filtered)
                    .low;
                *back = saturate(filtered, saturation) * feedback;
            }

            // In ping-pong the left line takes the whole input and the
            // right line's echoes, and the right line takes only the left
            // line's echoes, so the sound bounces between them.
            let cross = self.cross.tick();
            let mono = 0.5 * (*left + *right);
            // Inverted stereo sends each side into the other side's line.
            // The repeats stay in that line, so they stay on that side.
            let swap = self.swap.tick();
            let (own_left, own_right) = if swap == 0.0 {
                // The other modes' samples are untouched by the switch.
                (*left, *right)
            } else {
                (lerp(*left, *right, swap), lerp(*right, *left, swap))
            };
            let into_left = lerp(own_left + back[0], mono + back[1], cross);
            let into_right = lerp(own_right + back[1], back[0], cross);
            self.lines[0].push(flush(into_left));
            self.lines[1].push(flush(into_right));

            let mix = self.mix.tick();
            *left += (echo_left - *left) * mix;
            *right += (echo_right - *right) * mix;
        }
    }

    fn tail_samples(&self) -> usize {
        let feedback = self.params.feedback;
        // Echoes after the first, until they are 60 dB down.
        let repeats = if feedback > 0.0 {
            (TAIL_FLOOR.ln() / feedback.ln()).ceil()
        } else {
            0.0
        };
        let longest = self.longest_delay();
        // Ping-pong takes two hops for a full round trip at the same loss
        // per hop, so the count of hops stays the same.
        longest * (repeats as usize + 2)
    }

    fn gap_samples(&self) -> usize {
        // The wait for the next echo. Every echo is quieter than the one
        // before it, so a silent one is the last.
        self.longest_delay()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn omitted_modulation_fields_default_to_zero() {
        let params: DelayParams = serde_json::from_str(
            r#"{"sync":false,"timeMs":123.0,"division":"eighth","feedback":0.4,
                "mode":"stereo","stereoOffsetMs":5.0,"lowCutHz":40.0,
                "highCutHz":8000.0,"saturation":0.2,"mix":0.7}"#,
        )
        .unwrap();
        assert_eq!(params.mod_rate_hz, 0.0);
        assert_eq!(params.mod_depth_ms, 0.0);
        assert_eq!(params.time_ms, 123.0);
        assert_eq!(params.mix, 0.7);
    }

    #[test]
    fn modulation_parameters_follow_all_existing_automation_indices() {
        let ids: Vec<_> = DelayParams::descriptors()
            .iter()
            .map(|info| info.id)
            .collect();
        assert_eq!(
            ids,
            [
                "sync",
                "timeMs",
                "division",
                "feedback",
                "mode",
                "stereoOffsetMs",
                "lowCutHz",
                "highCutHz",
                "saturation",
                "mix",
                "modRateHz",
                "modDepthMs",
            ]
        );
        let mut params = DelayParams::default();
        assert!(params.set(10, 8.0));
        assert!(params.set(11, 20.0));
        assert_eq!(params.mod_rate_hz, 8.0);
        assert_eq!(params.mod_depth_ms, 20.0);
        let clean = DelayParams {
            mod_rate_hz: f32::NAN,
            mod_depth_ms: f32::INFINITY,
            ..params
        }
        .sanitized();
        assert_eq!(clean.mod_rate_hz, 0.0);
        assert_eq!(clean.mod_depth_ms, 0.0);
    }

    #[test]
    fn zero_depth_preserves_the_unmodulated_delay_time() {
        let rate = 44_100.0;
        for sync in [false, true] {
            for offset in [-3.5, 0.0, 3.5] {
                let params = DelayParams {
                    sync,
                    time_ms: 13.37,
                    stereo_offset_ms: offset,
                    feedback: 0.0,
                    mix: 1.0,
                    mod_rate_hz: 8.0,
                    mod_depth_ms: 0.0,
                    ..DelayParams::default()
                };
                let mut delay = Delay::default();
                delay.prepare(rate, 64);
                delay.set_tempo(137.0);
                delay.set_params(&params);
                let base = params.time_ms_at(137.0);
                let expected = [base + (-offset).max(0.0), base + offset.max(0.0)]
                    .map(|ms| (ms * (0.001 * rate)).round() as usize);
                assert_eq!(delay.delays(), expected);
                let mut left = vec![0.0; expected[0].max(expected[1]) + 10];
                let mut right = left.clone();
                left[0] = 1.0;
                right[0] = 1.0;
                delay.process(&mut left, &mut right);
                for (output, at) in [(&left, expected[0]), (&right, expected[1])] {
                    assert_eq!(output[at], 1.0);
                    assert_eq!(output.iter().filter(|sample| **sample != 0.0).count(), 1);
                }
            }
        }
    }

    #[test]
    fn nonzero_depth_moves_the_echoes_and_stays_finite() {
        for mode in [DelayMode::Stereo, DelayMode::PingPong] {
            let params = DelayParams {
                sync: false,
                time_ms: 50.0,
                stereo_offset_ms: 50.0,
                feedback: 0.95,
                mix: 1.0,
                mode,
                mod_rate_hz: 8.0,
                mod_depth_ms: 20.0,
                ..DelayParams::default()
            };
            let mut moving = Delay::default();
            let mut stationary = Delay::default();
            for delay in [&mut moving, &mut stationary] {
                delay.prepare(1_000.0, 64);
            }
            moving.set_params(&params);
            stationary.set_params(&DelayParams {
                mod_depth_ms: 0.0,
                ..params
            });
            let input: Vec<_> = (0..2_000)
                .map(|n| (std::f32::consts::TAU * 0.037 * n as f32).sin() * 0.25)
                .collect();
            let mut left = input.clone();
            let mut right = input.clone();
            let mut plain_left = input.clone();
            let mut plain_right = input;
            moving.process(&mut left, &mut right);
            stationary.process(&mut plain_left, &mut plain_right);
            assert!(left.iter().chain(&right).all(|sample| sample.is_finite()));
            assert!(
                left.iter()
                    .zip(&plain_left)
                    .any(|(a, b)| (a - b).abs() > 0.01)
            );
            assert_eq!(moving.gap_samples(), stationary.gap_samples() + 20);
        }
    }

    #[test]
    fn modulation_reads_stay_inside_the_line_including_during_time_fades() {
        let mut line = DelayLine::new(64);
        let longest = line.max_delay();
        // Distinguish every valid read from a wrapped position.
        for n in 0..longest + 4 {
            line.push(n as f32);
        }
        let mut tap = Tap::new();
        tap.set_target(1, 4, true);
        for offset in [-1_000.0, -0.5, 0.0, 0.5, 1_000.0] {
            let expected = if offset == 0.0 {
                line.tap(1)
            } else {
                line.tap_linear((1.0_f32 + offset).clamp(1.0, longest as f32))
            };
            assert_eq!(tap.read(&line, offset), expected);
        }
        tap.set_target(longest, 4, false);
        for _ in 0..5 {
            assert_eq!(tap.read(&line, 1_000.0), line.tap(longest));
        }
        assert_eq!(tap.read(&line, -1_000.0), line.tap(1));
    }

    #[test]
    fn zero_rate_holds_the_lfo_and_reset_restarts_it() {
        let mut delay = Delay::default();
        delay.prepare(1_000.0, 64);
        let mut params = DelayParams {
            mod_rate_hz: 1.0,
            mod_depth_ms: 20.0,
            ..DelayParams::default()
        };
        delay.set_params(&params);
        for _ in 0..250 {
            delay.modulation_samples();
        }
        params.mod_rate_hz = 0.0;
        delay.set_params(&params);
        let held = delay.modulation_samples();
        assert!(held > 19.9);
        for _ in 0..1_000 {
            assert_eq!(delay.modulation_samples(), held);
        }
        delay.reset();
        assert_eq!(delay.modulation_samples(), 0.0);
    }

    /// A delay prepared at 44.1 kHz with `params` in force from the start.
    fn prepared(params: &DelayParams) -> Delay {
        let mut delay = Delay::default();
        delay.prepare(44_100.0, 64);
        delay.set_params(params);
        delay
    }

    /// Ten milliseconds, 441 samples at 44.1 kHz, with the feedback filters
    /// as far out of the way as they go.
    fn inverted(feedback: f32) -> DelayParams {
        DelayParams {
            sync: false,
            time_ms: 10.0,
            feedback,
            mode: DelayMode::Inverted,
            high_cut_hz: 20_000.0,
            mix: 1.0,
            ..DelayParams::default()
        }
    }

    #[test]
    fn the_inverted_mode_is_appended_after_the_existing_choices() {
        let index = DelayParams::index_of("mode").unwrap();
        assert_eq!(index, 4);
        let info = DelayParams::descriptors()[index];
        let values: Vec<_> = info.choices.iter().map(|choice| choice.value).collect();
        assert_eq!(values, ["stereo", "pingPong", "inverted"]);
        assert_eq!((info.min, info.max, info.default), (0.0, 2.0, 0.0));

        let mut params = DelayParams::default();
        assert!(params.set(index, 1.0));
        assert_eq!(params.mode, DelayMode::PingPong);
        assert!(params.set(index, 2.0));
        assert_eq!(params.mode, DelayMode::Inverted);
        assert_eq!(params.get(index), Some(2.0));
        // Past the end holds at the last choice, as for every choice.
        assert!(params.set(index, 9.0));
        assert_eq!(params.mode, DelayMode::Inverted);

        let json = serde_json::to_value(params).unwrap();
        assert_eq!(json["mode"], "inverted");
        let loaded: DelayParams = serde_json::from_value(json).unwrap();
        assert_eq!(loaded, params);
        for (stored, mode) in [
            ("stereo", DelayMode::Stereo),
            ("pingPong", DelayMode::PingPong),
        ] {
            let old: DelayParams =
                serde_json::from_str(&format!(r#"{{"mode":"{stored}"}}"#)).unwrap();
            assert_eq!(old.mode, mode);
        }
    }

    #[test]
    fn inverted_echoes_arrive_on_the_other_side_at_the_delay_time() {
        let at = 441;
        for side in 0..2 {
            let mut delay = prepared(&inverted(0.0));
            let mut channels = [vec![0.0_f32; 3 * at], vec![0.0_f32; 3 * at]];
            channels[side][0] = 1.0;
            {
                let [left, right] = &mut channels;
                delay.process(left, right);
            }
            let (same, other) = (&channels[side], &channels[1 - side]);
            assert!(same.iter().all(|sample| *sample == 0.0), "side {side}");
            assert_eq!(other[at], 1.0, "side {side}");
            assert_eq!(other.iter().filter(|sample| **sample != 0.0).count(), 1);
        }
    }

    #[test]
    fn inverted_repeats_stay_on_the_swapped_side_and_decay() {
        let at = 441;
        let mut delay = prepared(&inverted(0.5));
        let mut left = vec![0.0_f32; 8 * at];
        let mut right = left.clone();
        left[0] = 1.0;
        delay.process(&mut left, &mut right);
        // Nothing ever comes back on the side the sound came in on.
        assert!(left.iter().all(|sample| *sample == 0.0));
        assert_eq!(right[at], 1.0);
        // Independent bilinear Butterworth biquads, in direct form rather
        // than the processor's SVF state representation. IIR tails overlap
        // successive repeat windows, so their energies need not have an
        // exact <= feedback^2 ratio even though the loop is contractive.
        fn filter(hz: f64, high: bool) -> ([f64; 3], [f64; 2]) {
            let g = (std::f64::consts::PI * hz / 44_100.0).tan();
            let k = std::f64::consts::SQRT_2;
            let norm = 1.0 / (1.0 + k * g + g * g);
            let b = if high { norm } else { g * g * norm };
            (
                [b, if high { -2.0 * b } else { 2.0 * b }, b],
                [2.0 * (g * g - 1.0) * norm, (1.0 - k * g + g * g) * norm],
            )
        }
        fn step(input: f64, coeffs: ([f64; 3], [f64; 2]), state: &mut [f64; 4]) -> f64 {
            let (b, a) = coeffs;
            let output = b[0] * input + b[1] * state[0] + b[2] * state[1]
                - a[0] * state[2]
                - a[1] * state[3];
            *state = [input, state[0], output, state[2]];
            output
        }
        let high = filter(20.0, true);
        let low = filter(20_000.0, false);
        let mut states = [[0.0; 4]; 2];
        let mut history = vec![0.0; right.len()];
        for n in 0..right.len() {
            let echo = if n >= at { history[n - at] } else { 0.0 };
            let back = step(step(echo, high, &mut states[0]), low, &mut states[1]);
            history[n] = f64::from(n == 0) + 0.5 * back;
            assert!((f64::from(right[n]) - echo).abs() < 1e-5, "sample {n}");
        }
        let energy = |repeat: usize| -> f32 {
            let from = repeat * at - at / 2;
            right[from..from + at].iter().map(|x| x * x).sum()
        };
        let mut previous = energy(1);
        for repeat in 2..7 {
            let now = energy(repeat);
            assert!(
                now > 0.12 * previous && now < previous,
                "repeat {repeat}: {now} after {previous}"
            );
            previous = now;
        }
    }

    #[test]
    fn stereo_and_ping_pong_still_keep_their_sides() {
        let at = 441;
        let mut stereo = prepared(&DelayParams {
            mode: DelayMode::Stereo,
            ..inverted(0.5)
        });
        let mut left = vec![0.0_f32; 4 * at];
        let mut right = left.clone();
        left[0] = 1.0;
        stereo.process(&mut left, &mut right);
        assert_eq!(left[at], 1.0);
        assert!(right.iter().all(|sample| *sample == 0.0));

        let mut bounce = prepared(&DelayParams {
            mode: DelayMode::PingPong,
            ..inverted(0.5)
        });
        let mut left = vec![0.0_f32; 4 * at];
        let mut right = left.clone();
        left[0] = 1.0;
        bounce.process(&mut left, &mut right);
        // Half the left impulse is the mono sum; it lands left, then right.
        assert_eq!(left[at], 0.5);
        assert_eq!(right[at], 0.0);
        assert!(right[2 * at - 8..2 * at + 8].iter().any(|x| x.abs() > 0.05));
    }

    #[test]
    fn switching_to_inverted_glides_and_does_not_depend_on_the_blocks() {
        const FRAMES: usize = 8_820;
        const SWITCH: usize = 4_410;
        let input: Vec<f32> = (0..FRAMES)
            .map(|n| (std::f32::consts::TAU * 220.0 * n as f32 / 44_100.0).sin() * 0.5)
            .collect();
        let start = DelayParams {
            mode: DelayMode::Stereo,
            time_ms: 1.0,
            ..inverted(0.3)
        };
        let run = |sizes: &[usize]| {
            let mut delay = prepared(&start);
            let mut left = input.clone();
            let mut right = vec![0.0_f32; FRAMES];
            let (mut at, mut turn) = (0, 0);
            while at < FRAMES {
                if at == SWITCH {
                    delay.set_params(&DelayParams {
                        mode: DelayMode::Inverted,
                        ..start
                    });
                }
                let stop = if at < SWITCH { SWITCH } else { FRAMES };
                let to = (at + sizes[turn % sizes.len()]).min(stop);
                delay.process(&mut left[at..to], &mut right[at..to]);
                at = to;
                turn += 1;
            }
            (left, right)
        };
        let (left, right) = run(&[FRAMES]);
        let (pieces_left, pieces_right) = run(&[1, 64, 7, 333, 16]);
        assert_eq!(left, pieces_left);
        assert_eq!(right, pieces_right);
        // Before the switch the echoes are on the left and the right is
        // silent; long after it they have changed sides.
        assert!(right[..SWITCH].iter().all(|sample| *sample == 0.0));
        assert!(left[2_000..SWITCH].iter().any(|x| x.abs() > 0.2));
        assert!(right[7_000..].iter().any(|x| x.abs() > 0.2));
        assert!(left[7_000..].iter().all(|x| x.abs() < 1.0e-3));
        // The 220 Hz input moves at most 0.5 * 2 * pi * 220 / 44100 = 0.016
        // a sample. A switch that jumped would step by up to half a unit.
        for side in [&left, &right] {
            for pair in side[SWITCH - 10..6_000].windows(2) {
                assert!((pair[1] - pair[0]).abs() < 0.03);
            }
        }
    }

    #[test]
    fn inverted_keeps_the_stereo_offset_and_the_tail_of_the_other_modes() {
        let params = DelayParams {
            stereo_offset_ms: 4.0,
            ..inverted(0.0)
        };
        // 10 ms on the left line and 14 ms on the right: 441 and 617 samples.
        let (base, late) = (441, 617);
        let mut delay = prepared(&params);
        let mut left = vec![0.0_f32; 2 * late];
        let mut right = left.clone();
        left[0] = 1.0;
        right[0] = -1.0;
        delay.process(&mut left, &mut right);
        // The right input comes back on the left, on the left line's time.
        assert_eq!(left[base], -1.0);
        assert_eq!(right[late], 1.0);
        assert_eq!(left.iter().filter(|sample| **sample != 0.0).count(), 1);
        assert_eq!(right.iter().filter(|sample| **sample != 0.0).count(), 1);
        let stereo = prepared(&DelayParams {
            mode: DelayMode::Stereo,
            ..params
        });
        assert_eq!(delay.tail_samples(), stereo.tail_samples());
        assert_eq!(delay.gap_samples(), stereo.gap_samples());
    }

    #[test]
    fn divisions_are_the_right_number_of_beats() {
        assert_eq!(NoteDivision::Whole.beats(), 4.0);
        assert_eq!(NoteDivision::Quarter.beats(), 1.0);
        assert_eq!(NoteDivision::EighthDotted.beats(), 0.75);
        assert!((NoteDivision::QuarterTriplet.beats() * 3.0 - 2.0).abs() < 1e-6);
        assert_eq!(NoteDivision::ThirtySecond.beats(), 0.125);
    }

    #[test]
    fn synced_time_follows_the_tempo_and_stops_at_the_longest_delay() {
        let mut params = DelayParams {
            sync: true,
            division: NoteDivision::Quarter,
            ..DelayParams::default()
        };
        assert_eq!(params.time_ms_at(120.0), 500.0);
        assert_eq!(params.time_ms_at(60.0), 1_000.0);
        params.division = NoteDivision::Whole;
        assert_eq!(params.time_ms_at(20.0), MAX_DELAY_MS);
        params.sync = false;
        params.time_ms = 333.0;
        assert_eq!(params.time_ms_at(20.0), 333.0);
    }

    #[test]
    fn saturation_never_amplifies_and_is_off_at_zero() {
        assert_eq!(saturate(0.8, 0.0), 0.8);
        let mut previous = saturate(-4.0, 1.0);
        for step in -399..=400 {
            let x = step as f32 / 100.0;
            for amount in [0.2, 0.6, 1.0] {
                assert!(saturate(x, amount).abs() <= x.abs() + 1e-6);
            }
            let value = saturate(x, 1.0);
            assert!(value >= previous - 1e-6 && value - previous <= 0.01 + 1e-5);
            previous = value;
        }
    }

    #[test]
    fn a_tap_fades_to_the_newest_time_asked_for() {
        let mut line = DelayLine::new(64);
        for n in 0..64 {
            line.push(n as f32);
        }
        let mut tap = Tap::new();
        tap.set_target(10, 4, true);
        assert_eq!(tap.read(&line, 0.0), 54.0);
        tap.set_target(20, 4, false);
        tap.set_target(30, 4, false);
        // Four samples of fade to 20, then four more to 30.
        let values: Vec<f32> = (0..9).map(|_| tap.read(&line, 0.0)).collect();
        assert_eq!(values[0], 54.0);
        assert!(values[1] < 54.0 && values[1] > 44.0);
        assert_eq!(values[4], 44.0);
        assert_eq!(values[8], 34.0);
        assert_eq!(tap.longest(), 30);
    }
}
