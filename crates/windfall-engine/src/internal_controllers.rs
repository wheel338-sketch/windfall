//! Engine-owned control sources. Fixed 64-frame link grid; sample-rate source
//! evaluation at the owning slot, on its real input. No callback allocation.
use crate::{
    automation::{self, Target},
    plan::Plan,
    sequencer::Clock,
    state::PlanState,
    voice::{Note, Origin},
};
use windfall_dsp::control::{
    EnvelopeFollower, FormulaSource, KeyboardControl, KeyboardSource, NoteEnvelope, VoltageSource,
    XyPad, XyzPad,
};
use windfall_dsp::{Effect, ParamSet};
use windfall_project::controller_links::CompiledMap;
use windfall_project::{
    AutomationRange, ControlSource, EffectId, EffectParams, InternalControllerSettings, LinkTarget,
    Project,
};

#[derive(Debug)]
pub(crate) struct Source {
    pub id: EffectId,
    pub track: usize,
    pub channel: Option<usize>,
    pub note_channel: Option<usize>,
    pub config: InternalControllerSettings,
    pub params: EffectParams,
}

pub(crate) struct Link {
    source: usize,
    output: usize,
    target: Target,
    range: AutomationRange,
    map: CompiledMap,
    smoothing_ms: f32,
    initial: f32,
    key: windfall_project::ControllerLink,
}

impl std::fmt::Debug for Link {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Link").field("key", &self.key).finish()
    }
}

pub(crate) fn compile(project: &Project, plan: &Plan) -> (Vec<Source>, Vec<Link>) {
    // Invalid externally supplied projects cannot sneak past the checked document.
    if windfall_project::internal_controllers::settings_problem(
        project,
        &project.settings.internal_controllers,
    )
    .is_some()
        || windfall_project::internal_controllers::links_problem(
            project,
            &project.settings.controller_links,
        )
        .is_some()
    {
        return (Vec::new(), Vec::new());
    }
    let mut sources = Vec::new();
    for (track, chain) in plan.tracks.iter().enumerate() {
        for effect in &chain.effects {
            if effect.leaving
                || windfall_project::internal_controllers::outputs(effect.params.kind()).is_empty()
            {
                continue;
            }
            let config = project
                .settings
                .internal_controllers
                .iter()
                .find(|c| c.controller == effect.id.0)
                .cloned()
                .unwrap_or_default();
            let note_channel = config
                .note_channel
                .and_then(|id| plan.channel_ids.get(id.0));
            sources.push(Source {
                id: effect.id,
                track,
                channel: None,
                note_channel,
                config,
                params: effect.params,
            });
        }
    }
    for (index, channel) in plan.channels.iter().enumerate() {
        let Some(windfall_project::InstrumentParams::Dashboard(params)) = channel.instrument else {
            continue;
        };
        let config = project
            .settings
            .internal_controllers
            .iter()
            .find(|c| c.controller == channel.id.0)
            .cloned()
            .unwrap_or_default();
        sources.push(Source {
            id: EffectId(channel.id.0),
            track: channel.track,
            channel: Some(index),
            note_channel: None,
            config,
            params: dashboard_params(params),
        });
    }
    let mut links = Vec::new();
    for link in &project.settings.controller_links {
        let ControlSource::Internal { controller, output } = link.source else {
            continue;
        };
        let LinkTarget::Param { target: key } = link.target else {
            continue;
        };
        let Some(source) = sources.iter().position(|s| s.id.0 == controller) else {
            continue;
        };
        let Some(target) = automation::resolve(&key, plan) else {
            continue;
        };
        let Some(range) = project.automation_range(&key) else {
            continue;
        };
        let Some(value) = project.automation_stored_value(&key) else {
            continue;
        };
        let Ok(map) = link.map.compile() else {
            continue;
        };
        links.push(Link {
            source,
            output: usize::from(output),
            target,
            range,
            map,
            smoothing_ms: link.map.smoothing_ms,
            initial: range.normalized(value),
            key: link.clone(),
        });
    }
    (sources, links)
}

enum Dsp {
    Envelope(NoteEnvelope),
    Peak(EnvelopeFollower),
    Keyboard(KeyboardSource),
    Formula(FormulaSource),
    Xy(XyPad),
    Xyz(XyzPad),
    Surface,
    Voltage(VoltageSource),
}

#[derive(Clone, Copy)]
struct Held {
    key: u8,
    velocity: f32,
    end_tick: f64,
    started: u64,
    channel: usize,
    origin: Origin,
}

pub(crate) struct Node {
    dsp: Dsp,
    params: EffectParams,
    held: [Option<Held>; 128],
    values: [f32; 8],
    sync_rate: Option<f32>,
    drawn: f32,
    gate: bool,
    rate: f32,
    pub rejected_notes: u32,
    pub dc: [[f32; 8]; 64],
}

