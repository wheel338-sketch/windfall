//! The read tools: what is in the project, with the ids that refer to it.
//! Nothing here changes anything.

use serde::Serialize;
use serde_json::{Value, json};
use windfall_dsp::{ParamInfo, ParamKind, ParamSet, TrackParams};
use windfall_project::{
    Channel, ChannelId, ChannelSource, ClipContent, EffectKind, InstrumentKind, MAX_PATTERN_TICKS,
    MAX_SONG_TICKS, MixerTrack, Pattern, PatternId, Project, TrackId,
};

use crate::args::{self, Args};
use crate::host::{Focus, Snapshot};
use crate::music::{self, Grid};

/// Items of one kind a summary lists before it says how many it left out.
pub const SUMMARY_ITEMS: usize = 128;
/// Items one page of a paged query holds at most.
pub const MAX_PAGE: u32 = 500;
const DEFAULT_PAGE: u32 = 200;

/// The camelCase name a kind has in commands, such as "subtractiveSynth".
pub fn kind_name<T: Serialize>(kind: &T) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub fn decibels(gain: f32) -> Value {
    if gain > 0.0 {
        json!(((20.0 * f64::from(gain).log10()) * 10.0).round() / 10.0)
    } else {
        json!("-inf")
    }
}

/// The grid of a pattern: its own meter, or the project's.
pub fn pattern_grid(project: &Project, pattern: &Pattern) -> Grid {
    let base = pattern.effective_signature(project.settings.time_signature);
    Grid::new(base, &pattern.timeline.meters, MAX_PATTERN_TICKS)
        .or_else(|_| Grid::new(base, &[], MAX_PATTERN_TICKS))
        .expect("a checked project has a usable meter")
}

/// The grid of the song: the project's meter and its changes.
pub fn song_grid(project: &Project) -> Grid {
    let base = project.settings.time_signature;
    Grid::new(base, &project.playlist.timeline.meters, MAX_SONG_TICKS)
        .or_else(|_| Grid::new(base, &[], MAX_SONG_TICKS))
        .expect("a checked project has a usable meter")
}

fn channel_kind(channel: &Channel) -> Value {
    match &channel.source {
        ChannelSource::Sampler(sampler) => json!({
            "type": "sampler",
            "sample": sampler.sample.map(|id| id.0),
        }),
        ChannelSource::Instrument { params, .. } => {
            let kind = params.kind();
            json!({
                "type": "instrument",
                "instrument": kind_name(&kind),
                "instrumentName": kind.name(),
            })
        }
    }
}

/// Instruments whose keys are drums or slices rather than pitches: their
/// notes say nothing about the song's key.
fn unpitched(kind: InstrumentKind) -> bool {
    matches!(
        kind,
        InstrumentKind::DrumRack
            | InstrumentKind::Kick
            | InstrumentKind::DrumVoice
            | InstrumentKind::LayerKick
            | InstrumentKind::Membrane
            | InstrumentKind::PadBank
            | InstrumentKind::SliceMap
            | InstrumentKind::SliceDeck
            | InstrumentKind::MidiOut
    )
}

/// The key the project's pitched notes suggest, weighting each note by its
/// length and velocity. `None` with fewer than eight pitched notes.
pub fn note_key(project: &Project) -> Option<music::KeyEstimate> {
    let pitched: Vec<ChannelId> = project
        .channels
        .iter()
        .filter(|channel| match &channel.source {
            ChannelSource::Instrument { params, .. } => !unpitched(params.kind()),
            ChannelSource::Sampler(_) => false,
        })
        .map(|channel| channel.id)
        .collect();
    let mut weights = [0.0; 12];
    let mut count = 0usize;
    for pattern in &project.patterns {
        for lane in &pattern.lanes {
            if !pitched.contains(&lane.channel) {
                continue;
            }
            for note in &lane.notes {
                weights[usize::from(note.key % 12)] +=
                    f64::from(note.length.min(4 * windfall_core::PPQ))
                        * f64::from(note.velocity.max(0.05));
                count += 1;
            }
        }
    }
    (count >= 8)
        .then(|| music::estimate_key(&weights))
        .flatten()
}

