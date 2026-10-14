//! Checks that a project obeys every rule the model promises.
//!
//! Commands keep these rules by construction. The check exists for projects
//! that come from outside, such as a file on disk, and for tests.

use std::collections::HashSet;
use std::hash::Hash;

use crate::model::{AutomationId, MAX_AUTOMATION_POINTS};
use crate::model::{
    Channel, ChannelId, ChannelSource, ClipContent, EffectId, Envelope, FORMAT_VERSION,
    MAX_EFFECT_SLOTS, MAX_ENVELOPE_MS, MAX_GAIN, MAX_KEY, MAX_MIXER_TRACKS, MAX_PATTERN_STEPS,
    MAX_SONG_TICKS, MAX_TEMPO_BPM, MAX_TUNE_SEMITONES, MIN_TEMPO_BPM, Mixer, MixerTrack, Note,
    NoteId, Pattern, Project, SamplerSettings, TimeSignature, TrackId,
};

impl Project {
    /// Verifies every rule of the project model and names the first one that
    /// is broken.
    ///
    /// The rules are the ones the doc comments in [`model`](crate::model)
    /// state, plus these, which the commands also keep:
    ///
    /// - Only the master mixer track has id 0, and every other id is below
    ///   `next_id`.
    /// - Lanes are sorted by channel id.
    /// - The master track has no sends, so rerouting a track to the master
    ///   can never close a loop.
    /// - A track has at most one send to any other track.
    /// - Sample paths are valid; unique audio assets may share an immutable file.
    /// - The sample and the mixer track of an audio clip exist.
    /// - What an automation moves exists: its channel, track, send or
    ///   effect is there, the effect is on the track the target names, and
    ///   the setting is one its processor has.
    /// - A note ends at or before the largest tick a `u32` holds. Where it
    ///   starts is not checked: the commands keep new and moved notes
    ///   inside the longest pattern, and a note that an older file has
    ///   further out is harmless, because it never plays.
    /// - The settings of an effect or instrument are stored with every
    ///   value already inside its range.
    pub fn check(&self) -> Result<(), String> {
        if self.format_version != FORMAT_VERSION {
            return Err(windfall_core::notice!(
                "project.check.formatVersion",
                found = self.format_version,
                expected = FORMAT_VERSION
            )
            .to_string());
        }
        check_settings(self)?;
        crate::preset::check_render_sources(self)?;
        if self.note_edit_groups.len() > 65536
            || self
                .note_edit_groups
                .iter()
                .map(|g| g.notes.len())
                .sum::<usize>()
                > 262144
            || self.note_edit_groups.iter().any(|g| {
                g.group == 0
                    || g.pattern.0 == 0
                    || g.channel.0 == 0
                    || g.notes.iter().any(|n| n.0 == 0 || n.0 >= self.next_id)
            })
        {
            return Err(windfall_core::notice!("project.check.noteEditGroups").to_string());
        }
        if self.retained_source_data.len() > 32
            || self.retained_source_data.iter().any(|entry| {
                entry.source.is_empty()
                    || entry.source.len() > 256
                    || entry.format.is_empty()
                    || entry.format.len() > 256
            })
            || self
                .retained_source_data
                .iter()
                .try_fold(0_usize, |size, entry| size.checked_add(entry.data.len()))
                .is_none_or(|size| size > 16 * 1024 * 1024 + 22)
        {
            return Err(windfall_core::notice!("project.check.retainedImportData").to_string());
        }
        if let Some(problem) =
            crate::internal_controllers::settings_problem(self, &self.settings.internal_controllers)
        {
            return Err(problem);
        }
        crate::rack::check_project(self)?;
        self.settings.stage_media.check(self)?;
        self.notebook.check().map_err(|error| error.to_string())?;
        self.settings.scripts.check()?;
        self.settings.scripts.check_notes(self)?;
        if let Some(problem) =
            crate::controller_links::links_problem(&self.settings.controller_links, Some(self))
        {
            return Err(problem);
        }
        self.playlist
            .timeline
            .check(self.settings.time_signature, self.next_id)?;
        let mut timeline_ids = HashSet::new();
        let book = &self.playlist.arrangement_book;
        for (&id, timeline) in &book.timelines {
            if !book.arrangements.iter().any(|a| a.id == id) {
                return Err(
                    windfall_core::notice!("project.check.timelineLayoutMissing").to_string(),
                );
            }
            timeline.check(self.settings.time_signature, self.next_id)?;
        }
        for timeline in std::iter::once(&self.playlist.timeline)
            .chain(self.patterns.iter().map(|pattern| &pattern.timeline))
            .chain(
                book.timelines
                    .iter()
                    .filter(|(id, _)| Some(**id) != book.active)
                    .map(|(_, timeline)| timeline),
            )
        {
            for id in timeline
                .meters
                .iter()
                .map(|meter| meter.id.0)
                .chain(timeline.markers.iter().map(|marker| marker.id.0))
            {
                if !timeline_ids.insert(id) {
                    return Err(
                        windfall_core::notice!("project.check.timelineIdsRepeated").to_string()
                    );
                }
            }
        }
        let entity_ids = self
            .samples
            .iter()
            .map(|s| s.id.0)
            .chain(self.channels.iter().map(|c| c.id.0))
            .chain(self.patterns.iter().map(|p| p.id.0))
            .chain(
                self.patterns
                    .iter()
                    .flat_map(|p| &p.lanes)
                    .flat_map(|l| &l.notes)
                    .map(|n| n.id.0),
            )
            .chain(self.mixer.tracks.iter().map(|t| t.id.0))
            .chain(
                self.mixer
                    .tracks
                    .iter()
                    .flat_map(|t| &t.effects)
                    .map(|e| e.id.0),
            )
            .chain(self.playlist.tracks.iter().map(|t| t.id.0))
            .chain(self.playlist.clips.iter().map(|c| c.id.0))
            .chain(self.automations.iter().map(|a| a.id.0))
            .collect::<HashSet<_>>();
        if entity_ids.iter().any(|id| timeline_ids.contains(id)) {
            return Err(windfall_core::notice!("project.check.timelineIdCollides").to_string());
        }
        if self.playlist.take_groups.len() > crate::take_groups::MAX_TAKE_GROUPS {
            return Err(windfall_core::notice!("project.check.tooManyTakeGroups").to_string());
        }
        let mut groups = HashSet::new();
        let mut associated = HashSet::new();
        for group in &self.playlist.take_groups {
            check_id(self, "take group", group.id, &mut groups)?;
            if entity_ids.contains(&group.id) || timeline_ids.contains(&group.id) {
                return Err(windfall_core::notice!("project.check.takeGroupIdCollides").to_string());
            }
            crate::take_groups::check_group(self, group)?;
            for id in group
                .lanes
                .iter()
                .flat_map(|lane| &lane.takes)
                .map(|take| take.clip)
                .chain(group.comp.iter().copied())
            {
                if !associated.insert(id) {
                    return Err(
                        windfall_core::notice!("project.check.clipInTwoTakeGroups").to_string()
                    );
                }
            }
        }
        check_arrangement_references(self, &self.playlist.arrangement_book)?;
        let book = &self.playlist.arrangement_book;
        let mut metadata_ids = HashSet::new();
        for id in book
            .arrangements
            .iter()
            .map(|item| item.id)
            .chain(book.track_groups.iter().map(|item| item.id))
            .chain(book.clip_groups.iter().map(|item| item.id))
        {
            check_id(self, "arrangement metadata", id, &mut metadata_ids)?;
            if entity_ids.contains(&id) || timeline_ids.contains(&id) || groups.contains(&id) {
                return Err(
                    windfall_core::notice!("project.check.arrangementIdCollides").to_string(),
                );
            }
        }
        check_samples(self)?;
        check_mixer(self)?;
        check_channels(self)?;
        let mut targets = HashSet::new();
        for plugin in &self.plugins {
            plugin.validate().map_err(str::to_owned)?;
            let exists = match plugin.target {
                crate::PluginTarget::Instrument { channel } => {
                    self.channel(channel).is_some_and(|channel| {
                        matches!(channel.source, ChannelSource::Instrument { .. })
                    })
                }
                crate::PluginTarget::Effect { effect } => self
                    .mixer
                    .tracks
                    .iter()
                    .any(|track| track.effect(effect).is_some()),
                crate::PluginTarget::Rack { .. } => false, // bindings live in their node
            };
            if !exists || !targets.insert(plugin.target) {
                return Err(windfall_core::notice!("project.check.pluginOwner").to_string());
            }
        }
        self.check_plugin_routing()?;
        check_patterns(self)?;
        check_automations(self)?;
        check_playlist(self)
    }
}

