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
/// Frozen source-view timing at detection. Audio clips play at source speed;
/// only playlist edges and fades follow the tempo map. Stored in source frames
/// so later tempo edits cannot rewrite the input behind retained estimates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioViewTiming {
    pub duration_frames: f64,
    pub offset_frames: f64,
    pub fade_in_frames: f64,
    pub fade_out_frames: f64,
}
impl AudioViewTiming {
    pub fn check(&self) -> Result<(), String> {
        if !self.duration_frames.is_finite()
            || self.duration_frames <= 0.0
            || !self.offset_frames.is_finite()
            || self.offset_frames < 0.0
            || [self.fade_in_frames, self.fade_out_frames]
                .iter()
                .any(|v| !v.is_finite() || *v < 0.0 || *v > self.duration_frames)
        {
            return Err("Invalid frozen audio view timing.".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AudioEditRecipe {
    pub source_clip: Clip,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_timing: Option<AudioViewTiming>,
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
        if let Some(timing) = self.source_timing {
            timing.check()?;
            if (timing.duration_frames - 1e-6).ceil() != f64::from(self.analysis.frames) {
                return Err("Frozen source timing disagrees with analyzed frames.".into());
            }
        }
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
        let render_markers = self.render_markers()?;
        for (i, note) in self.analysis.notes.iter().enumerate() {
            let span = self.overrides.iter().find(|o| o.note as usize == i).map_or(
                (
                    map_frame(&render_markers, note.start_frame),
                    map_frame(&render_markers, note.end_frame),
                ),
                |o| (o.start_frame, o.end_frame),
            );
            if span.1 > output {
                return Err("Edited note exceeds output duration.".into());
            }
            spans.push(span);
        }
        if !self.analysis.polyphonic && spans.windows(2).any(|w| w[0].1 > w[1].0) {
            return Err("Corrected monophonic notes must stay ordered without overlap.".into());
        }
        if let Some(d) = self.denoise
            && (d.profile_start >= d.profile_end
                || d.profile_end > self.analysis.frames
                || !d.reduction_db.is_finite()
                || !(0.0..=40.0).contains(&d.reduction_db)
                || !d.wet.is_finite()
                || !(0.0..=1.0).contains(&d.wet))
        {
            return Err("Select a noise-only profile and valid attenuation/wet mix.".into());
        }
        Ok(())
    }
    /// Override note edges take precedence over transient anchors at that
    /// source frame. Adjacent notes must agree on a shared boundary.
    pub fn render_markers(&self) -> Result<Vec<AudioWarpMarker>, String> {
        let output = self.output_frames()?;
        let mut edits = Vec::with_capacity(self.overrides.len() * 2);
        for edit in &self.overrides {
            let note = self
                .analysis
                .notes
                .get(edit.note as usize)
                .ok_or("Unknown note override.")?;
            for (source_frame, target_frame) in [
                (note.start_frame, edit.start_frame),
                (note.end_frame, edit.end_frame),
            ] {
                if (source_frame == 0 && target_frame != 0)
                    || (source_frame == self.analysis.frames && target_frame != output)
                {
                    return Err("Note edits cannot move fixed render endpoints.".into());
                }
                if edits.iter().any(|m: &AudioWarpMarker| {
                    m.source_frame == source_frame && m.target_frame != target_frame
                }) {
                    return Err("Adjacent edited notes disagree on their shared boundary.".into());
                }
                edits.push(AudioWarpMarker {
                    source_frame,
                    target_frame,
                });
            }
        }
        let mut markers: Vec<_> = self
            .markers
            .iter()
            .filter(|m| !edits.iter().any(|e| e.source_frame == m.source_frame))
            .copied()
            .collect();
        markers.extend(edits);
        markers.sort_by_key(|m| m.source_frame);
        markers.dedup();
        for pair in markers.windows(2) {
            if pair[0].source_frame >= pair[1].source_frame
                || pair[0].target_frame >= pair[1].target_frame
            {
                return Err("Combined note and warp markers cross.".into());
            }
            let ratio = (pair[1].target_frame - pair[0].target_frame) as f64
                / (pair[1].source_frame - pair[0].source_frame) as f64;
            if !(0.25..=4.0).contains(&ratio) {
                return Err("Combined note/warp interval ratio must be 0.25 to 4.".into());
            }
        }
        Ok(markers)
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
fn map_frame(markers: &[AudioWarpMarker], source: u32) -> u32 {
    let pair = markers
        .windows(2)
        .find(|w| source <= w[1].source_frame)
        .expect("checked recipe endpoint");
    let part = (source - pair[0].source_frame) as f64
        / (pair[1].source_frame - pair[0].source_frame) as f64;
    (pair[0].target_frame as f64 + part * (pair[1].target_frame - pair[0].target_frame) as f64)
        .round() as u32
}
