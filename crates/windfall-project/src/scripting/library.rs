use super::{Program, SCRIPT_VERSION};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum ScriptKind {
    Piano,
    Controller,
    NoteTransform,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ScriptReference {
    pub id: String,
    pub revision: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ScriptAsset {
    pub reference: ScriptReference,
    pub language_version: u32,
    pub kind: ScriptKind,
    pub name: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ChannelScript {
    pub channel: crate::ChannelId,
    pub script: ScriptReference,
    pub parameters: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ControllerScript {
    pub source: crate::ControlSource,
    pub script: ScriptReference,
    pub parameters: Vec<f64>,
    /// A normalized script `map` output drives this link. Tempo and feedback
    /// statements need no target. Action targets use the existing rising-edge
    /// link runtime rather than this continuous mapping surface.
    pub target: Option<crate::LinkTarget>,
    /// Feedback uses the selected native MIDI output and this one-based channel.
    /// None disables external feedback, even when the script emits it.
    pub feedback_channel: Option<u8>,
    /// Applied after the script's normalized map/tempo output. Legacy bindings
    /// retain immediate takeover; new bindings can select pickup and smoothing.
    #[serde(default = "legacy_controller_map")]
    pub map: crate::ControlMap,
}

fn legacy_controller_map() -> crate::ControlMap {
    crate::ControlMap {
        pickup: crate::PickupMode::Off,
        ..Default::default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ScriptLibrary {
    /// InstrumentParam 0x80000000+n. Removed bindings leave tombstones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<ScriptParameterAddress>>", optional)]
    pub parameter_addresses: Vec<ScriptParameterAddress>,
    pub assets: Vec<ScriptAsset>,
    pub channels: Vec<ChannelScript>,
    pub controllers: Vec<ControllerScript>,
}

pub const SCRIPT_PARAMETER_BASE: u32 = 0x8000_0000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[ts(export)]
pub struct ScriptParameterAddress {
    pub channel: crate::ChannelId,
    pub script: String,
    pub name: String,
}

impl ScriptLibrary {
    /// Control-side append-only registration; clients cannot renumber saved lanes.
    pub fn register_parameters(&mut self, old: &Self) -> Result<(), String> {
        if !self.parameter_addresses.is_empty()
            && !self
                .parameter_addresses
                .starts_with(&old.parameter_addresses)
        {
            return Err("script parameter addresses are append-only".into());
        }
        let mut addresses = old.parameter_addresses.clone();
        for binding in &self.channels {
            let asset = self.asset(&binding.script).ok_or("missing script")?;
            for parameter in Program::compile(&asset.source)
                .map_err(|e| e.to_string())?
                .parameters
            {
                let address = ScriptParameterAddress {
                    channel: binding.channel,
                    script: binding.script.id.clone(),
                    name: parameter.name,
                };
                if !addresses.contains(&address) {
                    addresses.push(address);
                }
            }
        }
        if addresses.len() > 4096 {
            return Err("script parameter address capacity exceeded".into());
        }
        self.parameter_addresses = addresses;
        Ok(())
    }

    pub fn parameter(
        &self,
        channel: crate::ChannelId,
        address: u32,
    ) -> Option<(usize, super::ScriptParameter, f64)> {
        let entry = self
            .parameter_addresses
            .get(address.checked_sub(SCRIPT_PARAMETER_BASE)? as usize)?;
        if entry.channel != channel {
            return None;
        }
        let binding = self
            .channels
            .iter()
            .find(|b| b.channel == channel && b.script.id == entry.script)?;
        let program = Program::compile(&self.asset(&binding.script)?.source).ok()?;
        let index = program
            .parameters
            .iter()
            .position(|p| p.name == entry.name)?;
        let parameter = program.parameters[index].clone();
        let value = binding
            .parameters
            .get(index)
            .copied()
            .unwrap_or(parameter.default);
        Some((index, parameter, value))
    }

    pub fn parameter_state(
        &self,
        channel: crate::ChannelId,
        address: u32,
    ) -> Option<(crate::AutomationRange, f32)> {
        let (_, parameter, value) = self.parameter(channel, address)?;
        Some((
            crate::AutomationRange {
                min: parameter.min as f32,
                max: parameter.max as f32,
                taper: crate::AutomationTaper::Linear,
            },
            value as f32,
        ))
    }
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
            && self.channels.is_empty()
            && self.controllers.is_empty()
            && self.parameter_addresses.is_empty()
    }
    pub fn asset(&self, reference: &ScriptReference) -> Option<&ScriptAsset> {
        self.assets.iter().find(|a| a.reference == *reference)
    }

    pub fn check(&self) -> Result<(), String> {
        if self.parameter_addresses.len() > 4096
            || self
                .parameter_addresses
                .iter()
                .enumerate()
                .any(|(i, address)| self.parameter_addresses[..i].contains(address))
        {
            return Err("invalid script parameter address registry".into());
        }
        if self.assets.len() > 64 || self.channels.len() > 128 || self.controllers.len() > 64 {
            return Err("script library count limit exceeded".into());
        }
        let total: usize = self.assets.iter().map(|a| a.source.len()).sum();
        if total > 262_144 {
            return Err("script library source memory limit exceeded".into());
        }
        for (i, asset) in self.assets.iter().enumerate() {
            if asset.language_version != SCRIPT_VERSION {
                return Err("unsupported script language version".into());
            }
            if asset.reference.revision == 0
                || asset.reference.id.is_empty()
                || asset.reference.id.len() > 64
                || !asset
                    .reference
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || asset.name.trim().is_empty()
                || asset.name.len() > 128
            {
                return Err("invalid script identity/name".into());
            }
            if self.assets[..i]
                .iter()
                .any(|other| other.reference == asset.reference)
            {
                return Err("duplicate script revision".into());
            }
            let program = Program::compile(&asset.source).map_err(|e| e.to_string())?;
            if asset.kind == ScriptKind::NoteTransform {
                if program.parameters.iter().any(|p| {
                    !(p.min as f32).is_finite()
                        || !(p.max as f32).is_finite()
                        || !(p.default as f32).is_finite()
                }) {
                    return Err("note script parameter exceeds the automation numeric range".into());
                }
                program.note_transform(&[]).map_err(|e| e.to_string())?;
            }
        }
        for (i, binding) in self.channels.iter().enumerate() {
            if self.channels[..i]
                .iter()
                .any(|other| other.channel == binding.channel)
            {
                return Err("duplicate channel script".into());
            }
            let asset = self
                .asset(&binding.script)
                .filter(|a| a.kind == ScriptKind::NoteTransform)
                .ok_or("missing note-transform script revision")?;
            Program::compile(&asset.source)
                .map_err(|e| e.to_string())?
                .note_transform(&binding.parameters)
                .map_err(|e| e.to_string())?;
        }
        for (i, binding) in self.controllers.iter().enumerate() {
            if self.controllers[..i]
                .iter()
                .any(|other| other.source == binding.source)
            {
                return Err("duplicate controller script source".into());
            }
            if binding
                .feedback_channel
                .is_some_and(|c| !(1..=16).contains(&c))
            {
                return Err("invalid feedback channel".into());
            }
            if !matches!(binding.source, crate::ControlSource::Midi { .. }) {
                return Err("controller scripts require a MIDI source".into());
            }
            binding.map.compile()?;
            let link = crate::ControllerLink {
                source: binding.source.clone(),
                target: binding.target.unwrap_or(crate::LinkTarget::Slot {
                    slot: crate::SlotTarget::Tempo,
                }),
                map: binding.map.clone(),
            };
            if let Some(problem) = link.problem() {
                return Err(problem);
            }
            let asset = self
                .asset(&binding.script)
                .filter(|a| a.kind == ScriptKind::Controller)
                .ok_or("missing controller script revision")?;
            Program::compile(&asset.source)
                .map_err(|e| e.to_string())?
                .validate_parameters(&binding.parameters)
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn channel_is_dynamic(&self, project: &crate::Project, channel: crate::ChannelId) -> bool {
        let target = |target: crate::AutomationTarget| {
            matches!(target, crate::AutomationTarget::InstrumentParam { channel: owner, param }
            if owner == channel && param >= SCRIPT_PARAMETER_BASE)
        };
        project.automations.iter().any(|a| target(a.target))
            || project.patterns.iter().flat_map(|p| &p.events).any(|a| target(a.target))
            || project.settings.controller_links.iter().any(|link| matches!(link.target, crate::LinkTarget::Param { target: t } if target(t)))
            || self.controllers.iter().any(|binding| matches!(binding.target, Some(crate::LinkTarget::Param { target: t }) if target(t)))
    }

    pub fn channel_transform(&self, channel: crate::ChannelId) -> Option<super::NoteTransform> {
        let binding = self.channels.iter().find(|b| b.channel == channel)?;
        let asset = self.asset(&binding.script)?;
        Program::compile(&asset.source)
            .ok()?
            .note_transform(&binding.parameters)
            .ok()
    }

    /// Admission validates every stored event against its assigned bytecode.
    /// An invalid transform cannot silently delete notes in a checked project.
    pub fn check_notes(&self, project: &crate::Project) -> Result<(), String> {
        for binding in &self.controllers {
            if let Some(target) = binding.target {
                let link = crate::ControllerLink {
                    source: binding.source.clone(),
                    target,
                    map: binding.map.clone(),
                };
                if let Some(problem) =
                    crate::controller_links::links_problem(&[link], Some(project))
                {
                    return Err(problem);
                }
            }
        }
        for binding in &self.channels {
            let Some(transform) = self.channel_transform(binding.channel) else {
                continue;
            };
            for note in project
                .patterns
                .iter()
                .flat_map(|p| &p.lanes)
                .filter(|l| l.channel == binding.channel)
                .flat_map(|l| &l.notes)
            {
                if self.channel_is_dynamic(project, binding.channel) {
                    transform
                        .maximum_end(super::ScriptNote::from(note))
                        .map_err(|e| {
                            windfall_core::notice!("scripts.timeBounds", error = format!("{:?}", e))
                                .to_string()
                        })?;
                }
                transform
                    .evaluate_notes(super::ScriptNote::from(note))
                    .map_err(|e| {
                        windfall_core::notice!(
                            "scripts.noteFailed",
                            script = binding.script.id,
                            revision = binding.script.revision,
                            note = note.id.0,
                            error = format!("{:?}", e)
                        )
                        .to_string()
                    })?;
            }
        }
        // Expanded playback is checked just like authored clip placement.
        for pattern in &project.patterns {
            let source_length = pattern
                .length_steps
                .max(1)
                .saturating_mul(crate::TICKS_PER_STEP);
            let mut length = source_length;
            let mut notes = Vec::new();
            for lane in &pattern.lanes {
                let Some(transform) = self.channel_transform(lane.channel) else {
                    continue;
                };
                let Some(channel) = project.channel(lane.channel) else {
                    continue;
                };
                for note in lane.notes.iter().filter(|note| note.start < source_length) {
                    if self.channel_is_dynamic(project, lane.channel) {
                        let end = transform
                            .maximum_end(super::ScriptNote::from(note))
                            .map_err(|e| {
                                windfall_core::notice!(
                                    "scripts.timeBounds",
                                    error = format!("{:?}", e)
                                )
                                .to_string()
                            })?;
                        length = length.max(end);
                        notes.push((
                            channel.timing,
                            super::ScriptNote {
                                length: end - note.start,
                                ..super::ScriptNote::from(note)
                            },
                        ));
                    }
                    for note in transform
                        .evaluate_notes(super::ScriptNote::from(note))
                        .map_err(|e| {
                            windfall_core::notice!("scripts.noteBounds", error = format!("{:?}", e))
                                .to_string()
                        })?
                        .as_slice()
                    {
                        length = length.max(note.start.saturating_add(note.length));
                        notes.push((channel.timing, *note));
                    }
                }
            }
            if notes.is_empty() {
                continue;
            }
            for clip in project.playlist.clips.iter().filter(|clip| {
                clip.content
                    == (crate::ClipContent::Pattern {
                        pattern: pattern.id,
                    })
            }) {
                if clip.length == 0 {
                    return Err("scripted clip must have a nonempty window".into());
                }
                let extension = clip
                    .length
                    .div_ceil(source_length)
                    .checked_mul(length.saturating_sub(source_length))
                    .ok_or("scripted clip extent exceeds song bound")?;
                let end = clip
                    .start
                    .checked_add(clip.length)
                    .and_then(|end| end.checked_add(extension))
                    .filter(|end| *end <= crate::MAX_SONG_TICKS)
                    .ok_or("scripted clip extent exceeds song bound")?;
                let offset = clip.offset % length;
                let origin = f64::from(clip.start) - f64::from(offset);
                let repeat =
                    (u64::from(end - clip.start - 1) + u64::from(offset)) / u64::from(length);
                for repeat in [repeat.saturating_sub(1), repeat] {
                    let shift = origin + repeat as f64 * f64::from(length);
                    for (timing, note) in &notes {
                        if let Some((tick, duration)) = timing.place(
                            note.start,
                            note.length,
                            length,
                            f64::from(project.settings.swing),
                        ) {
                            let onset = shift + tick;
                            if onset >= f64::from(clip.start)
                                && onset < f64::from(end)
                                && (onset + duration).ceil() > f64::from(crate::MAX_SONG_TICKS)
                            {
                                return Err("scripted note tail exceeds song bound".into());
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