/// True when `value` is a number from `min` to `max`. False for NaN.
fn within(value: f32, min: f32, max: f32) -> bool {
    value >= min && value <= max
}

fn check_color(owner: &str, color: u32) -> Result<(), String> {
    if color > 0xFF_FFFF {
        return Err(windfall_core::notice!(
            "project.check.color",
            owner = owner,
            color = format!("{:#x}", color)
        )
        .to_string());
    }
    Ok(())
}

/// Checks that an id is below `next_id`, is not the reserved 0, and has not
/// been seen before in `seen`.
fn check_id<T: Copy + Eq + Hash + Into<u32>>(
    project: &Project,
    kind: &str,
    id: T,
    seen: &mut HashSet<T>,
) -> Result<(), String> {
    let raw: u32 = id.into();
    if raw == 0 {
        return Err(windfall_core::notice!(
            "project.check.idZero",
            kind = windfall_core::notice::term(kind)
        )
        .to_string());
    }
    if raw >= project.next_id {
        return Err(windfall_core::notice!(
            "project.check.idTooHigh",
            kind = windfall_core::notice::term(kind),
            id = raw,
            next = project.next_id
        )
        .to_string());
    }
    if !seen.insert(id) {
        return Err(windfall_core::notice!(
            "project.check.idRepeated",
            kind = windfall_core::notice::term(kind),
            id = raw
        )
        .to_string());
    }
    Ok(())
}

