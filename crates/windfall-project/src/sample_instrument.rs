//! Saved sample-instrument recipes and references; never inline PCM.
use crate::{Channel, ChannelSource, InstrumentKind, Project, SampleId, SamplePath};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use windfall_dsp::sample_bank::{MAX_BANK_ZONES, SampleZone};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[ts(export)]
pub enum SampleInstrumentAsset {
    Zones {
        zones: Vec<SampleZone>,
    },
    SoundFont {
        path: SamplePath,
        bank: u16,
        program: u16,
        /// Project-owned file identity. `path` is a legacy fallback only.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        source: Option<SampleId>,
    },
    Piano {
        electric: bool,
        version: u32,
    },
    Speech {
        text: String,
        pitch: f32,
        duration: f32,
        version: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        source: Option<SampleId>,
    },
}
impl SampleInstrumentAsset {
    pub fn soundfont_source(&self) -> Option<SampleId> {
        match self {
            Self::SoundFont { source, .. } => *source,
            _ => None,
        }
    }
    pub fn soundfont_path<'a>(&'a self, project: &'a Project) -> Option<&'a SamplePath> {
        match self {
            Self::SoundFont { source, path, .. } => Some(
                source
                    .and_then(|id| project.sample(id))
                    .map_or(path, |s| &s.path),
            ),
            _ => None,
        }
    }
    pub fn canonical(&self, project: &Project) -> Self {
        let mut asset = self.clone();
        if let Self::SoundFont { path, .. } = &mut asset {
            *path = self
                .soundfont_path(project)
                .expect("SoundFont path")
                .clone();
        }
        asset
    }
    pub fn samples(&self) -> impl Iterator<Item = SampleId> + '_ {
        let speech = match self {
            Self::Speech { source, .. } => *source,
            _ => None,
        };
        let zones: &[SampleZone] = match self {
            Self::Zones { zones } => zones,
            _ => &[],
        };
        zones.iter().map(|zone| SampleId(zone.sample)).chain(speech)
    }
    pub fn validate(&self, project: &Project, kind: InstrumentKind) -> Result<(), String> {
        match self {
            Self::Zones { zones } => {
                if !matches!(
                    kind,
                    InstrumentKind::MultiSampler
                        | InstrumentKind::SamplePlayer
                        | InstrumentKind::PadBank
                ) {
                    return Err("this instrument does not accept a zone bank".into());
                }
                if zones.len() > MAX_BANK_ZONES {
                    return Err("a bank may have at most 256 zones".into());
                }
                for zone in zones {
                    zone.validate().map_err(str::to_owned)?;
                    if project
                        .sample(SampleId(zone.sample))
                        .is_none_or(|source| source.soundfont)
                    {
                        return Err("zone sample reference does not exist".into());
                    }
                    if kind == InstrumentKind::PadBank
                        && (zone.key_low != zone.key_high || !(36..=51).contains(&zone.key_low))
                    {
                        return Err("pad layers must target one of MIDI keys 36..51".into());
                    }
                }
            }
            Self::SoundFont {
                path,
                program,
                source,
                ..
            } => {
                if kind != InstrumentKind::SoundFont {
                    return Err("choose the SoundFont bank instrument".into());
                }
                if let Some(reason) = path.problem() {
                    return Err(reason.into());
                }
                if *program > 127 {
                    return Err("SoundFont program must be within 0..127".into());
                }
                if source.is_some_and(|id| project.sample(id).is_none_or(|s| !s.soundfont)) {
                    return Err("SoundFont source identity does not exist".into());
                }
            }
            Self::Piano { version, .. } => {
                if kind != InstrumentKind::PianoKeys || *version != 1 {
                    return Err("unsupported piano source recipe".into());
                }
            }
            Self::Speech {
                text,
                pitch,
                duration,
                version,
                source,
            } => {
                if kind != InstrumentKind::TextVoice || *version != 1 {
                    return Err("unsupported text voice recipe".into());
                }
                if source.is_some_and(|id| project.sample(id).is_none()) {
                    return Err("speech source reference does not exist".into());
                }
                if text.trim().is_empty()
                    || text.len() > 512
                    || !pitch.is_finite()
                    || !(24.0..=84.0).contains(pitch)
                    || !duration.is_finite()
                    || !(0.1..=60.0).contains(duration)
                {
                    return Err("invalid speech text, pitch or duration".into());
                }
                if text.chars().any(|c| {
                    !c.is_ascii_alphabetic() && !c.is_ascii_whitespace() && !".,!?'-".contains(c)
                }) {
                    return Err(
                        "the rule voice accepts English letters, spaces and punctuation".into(),
                    );
                }
            }
        }
        Ok(())
    }
}
impl Project {
    pub fn is_soundfont_source(&self, id: SampleId) -> bool {
        self.sample(id).is_some_and(|sample| sample.soundfont)
    }
    /// Legacy path-only banks acquire stable IDs on reading/exporting a private
    /// snapshot. Normal imports add the source and reference in one checked batch.
    pub fn own_soundfont_sources(&mut self) -> Result<(), String> {
        for index in 0..self.channels.len() {
            let Some(SampleInstrumentAsset::SoundFont {
                path, source: None, ..
            }) = &self.channels[index].sample_instrument
            else {
                continue;
            };
            let path = path.clone();
            let id = if let Some(sample) = self.samples.iter().find(|s| s.path == path) {
                sample.id
            } else {
                let id = SampleId(self.next_id);
                self.next_id = self
                    .next_id
                    .checked_add(1)
                    .ok_or("Project source ID overflow")?;
                self.samples.push(crate::SampleAsset {
                    soundfont: false,
                    id,
                    name: "SoundFont bank".into(),
                    path,
                    processing: None,
                    regions: Vec::new(),
                });
                id
            };
            self.samples
                .iter_mut()
                .find(|s| s.id == id)
                .expect("owned source")
                .soundfont = true;
            if let Some(SampleInstrumentAsset::SoundFont { source, .. }) =
                &mut self.channels[index].sample_instrument
            {
                *source = Some(id);
            }
        }
        Ok(())
    }
}
pub fn check(project: &Project, channel: &Channel) -> Result<(), String> {
    if let Some(asset) = &channel.sample_instrument {
        let ChannelSource::Instrument { params, .. } = channel.source else {
            return Err("a sampler channel cannot own a sample-instrument bank".into());
        };
        asset.validate(project, params.kind())?;
    }
    Ok(())
}
