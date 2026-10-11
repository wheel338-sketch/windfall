//! File semantics shared by browser UI and the native shell.
use crate::{json, parse, with_document, Reply};
use serde::{Deserialize, Serialize};
use windfall_project::{ChannelId, Command, NoteId, PatternId, Score, ScoreLoadMode};

#[derive(Deserialize)]
struct Capture {
    pattern: PatternId,
    channel: ChannelId,
    only: Option<Vec<NoteId>>,
}
#[derive(Deserialize)]
struct Apply {
    score: Score,
    pattern: PatternId,
    channel: ChannelId,
    mode: ScoreLoadMode,
}

pub fn doc_score_capture(handle: u32, input: &str) -> Reply {
    let r: Capture = parse("score capture", input)?;
    with_document(handle, |doc| {
        json(&Score::capture(
            doc.project(),
            r.pattern,
            r.channel,
            r.only.as_deref(),
        )?)
    })?
}
pub fn doc_score_command(handle: u32, input: &str) -> Reply {
    let r: Apply = parse("score load", input)?;
    with_document(handle, |doc| {
        let command: Command = r
            .score
            .load_command(doc.project(), r.pattern, r.channel, r.mode)?;
        json(&command)
    })?
}
pub fn doc_score_write(handle: u32, input: &str) -> Reply {
    #[derive(Deserialize)]
    struct Request {
        score: Score,
        path: String,
    }
    #[derive(Serialize)]
    struct Written {
        bytes: Vec<u8>,
        notes: Vec<String>,
    }
    let r: Request = parse("score write", input)?;
    r.score.validate()?;
    with_document(handle, |doc| {
        let (bytes, notes) = if r.path.to_lowercase().ends_with(".mid")
            || r.path.to_lowercase().ends_with(".midi")
        {
            let (song, notes) =
                windfall_midi::score_to_song(&r.score, doc.project().settings.tempo_bpm);
            (windfall_midi::write(&song, &Default::default()), notes)
        } else {
            (r.score.to_json()?.into_bytes(), Vec::new())
        };
        json(&Written { bytes, notes })
    })?
}
pub fn score_read(_handle: u32, input: &str) -> Reply {
    #[derive(Deserialize)]
    struct Request {
        bytes: Vec<u8>,
        name: String,
    }
    let r: Request = parse("score read", input)?;
    if r.bytes.len() > windfall_project::score::MAX_SCORE_FILE_BYTES {
        return Err("Score file exceeds 32 MiB".into());
    }
    let (score, notes) = if r.bytes.starts_with(b"MThd") {
        windfall_midi::score_from_song(
            &windfall_midi::read(&r.bytes).map_err(|e| e.to_string())?,
            &r.name,
        )?
    } else {
        (Score::from_json(&r.bytes)?, Vec::new())
    };
    json(&windfall_ipc::ScoreReview { score, notes })
}
pub fn plugin_database_view(_handle: u32, input: &str) -> Reply {
    let annotations = parse("plugin annotations", input)?;
    let database = windfall_ipc::PluginDatabase {
        annotations,
        ..Default::default()
    };
    json(&database.view(&Default::default(), None))
}