pub(crate) fn time_signature_problem(signature: TimeSignature) -> Option<String> {
    if !(1..=16).contains(&signature.numerator) {
        return Some(
            windfall_core::notice!("project.check.signatureBeats", beats = signature.numerator)
                .to_string(),
        );
    }
    if ![2, 4, 8, 16].contains(&signature.denominator) {
        return Some(
            windfall_core::notice!("project.check.signatureUnit", unit = signature.denominator)
                .to_string(),
        );
    }
    None
}

fn check_settings(project: &Project) -> Result<(), String> {
    let settings = &project.settings;
    if !(settings.tempo_bpm >= MIN_TEMPO_BPM && settings.tempo_bpm <= MAX_TEMPO_BPM) {
        return Err(windfall_core::notice!(
            "project.check.tempo",
            tempo = settings.tempo_bpm,
            min = MIN_TEMPO_BPM,
            max = MAX_TEMPO_BPM
        )
        .to_string());
    }
    if let Some(problem) = time_signature_problem(settings.time_signature) {
        return Err(problem);
    }
    if !within(settings.swing, 0.0, 1.0) {
        return Err(
            windfall_core::notice!("project.check.swing", swing = settings.swing).to_string(),
        );
    }
    if !crate::timebase::is_timebase(settings.timebase_ppq) {
        return Err(windfall_core::notice!(
            "project.check.timebase",
            timebase = settings.timebase_ppq,
            allowed = format!("{:?}", crate::timebase::TIMEBASES)
        )
        .to_string());
    }
    Ok(())
}

fn check_samples(project: &Project) -> Result<(), String> {
    let mut ids = HashSet::new();
    for sample in &project.samples {
        if sample.soundfont
            && (sample.processing.is_some()
                || project.channels.iter().any(|c| {
                    c.source.sample() == Some(sample.id)
                        || c.sample_playback
                            .as_ref()
                            .is_some_and(|a| a.references(sample.id))
                        || c.sample_instrument
                            .as_ref()
                            .is_some_and(|a| a.samples().any(|id| id == sample.id))
                })
                || project
                    .playlist
                    .clips
                    .iter()
                    .any(|c| c.content.sample() == Some(sample.id)))
        {
            return Err(windfall_core::notice!("project.check.soundFontAsSample").to_string());
        }
        crate::model::check_audio_regions(&sample.regions)?;
        check_id(project, "sample", sample.id, &mut ids)?;
        if let Some(recipe) = &sample.processing {
            recipe.check()?;
            let source = recipe
                .source_clip
                .content
                .sample()
                .expect("checked audio recipe");
            if source == sample.id
                || project
                    .sample(source)
                    .is_none_or(|s| s.processing.is_some())
            {
                return Err(windfall_core::notice!("project.check.processedSourceLost").to_string());
            }
        }
        if let Some(problem) = sample.path.problem() {
            return Err(windfall_core::notice!(
                "project.check.samplePath",
                sample = sample.id.0,
                problem = problem
            )
            .to_string());
        }
    }
    Ok(())
}

