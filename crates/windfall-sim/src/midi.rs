//! The browser uses the same MIDI conversion and checked batch as the shell.

use serde::Deserialize;
use windfall_ipc::{MidiExportOptions, MidiImportOptions, MidiImportPreview};
use windfall_project::DispatchResult;

use crate::{Reply, json, parse, with_document};

#[derive(Deserialize)]
struct Import {
    bytes: Vec<u8>,
    options: MidiImportOptions,
}

fn plan(input: &str) -> Result<windfall_midi::ImportPlan, String> {
    let request: Import = parse("MIDI import", input)?;
    let song = windfall_midi::read(&request.bytes).map_err(windfall_ipc::midi_error_message)?;
    Ok(windfall_midi::import(&song, &request.options.core()))
}

/// Input: bytes and import options. Returns review facts, with token zero.
pub fn midi_preview(handle: u32, input: &str) -> Reply {
    let plan = plan(input)?;
    with_document(handle, |document| {
        let mut trial = document.clone();
        let command = plan.command(trial.project());
        trial
            .dispatch(command, None)
            .map_err(|error| error.to_string())?;
        trial.project().check().map_err(|error| error.to_string())?;
        json(&MidiImportPreview::from_plan(0, &plan))
    })?
}

/// Input: retained preview bytes and options. Appends one validated undo step.
pub fn doc_midi_import(handle: u32, input: &str) -> Reply {
    let plan = plan(input)?;
    with_document(handle, |document| {
        let mut trial = document.clone();
        let command = plan.command(trial.project());
        let applied = trial
            .dispatch(command, None)
            .map_err(|error| error.to_string())?;
        trial.project().check().map_err(|error| error.to_string())?;
        *document = trial;
        json(&DispatchResult {
            created: applied.created,
            patch: document.patch(&applied.touched),
        })
    })?
}

/// Input: MIDI export options. Returns SMF bytes from the document snapshot.
pub fn doc_midi_export(handle: u32, input: &str) -> Reply {
    let options: MidiExportOptions = parse("MIDI export", input)?;
    with_document(handle, |document| json(&options.bytes(document.project())?))?
}
