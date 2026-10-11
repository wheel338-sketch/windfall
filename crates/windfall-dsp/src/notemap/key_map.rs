use super::{MappedNote, NoteTransform};
use crate::param::{ParamSet, param_set};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A destination for every MIDI key. Collisions retain each incoming note.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
pub struct KeyMapParams {
    #[serde(with = "key_table")]
    #[ts(as = "Vec<u8>")]
    pub table: [u8; 128],
}
impl Default for KeyMapParams {
    fn default() -> Self {
        Self {
            table: std::array::from_fn(|key| key as u8),
        }
    }
}
param_set!(KeyMapParams, "Key map", {
    int [table[0]] "table.0" "Key 0" { None, 0, 127, 0 }
    int [table[1]] "table.1" "Key 1" { None, 0, 127, 1 }
    int [table[2]] "table.2" "Key 2" { None, 0, 127, 2 }
    int [table[3]] "table.3" "Key 3" { None, 0, 127, 3 }
    int [table[4]] "table.4" "Key 4" { None, 0, 127, 4 }
    int [table[5]] "table.5" "Key 5" { None, 0, 127, 5 }
    int [table[6]] "table.6" "Key 6" { None, 0, 127, 6 }
    int [table[7]] "table.7" "Key 7" { None, 0, 127, 7 }
    int [table[8]] "table.8" "Key 8" { None, 0, 127, 8 }
    int [table[9]] "table.9" "Key 9" { None, 0, 127, 9 }
    int [table[10]] "table.10" "Key 10" { None, 0, 127, 10 }
    int [table[11]] "table.11" "Key 11" { None, 0, 127, 11 }
    int [table[12]] "table.12" "Key 12" { None, 0, 127, 12 }
    int [table[13]] "table.13" "Key 13" { None, 0, 127, 13 }
    int [table[14]] "table.14" "Key 14" { None, 0, 127, 14 }
    int [table[15]] "table.15" "Key 15" { None, 0, 127, 15 }
    int [table[16]] "table.16" "Key 16" { None, 0, 127, 16 }
    int [table[17]] "table.17" "Key 17" { None, 0, 127, 17 }
    int [table[18]] "table.18" "Key 18" { None, 0, 127, 18 }
    int [table[19]] "table.19" "Key 19" { None, 0, 127, 19 }
    int [table[20]] "table.20" "Key 20" { None, 0, 127, 20 }
    int [table[21]] "table.21" "Key 21" { None, 0, 127, 21 }
    int [table[22]] "table.22" "Key 22" { None, 0, 127, 22 }
    int [table[23]] "table.23" "Key 23" { None, 0, 127, 23 }
    int [table[24]] "table.24" "Key 24" { None, 0, 127, 24 }
    int [table[25]] "table.25" "Key 25" { None, 0, 127, 25 }
    int [table[26]] "table.26" "Key 26" { None, 0, 127, 26 }
    int [table[27]] "table.27" "Key 27" { None, 0, 127, 27 }
    int [table[28]] "table.28" "Key 28" { None, 0, 127, 28 }
    int [table[29]] "table.29" "Key 29" { None, 0, 127, 29 }
    int [table[30]] "table.30" "Key 30" { None, 0, 127, 30 }
    int [table[31]] "table.31" "Key 31" { None, 0, 127, 31 }
    int [table[32]] "table.32" "Key 32" { None, 0, 127, 32 }
    int [table[33]] "table.33" "Key 33" { None, 0, 127, 33 }
    int [table[34]] "table.34" "Key 34" { None, 0, 127, 34 }
    int [table[35]] "table.35" "Key 35" { None, 0, 127, 35 }
    int [table[36]] "table.36" "Key 36" { None, 0, 127, 36 }
    int [table[37]] "table.37" "Key 37" { None, 0, 127, 37 }
    int [table[38]] "table.38" "Key 38" { None, 0, 127, 38 }
    int [table[39]] "table.39" "Key 39" { None, 0, 127, 39 }
    int [table[40]] "table.40" "Key 40" { None, 0, 127, 40 }
    int [table[41]] "table.41" "Key 41" { None, 0, 127, 41 }
    int [table[42]] "table.42" "Key 42" { None, 0, 127, 42 }
    int [table[43]] "table.43" "Key 43" { None, 0, 127, 43 }
    int [table[44]] "table.44" "Key 44" { None, 0, 127, 44 }
    int [table[45]] "table.45" "Key 45" { None, 0, 127, 45 }
    int [table[46]] "table.46" "Key 46" { None, 0, 127, 46 }
    int [table[47]] "table.47" "Key 47" { None, 0, 127, 47 }
    int [table[48]] "table.48" "Key 48" { None, 0, 127, 48 }
    int [table[49]] "table.49" "Key 49" { None, 0, 127, 49 }
    int [table[50]] "table.50" "Key 50" { None, 0, 127, 50 }
    int [table[51]] "table.51" "Key 51" { None, 0, 127, 51 }
    int [table[52]] "table.52" "Key 52" { None, 0, 127, 52 }
    int [table[53]] "table.53" "Key 53" { None, 0, 127, 53 }
    int [table[54]] "table.54" "Key 54" { None, 0, 127, 54 }
    int [table[55]] "table.55" "Key 55" { None, 0, 127, 55 }
    int [table[56]] "table.56" "Key 56" { None, 0, 127, 56 }
    int [table[57]] "table.57" "Key 57" { None, 0, 127, 57 }
    int [table[58]] "table.58" "Key 58" { None, 0, 127, 58 }
    int [table[59]] "table.59" "Key 59" { None, 0, 127, 59 }
    int [table[60]] "table.60" "Key 60" { None, 0, 127, 60 }
    int [table[61]] "table.61" "Key 61" { None, 0, 127, 61 }
    int [table[62]] "table.62" "Key 62" { None, 0, 127, 62 }
    int [table[63]] "table.63" "Key 63" { None, 0, 127, 63 }
    int [table[64]] "table.64" "Key 64" { None, 0, 127, 64 }
    int [table[65]] "table.65" "Key 65" { None, 0, 127, 65 }
    int [table[66]] "table.66" "Key 66" { None, 0, 127, 66 }
    int [table[67]] "table.67" "Key 67" { None, 0, 127, 67 }
    int [table[68]] "table.68" "Key 68" { None, 0, 127, 68 }
    int [table[69]] "table.69" "Key 69" { None, 0, 127, 69 }
    int [table[70]] "table.70" "Key 70" { None, 0, 127, 70 }
    int [table[71]] "table.71" "Key 71" { None, 0, 127, 71 }
    int [table[72]] "table.72" "Key 72" { None, 0, 127, 72 }
    int [table[73]] "table.73" "Key 73" { None, 0, 127, 73 }
    int [table[74]] "table.74" "Key 74" { None, 0, 127, 74 }
    int [table[75]] "table.75" "Key 75" { None, 0, 127, 75 }
    int [table[76]] "table.76" "Key 76" { None, 0, 127, 76 }
    int [table[77]] "table.77" "Key 77" { None, 0, 127, 77 }
    int [table[78]] "table.78" "Key 78" { None, 0, 127, 78 }
    int [table[79]] "table.79" "Key 79" { None, 0, 127, 79 }
    int [table[80]] "table.80" "Key 80" { None, 0, 127, 80 }
    int [table[81]] "table.81" "Key 81" { None, 0, 127, 81 }
    int [table[82]] "table.82" "Key 82" { None, 0, 127, 82 }
    int [table[83]] "table.83" "Key 83" { None, 0, 127, 83 }
    int [table[84]] "table.84" "Key 84" { None, 0, 127, 84 }
    int [table[85]] "table.85" "Key 85" { None, 0, 127, 85 }
    int [table[86]] "table.86" "Key 86" { None, 0, 127, 86 }
    int [table[87]] "table.87" "Key 87" { None, 0, 127, 87 }
    int [table[88]] "table.88" "Key 88" { None, 0, 127, 88 }
    int [table[89]] "table.89" "Key 89" { None, 0, 127, 89 }
    int [table[90]] "table.90" "Key 90" { None, 0, 127, 90 }
    int [table[91]] "table.91" "Key 91" { None, 0, 127, 91 }
    int [table[92]] "table.92" "Key 92" { None, 0, 127, 92 }
    int [table[93]] "table.93" "Key 93" { None, 0, 127, 93 }
    int [table[94]] "table.94" "Key 94" { None, 0, 127, 94 }
    int [table[95]] "table.95" "Key 95" { None, 0, 127, 95 }
    int [table[96]] "table.96" "Key 96" { None, 0, 127, 96 }
    int [table[97]] "table.97" "Key 97" { None, 0, 127, 97 }
    int [table[98]] "table.98" "Key 98" { None, 0, 127, 98 }
    int [table[99]] "table.99" "Key 99" { None, 0, 127, 99 }
    int [table[100]] "table.100" "Key 100" { None, 0, 127, 100 }
    int [table[101]] "table.101" "Key 101" { None, 0, 127, 101 }
    int [table[102]] "table.102" "Key 102" { None, 0, 127, 102 }
    int [table[103]] "table.103" "Key 103" { None, 0, 127, 103 }
    int [table[104]] "table.104" "Key 104" { None, 0, 127, 104 }
    int [table[105]] "table.105" "Key 105" { None, 0, 127, 105 }
    int [table[106]] "table.106" "Key 106" { None, 0, 127, 106 }
    int [table[107]] "table.107" "Key 107" { None, 0, 127, 107 }
    int [table[108]] "table.108" "Key 108" { None, 0, 127, 108 }
    int [table[109]] "table.109" "Key 109" { None, 0, 127, 109 }
    int [table[110]] "table.110" "Key 110" { None, 0, 127, 110 }
    int [table[111]] "table.111" "Key 111" { None, 0, 127, 111 }
    int [table[112]] "table.112" "Key 112" { None, 0, 127, 112 }
    int [table[113]] "table.113" "Key 113" { None, 0, 127, 113 }
    int [table[114]] "table.114" "Key 114" { None, 0, 127, 114 }
    int [table[115]] "table.115" "Key 115" { None, 0, 127, 115 }
    int [table[116]] "table.116" "Key 116" { None, 0, 127, 116 }
    int [table[117]] "table.117" "Key 117" { None, 0, 127, 117 }
    int [table[118]] "table.118" "Key 118" { None, 0, 127, 118 }
    int [table[119]] "table.119" "Key 119" { None, 0, 127, 119 }
    int [table[120]] "table.120" "Key 120" { None, 0, 127, 120 }
    int [table[121]] "table.121" "Key 121" { None, 0, 127, 121 }
    int [table[122]] "table.122" "Key 122" { None, 0, 127, 122 }
    int [table[123]] "table.123" "Key 123" { None, 0, 127, 123 }
    int [table[124]] "table.124" "Key 124" { None, 0, 127, 124 }
    int [table[125]] "table.125" "Key 125" { None, 0, 127, 125 }
    int [table[126]] "table.126" "Key 126" { None, 0, 127, 126 }
    int [table[127]] "table.127" "Key 127" { None, 0, 127, 127 }
});