fn check_mixer(project: &Project) -> Result<(), String> {
    let tracks = &project.mixer.tracks;
    let Some(master) = tracks.first() else {
        return Err(windfall_core::notice!("project.check.noMaster").to_string());
    };
    if master.id != TrackId::MASTER {
        return Err(windfall_core::notice!("project.check.firstNotMaster").to_string());
    }
    if master.current
        || tracks.iter().filter(|track| track.current).count() > 1
        || tracks.iter().filter(|track| !track.current).count() > crate::MAX_MIXER_SIGNAL_TRACKS
    {
        return Err(windfall_core::notice!("project.check.mixerShape").to_string());
    }
    if master.output.is_some() {
        return Err(windfall_core::notice!("project.check.masterOutput").to_string());
    }
    if !master.sends.is_empty() {
        return Err(windfall_core::notice!("project.check.masterSends").to_string());
    }
    if tracks.len() > MAX_MIXER_TRACKS {
        return Err(windfall_core::notice!(
            "project.check.tooManyMixerTracks",
            count = tracks.len(),
            max = MAX_MIXER_TRACKS
        )
        .to_string());
    }

    let mut ids = HashSet::new();
    for track in &tracks[1..] {
        check_id(project, "mixer track", track.id, &mut ids)?;
    }
    let mut effects = HashSet::new();
    for track in tracks {
        let owner = windfall_core::notice!("project.owner.mixerTrack", id = track.id.0).to_string();
        {
            use windfall_dsp::ParamSet;
            if track.processing.sanitized() != track.processing {
                return Err(
                    windfall_core::notice!("project.check.processing", owner = owner).to_string(),
                );
            }
        }
        if track.current
            && (track.output.is_some()
                || !track.sends.is_empty()
                || !track.sidechains.is_empty()
                || track.external_output.is_some()
                || track.recording.is_some()
                || track.solo)
        {
            return Err(windfall_core::notice!("project.check.currentShape").to_string());
        }
        if track
            .output
            .is_some_and(|id| project.mixer.track(id).is_some_and(|target| target.current))
            || track.sends.iter().chain(&track.sidechains).any(|send| {
                project
                    .mixer
                    .track(send.target)
                    .is_some_and(|target| target.current)
            })
        {
            return Err(windfall_core::notice!("project.check.currentRoutes").to_string());
        }
        if let Some(recording) = &track.recording {
            recording.check().map_err(|error| {
                windfall_core::notice!("project.check.ownerProblem", owner = owner, problem = error)
                    .to_string()
            })?;
        }
        if let Some(route) = &track.external_output {
            route.check().map_err(|error| {
                windfall_core::notice!("project.check.ownerProblem", owner = owner, problem = error)
                    .to_string()
            })?;
        }
        if !track.latency_offset_ms.is_finite() || track.latency_offset_ms.abs() > 1000.0 {
            return Err(windfall_core::notice!("project.check.latency", owner = owner).to_string());
        }
        check_effects(project, &owner, track, &mut effects)?;
        check_color(&owner, track.color)?;
        if !within(track.volume, 0.0, MAX_GAIN) {
            return Err(windfall_core::notice!(
                "project.check.volume",
                owner = owner,
                value = track.volume,
                max = MAX_GAIN
            )
            .to_string());
        }
        if !within(track.pan, -1.0, 1.0) {
            return Err(windfall_core::notice!(
                "project.check.pan",
                owner = owner,
                value = track.pan
            )
            .to_string());
        }
        if let Some(output) = track.output
            && project.mixer.track(output).is_none()
        {
            return Err(windfall_core::notice!(
                "project.check.outputMissing",
                owner = owner,
                track = output.0
            )
            .to_string());
        }
        let mut targets = HashSet::new();
        for send in &track.sends {
            if project.mixer.track(send.target).is_none() {
                return Err(windfall_core::notice!(
                    "project.check.sendMissing",
                    owner = owner,
                    track = send.target.0
                )
                .to_string());
            }
            if !targets.insert(send.target) {
                return Err(windfall_core::notice!(
                    "project.check.sendRepeated",
                    owner = owner,
                    track = send.target.0
                )
                .to_string());
            }
            if !within(send.gain, 0.0, MAX_GAIN) {
                return Err(windfall_core::notice!(
                    "project.check.sendGain",
                    owner = owner,
                    value = send.gain,
                    max = MAX_GAIN
                )
                .to_string());
            }
        }
        let mut sidechains = HashSet::new();
        for send in &track.sidechains {
            if track.id == TrackId::MASTER
                || send.target == track.id
                || project.mixer.track(send.target).is_none()
                || !sidechains.insert(send.target)
                || !within(send.gain, 0.0, MAX_GAIN)
            {
                return Err(
                    windfall_core::notice!("project.check.sidechain", owner = owner).to_string(),
                );
            }
        }
    }
    crate::effect_outputs::check(project)?;
    if let Some(track) = routing_cycle(&project.mixer) {
        return Err(
            windfall_core::notice!("project.check.routingLoop", track = track.0).to_string(),
        );
    }
    if project.channels.iter().any(|channel| project.mixer.track(channel.mixer_track).is_some_and(|track| track.current)) || project.playlist.clips.iter().any(|clip| matches!(clip.content, ClipContent::Audio { mixer_track, .. } if project.mixer.track(mixer_track).is_some_and(|track| track.current))) {
        return Err(windfall_core::notice!("project.check.currentInputs").to_string());
    }
    Ok(())
}

fn check_effects(
    project: &Project,
    owner: &str,
    track: &MixerTrack,
    seen: &mut HashSet<EffectId>,
) -> Result<(), String> {
    if track.effects.len() > MAX_EFFECT_SLOTS {
        return Err(windfall_core::notice!(
            "project.check.tooManyEffects",
            owner = owner,
            count = track.effects.len(),
            max = MAX_EFFECT_SLOTS
        )
        .to_string());
    }
    for effect in &track.effects {
        check_id(project, "effect", effect.id, seen)?;
        let name = windfall_core::notice!("project.owner.effect", owner = owner, id = effect.id.0)
            .to_string();
        if !within(effect.mix, 0.0, 1.0) {
            return Err(windfall_core::notice!(
                "project.check.effectMix",
                owner = name,
                value = effect.mix
            )
            .to_string());
        }
        if let Some(asset) = &effect.impulse {
            if effect.kind() != crate::EffectKind::Convolver {
                return Err(
                    windfall_core::notice!("project.check.irOnNonConvolver", owner = name)
                        .to_string(),
                );
            }
            asset.check().map_err(str::to_owned)?;
        }
        if effect.params.sanitized() != effect.params {
            return Err(
                windfall_core::notice!("project.check.effectSetting", owner = name).to_string(),
            );
        }
        if let crate::EffectParams::Scratch(p) = effect.params
            && p.source_id != 0
            && project.sample(crate::SampleId(p.source_id)).is_none()
        {
            return Err(windfall_core::notice!(
                "project.check.scratchSample",
                owner = name,
                sample = p.source_id
            )
            .to_string());
        }
    }
    Ok(())
}