impl Node {
    fn new(source: &Source, rate: f32) -> Self {
        let dsp = match source.params {
            EffectParams::NoteEnvelope(p) => {
                let mut d = NoteEnvelope::default();
                d.prepare(rate, 64);
                d.set_params(&p);
                Dsp::Envelope(d)
            }
            EffectParams::EnvelopeFollower(p) => {
                let mut d = EnvelopeFollower::default();
                d.prepare(rate, 64);
                d.set_params(&p);
                Dsp::Peak(d)
            }
            EffectParams::KeyboardSource(p) => {
                let mut d = KeyboardSource::default();
                d.prepare(rate, 64);
                d.set_params(&p);
                Dsp::Keyboard(d)
            }
            EffectParams::FormulaSource(p) => {
                let mut d = FormulaSource::default();
                d.prepare(rate, 64);
                d.set_params(&p);
                if let Some(text) = &source.config.expression {
                    let _ = d.set_expression(text);
                }
                Dsp::Formula(d)
            }
            EffectParams::XyPad(p) => {
                let mut d = XyPad::default();
                d.prepare(rate, 64);
                d.set_params(&p);
                Dsp::Xy(d)
            }
            EffectParams::XyzPad(p) => {
                let mut d = XyzPad::default();
                d.prepare(rate, 64);
                d.set_params(&p);
                Dsp::Xyz(d)
            }
            EffectParams::VoltageSource(p) => {
                let mut d = VoltageSource::default();
                d.prepare(rate, 64);
                d.set_params(&p);
                Dsp::Voltage(d)
            }
            _ => Dsp::Surface,
        };
        let mut values = [0.0; 8];
        match source.params {
            EffectParams::XyPad(p) => values[..2].copy_from_slice(&[p.x, p.y]),
            EffectParams::XyzPad(p) => values[..3].copy_from_slice(&[p.x, p.y, p.z]),
            EffectParams::ControlSurface(p) => {
                for (i, value) in values.iter_mut().enumerate() {
                    *value = p.get(i).unwrap_or(0.0);
                }
            }
            EffectParams::KeyboardSource(p) => values[0] = f32::from(p.root_key) / 127.0,
            _ => {}
        }
        Self {
            dsp,
            params: source.params,
            held: [None; 128],
            values,
            sync_rate: None,
            drawn: 0.0,
            gate: false,
            rate,
            rejected_notes: 0,
            dc: [[0.0; 8]; 64],
        }
    }

