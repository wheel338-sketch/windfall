//! Worker-side RIFF/SF2 parsing. No file IO, native libraries or hidden truncation.
use super::{
    BankLoop as LoopMode, LoadedZone, SampleBank, SampleZone as Zone, MAX_BANK_BYTES,
    MAX_BANK_ZONES as MAX_ZONES,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_core::AudioBuffer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundFontError {
    Truncated,
    InvalidRiff,
    MissingChunk([u8; 4]),
    DuplicateChunk([u8; 4]),
    InvalidTable([u8; 4]),
    UnsupportedVersion { major: u16, minor: u16 },
    PresetNotFound { bank: u16, preset: u16 },
    UnsupportedGenerator(u16),
    UnsupportedModulators,
    UnsupportedSampleType(u16),
    UnsupportedSampleRate(u32),
    UnsupportedTuning(i32),
    Unsupported24BitSamples,
    InvalidSample(usize),
    TooManyZones,
    TooManyFrames { sample: usize, frames: usize },
    NoZones,
}
impl std::fmt::Display for SoundFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "SoundFont import failed: {self:?}")
    }
}
impl std::error::Error for SoundFontError {}

/// Imports the first preset in file order. Does not merge unrelated presets.
pub fn parse_soundfont(bytes: &[u8]) -> Result<SampleBank, SoundFontError> {
    let tables = Tables::read(bytes)?;
    tables.import(0)
}