/// Finds a track that can reach itself through outputs and sends.
fn routing_cycle(mixer: &Mixer) -> Option<TrackId> {
    mixer
        .tracks
        .iter()
        .find(|track| {
            routing_targets(mixer, track.id).any(|target| reaches(mixer, target, track.id))
        })
        .map(|track| track.id)
}

/// The tracks `from` feeds directly: its output, its sends and sidechains,
/// and the routed outputs of its effects.
fn routing_targets(mixer: &Mixer, from: TrackId) -> impl Iterator<Item = TrackId> + '_ {
    mixer.track(from).into_iter().flat_map(move |track| {
        track
            .output
            .into_iter()
            .chain(track.sends.iter().map(|send| send.target))
            .chain(track.sidechains.iter().map(|send| send.target))
            .chain(crate::effect_outputs::targets(mixer, from))
    })
}

/// True when signal leaving `from` can arrive at `to`, or they are the same
/// track.
pub(crate) fn reaches(mixer: &Mixer, from: TrackId, to: TrackId) -> bool {
    let mut seen = HashSet::new();
    let mut pending = vec![from];
    while let Some(track) = pending.pop() {
        if track == to {
            return true;
        }
        if seen.insert(track) {
            pending.extend(routing_targets(mixer, track));
        }
    }
    false
}

fn check_channels(project: &Project) -> Result<(), String> {
    let mut ids = HashSet::new();
    for channel in &project.channels {
        check_id(project, "channel", channel.id, &mut ids)?;
        check_channel(project, channel)?;
    }
    crate::layer::check(&project.channels)?;
    Ok(())
}

fn check_channel(project: &Project, channel: &Channel) -> Result<(), String> {
    if let Some(asset) = &channel.sample_playback {
        asset.check(project, channel)?;
    }
    if channel.voice.sanitized() != channel.voice {
        return Err(
            windfall_core::notice!("project.check.voice", channel = channel.id.0).to_string(),
        );
    }
    channel.timing.validate().map_err(|reason| {
        windfall_core::notice!(
            "project.check.channelTiming",
            channel = channel.id.0,
            problem = reason
        )
        .to_string()
    })?;
    crate::expression_map::check(channel)?;
    crate::sample_instrument::check(project, channel)?;
    if channel.group.len() > crate::MAX_CHANNEL_GROUP_NAME_BYTES
        || channel.group.trim() != channel.group
        || channel.group.chars().any(char::is_control)
    {
        return Err(
            windfall_core::notice!("project.check.groupName", channel = channel.id.0).to_string(),
        );
    }
    let owner = windfall_core::notice!("project.owner.channel", id = channel.id.0).to_string();
    check_color(&owner, channel.color)?;
    if !within(channel.volume, 0.0, MAX_GAIN) {
        return Err(windfall_core::notice!(
            "project.check.volume",
            owner = owner,
            value = channel.volume,
            max = MAX_GAIN
        )
        .to_string());
    }
    if !within(channel.pan, -1.0, 1.0) {
        return Err(windfall_core::notice!(
            "project.check.pan",
            owner = owner,
            value = channel.pan
        )
        .to_string());
    }
    if project.mixer.track(channel.mixer_track).is_none() {
        return Err(windfall_core::notice!(
            "project.check.mixerTrackMissing",
            owner = owner,
            track = channel.mixer_track.0
        )
        .to_string());
    }
    match &channel.source {
        ChannelSource::Sampler(sampler) => check_sampler(project, &owner, sampler),
        ChannelSource::Instrument { params, asset } => {
            if let Some(asset) = asset {
                asset.validate(params.kind()).map_err(|error| {
                    windfall_core::notice!(
                        "project.check.ownerProblem",
                        owner = owner,
                        problem = error
                    )
                    .to_string()
                })?;
            }
            if params.sanitized() != *params {
                return Err(windfall_core::notice!(
                    "project.check.instrumentSetting",
                    owner = owner
                )
                .to_string());
            }
            if let windfall_dsp::InstrumentParams::MidiOut(params) = params {
                if !(0..=7).contains(&params.output_port) || params.plugin_destination < -1 {
                    return Err(windfall_core::notice!(
                        "project.check.midiDestination",
                        owner = owner
                    )
                    .to_string());
                }
                if params.plugin_destination > 0 {
                    let target = ChannelId(params.plugin_destination as u32);
                    if target == channel.id
                        || !project.plugins.iter().any(|binding| {
                            binding.target == crate::PluginTarget::Instrument { channel: target }
                        })
                    {
                        return Err(windfall_core::notice!(
                            "project.check.midiDestinationHosted",
                            owner = owner
                        )
                        .to_string());
                    }
                }
            }
            Ok(())
        }
    }
}

