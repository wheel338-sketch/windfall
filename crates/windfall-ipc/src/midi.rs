//! Choices and review summaries for desktop MIDI files.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_project::{PatternId, PlayMode, Project};

/// Append MIDI notes to the current project, optionally in shared bar sections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MidiImportOptions {
    /// Zero means one pattern per part, split only at the model limit.
    pub bars: u32,
    pub share_patterns: bool,
    pub factory_drums: bool,
    pub tempo: bool,
    pub time_signature: bool,
    pub channel_mix: bool,
    /// Pitch bend, controller 74 and the modulation wheel at each note's
    /// start become its fine pitch and modulation. Omitted means on.
    #[ts(as = "Option<bool>", optional)]
    pub note_expression: bool,
}

impl Default for MidiImportOptions {
    fn default() -> Self {
        Self {
            bars: 0,
            share_patterns: true,
            factory_drums: false,
            tempo: true,
            time_signature: true,
            channel_mix: true,
            note_expression: true,
        }
    }
}

impl MidiImportOptions {
    /// The choices understood by the pure MIDI converter.
    pub fn core(&self) -> windfall_midi::ImportOptions {
        windfall_midi::ImportOptions {
            patterns: if self.bars == 0 {
                windfall_midi::PatternStrategy::PerTrack
            } else {
                windfall_midi::PatternStrategy::Bars {
                    bars: self.bars,
                    share: self.share_patterns,
                }
            },
            drum_kit: self.factory_drums.then(windfall_midi::DrumKit::factory),
            tempo: self.tempo,
            time_signature: self.time_signature,
            channel_mix: self.channel_mix,
            note_expression: self.note_expression,
        }
    }
}

/// Review of a prepared import; the token refers to exactly the bytes reviewed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MidiImportPreview {
    pub token: u32,
    pub channels: Vec<String>,
    pub patterns: u32,
    pub clips: u32,
    pub notes: u32,
    pub length: u32,
    pub adjustments: Vec<String>,
}

impl MidiImportPreview {
    /// UI facts without exposing project commands or predicted ids.
    pub fn from_plan(token: u32, plan: &windfall_midi::ImportPlan) -> Self {
        Self {
            token,
            channels: plan.channels.iter().map(|c| c.name.clone()).collect(),
            patterns: plan.patterns.len() as u32,
            clips: plan.clips.len() as u32,
            notes: plan.note_count() as u32,
            length: plan.length,
            adjustments: plan.adjustments.iter().map(ToString::to_string).collect(),
        }
    }
}

/// How a MIDI export writes what a note cannot say itself.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum MidiExpressionMode {
    /// Notes, release velocity and colour-group channels only.
    #[default]
    Off,
    /// Pitch bend and controllers on each note's own MIDI channel.
    ChannelVoice,
    /// A channel for each note in the lower MPE zone.
    Mpe,
}

/// Note expression in a MIDI export. Omitted, nothing but notes is written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct MidiExpressionExport {
    pub mode: MidiExpressionMode,
    /// Semitones of a full pitch bend, 1 to 96. Zero means 12, or 48 in MPE.
    pub bend_range: u8,
    /// Ticks between two messages of a moving value. Zero means 10.
    pub step: u32,
}

impl MidiExpressionExport {
    /// The choices understood by the pure MIDI converter.
    pub fn core(&self) -> Result<Option<windfall_midi::ExpressionOptions>, String> {
        let mut options = match self.mode {
            MidiExpressionMode::Off => return Ok(None),
            MidiExpressionMode::ChannelVoice => windfall_midi::ExpressionOptions::channel_voice(),
            MidiExpressionMode::Mpe => windfall_midi::ExpressionOptions::mpe(),
        };
        if self.bend_range > windfall_midi::expression::MAX_BEND_RANGE {
            return Err(format!(
                "The pitch-bend range must be between 1 and {} semitones.",
                windfall_midi::expression::MAX_BEND_RANGE
            ));
        }
        if self.bend_range != 0 {
            options.bend_range = self.bend_range;
        }
        if self.step != 0 {
            options.step = self.step;
        }
        Ok(Some(options))
    }
}

