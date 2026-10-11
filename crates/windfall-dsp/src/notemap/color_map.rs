use super::{MappedNote, NoteTransform};
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Sixteen color destinations, each 0..15. Default is identity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct ColorMapParams {
    #[ts(as = "Vec<u8>")]
    pub table: [u8; 16],
}
impl Default for ColorMapParams {
    fn default() -> Self {
        Self {
            table: std::array::from_fn(|key| key as u8),
        }
    }
}
param_set!(ColorMapParams, "Color map", {
    int [table[0]] "table.0" "Color 0" { None, 0, 15, 0 }
    int [table[1]] "table.1" "Color 1" { None, 0, 15, 1 }
    int [table[2]] "table.2" "Color 2" { None, 0, 15, 2 }
    int [table[3]] "table.3" "Color 3" { None, 0, 15, 3 }
    int [table[4]] "table.4" "Color 4" { None, 0, 15, 4 }
    int [table[5]] "table.5" "Color 5" { None, 0, 15, 5 }
    int [table[6]] "table.6" "Color 6" { None, 0, 15, 6 }
    int [table[7]] "table.7" "Color 7" { None, 0, 15, 7 }
    int [table[8]] "table.8" "Color 8" { None, 0, 15, 8 }
    int [table[9]] "table.9" "Color 9" { None, 0, 15, 9 }
    int [table[10]] "table.10" "Color 10" { None, 0, 15, 10 }
    int [table[11]] "table.11" "Color 11" { None, 0, 15, 11 }
    int [table[12]] "table.12" "Color 12" { None, 0, 15, 12 }
    int [table[13]] "table.13" "Color 13" { None, 0, 15, 13 }
    int [table[14]] "table.14" "Color 14" { None, 0, 15, 14 }
    int [table[15]] "table.15" "Color 15" { None, 0, 15, 15 }
});

#[derive(Debug, Clone, Copy, Default)]
pub struct ColorMap {
    params: ColorMapParams,
}
impl NoteTransform for ColorMap {
    type Params = ColorMapParams;
    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
    }
    fn map(&self, note: MappedNote) -> Option<MappedNote> {
        let mut note = note.sanitized();
        note.color = self.params.table[usize::from(note.color)];
        Some(note)
    }
}
