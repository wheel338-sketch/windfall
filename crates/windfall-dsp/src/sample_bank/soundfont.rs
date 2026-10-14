//! Worker-side RIFF/SF2 parsing. No file IO, native libraries or hidden truncation.
use super::{
    BankLoop as LoopMode, LoadedZone, MAX_BANK_BYTES, MAX_BANK_ZONES as MAX_ZONES, SampleBank,
    SampleZone as Zone,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_core::AudioBuffer;

#[derive(Debug, Clone, PartialEq, Eq)]
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
    MemoryBudget(String),
}
impl std::fmt::Display for SoundFontError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let notice = windfall_core::notice!("dsp.soundFontImport", detail = format!("{self:?}"));
        write!(f, "{notice}")
    }
}
impl std::error::Error for SoundFontError {}

/// Imports the first preset in file order. Does not merge unrelated presets.
pub fn parse_soundfont(bytes: &[u8]) -> Result<SampleBank, SoundFontError> {
    let tables = Tables::read(bytes)?;
    tables.import(0, None)
}

/// Imports a specific SF2 bank/program pair into the bounded Copy zone table.
/// Resolves SF2 2.04 generators and controller precedence before publication.
pub fn parse_soundfont_preset(
    bytes: &[u8],
    bank: u16,
    preset: u16,
) -> Result<SampleBank, SoundFontError> {
    let tables = Tables::read(bytes)?;
    for index in 0..tables.phdr.len() / 38 - 1 {
        let record = &tables.phdr[index * 38..(index + 1) * 38];
        if u16_at(record, 20) == preset && u16_at(record, 22) == bank {
            return tables.import(index, None);
        }
    }
    Err(SoundFontError::PresetNotFound { bank, preset })
}

