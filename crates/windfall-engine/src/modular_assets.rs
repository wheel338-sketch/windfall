//! Rack assets use the standalone DSP asset/bank/convolver preparation paths.
//! This module is called exclusively while constructing an unpublished rack.
use std::sync::Arc;
use windfall_dsp::{AnyEffect, AnyInstrument, EffectParams, InstrumentParams};
use windfall_project::{RackAudioAsset, RackNodeAsset};

/// Admission covers unpublished, active and outgoing generations together.
#[derive(Debug)]
pub(crate) struct GenerationFamily {
    used: std::sync::atomic::AtomicUsize,
    payload_budget: Arc<windfall_dsp::sample_bank::BankBudget>,
    pub(super) bank_budget: Arc<windfall_dsp::sample_bank::BankBudget>,
}
impl GenerationFamily {
    pub(crate) fn new() -> Arc<Self> { Arc::new(Self { used: std::sync::atomic::AtomicUsize::new(0), payload_budget: Arc::new(windfall_dsp::sample_bank::BankBudget::default()), bank_budget: Arc::new(windfall_dsp::sample_bank::BankBudget::default()) }) }
    pub(super) fn reserve(self: &Arc<Self>, graph: &windfall_project::RackGraph, rate: u32) -> Result<GenerationPermit, String> {
        fn bytes(graph: &windfall_project::RackGraph, rate: u32) -> Result<usize, String> {
            let mut total = 0usize;
            for node in &graph.nodes {
                let payload = match &node.asset {
                    Some(RackNodeAsset::Impulse { impulse }) => {
                        let frames = (impulse.frames.len() as u64 * u64::from(rate)).div_ceil(u64::from(impulse.sample_rate.max(1)));
                        usize::try_from(frames).ok().and_then(|f| f.checked_mul(80)).ok_or("Rack impulse preparation size overflow")?
                    }
                    Some(asset) => asset.payload_bytes(),
                    _ => 0,
                };
                total = total.checked_add(payload).ok_or("Rack asset preparation size overflow")?;
                total = total.checked_add(node.hosted.as_ref().map_or(0, |p| p.state.len())).ok_or("Rack native state size overflow")?;
                if let windfall_project::RackProcessor::Rack { graph } = &node.processor {
                    total = total.checked_add(bytes(graph, rate)?).ok_or("Rack nested asset preparation size overflow")?;
                }
            }
            Ok(total)
        }
        let payload = self.payload_budget.reserve(bytes(graph, rate)?.checked_add(512 * 1024).ok_or("Rack fixed storage size overflow")?)?;
        self.used.fetch_update(std::sync::atomic::Ordering::AcqRel, std::sync::atomic::Ordering::Acquire, |used| (used < 64).then_some(used + 1)).map_err(|_| "Rack has 64 retained generations; wait for voices and tails to finish".to_owned())?;
        Ok(GenerationPermit { family: self.clone(), _payload: payload, runtime: Vec::new() })
    }
}
pub(crate) struct GenerationPermit { pub(crate) family: Arc<GenerationFamily>, _payload: windfall_dsp::sample_bank::BankReservation, runtime: Vec<windfall_dsp::sample_bank::BankReservation> }
impl GenerationPermit {
    pub(super) fn reserve_runtime(&mut self, bytes: usize) -> Result<(), String> { self.runtime.push(self.family.payload_budget.reserve(bytes)?); Ok(()) }
}
impl Drop for GenerationPermit {
    fn drop(&mut self) { self.family.used.fetch_sub(1, std::sync::atomic::Ordering::AcqRel); }
}

fn audio(source: &RackAudioAsset) -> Option<windfall_core::AudioBuffer> {
    source.frames.as_ref().map(|frames| windfall_core::AudioBuffer::from_interleaved(
        source.sample_rate, 2, frames.iter().flatten().copied().collect()))
}

