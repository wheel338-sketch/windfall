//! Control-side graph compilation and bounded, allocation-free rack playback.
//! All vectors, delay rings, DSP owners and event seats are built in `prepare`.
//! A changed rack is retired with its containing PlanState on the control side.
use windfall_dsp::rack_note::{
    EnvelopeDestination, NoteSequence, RackNote, RackNoteModule, SequenceMode,
};
use windfall_dsp::{EffectParams, EffectSlot, InstrumentParams, NoteExpression, NoteInstanceId};
use windfall_project::rack::*;

#[path = "modular_assets.rs"]
pub(crate) mod assets;
#[path = "modular_provider.rs"]
mod provider;
use provider::{RackEffect, RackInstrument};

const MAX_OWNERS: usize = 1024;
const MAX_GENERATORS: usize = 64;
const MAX_FLAT_EDGES: usize = 1024;

struct FlatNode {
    path: Vec<u32>,
    processor: RackProcessor,
    muted: bool,
    solo: bool,
    root: bool,
    asset: Option<windfall_project::RackNodeAsset>,
    hosted: Option<windfall_project::PluginBinding>,
}
struct FlatEdge {
    from: usize,
    to: usize,
    port: u16,
    signal: RackSignal,
    gain: f32,
    parameter: Option<usize>,
}
#[derive(Default)]
struct Boundary {
    audio_in: Vec<usize>,
    audio_out: Vec<usize>,
    note_in: Vec<usize>,
    note_out: Vec<usize>,
    control_in: Vec<usize>,
    control_out: Vec<usize>,
}
struct Flat {
    nodes: Vec<FlatNode>,
    edges: Vec<FlatEdge>,
}
impl Flat {
    fn add(
        &mut self,
        graph: &RackGraph,
        root: bool,
        muted: bool,
        solo: bool,
        path: &mut Vec<u32>,
    ) -> Result<Boundary, String> {
        let mut places: Vec<Boundary> = Vec::new();
        let mut boundary = Boundary::default();
        for node in &graph.nodes {
            path.push(node.id);
            let own = if let RackProcessor::Rack { graph } = &node.processor {
                self.add(graph, false, muted || node.muted, solo || node.solo, path)?
            } else {
                let index = self.nodes.len();
                self.nodes.push(FlatNode {
                    path: path.clone(),
                    processor: node.processor.clone(),
                    muted: muted || node.muted,
                    solo: solo || node.solo,
                    root,
                    asset: node.asset.clone(),
                    hosted: node.hosted.clone(),
                });
                let mut own = Boundary::default();
                if node.processor.port(AUDIO_IN, false).is_some() {
                    own.audio_in.push(index);
                }
                if node.processor.port(AUDIO_OUT, true).is_some() {
                    own.audio_out.push(index);
                }
                if node.processor.port(NOTE_IN, false).is_some() {
                    own.note_in.push(index);
                }
                if node.processor.port(NOTE_OUT, true).is_some() {
                    own.note_out.push(index);
                }
                if node.processor.port(CONTROL_IN, false).is_some() {
                    own.control_in.push(index);
                }
                if node.processor.port(CONTROL_OUT, true).is_some() {
                    own.control_out.push(index);
                }
                if matches!(node.processor, RackProcessor::AudioInput) {
                    boundary.audio_in.push(index);
                }
                if matches!(node.processor, RackProcessor::AudioOutput) {
                    boundary.audio_out.push(index);
                }
                if matches!(node.processor, RackProcessor::NoteInput) {
                    boundary.note_in.push(index);
                }
                if matches!(node.processor, RackProcessor::NoteOutput) {
                    boundary.note_out.push(index);
                }
                own
            };
            places.push(own);
            path.pop();
        }
        for edge in &graph.edges {
            let from = graph
                .nodes
                .iter()
                .position(|node| node.id == edge.from.node)
                .ok_or("Missing rack source")?;
            let to = graph
                .nodes
                .iter()
                .position(|node| node.id == edge.to.node)
                .ok_or("Missing rack destination")?;
            let signal = graph.nodes[from]
                .processor
                .port(edge.from.port, true)
                .ok_or("Missing rack port")?;
            let (sources, targets) = match signal {
                RackSignal::Audio => (&places[from].audio_out, &places[to].audio_in),
                RackSignal::Note => (&places[from].note_out, &places[to].note_in),
                RackSignal::Control => (&places[from].control_out, &places[to].control_in),
            };
            for &source in sources {
                for &target in targets {
                    self.edges.push(FlatEdge {
                        from: source,
                        to: target,
                        port: edge.from.port,
                        signal,
                        gain: edge.gain,
                        parameter: edge
                            .parameter
                            .as_deref()
                            .and_then(|id| graph.nodes[to].processor.parameter_index(id)),
                    });
                    if self.edges.len() > MAX_FLAT_EDGES {
                        return Err("Flattened rack exceeds 1024 wires".into());
                    }
                }
            }
        }
        Ok(boundary)
    }
    fn order(&self) -> Result<Vec<usize>, String> {
        let mut out = Vec::with_capacity(self.nodes.len());
        let mut done = vec![false; self.nodes.len()];
        while let Some(index) = (0..self.nodes.len()).find(|&index| {
            !done[index]
                && self
                    .edges
                    .iter()
                    .filter(|edge| edge.to == index)
                    .all(|edge| done[edge.from])
        }) {
            done[index] = true;
            out.push(index);
        }
        if out.len() != self.nodes.len() {
            Err("Flattened rack cycle".into())
        } else {
            Ok(out)
        }
    }
}

