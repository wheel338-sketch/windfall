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

/// A MIDI file contains notes and tempo, with no rendered audio or effects.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct MidiExportOptions {
    pub mode: PlayMode,
    pub pattern: PatternId,
    /// Format 0 instead of the default format 1.
    pub single_track: bool,
    /// Ticks per quarter note of the file: 0 for the project's own
    /// timebase, or one of the timebases a project can have (24, 48, 96,
    /// 120, 192, 240, 480, 960). Other values are refused.
    pub ppq: u16,
    pub running_status: bool,
    pub swing: bool,
}

impl MidiExportOptions {
    /// Converts a checked project snapshot into deterministic MIDI bytes.
    pub fn bytes(&self, project: &Project) -> Result<Vec<u8>, String> {
        project.check().map_err(|error| error.to_string())?;
        // The file holds what plays: every note and clip on the grid of the
        // project's timebase. At the default timebase nothing moves.
        let played = windfall_project::timebase::playback_project(project);
        let project: &Project = &played;
        let resolution = match self.ppq {
            0 => match project.settings.timebase_ppq {
                960 => windfall_midi::Resolution::Ppq960,
                480 => windfall_midi::Resolution::Ppq480,
                ppq => windfall_midi::Resolution::Ppq(ppq),
            },
            480 => windfall_midi::Resolution::Ppq480,
            960 => windfall_midi::Resolution::Ppq960,
            ppq if windfall_project::timebase::is_timebase(ppq) => {
                windfall_midi::Resolution::Ppq(ppq)
            }
            _ => {
                return Err(format!(
                    "MIDI resolution must be the project timebase or one of {:?} PPQ.",
                    windfall_project::timebase::TIMEBASES
                ));
            }
        };
        let options = windfall_midi::ExportOptions {
            swing: self.swing,
            ..Default::default()
        };
        let song = match self.mode {
            PlayMode::Pattern => windfall_midi::export_pattern(project, self.pattern, &options),
            PlayMode::Song => windfall_midi::export_song(project, &options),
        }
        .map_err(|error| error.to_string())?;
        Ok(windfall_midi::write(
            &song,
            &windfall_midi::WriteOptions {
                format: if self.single_track {
                    windfall_midi::SmfFormat::Single
                } else {
                    windfall_midi::SmfFormat::Multi
                },
                resolution,
                running_status: self.running_status,
            },
        ))
    }
}

#[cfg(test)]
mod timebase_tests {
    use super::*;
    use windfall_project::{ChannelId, Command, Document, InstrumentKind, NoteInit, SettingsPatch};

    fn options(project: &Project, ppq: u16) -> MidiExportOptions {
        MidiExportOptions {
            mode: PlayMode::Pattern,
            pattern: project.patterns[0].id,
            single_track: true,
            ppq,
            running_status: false,
            swing: false,
        }
    }

    /// The notes of a file, as starts and lengths at 960 to the quarter.
    fn notes(bytes: &[u8]) -> Vec<(u32, u32)> {
        let song = windfall_midi::read(bytes).unwrap();
        song.tracks
            .iter()
            .flat_map(|track| &track.notes)
            .map(|note| (note.start, note.length))
            .collect()
    }

    fn note(start: u32, length: u32, key: u8) -> NoteInit {
        NoteInit {
            start,
            length,
            key,
            velocity: None,
            pan: None,
            expression: None,
        }
    }

    #[test]
    fn midi_export_writes_what_plays_at_the_project_timebase() {
        let mut doc = Document::new(Project::new("Timebase"));
        let pattern = doc.project().patterns[0].id;
        let channel = ChannelId(
            doc.dispatch(
                Command::AddChannel {
                    name: None,
                    sample: None,
                    instrument: Some(InstrumentKind::SubtractiveSynth),
                    index: None,
                    mixer_track: None,
                },
                None,
            )
            .unwrap()
            .created[0],
        );
        doc.dispatch(
            Command::AddNotes {
                pattern,
                channel,
                notes: vec![note(3, 7, 60), note(244, 233, 62)],
            },
            None,
        )
        .unwrap();

        // At the default timebase the file holds the notes as drawn, and
        // "the project's timebase" is 960.
        let drawn = [(3, 7), (244, 233)];
        let project = doc.project().clone();
        let default = options(&project, 0).bytes(&project).unwrap();
        assert_eq!(&default[12..14], &960_u16.to_be_bytes());
        assert_eq!(notes(&default), drawn);
        assert_eq!(default, options(&project, 960).bytes(&project).unwrap());

        doc.dispatch(
            Command::UpdateSettings {
                patch: SettingsPatch {
                    timebase_ppq: Some(96),
                    ..SettingsPatch::default()
                },
            },
            None,
        )
        .unwrap();
        let project = doc.project().clone();
        let on_grid = [(0, 10), (240, 240)];
        // The project's own resolution: its division and its grid.
        let own = options(&project, 0).bytes(&project).unwrap();
        assert_eq!(&own[12..14], &96_u16.to_be_bytes());
        assert_eq!(notes(&own), on_grid);
        assert_eq!(own, options(&project, 96).bytes(&project).unwrap());
        // A finer file still holds the grid positions: they are what plays.
        for ppq in [480_u16, 960] {
            let finer = options(&project, ppq).bytes(&project).unwrap();
            assert_eq!(&finer[12..14], &ppq.to_be_bytes());
            assert_eq!(notes(&finer), on_grid, "{ppq}");
        }
        // Exporting changed nothing.
        assert_eq!(*doc.project(), project);

        for ppq in [1_u16, 72, 123, 961, u16::MAX] {
            let error = options(&project, ppq).bytes(&project).unwrap_err();
            assert!(error.contains("resolution"), "{ppq}: {error}");
        }
    }
}
