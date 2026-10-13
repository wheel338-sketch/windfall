//! Saved references to hosted plugins, independent of native hosting code.

use crate::{ChannelId, EffectId};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[path = "plugin_wrapper.rs"]
mod wrapper;
pub use wrapper::{PluginProcessingMode, PluginWrapperOptions};

/// The project object a plugin replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum PluginTarget {
    Instrument { channel: ChannelId },
    Effect { effect: EffectId },
    /// A graph-local owner, with zero padding after the nested node path.
    Rack { channel: ChannelId, path: [u32; 4], instrument: bool },
}
impl PluginTarget {
    pub fn is_instrument(self) -> bool { matches!(self, Self::Instrument { .. } | Self::Rack { instrument: true, .. }) }
}

/// A discovered parameter in native units. Its id survives list reordering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginParameter {
    pub id: u32,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub value: f32,
    pub stepped: bool,
    pub read_only: bool,
    pub automatable: bool,
}

/// A discovered auxiliary audio input in the native input list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginAuxInput {
    pub index: u32,
    pub name: String,
    pub channels: u32,
}

/// Saved output routing names a native bus, independent of parameter addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginOutputRoute {
    pub output: u32,
    pub track: crate::TrackId,
}

/// Append-only parameter addresses and the last admitted native output layout.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct PluginCapabilities {
    /// Each accepted snapshot prepares a new native owner even if opaque state
    /// bytes are unchanged (latency/note flags need not be saved by the plugin).
    #[ts(as = "Option<u32>", optional)]
    pub revision: u32,
    pub unavailable_parameters: Vec<u32>,
    pub outputs: Vec<PluginAuxInput>,
    pub routes: Vec<PluginOutputRoute>,
}
impl PluginCapabilities {
    pub fn is_default(&self) -> bool { *self == Self::default() }
}

/// Everything needed to reopen an instance. Missing files retain this data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginBinding {
    #[serde(default, skip_serializing_if = "PluginCapabilities::is_default")]
    #[ts(as = "Option<PluginCapabilities>", optional)]
    pub capabilities: PluginCapabilities,
    #[serde(default, skip_serializing_if = "PluginWrapperOptions::is_default")]
    #[ts(as = "Option<PluginWrapperOptions>", optional)]
    pub wrapper: PluginWrapperOptions,
    pub target: PluginTarget,
    /// "clap", "vst3" or "au"; the model does not load native formats.
    pub format: String,
    pub path: String,
    pub id: String,
    pub name: String,
    /// Host-wrapped opaque state, including its version header.
    #[serde(default)]
    pub state: Vec<u8>,
    #[serde(default)]
    pub parameters: Vec<PluginParameter>,
    /// None selects the first auxiliary input. The index is in the native input list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sidechain_input: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<PluginAuxInput>>", optional)]
    pub auxiliary_inputs: Vec<PluginAuxInput>,
}

impl PluginBinding {
    /// Re-admission preserves every saved address, including missing parameters.
    /// Native order never changes an existing automation/controller index.
    pub fn admit_parameters(&mut self, parameters: Vec<PluginParameter>) {
        self.capabilities.unavailable_parameters.clear();
        for saved in &mut self.parameters {
            if let Some(current) = parameters.iter().find(|param| param.id == saved.id) {
                *saved = current.clone();
            } else { self.capabilities.unavailable_parameters.push(saved.id); }
        }
        for current in parameters {
            if !self.parameters.iter().any(|param| param.id == current.id) { self.parameters.push(current); }
        }
    }
    pub fn parameter_available(&self, id: u32) -> bool {
        !self.capabilities.unavailable_parameters.contains(&id)
    }
    /// Diagnostics are derived from retained data so reopening shows the same
    /// stale addresses/routes and returning capabilities automatically revive them.
    pub fn capability_diagnostics(&self) -> Vec<String> {
        let mut diagnostics = self.capabilities.unavailable_parameters.iter().map(|id| {
            format!("Parameter {id} is unavailable; its automation and controller links are retained.")
        }).collect::<Vec<_>>();
        diagnostics.extend(self.capabilities.routes.iter().filter(|route| {
            !self.capabilities.outputs.iter().any(|port| port.index == route.output)
        }).map(|route| format!("Saved output {} to mixer track {} is unavailable.", route.output + 1, route.track.0)));
        diagnostics
    }
    /// Validates bounded saved data before it reaches native code.
    pub fn validate(&self) -> Result<(), &'static str> {
        self.wrapper.validate()?;
        if self.capabilities.outputs.len() > 64 || self.capabilities.routes.len() > 64
            || self.capabilities.unavailable_parameters.len() > self.parameters.len()
            || self.capabilities.outputs.iter().enumerate().any(|(at, port)| {
                port.index >= 64 || port.channels == 0 || port.channels > 64
                    || self.capabilities.outputs[..at].iter().any(|other| other.index == port.index)
            })
            || self.capabilities.routes.iter().enumerate().any(|(at, route)| {
                route.output >= 64 || self.capabilities.routes[..at].iter().any(|other| other.output == route.output)
            })
            || self.capabilities.unavailable_parameters.iter().enumerate().any(|(at, id)| {
                !self.parameters.iter().any(|param| param.id == *id)
                    || self.capabilities.unavailable_parameters[..at].contains(id)
            }) { return Err("invalid plugin capability snapshot"); }
        if self.sidechain_input.is_some_and(|index| index >= 64)
            || self.auxiliary_inputs.len() > 64
            || self
                .auxiliary_inputs
                .iter()
                .any(|input| input.index >= 64 || input.channels == 0 || input.channels > 64)
            || self.auxiliary_inputs.iter().enumerate().any(|(at, input)| {
                self.auxiliary_inputs[..at]
                    .iter()
                    .any(|other| input.index == other.index)
            })
        {
            return Err("the plugin auxiliary input list or selection is invalid");
        }
        if !["clap", "vst3", "au"].contains(&self.format.as_str())
            || self.id.is_empty()
            || self.path.is_empty()
        {
            return Err("a plugin needs its format, file and identifier");
        }
        if self.state.len() > 256 * 1024 * 1024 || self.parameters.len() > 4096 {
            return Err("the saved plugin state or parameter list is too large");
        }
        let mut ids = std::collections::HashSet::new();
        for param in &self.parameters {
            if !ids.insert(param.id)
                || !param.min.is_finite()
                || !param.max.is_finite()
                || !param.value.is_finite()
                || param.min > param.max
                || !(param.min..=param.max).contains(&param.value)
            {
                return Err(
                    "a plugin parameter has an invalid range, value or repeated identifier",
                );
            }
        }
        Ok(())
    }
}

