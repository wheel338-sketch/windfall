//! Snapshot-only packaging. Export never edits document paths or history.
use super::Session;
use windfall_plugin::{
    EmbeddedSample, EmbeddedSoundFont, MAX_RAW_BANK_BYTES, Program,
    package::{Platform, export_bundle, resolve_runtime},
};
impl Session {
    pub fn export_plugin_bundle(
        &self,
        destination: &str,
        runtime: &str,
        resources: &std::path::Path,
    ) -> Result<(), String> {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let binary = resolve_runtime(
            resources,
            &executable,
            (!runtime.trim().is_empty()).then(|| std::path::Path::new(runtime.trim())),
        )?;
        let (project, pool, pins, sample_dir) = {
            let state = self.state();
            (
                state.document.project().clone(),
                state.pool.clone(),
                state.document.effect_asset_pins(),
                state.sample_dir.clone(),
            )
        };
        let program = prepare_export_program(
            project,
            &pool,
            sample_dir.as_deref(),
            &self.inner.factory_dir,
        )?;
        let result = export_bundle(
            &program,
            &binary,
            std::path::Path::new(destination),
            Platform::native()?,
        );
        drop(pins);
        result
    }
}

/// Immutable snapshot, file IO and byte admission outside State/audio guards.
pub(super) fn prepare_export_program(
    mut project: windfall_project::Project,
    pool: &windfall_engine::SamplePool,
    sample_dir: Option<&std::path::Path>,
    factory_dir: &std::path::Path,
) -> Result<Program, String> {
    project.own_soundfont_sources()?;
    let mut samples = Vec::new();
    let mut soundfonts = Vec::new();
    let mut raw_bytes = 0usize;
    for asset in &project.samples {
        if asset.soundfont {
            let path =
                windfall_project::file::resolve_sample_path(&asset.path, sample_dir, factory_dir)
                    .ok_or_else(|| format!("Raw SF2 source '{}' cannot be resolved", asset.name))?;
            let size = std::fs::metadata(&path)
                .map_err(|e| format!("Raw SF2 source '{}': {e}", asset.name))?
                .len();
            let size = usize::try_from(size).map_err(|_| "Raw SF2 source size overflow")?;
            raw_bytes = raw_bytes
                .checked_add(size)
                .ok_or("Raw SF2 source size overflow")?;
            if raw_bytes > MAX_RAW_BANK_BYTES {
                return Err("Plugin raw SF2 sources exceed the aggregate 16 MiB bank limit".into());
            }
            let (bytes, _workspace) = super::sample_instruments::read_soundfont_with_limit(
                &path,
                &pool.sample_bank_budget_handle(),
                size as u64,
            )?;
            // The file may have changed between metadata and the bounded reader.
            if bytes.len() != size {
                return Err("Raw SF2 source changed while exporting; retry".into());
            }
            soundfonts.push(EmbeddedSoundFont {
                id: asset.id,
                bytes,
            });
        } else {
            let audio = pool.get(asset.id).ok_or_else(|| {
                format!(
                    "Sample '{}' is not decoded; plugin export requires every PCM source",
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
    }
    let program = Program::new(project, samples, soundfonts);
    program.validate()?;
    // Export refuses corrupt or missing preset data before publishing a bundle.
    program.validate_soundfont_sources()?;
    Ok(program)
}