fn track_inputs(project: &Project, id: TrackId) -> Value {
    let channels: Vec<u32> = project
        .channels
        .iter()
        .filter(|channel| channel.mixer_track == id)
        .map(|channel| channel.id.0)
        .collect();
    let tracks: Vec<u32> = project
        .mixer
        .tracks
        .iter()
        .filter(|track| track.id != id && track.output == Some(id))
        .map(|track| track.id.0)
        .collect();
    let sends: Vec<u32> = project
        .mixer
        .tracks
        .iter()
        .filter(|track| track.sends.iter().any(|send| send.target == id))
        .map(|track| track.id.0)
        .collect();
    json!({"channels": channels, "tracks": tracks, "sendsFrom": sends})
}

fn track_in_use(project: &Project, track: &MixerTrack) -> bool {
    track.id == TrackId::MASTER
        || !track.effects.is_empty()
        || !track.sends.is_empty()
        || track.output.is_some_and(|output| output != TrackId::MASTER)
        || project
            .channels
            .iter()
            .any(|channel| channel.mixer_track == track.id)
        || project.mixer.tracks.iter().any(|other| {
            other.output == Some(track.id) || other.sends.iter().any(|send| send.target == track.id)
        })
        || project.playlist.clips.iter().any(|clip| {
            matches!(clip.content, ClipContent::Audio { mixer_track, .. } if mixer_track == track.id)
        })
}

fn track_summary(project: &Project, track: &MixerTrack) -> Value {
    json!({
        "id": track.id.0,
        "name": track.name,
        "color": music::format_color(track.color),
        "volume": track.volume,
        "volumeDb": decibels(track.volume),
        "pan": track.pan,
        "muted": track.muted,
        "solo": track.solo,
        "output": track.output.map(|output| output.0),
        "sends": track.sends.iter().map(|send| json!({"to": send.target.0, "level": send.gain, "levelDb": decibels(send.gain)})).collect::<Vec<_>>(),
        "effects": track.effects.iter().enumerate().map(|(index, effect)| {
            let kind = effect.params.kind();
            json!({"id": effect.id.0, "index": index, "kind": kind_name(&kind), "name": kind.name(), "enabled": effect.enabled, "mix": effect.mix})
        }).collect::<Vec<_>>(),
        "inputs": track_inputs(project, track.id),
    })
}

fn song_end(project: &Project) -> u32 {
    project
        .playlist
        .clips
        .iter()
        .map(|clip| clip.start.saturating_add(clip.length))
        .max()
        .unwrap_or(0)
}

