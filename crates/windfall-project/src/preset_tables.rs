//! Project-owned preset configuration and its portable object namespace.
//! External references require a unique semantic match; numeric ID coincidence
//! never establishes ownership in a different project. Refusal is atomic and
//! its reason is displayed by the ordinary command error UI.
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PresetObject {
    pub id: u32,
    pub role: String,
    pub name: String,
    pub processor: String,
    pub plugin: Option<PresetPluginIdentity>,
    #[serde(default)]
    pub parameter_ids: Vec<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PresetTables {
    pub owners: Vec<u32>,
    pub objects: Vec<PresetObject>,
    pub controllers: Vec<InternalControllerSettings>,
    pub links: Vec<ControllerLink>,
    pub scripts: scripting::ScriptLibrary,
}

fn objects(project: &Project) -> Vec<PresetObject> {
    let identity = |target| {
        project.plugin(target).map(|p| PresetPluginIdentity {
            format: p.format.clone(),
            id: p.id.clone(),
            instrument: matches!(target, PluginTarget::Instrument { .. }),
        })
    };
    let mut out = Vec::new();
    for channel in &project.channels {
        out.push(PresetObject {
            id: channel.id.0,
            role: "channel".into(),
            name: channel.name.clone(),
            processor: match &channel.source {
                ChannelSource::Sampler(_) => "sampler".into(),
                ChannelSource::Instrument { params, .. } => format!("{:?}", params.kind()),
            },
            plugin: identity(PluginTarget::Instrument {
                channel: channel.id,
            }),
            parameter_ids: project
                .plugin(PluginTarget::Instrument {
                    channel: channel.id,
                })
                .map(|p| p.parameters.iter().map(|v| v.id).collect())
                .unwrap_or_default(),
        });
    }
    for track in &project.mixer.tracks {
        out.push(PresetObject {
            id: track.id.0,
            role: "track".into(),
            name: track.name.clone(),
            processor: String::new(),
            plugin: None,
            parameter_ids: Vec::new(),
        });
        for effect in &track.effects {
            out.push(PresetObject {
                id: effect.id.0,
                role: "effect".into(),
                name: track.name.clone(),
                processor: format!("{:?}", effect.kind()),
                plugin: identity(PluginTarget::Effect { effect: effect.id }),
                parameter_ids: project
                    .plugin(PluginTarget::Effect { effect: effect.id })
                    .map(|p| p.parameters.iter().map(|v| v.id).collect())
                    .unwrap_or_default(),
            });
        }
    }
    out
}

fn target_ids(target: LinkTarget) -> Vec<u32> {
    use AutomationTarget::*;
    match target {
        LinkTarget::Param { target } => match target {
            ChannelVolume { channel }
            | ChannelPan { channel }
            | InstrumentParam { channel, .. } => vec![channel.0],
            TrackVolume { track } | TrackPan { track } | TrackParam { track, .. } => vec![track.0],
            SendGain { track, target } | SidechainGain { track, target } => vec![track.0, target.0],
            EffectParam { track, effect, .. } | EffectMix { track, effect } => {
                vec![track.0, effect.0]
            }
            Tempo => vec![],
        },
        _ => vec![],
    }
}
fn source_id(source: &ControlSource) -> Option<u32> {
    if let ControlSource::Internal { controller, .. } = source {
        Some(*controller)
    } else {
        None
    }
}
fn concrete(project: &Project, target: LinkTarget) -> LinkTarget {
    if let LinkTarget::Slot { slot } = target
        && let Some(target) = slot.resolve(project, 0)
    {
        LinkTarget::Param { target }
    } else {
        target
    }
}

impl PresetTables {
    pub fn has_configuration(&self) -> bool {
        !self.controllers.is_empty()
            || !self.links.is_empty()
            || !self.scripts.is_empty()
            || self
                .objects
                .iter()
                .any(|object| !self.owners.contains(&object.id))
    }
    pub fn capture(project: &Project, owners: Vec<u32>) -> Self {
        let owned = |id: &u32| owners.contains(id);
        let links = project
            .settings
            .controller_links
            .iter()
            .cloned()
            .map(|mut link| {
                link.target = concrete(project, link.target);
                link
            })
            .filter(|link| {
                source_id(&link.source).is_some_and(|id| owned(&id))
                    || target_ids(link.target).iter().any(owned)
            })
            .collect();
        let mut scripts = scripting::ScriptLibrary::default();
        scripts.channels = project
            .settings
            .scripts
            .channels
            .iter()
            .filter(|b| owned(&b.channel.0))
            .cloned()
            .collect();
        scripts.controllers = project
            .settings
            .scripts
            .controllers
            .iter()
            .cloned()
            .map(|mut b| {
                b.target = b.target.map(|target| concrete(project, target));
                b
            })
            .filter(|b| b.target.is_some_and(|t| target_ids(t).iter().any(owned)))
            .collect();
        scripts.assets = project
            .settings
            .scripts
            .assets
            .iter()
            .filter(|asset| {
                scripts.channels.iter().any(|b| b.script == asset.reference)
                    || scripts
                        .controllers
                        .iter()
                        .any(|b| b.script == asset.reference)
            })
            .cloned()
            .collect();
        let controllers = project
            .settings
            .internal_controllers
            .iter()
            .filter(|c| owned(&c.controller))
            .cloned()
            .collect();
        let mut tables = Self {
            owners,
            objects: objects(project),
            links,
            scripts,
            controllers,
        };
        // Keep only descriptors actually referenced, plus the owners themselves.
        let mut used: HashSet<u32> = tables.owners.iter().copied().collect();
        for link in &tables.links {
            used.extend(source_id(&link.source));
            used.extend(target_ids(link.target));
        }
        for c in &tables.controllers {
            used.insert(c.controller);
            used.extend(c.note_channel.map(|id| id.0));
        }
        for b in &tables.scripts.controllers {
            if let Some(t) = b.target {
                used.extend(target_ids(t));
            }
        }
        // MIDI destinations may be referenced by the channel/rack body.
        // Their descriptors are added by include_midi below.
        tables.objects.retain(|object| used.contains(&object.id));
        tables
    }

    pub fn include_midi(&mut self, project: &Project, params: &InstrumentParams) {
        if let InstrumentParams::MidiOut(p) | InstrumentParams::Dashboard(p) = params
            && p.plugin_destination > 0
        {
            let id = p.plugin_destination as u32;
            if !self.objects.iter().any(|o| o.id == id)
                && let Some(object) = objects(project).into_iter().find(|o| o.id == id)
            {
                self.objects.push(object);
            }
        }
    }

    pub fn include_rack(&mut self, project: &Project, graph: &RackGraph) {
        for node in &graph.nodes {
            match &node.processor {
                RackProcessor::Instrument { params } => self.include_midi(project, params),
                RackProcessor::Rack { graph } => self.include_rack(project, graph),
                _ => {}
            }
        }
    }

    pub fn check(&self) -> Result<(), String> {
        if self.objects.len() > 1024
            || self.owners.len() > 129
            || self.controllers.len() > internal_controllers::MAX_INTERNAL_CONTROLLERS
            || self.links.len() > controller_links::MAX_CONTROLLER_LINKS
        {
            return Err("Preset side tables exceed their limits".into());
        }
        let mut ids = HashSet::new();
        for object in &self.objects {
            if !ids.insert(object.id)
                || !matches!(object.role.as_str(), "channel" | "track" | "effect")
                || object.name.len() > 4096
                || object.processor.len() > 128
                || (object.id == 0 && object.role != "track")
                || object.parameter_ids.len() > 65536
            {
                return Err("Invalid portable preset object descriptor".into());
            }
        }
        if self.owners.iter().any(|id| !ids.contains(id)) {
            return Err("Preset owner has no descriptor".into());
        }
        let owners: HashSet<_> = self.owners.iter().copied().collect();
        if owners.len() != self.owners.len()
            || self
                .controllers
                .iter()
                .any(|c| !owners.contains(&c.controller))
            || self
                .scripts
                .channels
                .iter()
                .any(|c| !owners.contains(&c.channel.0))
        {
            return Err("Preset side table belongs to an unrelated owner".into());
        }
        self.scripts.check()?;
        for (index, link) in self.links.iter().enumerate() {
            if let Some(problem) = link.problem() {
                return Err(problem);
            }
            if self.links[..index]
                .iter()
                .any(|old| old.source == link.source && old.target == link.target)
            {
                return Err("Duplicate preset controller link".into());
            }
        }
        Ok(())
    }

    /// Resolve every object that is actually consumed; unresolvable descriptors
    /// can stay in a file, but applying one never silently deletes its settings.
    pub fn resolve(
        &self,
        project: &Project,
        local: &HashMap<u32, u32>,
        id: u32,
    ) -> Result<u32, String> {
        if let Some(id) = local.get(&id) {
            return Ok(*id);
        }
        if id == TrackId::MASTER.0 {
            return Ok(id);
        }
        let saved = self.objects.iter().find(|o| o.id == id).ok_or_else(|| {
            windfall_core::notice!("presets.remapNoIdentity", id = id).to_string()
        })?;
        let candidates: Vec<_> = objects(project)
            .into_iter()
            .filter(|o| {
                o.role == saved.role
                    && o.name == saved.name
                    && o.processor == saved.processor
                    && o.plugin == saved.plugin
                    && o.parameter_ids == saved.parameter_ids
            })
            .collect();
        if candidates.len() != 1 {
            return Err(windfall_core::notice!(
                "presets.remapAmbiguous",
                role = windfall_core::notice::term(&saved.role),
                name = saved.name,
                count = candidates.len()
            )
            .to_string());
        }
        Ok(candidates[0].id)
    }

    pub fn remap_midi(
        &self,
        project: &Project,
        local: &HashMap<u32, u32>,
        params: &mut InstrumentParams,
    ) -> Result<(), String> {
        if let InstrumentParams::MidiOut(p) | InstrumentParams::Dashboard(p) = params
            && p.plugin_destination > 0
        {
            let old = p.plugin_destination as u32;
            let saved = self.objects.iter().find(|o| o.id == old).ok_or("Cannot remap hosted MIDI destination: this older preset has no target identity; the preset was not applied")?;
            let id = self.resolve(project, local, old)?;
            let binding = project.plugin(PluginTarget::Instrument { channel: ChannelId(id) })
                .ok_or("Cannot remap MIDI destination: the destination has no hosted instrument; the preset was not applied")?;
            if saved
                .plugin
                .as_ref()
                .is_none_or(|p| p.format != binding.format || p.id != binding.id || !p.instrument)
            {
                return Err("Cannot remap MIDI destination: hosted plugin identity differs; the preset was not applied".into());
            }
            p.plugin_destination =
                i32::try_from(id).map_err(|_| "MIDI destination ID exceeds its range")?;
        }
        Ok(())
    }
    pub fn check_single_owner(&self, role: &str) -> Result<(), String> {
        self.check()?;
        if self.owners.len() != 1
            || self
                .objects
                .iter()
                .find(|o| o.id == self.owners[0])
                .is_none_or(|o| o.role != role)
        {
            return Err(windfall_core::notice!(
                "presets.ownerCount",
                role = windfall_core::notice::term(role)
            )
            .to_string());
        }
        Ok(())
    }
    pub fn remap_rack(
        &self,
        project: &Project,
        local: &HashMap<u32, u32>,
        graph: &mut RackGraph,
    ) -> Result<(), String> {
        for node in &mut graph.nodes {
            match &mut node.processor {
                RackProcessor::Instrument { params } => self.remap_midi(project, local, params)?,
                RackProcessor::Rack { graph } => self.remap_rack(project, local, graph)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn source(
        &self,
        project: &Project,
        local: &HashMap<u32, u32>,
        source: &mut ControlSource,
    ) -> Result<(), String> {
        if let ControlSource::Internal { controller, .. } = source {
            *controller = self.resolve(project, local, *controller)?;
        }
        Ok(())
    }
    fn target(
        &self,
        project: &Project,
        local: &HashMap<u32, u32>,
        target: &mut LinkTarget,
    ) -> Result<(), String> {
        use AutomationTarget::*;
        let map = |id| self.resolve(project, local, id);
        if let LinkTarget::Param { target } = target {
            match target {
                ChannelVolume { channel }
                | ChannelPan { channel }
                | InstrumentParam { channel, .. } => channel.0 = map(channel.0)?,
                TrackVolume { track } | TrackPan { track } | TrackParam { track, .. } => {
                    track.0 = map(track.0)?
                }
                SendGain { track, target } | SidechainGain { track, target } => {
                    track.0 = map(track.0)?;
                    target.0 = map(target.0)?;
                }
                EffectParam { track, effect, .. } | EffectMix { track, effect } => {
                    effect.0 = map(effect.0)?;
                    *track = project
                        .mixer
                        .tracks
                        .iter()
                        .find(|t| t.effects.iter().any(|e| e.id == *effect))
                        .ok_or("Cannot remap preset effect target: its mixer owner is unavailable")?
                        .id;
                }
                Tempo => {}
            }
            if project.automation_range(target).is_none() {
                return Err("Cannot remap preset controller target: parameter or route is unavailable; the preset was not applied".into());
            }
        }
        Ok(())
    }

    pub fn apply(
        &self,
        project: &Project,
        local: &HashMap<u32, u32>,
        retired: &[u32],
    ) -> Result<ProjectSettings, String> {
        self.check()?;
        let owned = |id: u32| retired.contains(&id) || local.values().any(|new| *new == id);
        let mut settings = project.settings.clone();
        settings
            .internal_controllers
            .retain(|c| !owned(c.controller));
        settings.controller_links.retain(|l| {
            !source_id(&l.source).is_some_and(owned)
                && !target_ids(concrete(project, l.target))
                    .into_iter()
                    .any(owned)
        });
        settings.scripts.channels.retain(|b| !owned(b.channel.0));
        settings.scripts.controllers.retain(|b| {
            !b.target
                .is_some_and(|t| target_ids(concrete(project, t)).into_iter().any(owned))
        });
        for mut c in self.controllers.clone() {
            c.controller = self.resolve(project, local, c.controller)?;
            if let Some(channel) = &mut c.note_channel {
                channel.0 = self.resolve(project, local, channel.0)?;
            }
            settings.internal_controllers.push(c);
        }
        for mut link in self.links.clone() {
            self.source(project, local, &mut link.source)?;
            self.target(project, local, &mut link.target)?;
            settings.controller_links.push(link);
        }
        // Script revisions are immutable. Refuse a collision instead of replacing
        // a different program used elsewhere in the destination project.
        for asset in &self.scripts.assets {
            if let Some(old) = settings.scripts.asset(&asset.reference) {
                if old != asset {
                    return Err(windfall_core::notice!(
                        "presets.scriptConflict",
                        script = asset.reference.id,
                        revision = asset.reference.revision
                    )
                    .to_string());
                }
            } else {
                settings.scripts.assets.push(asset.clone());
            }
        }
        for mut b in self.scripts.channels.clone() {
            b.channel.0 = self.resolve(project, local, b.channel.0)?;
            settings.scripts.channels.push(b);
        }
        for mut b in self.scripts.controllers.clone() {
            if let Some(target) = &mut b.target {
                self.target(project, local, target)?;
            }
            if settings
                .scripts
                .controllers
                .iter()
                .any(|old| old.source == b.source)
            {
                return Err("Cannot import controller script: its MIDI source is already assigned; the preset was not applied".into());
            }
            settings.scripts.controllers.push(b);
        }
        // Validate before the ordinary post-command cleanup can normalize any
        // invalid imported configuration away. Refusals retain every saved byte.
        let mut proposed = project.clone();
        proposed.settings = settings.clone();
        if let Some(problem) =
            internal_controllers::settings_problem(&proposed, &settings.internal_controllers)
                .or_else(|| {
                    controller_links::links_problem(&settings.controller_links, Some(&proposed))
                })
        {
            return Err(
                windfall_core::notice!("presets.controllersRefused", problem = problem).to_string(),
            );
        }
        settings.scripts.check()?;
        settings.scripts.check_notes(&proposed)?;
        Ok(settings)
    }
}
