//! Persisted configuration of internal sources. The owner's ordinary parameters
//! remain automatable; variable-size assets are compiled away from the callback.
use crate::{
    AutomationTarget, ChannelId, ControlSource, ControllerLink, EffectId, EffectKind, LinkTarget,
    Project,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub const MAX_INTERNAL_CONTROLLERS: usize = 256;
pub const MAX_CONTROL_POINTS: usize = 64;
pub const MAX_SURFACE_CONTROLS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ControlPoint {
    pub position: f32,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum SurfaceControlKind {
    Knob,
    Slider,
    Button,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SurfaceControl {
    pub output: u8,
    pub name: String,
    pub kind: SurfaceControlKind,
    /// Grid coordinates, 0 through 7.
    pub column: u8,
    pub row: u8,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct InternalControllerSettings {
    pub controller: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub note_channel: Option<ChannelId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expression: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<ControlPoint>>", optional)]
    pub envelope: Vec<ControlPoint>,
    /// Duration of the drawn envelope; 1..60,000 ms. ADSR is used if empty.
    pub duration_ms: f32,
    /// A complete LFO cycle in beats, or free Hz when absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sync_beats: Option<f32>,
    pub rms: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<SurfaceControl>>", optional)]
    pub controls: Vec<SurfaceControl>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<VoltageRoute>>", optional)]
    pub voltage_routes: Vec<VoltageRoute>,
}

impl Default for InternalControllerSettings {
    fn default() -> Self {
        Self {
            controller: 0,
            note_channel: None,
            expression: None,
            envelope: Vec::new(),
            duration_ms: 1000.0,
            sync_beats: None,
            rms: false,
            controls: Vec::new(),
            voltage_routes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct VoltageRoute {
    pub output: u8,
    /// Zero-based physical audio channel, 0..255.
    pub channel: u8,
    /// Normalized audio sample at control zero. No hardware voltage is implied.
    pub zero: f32,
    /// Audio sample delta for a full-scale control.
    pub scale: f32,
}

pub enum SourceOwner<'a> {
    Effect(&'a crate::EffectSlot),
    Dashboard(&'a windfall_dsp::MidiOutParams),
}
impl SourceOwner<'_> {
    pub fn kind(&self) -> EffectKind {
        match self {
            Self::Effect(effect) => effect.params.kind(),
            Self::Dashboard(_) => EffectKind::ControlSurface,
        }
    }
    pub fn ports(&self) -> &'static [&'static str] {
        outputs(self.kind())
    }
}

/// Stable normalized ports. These descriptions are also suitable for rack nodes.
pub fn outputs(kind: EffectKind) -> &'static [&'static str] {
    match kind {
        EffectKind::NoteEnvelope => &["Envelope"],
        EffectKind::KeyboardSource => &["Pitch", "Velocity"],
        EffectKind::EnvelopeFollower => &["Peak + LFO", "LFO"],
        EffectKind::FormulaSource => &["Result"],
        EffectKind::XyPad => &["X", "Y"],
        EffectKind::XyzPad => &["X", "Y", "Z"],
        EffectKind::ControlSurface | EffectKind::VoltageSource => &[
            "Control 1",
            "Control 2",
            "Control 3",
            "Control 4",
            "Control 5",
            "Control 6",
            "Control 7",
            "Control 8",
        ],
        _ => &[],
    }
}

pub fn source(project: &Project, id: u32) -> Option<(crate::TrackId, SourceOwner<'_>)> {
    for track in &project.mixer.tracks {
        if let Some(effect) = track.effect(EffectId(id))
            && !outputs(effect.params.kind()).is_empty()
        {
            return Some((track.id, SourceOwner::Effect(effect)));
        }
    }
    let channel = project.channel(ChannelId(id))?;
    if let crate::ChannelSource::Instrument {
        params: windfall_dsp::InstrumentParams::Dashboard(params),
        ..
    } = &channel.source
    {
        Some((channel.mixer_track, SourceOwner::Dashboard(params)))
    } else {
        None
    }
}

pub fn settings_problem(
    project: &Project,
    settings: &[InternalControllerSettings],
) -> Option<String> {
    if settings.len() > MAX_INTERNAL_CONTROLLERS {
        return Some("too many internal controller configurations".into());
    }
    for (index, config) in settings.iter().enumerate() {
        let Some((_, effect)) = source(project, config.controller) else {
            return Some("internal controller owner is missing or has no control ports".into());
        };
        if settings[..index]
            .iter()
            .any(|c| c.controller == config.controller)
        {
            return Some("duplicate controller configuration".into());
        }
        if !config.duration_ms.is_finite() || !(1.0..=60_000.0).contains(&config.duration_ms) {
            return Some("envelope duration must be 1..60000 ms".into());
        }
        if config.note_channel.is_some()
            && !matches!(
                effect.kind(),
                EffectKind::NoteEnvelope | EffectKind::KeyboardSource
            )
        {
            return Some("only a note or keyboard source has a note input".into());
        }
        if !config.envelope.is_empty() && effect.kind() != EffectKind::NoteEnvelope {
            return Some("only a note envelope has drawn points".into());
        }
        if (config.rms || config.sync_beats.is_some())
            && effect.kind() != EffectKind::EnvelopeFollower
        {
            return Some("only an audio follower has detector and LFO settings".into());
        }
        if config
            .note_channel
            .is_some_and(|id| project.channel(id).is_none())
        {
            return Some("controller note channel is missing".into());
        }
        if config
            .sync_beats
            .is_some_and(|v| !v.is_finite() || !(0.0625..=64.0).contains(&v))
        {
            return Some("LFO cycle must be 1/16 through 64 beats".into());
        }
        if !config.envelope.is_empty() {
            if config.envelope.len() < 2
                || config.envelope.len() > MAX_CONTROL_POINTS
                || !config.duration_ms.is_finite()
                || !(1.0..=60_000.0).contains(&config.duration_ms)
            {
                return Some(
                    "drawn envelope must have 2..64 points and a duration of 1..60000 ms".into(),
                );
            }
            if config.envelope.first().unwrap().position != 0.0
                || config.envelope.last().unwrap().position != 1.0
            {
                return Some("drawn envelope must start at 0 and end at 1".into());
            }
            let mut previous = -1.0;
            for point in &config.envelope {
                if !point.position.is_finite()
                    || !point.value.is_finite()
                    || !(0.0..=1.0).contains(&point.value)
                    || point.position <= previous
                {
                    return Some("envelope points must be ordered and normalized".into());
                }
                previous = point.position;
            }
        }
        if let Some(text) = &config.expression {
            if effect.kind() != EffectKind::FormulaSource {
                return Some("only a formula source has an expression".into());
            }
            if let Err(error) = windfall_dsp::control::FormulaSource::default().set_expression(text)
            {
                return Some(error.to_string());
            }
        }
        if config.voltage_routes.len() > 8 {
            return Some("DC source has at most eight routes".into());
        }
        for route in &config.voltage_routes {
            if settings
                .iter()
                .filter(|c| c.controller != config.controller)
                .flat_map(|c| &c.voltage_routes)
                .any(|r| r.channel == route.channel)
            {
                return Some("two DC sources reserve the same audio channel".into());
            }
        }
        for route in &config.voltage_routes {
            let audible = project.mixer.tracks.iter().any(|track| {
                let route_audio = track.external_output.or_else(|| {
                    (track.id == crate::TrackId::MASTER).then_some(crate::ExternalOutputRoute {
                        left: 0,
                        right: Some(1),
                        exclusive: false,
                    })
                });
                route_audio.is_some_and(|audio| {
                    audio.left == u16::from(route.channel)
                        || audio.right == Some(u16::from(route.channel))
                })
            });
            if audible {
                return Some("DC output channel is already reserved for audible audio".into());
            }
        }
        for (at, route) in config.voltage_routes.iter().enumerate() {
            if effect.kind() != EffectKind::VoltageSource
                || route.output > 7
                || !route.zero.is_finite()
                || !route.scale.is_finite()
                || !(-1.0..=1.0).contains(&route.zero)
                || !(-2.0..=2.0).contains(&route.scale)
                || config.voltage_routes[..at]
                    .iter()
                    .any(|r| r.output == route.output || r.channel == route.channel)
            {
                return Some("invalid or duplicate DC calibration route".into());
            }
        }
        if config.controls.len() > MAX_SURFACE_CONTROLS {
            return Some("a surface holds at most eight controls".into());
        }
        for (at, control) in config.controls.iter().enumerate() {
            if effect.kind() != EffectKind::ControlSurface
                || control.output >= 8
                || control.name.trim().is_empty()
                || control.name.len() > 64
                || control.column > 7
                || control.row > 7
                || config.controls[..at].iter().any(|c| {
                    c.output == control.output
                        || (c.column == control.column && c.row == control.row)
                })
            {
                return Some("invalid or overlapping surface control".into());
            }
        }
    }
    None
}

fn owner(target: AutomationTarget) -> Option<u32> {
    match target {
        AutomationTarget::EffectParam { effect, .. }
        | AutomationTarget::EffectMix { effect, .. } => Some(effect.0),
        AutomationTarget::InstrumentParam { channel, .. } => Some(channel.0),
        _ => None,
    }
}

/// Reject zero-delay source dependencies, including indirect parameter loops.
pub fn links_problem(project: &Project, links: &[ControllerLink]) -> Option<String> {
    let count = project
        .mixer
        .tracks
        .iter()
        .flat_map(|t| &t.effects)
        .filter(|e| !outputs(e.params.kind()).is_empty())
        .count()
        + project
            .channels
            .iter()
            .filter(|c| {
                matches!(
                    c.source,
                    crate::ChannelSource::Instrument {
                        params: windfall_dsp::InstrumentParams::Dashboard(_),
                        ..
                    }
                )
            })
            .count();
    if count > MAX_INTERNAL_CONTROLLERS {
        return Some("project has more than 256 internal sources".into());
    }
    let mut edges = Vec::new();
    for link in links {
        let ControlSource::Internal { controller, output } = link.source else {
            continue;
        };
        let Some((_, effect)) = source(project, controller) else {
            return Some("internal link source is missing or has no control ports".into());
        };
        if usize::from(output) >= effect.ports().len() {
            return Some("internal link output does not exist".into());
        }
        if link.map.mode != crate::SourceMode::Absolute || link.map.pickup == crate::PickupMode::On
        {
            return Some("internal signals use absolute maps without pickup".into());
        }
        let LinkTarget::Param { target } = link.target else {
            return Some("internal sources require a project parameter target".into());
        };
        if target == AutomationTarget::Tempo {
            return Some("internal tempo modulation requires a clock feedback node".into());
        }
        if let Some(to) = owner(target) {
            edges.push((controller, to));
        }
        // A parameter edit can feed a detector through audio as well as through
        // another scalar source. Add both kinds of dependency to the same graph.
        for detector_track in &project.mixer.tracks {
            for detector in &detector_track.effects {
                if detector.params.kind() != EffectKind::EnvelopeFollower {
                    continue;
                }
                let affected = match target {
                    AutomationTarget::ChannelVolume { channel }
                    | AutomationTarget::ChannelPan { channel }
                    | AutomationTarget::InstrumentParam { channel, .. } => {
                        project.channel(channel).map(|c| c.mixer_track)
                    }
                    AutomationTarget::EffectParam {
                        track,
                        effect: destination,
                        ..
                    }
                    | AutomationTarget::EffectMix {
                        track,
                        effect: destination,
                    } => {
                        if track == detector_track.id {
                            let chain = &detector_track.effects;
                            let before = chain.iter().position(|e| e.id == destination);
                            let at = chain.iter().position(|e| e.id == detector.id);
                            if before.zip(at).is_some_and(|(before, at)| before < at) {
                                Some(track)
                            } else {
                                None
                            }
                        } else {
                            Some(track)
                        }
                    }
                    AutomationTarget::TrackVolume { track }
                    | AutomationTarget::TrackPan { track }
                    | AutomationTarget::TrackParam { track, .. } => {
                        if track == detector_track.id {
                            None
                        } else {
                            Some(track)
                        }
                    }
                    AutomationTarget::SendGain { target, .. } => Some(target),
                    // A changed detector send can alter a compressor ahead of
                    // the follower, so include that indirect audio dependency.
                    AutomationTarget::SidechainGain { target, .. } => Some(target),
                    AutomationTarget::Tempo => None,
                };
                if affected.is_some_and(|from| {
                    audio_reaches(project, from, detector_track.id, &mut Vec::new())
                }) {
                    edges.push((controller, detector.id.0));
                }
            }
        }
    }
    for &(from, to) in &edges {
        if reaches(&edges, to, from, &mut Vec::new()) {
            return Some("internal controller cycle".into());
        }
    }
    None
}

fn reaches(edges: &[(u32, u32)], from: u32, to: u32, visited: &mut Vec<u32>) -> bool {
    if from == to {
        return true;
    }
    if visited.contains(&from) {
        return false;
    }
    visited.push(from);
    edges
        .iter()
        .filter(|e| e.0 == from)
        .any(|e| reaches(edges, e.1, to, visited))
}
fn audio_reaches(
    project: &Project,
    from: crate::TrackId,
    to: crate::TrackId,
    visited: &mut Vec<crate::TrackId>,
) -> bool {
    if from == to || project.mixer.track(to).is_some_and(|track| track.current) {
        return true;
    }
    if visited.contains(&from) {
        return false;
    }
    visited.push(from);
    let Some(track) = project.mixer.track(from) else {
        return false;
    };
    track
        .output
        .is_some_and(|id| audio_reaches(project, id, to, visited))
        || track
            .sends
            .iter()
            .any(|s| audio_reaches(project, s.target, to, visited))
        || crate::effect_outputs::targets(&project.mixer, from)
            .any(|target| audio_reaches(project, target, to, visited))
}