/// Imports a specific SF2 bank/program pair into the bounded Copy zone table.
/// Only the documented mono PCM/generator subset is supported.
pub fn parse_soundfont_preset(
    bytes: &[u8],
    bank: u16,
    preset: u16,
) -> Result<SampleBank, SoundFontError> {
    let tables = Tables::read(bytes)?;
    for index in 0..tables.phdr.len() / 38 - 1 {
        let record = &tables.phdr[index * 38..(index + 1) * 38];
        if u16_at(record, 20) == preset && u16_at(record, 22) == bank {
            return tables.import(index);
        }
    }
    Err(SoundFontError::PresetNotFound { bank, preset })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SoundFontPreset {
    pub name: String,
    pub bank: u16,
    pub program: u16,
}
/// Validates all hydra references before exposing a bank's preset browser.
pub fn presets(bytes: &[u8]) -> Result<Vec<SoundFontPreset>, SoundFontError> {
    let tables = Tables::read(bytes)?;
    Ok(tables
        .phdr
        .chunks_exact(38)
        .take(tables.phdr.len() / 38 - 1)
        .map(|r| {
            let end = r[..20].iter().position(|&x| x == 0).unwrap_or(20);
            SoundFontPreset {
                name: String::from_utf8_lossy(&r[..end]).into_owned(),
                program: u16_at(r, 20),
                bank: u16_at(r, 22),
            }
        })
        .collect())
}

fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

/// Visits every chunk, including unknown ones, and checks its declared payload/padding.
fn chunks<'a>(
    bytes: &'a [u8],
    mut visit: impl FnMut([u8; 4], &'a [u8]) -> Result<(), SoundFontError>,
) -> Result<(), SoundFontError> {
    let mut position = 0;
    while position < bytes.len() {
        if bytes.len() - position < 8 {
            return Err(SoundFontError::Truncated);
        }
        let id = bytes[position..position + 4]
            .try_into()
            .map_err(|_| SoundFontError::Truncated)?;
        let size = u32_at(bytes, position + 4) as usize;
        let start = position + 8;
        let end = start.checked_add(size).ok_or(SoundFontError::Truncated)?;
        let next = end.checked_add(size & 1).ok_or(SoundFontError::Truncated)?;
        if next > bytes.len() {
            return Err(SoundFontError::Truncated);
        }
        visit(id, &bytes[start..end])?;
        position = next;
    }
    Ok(())
}

const LEAVES: [[u8; 4]; 11] = [
    *b"ifil", *b"smpl", *b"phdr", *b"pbag", *b"pmod", *b"pgen", *b"inst", *b"ibag", *b"imod",
    *b"igen", *b"shdr",
];
struct Tables<'a> {
    smpl: &'a [u8],
    phdr: &'a [u8],
    pbag: &'a [u8],
    pmod: &'a [u8],
    pgen: &'a [u8],
    inst: &'a [u8],
    ibag: &'a [u8],
    imod: &'a [u8],
    igen: &'a [u8],
    shdr: &'a [u8],
}
impl<'a> Tables<'a> {
    fn read(bytes: &'a [u8]) -> Result<Self, SoundFontError> {
        if bytes.len() < 12 {
            return Err(SoundFontError::Truncated);
        }
        if &bytes[..4] != b"RIFF" || &bytes[8..12] != b"sfbk" {
            return Err(SoundFontError::InvalidRiff);
        }
        let end = (u32_at(bytes, 4) as usize)
            .checked_add(8)
            .ok_or(SoundFontError::Truncated)?;
        if end > bytes.len() {
            return Err(SoundFontError::Truncated);
        }
        if end != bytes.len() || end < 12 {
            return Err(SoundFontError::InvalidRiff);
        }
        let mut leaves = [None; LEAVES.len()];
        let mut lists = [false; 3];
        chunks(&bytes[12..], |id, data| {
            if id != *b"LIST" {
                return Ok(());
            }
            if data.len() < 4 {
                return Err(SoundFontError::Truncated);
            }
            let list_id = &data[..4];
            let list = match list_id {
                b"INFO" => Some(0),
                b"sdta" => Some(1),
                b"pdta" => Some(2),
                _ => None,
            };
            if let Some(index) = list {
                if lists[index] {
                    return Err(SoundFontError::DuplicateChunk(list_id.try_into().unwrap()));
                }
                lists[index] = true;
            }
            chunks(&data[4..], |id, data| {
                if list == Some(1) && id == *b"sm24" {
                    return Err(SoundFontError::Unsupported24BitSamples);
                }
                if let Some(index) = LEAVES.iter().position(|candidate| *candidate == id) {
                    let expected_list = if index == 0 {
                        0
                    } else if index == 1 {
                        1
                    } else {
                        2
                    };
                    if list != Some(expected_list) {
                        return Err(SoundFontError::InvalidTable(id));
                    }
                    if leaves[index].replace(data).is_some() {
                        return Err(SoundFontError::DuplicateChunk(id));
                    }
                }
                Ok(())
            })
        })?;
        for (index, id) in LEAVES.iter().enumerate() {
            if leaves[index].is_none() {
                return Err(SoundFontError::MissingChunk(*id));
            }
        }
        let get = |index: usize| leaves[index].unwrap();
        let version = get(0);
        if version.len() != 4 {
            return Err(SoundFontError::InvalidTable(*b"ifil"));
        }
        let major = u16_at(version, 0);
        let minor = u16_at(version, 2);
        if major != 2 || minor > 4 {
            return Err(SoundFontError::UnsupportedVersion { major, minor });
        }
        if get(1).len() > MAX_BANK_BYTES / 2 {
            return Err(SoundFontError::TooManyFrames {
                sample: 0,
                frames: get(1).len() / 2,
            });
        }
        let tables = Self {
            smpl: get(1),
            phdr: get(2),
            pbag: get(3),
            pmod: get(4),
            pgen: get(5),
            inst: get(6),
            ibag: get(7),
            imod: get(8),
            igen: get(9),
            shdr: get(10),
        };
        tables.validate()?;
        Ok(tables)
    }
    fn validate(&self) -> Result<(), SoundFontError> {
        for (id, data, width, minimum) in [
            (*b"smpl", self.smpl, 2, 1),
            (*b"phdr", self.phdr, 38, 2),
            (*b"pbag", self.pbag, 4, 2),
            (*b"pmod", self.pmod, 10, 1),
            (*b"pgen", self.pgen, 4, 1),
            (*b"inst", self.inst, 22, 2),
            (*b"ibag", self.ibag, 4, 2),
            (*b"imod", self.imod, 10, 1),
            (*b"igen", self.igen, 4, 1),
            (*b"shdr", self.shdr, 46, 2),
        ] {
            if data.len() % width != 0 || data.len() / width < minimum {
                return Err(SoundFontError::InvalidTable(id));
            }
        }
        validate_headers(self.phdr, 38, 24, self.pbag.len() / 4, *b"phdr")?;
        validate_headers(self.inst, 22, 20, self.ibag.len() / 4, *b"inst")?;
        validate_bags(
            self.pbag,
            self.pgen.len() / 4,
            self.pmod.len() / 10,
            *b"pbag",
        )?;
        validate_bags(
            self.ibag,
            self.igen.len() / 4,
            self.imod.len() / 10,
            *b"ibag",
        )?;
        for record in self
            .pgen
            .as_chunks::<4>()
            .0
            .iter()
            .take(self.pgen.len() / 4 - 1)
        {
            if u16_at(record, 0) == 41 && u16_at(record, 2) as usize >= self.inst.len() / 22 - 1 {
                return Err(SoundFontError::InvalidTable(*b"pgen"));
            }
        }
        for record in self
            .igen
            .as_chunks::<4>()
            .0
            .iter()
            .take(self.igen.len() / 4 - 1)
        {
            if u16_at(record, 0) == 53 && u16_at(record, 2) as usize >= self.shdr.len() / 46 - 1 {
                return Err(SoundFontError::InvalidTable(*b"igen"));
            }
        }
        for (index, header) in self
            .shdr
            .as_chunks::<46>()
            .0
            .iter()
            .take(self.shdr.len() / 46 - 1)
            .enumerate()
        {
            let start = u32_at(header, 20) as usize;
            let end = u32_at(header, 24) as usize;
            let loop_start = u32_at(header, 28) as usize;
            let loop_end = u32_at(header, 32) as usize;
            if start >= end
                || end > self.smpl.len() / 2
                || loop_start < start
                || loop_start > loop_end
                || loop_end > end
            {
                return Err(SoundFontError::InvalidSample(index));
            }
        }
        Ok(())
    }
    fn import(&self, preset: usize) -> Result<SampleBank, SoundFontError> {
        let begin = u16_at(self.phdr, preset * 38 + 24) as usize;
        let end = u16_at(self.phdr, (preset + 1) * 38 + 24) as usize;
        let mut table = Vec::new();
        let mut global = Generators::default();
        for bag in begin..end {
            let local = generators(self.pbag, self.pgen, bag, true)?;
            if local.link.is_none() {
                if bag != begin {
                    return Err(SoundFontError::InvalidTable(*b"pgen"));
                }
                global = local;
                continue;
            }
            let preset = global.overridden(local);
            self.import_instrument(preset, &mut table)?;
        }
        if table.is_empty() {
            Err(SoundFontError::NoZones)
        } else {
            SampleBank::new(table).map_err(|_| SoundFontError::TooManyZones)
        }
    }
    fn import_instrument(
        &self,
        preset: Generators,
        table: &mut Vec<LoadedZone>,
    ) -> Result<(), SoundFontError> {
        let instrument = preset.link.ok_or(SoundFontError::InvalidTable(*b"pgen"))?;
        let begin = u16_at(self.inst, instrument * 22 + 20) as usize;
        let end = u16_at(self.inst, (instrument + 1) * 22 + 20) as usize;
        let mut global = Generators::default();
        for bag in begin..end {
            let local = generators(self.ibag, self.igen, bag, false)?;
            if local.link.is_none() {
                if bag != begin {
                    return Err(SoundFontError::InvalidTable(*b"igen"));
                }
                global = local;
                continue;
            }
            let instrument = global.overridden(local);
            let key = intersection(
                preset.key.unwrap_or((0, 127)),
                instrument.key.unwrap_or((0, 127)),
            );
            let velocity = intersection(
                preset.velocity.unwrap_or((0, 127)),
                instrument.velocity.unwrap_or((0, 127)),
            );
            if key.0 > key.1 || velocity.0 > velocity.1 {
                continue;
            }
            if table.len() == MAX_ZONES {
                return Err(SoundFontError::TooManyZones);
            }
            let used: usize = table.iter().map(|z| z.audio.samples().len() * 4).sum();
            let mut zone = self.sample_zone(instrument, MAX_BANK_BYTES.saturating_sub(used))?;
            zone.mapping.key_low = key.0;
            zone.mapping.key_high = key.1;
            zone.mapping.velocity_low = velocity.0;
            zone.mapping.velocity_high = velocity.1;
            let attenuation =
                preset.attenuation.unwrap_or(0) as f32 + instrument.attenuation.unwrap_or(0) as f32;
            zone.mapping.gain = 10.0_f32.powf(-attenuation / 200.0);
            zone.mapping.pan =
                ((preset.pan.unwrap_or(0) as f32 + instrument.pan.unwrap_or(0) as f32) / 500.0)
                    .clamp(-1.0, 1.0);
            zone.mapping.tune_cents +=
                (preset.coarse.unwrap_or(0) as f32 + instrument.coarse.unwrap_or(0) as f32) * 100.0
                    + preset.fine.unwrap_or(0) as f32
                    + instrument.fine.unwrap_or(0) as f32;
            if zone.mapping.tune_cents.abs() > 12_000.0 {
                return Err(SoundFontError::UnsupportedTuning(
                    zone.mapping.tune_cents as i32,
                ));
            }
            let tc = |slot: usize| -> f32 {
                let value = i32::from(instrument.envelope[slot].unwrap_or(if slot == 4 {
                    0
                } else {
                    -12000
                })) + i32::from(preset.envelope[slot].unwrap_or(0));
                if slot == 4 {
                    10.0_f32.powf(-(value as f32).max(0.0) / 200.0)
                } else if value <= -32768 {
                    0.0
                } else {
                    (1000.0 * 2.0_f32.powf(value as f32 / 1200.0)).min(100000.0)
                }
            };
            zone.mapping.attack_ms = tc(1);
            zone.mapping.hold_ms = tc(2);
            zone.mapping.decay_ms = tc(3);
            zone.mapping.sustain = tc(4);
            zone.mapping.release_ms = tc(5);
            // DelayVolEnv is not representable by this engine yet.
            if instrument.envelope[0].is_some_and(|x| x > -12000)
                || preset.envelope[0].is_some_and(|x| x != 0)
            {
                return Err(SoundFontError::UnsupportedGenerator(33));
            }
            zone.mapping
                .validate()
                .map_err(|_| SoundFontError::InvalidSample(table.len()))?;
            table.push(zone);
        }
        Ok(())
    }
    fn sample_zone(
        &self,
        instrument: Generators,
        remaining: usize,
    ) -> Result<LoadedZone, SoundFontError> {
        let index = instrument
            .link
            .ok_or(SoundFontError::InvalidTable(*b"igen"))?;
        let header = &self.shdr[index * 46..(index + 1) * 46];
        let sample_type = u16_at(header, 44);
        if sample_type != 1 {
            return Err(SoundFontError::UnsupportedSampleType(sample_type));
        }
        let address = |field: usize, fine: usize, coarse: usize| -> Result<usize, SoundFontError> {
            let value = i64::from(u32_at(header, field))
                + i64::from(instrument.offsets[fine].unwrap_or(0))
                + i64::from(instrument.offsets[coarse].unwrap_or(0)) * 32768;
            if value < 0 || value > (self.smpl.len() / 2) as i64 {
                return Err(SoundFontError::InvalidSample(index));
            }
            Ok(value as usize)
        };
        let start = address(20, 0, 4)?;
        let end = address(24, 1, 5)?;
        if start >= end {
            return Err(SoundFontError::InvalidSample(index));
        }
        let frames = end - start;
        if frames > remaining / 4 {
            return Err(SoundFontError::TooManyFrames {
                sample: index,
                frames,
            });
        }
        let sample_rate = u32_at(header, 36);
        if !(1000..=384_000).contains(&sample_rate) {
            return Err(SoundFontError::UnsupportedSampleRate(sample_rate));
        }
        let root = instrument.root.unwrap_or(header[40] as i16);
        if !(0..=127).contains(&root) {
            return Err(SoundFontError::InvalidSample(index));
        }
        let pcm = (start..end)
            .map(|frame| u16_at(self.smpl, frame * 2) as i16 as f32 / 32768.0)
            .collect();
        let sample = AudioBuffer::from_interleaved(sample_rate, 1, pcm);
        let loop_start_abs = address(28, 2, 6)?;
        let loop_end_abs = address(32, 3, 7)?;
        let loop_start = loop_start_abs.saturating_sub(start) as u32;
        let loop_end = loop_end_abs.saturating_sub(start) as u32;
        let loop_mode = match instrument.mode.unwrap_or(0) {
            0 | 2 => LoopMode::Off,
            1 => LoopMode::Continuous,
            3 => LoopMode::UntilRelease,
            _ => return Err(SoundFontError::UnsupportedGenerator(54)),
        };
        if loop_mode != LoopMode::Off
            && (loop_start >= loop_end || loop_start_abs < start || loop_end_abs > end)
        {
            return Err(SoundFontError::InvalidSample(index));
        }
        Ok(LoadedZone {
            audio: sample,
            mapping: Zone {
                root_key: root as u8,
                tune_cents: header[41] as i8 as f32,
                loop_mode,
                loop_start,
                loop_end,
                scale_tuning: instrument.scale.unwrap_or(100) as f32,
                choke_group: instrument.choke.unwrap_or(0).min(255) as u8,
                ..Zone::default()
            },
        })
    }
}

