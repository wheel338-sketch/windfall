//! Automation: how a curve's points become a value, and how that value
//! maps onto what it moves.
//!
//! Everything here is a pure function of its arguments. The engine calls
//! these while it plays and a UI calls them to draw a curve in real units,
//! so both always agree on what a point means.
//!
//! # From points to a value
//!
//! [`curve_value`] reads a curve at a tick. Before the first point the
//! curve has the first point's value and after the last it has the last
//! point's. Between two points the value moves from one to the other,
//! shaped by the `curve` of the point it leaves ([`curve_shape`]), or
//! stays on the first point's value until the next point when that point
//! is a `hold`.
//!
//! # From a value to the target
//!
//! A point's value runs from 0 to 1. [`AutomationRange::value`] turns it
//! into the target's own unit, and [`AutomationRange::normalized`] goes
//! the other way. [`Project::automation_range`] gives the range of a
//! target:
//!
//! | Target | Range | Mapping |
//! |---|---|---|
//! | `channelVolume`, `trackVolume`, `sendGain` | linear gain 0 to 2 | square: `2 * n * n`, so 0 is silence, 0.7071 is 0 dB and 1 is +6 dB |
//! | `channelPan`, `trackPan` | -1 to 1 | linear: `2 * n - 1` |
//! | `effectMix` | 0 to 1 | linear |
//! | `tempo` | 10 to 522 bpm | linear: `10 + 512 * n` |
//! | `effectParam`, `instrumentParam` | the descriptor's `min` to `max` | by the descriptor: linear, or logarithmic (`min * (max / min)^n`) where its scale says so; whole numbers and choices round to the nearest step, and a toggle is on from 0.5 up |

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_dsp::{ParamInfo, ParamKind, ParamScale};

use crate::model::{
    AutomationPoint, AutomationTarget, ChannelSource, MAX_GAIN, MAX_TEMPO_BPM, MIN_TEMPO_BPM,
    Project,
};

/// How strongly a `curve` of 1 bends a segment. See [`curve_shape`].
const CURVE_STRENGTH: f64 = 6.0;

/// How the values 0 to 1 of an automation spread over a target's range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AutomationTaper {
    /// Equal steps of the value: `min + n * (max - min)`.
    Linear,
    /// Equal ratios of the value: `min * (max / min)^n`. The minimum is
    /// above zero.
    Logarithmic,
    /// For gains, which start at zero: `max * n * n`. Half way up is a
    /// quarter of the gain, 12 dB down.
    Square,
    /// Whole numbers: linear, rounded to the nearest.
    Stepped,
    /// Off below 0.5, on from there up.
    Toggle,
}

/// The range of what an automation moves, in the target's own unit, and
/// how the automation's values 0 to 1 map onto it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AutomationRange {
    /// The value at 0.
    pub min: f32,
    /// The value at 1.
    pub max: f32,
    pub taper: AutomationTaper,
}

impl AutomationRange {
    /// A channel volume, a mixer fader or a send level: linear gain from
    /// silence to [`MAX_GAIN`], on a square taper.
    pub const GAIN: Self = Self {
        min: 0.0,
        max: MAX_GAIN,
        taper: AutomationTaper::Square,
    };

    /// A pan: -1 is hard left and 1 is hard right.
    pub const PAN: Self = Self {
        min: -1.0,
        max: 1.0,
        taper: AutomationTaper::Linear,
    };

    /// The mix of an effect slot: 0 is the untouched signal, 1 the effect.
    pub const MIX: Self = Self {
        min: 0.0,
        max: 1.0,
        taper: AutomationTaper::Linear,
    };

    /// The tempo in beats per minute, [`MIN_TEMPO_BPM`] to
    /// [`MAX_TEMPO_BPM`].
    pub const TEMPO: Self = Self {
        min: MIN_TEMPO_BPM as f32,
        max: MAX_TEMPO_BPM as f32,
        taper: AutomationTaper::Linear,
    };

