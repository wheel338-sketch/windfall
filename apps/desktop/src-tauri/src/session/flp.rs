//! Prepare an FL conversion and its samples without touching the open project.
//! Installation uses the same replacement ticket as ordinary new/open.
use super::{
    Session,
    files::Replacement,
    samples::{Decoded, decode_all},
};
use crate::{paths, sync::lock};
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use windfall_flp::{ConvertOptions, event::MAX_FILE_BYTES, import};
use windfall_ipc::{FlpImportOptions, FlpImportPreview, FlpMissingSample};
use windfall_project::{DocumentSnapshot, Project, SamplePath};

pub(super) struct PreparedFlp {
    ticket: Replacement,
    project: Project,
    decoded: Decoded,
}

impl Session {
    /// Reads, converts, resolves references and decodes samples for review.
    /// Missing audio is reported and leaves its references recoverable.
    pub fn flp_preview(
        &self,
        path: &str,
        options: &FlpImportOptions,
    ) -> Result<FlpImportPreview, String> {
        if options.sample_search_folders.len() > 32 {
            return Err("Choose at most 32 sample search folders.".into());
        }
        let path = paths::absolute(path)?;
        let ticket = self.begin_replacement();
        let mut bytes = Vec::new();
        File::open(&path)
            .map_err(|e| format!("Could not read the FL project: {e}"))?
            .take(MAX_FILE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| format!("Could not read the FL project: {e}"))?;
        let conversion = import(
            &bytes,
            &ConvertOptions {
                project_dir: path.parent().map(paths::display),
                factory_data_dir: options.factory_data_dir.clone(),
                user_data_dir: options.user_data_dir.clone(),
                fallback_name: path.file_stem().map(|p| p.to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;
        let mut project = conversion.project;
        let mut warnings = Vec::new();
        resolve_samples(&mut project, options, &mut warnings)?;
        project
            .check()
            .map_err(|e| format!("The converted project is invalid: {e}"))?;
        let decoded = decode_all(&self.inner.cache, &project, None, &self.inner.factory_dir);
        warnings.extend(decoded.warnings.clone());
        resolve_sampler_loops(
            &mut project,
            &decoded,
            &conversion.sampler_loops,
            &mut warnings,
        )?;
        let missing_samples = project
            .samples
            .iter()
            .filter(|s| decoded.failed.contains(&s.id))
            .map(|s| {
                let (SamplePath::External(path)
                | SamplePath::Factory(path)
                | SamplePath::Project(path)) = &s.path;
                FlpMissingSample {
                    sample: s.id,
                    name: s.name.clone(),
                    path: path.clone(),
                }
            })
            .collect();
        let preview = FlpImportPreview {
            token: ticket.request,
            name: project.settings.name.clone(),
            report: conversion.report,
            warnings,
            missing_samples,
            retained_plugins: project.retained_plugins.len() as u32,
        };
        // A slower older preview cannot overwrite the pending newer one.
        let mut pending = lock(&self.inner.flp_import);
        if self.state().replacements != ticket.request {
            return Err("This import was superseded by another project request.".into());
        }
        *pending = Some(PreparedFlp {
            ticket,
            project,
            decoded,
        });
        Ok(preview)
    }

    /// Opens the exact prepared project after the user reviews its report.
    pub fn flp_open(&self, token: u64) -> Result<DocumentSnapshot, String> {
        drop(self.recording_idle()?);
        let mut pending = lock(&self.inner.flp_import);
        if pending.as_ref().is_none_or(|p| p.ticket.request != token) {
            return Err("The import review expired. Choose the FL project again.".into());
        }
        let prepared = pending.take().ok_or("The import review expired.")?;
        drop(pending);
        self.install(
            &prepared.ticket,
            windfall_project::Document::new_unsaved(prepared.project),
            None,
            None,
            None,
            prepared.decoded,
        )
        .map_err(|refusal| refusal.message("The FL project was not imported"))
    }

    /// Cancels a review. It never changes the current document or playback.
    pub fn flp_cancel(&self, token: u64) {
        let mut pending = lock(&self.inner.flp_import);
        if pending.as_ref().is_some_and(|p| p.ticket.request == token) {
            *pending = None;
        }
    }
}

/// Folder searches are bounded, sorted and do not follow directory symlinks.
/// A basename match is used only when unique; ambiguities remain missing.
fn resolve_samples(
    project: &mut Project,
    options: &FlpImportOptions,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    if options.sample_search_folders.is_empty() {
        return Ok(());
    }
    let mut files = Vec::new();
    let mut pending: Vec<(PathBuf, usize)> = options
        .sample_search_folders
        .iter()
        .map(|p| paths::absolute(p).map(|p| (p, 0)))
        .collect::<Result<_, _>>()?;
    let mut visited = 0;
    while let Some((folder, depth)) = pending.pop() {
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(e) => {
                warnings.push(format!("Could not search {}: {e}", paths::display(&folder)));
                continue;
            }
        };
        let mut entries: Vec<_> = entries
            .take(20_001usize.saturating_sub(visited))
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            visited += 1;
            if visited > 20_000 {
                warnings.push("Sample search stopped after 20,000 entries. Choose a more specific folder for remaining samples.".into());
                pending.clear();
                break;
            }
            let kind = match entry.file_type() {
                Ok(kind) => kind,
                Err(_) => continue,
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() && depth < 8 {
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file() {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    files.dedup();
    for sample in &mut project.samples {
        let SamplePath::External(written) = &sample.path else {
            continue;
        };
        if Path::new(written).is_file() {
            continue;
        }
        let basename = written.rsplit(['/', '\\']).next().unwrap_or(written);
        let mut matches = files.iter().filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(basename))
        });
        if let Some(found) = matches.next() {
            if matches.next().is_none() {
                sample.path = SamplePath::External(paths::display(found));
            } else {
                warnings.push(format!(
                    "More than one file matches {basename}; choose a more specific search folder."
                ));
            }
        }
    }
    Ok(())
}

/// Metadata work happens before the prepared snapshot is published, off locks.
fn resolve_sampler_loops(
    project: &mut Project,
    decoded: &Decoded,
    requests: &[(windfall_project::ChannelId, windfall_project::SampleId)],
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    use windfall_codec::{WavLoopKind, read_wav_metadata_file};
    use windfall_project::{Command, Document, SamplerLoopMode, SamplerPatch};
    let mut document = Document::new(project.clone());
    for &(channel, sample) in requests {
        let Some(asset) = project.samples.iter().find(|asset| asset.id == sample) else {
            continue;
        };
        let Some(buffer) = decoded.pool.get(sample) else {
            continue;
        };
        let SamplePath::External(path) = &asset.path else {
            continue;
        };
        if !Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav") || ext.eq_ignore_ascii_case("wave"))
        {
            warnings.push(format!(
                "Sample {} requests loop points, but only RIFF/WAVE loop metadata is supported.",
                asset.name
            ));
            continue;
        }
        let metadata = match read_wav_metadata_file(path) {
            Ok(metadata) => metadata,
            Err(error) => {
                warnings.push(format!(
                    "Sample {} loop metadata was not applied: {error}",
                    asset.name
                ));
                continue;
            }
        };
        let Some(points) = metadata.loops.first() else {
            warnings.push(format!(
                "Sample {} requests loop points but its WAV has none.",
                asset.name
            ));
            continue;
        };
        if buffer.frames() == 0
            || points.end as usize > buffer.frames()
            || points.kind == WavLoopKind::Backward
        {
            warnings.push(format!(
                "Sample {} loop bounds or backward mode cannot be applied.",
                asset.name
            ));
            continue;
        }
        let mode = match points.kind {
            WavLoopKind::Forward => SamplerLoopMode::Forward,
            WavLoopKind::Alternating => SamplerLoopMode::PingPong,
            WavLoopKind::Backward => unreachable!("refused above"),
        };
        let Some(source) = project.channel(channel) else {
            continue;
        };
        let windfall_project::ChannelSource::Sampler(settings) = &source.source else {
            continue;
        };
        let total = buffer.frames() as f64;
        let trim_start = (f64::from(settings.start) * total).round();
        let trim_end = (f64::from(settings.end) * total).round();
        if f64::from(points.start) < trim_start
            || f64::from(points.end) > trim_end
            || trim_end <= trim_start
        {
            warnings.push(format!(
                "Sample {} loop lies outside its trimmed playback region.",
                asset.name
            ));
            continue;
        }
        let frames = trim_end - trim_start;
        let patch = SamplerPatch {
            loop_mode: Some(mode),
            loop_start: Some(((f64::from(points.start) - trim_start) / frames) as f32),
            loop_end: Some(((f64::from(points.end) - trim_start) / frames) as f32),
            ..Default::default()
        };
        match document.dispatch(Command::UpdateSampler { id: channel, patch }, None) {
            Ok(_) => warnings.push(format!("Sample {} loop points came from its WAV smpl chunk; loop release/seam behavior can differ.", asset.name)),
            Err(error) => warnings.push(format!("Sample {} loop metadata was not applied: {error}", asset.name)),
        }
        if metadata.loops.len() > 1 || points.play_count != 0 {
            warnings.push(format!(
                "Sample {}: only the first loop and indefinite repetition are represented.",
                asset.name
            ));
        }
    }
    *project = document.project().clone();
    project.check().map_err(|error| error.to_string())
}