fn validate_headers(
    data: &[u8],
    width: usize,
    field: usize,
    bags: usize,
    id: [u8; 4],
) -> Result<(), SoundFontError> {
    let mut previous = 0;
    for (index, record) in data.chunks_exact(width).enumerate() {
        let value = u16_at(record, field) as usize;
        if (index == 0 && value != 0) || value < previous || value >= bags {
            return Err(SoundFontError::InvalidTable(id));
        }
        previous = value;
    }
    if previous != bags - 1 {
        return Err(SoundFontError::InvalidTable(id));
    }
    Ok(())
}
fn validate_bags(
    data: &[u8],
    generators: usize,
    modulators: usize,
    id: [u8; 4],
) -> Result<(), SoundFontError> {
    let (mut previous_gen, mut previous_mod) = (0, 0);
    for (index, record) in data.as_chunks::<4>().0.iter().enumerate() {
        let gen_index = u16_at(record, 0) as usize;
        let mod_index = u16_at(record, 2) as usize;
        if (index == 0 && (gen_index != 0 || mod_index != 0))
            || gen_index < previous_gen
            || mod_index < previous_mod
            || gen_index >= generators
            || mod_index >= modulators
        {
            return Err(SoundFontError::InvalidTable(id));
        }
        previous_gen = gen_index;
        previous_mod = mod_index;
    }
    if previous_gen != generators - 1 || previous_mod != modulators - 1 {
        return Err(SoundFontError::InvalidTable(id));
    }
    Ok(())
}
fn intersection(a: (u8, u8), b: (u8, u8)) -> (u8, u8) {
    (a.0.max(b.0), a.1.min(b.1))
}

