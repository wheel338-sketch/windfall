//! The MCP server's way into the session. An agent reads snapshots of the
//! document and applies nothing but approved [`CommandProposal`]s through
//! [`Session::proposal_apply`], the same path the assistant's review takes:
//! one undo step each, refused while recording, refused once stale. Its
//! renders run offline on the agent's request thread, as the playlist
//! bounce's do, and are kept in a folder of Windfall's own.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use windfall_core::AudioBuffer;
use windfall_engine::{RenderOptions, render_streaming_checked};
use windfall_ipc::{
    McpAgent, McpCount, McpFocus, McpLoad, McpPanel, McpProposal, McpSettings, McpSettingsUpdate,
    PlayMode,
};
use windfall_mcp::{
    Approver, ChannelPresetInfo, Focus, Host, LibraryFile, LibraryPage, NativeEdit, RenderRegion,
    RenderRequest, RenderedAudio, Snapshot, SoundFontPreset, Status, Transport,
};
use windfall_project::proposal::{CommandProposal, ProposalStamp};
use windfall_project::{
    AutomationRange, AutomationTarget, ChannelId, ClipContent, Command, PPQ, Project, TickRange,
};

use super::{Session, WeakSession};
use crate::events::Event;
use crate::mcp::config;
use crate::sync::lock;

mod plugins;

/// Renders kept at most; the oldest go first.
const KEPT_RENDERS: usize = 20;
const RENDER_RATE: u32 = 48_000;

/// What the server holds of the session: not the session itself, so the
/// listener never keeps the app alive.
pub(crate) struct SessionHost(WeakSession);

impl SessionHost {
    fn session(&self) -> Result<Session, String> {
        self.0
            .upgrade()
            .ok_or_else(|| "Windfall is closing.".to_owned())
    }
}

impl Host for SessionHost {
    fn plugins(&self) -> Result<Vec<windfall_mcp::PluginInfo>, String> {
        self.session()?.mcp_plugins()
    }
    fn plugin_diagnostics(
        &self,
        target: Option<windfall_project::PluginTarget>,
    ) -> Result<Value, String> {
        self.session()?.mcp_plugin_diagnostics(target)
    }
    fn plugin_editor(
        &self,
        target: windfall_project::PluginTarget,
        open: bool,
    ) -> Result<Value, String> {
        let session = self.session()?;
        match session.change_plugin_editor(target, open, None) {
            Ok(()) => Ok(
                serde_json::json!({"ok": true, "openRequested": open, "report": session.mcp_plugin_diagnostics(Some(target))?}),
            ),
            Err(error) => {
                Err(session.mcp_plugin_error_report("editor", Some(target), None, &error))
            }
        }
    }
    fn snapshot(&self) -> Result<Snapshot, String> {
        let session = self.session()?;
        let recording = session.recording_idle().is_err();
        let transport = session.transport_state();
        let state = session.state();
        Ok(Snapshot {
            document: state.document.clone(),
            stamp: ProposalStamp {
                generation: state.generation,
                revision: state.document.revision(),
            },
            recording,
            transport: Transport {
                playing: transport.playing,
                mode: transport.mode,
                pattern: transport.pattern,
            },
        })
    }

    fn focus(&self) -> Focus {
        self.0
            .upgrade()
            .map(|session| lock(&session.inner.mcp.focus).clone())
            .unwrap_or_default()
    }

    fn apply(&self, proposal: CommandProposal) -> Result<u64, String> {
        self.session()?
            .proposal_apply(proposal)
            .map(|result| result.patch.revision)
    }