pub(super) fn instrument(params: &InstrumentParams, asset: Option<&RackNodeAsset>, budget: &Arc<windfall_dsp::sample_bank::BankBudget>) -> Result<AnyInstrument, String> {
    let mut dsp = AnyInstrument::new_with_asset(params, match asset {
        Some(RackNodeAsset::Instrument { asset }) => Some(asset),
        _ => None,
    });
    let bank = match asset {
        Some(RackNodeAsset::Zones { zones }) => {
            if zones.iter().any(|zone| zone.audio.frames.is_none()) {
                Some(windfall_dsp::sample_bank::SampleBank::unavailable("Missing rack sample; relink in the module editor".into()))
            } else {
                let bytes = zones.iter().map(|z| z.audio.frames.as_ref().map_or(0, |f| f.len() * 8)).sum();
                let reservation = budget.reserve(bytes)?;
                let mut bank = windfall_dsp::sample_bank::SampleBank::new(zones.iter().map(|zone| windfall_dsp::sample_bank::LoadedZone {
                    mapping: zone.mapping,
                    audio: audio(&zone.audio).expect("validated rack sample"),
                }).collect()).map_err(str::to_owned)?;
                bank.retain_reservation(reservation);
                Some(bank)
            }
        }
        Some(RackNodeAsset::SoundFont { bytes: Some(bytes), bank, program, .. }) => Some(windfall_dsp::sample_bank::soundfont::parse_soundfont_preset_budgeted(bytes, *bank, *program, budget).map_err(|e| e.to_string())?),
        Some(RackNodeAsset::SoundFont { bytes: None, .. }) => Some(windfall_dsp::sample_bank::SampleBank::unavailable("Missing rack SoundFont; relink in the module editor".into())),
        None if params.kind() == windfall_dsp::InstrumentKind::PianoKeys => {
            // Match the standalone channel's version-one original piano recipe.
            let reservation = budget.reserve(30 * 24000 * 3 * 4)?;
            let mut bank = windfall_dsp::sample_bank::sources::piano_bank(false);
            bank.retain_reservation(reservation);
            Some(bank)
        }
        _ => None,
    };
    if let Some(bank) = bank {
        if !dsp.install_sample_bank(Arc::new(bank)) { return Err("Rack instrument refused its prepared sample bank".into()); }
    }
    Ok(dsp)
}

pub(super) fn effect(params: &mut EffectParams, asset: Option<&RackNodeAsset>, rate: u32) -> Result<AnyEffect, String> {
    match asset {
        Some(RackNodeAsset::Impulse { impulse }) => {
            let frames = impulse.at_rate(rate).map_err(str::to_owned)?;
            let prepared = windfall_dsp::spectral::PreparedImpulse::prepare_with_delay(&frames, impulse.group_delay_at(rate)).map_err(str::to_owned)?;
            Ok(AnyEffect::Convolver(Box::new(windfall_dsp::spectral::Convolver::with_impulse(Arc::new(prepared)))))
        }
        Some(RackNodeAsset::Audio { audio: source }) => {
            let EffectParams::Scratch(scratch) = params else { return Err("Rack audio source requires Scratch".into()); };
            // Nonzero denotes asset playback rather than the live recorder.
            scratch.source_id = u32::MAX;
            scratch.runtime_source = audio(source).as_ref().map(windfall_dsp::asset::publish_audio).transpose().map_err(|e| e.to_string())?.unwrap_or(windfall_dsp::asset::SourceRef::NONE);
            Ok(AnyEffect::new(params))
        }
        _ => Ok(AnyEffect::new(params)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rack_generation_admission_counts_pending_and_retired_owners_until_control_drop() {
        let graph = windfall_project::RackGraph { next_id: 1, ..Default::default() };
        let family = GenerationFamily::new();
        let mut owners: Vec<_> = (0..64).map(|_| family.reserve(&graph, 48000).unwrap()).collect();
        assert!(family.reserve(&graph, 48000).is_err());
        drop(owners.pop());
        let restored = family.reserve(&graph, 48000).unwrap();
        assert_eq!(family.used.load(std::sync::atomic::Ordering::Acquire), 64);
        drop(restored); drop(owners);
        assert_eq!(family.used.load(std::sync::atomic::Ordering::Acquire), 0);
        assert_eq!(family.payload_budget.used(), 0);
    }
}