fn check_sampler(project: &Project, owner: &str, sampler: &SamplerSettings) -> Result<(), String> {
    sampler.stretch.validate().map_err(|error| {
        windfall_core::notice!("project.check.ownerProblem", owner = owner, problem = error)
            .to_string()
    })?;
    if let Some(sample) = sampler.sample
        && project.sample(sample).is_none()
    {
        return Err(windfall_core::notice!(
            "project.check.sampleMissing",
            owner = owner,
            sample = sample.0
        )
        .to_string());
    }
    if sampler.root_key > MAX_KEY {
        return Err(windfall_core::notice!(
            "project.check.rootKey",
            owner = owner,
            key = sampler.root_key,
            max = MAX_KEY
        )
        .to_string());
    }
    if !within(sampler.tune, -MAX_TUNE_SEMITONES, MAX_TUNE_SEMITONES) {
        return Err(windfall_core::notice!(
            "project.check.tune",
            owner = owner,
            value = sampler.tune,
            max = MAX_TUNE_SEMITONES
        )
        .to_string());
    }
    if !within(sampler.gain, 0.0, MAX_GAIN) {
        return Err(windfall_core::notice!(
            "project.check.sampleGain",
            owner = owner,
            value = sampler.gain,
            max = MAX_GAIN
        )
        .to_string());
    }
    if !within(sampler.start, 0.0, 1.0) || !within(sampler.end, 0.0, 1.0) {
        return Err(windfall_core::notice!(
            "project.check.sampleRange",
            owner = owner,
            start = sampler.start,
            end = sampler.end
        )
        .to_string());
    }
    if sampler.start >= sampler.end {
        return Err(windfall_core::notice!(
            "project.check.sampleStartAfterEnd",
            owner = owner,
            start = sampler.start,
            end = sampler.end
        )
        .to_string());
    }
    if !within(sampler.loop_start, 0.0, 1.0) || !within(sampler.loop_end, 0.0, 1.0) {
        return Err(windfall_core::notice!("project.check.loopRange", owner = owner).to_string());
    }
    if sampler.loop_start >= sampler.loop_end {
        return Err(windfall_core::notice!("project.check.loopOrder", owner = owner).to_string());
    }
    match &sampler.envelope {
        Some(envelope) => check_envelope(owner, envelope),
        None => Ok(()),
    }
}

fn check_envelope(owner: &str, envelope: &Envelope) -> Result<(), String> {
    let times = [
        ("attack", envelope.attack_ms),
        ("decay", envelope.decay_ms),
        ("release", envelope.release_ms),
    ];
    for (name, value) in times {
        if !within(value, 0.0, MAX_ENVELOPE_MS) {
            return Err(windfall_core::notice!(
                "project.check.envelopeTime",
                owner = owner,
                stage = windfall_core::notice::term(name),
                value = value,
                max = MAX_ENVELOPE_MS
            )
            .to_string());
        }
    }
    if !within(envelope.sustain, 0.0, 1.0) {
        return Err(windfall_core::notice!(
            "project.check.envelopeSustain",
            owner = owner,
            value = envelope.sustain
        )
        .to_string());
    }
    Ok(())
}

fn check_patterns(project: &Project) -> Result<(), String> {
    if project.patterns.is_empty() {
        return Err(windfall_core::notice!("project.check.noPatterns").to_string());
    }
    let mut ids = HashSet::new();
    let mut notes = HashSet::new();
    for pattern in &project.patterns {
        check_id(project, "pattern", pattern.id, &mut ids)?;
        check_pattern(project, pattern, &mut notes)?;
    }
    crate::pattern_events::check(project)?;
    Ok(())
}

fn check_pattern(
    project: &Project,
    pattern: &Pattern,
    notes: &mut HashSet<NoteId>,
) -> Result<(), String> {
    let owner = windfall_core::notice!("project.owner.pattern", id = pattern.id.0).to_string();
    pattern.check_musical(project.settings.time_signature, project.next_id)?;
    check_color(&owner, pattern.color)?;
    if !(1..=MAX_PATTERN_STEPS).contains(&pattern.length_steps) {
        return Err(windfall_core::notice!(
            "project.check.patternLength",
            owner = owner,
            steps = pattern.length_steps,
            max = MAX_PATTERN_STEPS
        )
        .to_string());
    }
    let mut previous: Option<ChannelId> = None;
    for lane in &pattern.lanes {
        let lane_owner = windfall_core::notice!(
            "project.owner.lane",
            owner = owner,
            channel = lane.channel.0
        )
        .to_string();
        if previous.is_some_and(|previous| previous >= lane.channel) {
            return Err(
                windfall_core::notice!("project.check.lanesUnsorted", owner = owner).to_string(),
            );
        }
        previous = Some(lane.channel);
        if project.channel(lane.channel).is_none() {
            return Err(windfall_core::notice!(
                "project.check.laneChannelMissing",
                owner = lane_owner
            )
            .to_string());
        }
        if lane.notes.is_empty() {
            return Err(
                windfall_core::notice!("project.check.laneEmpty", owner = lane_owner).to_string(),
            );
        }
        if !lane.notes.is_sorted_by(|a, b| a.sort_key() < b.sort_key()) {
            return Err(
                windfall_core::notice!("project.check.notesUnsorted", owner = lane_owner)
                    .to_string(),
            );
        }
        for note in &lane.notes {
            check_id(project, "note", note.id, notes)?;
            check_note(&lane_owner, note)?;
        }
    }
    Ok(())
}

