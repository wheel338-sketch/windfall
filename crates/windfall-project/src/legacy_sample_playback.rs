//! One-way v1 migration from primitive Copy payloads to saved assets.
use crate::model::*;
use crate::{InlinePlaybackSource, SamplePlayback};
use serde::{Deserialize, Deserializer, de::Error};
use windfall_dsp::granular::{
    PlaybackCurve, PlaybackPoint, SampleTable, SliceRegion, SliceSettings,
};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelWire {
    /// Immutable source/marker/curve authoring data, separate from Copy controls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sample_playback: Option<crate::SamplePlayback>,
    /// Additional note generators and modulation; absent in older files.
    #[serde(
        default,
        deserialize_with = "crate::channel_voice::deserialize_settings"
    )]
    pub voice: crate::ChannelVoiceSettings,
    pub id: ChannelId,
    pub name: String,
    /// Named rack group. The empty string is the ungrouped category.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// Non-destructive gate, shift and per-channel swing.
    #[serde(default, skip_serializing_if = "crate::ChannelTiming::is_default")]
    pub timing: crate::ChannelTiming,
    /// Display color as 0xRRGGBB.
    pub color: u32,
    /// Linear gain, 0 to [`MAX_GAIN`].
    pub volume: f32,
    /// -1 is hard left, 1 is hard right.
    pub pan: f32,
    pub muted: bool,
    pub solo: bool,
    /// The mixer track this channel plays into.
    pub mixer_track: TrackId,
    pub source: serde_json::Value,
    /// What modulation X, modulation Y and release drive in this channel's
    /// voices. Absent in older files, which is the fixed mapping they had.
    #[serde(default, skip_serializing_if = "crate::ExpressionMap::is_default")]
    pub expression_map: crate::ExpressionMap,
}
impl<'de> Deserialize<'de> for Channel {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ChannelWire::deserialize(deserializer)?;
        let mut asset = wire.sample_playback;
        let raw = wire.source.get("params");
        if asset.is_none()
            && let Some(raw) = raw.filter(|p| {
                matches!(
                    p.get("type").and_then(|v| v.as_str()),
                    Some("sliceMap" | "sliceDeck" | "grainCloud" | "waveRide")
                )
            })
            && (raw.get("table").is_some()
                || raw.get("sliceStarts").is_some()
                || raw.get("positions").is_some())
        {
            let mut migrated = SamplePlayback::default();
            if let Some(table) = raw.get("table") {
                let table: SampleTable =
                    serde_json::from_value(table.clone()).map_err(D::Error::custom)?;
                if usize::from(table.len) > table.data.len()
                    || !table.sample_rate.is_finite()
                    || table.sample_rate < 1000.0
                {
                    return Err(D::Error::custom("Invalid legacy sample instrument source."));
                }
                if table.len > 0 {
                    migrated.inline_source = Some(InlinePlaybackSource {
                        sample_rate: table.sample_rate.round() as u32,
                        data: table.data[..usize::from(table.len)].to_vec(),
                    });
                }
            }
            if matches!(
                raw.get("type").and_then(|v| v.as_str()),
                Some("sliceMap" | "sliceDeck")
            ) {
                let starts: Vec<f32> = raw
                    .get("sliceStarts")
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(D::Error::custom)?
                    .unwrap_or_else(|| (0..16).map(|i| i as f32 / 16.0).collect());
                if starts.len() != 16 {
                    return Err(D::Error::custom("Invalid legacy slice map."));
                }
                let settings: Vec<SliceSettings> = raw
                    .get("slices")
                    .map(|v| serde_json::from_value(v.clone()))
                    .transpose()
                    .map_err(D::Error::custom)?
                    .unwrap_or_else(|| vec![SliceSettings::default(); 16]);
                if settings.len() != 16 {
                    return Err(D::Error::custom("Invalid legacy slice settings."));
                }
                migrated.legacy_key_wrap = true;
                migrated.slices = starts
                    .iter()
                    .enumerate()
                    .map(|(i, &start)| {
                        let end = starts.get(i + 1).copied().unwrap_or(1.0);
                        // Empty legacy slices are preserved as silent gain-zero regions.
                        SliceRegion {
                            start: if end > start { start } else { 0.0 },
                            end: if end > start { end } else { 1.0 },
                            gain: if end > start { settings[i].gain } else { 0.0 },
                            pitch: settings[i].pitch,
                            reverse: settings[i].reverse,
                            release_ms: 30.0,
                            ..Default::default()
                        }
                    })
                    .collect();
            }
            if let Some(positions) = raw.get("positions") {
                let positions: Vec<f32> =
                    serde_json::from_value(positions.clone()).map_err(D::Error::custom)?;
                if positions.len() != 8 {
                    return Err(D::Error::custom("Invalid legacy playback curve."));
                }
                migrated.curve = PlaybackCurve {
                    points: positions
                        .into_iter()
                        .enumerate()
                        .map(|(i, value)| PlaybackPoint {
                            time: i as f32 / 7.0,
                            value,
                        })
                        .collect(),
                    ..Default::default()
                };
            }
            migrated.validate().map_err(D::Error::custom)?;
            asset = Some(migrated);
        }
        let source = serde_json::from_value(wire.source).map_err(D::Error::custom)?;
        Ok(Channel {
            sample_playback: asset,
            voice: wire.voice,
            id: wire.id,
            name: wire.name,
            group: wire.group,
            timing: wire.timing,
            color: wire.color,
            volume: wire.volume,
            pan: wire.pan,
            muted: wire.muted,
            solo: wire.solo,
            mixer_track: wire.mixer_track,
            source,
            expression_map: wire.expression_map,
        })
    }
}