/// `get_project_summary`.
pub fn project_summary(snapshot: &Snapshot) -> Value {
    let project = snapshot.document.project();
    let song = song_grid(project);
    let channels: Vec<Value> = project
        .channels
        .iter()
        .take(SUMMARY_ITEMS)
        .map(|channel| {
            json!({
                "id": channel.id.0,
                "name": channel.name,
                "source": channel_kind(channel),
                "mixerTrack": channel.mixer_track.0,
                "volume": channel.volume,
                "pan": channel.pan,
                "muted": channel.muted,
                "solo": channel.solo,
                "color": music::format_color(channel.color),
                "group": channel.group,
            })
        })
        .collect();
    let patterns: Vec<Value> = project
        .patterns
        .iter()
        .take(SUMMARY_ITEMS)
        .map(|pattern| {
            let grid = pattern_grid(project, pattern);
            json!({
                "id": pattern.id.0,
                "name": pattern.name,
                "color": music::format_color(pattern.color),
                "lengthSteps": pattern.length_steps,
                "length": grid.format_position(pattern.length_ticks()),
                "meter": music::format_meter(grid.base()),
                "notes": pattern.lanes.iter().map(|lane| json!({"channel": lane.channel.0, "count": lane.notes.len()})).collect::<Vec<_>>(),
            })
        })
        .collect();
    let tracks: Vec<Value> = project
        .playlist
        .tracks
        .iter()
        .take(SUMMARY_ITEMS)
        .map(|track| {
            let clips = project
                .playlist
                .clips
                .iter()
                .filter(|clip| clip.track == track.id)
                .count();
            json!({"id": track.id.0, "name": track.name, "muted": track.muted, "solo": track.solo, "clips": clips, "color": music::format_color(track.color)})
        })
        .collect();
    let used: Vec<&MixerTrack> = project
        .mixer
        .tracks
        .iter()
        .filter(|track| track_in_use(project, track))
        .collect();
    let mixer: Vec<Value> = used
        .iter()
        .take(SUMMARY_ITEMS)
        .map(|track| track_summary(project, track))
        .collect();
    let automations: Vec<Value> = project
        .automations
        .iter()
        .take(SUMMARY_ITEMS)
        .map(|automation| {
            json!({"id": automation.id.0, "name": automation.name, "target": automation.target, "points": automation.points.len()})
        })
        .collect();
    let end = song_end(project);
    json!({
        "stamp": snapshot.stamp,
        "name": project.settings.name,
        "ppq": windfall_core::PPQ,
        "tempoBpm": project.settings.tempo_bpm,
        "meter": music::format_meter(project.settings.time_signature),
        "meterChanges": project.playlist.timeline.meters.iter().map(|change| json!({"at": song.format_position(change.tick), "meter": music::format_meter(change.signature)})).collect::<Vec<_>>(),
        "swing": project.settings.swing,
        "keyFromNotes": note_key(project),
        "songEnd": song.format_position(end),
        "songEndTick": end,
        "channels": channels,
        "channelCount": project.channels.len(),
        "patterns": patterns,
        "patternCount": project.patterns.len(),
        "playlist": {"tracks": tracks, "trackCount": project.playlist.tracks.len(), "clipCount": project.playlist.clips.len()},
        "mixer": mixer,
        "mixerTrackCount": project.mixer.tracks.len(),
        "mixerTracksNotShown": project.mixer.tracks.len().saturating_sub(mixer.len()),
        "automations": automations,
        "samples": project.samples.iter().take(SUMMARY_ITEMS).map(|sample| json!({"id": sample.id.0, "name": sample.name})).collect::<Vec<_>>(),
        "recording": snapshot.recording,
        "positions": "Positions are bar:beat:tick, bars and beats counted from 1; 960 ticks are a quarter note.",
    })
}

fn page(args: &Args) -> Result<(usize, usize), String> {
    let offset = args::optional_u32(args, "offset", 0..=u32::MAX)?.unwrap_or(0) as usize;
    let limit = args::optional_u32(args, "limit", 1..=MAX_PAGE)?.unwrap_or(DEFAULT_PAGE) as usize;
    Ok((offset, limit))
}

fn paged(total: usize, offset: usize, items: Vec<Value>) -> Value {
    let next = offset + items.len();
    json!({
        "total": total,
        "offset": offset,
        "items": items,
        "nextOffset": (next < total).then_some(next),
    })
}

pub fn pattern_of(project: &Project, id: u32) -> Result<&Pattern, String> {
    project
        .pattern(PatternId(id))
        .ok_or_else(|| format!("there is no pattern {id}; get_project_summary lists the patterns"))
}

pub fn channel_of(project: &Project, id: u32) -> Result<&Channel, String> {
    project
        .channel(ChannelId(id))
        .ok_or_else(|| format!("there is no channel {id}; get_project_summary lists the channels"))
}

