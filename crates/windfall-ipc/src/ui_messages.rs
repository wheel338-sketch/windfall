//! Localizable control-side diagnostics. Never construct these on the callback.
//!
//! The string envelope preserves the existing IPC field types. Its contents are
//! stable codes and typed parameters, never a preformatted English sentence.
//! Keep codes append-only; retained reviews can outlive a locale change.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use windfall_midi::{Adjustment, ExportReport, ExpressionMode, Unsupported};

pub const UI_MESSAGE_PREFIX: &str = "windfall-message:";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiMessage {
    pub code: String,
    pub params: BTreeMap<String, Value>,
}

/// Append-only wire names. The UI admission test compares this list with its
/// catalogs, rather than independently asserting two lists have equal lengths.
pub const UI_MESSAGE_CODES: &[&str] = &[
    "midi.import.notesPastEnd",
    "midi.import.notesCutAtEnd",
    "midi.import.notesSplit",
    "midi.import.notesOverLimit",
    "midi.import.tempoOutOfRange",
    "midi.import.tempoChangesThinned",
    "midi.import.timeSignatureFitted",
    "midi.import.timeSignatureChanges",
    "midi.import.drumsAsSynth",
    "midi.import.drumKeysWithoutPad",
    "midi.import.pitchBendNotImported",
    "midi.import.controllersNotImported",
    "midi.import.aftertouchNotImported",
    "midi.import.programChangesNotImported",
    "midi.import.keySignaturesNotImported",
    "midi.import.markersNotImported",
    "midi.import.expressionImported",
    "midi.import.finePitchFitted",
    "midi.export.unexportedNotes",
    "midi.export.overlapNotes",
    "midi.export.bendClampedNotes",
    "midi.export.modulationYClampedNotes",
    "midi.export.mpePanOmitted",
    "midi.export.channelPanOmitted",
    "midi.export.slidesWithoutTarget",
    "midi.export.mpeGroupNotes",
    "midi.export.mpeSharedNotes",
    "midi.preview.tokensExhausted",
    "midi.preview.replaced",
    "midi.preview.expired",
    "midi.export.extension",
    "midi.export.bendRange",
    "midi.export.resolution",
    "midi.export.patternMissing",
    "midi.export.tooManyNotes",
    "midi.export.tooManyMessages",
    "midi.read.notMidi",
    "midi.read.tooLarge",
    "midi.read.chunkHeaderTruncated",
    "midi.read.chunkTruncated",
    "midi.read.unsupportedFormat",
    "midi.read.headerShort",
    "midi.read.zeroQuarterTicks",
    "midi.read.zeroFrameTicks",
    "midi.read.frameRate",
    "midi.read.noTracks",
    "midi.read.eventTruncated",
    "midi.read.missingValue",
    "midi.read.variableLength",
    "midi.read.eventLength",
    "midi.read.systemMessage",
    "midi.read.runningStatus",
    "midi.read.tooLong",
    "midi.read.io",
    "midi.read.unknownHeaderProblem",
    "midi.read.unknownTrackProblem",
    "midi.read.unknownTruncation",
];

pub fn ui_message(code: &str, params: Value) -> String {
    debug_assert!(UI_MESSAGE_CODES.contains(&code), "unregistered UI message code");
    let params = match params {
        Value::Object(values) => values.into_iter().collect(),
        _ => BTreeMap::new(),
    };
    let message = UiMessage { code: code.to_owned(), params };
    // All values here are JSON values, so serialization cannot fail.
    format!("{UI_MESSAGE_PREFIX}{}", serde_json::to_string(&message).expect("JSON message"))
}