    pub fn note_on(&mut self, note: Note, frame: u64) {
        if note.velocity <= 0.0 || !matches!(self.dsp, Dsp::Envelope(_) | Dsp::Keyboard(_)) {
            return;
        }
        let entry = Held {
            key: note.key,
            velocity: note.velocity,
            end_tick: note.end,
            started: frame,
            channel: note.channel,
            origin: note.origin,
        };
        if let Some(slot) = self.held.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(entry);
        } else {
            self.rejected_notes = self.rejected_notes.saturating_add(1);
        }
    }
    pub fn note_off(&mut self, channel: usize, key: u8, origin: Origin) {
        for held in &mut self.held {
            if held.is_some_and(|n| n.channel == channel && n.key == key && n.origin == origin) {
                *held = None;
            }
        }
    }
    pub fn end_sequenced(&mut self) {
        for held in &mut self.held {
            if held.is_some_and(|n| n.origin == Origin::Sequenced) {
                *held = None;
            }
        }
        if let Dsp::Peak(d) = &mut self.dsp {
            d.reset();
        }
        self.drawn = 0.0;
        self.gate = false;
        if let (Dsp::Envelope(d), EffectParams::NoteEnvelope(params)) = (&mut self.dsp, self.params)
        {
            d.set_params(&params);
            d.reset();
        }
    }

    /// Same timeline for all callback partitions, including notes inside a grid.
    fn process(
        &mut self,
        source: &Source,
        params: EffectParams,
        left: &[f32],
        right: &[f32],
        base: u64,
        clock: Clock,
        tempo: f64,
    ) {
        for (offset, (&left, &right)) in left.iter().zip(right).enumerate() {
            let now = base + offset as u64;
            for held in &mut self.held {
                if held.is_some_and(|n| n.started <= now && clock.frame_of(n.end_tick) <= now) {
                    *held = None;
                }
            }
            let note = self
                .held
                .iter()
                .flatten()
                .filter(|n| n.started <= now)
                .max_by_key(|n| n.started)
                .copied();
            let gate = note.is_some();
            let retrigger = note.is_some_and(|n| n.started == now);
            match (&mut self.dsp, params) {
                (Dsp::Envelope(d), EffectParams::NoteEnvelope(mut p)) => {
                    p.trigger |= gate;
                    if params != self.params || p.trigger != self.gate {
                        d.set_params(&p);
                    }
                    self.gate = p.trigger;
                    if retrigger {
                        d.trigger();
                    }
                    let mut out = [0.0];
                    d.process_control(&mut out);
                    self.values[0] = out[0];
                    if !source.config.envelope.is_empty() {
                        if let Some(note) = note {
                            let position = ((now - note.started) as f32
                                / (source.config.duration_ms * self.rate / 1000.0))
                                .clamp(0.0, 1.0);
                            self.drawn = envelope_at(&source.config.envelope, position);
                        } else if p.release_ms <= 0.0 {
                            self.drawn = 0.0;
                        } else {
                            self.drawn *= (-1000.0 / (p.release_ms * self.rate)).exp();
                        }
                        self.values[0] = self.drawn;
                    }
                }
                (Dsp::Keyboard(d), EffectParams::KeyboardSource(p)) => {
                    if params != self.params {
                        d.set_params(&p);
                    }
                    let (key, velocity) = note.map_or((p.root_key, 0.0), |n| (n.key, n.velocity));
                    d.set_note(key, velocity);
                    let mut out = [KeyboardControl::default()];
                    d.process_control(&mut out);
                    self.values[0] =
                        ((out[0].pitch_semitones + f32::from(p.root_key)) / 127.0).clamp(0.0, 1.0);
                    self.values[1] = (1.0 + out[0].gain_offset).clamp(0.0, 1.0);
                }
                (Dsp::Peak(d), EffectParams::EnvelopeFollower(p)) => {
                    if params != self.params {
                        d.set_params(&p);
                    }
                    let sync_rate = source
                        .config
                        .sync_beats
                        .map(|beats| tempo as f32 / (60.0 * beats));
                    if sync_rate != self.sync_rate {
                        d.set_sync_rate(sync_rate);
                        self.sync_rate = sync_rate;
                    }
                    let mut out = [0.0];
                    if source.config.rms {
                        d.process_control_rms(&[left], &[right], &mut out);
                    } else {
                        d.process_control(&[left], &[right], &mut out);
                    }
                    self.values[0] = out[0];
                    self.values[1] = d.lfo_value();
                }
                (Dsp::Formula(d), EffectParams::FormulaSource(p)) => {
                    if params != self.params {
                        d.set_params(&p);
                    }
                    self.values[0] = d.next_value();
                }
                (Dsp::Xy(d), EffectParams::XyPad(p)) => {
                    if params != self.params {
                        d.set_params(&p);
                    }
                    let mut out = [[0.0; 2]];
                    d.process_control(&mut out);
                    self.values[..2].copy_from_slice(&out[0]);
                }
                (Dsp::Xyz(d), EffectParams::XyzPad(p)) => {
                    if params != self.params {
                        d.set_params(&p);
                    }
                    let mut out = [[0.0; 3]];
                    d.process_control(&mut out);
                    self.values[..3].copy_from_slice(&out[0]);
                }
                (Dsp::Surface, EffectParams::ControlSurface(p)) => {
                    for (i, v) in self.values.iter_mut().enumerate() {
                        *v = p.get(i).unwrap_or(0.0);
                    }
                }
                (Dsp::Voltage(d), EffectParams::VoltageSource(p)) => {
                    if params != self.params {
                        d.set_params(&p);
                    }
                    self.values = d.next_values();
                    for route in &source.config.voltage_routes {
                        self.dc[offset][usize::from(route.output)] = if p.armed {
                            VoltageSource::calibrated(
                                self.values[usize::from(route.output)],
                                route.zero,
                                route.scale,
                            )
                        } else {
                            0.0
                        };
                    }
                }
                _ => {}
            }
            self.params = params;
        }
    }
}

fn dashboard_params(params: windfall_dsp::MidiOutParams) -> EffectParams {
    let mut surface = windfall_dsp::surface::ControlSurfaceParams::default();
    for (index, controller) in params.controllers.iter().enumerate() {
        surface.set(index, controller.value as f32 / 127.0);
    }
    EffectParams::ControlSurface(surface)
}

fn envelope_at(points: &[windfall_project::ControlPoint], position: f32) -> f32 {
    let to = points.partition_point(|p| p.position <= position);
    if to == 0 {
        return points[0].value;
    }
    if to == points.len() {
        return points[to - 1].value;
    }
    let a = points[to - 1];
    let b = points[to];
    a.value + (b.value - a.value) * ((position - a.position) / (b.position - a.position))
}

