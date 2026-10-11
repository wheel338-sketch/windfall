//! Captured pattern meter grids for note tools. Control-side only.
use crate::{
    CommandError, MAX_PATTERN_TICKS, MeterChange, MeterMap, TICKS_PER_STEP, TimeSignature,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum NoteGridUnit {
    Step,
    Beat,
    Bar,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct NoteMusicalGrid {
    pub signature: TimeSignature,
    pub meters: Vec<MeterChange>,
    pub unit: NoteGridUnit,
    pub divisor: u32,
}

impl NoteMusicalGrid {
    pub(crate) fn segments(&self) -> Result<Vec<(u32, u32, u32)>, CommandError> {
        if !(1..=96).contains(&self.divisor)
            || self
                .meters
                .iter()
                .any(|meter| meter.tick > MAX_PATTERN_TICKS)
        {
            return Err(CommandError::invalid(
                "musical grids need a divisor from 1 to 96 and meters within the pattern limit",
            ));
        }
        let map = MeterMap::new(self.signature, &self.meters).map_err(CommandError::invalid)?;
        Ok(map
            .segments()
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                let signature = segment.signature();
                let ticks = match self.unit {
                    NoteGridUnit::Step => TICKS_PER_STEP,
                    NoteGridUnit::Beat => signature.ticks_per_beat(),
                    NoteGridUnit::Bar => signature.ticks_per_bar(),
                };
                let spacing = ((f64::from(ticks) / f64::from(self.divisor)).round() as u32).max(1);
                let end = map
                    .segments()
                    .get(index + 1)
                    .map_or(MAX_PATTERN_TICKS, |segment| segment.start_tick());
                (segment.start_tick(), end, spacing)
            })
            .collect())
    }
}