/// Reserve every decoded zone before allocating its PCM; partial failures release
/// all reservations on the worker, and successful banks carry them until retired.
pub fn parse_soundfont_preset_budgeted(
    bytes: &[u8],
    bank: u16,
    preset: u16,
    budget: &std::sync::Arc<super::BankBudget>,
) -> Result<SampleBank, SoundFontError> {
    let tables = Tables::read(bytes)?;
    for index in 0..tables.phdr.len() / 38 - 1 {
        let record = &tables.phdr[index * 38..(index + 1) * 38];
        if u16_at(record, 20) == preset && u16_at(record, 22) == bank {
            return tables.import(index, Some(budget));
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
        .as_chunks::<38>()
        .0
        .iter()
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
    sm24: Option<&'a [u8]>,
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
        let mut sm24 = None;
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
                    if sm24.replace(data).is_some() {
                        return Err(SoundFontError::DuplicateChunk(id));
                    }
                    return Ok(());
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
            sm24,
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
        if self
            .sm24
            .is_some_and(|bytes| bytes.len() != self.smpl.len() / 2)
        {
            return Err(SoundFontError::InvalidTable(*b"sm24"));
        }
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
            let kind = u16_at(header, 44);
            if matches!(kind, 2 | 4 | 8) {
                let link = u16_at(header, 42) as usize;
                if link == index || link >= self.shdr.len() / 46 - 1 {
                    return Err(SoundFontError::InvalidSample(index));
                }
                let other = &self.shdr[link * 46..(link + 1) * 46];
                let expected = match kind {
                    2 => 4,
                    4 => 2,
                    _ => 8,
                };
                if u16_at(other, 44) != expected || u16_at(other, 42) as usize != index {
                    return Err(SoundFontError::InvalidSample(index));
                }
            }
            if start >= end
                || end > self.smpl.len() / 2
                || loop_start > loop_end
                || loop_start > self.smpl.len() / 2
                || loop_end > self.smpl.len() / 2
            {
                return Err(SoundFontError::InvalidSample(index));
            }
        }
        Ok(())
    }
    fn import(
        &self,
        preset: usize,
        budget: Option<&std::sync::Arc<super::BankBudget>>,
    ) -> Result<SampleBank, SoundFontError> {
        use super::sf2_voice::Region;
        let mut reservations = Vec::new();
        let metadata =
            MAX_ZONES * (std::mem::size_of::<LoadedZone>() + std::mem::size_of::<Region>());
        if let Some(b) = budget {
            reservations.push(b.reserve(metadata).map_err(SoundFontError::MemoryBudget)?);
        }
        let mut zones = Vec::with_capacity(MAX_ZONES);
        let mut regions = Vec::with_capacity(MAX_ZONES);
        let begin = u16_at(self.phdr, preset * 38 + 24) as usize;
        let end = u16_at(self.phdr, (preset + 1) * 38 + 24) as usize;
        let mut global = Generators::default();
        let mut global_mod = Vec::new();
        for bag in begin..end {
            let local = generators(self.pbag, self.pgen, bag, true)?;
            let local_mod = modulators(self.pbag, self.pmod, bag, budget)?;
            if local.link.is_none() {
                if bag != begin {
                    return Err(SoundFontError::InvalidTable(*b"pgen"));
                }
                global = local;
                global_mod = local_mod;
                continue;
            }
            let preset = global.overridden(local);
            let preset_mod = replace_modulators(global_mod.clone(), local_mod);
            let instrument = preset.link.ok_or(SoundFontError::InvalidTable(*b"pgen"))?;
            let ibegin = u16_at(self.inst, instrument * 22 + 20) as usize;
            let iend = u16_at(self.inst, (instrument + 1) * 22 + 20) as usize;
            let mut iglobal = Generators::default();
            let mut iglobal_mod = Vec::new();
            for ibag in ibegin..iend {
                let ilocal = generators(self.ibag, self.igen, ibag, false)?;
                let ilocal_mod = modulators(self.ibag, self.imod, ibag, budget)?;
                if ilocal.link.is_none() {
                    if ibag != ibegin {
                        return Err(SoundFontError::InvalidTable(*b"igen"));
                    }
                    iglobal = ilocal;
                    iglobal_mod = ilocal_mod;
                    continue;
                }
                let instrument = iglobal.overridden(ilocal);
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
                let mut mods = replace_modulators(
                    super::sf2_voice::defaults()
                        .into_iter()
                        .map(|m| OwnedModulator {
                            modulator: m,
                            program: None,
                        })
                        .collect(),
                    iglobal_mod.clone(),
                );
                mods = replace_modulators(mods, ilocal_mod);
                // Preset global/local replacement happened above. At this level
                // identical modulators add (including negative cancellation).
                for m in &preset_mod {
                    if let Some(old) = mods
                        .iter_mut()
                        .find(|old| old.modulator.identical(m.modulator))
                    {
                        if old.program == m.program {
                            old.modulator.amount += m.modulator.amount;
                        } else {
                            mods.push(m.clone());
                        }
                    } else {
                        mods.push(m.clone());
                    }
                }
                let mut values: [f32; super::sf2_voice::GENERATORS] = std::array::from_fn(|op| {
                    let absolute =
                        instrument.values[op].map_or(super::sf2_voice::limits(op).2, |v| v as f32);
                    if matches!(op,21|23|25..=28|30|33..=36|38) && absolute <= -32768.0 {
                        absolute
                    } else if super::sf2_voice::value_generator(op) {
                        absolute + preset.values[op].unwrap_or(0) as f32
                    } else {
                        absolute
                    }
                });
                for op in [46, 47, 58] {
                    values[op] = values[op].clamp(-1.0, 127.0);
                }
                if values[47] == 0.0 {
                    values[47] = 1.0;
                }
                let index = instrument
                    .link
                    .ok_or(SoundFontError::InvalidTable(*b"igen"))?;
                let header = &self.shdr[index * 46..(index + 1) * 46];
                let kind = u16_at(header, 44);
                let partner = if matches!(kind, 2 | 4 | 8) {
                    Some(u16_at(header, 42) as usize)
                } else {
                    None
                };
                // Paired hard-left/right zones encode the channel layout, not a
                // second musical pan offset. Both samples share one oscillator.
                let channel_pan = instrument.values[17].unwrap_or(0) as f32;
                if matches!(kind, 2 | 4) && channel_pan.abs() == 500.0 {
                    values[17] -= channel_pan;
                }
                let root = if values[58] >= 0.0 {
                    values[58] as u8
                } else if header[40] <= 127 {
                    header[40]
                } else {
                    60
                };
                if header[40] == 255 && values[58] < 0.0 {
                    values[56] = 0.0;
                }
                let mut programs = Vec::new();
                let mut routes = Vec::with_capacity(mods.len());
                for owned in mods {
                    let mut m = owned.modulator;
                    if let Some(program) = owned.program {
                        let slot = programs
                            .iter()
                            .position(|p| std::sync::Arc::ptr_eq(p, &program))
                            .unwrap_or_else(|| {
                                programs.push(program);
                                programs.len() - 1
                            });
                        if let Some((_, root)) = m.link {
                            m.link = Some((slot, root));
                        }
                    }
                    routes.push(m);
                }
                let mod_bytes = routes.len() * std::mem::size_of::<super::sf2_voice::Modulator>()
                    + programs.len()
                        * std::mem::size_of::<std::sync::Arc<super::sf2_voice::Program>>()
                    + programs.iter().map(|p| p.bytes()).sum::<usize>();
                let region_lease = budget
                    .map(|b| b.reserve(mod_bytes).map_err(SoundFontError::MemoryBudget))
                    .transpose()?;
                let mut region = Region {
                    generators: values,
                    modulators: routes.into_boxed_slice(),
                    programs: programs.into_boxed_slice(),
                    root,
                    correction: header[41] as i8,
                    loop_start: 0,
                    loop_end: 0,
                    mode: (instrument.values[54].unwrap_or(0) as u16 & 3) as u8,
                    sample: index,
                    partner,
                };
                if let Some(other) = partner
                    && regions
                        .iter()
                        .zip(&zones)
                        .any(|(r, z): (&Region, &LoadedZone)| {
                            r.sample == other
                                && r.partner == Some(index)
                                && r.generators == region.generators
                                && r.modulators == region.modulators
                                && r.programs == region.programs
                                && z.mapping.key_low == key.0
                                && z.mapping.key_high == key.1
                                && z.mapping.velocity_low == velocity.0
                                && z.mapping.velocity_high == velocity.1
                        })
                {
                    continue;
                }
                if zones.len() == MAX_ZONES {
                    return Err(SoundFontError::TooManyZones);
                }
                if let Some(lease) = region_lease {
                    reservations.push(lease);
                }
                let used: usize = zones
                    .iter()
                    .map(|z: &LoadedZone| z.audio.retained_pcm_bytes())
                    .sum();
                let mut zone = self.sample_zone(
                    instrument,
                    &mut region,
                    MAX_BANK_BYTES.saturating_sub(used + metadata),
                    budget,
                    &mut reservations,
                )?;
                zone.mapping.key_low = key.0;
                zone.mapping.key_high = key.1;
                zone.mapping.velocity_low = velocity.0;
                zone.mapping.velocity_high = velocity.1;
                let g = region.generators;
                zone.mapping.gain = super::sf2_voice::amplitude(g[48].clamp(0.0, 1440.0));
                zone.mapping.pan = (g[17] / 500.0).clamp(-1.0, 1.0);
                zone.mapping.tune_cents =
                    (region.correction as f32 + g[51] * 100.0 + g[52]).clamp(-12000.0, 12000.0);
                zone.mapping.scale_tuning = g[56].clamp(0.0, 1200.0);
                let tc = |op: usize| {
                    1000.0
                        * super::sf2_voice::seconds(if g[op] <= -32768.0 {
                            -32768.0
                        } else {
                            g[op].clamp(
                                -12000.0,
                                if matches!(op, 33 | 35) {
                                    5000.0
                                } else {
                                    8000.0
                                },
                            )
                        })
                        .min(100.0)
                };
                zone.mapping.delay_ms = tc(33);
                zone.mapping.attack_ms = tc(34);
                zone.mapping.hold_ms = tc(35);
                zone.mapping.decay_ms = tc(36);
                zone.mapping.release_ms = tc(38);
                zone.mapping.sustain = super::sf2_voice::amplitude(g[37].clamp(0.0, 1440.0));
                zone.mapping
                    .validate()
                    .map_err(|_| SoundFontError::InvalidSample(index))?;
                zones.push(zone);
                regions.push(region);
            }
        }
        if zones.is_empty() {
            return Err(SoundFontError::NoZones);
        }
        if budget.is_some() {
            reservations[0]
                .shrink(
                    zones.len()
                        * (std::mem::size_of::<LoadedZone>() + std::mem::size_of::<Region>()),
                )
                .map_err(SoundFontError::MemoryBudget)?;
        }
        let mut bank = SampleBank::new(zones).map_err(|_| SoundFontError::TooManyZones)?;
        bank.install_soundfont_regions(regions.into_boxed_slice())
            .map_err(|e| SoundFontError::MemoryBudget(e.into()))?;
        for r in reservations {
            bank.retain_reservation(r);
        }
        Ok(bank)
    }
    fn sample_zone(
        &self,
        instrument: Generators,
        region: &mut super::sf2_voice::Region,
        remaining: usize,
        budget: Option<&std::sync::Arc<super::BankBudget>>,
        reservations: &mut Vec<super::BankReservation>,
    ) -> Result<LoadedZone, SoundFontError> {
        let index = region.sample;
        let header = &self.shdr[index * 46..(index + 1) * 46];
        let kind = u16_at(header, 44);
        if !matches!(kind, 1 | 2 | 4 | 8) {
            return Err(SoundFontError::UnsupportedSampleType(kind));
        }
        let address =
            |h: &[u8], field: usize, fine: usize, coarse: usize| -> Result<usize, SoundFontError> {
                let value = u32_at(h, field) as i64
                    + instrument.values[fine].unwrap_or(0) as i64
                    + instrument.values[coarse].unwrap_or(0) as i64 * 32768;
                if value < 0 || value > (self.smpl.len() / 2) as i64 {
                    Err(SoundFontError::InvalidSample(index))
                } else {
                    Ok(value as usize)
                }
            };
        let start = address(header, 20, 0, 4)?;
        let end = address(header, 24, 1, 12)?;
        if start >= end {
            return Err(SoundFontError::InvalidSample(index));
        }
        let frames = end - start;
        let sample_rate = u32_at(header, 36);
        if !(400..=384000).contains(&sample_rate) {
            return Err(SoundFontError::UnsupportedSampleRate(sample_rate));
        }
        let ls = address(header, 28, 2, 45)?;
        let le = address(header, 32, 3, 50)?;
        if matches!(region.mode, 1 | 3) && (ls < start || ls >= le || le > end) {
            return Err(SoundFontError::InvalidSample(index));
        }
        region.loop_start = ls.saturating_sub(start) as u32;
        region.loop_end = le.saturating_sub(start) as u32;
        let linked = region.partner.map(|p| &self.shdr[p * 46..(p + 1) * 46]);
        let channels = if linked.is_some() { 2 } else { 1 };
        let linked_start = if let Some(h) = linked {
            let s = address(h, 20, 0, 4)?;
            let e = address(h, 24, 1, 12)?;
            if e.checked_sub(s) != Some(frames)
                || u32_at(h, 36) != sample_rate
                || h[40..42] != header[40..42]
                || (matches!(region.mode, 1 | 3)
                    && (address(h, 28, 2, 45)?.checked_sub(s) != ls.checked_sub(start)
                        || address(h, 32, 3, 50)?.checked_sub(s) != le.checked_sub(start)))
            {
                return Err(SoundFontError::InvalidSample(index));
            }
            s
        } else {
            start
        };
        if frames > remaining / (4 * channels) {
            return Err(SoundFontError::TooManyFrames {
                sample: index,
                frames,
            });
        }
        if let Some(b) = budget {
            reservations.push(
                b.reserve(frames * channels * 4)
                    .map_err(SoundFontError::MemoryBudget)?,
            );
        }
        let decode = |frame: usize| {
            let high = u16_at(self.smpl, frame * 2) as i16 as i32;
            if let Some(low) = self.sm24 {
                ((high << 8) | low[frame] as i32) as f32 / 8388608.0
            } else {
                high as f32 / 32768.0
            }
        };
        let mut pcm = Vec::with_capacity(frames * channels);
        for offset in 0..frames {
            if channels == 1 {
                pcm.push(decode(start + offset));
            } else if kind == 2 {
                pcm.push(decode(linked_start + offset));
                pcm.push(decode(start + offset));
            } else {
                pcm.push(decode(start + offset));
                pcm.push(decode(linked_start + offset));
            }
        }
        let mode = match region.mode {
            1 => LoopMode::Continuous,
            3 => LoopMode::UntilRelease,
            _ => LoopMode::Off,
        };
        Ok(LoadedZone {
            audio: AudioBuffer::from_interleaved(sample_rate, channels as u16, pcm),
            mapping: Zone {
                root_key: region.root,
                loop_mode: mode,
                loop_start: region.loop_start,
                loop_end: region.loop_end,
                choke_group: instrument.values[57].unwrap_or(0).clamp(0, 127) as u8,
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

#[derive(Debug, Clone, Copy)]
struct Generators {
    key: Option<(u8, u8)>,
    velocity: Option<(u8, u8)>,
    link: Option<usize>,
    values: [Option<i16>; super::sf2_voice::GENERATORS],
}
impl Default for Generators {
    fn default() -> Self {
        Self {
            key: None,
            velocity: None,
            link: None,
            values: [None; super::sf2_voice::GENERATORS],
        }
    }
}
impl Generators {
    fn overridden(self, local: Self) -> Self {
        Self {
            key: local.key.or(self.key),
            velocity: local.velocity.or(self.velocity),
            link: local.link,
            values: std::array::from_fn(|i| local.values[i].or(self.values[i])),
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
    let mut result = Generators::default();
    let mut seen = 0u64;
    for index in begin..end {
        let op = u16_at(data, index * 4);
        let amount = u16_at(data, index * 4 + 2);
        // Reserved/unknown generators are ignored, per section section 8.1/10.3.
        if op > 60 || matches!(op,14|18..=20|42|49|55|59..=60) {
            continue;
        }
        if seen & (1u64 << op) != 0 {
            return Err(SoundFontError::InvalidTable(id));
        }
        seen |= 1u64 << op;
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
            0..=4 | 12 | 45..=47 | 50 | 54 | 57..=58 if preset => {}
            41 | 53 => {}
            _ => {
                result.values[op as usize] = Some(match op {
                    0 | 4 => (amount as i16).max(0),
                    1 | 12 => (amount as i16).min(0),
                    _ => amount as i16,
                })
            }
        }
    }
    Ok(result)
}
#[derive(Clone)]
struct OwnedModulator {
    modulator: super::sf2_voice::Modulator,
    program: Option<std::sync::Arc<super::sf2_voice::Program>>,
}
fn replace_modulators(
    mut base: Vec<OwnedModulator>,
    local: Vec<OwnedModulator>,
) -> Vec<OwnedModulator> {
    for m in local {
        if let Some(old) = base
            .iter_mut()
            .find(|old| old.modulator.identical(m.modulator))
        {
            *old = m;
        } else {
            base.push(m);
        }
    }
    base
}
fn modulators(
    bags: &[u8],
    data: &[u8],
    bag: usize,
    budget: Option<&std::sync::Arc<super::BankBudget>>,
) -> Result<Vec<OwnedModulator>, SoundFontError> {
    use super::sf2_voice::{GraphNode, Modulator, Program};
    let begin = u16_at(bags, bag * 4 + 2) as usize;
    let end = u16_at(bags, (bag + 1) * 4 + 2) as usize;
    let mut raw = Vec::with_capacity(end - begin);
    for record in data[begin * 10..end * 10].as_chunks::<10>().0 {
        let m = Modulator {
            source: u16_at(record, 0),
            destination: u16_at(record, 2),
            amount: u16_at(record, 4) as i16 as f32,
            amount_source: u16_at(record, 6),
            transform: u16_at(record, 8),
            link: None,
        };
        let valid = super::sf2_voice::valid_source(m.source)
            && super::sf2_voice::valid_source(m.amount_source)
            && m.amount_source & 255 != 127
            && matches!(m.transform, 0 | 2)
            && (super::sf2_voice::value_generator(m.destination as usize)
                || m.destination as usize == super::sf2_voice::INITIAL_PITCH
                || (m.destination & 0x8000 != 0
                    && ((m.destination & 0x7fff) as usize) < end - begin));
        raw.push(valid.then_some(m));
    }
    let linked = raw.iter().flatten().any(|m| m.source & 255 == 127);
    let mut included = vec![true; raw.len()];
    let program = if linked {
        let bytes = std::mem::size_of::<Program>()
            + 2 * std::mem::size_of::<usize>()
            + raw.len() * (std::mem::size_of::<GraphNode>() + std::mem::size_of::<usize>());
        let mut reservation = budget
            .map(|b| b.reserve(bytes).map_err(SoundFontError::MemoryBudget))
            .transpose()?;
        let mut incoming = vec![Vec::new(); raw.len()];
        let mut outgoing = vec![Vec::new(); raw.len()];
        let mut degree = vec![0usize; raw.len()];
        for (index, m) in raw.iter().enumerate() {
            if let Some(m) = m
                && m.destination & 0x8000 != 0
            {
                let target = (m.destination & 0x7fff) as usize;
                if raw[target].is_some_and(|m| m.source & 255 == 127) {
                    incoming[target].push(index);
                    outgoing[index].push(target);
                    degree[target] += 1;
                }
            }
        }
        let mut ready = (0..raw.len())
            .filter(|i| degree[*i] == 0 && raw[*i].is_some())
            .collect::<Vec<_>>();
        let mut nodes = Vec::with_capacity(raw.len());
        included.fill(false);
        let mut next = 0;
        while next < ready.len() {
            let index = ready[next];
            next += 1;
            included[index] = true;
            nodes.push(GraphNode {
                original: index,
                modulator: raw[index].expect("validated modulator"),
                inputs: incoming[index].clone().into_boxed_slice(),
            });
            for target in &outgoing[index] {
                degree[*target] -= 1;
                if degree[*target] == 0 {
                    ready.push(*target);
                }
            }
        }
        // Cyclic nodes and their dependent routes are omitted; unrelated
        // acyclic controller destinations continue to work (§9.5.4).
        let mut program = Program {
            nodes: nodes.into_boxed_slice(),
            _reservation: None,
        };
        if let Some(lease) = &mut reservation {
            lease
                .shrink(program.bytes())
                .map_err(SoundFontError::MemoryBudget)?;
        }
        program._reservation = reservation;
        Some(std::sync::Arc::new(program))
    } else {
        None
    };
    let mut out = Vec::new();
    for (index, m) in raw.into_iter().enumerate() {
        let Some(mut m) = m else {
            continue;
        };
        if !included[index] || m.destination & 0x8000 != 0 {
            continue;
        }
        let graph = if m.source & 255 == 127 {
            m.link = Some((0, index));
            program.clone()
        } else {
            None
        };
        if out
            .iter()
            .any(|old: &OwnedModulator| old.modulator.identical(m))
        {
            return Err(SoundFontError::UnsupportedModulators);
        }
        out.push(OwnedModulator {
            modulator: m,
            program: graph,
        });
    }
    Ok(out)
}
