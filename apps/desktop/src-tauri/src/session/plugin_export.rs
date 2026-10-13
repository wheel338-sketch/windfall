//! Snapshot-only packaging. Export never edits document paths or history.
use super::Session;
use windfall_plugin::{
    EmbeddedSample, Program,
    package::{Platform, export_bundle, resolve_runtime},
};
impl Session {
    pub fn export_plugin_bundle(&self, destination: &str, runtime: &str, resources: &std::path::Path) -> Result<(), String> {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let binary = resolve_runtime(resources, &executable,
            (!runtime.trim().is_empty()).then(|| std::path::Path::new(runtime.trim())))?;
        let (project, pool, pins) = {
            let state = self.state();
            (
                state.document.project().clone(),
                state.pool.clone(),
                state.document.effect_asset_pins(),
            )
        };
        let mut samples = Vec::with_capacity(project.samples.len());
        for asset in &project.samples {
            let audio = pool.get(asset.id).ok_or_else(|| {
                format!(
                    "Sample '{}' is not decoded; plugin export requires every source",
                    asset.name
                )
            })?;
            samples.push(EmbeddedSample {
                id: asset.id,
                sample_rate: audio.sample_rate(),
                channels: audio.channels(),
                data: audio.samples().to_vec(),
            });
        }
        let result = export_bundle(
            &Program { project, samples },
            &binary,
            std::path::Path::new(destination),
            Platform::native()?,
        );
        drop(pins);
        result
    }
}