enum Unit {
    Bus,
    Control {
        value: f32,
        index: Option<u8>,
    },
    Instrument {
        dsp: RackInstrument,
        params: Box<InstrumentParams>,
    },
    Effect {
        dsp: RackEffect,
        params: Box<EffectParams>,
    },
    Note(RackNoteModule),
}
struct Node {
    unit: Unit,
    muted: bool,
    root_audio: bool,
    root_note: bool,
}
struct Ring {
    data: Box<[[f32; 2]]>,
    at: usize,
}
impl Ring {
    fn new(delay: usize) -> Self {
        Self {
            data: vec![[0.0; 2]; delay].into_boxed_slice(),
            at: 0,
        }
    }
    fn frame(&mut self, input: [f32; 2]) -> [f32; 2] {
        if self.data.is_empty() {
            return input;
        }
        let out = self.data[self.at];
        self.data[self.at] = input;
        self.at += 1;
        if self.at == self.data.len() {
            self.at = 0;
        }
        out
    }
    fn reset(&mut self) {
        self.data.fill([0.0; 2]);
        self.at = 0;
    }
}
struct Edge {
    from: usize,
    to: usize,
    port: u16,
    signal: RackSignal,
    gain: f32,
    parameter: Option<usize>,
    delay: Ring,
}
#[derive(Clone, Copy)]
struct Owner {
    active: bool,
    parent: NoteInstanceId,
    child: NoteInstanceId,
    node: usize,
    note: RackNote,
    age: u64,
    release_age: Option<u64>,
    release_levels: [f32; windfall_dsp::rack_note::MAX_NOTE_ENVELOPES],
    end: f64,
    transforms: u64,
    pitch_offset: f32,
}
impl Owner {
    const EMPTY: Self = Self {
        active: false,
        parent: NoteInstanceId(0),
        child: NoteInstanceId(0),
        node: 0,
        note: RackNote {
            key: 0,
            velocity: 0.0,
            gain: 1.0,
            pan: 0.0,
            expression: NoteExpression::NEUTRAL,
            channel: 0,
            envelopes: [None; windfall_dsp::rack_note::MAX_NOTE_ENVELOPES],
        },
        age: 0,
        release_age: None,
        release_levels: [0.0; windfall_dsp::rack_note::MAX_NOTE_ENVELOPES],
        end: f64::INFINITY,
        transforms: 0,
        pitch_offset: 0.0,
    };
}
#[derive(Clone, Copy)]
struct Generator {
    active: bool,
    parent: NoteInstanceId,
    node: usize,
    note: RackNote,
    params: NoteSequence,
    phase: f64,
    step: u64,
    transforms: u64,
    admission: u64,
}
impl Generator {
    const EMPTY: Self = Self {
        active: false,
        parent: NoteInstanceId(0),
        node: 0,
        note: Owner::EMPTY.note,
        params: NoteSequence {
            mode: SequenceMode::Sequence,
            grid: windfall_dsp::notemap::StepGridParams {
                steps_per_beat: 4,
                steps: [windfall_dsp::notemap::GridStep {
                    gate: true,
                    pitch_offset: 0,
                    velocity_scale: 1.0,
                }; 16],
            },
            length: 16,
            gate: 0.5,
            octaves: 1,
        },
        phase: 0.0,
        step: 0,
        transforms: 0,
        admission: 0,
    };
}
#[derive(Clone, Copy)]
struct Event {
    node: usize,
    parent: NoteInstanceId,
    note: RackNote,
    duration: f64,
    transforms: u64,
}
impl Event {
    const EMPTY: Self = Self {
        node: 0,
        parent: NoteInstanceId(0),
        note: Owner::EMPTY.note,
        duration: f64::INFINITY,
        transforms: 0,
    };
}