#[derive(Debug, Clone, Copy, Default)]
pub struct KeyMap {
    params: KeyMapParams,
}
impl NoteTransform for KeyMap {
    type Params = KeyMapParams;
    fn set_params(&mut self, params: &Self::Params) {
        self.params = params.sanitized();
    }
    fn map(&self, note: MappedNote) -> Option<MappedNote> {
        let mut note = note.sanitized();
        note.key = self.params.table[usize::from(note.key)];
        Some(note)
    }
}

// Serde's built-in array implementations stop at 32 entries.
// Decode exactly 128 entries directly into fixed storage.
mod key_table {
    use serde::de::{Error, IgnoredAny, SeqAccess, Visitor};
    use serde::{Deserializer, Serialize, Serializer};
    use std::fmt;

    pub fn serialize<S: Serializer>(table: &[u8; 128], serializer: S) -> Result<S::Ok, S::Error> {
        table.as_slice().serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<[u8; 128], D::Error> {
        struct Table;
        impl<'de> Visitor<'de> for Table {
            type Value = [u8; 128];
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("exactly 128 MIDI key destinations")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut table = [0; 128];
                for (index, key) in table.iter_mut().enumerate() {
                    *key = seq
                        .next_element()?
                        .ok_or_else(|| A::Error::invalid_length(index, &self))?;
                }
                if seq.next_element::<IgnoredAny>()?.is_some() {
                    return Err(A::Error::invalid_length(129, &self));
                }
                Ok(table)
            }
        }
        deserializer.deserialize_seq(Table)
    }
}
