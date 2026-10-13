//! Session-owned cancellation and stale-result admission for preset jobs.
use super::{Session, State, library::Import};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::{Arc, atomic::{AtomicBool, Ordering}}};
use windfall_engine::preset_render::{self as engine, PresetRenderOptions};
use windfall_ipc::LibraryFileToken;
use windfall_project::{ChannelId, DispatchResult, SampleId};

#[derive(Default)]
pub(super) struct Jobs {
    next: u64,
    active: Option<(u64, Guard)>,
}
#[derive(Clone)]
pub(super) struct Guard {
    generation: u64,
    edits: u64,
    replacements: u64,
    rate: u32,
    cancelled: Arc<AtomicBool>,
    claimed: Arc<AtomicBool>,
}
impl Guard {
    pub(super) fn cancellation(&self) -> Arc<AtomicBool> { self.cancelled.clone() }
    pub(super) fn check(&self, state: &State) -> Result<(), String> {
        if self.cancelled.load(Ordering::Acquire) { return Err("Preset rendering cancelled".into()); }
        if (state.generation, state.edits, state.replacements) != (self.generation, self.edits, self.replacements) {
            return Err("The project changed while the preset was rendering. Try again.".into());
        }
        Ok(())
    }
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetRenderTicket {
    pub request: u64,
    pub sample_rate: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresetRenderRequest {
    pub path: String,
    #[serde(default)]
    pub source: Option<SampleId>,
    #[serde(default)]
    pub channel: Option<ChannelId>,
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub browser: Option<LibraryFileToken>,
    pub options: PresetRenderOptions,
}
struct Lease { session: Session, request: u64 }
impl Drop for Lease {
    fn drop(&mut self) {
        let mut jobs = crate::sync::lock(&self.session.inner.preset_jobs);
        if jobs.active.as_ref().is_some_and(|(id, _)| *id == self.request) { jobs.active = None; }
    }
}
impl Session {
    pub fn preset_render_begin(&self) -> Result<PresetRenderTicket, String> {
        let mut jobs = crate::sync::lock(&self.inner.preset_jobs);
        let _recording = self.recording_idle()?;
        let state = self.state();
        if let Some((_, guard)) = &jobs.active { guard.cancelled.store(true, Ordering::Release); }
        jobs.next = jobs.next.checked_add(1).ok_or("Preset render request counter exhausted")?;
        let request = jobs.next;
        let sample_rate = self.inner.audio.status().sample_rate;
        jobs.active = Some((request, Guard {
            generation: state.generation, edits: state.edits, replacements: state.replacements,
            rate: sample_rate,
            cancelled: Arc::new(AtomicBool::new(false)),
            claimed: Arc::new(AtomicBool::new(false)),
        }));
        Ok(PresetRenderTicket { request, sample_rate })
    }
    pub fn preset_render_cancel(&self, request: u64) {
        let mut jobs = crate::sync::lock(&self.inner.preset_jobs);
        if let Some((id, guard)) = &jobs.active && *id == request {
            guard.cancelled.store(true, Ordering::Release);
            jobs.active = None;
        }
    }
    pub fn preset_render_file(&self, request: u64, input: PresetRenderRequest) -> Result<DispatchResult, String> {
        let guard = crate::sync::lock(&self.inner.preset_jobs).active.as_ref()
            .filter(|(id, _)| *id == request).map(|(_, guard)| guard.clone())
            .ok_or("Preset render request is no longer active")?;
        if guard.claimed.swap(true, Ordering::AcqRel) { return Err("Preset render request is already running".into()); }
        let _lease = Lease { session: self.clone(), request };
        let file = if let Some(source) = input.source {
            let state = self.state();
            guard.check(&state)?;
            let asset = state.document.project().sample(source).ok_or("Retained preset source no longer exists")?;
            super::samples::locate(asset, state.project_dir(), &self.inner.factory_dir)?
        } else { crate::paths::absolute(&input.path)? };
        if !file.extension().is_some_and(|e| e.eq_ignore_ascii_case("wfpreset") || e.eq_ignore_ascii_case("ds")) {
            return Err(unsupported_source(&file).unwrap_or("Choose a Windfall .wfpreset or a public .ds preset.").into());
        }
        let token = input.browser.map(|token| self.inner.library.pin_file(&token, &crate::paths::display(&file))).transpose()?;
        let import = self.prepare_preset_import(&file, guard.generation, token, input.options, Some(guard))?;
        match input.channel {
            Some(channel) => self.replace_sample_import(channel, import),
            None => self.add_channel_import(import, input.index),
        }
    }
    pub(super) fn prepare_preset_import(&self, file: &Path, generation: u64,
        browser: Option<LibraryFileToken>, options: PresetRenderOptions, guard: Option<Guard>) -> Result<Import, String> {
        let guard = match guard {
            Some(guard) => guard,
            None => {
                let _recording = self.recording_idle()?;
                let state = self.state();
                if state.generation != generation { return Err(super::library::PROJECT_REPLACED.into()); }
                Guard { generation, edits: state.edits, replacements: state.replacements,
                    rate: self.inner.audio.status().sample_rate,
                    cancelled: Arc::new(AtomicBool::new(false)), claimed: Arc::new(AtomicBool::new(false)) }
            }
        };
        guard.check(&self.state())?;
        let rate = self.inner.audio.status().sample_rate;
        if rate != guard.rate { return Err("The project audio rate changed. Reopen the render dialog.".into()); }
        engine::check_options(options, rate)?;
        let original_version = crate::samples::FileKey::of(file)?;
        let bytes = super::preset_assets::read_original(file)?;
        let keep_going = || !guard.cancelled.load(Ordering::Acquire);
        let audio = if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("ds")) {
            // Public DrumSynth rendering also enters the common job/ownership path.
            if !keep_going() { return Err("Preset rendering cancelled".into()); }
            windfall_codec::DrumSynthPreset::parse(&bytes).and_then(|p| p.render_cancellable(rate,
                windfall_codec::MAX_DRUMSYNTH_FRAMES, windfall_codec::DRUMSYNTH_SOURCE_SEED, &mut || keep_going()))
                .map_err(|e| e.to_string())?.audio
        } else { engine::render(&bytes, options, rate, &mut || keep_going())? };
        #[cfg(test)] self.pause("preset-render:rendered");
        guard.check(&self.state())?;
        if crate::samples::FileKey::of(file)? != original_version || super::preset_assets::read_original(file)? != bytes { return Err("The preset source changed during rendering. Try again.".into()); }
        let mut owned = self.stage_preset(file, bytes, &audio, original_version)?;
        owned.settings = file.extension().is_some_and(|e| e.eq_ignore_ascii_case("wfpreset")).then_some(options);
        let buffer = self.inner.cache.decode(&owned.rendered)?;
        if let Some(token) = &browser { self.inner.library.check_file(token, &crate::paths::display(file))?; }
        Ok(Import { file: owned.rendered.clone(), buffer, browser, generation,
            owned: Some(owned), render_guard: Some(guard) })
    }
}

pub(crate) fn unsupported_source(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("syn") || extension.eq_ignore_ascii_case("speech") {
        Some("This proprietary preset source is unsupported: no public file specification is available. Use a Windfall synthesizer .wfpreset or save a Text Voice channel .wfpreset, then choose Render as sample.")
    } else { None }
}
