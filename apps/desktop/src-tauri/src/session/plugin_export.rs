//! Snapshot-only packaging. Export never edits document paths or history.
use super::Session;
use windfall_plugin::{EmbeddedSample, Program, package::{Platform, export_bundle}};
impl Session {
    pub fn export_plugin_bundle(&self, destination: &str, runtime: &str) -> Result<(), String> {
        let (project, pool, pins) = {
            let state = self.state();
            (state.document.project().clone(), state.pool.clone(), state.document.effect_asset_pins())
        };
        let mut samples = Vec::with_capacity(project.samples.len());
        for asset in &project.samples {
            let audio = pool.get(asset.id).ok_or_else(|| format!("Sample '{}' is not decoded; plugin export requires every source", asset.name))?;
            samples.push(EmbeddedSample { id: asset.id, sample_rate: audio.sample_rate(), channels: audio.channels(), data: audio.samples().to_vec() });
        }
        let result = export_bundle(&Program { project, samples }, std::path::Path::new(runtime), std::path::Path::new(destination), Platform::native()?);
        drop(pins);
        result
    }
}