fn check_note(owner: &str, note: &Note) -> Result<(), String> {
    let name =
        windfall_core::notice!("project.owner.note", owner = owner, id = note.id.0).to_string();
    if note.length == 0 {
        return Err(windfall_core::notice!("project.check.noteLength", owner = name).to_string());
    }
    if note.start.checked_add(note.length).is_none() {
        return Err(windfall_core::notice!("project.check.noteEnd", owner = name).to_string());
    }
    if note.key > MAX_KEY {
        return Err(windfall_core::notice!(
            "project.check.noteKey",
            owner = name,
            key = note.key,
            max = MAX_KEY
        )
        .to_string());
    }
    if !within(note.velocity, 0.0, 1.0) {
        return Err(windfall_core::notice!(
            "project.check.velocity",
            owner = name,
            value = note.velocity
        )
        .to_string());
    }
    if !within(note.pan, -1.0, 1.0) {
        return Err(
            windfall_core::notice!("project.check.pan", owner = name, value = note.pan).to_string(),
        );
    }
    if !note.expression.valid() {
        return Err(
            windfall_core::notice!("project.check.noteExpression", owner = name).to_string(),
        );
    }
    Ok(())
}

fn check_automations(project: &Project) -> Result<(), String> {
    let mut ids: HashSet<AutomationId> = HashSet::new();
    for automation in &project.automations {
        check_id(project, "automation", automation.id, &mut ids)?;
        let owner =
            windfall_core::notice!("project.owner.automation", id = automation.id.0).to_string();
        check_color(&owner, automation.color)?;
        if project.automation_range(&automation.target).is_none() {
            return Err(windfall_core::notice!(
                "project.check.automationTarget",
                owner = owner,
                target = format!("{:?}", automation.target)
            )
            .to_string());
        }
        let points = &automation.points;
        if points.is_empty() {
            return Err(
                windfall_core::notice!("project.check.noPoints", owner = owner).to_string(),
            );
        }
        if points.len() > MAX_AUTOMATION_POINTS {
            return Err(windfall_core::notice!(
                "project.check.tooManyPoints",
                owner = owner,
                count = points.len(),
                max = MAX_AUTOMATION_POINTS
            )
            .to_string());
        }
        if !points.is_sorted_by_key(|point| point.tick) {
            return Err(
                windfall_core::notice!("project.check.pointsUnsorted", owner = owner).to_string(),
            );
        }
        for point in points {
            if point.tick > MAX_SONG_TICKS {
                return Err(windfall_core::notice!(
                    "project.check.pointPastEnd",
                    owner = owner,
                    tick = MAX_SONG_TICKS
                )
                .to_string());
            }
            if !within(point.value, 0.0, 1.0) {
                return Err(windfall_core::notice!(
                    "project.check.pointValue",
                    owner = owner,
                    value = point.value
                )
                .to_string());
            }
            if !within(point.curve, -1.0, 1.0) {
                return Err(windfall_core::notice!(
                    "project.check.pointCurve",
                    owner = owner,
                    value = point.curve
                )
                .to_string());
            }
        }
    }
    Ok(())
}

