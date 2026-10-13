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

/// Everything needed to reopen an instance. Missing files retain this data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PluginBinding {
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
    /// Validates bounded saved data before it reaches native code.
    pub fn validate(&self) -> Result<(), &'static str> {
        self.wrapper.validate()?;
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
