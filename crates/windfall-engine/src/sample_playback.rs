use std::sync::Arc;
use windfall_dsp::granular::PlaybackAsset;
use windfall_project::SamplePlayback;
use crate::SamplePool;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Identity {
    settings: SamplePlayback,
    sources: [Option<(usize, usize, u32)>; 2],
}
#[derive(Debug)]
pub(crate) struct Prepared {
    pub asset: Arc<PlaybackAsset>,
    pub identity: Identity,
}
pub(crate) fn prepare(settings: &SamplePlayback, pool: &SamplePool) -> Result<Arc<Prepared>, &'static str> {
    let sources = [settings.sample.and_then(|id| pool.get(id)).cloned().or_else(|| settings.inline_source.as_ref().map(|s| windfall_core::AudioBuffer::from_interleaved(s.sample_rate, 1, s.data.clone()))),
        settings.sample_b.and_then(|id| pool.get(id)).cloned()];
    let identity = Identity { settings: settings.clone(), sources: std::array::from_fn(|i| {
        sources[i].as_ref().filter(|_| i != 0 || settings.sample.is_some()).map(|a| (a.samples().as_ptr() as usize, a.frames(), a.sample_rate()))
    }) };
    let asset = PlaybackAsset::new(sources, settings.slices.clone(), settings.curve.clone(),
        settings.key_curves.iter().map(|item| (item.key, item.curve.clone())).collect())?;
    Ok(Arc::new(Prepared { asset: Arc::new(asset), identity }))
}