/// A prepared rack owns all callback storage. Overflow increments `rejected`
/// and refuses the new event; it never steals an unrelated held occurrence.
pub struct ModularRack {
    plugin_records: Vec<(windfall_project::PluginTarget, u64, usize)>,
    _asset_pins: windfall_dsp::asset::AssetPins,
    parameters: Vec<Option<(usize, usize)>>,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    order: Vec<usize>,
    outputs: Vec<(usize, Ring)>,
    owners: Box<[Owner]>,
    generators: Box<[Generator]>,
    queue: Box<[Event]>,
    next_id: u64,
    sample_rate: f64,
    tempo: f64,
    latency: usize,
    rejected: u64,
    quiet_frames: usize,
    pub(crate) retirement_audible: bool,
    _generation: assets::GenerationPermit,
}
impl ModularRack {
    pub fn prepare(graph: &RackGraph, sample_rate: u32, tempo: f64) -> Result<Self, String> {
        Self::prepare_with_input_latency(graph, sample_rate, tempo, 0)
    }
    pub fn prepare_with_factory(
        graph: &RackGraph,
        sample_rate: u32,
        tempo: f64,
        factory: &dyn crate::plugins::PluginFactory,
    ) -> Result<Self, String> {
        Self::prepare_in_family(
            graph,
            sample_rate,
            tempo,
            0,
            assets::GenerationFamily::new(),
            Some(factory),
        )
    }
    pub fn parallel_safe(&self) -> bool {
        self.nodes.iter().all(|node| match &node.unit {
            Unit::Instrument { dsp, .. } => !dsp.is_hosted(),
            Unit::Effect { dsp, .. } => !dsp.is_hosted(),
            _ => true,
        })
    }
    pub fn control_boundary(&mut self) {
        for node in &mut self.nodes {
            match &mut node.unit {
                Unit::Instrument { dsp, .. } => dsp.control_boundary(),
                Unit::Effect { dsp, .. } => dsp.control_boundary(),
                _ => {}
            }
        }
    }
    pub fn transport(&mut self, transport: crate::plugins::PluginTransport) {
        for node in &mut self.nodes {
            match &mut node.unit {
                Unit::Instrument { dsp, .. } => dsp.transport(transport),
                Unit::Effect { dsp, .. } => dsp.transport(transport),
                _ => {}
            }
        }
    }
    pub(crate) fn retire_from_document(&mut self) {
        for node in &mut self.nodes {
            match &mut node.unit {
                Unit::Instrument { dsp, .. } => dsp.retire(),
                Unit::Effect { dsp, .. } => dsp.retire(),
                _ => {}
            }
        }
        self.all_notes_off();
    }
    pub fn prepare_with_input_latency(
        graph: &RackGraph,
        sample_rate: u32,
        tempo: f64,
        source_latency: usize,
    ) -> Result<Self, String> {
        Self::prepare_in_family(
            graph,
            sample_rate,
            tempo,
            source_latency,
            assets::GenerationFamily::new(),
            None,
        )
    }
    pub(crate) fn prepare_in_family(
        graph: &RackGraph,
        sample_rate: u32,
        tempo: f64,
        source_latency: usize,
        family: std::sync::Arc<assets::GenerationFamily>,
        factory: Option<&dyn crate::plugins::PluginFactory>,
    ) -> Result<Self, String> {
        graph.check()?;
        if !(8_000..=384_000).contains(&sample_rate) || !tempo.is_finite() || tempo <= 0.0 {
            return Err("Invalid rack sample rate or tempo".into());
        }
        let mut generation = family.reserve(graph, sample_rate)?;
        let mut flat = Flat {
            nodes: Vec::new(),
            edges: Vec::new(),
        };
        let boundary = flat.add(graph, true, false, false, &mut Vec::new())?;
        let parameters = graph
            .parameters
            .iter()
            .map(|address| {
                let index = flat
                    .nodes
                    .iter()
                    .position(|node| node.path == address.path)?;
                let RackProcessor::Note { module } = &flat.nodes[index].processor else {
                    return None;
                };
                (module.kind_id() == address.kind
                    && module.parameter_info(address.parameter as usize).is_some())
                .then_some((index, address.parameter as usize))
            })
            .collect();
        let order = flat.order()?;
        let any_solo = flat.nodes.iter().any(|node| node.solo);
        let mut audible: Vec<bool> = flat
            .nodes
            .iter()
            .map(|node| node.solo || !any_solo)
            .collect();
        if any_solo {
            let mut upstream = audible.clone();
            let mut downstream = audible.clone();
            for _ in 0..flat.nodes.len() {
                for edge in &flat.edges {
                    if upstream[edge.to] {
                        upstream[edge.from] = true;
                    }
                    if downstream[edge.from] {
                        downstream[edge.to] = true;
                    }
                }
            }
            for index in 0..audible.len() {
                audible[index] = upstream[index] || downstream[index];
            }
        }
        let bank_budget = &family.bank_budget;
        let nodes: Vec<Node> = flat
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                let unit = match &node.processor {
                    RackProcessor::Instrument { params } => {
                        if let Some(binding) = &node.hosted {
                            let mut unit = factory
                                .map(|factory| {
                                    factory.instrument(
                                        binding,
                                        sample_rate,
                                        crate::mixer::MAX_BLOCK,
                                    )
                                })
                                .transpose()?;
                            if let Some(unit) = &mut unit {
                                unit.adopt_parameters(&binding.parameters);
                                for parameter in &binding.parameters {
                                    if !parameter.read_only {
                                        unit.set_param(parameter.id, parameter.value);
                                    }
                                }
                                unit.set_tempo(tempo as f32);
                            }
                            Unit::Instrument {
                                dsp: RackInstrument::Hosted(unit),
                                params: Box::new(*params),
                            }
                        } else {
                            let mut dsp =
                                assets::instrument(params, node.asset.as_ref(), bank_budget)?;
                            dsp.prepare(sample_rate as f32, 1);
                            dsp.set_params(params);
                            dsp.set_tempo(tempo as f32);
                            Unit::Instrument {
                                dsp: RackInstrument::Builtin(dsp),
                                params: Box::new(*params),
                            }
                        }
                    }
                    RackProcessor::Effect { params, mix } => {
                        if let Some(binding) = &node.hosted {
                            let unit = factory
                                .map(|factory| {
                                    factory.effect(binding, sample_rate, crate::mixer::MAX_BLOCK)
                                })
                                .transpose()?;
                            if unit
                                .as_ref()
                                .is_some_and(|unit| unit.latency() > sample_rate as usize)
                            {
                                return Err("Native rack latency exceeds one second".into());
                            }
                            generation.reserve_runtime(
                                crate::rack::Compensation::reserved_payload(
                                    unit.as_ref().map_or(0, |unit| unit.latency()),
                                    0,
                                )
                                .ok_or("Native rack dry history size overflow")?,
                            )?;
                            let mut dsp = crate::plugins::ExternalEffect::new(
                                unit,
                                binding,
                                sample_rate,
                                !node.muted,
                                *mix,
                            );
                            if let Some(unit) = &mut dsp.unit {
                                unit.set_tempo(tempo as f32);
                            }
                            Unit::Effect {
                                dsp: RackEffect::Hosted(dsp),
                                params: Box::new(*params),
                            }
                        } else {
                            let mut params = *params;
                            if let Some(bytes) = params.kind().history_bytes(sample_rate as f32) {
                                generation.reserve_runtime(bytes)?;
                            }
                            let mut dsp = EffectSlot::new(assets::effect(
                                &mut params,
                                node.asset.as_ref(),
                                sample_rate,
                            )?);
                            dsp.prepare(sample_rate as f32, 1);
                            dsp.set_params(&params);
                            dsp.set_tempo(tempo as f32);
                            dsp.set_mix(*mix);
                            dsp.set_enabled(!node.muted);
                            Unit::Effect {
                                dsp: RackEffect::Builtin(dsp),
                                params: Box::new(params),
                            }
                        }
                    }
                    RackProcessor::Note { module } => Unit::Note(*module),
                    RackProcessor::ControlInput { value, index } => Unit::Control {
                        value: *value,
                        index: node.root.then_some(*index),
                    },
                    _ => Unit::Bus,
                };
                Ok::<_, String>(Node {
                    unit,
                    muted: node.muted || !audible[index],
                    root_audio: node.root && matches!(node.processor, RackProcessor::AudioInput),
                    root_note: node.root && matches!(node.processor, RackProcessor::NoteInput),
                })
            })
            .collect::<Result<_, _>>()?;
        let asset_pins =
            windfall_dsp::asset::AssetPins::for_effects(nodes.iter().filter_map(|node| {
                match &node.unit {
                    Unit::Effect { params, .. } => Some(params.as_ref()),
                    _ => None,
                }
            }));
        let mut arrival = vec![0usize; nodes.len()];
        let mut input_latency = vec![0usize; nodes.len()];
        for &index in &order {
            input_latency[index] = flat
                .edges
                .iter()
                .filter(|edge| edge.to == index && edge.signal == RackSignal::Audio)
                .map(|edge| arrival[edge.from])
                .max()
                .unwrap_or(0);
            let own = match &nodes[index].unit {
                Unit::Instrument { dsp, .. } => dsp.latency_samples(),
                Unit::Effect { dsp, .. } => dsp.latency_samples(),
                _ => 0,
            };
            arrival[index] = if nodes[index].root_audio {
                source_latency.max(input_latency[index])
            } else {
                input_latency[index].saturating_add(own)
            };
        }
        let latency = boundary
            .audio_out
            .iter()
            .map(|&index| arrival[index])
            .max()
            .unwrap_or(0);
        if latency > sample_rate as usize {
            return Err("Rack latency exceeds one second".into());
        }
        let frames = boundary
            .audio_out
            .iter()
            .map(|&index| latency.saturating_sub(arrival[index]))
            .sum::<usize>()
            + flat
                .edges
                .iter()
                .filter(|edge| edge.signal == RackSignal::Audio)
                .map(|edge| input_latency[edge.to].saturating_sub(arrival[edge.from]))
                .sum::<usize>();
        if frames > sample_rate as usize * 16 || frames > 1024 * 1024 {
            return Err("Rack compensation storage exceeds 16 seconds of stereo frames".into());
        }
        generation.reserve_runtime(
            frames
                .checked_mul(8)
                .ok_or("Rack compensation byte size overflow")?,
        )?;
        let edges = flat
            .edges
            .into_iter()
            .map(|edge| Edge {
                from: edge.from,
                to: edge.to,
                port: edge.port,
                signal: edge.signal,
                gain: edge.gain,
                parameter: edge.parameter,
                delay: Ring::new(if edge.signal == RackSignal::Audio {
                    input_latency[edge.to].saturating_sub(arrival[edge.from])
                } else {
                    0
                }),
            })
            .collect();
        let outputs = boundary
            .audio_out
            .into_iter()
            .map(|index| (index, Ring::new(latency - arrival[index])))
            .collect();
        let plugin_records = flat
            .nodes
            .iter()
            .zip(&nodes)
            .filter_map(|(flat, node)| {
                let binding = flat.hosted.as_ref()?;
                let latency = match &node.unit {
                    Unit::Instrument { dsp, .. } => dsp.latency_samples(),
                    Unit::Effect { dsp, .. } => dsp.latency_samples(),
                    _ => 0,
                };
                Some((binding.target, crate::plugins::identity(binding), latency))
            })
            .collect();
        Ok(Self {
            plugin_records,
            _asset_pins: asset_pins,
            parameters,
            nodes,
            edges,
            order,
            outputs,
            owners: vec![Owner::EMPTY; MAX_OWNERS].into_boxed_slice(),
            generators: vec![Generator::EMPTY; MAX_GENERATORS].into_boxed_slice(),
            queue: vec![Event::EMPTY; MAX_RACK_EVENTS].into_boxed_slice(),
            next_id: 0,
            sample_rate: f64::from(sample_rate),
            tempo,
            latency,
            rejected: 0,
            quiet_frames: 0,
            retirement_audible: true,
            _generation: generation,
        })
    }
    pub(crate) fn finished_retiring(&self) -> bool {
        self.active_voices() == 0 && self.quiet_frames >= self.tail_samples().max(self.latency)
    }
    pub fn latency_samples(&self) -> usize {
        self.latency
    }
    pub(crate) fn plugin_records(&self) -> &[(windfall_project::PluginTarget, u64, usize)] {
        &self.plugin_records
    }
    /// Drained once per callback into the engine-wide observable counter.
    pub fn take_rejected_events(&mut self) -> u64 {
        std::mem::take(&mut self.rejected)
    }
    pub fn rejected_events(&self) -> u64 {
        self.rejected
    }
    pub fn uses_audio_input(&self) -> bool {
        self.nodes.iter().any(|node| node.root_audio && !node.muted)
    }
    pub fn active_voices(&self) -> usize {
        self.nodes
            .iter()
            .map(|node| match &node.unit {
                Unit::Instrument { dsp, .. } => dsp.active_voices(),
                _ => 0,
            })
            .sum()
    }
    pub fn tail_samples(&self) -> usize {
        self.nodes
            .iter()
            .map(|node| match &node.unit {
                Unit::Instrument { dsp, .. } => dsp.tail_samples(),
                Unit::Effect { dsp, .. } => dsp.tail_samples(),
                _ => 0,
            })
            .fold(0usize, usize::saturating_add)
    }
    pub fn set_tempo(&mut self, tempo: f64) {
        if !tempo.is_finite() || tempo <= 0.0 {
            return;
        }
        self.tempo = tempo.clamp(1.0, 1000.0);
        for node in &mut self.nodes {
            match &mut node.unit {
                Unit::Instrument { dsp, .. } => dsp.set_tempo(self.tempo as f32),
                Unit::Effect { dsp, .. } => dsp.set_tempo(self.tempo as f32),
                _ => {}
            }
        }
    }
    pub fn set_control(&mut self, index: usize, value: f32) {
        if !value.is_finite() {
            return;
        }
        if index >= 32 {
            if let Some(Some((node, parameter))) = self.parameters.get(index - 32)
                && let Unit::Note(module) = &mut self.nodes[*node].unit
                && module.set_parameter(*parameter, value)
                && let RackNoteModule::Sequencer { params } = module
            {
                // Held generators keep their phase and ownership, but future
                // steps use the new controls on the same 64-frame grid.
                for generator in &mut self.generators {
                    if generator.active && generator.node == *node {
                        generator.params = *params;
                    }
                }
            }
            return;
        }
        for node in &mut self.nodes {
            if let Unit::Control {
                index: Some(control),
                value: stored,
            } = &mut node.unit
                && usize::from(*control) == index
            {
                *stored = value.clamp(0.0, 1.0);
            }
        }
    }
    pub fn note_on(&mut self, parent: NoteInstanceId, note: RackNote) {
        if !note.velocity.is_finite() || note.velocity <= 0.0 {
            return;
        }
        let mut len = 0;
        for index in 0..self.nodes.len() {
            if self.nodes[index].root_note {
                self.enqueue(
                    &mut len,
                    Event {
                        node: index,
                        parent,
                        note,
                        duration: f64::INFINITY,
                        transforms: 0,
                    },
                );
            }
        }
        self.dispatch(len);
    }
    fn enqueue(&mut self, len: &mut usize, event: Event) {
        if *len < self.queue.len() {
            self.queue[*len] = event;
            *len += 1;
        } else {
            self.rejected = self.rejected.saturating_add(1);
        }
    }
    fn successors(&mut self, len: &mut usize, event: Event) {
        for index in 0..self.edges.len() {
            let edge = &self.edges[index];
            if edge.from != event.node
                || edge.signal != RackSignal::Note
                || edge.port != NOTE_OUT
                    && edge.port != NOTE_CHANNEL_BASE + u16::from(event.note.channel)
            {
                continue;
            }
            let next = Event {
                node: edge.to,
                note: RackNote {
                    velocity: (event.note.velocity * edge.gain).clamp(0.0, 1.0),
                    ..event.note
                },
                ..event
            };
            self.enqueue(len, next);
        }
    }
    fn dispatch(&mut self, mut len: usize) {
        let mut at = 0;
        while at < len {
            let event = self.queue[at];
            at += 1;
            if self.nodes[event.node].muted || event.note.velocity <= 0.0 {
                continue;
            }
            match &self.nodes[event.node].unit {
                Unit::Note(RackNoteModule::Sequencer { params }) => {
                    let params = *params;
                    let clock = self
                        .generators
                        .iter()
                        .find(|generator| generator.active && generator.node == event.node)
                        .filter(|_| params.mode != SequenceMode::Sequence)
                        .map(|generator| (generator.phase, generator.step));
                    self.next_id = self.next_id.wrapping_add(1).max(1);
                    if let Some(slot) = self
                        .generators
                        .iter_mut()
                        .find(|generator| !generator.active)
                    {
                        *slot = Generator {
                            active: true,
                            parent: event.parent,
                            node: event.node,
                            note: event.note,
                            params,
                            phase: clock.map_or(0.0, |clock| clock.0),
                            step: clock.map_or(0, |clock| clock.1),
                            transforms: event.transforms,
                            admission: self.next_id,
                        };
                    } else {
                        self.rejected = self.rejected.saturating_add(1);
                    }
                }
                Unit::Note(module) => {
                    let module = *module;
                    let mut out = [RackNote::default(); 4];
                    let count = module.map(event.note, &mut out);
                    for note in out.into_iter().take(count) {
                        self.successors(
                            &mut len,
                            Event {
                                note,
                                transforms: event.transforms
                                    | if matches!(module, RackNoteModule::PropertyScaler { .. }) {
                                        1u64 << event.node
                                    } else {
                                        0
                                    },
                                ..event
                            },
                        );
                    }
                }
                Unit::Instrument { .. } => self.admit(event),
                _ => self.successors(&mut len, event),
            }
        }
    }
    fn admit(&mut self, event: Event) {
        let Some(slot) = self.owners.iter().position(|owner| !owner.active) else {
            self.rejected = self.rejected.saturating_add(1);
            return;
        };
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let child = NoteInstanceId(self.next_id);
        self.owners[slot] = Owner {
            active: true,
            parent: event.parent,
            child,
            node: event.node,
            note: event.note,
            age: 0,
            release_age: None,
            release_levels: [0.0; windfall_dsp::rack_note::MAX_NOTE_ENVELOPES],
            end: event.duration,
            transforms: event.transforms,
            pitch_offset: 0.0,
        };
        if let Unit::Instrument { dsp, .. } = &mut self.nodes[event.node].unit {
            dsp.note_on_instance(
                child,
                event.note.key,
                event.note.velocity,
                event.note.pan,
                event.note.expression,
            );
            if event
                .note
                .envelopes
                .iter()
                .flatten()
                .any(|envelope| envelope.destination == EnvelopeDestination::Gain)
            {
                dsp.set_note_gain(child, 0.0);
            } else {
                dsp.set_note_gain(child, event.note.gain);
            }
        }
    }
    pub fn note_off(&mut self, parent: NoteInstanceId) {
        for generator in &mut self.generators {
            if generator.active && generator.parent == parent {
                generator.active = false;
            }
        }
        for index in 0..self.owners.len() {
            if self.owners[index].active && self.owners[index].parent == parent {
                self.release(index);
            }
        }
    }
    fn release(&mut self, index: usize) {
        let owner = self.owners[index];
        if owner.release_age.is_some() {
            return;
        }
        if owner
            .note
            .envelopes
            .iter()
            .flatten()
            .any(|envelope| envelope.release_ms > 0.0)
        {
            self.owners[index].release_age = Some(0);
            for (shape, envelope) in owner.note.envelopes.iter().enumerate() {
                if let Some(envelope) = envelope {
                    self.owners[index].release_levels[shape] =
                        envelope.level(owner.age as f64 / self.sample_rate);
                }
            }
        } else {
            self.finish(index);
        }
    }
    fn finish(&mut self, index: usize) {
        let owner = self.owners[index];
        self.owners[index].active = false;
        let other = self.owners.iter().any(|other| {
            other.active && other.node == owner.node && other.note.key == owner.note.key
        });
        if let Unit::Instrument { dsp, .. } = &mut self.nodes[owner.node].unit
            && (dsp.supports_note_instances() || !other)
        {
            dsp.note_off_instance(owner.child, owner.note.key);
        }
    }
    fn updated_note(
        &self,
        mut note: RackNote,
        transforms: u64,
        pan: f32,
        expression: NoteExpression,
    ) -> RackNote {
        note.pan = pan.clamp(-1.0, 1.0);
        note.expression = expression.clamped();
        for &index in &self.order {
            if transforms & (1u64 << index) != 0
                && let Unit::Note(module @ RackNoteModule::PropertyScaler { params }) =
                    self.nodes[index].unit
            {
                // Velocity is fixed at admission; other properties track live curves.
                if params.property != windfall_dsp::rack_note::NoteProperty::Velocity {
                    let mut mapped = [RackNote::default(); 4];
                    module.map(note, &mut mapped);
                    note = mapped[0];
                }
            }
        }
        note
    }
    pub fn set_note_expression(
        &mut self,
        parent: NoteInstanceId,
        pan: f32,
        expression: NoteExpression,
    ) {
        for index in 0..self.owners.len() {
            let owner = self.owners[index];
            if owner.active && owner.parent == parent {
                let note = self.updated_note(owner.note, owner.transforms, pan, expression);
                self.owners[index].note = note;
                if let Unit::Instrument { dsp, .. } = &mut self.nodes[owner.node].unit {
                    dsp.set_note_expression(owner.child, note.pan, note.expression);
                    dsp.set_note_pitch(
                        owner.child,
                        f32::from(note.key)
                            + note.expression.fine_pitch_cents / 100.0
                            + owner.pitch_offset,
                    );
                }
            }
        }
        for index in 0..self.generators.len() {
            let generator = self.generators[index];
            if generator.active && generator.parent == parent {
                self.generators[index].note =
                    self.updated_note(generator.note, generator.transforms, pan, expression);
            }
        }
    }
    pub fn set_note_gain(&mut self, parent: NoteInstanceId, gain: f32) {
        if !gain.is_finite() {
            return;
        }
        let gain = gain.clamp(0.0, 4.0);
        for generator in &mut self.generators {
            if generator.active && generator.parent == parent {
                generator.note.gain = gain;
            }
        }
        for owner in &mut self.owners {
            if owner.active && owner.parent == parent {
                owner.note.gain = gain;
                if owner
                    .note
                    .envelopes
                    .iter()
                    .flatten()
                    .all(|envelope| envelope.destination != EnvelopeDestination::Gain)
                    && let Unit::Instrument { dsp, .. } = &mut self.nodes[owner.node].unit
                {
                    dsp.set_note_gain(owner.child, gain);
                }
            }
        }
    }
    pub fn set_note_pitch_offset(&mut self, parent: NoteInstanceId, offset: f32) {
        for owner in &mut self.owners {
            if owner.active && owner.parent == parent {
                owner.pitch_offset = offset;
                if let Unit::Instrument { dsp, .. } = &mut self.nodes[owner.node].unit {
                    dsp.set_note_pitch(
                        owner.child,
                        f32::from(owner.note.key)
                            + owner.note.expression.fine_pitch_cents / 100.0
                            + offset,
                    );
                }
            }
        }
    }
    pub fn all_notes_off(&mut self) {
        self.generators.fill(Generator::EMPTY);
        for index in 0..self.owners.len() {
            if self.owners[index].active {
                self.finish(index);
            }
        }
    }
    pub fn reset(&mut self) {
        self.all_notes_off();
        self.owners.fill(Owner::EMPTY);
        for node in &mut self.nodes {
            match &mut node.unit {
                Unit::Instrument { dsp, .. } => dsp.reset(),
                Unit::Effect { dsp, .. } => dsp.reset(),
                _ => {}
            }
        }
        for edge in &mut self.edges {
            edge.delay.reset();
        }
        for (_, delay) in &mut self.outputs {
            delay.reset();
        }
    }
    fn generate(&mut self) {
        for index in 0..self.generators.len() {
            let before = self.generators[index];
            if !before.active {
                continue;
            }
            let arpeggio = before.params.mode != SequenceMode::Sequence;
            if arpeggio
                && self.generators[..index]
                    .iter()
                    .any(|generator| generator.active && generator.node == before.node)
            {
                continue;
            }
            let mut phase = before.phase;
            let mut next_step = before.step;
            if phase <= 1.0e-12 {
                let step = before.params.grid.steps
                    [(before.step % u64::from(before.params.length)) as usize];
                let mut selected = before;
                let mut octave_cycle = before.step / u64::from(before.params.length);
                if arpeggio {
                    let mut held = [0usize; MAX_GENERATORS];
                    let mut count = 0;
                    for (seat, generator) in self.generators.iter().enumerate() {
                        if generator.active && generator.node == before.node {
                            held[count] = seat;
                            count += 1;
                        }
                    }
                    held[..count].sort_unstable_by_key(|&seat| {
                        let generator = self.generators[seat];
                        if before.params.mode == SequenceMode::AsPlayed {
                            (0, generator.admission)
                        } else {
                            (generator.note.key, generator.admission)
                        }
                    });
                    let ordinal = (before.step % count as u64) as usize;
                    let ordinal = if before.params.mode == SequenceMode::Down {
                        count - ordinal - 1
                    } else {
                        ordinal
                    };
                    selected = self.generators[held[ordinal]];
                    octave_cycle = before.step / count as u64;
                }
                if step.gate {
                    let octave = (octave_cycle % u64::from(before.params.octaves)) as i16 * 12;
                    let note = RackNote {
                        key: (i16::from(selected.note.key) + step.pitch_offset + octave)
                            .clamp(0, 127) as u8,
                        velocity: (selected.note.velocity * step.velocity_scale).clamp(0.0, 1.0),
                        ..selected.note
                    };
                    let mut len = 0;
                    self.successors(
                        &mut len,
                        Event {
                            node: before.node,
                            parent: selected.parent,
                            note,
                            duration: f64::from(before.params.gate)
                                / f64::from(before.params.grid.steps_per_beat),
                            transforms: selected.transforms,
                        },
                    );
                    self.dispatch(len);
                }
                next_step = before.step.wrapping_add(1);
                phase += 1.0;
            }
            phase -= self.tempo * f64::from(before.params.grid.steps_per_beat)
                / (60.0 * self.sample_rate);
            if arpeggio {
                for generator in &mut self.generators {
                    if generator.active && generator.node == before.node {
                        generator.phase = phase;
                        generator.step = next_step;
                    }
                }
            } else {
                self.generators[index].phase = phase;
                self.generators[index].step = next_step;
            }
        }
    }
    fn envelopes(&mut self) {
        for index in 0..self.owners.len() {
            let before = self.owners[index];
            if !before.active {
                continue;
            }
            if before.end <= 1.0e-12 && before.release_age.is_none() {
                self.release(index);
            }
            let owner = self.owners[index];
            if !owner.active {
                continue;
            }
            let release_frames = owner
                .note
                .envelopes
                .iter()
                .flatten()
                .map(|e| f64::from(e.release_ms) * self.sample_rate / 1000.0)
                .fold(0.0f64, f64::max);
            if let Some(age) = owner.release_age {
                if age as f64 >= release_frames {
                    self.finish(index);
                    continue;
                }
                self.owners[index].release_age = Some(age.saturating_add(1));
            }
            let mut expression = owner.note.expression;
            let mut gain = owner.note.gain;
            let mut pan = owner.note.pan;
            let mut pitch = f32::from(owner.note.key)
                + expression.fine_pitch_cents / 100.0
                + owner.pitch_offset;
            for (shape, envelope) in owner.note.envelopes.iter().enumerate() {
                let Some(envelope) = envelope else {
                    continue;
                };
                let level = if let Some(age) = owner.release_age {
                    let length = f64::from(envelope.release_ms) * self.sample_rate / 1000.0;
                    if length <= 0.0 {
                        0.0
                    } else {
                        owner.release_levels[shape] * (1.0 - age as f64 / length).max(0.0) as f32
                    }
                } else {
                    envelope.level(owner.age as f64 / self.sample_rate)
                };
                match envelope.destination {
                    EnvelopeDestination::Gain => {
                        gain *=
                            (1.0 - envelope.amount.abs() + level * envelope.amount.abs()).max(0.0)
                    }
                    EnvelopeDestination::Pan => pan += level * envelope.amount,
                    EnvelopeDestination::Pitch => pitch += level * envelope.amount * 12.0,
                    EnvelopeDestination::ModulationX => {
                        expression.modulation_x += level * envelope.amount
                    }
                    EnvelopeDestination::ModulationY => {
                        expression.modulation_y += level * envelope.amount
                    }
                }
            }
            if owner.note.envelopes.iter().any(Option::is_some)
                && let Unit::Instrument { dsp, .. } = &mut self.nodes[owner.node].unit
            {
                dsp.set_note_gain(owner.child, gain);
                dsp.set_note_expression(owner.child, pan.clamp(-1.0, 1.0), expression.clamped());
                dsp.set_note_pitch(owner.child, pitch);
            }
            self.owners[index].age = owner.age.saturating_add(1);
            self.owners[index].end = owner.end - self.tempo / (60.0 * self.sample_rate);
        }
    }
    /// Input/output may alias. The same frame loop handles live and render.
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for frame in 0..left.len().min(right.len()) {
            self.generate();
            self.envelopes();
            let input = [left[frame], right[frame]];
            let mut audio = [[0.0f32; 2]; MAX_RACK_NODES];
            let mut auxiliary = [[[0.0f32; 2]; windfall_dsp::MAX_AUX_OUTPUTS]; MAX_RACK_NODES];
            for &index in &self.order {
                let mut sample = if self.nodes[index].root_audio {
                    input
                } else {
                    [0.0; 2]
                };
                for edge in &mut self.edges {
                    if edge.to != index {
                        continue;
                    }
                    if edge.signal == RackSignal::Audio {
                        let source = if edge.port >= AUX_AUDIO_BASE {
                            auxiliary[edge.from][usize::from(edge.port - AUX_AUDIO_BASE)]
                        } else {
                            audio[edge.from]
                        };
                        let source = edge.delay.frame(source);
                        sample[0] += source[0] * edge.gain;
                        sample[1] += source[1] * edge.gain;
                    } else if edge.signal == RackSignal::Control {
                        let value = if self.nodes[edge.from].muted {
                            0.0
                        } else {
                            match self.nodes[edge.from].unit {
                                Unit::Control { value, .. } => value,
                                _ => 0.0,
                            }
                        };
                        if let Some(parameter) = edge.parameter {
                            match &mut self.nodes[index].unit {
                                Unit::Instrument { dsp, params } => {
                                    let info = &params.kind().descriptors()[parameter];
                                    let value = windfall_project::AutomationRange::of_param(info)
                                        .value((value * edge.gain).clamp(0.0, 1.0));
                                    if params.get(parameter) != Some(value) {
                                        params.set(parameter, value);
                                        dsp.set_params(params);
                                    }
                                }
                                Unit::Effect { dsp, params } => {
                                    let info = &params.kind().descriptors()[parameter];
                                    let value = windfall_project::AutomationRange::of_param(info)
                                        .value((value * edge.gain).clamp(0.0, 1.0));
                                    if params.get(parameter) != Some(value) {
                                        params.set(parameter, value);
                                        dsp.set_params(params);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                let mut l = [sample[0]];
                let mut r = [sample[1]];
                match &mut self.nodes[index].unit {
                    Unit::Instrument { dsp, .. } => {
                        l[0] = 0.0;
                        r[0] = 0.0;
                        dsp.process(&mut l, &mut r);
                    }
                    Unit::Effect { dsp, .. } => {
                        let [first, second, third] = &mut auxiliary[index];
                        let mut block = windfall_dsp::AuxBlock {
                            routed: 7,
                            outputs: [
                                std::slice::from_mut(first),
                                std::slice::from_mut(second),
                                std::slice::from_mut(third),
                            ],
                        };
                        dsp.process_routed(&mut l, &mut r, None, &mut block);
                    }
                    _ => {}
                }
                if self.nodes[index].muted {
                    l[0] = 0.0;
                    r[0] = 0.0;
                    auxiliary[index].fill([0.0; 2]);
                }
                audio[index] = [l[0], r[0]];
            }
            let mut out = [0.0f32; 2];
            for (index, delay) in &mut self.outputs {
                let sample = delay.frame(audio[*index]);
                out[0] += sample[0];
                out[1] += sample[1];
            }
            left[frame] = if out[0].is_finite() { out[0] } else { 0.0 };
            right[frame] = if out[1].is_finite() { out[1] } else { 0.0 };
            if out.iter().all(|v| v.abs() < 1e-8) {
                self.quiet_frames = self.quiet_frames.saturating_add(1);
            } else {
                self.quiet_frames = 0;
            }
        }
    }
}