fn check_playlist(project: &Project) -> Result<(), String> {
    let playlist = &project.playlist;
    let mut tracks = HashSet::new();
    for track in &playlist.tracks {
        check_id(project, "playlist track", track.id, &mut tracks)?;
    }
    if !playlist
        .clips
        .is_sorted_by(|a, b| a.sort_key() < b.sort_key())
    {
        return Err(windfall_core::notice!("project.check.clipsUnsorted").to_string());
    }
    let mut clips = HashSet::new();
    for clip in &playlist.clips {
        check_id(project, "clip", clip.id, &mut clips)?;
        let owner = windfall_core::notice!("project.owner.clip", id = clip.id.0).to_string();
        if !tracks.contains(&clip.track) {
            return Err(windfall_core::notice!(
                "project.check.playlistTrackMissing",
                owner = owner,
                track = clip.track.0
            )
            .to_string());
        }
        if clip.length == 0 {
            return Err(
                windfall_core::notice!("project.check.clipLength", owner = owner).to_string(),
            );
        }
        if u64::from(clip.start) + u64::from(clip.length) > u64::from(MAX_SONG_TICKS) {
            return Err(windfall_core::notice!(
                "project.check.clipPastEnd",
                owner = owner,
                tick = MAX_SONG_TICKS
            )
            .to_string());
        }
        if clip.offset > MAX_SONG_TICKS {
            return Err(windfall_core::notice!(
                "project.check.clipOffset",
                owner = owner,
                tick = MAX_SONG_TICKS
            )
            .to_string());
        }
        match &clip.content {
            ClipContent::Pattern { pattern } => {
                if project.pattern(*pattern).is_none() {
                    return Err(windfall_core::notice!(
                        "project.check.patternMissing",
                        owner = owner,
                        pattern = pattern.0
                    )
                    .to_string());
                }
            }
            ClipContent::Audio {
                sample,
                mixer_track,
                output: _,
                normalize: _,
                gain,
                pan,
                fade_in,
                fade_out,
                reverse: _,
                pitch,
                stretch,
            } => {
                if project.sample(*sample).is_none() {
                    return Err(windfall_core::notice!(
                        "project.check.sampleMissing",
                        owner = owner,
                        sample = sample.0
                    )
                    .to_string());
                }
                if project.mixer.track(*mixer_track).is_none() {
                    return Err(windfall_core::notice!(
                        "project.check.mixerTrackMissing",
                        owner = owner,
                        track = mixer_track.0
                    )
                    .to_string());
                }
                if !within(*gain, 0.0, MAX_GAIN) {
                    return Err(windfall_core::notice!(
                        "project.check.clipGain",
                        owner = owner,
                        value = gain,
                        max = MAX_GAIN
                    )
                    .to_string());
                }
                if !within(*pan, -1.0, 1.0) {
                    return Err(windfall_core::notice!(
                        "project.check.pan",
                        owner = owner,
                        value = pan
                    )
                    .to_string());
                }
                if !within(*pitch, -MAX_TUNE_SEMITONES, MAX_TUNE_SEMITONES) {
                    return Err(windfall_core::notice!(
                        "project.check.clipPitch",
                        owner = owner,
                        value = pitch,
                        max = MAX_TUNE_SEMITONES
                    )
                    .to_string());
                }
                if let crate::ClipStretch::Spectral { ratio, .. } = stretch
                    && (!ratio.is_finite()
                        || !(0.25..=4.0).contains(ratio)
                        || !within(*pitch, -24.0, 24.0))
                {
                    return Err(windfall_core::notice!(
                        "project.check.spectralStretch",
                        owner = owner
                    )
                    .to_string());
                }
                if *fade_in > MAX_SONG_TICKS || *fade_out > MAX_SONG_TICKS {
                    return Err(windfall_core::notice!(
                        "project.check.fadeTooLong",
                        owner = owner,
                        ticks = MAX_SONG_TICKS
                    )
                    .to_string());
                }
            }
            ClipContent::Automation { automation } => {
                if project.automation(*automation).is_none() {
                    return Err(windfall_core::notice!(
                        "project.check.automationMissing",
                        owner = owner,
                        automation = automation.0
                    )
                    .to_string());
                }
            }
        }
    }
    Ok(())
}

/// Resolves every arrangement reference against the single project entity pools.
pub(crate) fn check_arrangement_references(
    project: &Project,
    book: &crate::ArrangementBook,
) -> Result<(), String> {
    book.check().map_err(|error| error.to_string())?;
    let clips: HashSet<_> = project.playlist.clips.iter().map(|clip| clip.id).collect();
    let tracks: HashSet<_> = project
        .playlist
        .tracks
        .iter()
        .map(|track| track.id)
        .collect();
    for clip in book
        .arrangements
        .iter()
        .flat_map(|item| &item.clips)
        .chain(book.clip_groups.iter().flat_map(|group| &group.clips))
    {
        if !clips.contains(clip) {
            return Err(
                windfall_core::notice!("project.check.arrangementClip", id = clip.0).to_string(),
            );
        }
    }
    for track in book
        .arrangements
        .iter()
        .flat_map(|item| &item.tracks)
        .chain(book.track_parents.keys())
        .chain(book.linked_tracks.keys())
    {
        if !tracks.contains(track) {
            return Err(
                windfall_core::notice!("project.check.arrangementTrack", id = track.0).to_string(),
            );
        }
    }
    for kind in book.linked_tracks.values() {
        match *kind {
            crate::arrangement::TrackKind::Instrument { channel }
                if project.channel(channel).is_none() =>
            {
                return Err(windfall_core::notice!(
                    "project.check.arrangementChannel",
                    id = channel.0
                )
                .to_string());
            }
            crate::arrangement::TrackKind::Audio { source } if project.sample(source).is_none() => {
                return Err(windfall_core::notice!(
                    "project.check.arrangementSample",
                    id = source.0
                )
                .to_string());
            }
            crate::arrangement::TrackKind::AudioTrack { mixer }
                if project.mixer.track(mixer).is_none_or(|track| track.current) =>
            {
                return Err(windfall_core::notice!(
                    "project.check.arrangementMixerTrack",
                    id = mixer.0
                )
                .to_string());
            }
            _ => {}
        }
    }
    Ok(())
}
