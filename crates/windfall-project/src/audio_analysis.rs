//! Persistent estimates and independent user edits. Rendered samples retain
//! the immutable source clip so reopening never compounds pitch/stretch edits.
use crate::{Clip, ClipContent};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_AUDIO_NOTES: usize = 4096;
pub const MAX_WARP_MARKERS: usize = 512;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DetectedAudioNote {
    pub start_frame: u32,
    pub end_frame: u32,
    pub midi_pitch: f64,
    /// Periodicity/spectral salience, never a calibrated probability.
    pub confidence: f64,
    pub rms: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioNoteOverride {
    pub note: u32,
    pub midi_pitch: f64,
    pub start_frame: u32,
    pub end_frame: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioWarpMarker {
    pub source_frame: u32,
    pub target_frame: u32,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioAnalysisData {
    pub frames: u32,
    pub sample_rate: u32,
    pub polyphonic: bool,
    pub notes: Vec<DetectedAudioNote>,
    /// Immutable detected transient positions. User target positions live in
    /// the recipe's separate marker list.
    pub transients: Vec<u32>,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioDenoiseSettings {
    pub profile_start: u32,
    pub profile_end: u32,
    pub reduction_db: f64,
    pub wet: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioEditRecipe {
    pub source_clip: Clip,
    pub analysis: AudioAnalysisData,
    pub overrides: Vec<AudioNoteOverride>,
    /// Includes fixed origin and editable duration endpoint.
    pub markers: Vec<AudioWarpMarker>,
    pub denoise: Option<AudioDenoiseSettings>,
}

impl AudioAnalysisData {
    pub fn check(&self) -> Result<(), String> {
        if self.frames == 0
            || self.sample_rate < 8000
            || self.sample_rate > 192000
            || self.frames as f64 / self.sample_rate as f64 > 120.0
            || self.notes.len() > MAX_AUDIO_NOTES
            || self.transients.len() > MAX_WARP_MARKERS - 2
        {
            return Err("Audio analysis exceeds its frame/rate/note/marker limits.".into());
        }
        let mut previous = 0;
        for (i, note) in self.notes.iter().enumerate() {
            if note.start_frame >= note.end_frame
                || note.end_frame > self.frames
                || !note.midi_pitch.is_finite()
                || !(0.0..=127.0).contains(&note.midi_pitch)
                || !note.confidence.is_finite()
                || !(0.0..=1.0).contains(&note.confidence)
                || !note.rms.is_finite()
                || note.rms < 0.0
                || (i > 0 && note.start_frame < previous)
            {
                return Err("Invalid or unsorted detected audio note.".into());
            }
            previous = note.start_frame;
        }
        if self.transients.windows(2).any(|w| w[0] >= w[1])
            || self.transients.iter().any(|t| *t == 0 || *t >= self.frames)
        {
            return Err("Transient positions must be ordered and internal.".into());
        }
        Ok(())
    }
}
impl AudioEditRecipe {
    pub fn check(&self) -> Result<(), String> {
        self.analysis.check()?;
        if !matches!(self.source_clip.content, ClipContent::Audio { .. })
            || self.overrides.len() > MAX_AUDIO_NOTES
            || (self.analysis.polyphonic && !self.overrides.is_empty())
        {
            return Err("Pitch correction requires monophonic estimates.".into());
        }
        let mut seen = std::collections::HashSet::new();
        let mut spans = Vec::with_capacity(self.analysis.notes.len());
        for edit in &self.overrides {
            if !seen.insert(edit.note)
                || edit.note as usize >= self.analysis.notes.len()
                || !edit.midi_pitch.is_finite()
                || !(0.0..=127.0).contains(&edit.midi_pitch)
                || edit.start_frame >= edit.end_frame
            {
                return Err("Invalid or duplicate pitch override.".into());
            }
            let note = &self.analysis.notes[edit.note as usize];
            if (edit.midi_pitch - note.midi_pitch).abs() > 24.0 {
                return Err("Pitch correction is limited to 24 semitones.".into());
            }
            let ratio = (edit.end_frame - edit.start_frame) as f64
                / (note.end_frame - note.start_frame) as f64;
            if !(0.25..=4.0).contains(&ratio) {
                return Err("Note time ratio must be between 0.25 and 4.".into());
            }
        }
        let output = self.output_frames()?;
        for (i, note) in self.analysis.notes.iter().enumerate() {
            let span = self
                .overrides
                .iter()
                .find(|o| o.note as usize == i)
                .map_or((note.start_frame, note.end_frame), |o| {
                    (o.start_frame, o.end_frame)
                });
            if span.1 > output {
                return Err("Edited note exceeds output duration.".into());
            }
            spans.push(span);
        }
        if !self.analysis.polyphonic && spans.windows(2).any(|w| w[0].1 > w[1].0) {
            return Err("Corrected monophonic notes must stay ordered without overlap.".into());
        }
        if let Some(d) = self.denoise {
            if d.profile_start >= d.profile_end
                || d.profile_end > self.analysis.frames
                || !d.reduction_db.is_finite()
                || !(0.0..=40.0).contains(&d.reduction_db)
                || !d.wet.is_finite()
                || !(0.0..=1.0).contains(&d.wet)
            {
                return Err("Select a noise-only profile and valid attenuation/wet mix.".into());
            }
        }
        Ok(())
    }
    pub fn output_frames(&self) -> Result<u32, String> {
        if self.markers.len() < 2
            || self.markers.len() > MAX_WARP_MARKERS
            || self.markers[0]
                != (AudioWarpMarker {
                    source_frame: 0,
                    target_frame: 0,
                })
        {
            return Err("Warp needs fixed origin and duration endpoints.".into());
        }
        let last = self.markers.last().expect("two endpoints");
        if last.source_frame != self.analysis.frames
            || last.target_frame == 0
            || last.target_frame as f64 / self.analysis.sample_rate as f64 > 120.0
        {
            return Err("Warp endpoint exceeds source/duration limits.".into());
        }
        for w in self.markers.windows(2) {
            if w[0].source_frame >= w[1].source_frame || w[0].target_frame >= w[1].target_frame {
                return Err("Warp markers must be strictly ordered on both axes.".into());
            }
            let ratio = (w[1].target_frame - w[0].target_frame) as f64
                / (w[1].source_frame - w[0].source_frame) as f64;
            if !(0.25..=4.0).contains(&ratio) {
                return Err("Each warp interval must have a 0.25 to 4 time ratio.".into());
            }
        }
        Ok(last.target_frame)
    }
}
