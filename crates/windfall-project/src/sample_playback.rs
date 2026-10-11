//! Saved sample-instrument assets; edits use ordinary channel history/patches.
use crate::{Channel, ChannelSource, Project, SampleId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_core::AudioBuffer;
use windfall_dsp::{
    InstrumentKind,
    granular::{PlaybackCurve, SliceRegion, validate_slices},
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct KeyPlaybackCurve {
    pub key: u8,
    pub curve: PlaybackCurve,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct InlinePlaybackSource {
    pub sample_rate: u32,
    pub data: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct SamplePlayback {
    /// Pre-asset files mapped all keys modulo sixteen; new maps are bounded.
    #[serde(default, skip_serializing_if = "legacy_wrap_is_false")]
    #[ts(as = "Option<bool>", optional)]
    pub legacy_key_wrap: bool,
    /// Only used to migrate pre-asset v1 sample tables without losing their audio.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub inline_source: Option<InlinePlaybackSource>,
    pub sample: Option<SampleId>,
    pub sample_b: Option<SampleId>,
    pub slices: Vec<SliceRegion>,
    pub curve: PlaybackCurve,
    pub key_curves: Vec<KeyPlaybackCurve>,
    /// Authored source tempo and length used by the note-pattern generator.
    pub source_bpm: f64,
    pub source_beats: f64,
}
impl Default for SamplePlayback {
    fn default() -> Self {
        Self {
            legacy_key_wrap: false,
            inline_source: None,
            sample: None,
            sample_b: None,
            slices: vec![SliceRegion::default()],
            curve: PlaybackCurve::default(),
            key_curves: Vec::new(),
            source_bpm: 120.0,
            source_beats: 4.0,
        }
    }
}
fn legacy_wrap_is_false(value: &bool) -> bool {
    !*value
}
pub fn supported(kind: InstrumentKind) -> bool {
    matches!(
        kind,
        InstrumentKind::SliceMap
            | InstrumentKind::SliceDeck
            | InstrumentKind::GrainCloud
            | InstrumentKind::WaveRide
    )
}
impl SamplePlayback {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(source) = &self.inline_source {
            if source.data.is_empty()
                || source.data.len() > 4096
                || !(1000..=384000).contains(&source.sample_rate)
                || source.data.iter().any(|v| !v.is_finite())
            {
                return Err("Invalid migrated sample table.".into());
            }
        }
        validate_slices(&self.slices).map_err(str::to_owned)?;
        self.curve.validate().map_err(str::to_owned)?;
        if !self.source_bpm.is_finite()
            || !(10.0..=522.0).contains(&self.source_bpm)
            || !self.source_beats.is_finite()
            || !(0.001..=256.0).contains(&self.source_beats)
        {
            return Err("Source tempo/beat length is out of range.".into());
        }
        let mut keys = [false; 128];
        if self.key_curves.len() > 128 {
            return Err("Too many key curves.".into());
        }
        for item in &self.key_curves {
            if item.key > 127 || keys[item.key as usize] {
                return Err("Invalid/repeated curve key.".into());
            }
            keys[item.key as usize] = true;
            item.curve.validate().map_err(str::to_owned)?;
        }
        Ok(())
    }
    pub fn references(&self, id: SampleId) -> bool {
        self.sample == Some(id) || self.sample_b == Some(id)
    }
    pub fn check(&self, project: &Project, channel: &Channel) -> Result<(), String> {
        self.validate()?;
        if !matches!(channel.source, ChannelSource::Instrument { params, .. } if supported(params.kind()))
        {
            return Err(
                "Sample playback assets require a slice, grain or curve instrument.".into(),
            );
        }
        for id in [self.sample, self.sample_b].into_iter().flatten() {
            if project.sample(id).is_none() {
                return Err("The sample playback source no longer exists.".into());
            }
        }
        Ok(())
    }
}

/// Reuses the playlist detector with a natural-length, source-aligned clip.
/// Runs off the audio/document locks, and retains original sample identity.
pub fn detect(
    audio: &AudioBuffer,
    sample: SampleId,
    bpm: f64,
    options: crate::slicer::SliceOptions,
) -> Result<SamplePlayback, String> {
    let beats = audio.frames() as f64 / f64::from(audio.sample_rate()) * bpm / 60.0;
    if !(0.001..=256.0).contains(&beats) {
        return Err("Choose a loop no longer than 256 source beats.".into());
    }
    let length = (beats * f64::from(crate::PPQ)).ceil() as u32;
    let clip = crate::Clip {
        id: crate::ClipId(0),
        track: crate::PlaylistTrackId(0),
        start: 0,
        length,
        offset: 0,
        muted: false,
        content: crate::ClipContent::Audio {
            sample,
            mixer_track: crate::TrackId(0),
            gain: 1.0,
            pan: 0.0,
            pitch: 0.0,
            reverse: false,
            normalize: false,
            fade_in: 0,
            fade_out: 0,
            stretch: crate::ClipStretch::Tape,
            output: crate::ClipAudioOutput::default(),
        },
    };
    let analysis = crate::slicer::analyze(audio, &clip, bpm, 0.0, options)?;
    if analysis.markers.len() >= windfall_dsp::granular::MAX_ASSET_SLICES {
        return Err(
            "More than 128 slices detected. Use a coarser grid or lower sensitivity.".into(),
        );
    }
    let mut boundaries = vec![0.0];
    boundaries.extend(
        analysis
            .markers
            .iter()
            .map(|m| (f64::from(m.tick) / (beats * f64::from(crate::PPQ))) as f32),
    );
    boundaries.push(1.0);
    let slices = boundaries
        .windows(2)
        .map(|p| SliceRegion {
            start: p[0],
            end: p[1],
            ..Default::default()
        })
        .collect();
    let result = SamplePlayback {
        sample: Some(sample),
        slices,
        source_bpm: bpm,
        source_beats: beats,
        ..Default::default()
    };
    result.validate()?;
    Ok(result)
}
