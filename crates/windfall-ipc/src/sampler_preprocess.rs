//! A request to render a sampler channel's sample through offline
//! treatments into a new sample. The original sample is never overwritten.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_project::ChannelId;

/// The treatments to apply, in the order of the fields. Every field but
/// `channel` may be left out; a treatment that is left out is not applied.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SamplerPreprocessRequest {
    pub channel: ChannelId,
    /// Keep only the trimmed region and reset the trim to the whole sample.
    #[serde(default)]
    #[ts(optional)]
    pub trim: Option<bool>,
    /// Turn the audio around. The reverse switch stays as it is.
    #[serde(default)]
    #[ts(optional)]
    pub reverse: Option<bool>,
    #[serde(default)]
    #[ts(optional)]
    pub remove_dc: Option<bool>,
    #[serde(default)]
    #[ts(optional)]
    pub invert_polarity: Option<bool>,
    /// Exchange left and right. Refused for a mono sample.
    #[serde(default)]
    #[ts(optional)]
    pub swap_stereo: Option<bool>,
    /// Equal-power fades over the played region, 0 to 60000 ms.
    #[serde(default)]
    #[ts(optional)]
    pub fade_in_ms: Option<f32>,
    #[serde(default)]
    #[ts(optional)]
    pub fade_out_ms: Option<f32>,
    /// Share of the forward loop to blend across its seam, 0 to 1. At most
    /// half the loop is used. Refused when the loop is off or ping-pong.
    #[serde(default)]
    #[ts(optional)]
    pub loop_crossfade: Option<f32>,
    /// Set the peak to −1 dBFS with one gain for every channel.
    #[serde(default)]
    #[ts(optional)]
    pub normalize: Option<bool>,
}