/// A MIDI file contains notes and tempo, with no rendered audio or effects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MidiExportOptions {
    pub mode: PlayMode,
    pub pattern: PatternId,
    /// Format 0 instead of the default format 1.
    pub single_track: bool,
    /// 480 or 960 ticks per quarter note; other values are refused.
    pub ppq: u16,
    pub running_status: bool,
    pub swing: bool,
    /// Fine pitch, slides, portamento, modulation and note pan as pitch bend
    /// and controllers. Requests from before the option existed omit it and
    /// export exactly what they did.
    #[serde(default)]
    #[ts(as = "Option<MidiExpressionExport>", optional)]
    pub expression: MidiExpressionExport,
}

impl MidiExportOptions {
    /// Converts a checked project snapshot into deterministic MIDI bytes.
    pub fn bytes(&self, project: &Project) -> Result<Vec<u8>, String> {
        self.export(project).map(|(bytes, _)| bytes)
    }

    /// What the export of this snapshot would not say: one sentence each.
    /// Empty when the file says everything the project's notes do.
    pub fn report(&self, project: &Project) -> Result<Vec<String>, String> {
        self.export(project).map(|(_, report)| report)
    }

    /// The file and its report from one conversion of one snapshot.
    pub fn export(&self, project: &Project) -> Result<(Vec<u8>, Vec<String>), String> {
        project.check().map_err(|error| error.to_string())?;
        let resolution = match self.ppq {
            480 => windfall_midi::Resolution::Ppq480,
            960 => windfall_midi::Resolution::Ppq960,
            _ => return Err("MIDI resolution must be 480 or 960 PPQ.".to_owned()),
        };
        let options = windfall_midi::ExportOptions {
            swing: self.swing,
            expression: self.expression.core()?,
            ..Default::default()
        };
        let exported = match self.mode {
            PlayMode::Pattern => {
                windfall_midi::export_pattern_reported(project, self.pattern, &options)
            }
            PlayMode::Song => windfall_midi::export_song_reported(project, &options),
        }
        .map_err(|error| error.to_string())?;
        let bytes = windfall_midi::write(
            &exported.song,
            &windfall_midi::WriteOptions {
                format: if self.single_track {
                    windfall_midi::SmfFormat::Single
                } else {
                    windfall_midi::SmfFormat::Multi
                },
                resolution,
                running_status: self.running_status,
            },
        );
        Ok((bytes, exported.report.lines()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_requests_from_before_note_expression_still_parse_and_leave_it_out() {
        let old = r#"{"mode":"song","pattern":3,"singleTrack":false,"ppq":960,"runningStatus":true,"swing":true}"#;
        let options: MidiExportOptions = serde_json::from_str(old).expect("an old request");
        assert_eq!(options.expression, MidiExpressionExport::default());
        assert_eq!(options.expression.core(), Ok(None));

        let new = r#"{"mode":"song","pattern":3,"singleTrack":false,"ppq":960,"runningStatus":true,"swing":true,"expression":{"mode":"mpe"}}"#;
        let options: MidiExportOptions = serde_json::from_str(new).expect("a new request");
        assert_eq!(
            options.expression.core(),
            Ok(Some(windfall_midi::ExpressionOptions::mpe()))
        );
        let voice = MidiExpressionExport {
            mode: MidiExpressionMode::ChannelVoice,
            bend_range: 24,
            step: 30,
        };
        let core = voice.core().expect("in range").expect("on");
        assert_eq!((core.bend_range, core.step), (24, 30));
        let wide = MidiExpressionExport {
            bend_range: 97,
            ..voice
        };
        assert!(wide.core().is_err());
    }

    #[test]
    fn import_requests_from_before_note_expression_import_it() {
        let old = r#"{"bars":0,"sharePatterns":true,"factoryDrums":false,"tempo":true,"timeSignature":true,"channelMix":true}"#;
        let options: MidiImportOptions = serde_json::from_str(old).expect("an old request");
        assert!(options.note_expression && options.core().note_expression);
    }
}