pub fn track_of(project: &Project, id: u32) -> Result<&MixerTrack, String> {
    project.mixer.track(TrackId(id)).ok_or_else(|| {
        format!("there is no mixer track {id}; get_project_summary lists the mixer tracks in use")
    })
}

/// `list_notes`.
pub fn list_notes(snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &["patternId", "channelId", "offset", "limit"])?;
    let project = snapshot.document.project();
    let pattern = pattern_of(project, args::id(args, "patternId")?)?;
    let channel = args::optional_id(args, "channelId")?;
    if let Some(channel) = channel {
        channel_of(project, channel)?;
    }
    let (offset, limit) = page(args)?;
    let grid = pattern_grid(project, pattern);
    let notes: Vec<(ChannelId, &windfall_project::Note)> = pattern
        .lanes
        .iter()
        .filter(|lane| channel.is_none_or(|channel| lane.channel.0 == channel))
        .flat_map(|lane| lane.notes.iter().map(move |note| (lane.channel, note)))
        .collect();
    let items = notes
        .iter()
        .skip(offset)
        .take(limit)
        .map(|(channel, note)| {
            json!({
                "id": note.id.0,
                "channel": channel.0,
                "pitch": music::pitch_name(note.key),
                "key": note.key,
                "start": grid.format_position(note.start),
                "startTick": note.start,
                "lengthTicks": note.length,
                "velocity": note.velocity,
                "pan": note.pan,
            })
        })
        .collect();
    let mut result = paged(notes.len(), offset, items);
    result["pattern"] = json!({
        "id": pattern.id.0,
        "name": pattern.name,
        "lengthSteps": pattern.length_steps,
        "length": grid.format_position(pattern.length_ticks()),
        "meter": music::format_meter(grid.base()),
    });
    Ok(result)
}

/// `list_clips`.
pub fn list_clips(snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &["from", "to", "trackId", "offset", "limit"])?;
    let project = snapshot.document.project();
    let grid = song_grid(project);
    let from = args::optional_value(args, "from")
        .map(|value| grid.position_value(value))
        .transpose()?
        .unwrap_or(0);
    let to = args::optional_value(args, "to")
        .map(|value| grid.position_value(value))
        .transpose()?
        .unwrap_or(MAX_SONG_TICKS);
    if to <= from {
        return Err("argument to must come after from".into());
    }
    let track = args::optional_id(args, "trackId")?;
    if let Some(track) = track
        && !project.playlist.tracks.iter().any(|t| t.id.0 == track)
    {
        return Err(format!("there is no playlist track {track}"));
    }
    let (offset, limit) = page(args)?;
    let mut clips: Vec<&windfall_project::Clip> = project
        .playlist
        .clips
        .iter()
        .filter(|clip| track.is_none_or(|track| clip.track.0 == track))
        .filter(|clip| clip.start < to && clip.start.saturating_add(clip.length) > from)
        .collect();
    clips.sort_by_key(|clip| (clip.start, clip.id));
    let items = clips
        .iter()
        .skip(offset)
        .take(limit)
        .map(|clip| {
            let content = match &clip.content {
                ClipContent::Pattern { pattern } => json!({
                    "type": "pattern",
                    "patternId": pattern.0,
                    "name": project.pattern(*pattern).map(|p| p.name.as_str()),
                }),
                ClipContent::Audio {
                    sample,
                    mixer_track,
                    ..
                } => json!({
                    "type": "audio",
                    "sampleId": sample.0,
                    "name": project.sample(*sample).map(|s| s.name.as_str()),
                    "mixerTrack": mixer_track.0,
                }),
                ClipContent::Automation { automation } => json!({
                    "type": "automation",
                    "automationId": automation.0,
                    "name": project.automation(*automation).map(|a| a.name.as_str()),
                }),
            };
            json!({
                "id": clip.id.0,
                "track": clip.track.0,
                "start": grid.format_position(clip.start),
                "startTick": clip.start,
                "lengthTicks": clip.length,
                "end": grid.format_position(clip.start.saturating_add(clip.length)),
                "offsetTicks": clip.offset,
                "muted": clip.muted,
                "content": content,
            })
        })
        .collect();
    Ok(paged(clips.len(), offset, items))
}

