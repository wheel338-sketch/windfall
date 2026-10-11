//! Score files on disk. [`windfall_project::score`] has the format.
//!
//! A score is written and read in one of two forms, told apart by the file:
//! Windfall's own `.wfscore`, which holds everything, and a Standard MIDI
//! File, which other programs read and which holds the notes. Reading never
//! trusts the extension: a file that starts as a MIDI file is one.
//!
//! Neither function touches the project. What the UI does with a score that
//! was read is an ordinary command, and so one undo step.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use windfall_ipc::{ScoreReview, ScoreWritten};
use windfall_project::score::MAX_SCORE_FILE_BYTES;
use windfall_project::{Score, file};

use super::Session;
use crate::paths;

/// The extension of a score file.
pub const SCORE_EXTENSION: &str = "wfscore";

fn is_midi(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        extension.eq_ignore_ascii_case("mid") || extension.eq_ignore_ascii_case("midi")
    })
}

impl Session {
    /// Writes a score to `path` and returns the path written.
    ///
    /// A path that ends in `.mid` or `.midi` gets a Standard MIDI File at
    /// the tempo the project has now, and the result lists what of the
    /// score such a file does not hold. Any other path gets a score file,
    /// with the `.wfscore` extension added if it is missing. Either file
    /// is written whole or not at all.
    pub fn score_write(&self, path: &str, score: Score) -> Result<ScoreWritten, String> {
        score.validate()?;
        let target = paths::absolute(path)?;
        let (target, bytes, notes) = if is_midi(&target) {
            let tempo_bpm = self.state().document.project().settings.tempo_bpm;
            let (song, notes) = windfall_midi::score_to_song(&score, tempo_bpm);
            let bytes = windfall_midi::write(&song, &windfall_midi::WriteOptions::default());
            (target, bytes, notes)
        } else {
            let has = target
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case(SCORE_EXTENSION));
            let target = if has {
                target
            } else {
                PathBuf::from(format!("{}.{SCORE_EXTENSION}", paths::display(&target)))
            };
            (target, score.to_json()?.into_bytes(), Vec::new())
        };
        file::write_atomic(&target, &bytes)
            .map_err(|error| format!("Could not save the score: {error}"))?;
        Ok(ScoreWritten {
            path: paths::display(&target),
            notes,
        })
    }

    /// Reads the score at `path`: a score file, or any Standard MIDI File,
    /// whose notes become one score. The result lists what a MIDI file held
    /// that a score does not.
    pub fn score_read(&self, path: &str) -> Result<ScoreReview, String> {
        let source = paths::absolute(path)?;
        let opened =
            File::open(&source).map_err(|error| format!("Could not open the score: {error}"))?;
        let mut bytes = Vec::new();
        opened
            .take(MAX_SCORE_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("Could not read the score: {error}"))?;
        if bytes.len() > MAX_SCORE_FILE_BYTES {
            return Err("The score file is larger than 32 MiB.".into());
        }
        if bytes.starts_with(b"MThd") {
            let song = windfall_midi::read(&bytes).map_err(|error| error.to_string())?;
            let (score, notes) = windfall_midi::score_from_song(&song, &paths::stem(&source))?;
            return Ok(ScoreReview { score, notes });
        }
        Ok(ScoreReview {
            score: Score::from_json(&bytes)?,
            notes: Vec::new(),
        })
    }
}

impl Session {
    pub fn score_capture(
        &self,
        pattern: windfall_project::PatternId,
        channel: windfall_project::ChannelId,
        only: Option<Vec<windfall_project::NoteId>>,
        generation: u64,
        revision: u64,
    ) -> Result<Score, String> {
        let state = self.state();
        if state.generation != generation || state.document.revision() != revision {
            return Err("The project changed before score capture".into());
        }
        Score::capture(state.document.project(), pattern, channel, only.as_deref())
    }
    pub fn score_apply(
        &self,
        score: Score,
        pattern: windfall_project::PatternId,
        channel: windfall_project::ChannelId,
        mode: windfall_project::ScoreLoadMode,
        generation: u64,
        revision: u64,
    ) -> Result<windfall_project::DispatchResult, String> {
        let ticket = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state.generation != generation || state.document.revision() != revision {
                return Err("The project changed while the score was chosen; try again".into());
            }
            let command = score.load_command(state.document.project(), pattern, channel, mode)?;
            self.sample_edit_ticket(&state, command, None, Vec::new())?
                .ordinary()
        };
        self.finish_sample_edit(ticket)
    }
}