impl crate::Project {
    /// The saved plugin that replaces this instrument or effect.
    pub fn plugin(&self, target: PluginTarget) -> Option<&PluginBinding> {
        if let PluginTarget::Rack { channel, path, .. } = target {
            let count = path.iter().take_while(|id| **id != 0).count();
            return self.settings.modular_racks.iter().find(|r| r.channel == channel)?.graph.node_at(&path[..count])?.hosted.as_ref().filter(|p| p.target == target);
        }
        self.plugins.iter().find(|plugin| plugin.target == target)
    }
    /// Control-side inventory shared by hosting, capture and file review.
    pub fn plugin_bindings(&self) -> Vec<&PluginBinding> {
        fn add<'a>(graph: &'a crate::RackGraph, out: &mut Vec<&'a PluginBinding>) {
            for node in &graph.nodes { if let Some(binding) = &node.hosted { out.push(binding); } if let crate::RackProcessor::Rack { graph } = &node.processor { add(graph, out); } }
        }
        let mut out: Vec<_> = self.plugins.iter().collect();
        for rack in &self.settings.modular_racks { add(&rack.graph, &mut out); }
        out
    }
    pub fn plugin_bindings_mut(&mut self) -> Vec<&mut PluginBinding> {
        fn add<'a>(graph: &'a mut crate::RackGraph, out: &mut Vec<&'a mut PluginBinding>) {
            for node in &mut graph.nodes { if let Some(binding) = &mut node.hosted { out.push(binding); } if let crate::RackProcessor::Rack { graph } = &mut node.processor { add(graph, out); } }
        }
        let mut out: Vec<_> = self.plugins.iter_mut().collect();
        for rack in &mut self.settings.modular_racks { add(&mut rack.graph, &mut out); }
        out
    }
}

impl crate::Project {
    pub(crate) fn check_plugin_routing(&self) -> Result<(), String> {
        let mut edges = std::collections::HashMap::<crate::TrackId, Vec<crate::TrackId>>::new();
        for track in &self.mixer.tracks {
            edges.entry(track.id).or_default().extend(track.output.into_iter().chain(track.sends.iter().map(|send| send.target)).chain(track.sidechains.iter().map(|send| send.target)));
        }
        for route in &self.mixer.effect_outputs {
            if let Some(track) = self.mixer.tracks.iter().find(|track| track.effect(route.effect).is_some()) { edges.entry(track.id).or_default().push(route.target); }
        }
        for binding in &self.plugins {
            let PluginTarget::Effect { effect } = binding.target else { continue; };
            let Some(source) = self.mixer.tracks.iter().find(|track| track.effect(effect).is_some()) else { continue; };
            for route in &binding.capabilities.routes {
                // Retained stale routes cannot create signal edges. They remain
                // visible and will be checked again if the native bus returns.
                if !binding.capabilities.outputs.iter().any(|port| port.index == route.output)
                    || self.mixer.track(route.track).is_none() { continue; }
                if source.id == crate::TrackId::MASTER || source.current || self.mixer.track(route.track).is_some_and(|track| track.current) {
                    return Err("native output cannot feed from Master/Current or into Current".into());
                }
                edges.entry(source.id).or_default().push(route.track);
            }
        }
        for source in edges.keys() {
            let mut pending = edges.get(source).cloned().unwrap_or_default();
            let mut seen = std::collections::HashSet::new();
            while let Some(next) = pending.pop() {
                if next == *source { return Err("native output routing would create a mixer feedback loop".into()); }
                if seen.insert(next) { pending.extend(edges.get(&next).into_iter().flatten().copied()); }
            }
        }
        Ok(())
    }
}