pub fn midi_adjustment_message(adjustment: &Adjustment) -> String {
    let (code, params) = match adjustment {
        Adjustment::NotesPastEnd { count } => ("midi.import.notesPastEnd", json!({ "count": count })),
        Adjustment::NotesCutAtEnd { count } => ("midi.import.notesCutAtEnd", json!({ "count": count })),
        Adjustment::NotesSplit { count } => ("midi.import.notesSplit", json!({ "count": count })),
        Adjustment::NotesOverLimit { count } => ("midi.import.notesOverLimit", json!({ "count": count, "limit": windfall_midi::MAX_IMPORTED_NOTES })),
        Adjustment::TempoOutOfRange { count } => ("midi.import.tempoOutOfRange", json!({ "count": count, "min": windfall_project::MIN_TEMPO_BPM, "max": windfall_project::MAX_TEMPO_BPM })),
        Adjustment::TempoChangesThinned { changes, kept } => ("midi.import.tempoChangesThinned", json!({ "changes": changes, "kept": kept })),
        Adjustment::TimeSignatureFitted { numerator, denominator, used } => ("midi.import.timeSignatureFitted", json!({ "numerator": numerator, "denominator": denominator, "usedNumerator": used.numerator, "usedDenominator": used.denominator })),
        Adjustment::TimeSignatureChanges { count } => ("midi.import.timeSignatureChanges", json!({ "count": count })),
        Adjustment::DrumsAsSynth { channel } => ("midi.import.drumsAsSynth", json!({ "channel": channel })),
        Adjustment::DrumKeysWithoutPad { notes } => ("midi.import.drumKeysWithoutPad", json!({ "count": notes })),
        Adjustment::NotImported { what, count } => (
            match what {
                Unsupported::PitchBend => "midi.import.pitchBendNotImported",
                Unsupported::Controllers => "midi.import.controllersNotImported",
                Unsupported::Aftertouch => "midi.import.aftertouchNotImported",
                Unsupported::ProgramChanges => "midi.import.programChangesNotImported",
                Unsupported::KeySignatures => "midi.import.keySignaturesNotImported",
                Unsupported::Markers => "midi.import.markersNotImported",
            },
            json!({ "count": count }),
        ),
        Adjustment::ExpressionImported { notes } => ("midi.import.expressionImported", json!({ "count": notes })),
        Adjustment::FinePitchFitted { notes } => ("midi.import.finePitchFitted", json!({ "count": notes })),
    };
    ui_message(code, params)
}

pub fn midi_export_messages(report: &ExportReport) -> Vec<String> {
    [
        ("midi.export.unexportedNotes", report.unexported_notes),
        ("midi.export.overlapNotes", report.overlap_notes),
        ("midi.export.bendClampedNotes", report.bend_clamped_notes),
        ("midi.export.modulationYClampedNotes", report.modulation_y_clamped_notes),
        (if report.mode == Some(ExpressionMode::Mpe) { "midi.export.mpePanOmitted" } else { "midi.export.channelPanOmitted" }, report.pan_omitted_notes),
        ("midi.export.slidesWithoutTarget", report.slides_without_target),
        ("midi.export.mpeGroupNotes", report.mpe_group_notes),
        ("midi.export.mpeSharedNotes", report.mpe_shared_notes),
    ].into_iter().filter(|(_, count)| *count > 0).map(|(code, count)| {
        let params = if code == "midi.export.bendClampedNotes" {
            json!({ "count": count, "range": report.bend_range })
        } else { json!({ "count": count }) };
        ui_message(code, params)
    }).collect()
}

pub fn midi_export_error(error: windfall_midi::ExportError) -> String {
    use windfall_midi::ExportError;
    match error {
        ExportError::PatternNotFound(id) => ui_message("midi.export.patternMissing", json!({ "id": id.0 })),
        ExportError::TooManyNotes => ui_message("midi.export.tooManyNotes", json!({ "limit": windfall_midi::MAX_EXPORTED_NOTES })),
        ExportError::TooManyMessages => ui_message("midi.export.tooManyMessages", json!({ "limit": windfall_midi::MAX_EXPRESSION_MESSAGES })),
    }
}