pub(crate) struct Runtime {
    pub nodes: Vec<Node>,
    values: Vec<f32>,
    rate: f32,
    pub clock: Clock,
}
impl Runtime {
    pub fn new(plan: &Plan, rate: u32) -> Self {
        Self {
            nodes: plan
                .control_sources
                .iter()
                .map(|s| Node::new(s, rate as f32))
                .collect(),
            values: plan.control_links.iter().map(|l| l.initial).collect(),
            rate: rate as f32,
            clock: Clock::control_default(rate, plan.tempo_bpm),
        }
    }
    pub fn process_slot(
        &mut self,
        plan: &Plan,
        id: EffectId,
        params: EffectParams,
        left: &[f32],
        right: &[f32],
        base: u64,
        tempo: f64,
    ) {
        for (i, source) in plan.control_sources.iter().enumerate() {
            if source.id == id {
                self.nodes[i].process(source, params, left, right, base, self.clock, tempo);
            }
        }
    }
    pub fn silence(&mut self, plan: &Plan, id: EffectId) {
        for (index, source) in plan.control_sources.iter().enumerate() {
            if source.id == id {
                self.nodes[index].values.fill(0.0);
                self.nodes[index].dc.fill([0.0; 8]);
            }
        }
    }
    pub fn dashboard(
        &mut self,
        plan: &Plan,
        channel: usize,
        params: windfall_project::InstrumentParams,
        base: u64,
    ) {
        let windfall_project::InstrumentParams::Dashboard(params) = params else {
            return;
        };
        for (i, source) in plan.control_sources.iter().enumerate() {
            if source.channel == Some(channel) {
                self.nodes[i].process(
                    source,
                    dashboard_params(params),
                    &[0.0],
                    &[0.0],
                    base,
                    self.clock,
                    plan.tempo_bpm,
                );
            }
        }
    }
    pub fn note_on(&mut self, plan: &Plan, note: Note, frame: u64) {
        for (i, source) in plan.control_sources.iter().enumerate() {
            if source
                .note_channel
                .map_or(plan.channels[note.channel].track == source.track, |c| {
                    c == note.channel
                })
            {
                self.nodes[i].note_on(note, frame);
            }
        }
    }
    pub fn note_off(&mut self, channel: usize, key: u8, origin: Origin) {
        for node in &mut self.nodes {
            node.note_off(channel, key, origin);
        }
    }
    pub fn move_ends(&mut self, moved: &impl Fn(f64) -> f64) {
        for node in &mut self.nodes {
            for note in node.held.iter_mut().flatten() {
                if note.end_tick.is_finite() {
                    note.end_tick = moved(note.end_tick);
                }
            }
        }
    }
    pub fn release_origin(&mut self, origin: Origin) {
        for node in &mut self.nodes {
            for note in &mut node.held {
                if note.is_some_and(|n| n.origin == origin) {
                    *note = None;
                }
            }
        }
    }
    pub fn end_sequenced(&mut self) {
        for node in &mut self.nodes {
            node.end_sequenced();
        }
    }
    pub fn take_over(&mut self, plan: &Plan, old: &mut Self, old_plan: &Plan) {
        for (at, source) in plan.control_sources.iter().enumerate() {
            if let Some(before) = old_plan.control_sources.iter().position(|s| {
                s.id == source.id
                    && s.params.kind() == source.params.kind()
                    && s.config == source.config
            }) {
                std::mem::swap(&mut self.nodes[at], &mut old.nodes[before]);
                for held in &mut self.nodes[at].held {
                    if let Some(note) = held {
                        if let Some(channel) =
                            plan.channel_ids.get(old_plan.channels[note.channel].id.0)
                        {
                            note.channel = channel;
                        } else {
                            *held = None;
                        }
                    }
                }
            }
        }
        for (at, link) in plan.control_links.iter().enumerate() {
            if let Some(before) = old_plan
                .control_links
                .iter()
                .position(|l| l.key == link.key)
            {
                self.values[at] = old.values[before];
            }
        }
    }
}

/// Links override ordinary automation deterministically in stored list order.
/// The source's preceding grid endpoint drives the following grid with smoothing.
pub(crate) fn step(plan: &Plan, state: &mut PlanState, now: u64) {
    for (index, link) in plan.control_links.iter().enumerate() {
        let raw = state.controls.nodes[link.source].values[link.output];
        let target = link.map.shape(raw);
        let alpha = if link.smoothing_ms <= 0.0 {
            1.0
        } else {
            1.0 - (-64_000.0 / (link.smoothing_ms * state.controls.rate)).exp()
        };
        let value = state.controls.values[index] + (target - state.controls.values[index]) * alpha;
        state.controls.values[index] = value;
        automation::set(
            plan,
            state,
            link.target,
            link.range.value(value),
            now,
            automation::GRID,
        );
    }
}

#[cfg(test)]
mod tests;
