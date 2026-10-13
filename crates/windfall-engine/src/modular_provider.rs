//! Rack facades over the existing prepared builtin and native providers.
use crate::plugins::{ExternalEffect, HostedInstrument, PluginTransport};
use windfall_dsp::{
    AnyInstrument, EffectParams, EffectSlot, InstrumentParams, NoteExpression, NoteInstanceId,
};

pub(super) enum RackInstrument {
    Builtin(AnyInstrument),
    Hosted(Option<Box<dyn HostedInstrument>>),
}
impl RackInstrument {
    pub fn latency_samples(&self) -> usize {
        match self {
            Self::Builtin(dsp) => dsp.latency_samples(),
            Self::Hosted(unit) => unit.as_ref().map_or(0, |u| u.latency()),
        }
    }
    pub fn active_voices(&self) -> usize {
        match self {
            Self::Builtin(dsp) => dsp.active_voices(),
            Self::Hosted(unit) => unit.as_ref().map_or(0, |u| u.voices()),
        }
    }
    pub fn tail_samples(&self) -> usize {
        match self {
            Self::Builtin(dsp) => dsp.tail_samples(),
            Self::Hosted(unit) => unit.as_ref().map_or(0, |u| u.tail()),
        }
    }
    pub fn supports_note_instances(&self) -> bool {
        match self {
            Self::Builtin(dsp) => dsp.supports_note_instances(),
            Self::Hosted(unit) => unit.as_ref().is_some_and(|u| u.supports_note_instances()),
        }
    }
    pub fn set_tempo(&mut self, tempo: f32) {
        match self {
            Self::Builtin(dsp) => dsp.set_tempo(tempo),
            Self::Hosted(Some(unit)) => unit.set_tempo(tempo),
            _ => {}
        }
    }
    pub fn set_params(&mut self, params: &InstrumentParams) {
        if let Self::Builtin(dsp) = self {
            dsp.set_params(params);
        }
    }
    pub fn note_on_instance(
        &mut self,
        id: NoteInstanceId,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: NoteExpression,
    ) {
        match self {
            Self::Builtin(dsp) => dsp.note_on_instance(id, key, velocity, pan, expression),
            Self::Hosted(Some(unit)) => unit.note_on_instance(id, key, velocity, pan, expression),
            _ => {}
        }
    }
    pub fn note_off_instance(&mut self, id: NoteInstanceId, key: u8) {
        match self {
            Self::Builtin(dsp) => dsp.note_off_instance(id, key),
            Self::Hosted(Some(unit)) => unit.note_off_instance(id, key),
            _ => {}
        }
    }
    pub fn set_note_gain(&mut self, id: NoteInstanceId, gain: f32) {
        match self {
            Self::Builtin(dsp) => dsp.set_note_gain(id, gain),
            Self::Hosted(Some(unit)) => unit.set_note_gain(id, gain),
            _ => {}
        }
    }
    pub fn set_note_expression(
        &mut self,
        id: NoteInstanceId,
        pan: f32,
        expression: NoteExpression,
    ) {
        match self {
            Self::Builtin(dsp) => dsp.set_note_expression(id, pan, expression),
            Self::Hosted(Some(unit)) => unit.set_note_expression(id, pan, expression),
            _ => {}
        }
    }
    pub fn set_note_pitch(&mut self, id: NoteInstanceId, pitch: f32) {
        match self {
            Self::Builtin(dsp) => dsp.set_note_pitch(id, pitch),
            Self::Hosted(Some(unit)) => unit.set_note_pitch(id, pitch),
            _ => {}
        }
    }
    pub fn reset(&mut self) {
        match self {
            Self::Builtin(dsp) => dsp.reset(),
            Self::Hosted(Some(unit)) => unit.all_notes_off(),
            _ => {}
        }
    }
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        match self {
            Self::Builtin(dsp) => dsp.process(left, right),
            Self::Hosted(Some(unit)) => unit.process(left, right),
            Self::Hosted(None) => {
                left.fill(0.0);
                right.fill(0.0);
            }
        }
    }
    pub fn control_boundary(&mut self) {
        if let Self::Hosted(Some(unit)) = self {
            unit.control_boundary();
        }
    }
    pub fn transport(&mut self, transport: PluginTransport) {
        if let Self::Hosted(Some(unit)) = self {
            unit.transport(transport);
        }
    }
    pub fn is_hosted(&self) -> bool {
        matches!(self, Self::Hosted(_))
    }
    pub fn retire(&mut self) {
        if let Self::Hosted(Some(unit)) = self {
            unit.retire_from_document();
        }
    }
}
pub(super) enum RackEffect {
    Builtin(EffectSlot),
    Hosted(ExternalEffect),
}
impl RackEffect {
    pub fn latency_samples(&self) -> usize {
        match self {
            Self::Builtin(dsp) => dsp.latency_samples(),
            Self::Hosted(dsp) => dsp.latency(),
        }
    }
    pub fn tail_samples(&self) -> usize {
        match self {
            Self::Builtin(dsp) => dsp.tail_samples(),
            Self::Hosted(dsp) => dsp.tail(),
        }
    }
    pub fn set_tempo(&mut self, tempo: f32) {
        match self {
            Self::Builtin(dsp) => dsp.set_tempo(tempo),
            Self::Hosted(dsp) => {
                if let Some(unit) = &mut dsp.unit {
                    unit.set_tempo(tempo);
                }
            }
        }
    }
    pub fn set_params(&mut self, params: &EffectParams) {
        if let Self::Builtin(dsp) = self {
            dsp.set_params(params);
        }
    }
    pub fn reset(&mut self) {
        if let Self::Builtin(dsp) = self {
            dsp.reset();
        }
    }
    pub fn process_routed(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
        aux: &mut windfall_dsp::AuxBlock<'_>,
    ) {
        match self {
            Self::Builtin(dsp) => dsp.process_routed(left, right, key, aux),
            Self::Hosted(dsp) => dsp.process_sidechain(left, right, key),
        }
    }
    pub fn control_boundary(&mut self) {
        if let Self::Hosted(dsp) = self {
            if let Some(unit) = &mut dsp.unit {
                unit.control_boundary();
            }
        }
    }
    pub fn transport(&mut self, transport: PluginTransport) {
        if let Self::Hosted(dsp) = self {
            if let Some(unit) = &mut dsp.unit {
                unit.transport(transport);
            }
        }
    }
    pub fn is_hosted(&self) -> bool {
        matches!(self, Self::Hosted(_))
    }
    pub fn retire(&mut self) {
        if let Self::Hosted(dsp) = self {
            if let Some(unit) = &mut dsp.unit {
                unit.retire_from_document();
            }
        }
    }
}