    /// The range of a setting of an effect or instrument, from its
    /// descriptor.
    pub fn of_param(info: &ParamInfo) -> Self {
        let taper = match (info.kind, info.scale) {
            (ParamKind::Toggle, _) => AutomationTaper::Toggle,
            (ParamKind::Integer | ParamKind::Choice, _) => AutomationTaper::Stepped,
            (ParamKind::Float, ParamScale::Logarithmic) if info.min > 0.0 => {
                AutomationTaper::Logarithmic
            }
            (ParamKind::Float, _) => AutomationTaper::Linear,
        };
        Self {
            min: info.min,
            max: info.max,
            taper,
        }
    }

    /// The target's value for an automation value of `normalized`, which is
    /// held to 0 to 1 first. Anything that is not a number counts as 0.
    pub fn value(&self, normalized: f32) -> f32 {
        let n = if normalized.is_finite() {
            normalized.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (min, max) = (self.min, self.max);
        match self.taper {
            AutomationTaper::Linear => min + n * (max - min),
            AutomationTaper::Logarithmic => min * (max / min).powf(n),
            AutomationTaper::Square => min + n * n * (max - min),
            AutomationTaper::Stepped => (min + n * (max - min)).round(),
            AutomationTaper::Toggle => {
                if n >= 0.5 {
                    max
                } else {
                    min
                }
            }
        }
    }

    /// The automation value, 0 to 1, that gives `value`: the inverse of
    /// [`value`](Self::value). A value outside the range gives the nearer
    /// end.
    pub fn normalized(&self, value: f32) -> f32 {
        let (min, max) = (self.min, self.max);
        let span = max - min;
        if !value.is_finite() || span.is_nan() || span <= 0.0 {
            return 0.0;
        }
        let part = ((value - min) / span).clamp(0.0, 1.0);
        match self.taper {
            AutomationTaper::Linear | AutomationTaper::Stepped => part,
            AutomationTaper::Logarithmic => {
                let ratio = (value / min).max(1.0);
                (ratio.ln() / (max / min).ln()).clamp(0.0, 1.0)
            }
            AutomationTaper::Square => part.sqrt(),
            AutomationTaper::Toggle => {
                if part >= 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }
}

/// How far along a segment's change of value is, `part` (0 to 1) of the way
/// through the segment in time, for a segment bent by `curve` (-1 to 1).
///
/// A `curve` of 0 is a straight line. A positive one holds back and
/// catches up at the end, a negative one moves fast first and settles:
/// `(e^(k * part) - 1) / (e^k - 1)` with `k = 6 * curve`. At a `curve` of
/// 1 the value has covered a twentieth of its way when half the time is
/// up, and at -1 all but a twentieth.
pub fn curve_shape(part: f64, curve: f32) -> f64 {
    let part = part.clamp(0.0, 1.0);
    let bend = if curve.is_finite() {
        f64::from(curve.clamp(-1.0, 1.0)) * CURVE_STRENGTH
    } else {
        0.0
    };
    if bend.abs() < 1e-6 {
        return part;
    }
    (bend * part).exp_m1() / bend.exp_m1()
}

/// The value of a curve, 0 to 1, at `tick` ticks from its start.
///
/// `points` are in order of their ticks. Before the first point the curve
/// has its value and after the last point that one's. Of several points on
/// one tick the last is the one the curve leaves from, which makes a jump.
/// A curve with no points is 0 everywhere.
pub fn curve_value(points: &[AutomationPoint], tick: f64) -> f32 {
    let after = points.partition_point(|point| f64::from(point.tick) <= tick);
    let Some(from) = after.checked_sub(1).map(|index| &points[index]) else {
        return points.first().map_or(0.0, |point| point.value);
    };
    let Some(to) = points.get(after) else {
        return from.value;
    };
    if from.hold {
        return from.value;
    }
    let span = f64::from(to.tick) - f64::from(from.tick);
    let part = (tick - f64::from(from.tick)) / span;
    let shaped = curve_shape(part, from.curve) as f32;
    from.value + (to.value - from.value) * shaped
}

impl Project {
    /// The range of what `target` moves, or `None` when the project has no
    /// such thing: the channel, track, send or effect is not there, the
    /// channel is not an instrument, or the processor has no setting with
    /// that index.
    pub fn automation_range(&self, target: &AutomationTarget) -> Option<AutomationRange> {
        self.automation_state(target).map(|(range, _)| range)
    }

    /// The value `target` has in the project right now, in its own unit,
    /// with the same `None` as [`automation_range`](Self::automation_range).
    /// Automation never changes this value: it is what the target has
    /// wherever no curve reaches it.
    pub fn automation_stored_value(&self, target: &AutomationTarget) -> Option<f32> {
        self.automation_state(target).map(|(_, value)| value)
    }

    /// The range of a target and the value it has.
    fn automation_state(&self, target: &AutomationTarget) -> Option<(AutomationRange, f32)> {
        let effect = |track, effect| self.mixer.track(track)?.effect(effect);
        let found = match *target {
            AutomationTarget::ChannelVolume { channel } => {
                (AutomationRange::GAIN, self.channel(channel)?.volume)
            }
            AutomationTarget::ChannelPan { channel } => {
                (AutomationRange::PAN, self.channel(channel)?.pan)
            }
            AutomationTarget::TrackVolume { track } => {
                (AutomationRange::GAIN, self.mixer.track(track)?.volume)
            }
            AutomationTarget::TrackPan { track } => {
                (AutomationRange::PAN, self.mixer.track(track)?.pan)
            }
            AutomationTarget::TrackParam { track, param } => {
                use windfall_dsp::ParamSet;
                let info = windfall_dsp::TrackParams::descriptors().get(param as usize)?;
                (
                    AutomationRange::of_param(info),
                    self.mixer.track(track)?.processing.get(param as usize)?,
                )
            }
            AutomationTarget::SidechainGain { track, target } => {
                let sends = &self.mixer.track(track)?.sidechains;
                let send = sends.iter().find(|send| send.target == target)?;
                (AutomationRange::GAIN, send.gain)
            }
            AutomationTarget::SendGain { track, target } => {
                let sends = &self.mixer.track(track)?.sends;
                let send = sends.iter().find(|send| send.target == target)?;
                (AutomationRange::GAIN, send.gain)
            }
            AutomationTarget::EffectParam {
                track,
                effect: id,
                param,
            } => {
                let slot = effect(track, id)?;
                if let Some(plugin) = self.plugin(crate::PluginTarget::Effect { effect: id }) {
                    return plugin_automation(plugin, param);
                }
                let info = slot.kind().descriptors().get(param as usize)?;
                let value = slot.params.get(param as usize)?;
                (AutomationRange::of_param(info), value)
            }
            AutomationTarget::EffectMix { track, effect: id } => {
                (AutomationRange::MIX, effect(track, id)?.mix)
            }
            AutomationTarget::InstrumentParam { channel, param } => {
                if let Some(rack) = self
                    .settings
                    .modular_racks
                    .iter()
                    .find(|rack| rack.channel == channel)
                {
                    return rack.graph.parameter_state(param as usize);
                }
                if let Some(plugin) = self.plugin(crate::PluginTarget::Instrument { channel }) {
                    return plugin_automation(plugin, param);
                }
                let ChannelSource::Instrument { params, .. } = &self.channel(channel)?.source
                else {
                    return None;
                };
                let info = params.kind().descriptors().get(param as usize)?;
                let value = params.get(param as usize)?;
                (AutomationRange::of_param(info), value)
            }
            AutomationTarget::Tempo => (AutomationRange::TEMPO, self.settings.tempo_bpm as f32),
        };
        Some(found)
    }
}

fn plugin_automation(plugin: &crate::PluginBinding, index: u32) -> Option<(AutomationRange, f32)> {
    let param = plugin.parameters.get(index as usize)?;
    if param.read_only || !param.automatable {
        return None;
    }
    Some((
        AutomationRange {
            min: param.min,
            max: param.max,
            taper: if param.stepped {
                AutomationTaper::Stepped
            } else {
                AutomationTaper::Linear
            },
        },
        param.value,
    ))
}

#[cfg(test)]
mod tests {
    use windfall_dsp::{EffectKind, InstrumentKind};

    use super::*;

    fn point(tick: u32, value: f32) -> AutomationPoint {
        AutomationPoint {
            tick,
            value,
            curve: 0.0,
            hold: false,
        }
    }

    #[test]
    fn a_curve_runs_straight_between_its_points_and_flat_outside_them() {
        let points = [point(100, 0.2), point(300, 0.6), point(400, 0.0)];
        assert_eq!(curve_value(&points, 0.0), 0.2);
        assert_eq!(curve_value(&points, 100.0), 0.2);
        assert!((curve_value(&points, 200.0) - 0.4).abs() < 1e-6);
        assert!((curve_value(&points, 250.0) - 0.5).abs() < 1e-6);
        assert_eq!(curve_value(&points, 300.0), 0.6);
        assert!((curve_value(&points, 350.0) - 0.3).abs() < 1e-6);
        assert_eq!(curve_value(&points, 400.0), 0.0);
        assert_eq!(curve_value(&points, 9_000.0), 0.0);
        assert_eq!(curve_value(&[point(50, 0.7)], 0.0), 0.7);
        assert_eq!(curve_value(&[point(50, 0.7)], 900.0), 0.7);
        assert_eq!(curve_value(&[], 10.0), 0.0);
    }

    #[test]
    fn a_hold_keeps_its_value_until_the_next_point_and_two_points_on_a_tick_jump() {
        let mut points = [point(0, 0.25), point(100, 0.75), point(200, 0.5)];
        points[0].hold = true;
        assert_eq!(curve_value(&points, 0.0), 0.25);
        assert_eq!(curve_value(&points, 99.999), 0.25);
        assert_eq!(curve_value(&points, 100.0), 0.75);
        assert!((curve_value(&points, 150.0) - 0.625).abs() < 1e-6);

        let jump = [
            point(0, 0.0),
            point(100, 0.5),
            point(100, 1.0),
            point(200, 0.0),
        ];
        assert!((curve_value(&jump, 99.0) - 0.495).abs() < 1e-6);
        assert_eq!(curve_value(&jump, 100.0), 1.0);
        assert!((curve_value(&jump, 150.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_bent_segment_still_starts_and_ends_on_its_points() {
        for curve in [-1.0, -0.4, 0.0, 0.3, 1.0] {
            assert_eq!(curve_shape(0.0, curve), 0.0);
            assert!((curve_shape(1.0, curve) - 1.0).abs() < 1e-12);
            let mut last = 0.0;
            for step in 1..=100 {
                let shaped = curve_shape(f64::from(step) / 100.0, curve);
                assert!(shaped > last, "curve {curve} at step {step}");
                last = shaped;
            }
        }
        assert_eq!(curve_shape(0.25, 0.0), 0.25);
        // A positive curve holds back, a negative one hurries, by as much.
        let (slow, fast) = (curve_shape(0.5, 1.0), curve_shape(0.5, -1.0));
        assert!((slow - 0.0474).abs() < 1e-3, "{slow}");
        assert!((slow + fast - 1.0).abs() < 1e-12);
        // Out of range and not a number are held to what makes sense.
        assert_eq!(curve_shape(0.5, f32::NAN), 0.5);
        assert_eq!(curve_shape(0.5, 9.0), slow);
        assert_eq!(curve_shape(7.0, 0.5), 1.0);

        let mut points = [point(0, 0.2), point(1_000, 1.0)];
        points[0].curve = 1.0;
        assert_eq!(curve_value(&points, 0.0), 0.2);
        assert_eq!(curve_value(&points, 1_000.0), 1.0);
        let half_way = curve_value(&points, 500.0);
        assert!((half_way - (0.2 + 0.8 * slow as f32)).abs() < 1e-6);
    }

    #[test]
    fn gains_are_on_a_square_taper_and_pans_and_tempo_are_linear() {
        let gain = AutomationRange::GAIN;
        assert_eq!(gain.value(0.0), 0.0);
        assert_eq!(gain.value(0.5), 0.5);
        assert_eq!(gain.value(1.0), 2.0);
        assert!((gain.value(std::f32::consts::FRAC_1_SQRT_2) - 1.0).abs() < 1e-6);
        assert!((gain.normalized(1.0) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert_eq!(gain.normalized(0.8), (0.4_f32).sqrt());

        assert_eq!(AutomationRange::PAN.value(0.5), 0.0);
        assert_eq!(AutomationRange::PAN.value(0.0), -1.0);
        assert_eq!(AutomationRange::PAN.normalized(0.5), 0.75);
        assert_eq!(AutomationRange::MIX.value(0.3), 0.3);
        assert_eq!(AutomationRange::TEMPO.value(0.0), 10.0);
        assert_eq!(AutomationRange::TEMPO.value(1.0), 522.0);
        assert_eq!(AutomationRange::TEMPO.value(0.214_843_75), 120.0);
        assert_eq!(AutomationRange::TEMPO.normalized(120.0), 0.214_843_75);

        // Values outside 0 to 1 are held to it, and so are values outside
        // the range on the way back.
        assert_eq!(gain.value(7.0), 2.0);
        assert_eq!(gain.value(-1.0), 0.0);
        assert_eq!(gain.value(f32::NAN), 0.0);
        assert_eq!(gain.normalized(9.0), 1.0);
        assert_eq!(gain.normalized(f32::NAN), 0.0);
    }

    #[test]
    fn every_setting_maps_onto_its_own_range_and_back() {
        let effects = EffectKind::ALL.iter().flat_map(|kind| kind.descriptors());
        let synth = InstrumentKind::SubtractiveSynth.descriptors();
        for info in effects.chain(synth) {
            let range = AutomationRange::of_param(info);
            assert_eq!(range.value(0.0), info.min, "{}", info.id);
            assert!((range.value(1.0) - info.max).abs() <= info.max.abs() * 1e-6);
            for step in 0..=20 {
                let n = step as f32 / 20.0;
                let value = range.value(n);
                assert!(value >= info.min && value <= info.max + info.max.abs() * 1e-6);
                match info.kind {
                    ParamKind::Float if info.min == info.max => {
                        // Reserved descriptors (such as the final echo-bank
                        // next-send) have one legal value, so normalization
                        // cannot retain the original automation position.
                        assert_eq!(value, info.min, "{} at {n}", info.id);
                        assert_eq!(range.normalized(value), 0.0, "{}", info.id);
                        assert_eq!(range.value(range.normalized(value)), value);
                    }
                    ParamKind::Float => {
                        let back = range.normalized(value);
                        assert!((back - n).abs() < 1e-4, "{} at {n}: {back}", info.id);
                    }
                    ParamKind::Integer | ParamKind::Choice | ParamKind::Toggle => {
                        assert_eq!(value, value.round(), "{}", info.id);
                        // A whole number maps back onto itself.
                        assert_eq!(range.value(range.normalized(value)), value);
                    }
                }
            }
            let default = range.value(range.normalized(info.default));
            assert!(
                (default - info.default).abs() <= (info.max - info.min) * 1e-5,
                "{}",
                info.id
            );
            if info.scale == ParamScale::Logarithmic {
                // Half way is the geometric middle, not the arithmetic one.
                let middle = (info.min * info.max).sqrt();
                assert!((range.value(0.5) - middle).abs() < middle * 1e-4);
            }
        }
    }
}
