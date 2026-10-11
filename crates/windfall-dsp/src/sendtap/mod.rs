//! A levelled stereo copy at this point in Windfall's effect chain.
//! The host supplies and routes the tap buffers; dry audio is never changed.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::effect::Effect;
use crate::param::{ParamSet, param_set};

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct SendTapParams {
    /// Tap gain, 0 to 1; default 0 (silent).
    pub level: f32,
}

param_set!(SendTapParams, "Send Tap", {
    float [level] "level" "Level" { Gain, Linear, 0.0, 1.0, 0.0 }
});

/// Time the routed send takes to travel its whole level range.
const SEND_GLIDE_MS: f32 = 5.0;

/// Unity passthrough with a separate, host-owned send output. The dry path
/// and [`SendTap::send`] keep no audio; [`SendTap::send_frames`] only keeps
/// the gain it last applied, so a routed send follows its level without a
/// step.
#[derive(Debug, Default)]
pub struct SendTap {
    params: SendTapParams,
    /// Gain [`SendTap::send_frames`] last applied.
    sent: f32,
    /// Largest change of that gain from one frame to the next.
    step: f32,
    /// Nothing has been sent since prepare or reset, so the first frame
    /// starts on the level instead of gliding up to it.
    fresh: bool,
}

impl SendTap {
    /// Writes the block just passed to `process` into `send` as stereo
    /// frames at the current level, for a host that routes the tap to
    /// another track. The gain glides to a changed level over a few
    /// milliseconds, so automating or dragging it never steps. The first
    /// block after `prepare` or `reset` starts on the level. `send` is
    /// overwritten. Does not allocate.
    pub fn send_frames(&mut self, left: &[f32], right: &[f32], send: &mut [[f32; 2]]) {
        let level = self.params.level;
        if std::mem::replace(&mut self.fresh, false) {
            self.sent = level;
        }
        let step = if self.step > 0.0 { self.step } else { 1.0 };
        for (frame, (left, right)) in send.iter_mut().zip(left.iter().zip(right)) {
            let gap = level - self.sent;
            self.sent = if gap.abs() <= step {
                level
            } else {
                self.sent + step.copysign(gap)
            };
            // Exact silence at zero, even for a source that is not a number.
            *frame = if self.sent == 0.0 {
                [0.0; 2]
            } else {
                [
                    crate::balance::audio(*left) * self.sent,
                    crate::balance::audio(*right) * self.sent,
                ]
            };
        }
    }

    /// Copies the block just passed to `process` into the destination pair,
    /// scaled by the current level. Call before downstream processing changes
    /// the source block. No audio is retained internally.
    ///
    /// All four slices must have the same length. Destinations are overwritten,
    /// not accumulated; the host handles mixing and destination routing.
    /// Level changes apply immediately, without a gain ramp.
    pub fn send(&self, left: &[f32], right: &[f32], send_left: &mut [f32], send_right: &mut [f32]) {
        let level = self.params.level;
        if level == 0.0 {
            // Exact silence even when the source contains nonfinite samples.
            send_left.fill(0.0);
            send_right.fill(0.0);
        } else if level == 1.0 {
            send_left.copy_from_slice(left);
            send_right.copy_from_slice(right);
        } else {
            for (output, input) in send_left.iter_mut().zip(left) {
                *output = *input * level;
            }
            for (output, input) in send_right.iter_mut().zip(right) {
                *output = *input * level;
            }
        }
    }
}

impl Effect for SendTap {
    type Params = SendTapParams;

    fn prepare(&mut self, sample_rate: f32, _max_block: usize) {
        let rate = crate::balance::rate(sample_rate);
        self.step = 1.0 / (SEND_GLIDE_MS * 0.001 * rate).max(1.0);
        self.reset();
    }

    fn reset(&mut self) {
        self.fresh = true;
    }

    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
    }

    fn process(&mut self, _left: &mut [f32], _right: &mut [f32]) {}
}

#[cfg(test)]
mod tests;
