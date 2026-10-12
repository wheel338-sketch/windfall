//! A channel's saved modular processor graph. All edits replace a checked
//! graph atomically; local IDs never depend on vector order or UI position.
use crate::{ChannelId, EffectParams, InstrumentParams, Project};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
pub use windfall_dsp::rack_note::RackNoteModule;

pub const MAX_RACK_NODES: usize = 64;
pub const MAX_RACK_EDGES: usize = 256;
pub const MAX_RACK_DEPTH: usize = 4;
pub const MAX_RACK_EVENTS: usize = 256;
pub const AUDIO_IN: u16 = 0;
pub const AUDIO_OUT: u16 = 1;
pub const NOTE_IN: u16 = 2;
pub const NOTE_OUT: u16 = 3;
pub const CONTROL_IN: u16 = 4;
pub const CONTROL_OUT: u16 = 5;
pub const NOTE_CHANNEL_BASE: u16 = 16;
/// Append-only auxiliary audio namespace, matching EffectKind::aux_outputs.
pub const AUX_AUDIO_BASE: u16 = 32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ChannelRack {
    pub channel: ChannelId,
    pub graph: RackGraph,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", default)]
#[ts(export)]
pub struct RackGraph {
    /// Monotonic namespace, including IDs of removed nodes. Undo retains IDs.
    pub next_id: u32,
    pub nodes: Vec<RackNode>,
    pub edges: Vec<RackEdge>,
    /// Root-only append-only registry. InstrumentParam indices 0..31 remain
    /// macros; index 32+n addresses entry n. Removed owners leave tombstones.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<RackParameterAddress>>", optional)]
    pub parameters: Vec<RackParameterAddress>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RackParameterAddress {
    pub path: Vec<u32>,
    /// Prevents a replaced module from inheriting a different kind's lanes.
    pub kind: String,
    pub parameter: u32,
}
pub const MAX_RACK_PARAMETERS: usize = 16_384;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RackNode {
    pub id: u32,
    pub name: String,
    pub position: [f32; 2],
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
    pub processor: RackProcessor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum RackProcessor {
    AudioInput,
    AudioOutput,
    NoteInput,
    NoteOutput,
    /// External control inlet, not a control-source processor.
    ControlInput {
        value: f32,
        index: u8,
    },
    Instrument {
        params: InstrumentParams,
    },
    Effect {
        params: EffectParams,
        mix: f32,
    },
    Note {
        module: RackNoteModule,
    },
    Rack {
        graph: Box<RackGraph>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RackEndpoint {
    pub node: u32,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RackEdge {
    pub from: RackEndpoint,
    pub to: RackEndpoint,
    pub gain: f32,
    /// Stable descriptor ID on a control destination (never a UI index).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub parameter: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RackSignal {
    Audio,
    Note,
    Control,
}

impl RackProcessor {
    pub fn port(&self, port: u16, output: bool) -> Option<RackSignal> {
        use RackProcessor::*;
        match (self, port, output) {
            (Effect { params, .. }, port, true) if port >= AUX_AUDIO_BASE
                && usize::from(port - AUX_AUDIO_BASE) < params.kind().aux_outputs().len() => Some(RackSignal::Audio),
            (AudioInput | Instrument { .. } | Effect { .. } | Rack { .. }, AUDIO_OUT, true)
            | (AudioOutput | Effect { .. } | Rack { .. }, AUDIO_IN, false) => {
                Some(RackSignal::Audio)
            }
            (NoteInput | Note { .. } | Rack { .. }, NOTE_OUT, true)
            | (NoteOutput | Instrument { .. } | Note { .. } | Rack { .. }, NOTE_IN, false) => {
                Some(RackSignal::Note)
            }
            (Note { .. }, port, true)
                if (NOTE_CHANNEL_BASE..NOTE_CHANNEL_BASE + 16).contains(&port) =>
            {
                Some(RackSignal::Note)
            }
            (ControlInput { .. }, CONTROL_OUT, true)
            | (Instrument { .. } | Effect { .. }, CONTROL_IN, false) => Some(RackSignal::Control),
            _ => None,
        }
    }
    pub fn parameter_index(&self, id: &str) -> Option<usize> {
        match self {
            Self::Instrument { params } => params
                .kind()
                .descriptors()
                .iter()
                .position(|info| info.id == id),
            Self::Effect { params, .. } => params
                .kind()
                .descriptors()
                .iter()
                .position(|info| info.id == id),
            _ => None,
        }
    }
}

impl RackGraph {
    pub fn node_at(&self, path: &[u32]) -> Option<&RackNode> {
        let (id, rest) = path.split_first()?;
        let node = self.node(*id)?;
        if rest.is_empty() { return Some(node); }
        let RackProcessor::Rack { graph } = &node.processor else { return None; };
        graph.node_at(rest)
    }
    fn node_at_mut(&mut self, path: &[u32]) -> Option<&mut RackNode> {
        let (id, rest) = path.split_first()?;
        let node = self.nodes.iter_mut().find(|node| node.id == *id)?;
        if rest.is_empty() { return Some(node); }
        let RackProcessor::Rack { graph } = &mut node.processor else { return None; };
        graph.node_at_mut(rest)
    }
    pub fn parameter_state(&self, index: usize) -> Option<(crate::AutomationRange, f32)> {
        if index < 32 { return Some((crate::AutomationRange::MIX, self.control(index)?)); }
        let address = self.parameters.get(index - 32)?;
        let RackProcessor::Note { module } = &self.node_at(&address.path)?.processor else { return None; };
        if address.kind != module.kind_id() { return None; }
        Some((crate::AutomationRange::of_param(&module.parameter_info(address.parameter as usize)?), module.parameter(address.parameter as usize)?))
    }
    pub fn set_parameter(&mut self, index: usize, value: f32) -> bool {
        let Some(address) = index.checked_sub(32).and_then(|i| self.parameters.get(i)).cloned() else { return false; };
        let Some(node) = self.node_at_mut(&address.path) else { return false; };
        let RackProcessor::Note { module } = &mut node.processor else { return false; };
        address.kind == module.kind_id() && module.set_parameter(address.parameter as usize, value)
    }
    /// Preserve every historical address before discovering new note controls.
    /// Node IDs are monotonic at every nested level, even after removal.
    pub fn retain_parameter_addresses(&mut self, previous: Option<&Self>) -> Result<(), String> {
        fn retain(next: &mut RackGraph, old: &RackGraph) {
            next.next_id = next.next_id.max(old.next_id);
            for node in &mut next.nodes {
                if let RackProcessor::Rack { graph } = &mut node.processor
                    && let Some(RackNode { processor: RackProcessor::Rack { graph: before }, .. }) = old.node(node.id)
                { retain(graph, before); }
            }
        }
        if let Some(previous) = previous {
            retain(self, previous);
            self.parameters = previous.parameters.clone();
        }
        fn collect(graph: &RackGraph, path: &mut Vec<u32>, out: &mut Vec<RackParameterAddress>, seen: &mut std::collections::HashSet<RackParameterAddress>) {
            for node in &graph.nodes {
                path.push(node.id);
                match &node.processor {
                    RackProcessor::Rack { graph } => collect(graph, path, out, seen),
                    RackProcessor::Note { module } => {
                        let mut parameter = 0;
                        while module.parameter_info(parameter).is_some() {
                            let address = RackParameterAddress { path: path.clone(), kind: module.kind_id().into(), parameter: parameter as u32 };
                            if seen.insert(address.clone()) { out.push(address); }
                            parameter += 1;
                        }
                    }
                    _ => {}
                }
                path.pop();
            }
        }
        let mut addresses = self.parameters.clone();
        let mut seen = addresses.iter().cloned().collect();
        collect(self, &mut Vec::new(), &mut addresses, &mut seen);
        if addresses.len() > MAX_RACK_PARAMETERS { return Err("Rack parameter address history exceeds 16384 entries; create a new rack".into()); }
        self.parameters = addresses;
        Ok(())
    }
    pub fn check(&self) -> Result<(), String> {
        let mut nodes = 0;
        let mut edges = 0;
        self.check_nested(1, &mut nodes, &mut edges)?;
        self.check_aggregate_fanout()
    }
    /// Count every transit event of the flattened note DAG, including the
    /// Cartesian expansion at nested boundaries. Assume four chord voices
    /// even when fewer are stored: automation must remain inside admission.
    fn check_aggregate_fanout(&self) -> Result<(), String> {
        #[derive(Default)]
        struct Boundary { input: Vec<usize>, output: Vec<usize> }
        #[derive(Default)]
        struct Admission { processors: Vec<RackProcessor>, edges: Vec<(usize, usize)>, roots: Vec<usize>, wires: usize }
        fn add(graph: &RackGraph, root: bool, flat: &mut Admission) -> Result<Boundary, String> {
            let mut boundary = Boundary::default();
            let mut places = Vec::new();
            for node in &graph.nodes {
                let place = if let RackProcessor::Rack { graph } = &node.processor { add(graph, false, flat)? }
                else {
                    let i = flat.processors.len();
                    flat.processors.push(node.processor.clone());
                    if matches!(node.processor, RackProcessor::NoteInput) {
                        boundary.input.push(i); if root { flat.roots.push(i); }
                    }
                    if matches!(node.processor, RackProcessor::NoteOutput) { boundary.output.push(i); }
                    Boundary {
                        input: node.processor.port(NOTE_IN, false).map(|_| vec![i]).unwrap_or_default(),
                        output: node.processor.port(NOTE_OUT, true).map(|_| vec![i]).unwrap_or_default(),
                    }
                };
                places.push(place);
            }
            for edge in &graph.edges {
                let from = graph.nodes.iter().position(|n| n.id == edge.from.node).unwrap();
                let to = graph.nodes.iter().position(|n| n.id == edge.to.node).unwrap();
                if graph.nodes[from].processor.port(edge.from.port, true) == Some(RackSignal::Note) {
                    for &a in &places[from].output { for &b in &places[to].input { flat.edges.push((a, b)); } }
                }
                // Each boundary can expose at most 64 nodes. Charge the
                // flattened audio/control Cartesian products conservatively.
                fn width(processor: &RackProcessor, signal: RackSignal, output: bool) -> usize {
                    if let RackProcessor::Rack { graph } = processor {
                        graph.nodes.iter().filter(|node| match (signal, output, &node.processor) {
                            (RackSignal::Audio, false, RackProcessor::AudioInput)
                            | (RackSignal::Audio, true, RackProcessor::AudioOutput)
                            | (RackSignal::Note, false, RackProcessor::NoteInput)
                            | (RackSignal::Note, true, RackProcessor::NoteOutput) => true,
                            _ => false,
                        }).count()
                    } else { 1 }
                }
                let signal = graph.nodes[from].processor.port(edge.from.port, true).unwrap();
                flat.wires += width(&graph.nodes[from].processor, signal, true) * width(&graph.nodes[to].processor, signal, false);
                if flat.wires > 1024 { return Err("Rack aggregate fan-out exceeds 1024 flattened wires".into()); }
            }
            Ok(boundary)
        }
        let mut flat = Admission::default();
        add(self, true, &mut flat)?;
        let mut done = vec![false; flat.processors.len()];
        let mut cost = vec![0usize; flat.processors.len()];
        let mut generators = cost.clone();
        let mut envelopes = cost.clone();
        for _ in 0..flat.processors.len() {
            let Some(i) = (0..done.len()).find(|&i| !done[i] && flat.edges.iter().filter(|(a, _)| *a == i).all(|(_, b)| done[*b])) else {
                return Err("Rack flattened note cycle".into());
            };
            let multiplier = if matches!(flat.processors[i], RackProcessor::Note { module: RackNoteModule::KeyMapper { .. } }) { 4usize } else { 1 };
            let successors = flat.edges.iter().filter(|(a, _)| *a == i).map(|(_, b)| cost[*b]).fold(0usize, usize::saturating_add);
            cost[i] = 1usize.saturating_add(multiplier.saturating_mul(successors));
            envelopes[i] = usize::from(matches!(flat.processors[i], RackProcessor::Note { module: RackNoteModule::Envelope { .. } }))
                + flat.edges.iter().filter(|(a, _)| *a == i).map(|(_, b)| envelopes[*b]).max().unwrap_or(0);
            if envelopes[i] > windfall_dsp::rack_note::MAX_NOTE_ENVELOPES {
                return Err("Rack note path exceeds eight composed envelope shapes".into());
            }
            generators[i] = if matches!(flat.processors[i], RackProcessor::Note { module: RackNoteModule::Sequencer { .. } }) { 1 }
                else { multiplier.saturating_mul(flat.edges.iter().filter(|(a, _)| *a == i).map(|(_, b)| generators[*b]).fold(0usize, usize::saturating_add)) };
            if cost[i] > MAX_RACK_EVENTS || generators[i] > 64 {
                return Err("Rack aggregate fan-out exceeds 256 transit events or 64 generator seats per trigger".into());
            }
            done[i] = true;
        }
        if flat.roots.iter().map(|&i| cost[i]).sum::<usize>() > MAX_RACK_EVENTS
            || flat.roots.iter().map(|&i| generators[i]).sum::<usize>() > 64
        { return Err("Rack root aggregate fan-out exceeds realtime event capacity".into()); }
        Ok(())
    }
    fn check_nested(
        &self,
        depth: usize,
        nodes: &mut usize,
        edges: &mut usize,
    ) -> Result<(), String> {
        *nodes += self.nodes.len();
        *edges += self.edges.len();
        if self.parameters.len() > MAX_RACK_PARAMETERS
            || self.parameters.iter().any(|p| p.path.is_empty() || p.path.len() > MAX_RACK_DEPTH || p.path.contains(&0) || p.kind.len() > 64 || p.parameter > 1024)
        { return Err("Invalid rack parameter address registry".into()); }
        let mut addresses = std::collections::HashSet::new();
        if self.parameters.iter().any(|p| !addresses.insert((&p.path, &p.kind, p.parameter))) {
            return Err("Duplicate rack parameter address".into());
        }
        if depth > MAX_RACK_DEPTH || *nodes > MAX_RACK_NODES || *edges > MAX_RACK_EDGES {
            return Err("Rack exceeds depth (4), total node (64), or edge (256) limit".into());
        }
        let mut ids = std::collections::HashSet::new();
        for node in &self.nodes {
            if node.id == 0 || node.id >= self.next_id || !ids.insert(node.id) {
                return Err("Rack node IDs must be unique, nonzero and below nextId".into());
            }
            if node.name.len() > 256 || node.position.iter().any(|v| !v.is_finite()) {
                return Err("Invalid rack node name or position".into());
            }
            match &node.processor {
                RackProcessor::Instrument { params } if *params != params.sanitized() => {
                    return Err("Invalid rack instrument controls".into());
                }
                RackProcessor::Effect { params, mix }
                    if *params != params.sanitized()
                        || !mix.is_finite()
                        || !(0.0..=1.0).contains(mix) =>
                {
                    return Err("Invalid rack effect controls".into());
                }
                RackProcessor::ControlInput { value, index }
                    if !value.is_finite() || !(0.0..=1.0).contains(value) || *index >= 32 =>
                {
                    return Err("Invalid rack control inlet".into());
                }
                RackProcessor::Note { module } if !module.valid() => {
                    return Err("Invalid note-module controls".into());
                }
                RackProcessor::Rack { graph } => graph.check_nested(depth + 1, nodes, edges)?,
                _ => {}
            }
        }
        let mut endpoints = std::collections::HashSet::new();
        let mut macros = std::collections::HashSet::new();
        for node in &self.nodes {
            if let RackProcessor::ControlInput { index, .. } = &node.processor
                && !macros.insert(*index)
            {
                return Err("Rack macro indices must be unique".into());
            }
        }
        for edge in &self.edges {
            let from = self
                .node(edge.from.node)
                .ok_or("Rack edge source was removed")?;
            let to = self
                .node(edge.to.node)
                .ok_or("Rack edge destination was removed")?;
            let signal = from
                .processor
                .port(edge.from.port, true)
                .ok_or("Unknown rack output port")?;
            if to.processor.port(edge.to.port, false) != Some(signal) {
                return Err("Rack edge port types do not match".into());
            }
            if !edge.gain.is_finite() || !(-4.0..=4.0).contains(&edge.gain) {
                return Err("Rack edge gain must be from -4 to 4".into());
            }
            if !endpoints.insert((
                edge.from.node,
                edge.from.port,
                edge.to.node,
                edge.to.port,
                edge.parameter.as_deref(),
            )) {
                return Err("Duplicate rack edge".into());
            }
            if signal == RackSignal::Control {
                if edge
                    .parameter
                    .as_deref()
                    .and_then(|id| to.processor.parameter_index(id))
                    .is_none()
                {
                    return Err("Rack control destination parameter was removed".into());
                }
                if self
                    .edges
                    .iter()
                    .filter(|other| other.to == edge.to && other.parameter == edge.parameter)
                    .count()
                    > 1
                {
                    return Err("A rack parameter accepts one control wire".into());
                }
                // Compilation fixes internal compensation lengths. Reject
                // latency-changing controls until dynamic graph PDC exists.
                if let RackProcessor::Effect { params, .. } = &to.processor {
                    let index = to
                        .processor
                        .parameter_index(edge.parameter.as_deref().unwrap())
                        .unwrap();
                    let info = &params.kind().descriptors()[index];
                    let mut low = *params;
                    let mut high = *params;
                    low.set(index, info.min);
                    high.set(index, info.max);
                    if [44_100.0, 48_000.0, 96_000.0].iter().any(|&rate| {
                        low.latency_samples(rate) != params.latency_samples(rate)
                            || high.latency_samples(rate) != params.latency_samples(rate)
                    }) {
                        return Err("Rack control cannot change processor latency; edit the saved node instead".into());
                    }
                }
            } else if edge.parameter.is_some() {
                return Err("Only control wires name a parameter".into());
            }
        }
        let order = self.order()?;
        // Bound multiplicative note fanout before admission, including chords.
        let mut cost = [0usize; MAX_RACK_NODES];
        for &index in order.iter().rev() {
            let node = &self.nodes[index];
            let successors = self
                .edges
                .iter()
                .filter(|edge| {
                    edge.from.node == node.id
                        && node.processor.port(edge.from.port, true) == Some(RackSignal::Note)
                })
                .map(|edge| {
                    self.nodes
                        .iter()
                        .position(|node| node.id == edge.to.node)
                        .unwrap()
                })
                .map(|next| cost[next])
                .sum::<usize>();
            let voices = match &node.processor {
                RackProcessor::Note {
                    module: RackNoteModule::KeyMapper { voices, .. },
                } => usize::from(*voices),
                _ => 1,
            };
            cost[index] = successors.max(1).saturating_mul(voices);
            if cost[index] > MAX_RACK_EVENTS {
                return Err("Rack note fanout exceeds 256 events per trigger".into());
            }
        }
        Ok(())
    }
    pub fn node(&self, id: u32) -> Option<&RackNode> {
        self.nodes.iter().find(|node| node.id == id)
    }
    pub fn control(&self, index: usize) -> Option<f32> {
        self.nodes.iter().find_map(|node| match node.processor {
            RackProcessor::ControlInput {
                index: macro_index,
                value,
            } if usize::from(macro_index) == index => Some(value),
            _ => None,
        })
    }
    /// Bounded metadata lookup, also used by mixer PDC on plan adoption.
    /// Checked graphs have at most 64 nodes. This uses no heap storage.
    pub fn latency_samples(&self, rate: f32) -> usize {
        self.latency_with_input(rate, 0)
    }
    /// Absolute arrival latency with the containing channel's audio source.
    /// Nested instruments have their own origin; only AudioInput inherits it.
    pub fn latency_with_input(&self, rate: f32, input: usize) -> usize {
        let mut done = [false; MAX_RACK_NODES];
        let mut arrival = [0usize; MAX_RACK_NODES];
        for _ in 0..self.nodes.len().min(MAX_RACK_NODES) {
            let Some(index) = (0..self.nodes.len().min(MAX_RACK_NODES)).find(|&index| {
                !done[index]
                    && self
                        .edges
                        .iter()
                        .filter(|edge| edge.to.node == self.nodes[index].id)
                        .all(|edge| {
                            self.nodes
                                .iter()
                                .position(|node| node.id == edge.from.node)
                                .is_some_and(|index| done[index])
                        })
            }) else {
                break;
            };
            let incoming = self
                .edges
                .iter()
                .filter(|edge| edge.to.node == self.nodes[index].id && edge.to.port == AUDIO_IN)
                .filter_map(|edge| self.nodes.iter().position(|node| node.id == edge.from.node))
                .map(|index| arrival[index])
                .max()
                .unwrap_or(0);
            arrival[index] = match &self.nodes[index].processor {
                RackProcessor::AudioInput => input.max(incoming),
                RackProcessor::Instrument { params } => params.latency_samples(rate),
                RackProcessor::Effect { params, .. } => {
                    incoming.saturating_add(params.latency_samples(rate))
                }
                RackProcessor::Rack { graph } => graph.latency_with_input(rate, incoming),
                _ => incoming,
            };
            done[index] = true;
        }
        self.nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| matches!(node.processor, RackProcessor::AudioOutput))
            .map(|(index, _)| arrival[index])
            .max()
            .unwrap_or(0)
    }
    /// Control-side topological compilation. Even control and muted wires
    /// participate: unmuting cannot activate a previously accepted cycle.
    pub fn order(&self) -> Result<Vec<usize>, String> {
        let mut degree = vec![0usize; self.nodes.len()];
        for edge in &self.edges {
            let index = self
                .nodes
                .iter()
                .position(|node| node.id == edge.to.node)
                .ok_or("Missing rack node")?;
            degree[index] += 1;
        }
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut used = vec![false; self.nodes.len()];
        while let Some(index) =
            (0..self.nodes.len()).find(|&index| !used[index] && degree[index] == 0)
        {
            used[index] = true;
            out.push(index);
            for edge in self
                .edges
                .iter()
                .filter(|edge| edge.from.node == self.nodes[index].id)
            {
                let next = self
                    .nodes
                    .iter()
                    .position(|node| node.id == edge.to.node)
                    .ok_or("Missing rack node")?;
                degree[next] -= 1;
            }
        }
        if out.len() != self.nodes.len() {
            Err("Rack cycles are refused".into())
        } else {
            Ok(out)
        }
    }
}

pub fn check_project(project: &Project) -> Result<(), String> {
    if project.settings.modular_racks.len() > 16 {
        return Err("A project supports at most 16 modular racks".into());
    }
    let mut channels = std::collections::HashSet::new();
    for rack in &project.settings.modular_racks {
        if !channels.insert(rack.channel) || project.channel(rack.channel).is_none() {
            return Err("Rack channel is missing or repeated".into());
        }
        if !matches!(
            project.channel(rack.channel).unwrap().source,
            crate::ChannelSource::Instrument { .. }
        ) {
            return Err("A modular rack currently requires a built-in instrument channel".into());
        }
        if project.plugins.iter().any(|binding| {
            binding.target
                == crate::PluginTarget::Instrument {
                    channel: rack.channel,
                }
        }) {
            return Err("A hosted channel cannot be replaced by a built-in rack; remove its native binding first".into());
        }
        rack.graph.check()?;
    }
    Ok(())
}

/// Rack-only preset: all nested module controls and wires, independent of
/// channel IDs. Native owners are not represented as fake built-in nodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RackPreset {
    pub schema: String,
    pub version: u32,
    pub name: String,
    pub graph: RackGraph,
}
impl RackPreset {
    pub fn new(name: String, graph: RackGraph) -> Result<Self, String> {
        graph.check()?;
        if name.len() > 256 {
            return Err("Rack preset name exceeds 256 bytes".into());
        }
        Ok(Self {
            schema: "windfall.rack".into(),
            version: 1,
            name,
            graph,
        })
    }
    pub fn from_json(json: &str) -> Result<Self, String> {
        if json.len() > 4 * 1024 * 1024 {
            return Err("Rack preset exceeds 4 MiB".into());
        }
        let preset: Self = serde_json::from_str(json).map_err(|error| error.to_string())?;
        if preset.schema != "windfall.rack" || preset.version != 1 {
            return Err("Unsupported rack preset schema or version".into());
        }
        preset.graph.check()?;
        if preset.name.len() > 256 {
            return Err("Rack preset name exceeds 256 bytes".into());
        }
        Ok(preset)
    }
}
