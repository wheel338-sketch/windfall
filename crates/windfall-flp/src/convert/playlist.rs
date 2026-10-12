//! Playlist placement, with automation linked only to understood controls.
use super::arrangements::PlaylistReferences;
use super::{Builder, ChannelRole, name_or};
use crate::{
    model::PlaylistSource,
    report::{Outcome, ReportSection},
    target::{ChannelParam, ControlTarget, InsertParam, MainParam, SlotParam},
    units,
};
use std::collections::{BTreeMap, BTreeSet};
use windfall_project::{
    AutomationId, AutomationPoint, AutomationTarget, ClipContent, ClipId, ClipInit, Command,
    MAX_AUTOMATION_POINTS, MAX_SONG_TICKS, PlaylistTrackId, PlaylistTrackPatch,
};

impl Builder<'_> {
    pub(super) fn automation_target(&self, target: ControlTarget) -> Option<AutomationTarget> {
        Some(match target {
            ControlTarget::Channel { channel, param } => {
                let ChannelRole::Plays(channel) = self.channels.get(&channel)? else {
                    return None;
                };
                match param {
                    ChannelParam::Volume => AutomationTarget::ChannelVolume { channel: *channel },
                    ChannelParam::Pan => AutomationTarget::ChannelPan { channel: *channel },
                    _ => return None,
                }
            }
            ControlTarget::Insert { insert, param } => {
                let track = *self.tracks.get(&insert)?;
                match param {
                    InsertParam::Volume => AutomationTarget::TrackVolume { track },
                    InsertParam::Pan => AutomationTarget::TrackPan { track },
                    _ => return None,
                }
            }
            ControlTarget::Route { insert, target } if self.sends.contains(&(insert, target)) => {
                AutomationTarget::SendGain {
                    track: *self.tracks.get(&insert)?,
                    target: *self.tracks.get(&target)?,
                }
            }
            ControlTarget::Slot {
                insert,
                slot,
                param: SlotParam::Mix,
            } => {
                let e = self.effects.get(&(insert, slot))?;
                AutomationTarget::EffectMix {
                    track: e.track,
                    effect: e.id,
                }
            }
            ControlTarget::SlotPlugin {
                insert,
                slot,
                param,
            } => {
                let e = self.effects.get(&(insert, slot))?;
                let link = e.links.iter().find(|l| l.fl_param == param)?;
                let index = e
                    .kind
                    .descriptors()
                    .iter()
                    .position(|info| info.id == link.id)?;
                AutomationTarget::EffectParam {
                    track: e.track,
                    effect: e.id,
                    param: index as u32,
                }
            }
            ControlTarget::Main(MainParam::Tempo) => AutomationTarget::Tempo,
            ControlTarget::Main(MainParam::Volume) => AutomationTarget::TrackVolume {
                track: windfall_project::TrackId::MASTER,
            },
            _ => return None,
        })
    }

    /// Event 223 stores source integers, not automation clips' normalized doubles.
    pub(super) fn recorded_value(
        &self,
        source: ControlTarget,
        target: &AutomationTarget,
        raw: i32,
    ) -> Option<f32> {
        let actual = match source {
            ControlTarget::Channel {
                param: ChannelParam::Volume,
                ..
            } => units::channel_gain(raw.max(0) as u32),
            ControlTarget::Channel {
                param: ChannelParam::Pan,
                ..
            } => units::channel_pan(raw.max(0) as u32),
            ControlTarget::Insert {
                param: InsertParam::Volume,
                ..
            }
            | ControlTarget::Route { .. }
            | ControlTarget::Main(MainParam::Volume) => units::fader_gain(raw),
            ControlTarget::Insert {
                param: InsertParam::Pan,
                ..
            } => units::insert_pan(raw),
            ControlTarget::Slot {
                param: SlotParam::Mix,
                ..
            } => (raw as f32 / 12800.0).clamp(0.0, 1.0),
            ControlTarget::SlotPlugin {
                insert,
                slot,
                param,
            } => {
                let link = self
                    .effects
                    .get(&(insert, slot))?
                    .links
                    .iter()
                    .find(|link| link.fl_param == param)?;
                (link.convert)(f64::from(raw)) as f32
            }
            _ => return None,
        };
        if !actual.is_finite() {
            return None;
        }
        self.project()
            .automation_range(target)
            .map(|range| range.normalized(actual))
    }

    pub(super) fn automation_value(
        &self,
        source: ControlTarget,
        target: &AutomationTarget,
        value: f64,
    ) -> f32 {
        let n = value.clamp(0.0, 1.0);
        let actual = match source {
            ControlTarget::Channel {
                param: ChannelParam::Volume,
                ..
            } => units::channel_gain((n * 12_800.0).round() as u32),
            ControlTarget::Insert {
                param: InsertParam::Volume,
                ..
            }
            | ControlTarget::Route { .. }
            | ControlTarget::Main(MainParam::Volume) => {
                units::fader_gain((n * 16_000.0).round() as i32)
            }
            ControlTarget::Main(MainParam::Tempo) => (10.0 + 512.0 * n) as f32,
            ControlTarget::SlotPlugin {
                insert,
                slot,
                param,
            } => {
                let Some(link) = self
                    .effects
                    .get(&(insert, slot))
                    .and_then(|e| e.links.iter().find(|l| l.fl_param == param))
                else {
                    return n as f32;
                };
                (link.convert)(link.fl_range.0 + n * (link.fl_range.1 - link.fl_range.0)) as f32
            }
            _ => return n as f32,
        };
        self.project()
            .automation_range(target)
            .map_or(n as f32, |range| range.normalized(actual))
    }

    fn automation_for(&mut self, iid: u16) -> Vec<AutomationId> {
        let Some(channel) = self.flp.channel(iid) else {
            return Vec::new();
        };
        let Some(curve) = &channel.automation else {
            return Vec::new();
        };
        let mut ids = Vec::new();
        for link in &self.flp.remote_controllers {
            if link.source_channel() != Some(iid) {
                continue;
            }
            let source = link.target();
            let Some(target) = self.automation_target(source) else {
                self.report
                    .count(ReportSection::Automation, Outcome::Dropped, 1);
                self.report.say(
                    ReportSection::Automation,
                    Outcome::Dropped,
                    "An automation target is unsupported or was not imported.",
                );
                continue;
            };
            let mut position = 0.0;
            let mut points: Vec<AutomationPoint> = Vec::new();
            let mut approximated = false;
            for point in curve.points.iter().take(MAX_AUTOMATION_POINTS) {
                if !point.offset.is_finite() || point.offset < 0.0 || !point.value.is_finite() {
                    approximated = true;
                    continue;
                }
                position += point.offset * 960.0;
                approximated |= matches!(
                    source,
                    ControlTarget::Channel {
                        param: ChannelParam::Volume,
                        ..
                    } | ControlTarget::Insert {
                        param: InsertParam::Volume,
                        ..
                    } | ControlTarget::Route { .. }
                        | ControlTarget::Main(MainParam::Volume)
                        | ControlTarget::SlotPlugin { .. }
                );
                if !position.is_finite() || position > f64::from(MAX_SONG_TICKS) {
                    approximated = true;
                    break;
                }
                if let Some(previous) = points.last_mut() {
                    previous.curve = units::automation_curve(point.tension);
                    previous.hold = point.mode == 2;
                }
                approximated |= point.tension != 0.0
                    || ![0, 2].contains(&point.mode)
                    || position.fract() != 0.0;
                points.push(AutomationPoint {
                    tick: position.round() as u32,
                    value: self.automation_value(source, &target, point.value),
                    curve: 0.0,
                    hold: false,
                });
            }
            approximated |= curve.points.len() > MAX_AUTOMATION_POINTS
                || link.smoothing != 0
                || link.flags != 0;
            if points.is_empty() {
                self.report
                    .count(ReportSection::Automation, Outcome::Dropped, 1);
                continue;
            }
            let outcome = if approximated {
                Outcome::Approximated
            } else {
                Outcome::Exact
            };
            let id = self.create(
                ReportSection::Automation,
                "An automation curve",
                Command::AddAutomation {
                    name: channel.name.clone(),
                    target,
                    points: Some(points),
                },
            );
            self.report.count(
                ReportSection::Automation,
                if id.is_some() {
                    outcome
                } else {
                    Outcome::Dropped
                },
                1,
            );
            if approximated {
                self.report.say(ReportSection::Automation, outcome, "Automation tension, unsupported modes, controller mappings or model limits were approximated; controller formulas and smoothing are not reproduced.");
            }
            if let Some(id) = id {
                ids.push(AutomationId(id));
            }
        }
        if ids.is_empty() {
            self.report.say(
                ReportSection::Automation,
                Outcome::Dropped,
                "An automation channel has no usable target and its clips were left out.",
            );
        }
        ids
    }

    pub(super) fn playlist_arrangement(
        &mut self,
        arrangement: &crate::model::Arrangement,
    ) -> PlaylistReferences {
        let mut references = PlaylistReferences {
            items: vec![Vec::new(); arrangement.items.len()],
            legacy_items: vec![Vec::new(); arrangement.legacy_items.len()],
            tracks: BTreeMap::new(),
        };
        let mut used: BTreeSet<u16> = arrangement.items.iter().map(|i| i.track).collect();
        used.extend(
            arrangement
                .tracks
                .iter()
                .filter_map(|t| u16::try_from(t.iid.checked_sub(1)?).ok()),
        );
        if !arrangement.legacy_items.is_empty() {
            used.insert(0);
        }
        let mut tracks = BTreeMap::new();
        for index in used {
            let source = arrangement
                .tracks
                .iter()
                .find(|t| t.iid == u32::from(index) + 1);
            let name = name_or(source.and_then(|t| t.name.as_deref()), || {
                format!("Track {}", u32::from(index) + 1)
            });
            let Some(id) = self.create(
                ReportSection::Playlist,
                "A playlist track",
                Command::AddPlaylistTrack {
                    name: Some(name),
                    index: None,
                },
            ) else {
                continue;
            };
            let id = PlaylistTrackId(id);
            tracks.insert(index, id);
            let mut track_outcome = Outcome::Exact;
            if let Some(source) = source {
                self.apply(
                    ReportSection::Playlist,
                    "Playlist track mute",
                    Command::UpdatePlaylistTrack {
                        id,
                        patch: PlaylistTrackPatch {
                            name: None,
                            muted: Some(!source.enabled),
                            solo: None,
                            color: super::chosen_color(source.color),
                            height: if source.height == 1.0 {
                                None
                            } else {
                                Some((48.0 * source.height).round().clamp(18.0, 160.0) as u32)
                            },
                        },
                    },
                );
                if !source.height.is_finite()
                    || !(18.0 / 48.0..=160.0 / 48.0).contains(&source.height)
                {
                    track_outcome = Outcome::Approximated;
                    self.report.say(
                        ReportSection::Playlist,
                        Outcome::Dropped,
                        "Playlist track height was brought within Windfall's limits.",
                    );
                }
            }
            self.report.count(ReportSection::Playlist, track_outcome, 1);
        }
        let mut automations = BTreeMap::new();
        for channel in &self.flp.channels {
            if matches!(
                self.channels.get(&channel.iid),
                Some(ChannelRole::Automation)
            ) {
                automations.insert(channel.iid, self.automation_for(channel.iid));
            }
        }
        for (index, item) in arrangement.items.iter().enumerate() {
            let Some(&track) = tracks.get(&item.track) else {
                continue;
            };
            let start = units::rescale(item.position, self.ppq);
            let length = units::rescale(item.length, self.ppq);
            let mut outcome = if start.exact && length.exact {
                Outcome::Exact
            } else {
                Outcome::Approximated
            };
            let mut contents = Vec::new();
            let mut muted = item.muted();
            let offset = match item.source {
                PlaylistSource::Pattern { pattern, start, .. } => {
                    if let Some(&(pattern, _)) = self.patterns.get(&pattern) {
                        contents.push(ClipContent::Pattern { pattern });
                    }
                    units::rescale(start.unwrap_or(0), self.ppq).ticks
                }
                PlaylistSource::Channel { channel, start, .. } => {
                    match self.channels.get(&channel) {
                        Some(ChannelRole::Audio(source)) => {
                            muted |= source.muted;
                            if let Some(sample) = source.sample {
                                let milliseconds = |ms: f32| {
                                    (f64::from(ms.max(0.0)) * self.tempo_bpm * 960.0 / 60_000.0)
                                        .round()
                                        .clamp(0.0, f64::from(MAX_SONG_TICKS))
                                        as u32
                                };
                                let extra = item.extra;
                                contents.push(ClipContent::Audio {
                                    sample,
                                    mixer_track: source.mixer_track,
                                    output: Default::default(),
                                    normalize: false,
                                    gain: source.gain
                                        * extra.map_or(1.0, |e| {
                                            if e.gain.is_finite() { e.gain } else { 1.0 }
                                        }),
                                    pan: source.pan,
                                    fade_in: extra.map_or(0, |e| milliseconds(e.fade_in)),
                                    fade_out: extra.map_or(0, |e| milliseconds(e.fade_out)),
                                    reverse: source.reverse,
                                    pitch: source.pitch,
                                    stretch: Default::default(),
                                });
                                if source.stretched {
                                    self.report.say(ReportSection::Playlist, Outcome::Approximated, format!("Audio clip \"{}\" keeps its sample but time stretching is not reproduced.", source.name));
                                }
                                outcome = Outcome::Approximated;
                            }
                        }
                        Some(ChannelRole::Automation) => {
                            for &automation in automations.get(&channel).into_iter().flatten() {
                                contents.push(ClipContent::Automation { automation });
                            }
                        }
                        _ => {}
                    }
                    if start.unwrap_or(0.0) != 0.0 {
                        outcome = Outcome::Approximated;
                        self.report.say(ReportSection::Playlist, outcome, "Channel clip offsets use disputed source units and may need adjustment after import.");
                    }
                    let amount = f64::from(start.unwrap_or(0.0));
                    let audio = matches!(self.channels.get(&channel), Some(ChannelRole::Audio(_)));
                    let ticks = if audio {
                        amount * 240.0
                    } else {
                        amount * 960.0
                    };
                    if ticks.is_finite() && ticks >= 0.0 {
                        ticks.round().min(f64::from(MAX_SONG_TICKS)) as u64
                    } else {
                        0
                    }
                }
            };
            if contents.is_empty()
                || length.ticks == 0
                || start.ticks.saturating_add(length.ticks) > u64::from(MAX_SONG_TICKS)
                || offset > u64::from(MAX_SONG_TICKS)
            {
                self.report
                    .count(ReportSection::Playlist, Outcome::Dropped, 1);
                self.report.say(
                    ReportSection::Playlist,
                    Outcome::Dropped,
                    "A clip has unavailable content or lies outside Windfall's timeline limits.",
                );
                continue;
            }
            for content in contents {
                if let Some(ids) = self.apply(
                    ReportSection::Playlist,
                    "A playlist clip",
                    Command::AddClips {
                        clips: vec![ClipInit {
                            track,
                            start: start.ticks as u32,
                            length: Some(length.ticks as u32),
                            offset: Some(offset as u32),
                            muted: Some(muted),
                            content,
                        }],
                    },
                ) {
                    references.items[index].extend(ids.into_iter().map(ClipId));
                } else {
                    outcome = Outcome::Dropped;
                }
            }
            self.report.count(ReportSection::Playlist, outcome, 1);
        }
        for (index, item) in arrangement.legacy_items.iter().enumerate() {
            if let (Some(&track), Some(&(pattern, _))) =
                (tracks.get(&0), self.patterns.get(&item.pattern))
            {
                if let Some(ids) = self.apply(
                    ReportSection::Playlist,
                    "A legacy playlist block",
                    Command::AddClips {
                        clips: vec![ClipInit {
                            track,
                            start: u32::from(item.bar) * self.bar_ticks,
                            length: Some(self.bar_ticks),
                            offset: None,
                            muted: None,
                            content: ClipContent::Pattern { pattern },
                        }],
                    },
                ) {
                    references.legacy_items[index].extend(ids.into_iter().map(ClipId));
                }
                self.report
                    .count(ReportSection::Playlist, Outcome::Approximated, 1);
            }
        }
        references.tracks = tracks;
        references
    }
}