/// `get_mixer_track`.
pub fn mixer_track(snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(args, &["trackId"])?;
    let project = snapshot.document.project();
    let track = track_of(project, args::id(args, "trackId")?)?;
    let mut result = track_summary(project, track);
    result["sidechains"] = json!(
        track
            .sidechains
            .iter()
            .map(|send| json!({"to": send.target.0, "level": send.gain}))
            .collect::<Vec<_>>()
    );
    result["processingParameters"] = json!(TrackParams::descriptors().len());
    Ok(result)
}

fn describe(info: &ParamInfo, index: usize, value: Option<f32>) -> Value {
    let mut item = json!({
        "index": index,
        "id": info.id,
        "name": info.name,
        "kind": info.kind,
        "unit": info.unit,
        "min": info.min,
        "max": info.max,
        "default": info.default,
        "value": value,
    });
    if info.kind == ParamKind::Choice {
        item["choices"] = json!(
            info.choices
                .iter()
                .map(|choice| choice.label)
                .collect::<Vec<_>>()
        );
        if let Some(choice) = value.and_then(|value| info.choices.get(value.round() as usize)) {
            item["valueLabel"] = json!(choice.label);
        }
    }
    item
}

/// What `get_parameters` and `set_parameter` address: an instrument, an
/// effect or a mixer track's own processing.
pub enum ParamTarget {
    Instrument {
        channel: ChannelId,
        kind: InstrumentKind,
    },
    Effect {
        track: TrackId,
        effect: windfall_project::EffectId,
        kind: EffectKind,
    },
    Track {
        track: TrackId,
    },
}

impl ParamTarget {
    pub fn find(project: &Project, args: &Args) -> Result<Self, String> {
        let channel = args::optional_id(args, "channelId")?;
        let track = args::optional_id(args, "trackId")?;
        let effect = args::optional_id(args, "effectId")?;
        match (channel, track, effect) {
            (Some(channel), None, None) => {
                let found = channel_of(project, channel)?;
                match &found.source {
                    ChannelSource::Instrument { params, .. } => Ok(Self::Instrument {
                        channel: found.id,
                        kind: params.kind(),
                    }),
                    ChannelSource::Sampler(_) => Err(format!(
                        "channel {channel} is a sampler and has no instrument settings"
                    )),
                }
            }
            (None, Some(track), Some(effect)) => {
                let found = track_of(project, track)?;
                let slot = found
                    .effect(windfall_project::EffectId(effect))
                    .ok_or_else(|| format!("mixer track {track} has no effect {effect}"))?;
                Ok(Self::Effect {
                    track: found.id,
                    effect: slot.id,
                    kind: slot.params.kind(),
                })
            }
            (None, Some(track), None) => Ok(Self::Track {
                track: track_of(project, track)?.id,
            }),
            _ => Err("give channelId for an instrument, trackId and effectId for an effect, or trackId alone for a mixer track's own processing".into()),
        }
    }

    pub fn descriptors(&self) -> &'static [ParamInfo] {
        match self {
            Self::Instrument { kind, .. } => kind.descriptors(),
            Self::Effect { kind, .. } => kind.descriptors(),
            Self::Track { .. } => TrackParams::descriptors(),
        }
    }

    pub fn value(&self, project: &Project, index: usize) -> Option<f32> {
        match self {
            Self::Instrument { channel, .. } => match &project.channel(*channel)?.source {
                ChannelSource::Instrument { params, .. } => params.get(index),
                ChannelSource::Sampler(_) => None,
            },
            Self::Effect { track, effect, .. } => project
                .mixer
                .track(*track)?
                .effect(*effect)?
                .params
                .get(index),
            Self::Track { track } => project.mixer.track(*track)?.processing.get(index),
        }
    }

    pub fn describe_target(&self) -> Value {
        match self {
            Self::Instrument { channel, kind } => {
                json!({"channelId": channel.0, "instrument": kind_name(kind), "name": kind.name()})
            }
            Self::Effect {
                track,
                effect,
                kind,
            } => {
                json!({"trackId": track.0, "effectId": effect.0, "effect": kind_name(kind), "name": kind.name()})
            }
            Self::Track { track } => json!({"trackId": track.0, "name": "Mixer track processing"}),
        }
    }
}