    fn render(
        &self,
        request: RenderRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RenderedAudio, String> {
        self.session()?.mcp_render(request, cancelled)
    }

    fn apply_native(&self, proposal: CommandProposal, edit: NativeEdit) -> Result<u64, String> {
        self.session()?
            .mcp_native_apply(proposal, edit)
            .map(|result| result.patch.revision)
    }

    fn library_file(&self, path: &str) -> Result<String, String> {
        self.session()?
            .mcp_library_file(path)
            .map(|file| crate::paths::display(&file))
    }

    fn soundfont_presets(&self, path: &str) -> Result<Vec<SoundFontPreset>, String> {
        let session = self.session()?;
        let file = session.mcp_library_file(path)?;
        session.soundfont_presets(&crate::paths::display(&file))
    }

    fn search_library(
        &self,
        query: &str,
        kind: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<LibraryPage, String> {
        self.session()?
            .mcp_search_library(query, kind, offset, limit)
    }

    fn channel_presets(&self) -> Result<Vec<ChannelPresetInfo>, String> {
        self.session()?.mcp_channel_presets()
    }

    fn proposals_changed(&self) {
        if let Some(session) = self.0.upgrade() {
            session.mcp_announce();
        }
    }

    fn random_hex(&self) -> String {
        let mut bytes = [0u8; 16];
        if getrandom::fill(&mut bytes).is_err() {
            // Unpredictability only matters against other local programs,
            // which also need the token; the time still keeps ids apart.
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            bytes = now.to_le_bytes();
        }
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

/// The slowest the project's tempo gets, for bounding a render's length.
fn slowest_tempo(project: &Project) -> f64 {
    project
        .automations
        .iter()
        .filter(|automation| automation.target == AutomationTarget::Tempo)
        .flat_map(|automation| &automation.points)
        .map(|point| f64::from(AutomationRange::TEMPO.value(point.value)))
        .fold(project.settings.tempo_bpm, f64::min)
        .max(windfall_project::MIN_TEMPO_BPM)
}

/// What an agent calls a file of the browser: "audio", "soundFont",
/// "project", "preset" or "other".
pub(crate) fn agent_file_kind(path: &std::path::Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("");
    if extension.eq_ignore_ascii_case("sf2") {
        "soundFont"
    } else if extension.eq_ignore_ascii_case("wfpreset") {
        "preset"
    } else if extension.eq_ignore_ascii_case(windfall_project::file::FILE_EXTENSION) {
        "project"
    } else if windfall_codec::is_audio_extension(extension) {
        "audio"
    } else {
        "other"
    }
}

/// The channels that play notes somewhere in a region, and are not muted:
/// those a render of it should hear.
fn channels_heard(
    project: &Project,
    region: RenderRegion,
    solo: Option<windfall_project::TrackId>,
) -> Vec<ChannelId> {
    let mut heard = std::collections::BTreeSet::new();
    let mut lanes = |pattern: &windfall_project::Pattern,
                     window: Option<(u32, u32, u32, (u32, u32))>| {
        let length = pattern.length_ticks().max(1);
        for lane in &pattern.lanes {
            let plays = lane.notes.iter().any(|note| match window {
                None => true,
                // A clip at `start`, `len` long, `offset` into its pattern,
                // within the region `from..to`; patterns repeat in a clip.
                Some((start, len, offset, (from, to))) => {
                    let end = start.saturating_add(len);
                    let mut at = i64::from(start) - i64::from(offset) + i64::from(note.start);
                    let mut passes = 0;
                    while at < i64::from(end) && passes < 4096 {
                        if at >= i64::from(start)
                            && at < i64::from(to)
                            && at + i64::from(note.length.max(1)) > i64::from(from)
                        {
                            return true;
                        }
                        at += i64::from(length);
                        passes += 1;
                    }
                    false
                }
            });
            if plays {
                heard.insert(lane.channel);
            }
        }
    };
    match region {
        RenderRegion::Pattern { pattern, .. } => {
            if let Some(pattern) = project.pattern(pattern) {
                lanes(pattern, None);
            }
        }
        RenderRegion::Song | RenderRegion::Range { .. } => {
            let (from, to) = match region {
                RenderRegion::Range { start, end } => (start, end),
                _ => (0, u32::MAX),
            };
            for clip in &project.playlist.clips {
                let ClipContent::Pattern { pattern } = clip.content else {
                    continue;
                };
                let track_muted = project
                    .playlist
                    .tracks
                    .iter()
                    .any(|track| track.id == clip.track && track.muted);
                if clip.muted || track_muted {
                    continue;
                }
                if let Some(pattern) = project.pattern(pattern) {
                    lanes(
                        pattern,
                        Some((clip.start, clip.length, clip.offset, (from, to))),
                    );
                }
            }
        }
    }
    project
        .channels
        .iter()
        .filter(|channel| heard.contains(&channel.id) && !channel.muted)
        .filter(|channel| solo.is_none_or(|track| channel.mixer_track == track))
        .map(|channel| channel.id)
        .collect()
}

/// Channels checked for silence in one render at most.
const MAX_SILENCE_CHECKS: usize = 64;

fn count_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

impl Session {
    fn mcp_host(&self) -> Arc<dyn Host> {
        Arc::new(SessionHost(self.downgrade()))
    }

    /// The host an agent's requests run against, for tests of the session.
    #[cfg(test)]
    pub(crate) fn mcp_test_host(&self) -> SessionHost {
        SessionHost(self.downgrade())
    }

    pub fn mcp_state(&self) -> McpSettings {
        let file = self.store().mcp_file();
        let service = &self.inner.mcp;
        let (config, error) = match config::load_with_token(&file) {
            Ok(config) => (config, lock(&service.error).clone()),
            Err(error) => (config::Config::default(), Some(error)),
        };
        McpSettings {
            enabled: config.enabled,
            running: service.listening().is_some(),
            port: config.port,
            url: format!("http://127.0.0.1:{}/mcp", config.port),
            token: config.token,
            executable: std::env::current_exe()
                .map(|path| crate::paths::display(&path))
                .unwrap_or_default(),
            error,
        }
    }

    /// Turns the server on or off, or moves it to another port. Turning it
    /// off ends every session and allowance and withdraws what waits.
    pub fn mcp_configure(&self, update: McpSettingsUpdate) -> Result<McpSettings, String> {
        if update.port < config::LOWEST_PORT {
            return Err("Choose a port from 1024 to 65535.".into());
        }
        let service = &self.inner.mcp;
        {
            let _configuring = lock(&service.configuring);
            let file = self.store().mcp_file();
            let mut config = config::load_with_token(&file)?;
            if update.enabled {
                if service.listening() != Some(update.port) {
                    let weak = self.downgrade();
                    service.listen(
                        self.mcp_host(),
                        Box::new(move || weak.upgrade().is_some()),
                        update.port,
                        config.token.clone(),
                    )?;
                }
            } else {
                service.stop(true);
                *lock(&service.error) = None;
            }
            config.enabled = update.enabled;
            config.port = update.port;
            config::save(&file, &config)?;
        }
        self.mcp_announce();
        Ok(self.mcp_state())
    }

    /// Makes a new token. Clients must be given it again; what the old one
    /// opened is closed.
    pub fn mcp_regenerate_token(&self) -> Result<McpSettings, String> {
        let service = &self.inner.mcp;
        {
            let _configuring = lock(&service.configuring);
            let file = self.store().mcp_file();
            let mut config = config::load_with_token(&file)?;
            config.token = config::new_token()?;
            config::save(&file, &config)?;
            if service.listening().is_some() {
                service.stop(true);
                let weak = self.downgrade();
                service.listen(
                    self.mcp_host(),
                    Box::new(move || weak.upgrade().is_some()),
                    config.port,
                    config.token,
                )?;
            }
        }
        self.mcp_announce();
        Ok(self.mcp_state())
    }

    /// Starts the server when the user left it on. Called once at start-up.
    pub(crate) fn mcp_restore(&self) {
        let file = self.store().mcp_file();
        let Ok(config) = config::load(&file) else {
            return;
        };
        if !config.enabled || config.token.is_empty() {
            return;
        }
        let service = &self.inner.mcp;
        let _configuring = lock(&service.configuring);
        let weak = self.downgrade();
        if let Err(error) = service.listen(
            self.mcp_host(),
            Box::new(move || weak.upgrade().is_some()),
            config.port,
            config.token,
        ) {
            log::warn!("MCP: {error}");
        }
    }

    pub fn mcp_panel(&self) -> McpPanel {
        let approvals = self.inner.mcp.server.approvals();
        let stamp = self.proposal_stamp();
        let proposals = approvals
            .entries()
            .into_iter()
            .map(|entry| McpProposal {
                stale: entry.status == Status::Pending && entry.proposal.stamp != stamp,
                status: entry.status.name().to_owned(),
                error: match &entry.status {
                    Status::Failed { error } => Some(error.clone()),
                    _ => None,
                },
                counts: entry
                    .preview
                    .counts
                    .iter()
                    .map(|(kind, [before, after])| McpCount {
                        kind: kind.clone(),
                        before: count_text(before),
                        after: count_text(after),
                    })
                    .collect(),
                id: entry.id,
                agent: entry.agent,
                label: entry.proposal.label,
                explanation: entry.proposal.explanation,
                automatic: entry.automatic,
                commands: entry.proposal.commands,
                load: entry.native.and_then(|native| {
                    let named = |channel: ChannelId| {
                        self.state()
                            .document
                            .project()
                            .channel(channel)
                            .map(|channel| channel.name.clone())
                            .unwrap_or_default()
                    };
                    Some(match native {
                        NativeEdit::Plugin { .. } => return None,
                        NativeEdit::SoundFont {
                            channel,
                            path,
                            bank,
                            program,
                            preset,
                            replace_instrument,
                        } => McpLoad::SoundFont {
                            channel_name: named(channel),
                            channel,
                            file: path,
                            preset,
                            bank,
                            program,
                            replace_instrument,
                        },
                        NativeEdit::ChannelPreset {
                            channel,
                            path,
                            name,
                        } => McpLoad::ChannelPreset {
                            channel_name: named(channel),
                            channel,
                            file: path,
                            preset: name,
                        },
                    })
                }),
            })
            .collect();
        let mut names = approvals.agents();
        for trusted in approvals.trusted_agents() {
            if !names.contains(&trusted) {
                names.push(trusted);
            }
        }
        McpPanel {
            proposals,
            agents: names
                .into_iter()
                .map(|name| McpAgent {
                    trusted: approvals.trusted(&name),
                    name,
                })
                .collect(),
            rendering: lock(&self.inner.mcp.render).is_some(),
        }
    }

    /// Tells the window what the panel shows now.
    fn mcp_announce(&self) {
        let _order = lock(&self.inner.mcp.emitting);
        let panel = self.mcp_panel();
        self.emit(Event::McpPanel(panel));
    }

    /// The project changed, so a waiting proposal may have gone stale, which
    /// the panel must show. Changes are published with the state locked,
    /// and the panel reads the state, so it is sent from another thread once
    /// the lock is free; a burst of edits sends it once.
    pub(super) fn mcp_project_changed(&self) {
        let service = &self.inner.mcp;
        if !service.server.approvals().waiting() || service.refreshing.swap(true, Ordering::AcqRel)
        {
            return;
        }
        let weak = self.downgrade();
        let spawned = std::thread::Builder::new()
            .name("windfall-mcp-panel".into())
            .spawn(move || {
                if let Some(session) = weak.upgrade() {
                    session.inner.mcp.refreshing.store(false, Ordering::Release);
                    session.mcp_announce();
                }
            });
        if spawned.is_err() {
            service.refreshing.store(false, Ordering::Release);
        }
    }

    /// The user applies a proposal: one undo step, through the checked
    /// proposal path.
    pub fn mcp_approve(&self, id: &str) -> Result<McpPanel, String> {
        let host = SessionHost(self.downgrade());
        let status = self
            .inner
            .mcp
            .server
            .approvals()
            .apply(&host, id, Approver::User)?;
        self.mcp_announce();
        match status {
            Status::Failed { error } => Err(error),
            _ => Ok(self.mcp_panel()),
        }
    }

    pub fn mcp_reject(&self, id: &str) -> Result<McpPanel, String> {
        self.inner.mcp.server.approvals().reject(id)?;
        self.mcp_announce();
        Ok(self.mcp_panel())
    }

    /// Takes a decided proposal off the panel.
    pub fn mcp_dismiss(&self, id: &str) -> McpPanel {
        self.inner.mcp.server.approvals().dismiss(id);
        self.mcp_announce();
        self.mcp_panel()
    }

    /// Lets an agent apply its own proposals without asking, until Windfall
    /// quits or the server is turned off; or stops it.
    pub fn mcp_trust(&self, agent: &str, trusted: bool) -> McpPanel {
        self.inner
            .mcp
            .server
            .approvals()
            .set_trusted(agent, trusted);
        self.mcp_announce();
        self.mcp_panel()
    }

    /// The interface reports what the user is looking at, while the server
    /// is on. Nothing is kept while it is off.
    pub fn mcp_focus(&self, focus: McpFocus) {
        if self.inner.mcp.listening().is_none() {
            return;
        }
        *lock(&self.inner.mcp.focus) = Focus {
            channel: focus.channel,
            track: focus.track,
            view: focus
                .view
                .filter(|view| view.len() <= 64 && view.chars().all(|c| c.is_ascii_alphanumeric())),
        };
    }

    /// Stops an agent's render from the panel.
    pub fn mcp_cancel_render(&self) {
        if let Some(flag) = lock(&self.inner.mcp.render).as_ref() {
            flag.store(true, Ordering::Release);
        }
    }

    /// Renders what an agent asked for, offline, with the export's engine
    /// and the same sampler and plugin preparation as a bounce.
    pub(crate) fn mcp_render(
        &self,
        request: RenderRequest,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<RenderedAudio, String> {
        let (mut project, pool, plugin_revision) = {
            let _recording = self.recording_idle()?;
            let state = self.state();
            if state
                .pool
                .needs_sampler_preparation(state.document.project())
            {
                return Err("Sampler audio is still being prepared; try again in a moment.".into());
            }
            let mut pool = state.pool.clone();
            pool.share_sampler_budget(&state.pool);
            (
                state.document.project().clone(),
                pool,
                self.plugin_revision(),
            )
        };
        project = self.capture_plugins(project, plugin_revision)?;
        let (mode, region, pattern, loops, ticks) = match request.region {
            RenderRegion::Song => {
                let end = project
                    .playlist
                    .clips
                    .iter()
                    .map(|clip| clip.start.saturating_add(clip.length))
                    .max()
                    .unwrap_or(0);
                (PlayMode::Song, None, None, 1, end)
            }
            RenderRegion::Range { start, end } => {
                let range = TickRange { start, end };
                range.check()?;
                (PlayMode::Song, Some(range), None, 1, end - start)
            }
            RenderRegion::Pattern { pattern, loops } => {
                let length = project
                    .pattern(pattern)
                    .ok_or("That pattern is no longer in the project.")?
                    .length_ticks();
                (
                    PlayMode::Pattern,
                    None,
                    Some(pattern),
                    loops,
                    length.saturating_mul(loops),
                )
            }
        };
        let seconds = f64::from(ticks) / f64::from(PPQ) * 60.0 / slowest_tempo(&project);
        if seconds > request.max_seconds {
            return Err(format!(
                "the region lasts up to {seconds:.0} seconds, longer than maxSeconds ({:.0}); render a shorter region",
                request.max_seconds
            ));
        }
        if let Some(solo) = request.solo {
            for track in &mut project.mixer.tracks {
                track.solo = track.id == solo;
            }
        }
        let flag = Arc::new(AtomicBool::new(false));
        *lock(&self.inner.mcp.render) = Some(flag.clone());
        self.mcp_announce();
        let options = RenderOptions {
            region,
            mode,
            pattern,
            pattern_loops: loops,
            sample_rate: RENDER_RATE,
            tail_secs: 0.0,
            auto_tail: false,
            ..Default::default()
        };
        let limit = (request.max_seconds * f64::from(RENDER_RATE)).ceil() as usize * 2;
        let mut samples = Vec::new();
        let mut over = false;
        let render_pool = pool.prepare_plugin_render();
        let rendered = render_streaming_checked(
            &project,
            &render_pool,
            &options,
            &mut |block| {
                if samples.len() + block.len() > limit {
                    over = true;
                    return false;
                }
                samples.extend_from_slice(block);
                true
            },
            &mut |_| !cancelled() && !flag.load(Ordering::Acquire),
        );
        *lock(&self.inner.mcp.render) = None;
        self.mcp_announce();
        let rendered = rendered.map_err(|error| error.to_string())?;
        if over {
            return Err(format!(
                "the render ran past maxSeconds ({:.0}); render a shorter region",
                request.max_seconds
            ));
        }
        if !rendered.completed {
            return Err("The render was cancelled.".into());
        }
        if rendered.frames == 0 {
            return Err("The region rendered no audio.".into());
        }
        let path = self.keep_render(&samples)?;
        // Each channel that has notes in the region is rendered alone, and
        // stops at its first audible block: a channel that sounds costs
        // little, one that stays silent costs one render of the region.
        let mut silent_channels = Vec::new();
        for channel in channels_heard(&project, request.region, request.solo)
            .into_iter()
            .take(MAX_SILENCE_CHECKS)
        {
            if cancelled() || flag.load(Ordering::Acquire) {
                return Err("The render was cancelled.".into());
            }
            let mut alone = project.clone();
            for other in &mut alone.channels {
                other.solo = false;
                other.muted = other.id != channel;
            }
            let mut audible = false;
            let mut frames = 0usize;
            let checked = render_streaming_checked(
                &alone,
                &render_pool,
                &options,
                &mut |block| {
                    frames += block.len();
                    audible = block.iter().any(|sample| sample.abs() > 1e-6);
                    !audible && frames <= limit
                },
                &mut |_| !cancelled() && !flag.load(Ordering::Acquire),
            );
            if checked.is_ok() && !audible {
                silent_channels.push(channel);
            }
        }
        Ok(RenderedAudio {
            samples,
            sample_rate: RENDER_RATE,
            path: Some(crate::paths::display(&path)),
            silent_channels,
        })
    }

    /// The file an agent named, if it is inside the browser's folders or
    /// the factory content. See [`crate::library::confined_file`].
    pub(crate) fn mcp_library_file(&self, path: &str) -> Result<PathBuf, String> {
        let roots: Vec<PathBuf> = self
            .browser_roots()
            .into_iter()
            .map(|root| PathBuf::from(root.path))
            .collect();
        crate::library::confined_file(path, &roots)
    }

    /// The browser's own search over its folders, for an agent.
    pub(crate) fn mcp_search_library(
        &self,
        query: &str,
        kind: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<LibraryPage, String> {
        let results = self.library_search(&windfall_ipc::LibrarySearch {
            query: query.to_owned(),
            favorites_only: false,
            tags: Vec::new(),
        })?;
        let matching: Vec<_> = results
            .entries
            .iter()
            .filter(|entry| {
                kind.is_none_or(|kind| {
                    agent_file_kind(std::path::Path::new(&entry.entry.path)) == kind
                })
            })
            .collect();
        Ok(LibraryPage {
            total: matching.len(),
            items: matching
                .iter()
                .skip(offset)
                .take(limit)
                .map(|entry| {
                    let path = std::path::Path::new(&entry.entry.path);
                    LibraryFile {
                        path: entry.entry.path.clone(),
                        kind: agent_file_kind(path).to_owned(),
                        bytes: std::fs::metadata(path).map_or(0, |metadata| metadata.len()),
                        relative_path: entry.relative_path.clone(),
                    }
                })
                .collect(),
            indexing: results.status == windfall_ipc::LibraryStatus::Indexing,
            truncated: results.truncated || results.results_truncated,
        })
    }

    /// The channel presets of the user's preset library.
    pub(crate) fn mcp_channel_presets(&self) -> Result<Vec<ChannelPresetInfo>, String> {
        Ok(self
            .preset_library()?
            .into_iter()
            .filter(|entry| entry.kind == windfall_project::PresetKind::Channel)
            .map(|entry| ChannelPresetInfo {
                instrument: entry.plugin.as_ref().map_or_else(
                    || "sampler".to_owned(),
                    |plugin| {
                        serde_json::from_value::<windfall_project::InstrumentKind>(
                            serde_json::json!(plugin.id),
                        )
                        .map_or_else(|_| plugin.id.clone(), |kind| kind.name().to_owned())
                    },
                ),
                name: entry.name,
                path: entry.path,
                origin: "library".to_owned(),
            })
            .collect())
    }

    /// Applies an agent's approved proposal whose edit Windfall makes
    /// itself, as one undo step.
    pub(crate) fn mcp_native_apply(
        &self,
        proposal: CommandProposal,
        edit: NativeEdit,
    ) -> Result<windfall_project::DispatchResult, String> {
        match edit {
            NativeEdit::Plugin {
                path, id, track, ..
            } => self.mcp_add_plugin(proposal, &path, &id, track),
            NativeEdit::SoundFont {
                channel,
                path,
                bank,
                program,
                replace_instrument,
                ..
            } => {
                // The folders may have changed since it was proposed.
                let file = self.mcp_library_file(&path)?;
                self.mcp_soundfont_apply(
                    &proposal,
                    channel,
                    &file,
                    bank,
                    program,
                    replace_instrument,
                )
            }
            NativeEdit::ChannelPreset { channel, path, .. } => {
                if !self
                    .mcp_channel_presets()?
                    .iter()
                    .any(|preset| preset.path == path)
                {
                    return Err("that channel preset is no longer in the preset library".into());
                }
                if !proposal.commands.is_empty() {
                    proposal.batch()?;
                }
                let expected = self
                    .state()
                    .document
                    .project()
                    .channel(channel)
                    .cloned()
                    .ok_or("the channel no longer exists")?;
                let load = self.preset_channel_command(&path, channel, expected, false, false)?;
                let mut commands = proposal.commands.clone();
                commands.push(load);
                self.dispatch_file_command(
                    Command::Batch {
                        label: Some(proposal.label.clone()),
                        commands,
                    },
                    proposal.stamp.generation,
                    proposal.stamp.revision,
                )
                .map_err(|error| {
                    if error.contains("changed while the file") {
                        "proposal is stale; generate a new preview".to_owned()
                    } else {
                        error
                    }
                })
            }
        }
    }

    /// Keeps a render as a Float32 WAV in the recordings folder's "Agent
    /// renders", and lets only the newest few stay there.
    fn keep_render(&self, samples: &[f32]) -> Result<PathBuf, String> {
        let folder = self.store().recordings_dir().join("Agent renders");
        std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
        let mut kept: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&folder)
            .map_err(|error| error.to_string())?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if !(name.starts_with("Render-") && name.ends_with(".wav")) {
                    return None;
                }
                Some((entry.metadata().ok()?.modified().ok()?, entry.path()))
            })
            .collect();
        kept.sort();
        while kept.len() >= KEPT_RENDERS {
            let (_, oldest) = kept.remove(0);
            let _ = std::fs::remove_file(oldest);
        }
        let path = tempfile::Builder::new()
            .prefix("Render-")
            .suffix(".wav")
            .tempfile_in(&folder)
            .map_err(|error| error.to_string())?
            .into_temp_path()
            .keep()
            .map_err(|error| error.to_string())?;
        let audio = AudioBuffer::from_interleaved(RENDER_RATE, 2, samples.to_vec());
        windfall_codec::write_wav(&path, &audio, windfall_codec::WavSampleFormat::Float32)
            .map_err(|error| error.to_string())?;
        Ok(path)
    }
}