pub fn midi_error_message(error: windfall_midi::MidiError) -> String {
    use windfall_midi::MidiError;
    let (code, params) = match error {
        MidiError::NotMidi => ("midi.read.notMidi", json!({})),
        MidiError::TooLarge { bytes, limit } => ("midi.read.tooLarge", json!({ "bytes": bytes, "limit": limit })),
        MidiError::Truncated { what, offset } => (
            match what {
                "the chunk header" => "midi.read.chunkHeaderTruncated",
                "the chunk" => "midi.read.chunkTruncated",
                _ => "midi.read.unknownTruncation",
            }, json!({ "offset": offset }),
        ),
        MidiError::UnsupportedFormat(format) => ("midi.read.unsupportedFormat", json!({ "format": format })),
        MidiError::BadHeader(problem) => (
            match problem {
                "it is shorter than the six bytes a header has" => "midi.read.headerShort",
                "it counts zero ticks to the quarter note" => "midi.read.zeroQuarterTicks",
                "it counts zero ticks to the frame" => "midi.read.zeroFrameTicks",
                "its frame rate is none of 24, 25, 29.97 and 30 frames a second" => "midi.read.frameRate",
                _ => "midi.read.unknownHeaderProblem",
            }, json!({}),
        ),
        MidiError::NoTracks => ("midi.read.noTracks", json!({})),
        MidiError::Corrupt { track, offset, problem } => (
            match problem {
                "the track ends in the middle of an event" => "midi.read.eventTruncated",
                "a new event begins where the last one still needs a value" => "midi.read.missingValue",
                "a length or a time takes more than four bytes" => "midi.read.variableLength",
                "an event is longer than what is left of the track" => "midi.read.eventLength",
                "a system message that has no place in a file" => "midi.read.systemMessage",
                "an event with no status byte before it" => "midi.read.runningStatus",
                _ => "midi.read.unknownTrackProblem",
            }, json!({ "track": track, "offset": offset }),
        ),
        MidiError::TooLong => ("midi.read.tooLong", json!({ "limit": u32::MAX })),
        // OS-owned diagnostics are opaque parameters, not semantic app labels.
        MidiError::Io { path, message } => ("midi.read.io", json!({ "path": path, "detail": message })),
    };
    ui_message(code, params)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(encoded: &str) -> UiMessage {
        serde_json::from_str(encoded.strip_prefix(UI_MESSAGE_PREFIX).expect("coded envelope"))
            .expect("typed JSON parameters")
    }

    #[test]
    fn ui_message_codes_are_unique_and_round_trip_without_preformatted_prose() {
        let codes: std::collections::BTreeSet<_> = UI_MESSAGE_CODES.iter().copied().collect();
        assert_eq!(codes.len(), UI_MESSAGE_CODES.len());
        for code in UI_MESSAGE_CODES {
            let decoded = decode(&ui_message(code, json!({ "count": 1234, "channel": "Mi batería" })));
            assert_eq!(decoded.code, *code);
            assert_eq!(decoded.params["count"], 1234);
            assert_eq!(decoded.params["channel"], "Mi batería");
        }
    }

    #[test]
    fn every_midi_adjustment_has_a_stable_code_and_numeric_or_authored_parameters() {
        use windfall_project::TimeSignature;
        let mut adjustments = vec![
            Adjustment::NotesPastEnd { count: 2 }, Adjustment::NotesCutAtEnd { count: 2 },
            Adjustment::NotesSplit { count: 2 }, Adjustment::NotesOverLimit { count: 2 },
            Adjustment::TempoOutOfRange { count: 2 }, Adjustment::TempoChangesThinned { changes: 3, kept: 2 },
            Adjustment::TimeSignatureFitted { numerator: 3, denominator: 128, used: TimeSignature { numerator: 3, denominator: 16 } },
            Adjustment::TimeSignatureChanges { count: 2 }, Adjustment::DrumsAsSynth { channel: "User kit".into() },
            Adjustment::DrumKeysWithoutPad { notes: 2 }, Adjustment::ExpressionImported { notes: 2 },
            Adjustment::FinePitchFitted { notes: 2 },
        ];
        for what in [Unsupported::PitchBend, Unsupported::Controllers, Unsupported::Aftertouch, Unsupported::ProgramChanges, Unsupported::KeySignatures, Unsupported::Markers] {
            adjustments.push(Adjustment::NotImported { what, count: 2 });
        }
        let messages: Vec<_> = adjustments.iter().map(|item| decode(&midi_adjustment_message(item))).collect();
        let actual: std::collections::BTreeSet<_> = messages.iter().map(|item| item.code.as_str()).collect();
        let expected: std::collections::BTreeSet<_> = UI_MESSAGE_CODES[..18].iter().copied().collect();
        assert_eq!(actual, expected);
        for message in &messages {
            assert!(message.params.values().all(|value| value.is_number() || value == "User kit"));
        }
    }

    #[test]
    fn midi_export_report_codes_preserve_counts_and_have_no_zero_count_notices() {
        assert!(midi_export_messages(&ExportReport::default()).is_empty());
        let report = ExportReport { mode: Some(ExpressionMode::Mpe), bend_range: 48, unexported_notes: 2, overlap_notes: 3, bend_clamped_notes: 4, modulation_y_clamped_notes: 5, pan_omitted_notes: 6, slides_without_target: 7, mpe_group_notes: 8, mpe_shared_notes: 9, ..Default::default() };
        let messages: Vec<_> = midi_export_messages(&report).iter().map(|line| decode(line)).collect();
        assert_eq!(messages.len(), 8);
        assert_eq!(messages[2].code, "midi.export.bendClampedNotes");
        assert_eq!(messages[2].params["count"], 4);
        assert_eq!(messages[2].params["range"], 48);
        assert!(messages.iter().any(|item| item.code == "midi.export.mpePanOmitted" && item.params["count"] == 6));
        let only_bend = ExportReport { bend_clamped_notes: 1, bend_range: 12, ..Default::default() };
        let only = midi_export_messages(&only_bend);
        assert_eq!(only.len(), 1);
        assert_eq!(decode(&only[0]).params["range"], 12);
    }

    #[test]
    fn parser_diagnostic_reasons_have_stable_codes_instead_of_english_parameters() {
        use windfall_midi::MidiError;
        for (problem, code) in [
            ("it is shorter than the six bytes a header has", "midi.read.headerShort"),
            ("it counts zero ticks to the quarter note", "midi.read.zeroQuarterTicks"),
            ("it counts zero ticks to the frame", "midi.read.zeroFrameTicks"),
            ("its frame rate is none of 24, 25, 29.97 and 30 frames a second", "midi.read.frameRate"),
        ] {
            let message = decode(&midi_error_message(MidiError::BadHeader(problem)));
            assert_eq!(message.code, code);
            assert!(message.params.is_empty());
        }
        for (problem, code) in [
            ("the track ends in the middle of an event", "midi.read.eventTruncated"),
            ("a new event begins where the last one still needs a value", "midi.read.missingValue"),
            ("a length or a time takes more than four bytes", "midi.read.variableLength"),
            ("an event is longer than what is left of the track", "midi.read.eventLength"),
            ("a system message that has no place in a file", "midi.read.systemMessage"),
            ("an event with no status byte before it", "midi.read.runningStatus"),
        ] {
            let message = decode(&midi_error_message(MidiError::Corrupt { track: 2, offset: 42, problem }));
            assert_eq!(message.code, code);
            assert_eq!(message.params["track"], 2);
            assert_eq!(message.params["offset"], 42);
            assert!(message.params.values().all(Value::is_number));
        }
    }

    #[test]
    fn every_parser_source_reason_is_registered_before_it_can_reach_the_ui() {
        use windfall_midi::MidiError;
        let reader = include_str!("../../windfall-midi/src/read.rs");
        let mut checked = 0;
        for (marker, header) in [("MidiError::BadHeader(", true), (".corrupt(", false)] {
            for tail in reader.split(marker).skip(1) {
                let Some(quoted) = tail.trim_start().strip_prefix('"') else { continue; };
                let reason = quoted.split('"').next().expect("source reason");
                let error = if header { MidiError::BadHeader(reason) } else {
                    MidiError::Corrupt { track: 1, offset: 2, problem: reason }
                };
                let message = decode(&midi_error_message(error));
                assert!(!message.code.contains("unknown"), "unregistered parser reason: {reason}");
                checked += 1;
            }
        }
        assert_eq!(checked, 10);
    }
}