/// `get_parameters`.
pub fn parameters(snapshot: &Snapshot, args: &Args) -> Result<Value, String> {
    args::known(
        args,
        &[
            "channelId",
            "trackId",
            "effectId",
            "search",
            "offset",
            "limit",
        ],
    )?;
    let project = snapshot.document.project();
    let target = ParamTarget::find(project, args)?;
    let search = args::optional_text(args, "search", 128)?.map(str::to_lowercase);
    let (offset, limit) = page(args)?;
    let matching: Vec<(usize, &ParamInfo)> = target
        .descriptors()
        .iter()
        .enumerate()
        .filter(|(_, info)| {
            search.as_ref().is_none_or(|search| {
                info.name.to_lowercase().contains(search) || info.id.to_lowercase().contains(search)
            })
        })
        .collect();
    let items = matching
        .iter()
        .skip(offset)
        .take(limit)
        .map(|(index, info)| describe(info, *index, target.value(project, *index)))
        .collect();
    let mut result = paged(matching.len(), offset, items);
    result["target"] = target.describe_target();
    Ok(result)
}

/// `get_selection`.
pub fn selection(snapshot: &Snapshot, focus: &Focus) -> Value {
    let project = snapshot.document.project();
    let pattern = project.pattern(snapshot.transport.pattern);
    json!({
        "pattern": pattern.map(|pattern| json!({"id": pattern.id.0, "name": pattern.name})),
        "mode": snapshot.transport.mode,
        "playing": snapshot.transport.playing,
        "recording": snapshot.recording,
        "channel": focus.channel.and_then(|id| project.channel(id)).map(|channel| json!({"id": channel.id.0, "name": channel.name})),
        "mixerTrack": focus.track.and_then(|id| project.mixer.track(id)).map(|track| json!({"id": track.id.0, "name": track.name})),
        "view": focus.view,
    })
}

fn highlights(descriptors: &[ParamInfo]) -> Vec<&'static str> {
    descriptors.iter().take(6).map(|info| info.name).collect()
}

/// `list_instruments_and_effects`.
pub fn kinds(args: &Args) -> Result<Value, String> {
    args::known(args, &["category"])?;
    let category = args::optional_text(args, "category", 16)?;
    if category.is_some_and(|c| c != "instrument" && c != "effect") {
        return Err("argument category must be \"instrument\" or \"effect\"".into());
    }
    let mut items = Vec::new();
    if category != Some("effect") {
        for kind in InstrumentKind::ALL {
            let descriptors = kind.descriptors();
            items.push(json!({
                "category": "instrument",
                "kind": kind_name(&kind),
                "name": kind.name(),
                "parameters": descriptors.len(),
                "description": format!("{}, a built-in instrument with {} settings", kind.name(), descriptors.len()),
                "firstSettings": highlights(descriptors),
            }));
        }
    }
    if category != Some("instrument") {
        for kind in EffectKind::ALL {
            let descriptors = kind.descriptors();
            items.push(json!({
                "category": "effect",
                "kind": kind_name(&kind),
                "name": kind.name(),
                "parameters": descriptors.len(),
                "description": format!("{}, a built-in mixer effect with {} settings", kind.name(), descriptors.len()),
                "firstSettings": highlights(descriptors),
            }));
        }
    }
    Ok(json!({"items": items}))
}
