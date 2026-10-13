//! Worker-only bank preparation. Compilation reads ready immutable handles.
use crate::SamplePool;
use std::sync::Arc;
use windfall_core::{AudioBuffer, AudioIdentity};
use windfall_dsp::sample_bank::{LoadedZone, SampleBank, SampleZone, soundfont, sources};
use windfall_project::{
    ChannelId, ChannelSource, InstrumentKind, Project, SampleId, SampleInstrumentAsset,
};

#[derive(Debug, Clone)]
pub(crate) struct PreparedSampleBank {
    pub channel: ChannelId,
    pub asset: SampleInstrumentAsset,
    pub identities: Vec<Option<AudioIdentity>>,
    pub bank: Arc<SampleBank>,
}

pub(crate) fn asset(channel: &windfall_project::Channel) -> Option<SampleInstrumentAsset> {
    channel.sample_instrument.clone().or_else(|| {
        let ChannelSource::Instrument { params, .. } = channel.source else {
            return None;
        };
        (params.kind() == InstrumentKind::PianoKeys).then_some(SampleInstrumentAsset::Piano {
            electric: false,
            version: 1,
        })
    })
}

impl SamplePool {
    /// Load SF2 bytes off audio. Pure parser validates before replacing a handle.
    /// The caller resolves/reads the reference and guards document admission.
    pub fn insert_soundfont(
        &mut self,
        asset: SampleInstrumentAsset,
        bytes: &[u8],
    ) -> Result<(), String> {
        let SampleInstrumentAsset::SoundFont { bank, program, .. } = &asset else {
            return Err("not a SoundFont reference".into());
        };
        let bank = Arc::new(
            soundfont::parse_soundfont_preset_budgeted(
                bytes,
                *bank,
                *program,
                &self.sample_bank_budget,
            )
            .map_err(|e| e.to_string())?,
        );
        Arc::make_mut(&mut self.sample_banks).retain(|held| held.asset != asset);
        Arc::make_mut(&mut self.sample_banks).push(PreparedSampleBank {
            channel: ChannelId(0),
            asset,
            identities: Vec::new(),
            bank,
        });
        Ok(())
    }

    /// Worker snapshot preparation, with no document/audio lock held. Private
    /// candidate storage ensures cancelled/stale results do not mutate caches.
    pub fn prepare_sample_instruments(
        &self,
        project: &Project,
        keep_going: &mut dyn FnMut() -> bool,
    ) -> Result<Self, String> {
        let mut ready: Vec<PreparedSampleBank> = Vec::new();
        for channel in &project.channels {
            let Some(asset) = asset(channel).map(|a|a.canonical(project)) else {
                continue;
            };
            if !keep_going() {
                return Err("sample instrument preparation cancelled".into());
            }
            let identities: Vec<_> = asset
                .samples()
                .map(|id| self.get(id).map(AudioBuffer::identity))
                .collect();
            let cached = ready
                .iter()
                .chain(self.sample_banks.iter())
                .find(|held| held.asset == asset && held.identities == identities);
            let bank = if let Some(cached) = cached {
                cached.bank.clone()
            } else if identities.iter().any(Option::is_none) {
                Arc::new(SampleBank::unavailable(
                    "Missing sample-bank sources; relink them in the instrument editor".into(),
                ))
            } else {
                // Reserve before source generation/cloning, including concurrent snapshots.
                let bytes = match &asset {
                    SampleInstrumentAsset::Zones { zones } => {
                        zones.iter().try_fold(0usize, |sum, zone| {
                            let audio = self
                                .get(SampleId(zone.sample))
                                .ok_or_else(|| format!("Missing zone sample {}", zone.sample))?;
                            sum.checked_add(audio.retained_pcm_bytes())
                                .ok_or_else(|| "sample bank byte overflow".to_owned())
                        })?
                    }
                    SampleInstrumentAsset::Piano { .. } => 30 * 24000 * 3 * 4,
                    SampleInstrumentAsset::Speech {
                        duration, source, ..
                    } => {
                        if let Some(id) = source {
                            self.get(*id)
                                .ok_or("saved speech source is unavailable")?
                                .retained_pcm_bytes()
                        } else {
                            (duration * 24000.0).round() as usize * 4
                        }
                    }
                    _ => 0,
                };
                if bytes > windfall_dsp::sample_bank::MAX_BANK_BYTES {
                    return Err("sample bank exceeds 256 MiB".into());
                }
                let reservation = self.sample_bank_budget.reserve(bytes)?;
                let mut bank = match &asset {
                    SampleInstrumentAsset::Zones { zones } => {
                        let loaded: Result<Vec<_>, String> = zones
                            .iter()
                            .map(|mapping| {
                                let audio = self
                                    .get(SampleId(mapping.sample))
                                    .ok_or_else(|| {
                                        format!("Missing zone sample {}", mapping.sample)
                                    })?
                                    .clone();
                                Ok(LoadedZone {
                                    mapping: *mapping,
                                    audio,
                                })
                            })
                            .collect();
                        SampleBank::new(loaded?).map_err(str::to_owned)?
                    }
                    SampleInstrumentAsset::Piano {
                        electric,
                        version: 1,
                    } => sources::piano_bank(*electric),
                    SampleInstrumentAsset::Speech {
                        text,
                        pitch,
                        duration,
                        version: 1,
                        source,
                    } => {
                        let audio = if let Some(id) = source {
                            self.get(*id)
                                .ok_or("saved speech source is unavailable")?
                                .clone()
                        } else {
                            sources::render_text(text, *pitch, *duration, keep_going)
                                .map_err(|e| e.to_string())?
                        };
                        SampleBank::new(vec![LoadedZone {
                            mapping: SampleZone {
                                root_key: 60,
                                ..Default::default()
                            },
                            audio,
                        }])
                        .map_err(str::to_owned)?
                    }
                    SampleInstrumentAsset::SoundFont { .. } => {
                        return Err("SoundFont source is unavailable; load its bank file".into());
                    }
                    _ => return Err("unsupported sample source recipe version".into()),
                };
                bank.retain_reservation(reservation);
                Arc::new(bank)
            };
            ready.push(PreparedSampleBank {
                channel: channel.id,
                asset,
                identities,
                bank,
            });
        }
        let mut pool = self.clone();
        pool.sample_banks = Arc::new(ready);
        Ok(pool)
    }

    pub(crate) fn sample_instrument_bank(&self, channel: ChannelId) -> Option<Arc<SampleBank>> {
        self.sample_banks
            .iter()
            .find(|held| held.channel == channel)
            .map(|held| held.bank.clone())
    }
    pub fn insert_unavailable_soundfont(&mut self, asset: SampleInstrumentAsset, reason: String) {
        Arc::make_mut(&mut self.sample_banks).retain(|held| held.asset != asset);
        Arc::make_mut(&mut self.sample_banks).push(PreparedSampleBank {
            channel: ChannelId(0),
            asset,
            identities: Vec::new(),
            bank: Arc::new(SampleBank::unavailable(reason)),
        });
    }
    pub fn has_sample_instrument_asset(&self, asset: &SampleInstrumentAsset) -> bool {
        self.sample_banks
            .iter()
            .any(|held| &held.asset == asset && held.bank.unavailable_reason().is_none())
    }
}