#[derive(Debug, Clone, Copy, Default)]
struct Generators {
    key: Option<(u8, u8)>,
    velocity: Option<(u8, u8)>,
    pan: Option<i16>,
    attenuation: Option<u16>,
    coarse: Option<i16>,
    fine: Option<i16>,
    mode: Option<u16>,
    root: Option<i16>,
    link: Option<usize>,
    envelope: [Option<i16>; 6],
    offsets: [Option<i16>; 8],
    scale: Option<u16>,
    choke: Option<u16>,
}
impl Generators {
    fn overridden(self, local: Self) -> Self {
        Self {
            key: local.key.or(self.key),
            velocity: local.velocity.or(self.velocity),
            pan: local.pan.or(self.pan),
            attenuation: local.attenuation.or(self.attenuation),
            coarse: local.coarse.or(self.coarse),
            fine: local.fine.or(self.fine),
            mode: local.mode.or(self.mode),
            root: local.root.or(self.root),
            link: local.link,
            envelope: std::array::from_fn(|i| local.envelope[i].or(self.envelope[i])),
            offsets: std::array::from_fn(|i| local.offsets[i].or(self.offsets[i])),
            scale: local.scale.or(self.scale),
            choke: local.choke.or(self.choke),
        }
    }
}
fn generators(
    bags: &[u8],
    data: &[u8],
    bag: usize,
    preset: bool,
) -> Result<Generators, SoundFontError> {
    let id = if preset { *b"pgen" } else { *b"igen" };
    let begin = u16_at(bags, bag * 4) as usize;
    let end = u16_at(bags, (bag + 1) * 4) as usize;
    if u16_at(bags, bag * 4 + 2) != u16_at(bags, (bag + 1) * 4 + 2) {
        return Err(SoundFontError::UnsupportedModulators);
    }
    let mut result = Generators::default();
    let mut seen = 0_u64;
    for index in begin..end {
        let op = u16_at(data, index * 4);
        let amount = u16_at(data, index * 4 + 2);
        if op > 60 {
            return Err(SoundFontError::UnsupportedGenerator(op));
        }
        if seen & (1_u64 << op) != 0 {
            return Err(SoundFontError::InvalidTable(id));
        }
        seen |= 1_u64 << op;
        match op {
            43 | 44 => {
                let range = (amount as u8, (amount >> 8) as u8);
                if range.0 > range.1 || range.1 > 127 {
                    return Err(SoundFontError::InvalidTable(id));
                }
                if op == 43 {
                    if index != begin {
                        return Err(SoundFontError::InvalidTable(id));
                    }
                    result.key = Some(range);
                } else {
                    if index != begin && !(index == begin + 1 && result.key.is_some()) {
                        return Err(SoundFontError::InvalidTable(id));
                    }
                    result.velocity = Some(range);
                }
            }
            17 => {
                if !(-500..=500).contains(&(amount as i16)) {
                    return Err(SoundFontError::InvalidTable(id));
                }
                result.pan = Some(amount as i16);
            }
            48 => {
                if amount > 1440 {
                    return Err(SoundFontError::InvalidTable(id));
                }
                result.attenuation = Some(amount);
            }
            51 => {
                if !(-120..=120).contains(&(amount as i16)) {
                    return Err(SoundFontError::InvalidTable(id));
                }
                result.coarse = Some(amount as i16);
            }
            52 => {
                if !(-99..=99).contains(&(amount as i16)) {
                    return Err(SoundFontError::InvalidTable(id));
                }
                result.fine = Some(amount as i16);
            }
            54 if !preset => result.mode = Some(amount),
            58 if !preset => {
                result.root = if amount as i16 == -1 {
                    None
                } else {
                    Some(amount as i16)
                }
            }
            33..=38 => result.envelope[(op - 33) as usize] = Some(amount as i16),
            0..=4 | 12 | 45 | 50 if !preset => {
                let slot = match op {
                    0..=4 => op as usize,
                    12 => 5,
                    45 => 6,
                    _ => 7,
                };
                result.offsets[slot] = Some(amount as i16);
            }
            56 if !preset && amount <= 1200 => result.scale = Some(amount),
            57 if !preset && amount <= 127 => result.choke = Some(amount),
            41 if preset => {
                if index + 1 != end {
                    return Err(SoundFontError::InvalidTable(id));
                }
                result.link = Some(amount as usize);
            }
            53 if !preset => {
                if index + 1 != end {
                    return Err(SoundFontError::InvalidTable(id));
                }
                result.link = Some(amount as usize);
            }
            _ => return Err(SoundFontError::UnsupportedGenerator(op)),
        }
    }
    Ok(result)
}
