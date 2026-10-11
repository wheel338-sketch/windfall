//! Pattern-local musical metadata. It labels PPQ ticks without retiming notes.
use crate::{MeterChange, MeterChangeId, TimeSignature, TimelineMarker, TimelineMarkerId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
#[ts(export)]
pub enum PatternTimelineEdit {
    SetSignature { signature: Option<TimeSignature> },
    AddMeter { tick: u32, signature: TimeSignature },
    UpdateMeter { change: MeterChange },
    RemoveMeter { id: MeterChangeId },
    AddMarker { tick: u32, name: String },
    UpdateMarker { marker: TimelineMarker },
    RemoveMarker { id: TimelineMarkerId },
}
