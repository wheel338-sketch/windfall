//! What the engine hosts on the audio thread: effects, instruments and the
//! delays that keep paths of different latency lined up.
//!
//! Each of these owns heap memory. The control side builds and prepares
//! them, the audio thread only ever moves them from one
//! [`PlanState`](crate::state::PlanState) to the next, and they travel back
//! inside the state they were last in to be dropped off the audio thread.

use windfall_dsp::blocks::tap_crossfade::TapCrossfade;
use windfall_dsp::{AnyEffect, AnyInstrument, EffectSlot, GainReductionMeter, NoteArticulation, NoteExpression, NoteInstanceId};
use windfall_project::{EffectKind, EffectParams, InstrumentKind, InstrumentParams, TrackId};

use crate::mixer::{Frame, MAX_BLOCK};
use crate::plan::{EffectLife, PlanEffect, Plan};
use crate::sequencer::Clock;
use crate::voice::Origin;
use crate::note_glide::NoteGlide;

/// Logical note owners, allocated before entering the audio callback.
const MAX_HELD_NOTES: usize = 1024;

/// How an effect is joining or leaving the chain it sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Splice {
    /// Part of the chain.
    Steady,
    /// Coming in. For `wait` frames the effect runs unheard, and then it
    /// fades in from the signal that enters it over `remaining` frames.
    In { wait: u32, remaining: u32 },
    /// Fading out to the signal that enters it, with this many frames to go.
    Out(u32),
    /// Faded out. The signal passes as if the effect were not there.
    Gone,
}

impl Splice {
    /// The share of the effect's output in the next frame of a splice that
    /// takes `length` frames, and moves the splice on by that frame.
    fn next(&mut self, length: f32) -> f32 {
        match self {
            Splice::Steady => 1.0,
            Splice::Gone => 0.0,
            Splice::In { wait, .. } if *wait > 0 => {
                *wait -= 1;
                0.0
            }
            Splice::In { remaining, .. } => {
                let wet = 1.0 - *remaining as f32 / length;
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    *self = Splice::Steady;
                }
                wet
            }
            Splice::Out(remaining) => {
                let wet = *remaining as f32 / length;
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    *self = Splice::Gone;
                }
                wet
            }
        }
    }
}

/// One effect of a mixer track's chain.
///
/// The on/off switch and the mix belong to the dsp crate's slot, which
/// crossfades them and delays the dry signal to match. The splice on top of
/// that is the engine's own: an effect that joins a chain sound is passing
/// through fades in from the signal as it enters, not delayed, and an effect
/// that leaves fades out to it. One with latency first runs unheard for as
/// long as its latency, so that by the time it fades in, what it puts out
/// is the signal it was given and not the silence, or the other track's
/// sound, that was in its delay. Without all that a new limiter would open
/// with a hole as long as its look-ahead, and a removed reverb would cut
/// its tail.
pub(crate) struct EffectUnit {
    external: Option<crate::plugins::ExternalEffect>,
    slot: EffectSlot,
    /// What the slot was last given.
    params: EffectParams,
    enabled: bool,
    mix: f32,
    splice: Splice,
    /// Length of the splice under way, in frames.
    splice_frames: u32,
    /// Frames already supplied during the current insertion/removal splice.
    splice_elapsed: u32,
    /// Removal starts at the audible insertion weight, even mid fade-in.
    splice_scale: f32,
    /// The track whose chain the effect was last part of.
    pub track: TrackId,
    sample_rate: f32,
    life: std::sync::Arc<EffectLife>,
    /// This particular prepared owner has actually had an audible splice.
    heard: bool,
}

impl EffectUnit {
    /// Builds and prepares an effect with its settings in force from its
    /// first frame. Allocates, so it is for the control side. Also returns
    /// the meter of a compressor or limiter.
    pub fn build(
        effect: &PlanEffect,
        track: TrackId,
        sample_rate: u32,
        tempo_bpm: f64,
    ) -> (Self, Option<GainReductionMeter>) {
        let processor = AnyEffect::new(&effect.params);
        let meter = processor.gain_reduction();
        let mut slot = EffectSlot::new(processor);
        slot.prepare(sample_rate as f32, MAX_BLOCK);
        slot.set_tempo(tempo_bpm as f32);
        slot.set_params(&effect.params);
        slot.set_enabled(effect.enabled);
        slot.set_mix(effect.mix);
        let unit = Self {
            external: None,
            slot,
            params: effect.params,
            enabled: effect.enabled,
            mix: effect.mix,
            splice: Splice::Steady,
            splice_frames: 1,
            splice_elapsed: 0,
            splice_scale: 1.0,
            track,
            sample_rate: sample_rate as f32,
            life: effect.life.clone(),
            heard: false,
        };
        (unit, meter)
    }

    pub fn kind(&self) -> EffectKind {
        self.params.kind()
    }

    pub fn install_plugin(
        &mut self,
        unit: Option<Box<dyn crate::plugins::HostedEffect>>,
        binding: &windfall_project::PluginBinding,
    ) {
        self.external = Some(crate::plugins::ExternalEffect::new(
            unit,
            binding,
            self.sample_rate as u32,
            self.enabled,
            self.mix,
        ));
    }

    pub fn apply_plugin(&mut self, binding: &windfall_project::PluginBinding) {
        if let Some(external) = &mut self.external {
            external.apply(binding);
        }
    }
    pub fn plugin_control_boundary(&mut self) {
        if let Some(external) = &mut self.external
            && let Some(unit) = &mut external.unit
        {
            unit.control_boundary();
        }
    }
    pub fn plugin_transport(&mut self, transport: crate::plugins::PluginTransport) {
        if let Some(external) = &mut self.external
            && let Some(unit) = &mut external.unit
        {
            unit.transport(transport);
        }
    }

    /// Follows the plan: whatever differs from what the effect was last
    /// given is handed to it, and it glides there.
    pub fn apply(&mut self, effect: &PlanEffect) {
        if let Some(external) = &mut self.external {
            self.enabled = effect.enabled;
            self.mix = effect.mix;
            external.set_mix(effect.enabled, effect.mix);
            return;
        }
        if effect.params != self.params && self.slot.set_params(&effect.params) {
            self.params = effect.params;
            if let Splice::In { wait, .. } = &mut self.splice
                && *wait > 0
            {
                let ready = u32::try_from(self.slot.effect().warm_up_samples()).unwrap_or(u32::MAX);
                *wait = (*wait).max(ready.saturating_sub(self.splice_elapsed));
                let transition = self.slot.effect().latency_transition_samples_remaining();
                *wait = (*wait).max(u32::try_from(transition).unwrap_or(u32::MAX));
            }
        }
        if effect.enabled != self.enabled {
            self.enabled = effect.enabled;
            self.slot.set_enabled(effect.enabled);
        }
        if effect.mix != self.mix {
            self.mix = effect.mix;
            self.slot.set_mix(effect.mix);
        }
    }

    pub fn set_tempo(&mut self, tempo_bpm: f64) {
        if let Some(external) = &mut self.external {
            if let Some(unit) = &mut external.unit {
                unit.set_tempo(tempo_bpm as f32);
            }
            return;
        }
        self.slot.set_tempo(tempo_bpm as f32);
    }

    /// Moves one setting, as automation does: the effect glides to it the
    /// way it does when the plan changes it. The settings the plan gave
    /// stay what they are, so [`EffectUnit::apply`] takes this back.
    ///
    /// A setting that changes how late the effect puts its output out, the
    /// limiter's look-ahead, is left alone: the delays that line the other
    /// paths up with the effect are made for the plan's value.
    pub fn automate(&mut self, param: usize, value: f32) {
        if let Some(external) = &mut self.external {
            external.automate(param, value);
            return;
        }
        let mut params = self.params;
        if !params.set(param, value) || params == self.params {
            return;
        }
        let rate = self.sample_rate;
        if params.latency_samples(rate) != self.params.latency_samples(rate) {
            return;
        }
        if self.slot.set_params(&params) {
            self.params = params;
        }
    }

    /// Moves the slot's mix, as automation does.
    pub fn automate_mix(&mut self, mix: f32) {
        if let Some(external) = &mut self.external {
            self.mix = mix.clamp(0.0, 1.0);
            external.set_mix(self.enabled, self.mix);
            return;
        }
        let mix = mix.clamp(0.0, 1.0);
        if mix != self.mix {
            self.mix = mix;
            self.slot.set_mix(mix);
        }
    }

    /// Brings the effect in: unheard until all outputs are ready, then
    /// fading in over `frames` frames. Matrix readiness is its longest
    /// channel delay; shared PDC remains its shortest. Returns the wait.
    pub fn fade_in(&mut self, frames: u32) -> u32 {
        let wait = u32::try_from(self.external.as_ref().map_or_else(
            || self.slot.effect().warm_up_samples(),
            |slot| slot.latency(),
        ))
        .unwrap_or(u32::MAX);
        self.splice_frames = frames.max(1);
        self.splice_elapsed = 0;
        self.splice_scale = 1.0;
        self.splice = Splice::In {
            wait,
            remaining: self.splice_frames,
        };
        wait
    }

    /// A later plan can edit a processor that is still being primed. New
    /// compensation transitions must share its remaining insertion wait.
    pub fn insertion_wait(&self) -> u32 {
        match self.splice {
            Splice::In { wait, .. } => wait,
            _ => 0,
        }
    }

    pub fn removal_remaining(&self) -> u32 {
        match self.splice {
            Splice::Out(remaining) => remaining,
            _ => 0,
        }
    }

    pub fn is_departure_source(&self) -> bool {
        self.heard && !matches!(self.splice, Splice::Gone)
    }

    pub fn owns_generation(&self, life: &std::sync::Arc<EffectLife>) -> bool {
        std::sync::Arc::ptr_eq(&self.life, life)
    }

    pub fn generation(&self) -> u64 {
        self.life.generation
    }

    /// Prime with the post-departure input before joining a restored id.
    pub fn wait_for_departure(&mut self, remaining: u32) {
        if let Splice::In { wait, .. } = &mut self.splice {
            *wait = wait.saturating_add(remaining);
        }
    }

    /// Starts fading the effect out over `frames` frames.
    pub fn fade_out(&mut self, frames: u32) {
        if matches!(self.splice, Splice::Gone | Splice::Out(_)) {
            return;
        }
        if !self.heard {
            self.drop_out();
            return;
        }
        self.splice_scale = match self.splice {
            Splice::In { wait: 0, remaining } => 1.0 - remaining as f32 / self.splice_frames as f32,
            Splice::In { .. } => 0.0,
            _ => 1.0,
        };
        self.splice_frames = frames.max(1);
        self.splice = Splice::Out(self.splice_frames);
    }

    /// Takes the effect out of the signal at once.
    pub fn drop_out(&mut self) {
        self.splice = Splice::Gone;
        self.life.finish();
    }

    /// Samples the effect can go on sounding for after its input stops.
    pub fn tail_samples(&self) -> usize {
        match self.splice {
            Splice::Gone => 0,
            Splice::Steady | Splice::In { .. } | Splice::Out(_) => self
                .external
                .as_ref()
                .map_or_else(|| self.slot.tail_samples(), |slot| slot.tail()),
        }
    }

    /// Samples for which the effect's output can fall silent and still
    /// come back with nothing new going in.
    pub fn gap_samples(&self) -> usize {
        match self.splice {
            Splice::Gone => 0,
            Splice::Steady | Splice::In { .. } | Splice::Out(_) => self
                .external
                .as_ref()
                .map_or_else(|| self.slot.gap_samples(), |slot| slot.tail()),
        }
    }

    /// Processes one block in place. `dry_left` and `dry_right` are scratch
    /// space at least as long as the block.
    #[cfg(test)]
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32], dry_left: &mut [f32], dry_right: &mut [f32]) {
        self.process_sidechain(left, right, dry_left, dry_right, None);
    }

    pub fn process_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        dry_left: &mut [f32],
        dry_right: &mut [f32],
        key: Option<&[[f32; 2]]>,
    ) {
        match self.splice {
            Splice::Steady => {
                self.heard = true;
                self.life.hear();
                self.process_slot(left, right, key);
                return;
            }
            Splice::Gone => return,
            Splice::In { .. } | Splice::Out(_) => {}
        }
        let frames = left.len();
        let (dry_left, dry_right) = (&mut dry_left[..frames], &mut dry_right[..frames]);
        dry_left.copy_from_slice(left);
        dry_right.copy_from_slice(right);
        self.process_slot(left, right, key);

        let length = self.splice_frames as f32;
        for index in 0..frames {
            // Once the effect is all the way in, its output is left as it is.
            if self.splice == Splice::Steady {
                break;
            }
            let wet = self.splice.next(length) * self.splice_scale;
            if wet > 0.0 {
                self.heard = true;
                self.life.hear();
            }
            self.splice_elapsed = self.splice_elapsed.saturating_add(1);
            left[index] = dry_left[index] + (left[index] - dry_left[index]) * wet;
            right[index] = dry_right[index] + (right[index] - dry_right[index]) * wet;
        }
        if self.splice == Splice::Gone {
            self.life.finish();
        }
    }

    fn process_slot(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        if let Some(external) = &mut self.external {
            external.process_sidechain(left, right, key);
        } else {
            self.slot.process_sidechain(left, right, key);
        }
    }
}

/// The instrument of one channel, with the notes it is holding.
///
/// An instrument takes notes with no position in time, so the unit is
/// rendered up to the frame of every note-on and note-off, which puts each
/// on its exact frame. A held note keeps the tick it ends on, not the
/// frame, so a tempo change moves the end of a sounding note the way it
/// does for a sampler voice.
#[derive(Clone, Copy)]
struct HeldInstrumentNote {
    channel_envelopes: Option<crate::channel_voice::PreparedChannelEnvelopes>,
    channel_pitch: f64,
    source: Option<crate::note_curves::CurveSource>,
    active: bool,
    id: NoteInstanceId,
    key: u8,
    velocity: f32,
    pan: f32,
    expression: NoteExpression,
    end: f64,
    origin: Origin,
    pitch: f64,
    glide: Option<NoteGlide>,
}

impl HeldInstrumentNote {
    const EMPTY: Self = Self { channel_envelopes: None, channel_pitch: 0.0, source: None, active: false, id: NoteInstanceId(0), key: 0, velocity: 0.0,
        pan: 0.0, expression: NoteExpression::NEUTRAL, end: f64::INFINITY, origin: Origin::Sequenced, pitch: 0.0, glide: None };
}

pub(crate) struct InstrumentUnit {
    voice_envelopes: Option<crate::channel_voice::PreparedChannelEnvelopes>,
    voice_glide: f64,
    external: Option<Option<Box<dyn crate::plugins::HostedInstrument>>>,
    plugin_params: Box<[(u32, f32)]>,
    instrument: AnyInstrument,
    params: InstrumentParams,
    /// Separate end, origin and expression for every sounding occurrence.
    notes: Box<[HeldInstrumentNote]>,
    next_instance: u64,
    /// Active logical note owners.
    held: usize,
    /// The output of the block being processed, a side each.
    left: Box<[f32]>,
    right: Box<[f32]>,
    /// One past the last frame on which the output was not silent.
    last_sound: u64,
}

impl InstrumentUnit {
    /// Builds and prepares an instrument. Allocates, so it is for the
    /// control side.
    pub fn build(params: &InstrumentParams, sample_rate: u32, tempo_bpm: f64) -> Self {
        let mut instrument = AnyInstrument::new(params);
        instrument.prepare(sample_rate as f32, MAX_BLOCK);
        instrument.set_tempo(tempo_bpm as f32);
        instrument.set_params(params);
        Self {
            voice_envelopes: None, voice_glide: 1.0,
            external: None,
            plugin_params: Box::new([]),
            instrument,
            params: *params,
            notes: vec![HeldInstrumentNote::EMPTY; MAX_HELD_NOTES].into_boxed_slice(),
            next_instance: 0,
            held: 0,
            left: vec![0.0; MAX_BLOCK].into_boxed_slice(),
            right: vec![0.0; MAX_BLOCK].into_boxed_slice(),
            last_sound: 0,
        }
    }

    pub fn configure_voice(&mut self, envelopes: crate::channel_voice::PreparedChannelEnvelopes, glide: f64) {
        self.voice_envelopes = envelopes.enabled().then_some(envelopes); self.voice_glide = glide;
    }

    pub fn kind(&self) -> InstrumentKind {
        self.params.kind()
    }

    pub fn install_plugin(
        &mut self,
        unit: Option<Box<dyn crate::plugins::HostedInstrument>>,
        binding: &windfall_project::PluginBinding,
    ) {
        self.external = Some(unit);
        self.plugin_params = binding
            .parameters
            .iter()
            .map(|param| (param.id, f32::NAN))
            .collect();
        self.apply_plugin(binding);
    }
    pub fn plugin_control_boundary(&mut self) {
        if let Some(Some(unit)) = &mut self.external {
            unit.control_boundary();
        }
    }
    /// An opaque-state snapshot/restart can replace the native unit without
    /// changing its channel. Preserve the current engine note owners, not an
    /// old event log. Removed channels and different plugins never call this.
    pub fn inherit_plugin_notes(&mut self, before: &Self) {
        self.notes.copy_from_slice(&before.notes);
        self.next_instance = before.next_instance;
        self.held = before.held;
        if let Some(Some(unit)) = &mut self.external {
            for note in self.notes.iter().filter(|note| note.active && note.end != f64::NEG_INFINITY) {
                unit.note_on_instance(note.id, note.key, note.velocity, note.pan, note.expression);
                if unit.supports_note_pitch() { unit.set_note_pitch(note.id, note.pitch as f32); }
            }
        }
    }
    pub fn plugin_transport(&mut self, transport: crate::plugins::PluginTransport) {
        if let Some(Some(unit)) = &mut self.external {
            unit.transport(transport);
        }
    }

    pub fn apply_plugin(&mut self, binding: &windfall_project::PluginBinding) {
        if let Some(Some(unit)) = &mut self.external {
            unit.adopt_parameters(&binding.parameters);
        }
        for (index, param) in binding.parameters.iter().enumerate() {
            self.automate(index, param.value);
        }
    }

    pub fn apply(&mut self, params: &InstrumentParams) {
        if self.external.is_some() {
            return;
        }
        if *params != self.params && self.instrument.set_params(params) {
            self.params = *params;
        }
    }

    pub fn set_tempo(&mut self, tempo_bpm: f64) {
        if let Some(external) = &mut self.external {
            if let Some(unit) = external {
                unit.set_tempo(tempo_bpm as f32);
            }
            return;
        }
        self.instrument.set_tempo(tempo_bpm as f32);
    }

    /// Moves one setting, as automation does. Sounding notes glide to it.
    pub fn automate(&mut self, param: usize, value: f32) {
        if let Some(external) = &mut self.external {
            if let Some(cached) = self.plugin_params.get_mut(param)
                && cached.1 != value
            {
                if let Some(unit) = external {
                    unit.set_param(cached.0, value);
                }
                cached.1 = value;
            }
            return;
        }
        let mut params = self.params;
        if params.set(param, value) && params != self.params && self.instrument.set_params(&params)
        {
            self.params = params;
        }
    }

    /// Voices sounding right now, fading ones included.
    pub fn voices(&self) -> u32 {
        self.external.as_ref().map_or_else(
            || self.instrument.active_voices(),
            |unit| unit.as_ref().map_or(0, |unit| unit.voices()),
        ) as u32
    }

    /// True while the instrument is putting out sound, or did within the
    /// last `window` frames before `now`.
    pub fn sounding(&self, now: u64, window: u64) -> bool {
        self.voices() > 0 || self.last_sound + window > now
    }

    /// True once nothing more can come out of the instrument until its
    /// next note.
    pub fn settled(&self, now: u64) -> bool {
        let tail = self.external.as_ref().map_or_else(
            || self.instrument.tail_samples(),
            |unit| unit.as_ref().map_or(0, |unit| unit.tail()),
        ) as u64;
        self.voices() == 0 && self.last_sound + tail <= now
    }

    // This internal realtime seam accepts note data plus its clock/glide and
    // curve ownership; retain explicit values without changing note scheduling.
    #[allow(clippy::too_many_arguments)]
    pub fn start_note_source(&mut self, key: u8, velocity: f32, pan: f32, expression: NoteExpression, end: f64, origin: Origin, at: f64, glide_end: f64, source: Option<crate::note_curves::CurveSource>) -> Option<NoteInstanceId> {
        if !velocity.is_finite() || velocity <= 0.0 || end.is_nan() { return None; }
        let expression = expression.clamped();
        let target_pitch = f64::from(key.min(127)) + f64::from(expression.fine_pitch_cents) / 100.0;
        let from = self.notes.iter().filter(|note| note.active && note.origin == origin && note.expression.color_group.unwrap_or(0) == expression.color_group.unwrap_or(0) && note.end > at)
            .max_by_key(|note| note.id.0).map(|note| note.glide.map_or(note.pitch, |glide| glide.at(at)));
        let glide = if expression.articulation == NoteArticulation::Portamento {
            from.map(|from| NoteGlide::new(at, glide_end, from, target_pitch))
        } else { None };
        let slot = if let Some(slot) = self.notes.iter().position(|note| !note.active) { slot }
        else {
            // At the fixed ownership ceiling, release the oldest owner first.
            let slot = self.notes.iter().enumerate().min_by_key(|(_, note)| note.id.0).map(|(slot, _)| slot)?;
            self.release_slot(slot);
            slot
        };
        let id = loop {
            self.next_instance = self.next_instance.wrapping_add(1) & (u64::MAX >> 1);
            if self.next_instance != 0 && !self.notes.iter().any(|note| note.active && note.id.0 == self.next_instance) {
                break NoteInstanceId(self.next_instance);
            }
        };
        let mut channel_envelopes = self.voice_envelopes;
        if let Some(envelopes) = &mut channel_envelopes { envelopes.gate_on(); }
        let note = HeldInstrumentNote {
            channel_envelopes,
            channel_pitch: if self.voice_glide < 1.0 { from.unwrap_or(target_pitch) } else { glide.map_or(target_pitch, |g| g.from) },
            source,
            active: true, id, key: key.min(127), velocity: velocity.min(1.0),
            pan: if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 }, expression, end, origin,
            pitch: glide.map_or(target_pitch, |glide| glide.from), glide,
        };
        self.notes[slot] = note;
        self.held += 1;
        if let Some(external) = &mut self.external {
            if let Some(unit) = external {
                unit.note_on_instance(note.id, note.key, note.velocity, note.pan, note.expression);
            }
        } else {
            self.instrument.note_on_instance(note.id, note.key, note.velocity, note.pan, note.expression);
        }
        if note.glide.is_some() { self.set_pitch(note.id, note.pitch); }
        Some(id)
    }

    /// A slide transposes the held chord from the most recent owner's pitch.
    /// It starts no new note, retains source envelopes and original endings.
    pub fn slide_notes(&mut self, key: u8, expression: NoteExpression, at: f64, end: f64, origin: Origin) {
        let group = expression.color_group.unwrap_or(0);
        let Some(anchor) = self.notes.iter().filter(|note| note.active && note.origin == origin && note.expression.color_group.unwrap_or(0) == group && note.end > at)
            .max_by_key(|note| note.id.0) else { return; };
        let delta = f64::from(key.min(127)) + f64::from(expression.clamped().fine_pitch_cents) / 100.0
            - anchor.glide.map_or(anchor.pitch, |glide| glide.at(at));
        for note in self.notes.iter_mut().filter(|note| note.active && note.origin == origin && note.expression.color_group.unwrap_or(0) == group && note.end > at) {
            let from = note.glide.map_or(note.pitch, |glide| glide.at(at));
            note.pitch = from;
            note.glide = Some(NoteGlide::new(at, end, from, (from + delta).clamp(-12.0, 139.0)));
        }
    }

    pub fn silence_hardware(&mut self) {
        let any = self.notes.iter().any(|note| note.active && note.origin == Origin::Hardware);
        if !any {
            return;
        }
        let others = self.notes.iter().any(|note| note.active && note.origin != Origin::Hardware);
        if !others {
            self.silence();
        } else {
            for slot in 0..self.notes.len() {
                if self.notes[slot].active && self.notes[slot].origin == Origin::Hardware {
                    self.release_slot(slot);
                }
            }
        }
    }

    /// Stops every note with a short fade, as when the transport stops.
    pub fn silence(&mut self) {
        if let Some(external) = &mut self.external {
            if let Some(unit) = external {
                unit.all_notes_off();
            }
        } else {
            self.instrument.all_notes_off();
        }
        self.notes.fill(HeldInstrumentNote::EMPTY);
        self.held = 0;
    }

    /// Ends the notes that came from a pattern, as when playback jumps
    /// away from under them. With no note held by hand they stop with a
    /// short fade; otherwise each is let go, so the hand-held notes carry
    /// on.
    pub fn end_sequenced(&mut self) {
        let any_live = self.notes.iter().any(|note| note.active && note.origin != Origin::Sequenced);
        if !any_live {
            self.silence();
            return;
        }
        for slot in 0..self.notes.len() {
            if self.notes[slot].active && self.notes[slot].origin == Origin::Sequenced {
                self.release_slot(slot);
            }
        }
    }

    /// Moves the end of every held note by `ticks`, because a jump made the
    /// clock read that much more.
    pub fn shift_ends(&mut self, ticks: f64) {
        if self.held > 0 {
            for note in self.notes.iter_mut().filter(|note| note.active) {
                note.end += ticks;
                if let Some(source) = &mut note.source { source.shift(ticks); }
                if let Some(glide) = &mut note.glide { glide.shift(ticks); }
            }
        }
    }

    /// Gives every held note the end `moved` makes of the one it has: the
    /// tempo map changed, and each clock tick now means another place in
    /// the song.
    pub fn move_ends(&mut self, moved: impl Fn(f64) -> f64) {
        if self.held > 0 {
            for note in self.notes.iter_mut().filter(|note| note.active) {
                note.end = moved(note.end);
                if let Some(source) = &mut note.source { source.move_clock(&moved); }
                if let Some(glide) = &mut note.glide { glide.move_clock(&moved); }
            }
        }
    }

    pub fn render_curves(&mut self, plan: &Plan, clock: Clock, base: u64, from: usize, to: usize) {
        self.render_with_plan(Some(plan), clock, base, from, to);
    }
    pub fn refresh_curve_sources(&mut self, plan: &Plan) {
        for note in self.notes.iter_mut().filter(|note| note.active) {
            if let Some(source) = &mut note.source { source.refresh(plan); }
        }
    }
    fn render_with_plan(&mut self, plan: Option<&Plan>, clock: Clock, base: u64, from: usize, to: usize) {
        let mut at = from;
        while self.held > 0 {
            // The note that ends first. Of notes that end on one frame the
            // earliest-created owner goes first, which keeps the order fixed.
            let mut next: Option<(u64, usize)> = None;
            for (slot, note) in self.notes.iter().enumerate() {
                if !note.active {
                    continue;
                }
                let frame = clock.frame_of(note.end);
                if frame < base + to as u64 && next.is_none_or(|(first, before)| (frame, note.id.0) < (first, self.notes[before].id.0)) {
                    next = Some((frame, slot));
                }
            }
            let Some((frame, slot)) = next else {
                break;
            };
            // An end that is already behind takes effect now.
            let split = (frame.saturating_sub(base) as usize).clamp(at, to);
            self.process(plan, clock, base, at, split);
            at = split;
            // Evaluate the boundary too: a held last segment may jump at 100%,
            // and its release value must be installed before key-up.
            let note = self.notes[slot];
            if let Some((pan, expression)) = note.source.filter(|source| source.active)
                .and_then(|source| plan.map(|plan| source.controls(plan, clock.tick_at(base + split as u64))))
                && (pan != note.pan || expression != note.expression)
            {
                self.set_note_expression(note.id, pan, expression);
                if !self.external.as_ref().is_some_and(|unit| unit.as_ref().is_none_or(|unit| !unit.supports_note_pitch())) {
                    self.set_pitch(note.id, self.notes[slot].pitch);
                }
            }
            self.release_slot(slot);
        }
        self.process(plan, clock, base, at, to);
    }

    fn process(&mut self, plan: Option<&Plan>, clock: Clock, base: u64, from: usize, to: usize) {
        let pitch_supported = !self.external.as_ref().is_some_and(|unit| unit.as_ref().is_none_or(|unit| !unit.supports_note_pitch()));
        if !self.notes.iter().any(|note| note.active && (note.channel_envelopes.is_some() || self.voice_glide < 1.0 || pitch_supported && note.glide.is_some() || plan.is_some() && note.source.is_some_and(|source| source.active))) {
            self.process_audio(base, from, to);
            return;
        }
        // Musical pitch is evaluated at the exact output frame, so changing
        // tempo or callback block size does not change the glide trajectory.
        let mut modulation = crate::channel_voice::ModulationBlock::default();
        for frame in from..to {
            let tick = clock.tick_at(base + frame as u64);
            for slot in 0..self.notes.len() {
                let before = self.notes[slot];
                if !before.active { continue; }
                let controls = before.source.filter(|source| source.active).and_then(|source| plan.map(|plan| source.controls(plan, tick)));
                let mut pitch_changed = false;
                if let Some((pan, expression)) = controls {
                    pitch_changed = expression.fine_pitch_cents != before.expression.fine_pitch_cents;
                    if pan != before.pan || expression != before.expression { self.set_note_expression(before.id, pan, expression); }
                }
                let note = &mut self.notes[slot];
                let gliding = pitch_supported && note.glide.is_some();
                if let Some(glide) = note.glide.filter(|_| pitch_supported) {
                    note.pitch = glide.at(tick);
                    if tick >= glide.end { note.glide = None; }
                }
                let mut modulated_controls = None;
                let mut pitch_offset = 0.0;
                if let Some(envelopes) = &mut note.channel_envelopes {
                    envelopes.process_block(1, &mut modulation);
                    let mut expression = note.expression;
                    expression.modulation_x = (expression.modulation_x + modulation.filter_cutoff_offset[0] / 8.0).clamp(0.0, 1.0);
                    let pan = (note.pan + modulation.pan[0]).clamp(-1.0, 1.0);
                    modulated_controls = Some((pan, expression));
                    pitch_offset = f64::from(modulation.pitch_cents[0]) / 100.0;
                }
                note.channel_pitch += self.voice_glide * (note.pitch - note.channel_pitch);
                let (id, pitch) = (note.id, note.channel_pitch + pitch_offset);
                if let Some((pan, expression)) = modulated_controls {
                    // Send through the existing per-instance expression API; don't overwrite base musical curves.
                    if let Some(Some(unit)) = &mut self.external { unit.set_note_expression(id, pan, expression); }
                    else if self.external.is_none() { self.instrument.set_note_expression(id, pan, expression); }
                }
                if pitch_supported && (gliding || pitch_changed || modulated_controls.is_some() || self.voice_glide < 1.0) { self.set_pitch(id, pitch); }
            }
            self.process_audio(base, frame, frame + 1);
        }
    }

    fn set_pitch(&mut self, id: NoteInstanceId, pitch: f64) {
        if let Some(Some(unit)) = &mut self.external { unit.set_note_pitch(id, pitch as f32); }
        else if self.external.is_none() { self.instrument.set_note_pitch(id, pitch as f32); }
    }

    fn process_audio(&mut self, base: u64, from: usize, to: usize) {
        if from >= to {
            return;
        }
        let (left, right) = (&mut self.left[from..to], &mut self.right[from..to]);
        if let Some(external) = &mut self.external {
            if let Some(unit) = external {
                unit.process(left, right);
            } else {
                left.fill(0.0);
                right.fill(0.0);
            }
        } else {
            // A MIDI output channel's messages belong to this block's frame.
            self.instrument.stamp_midi(base + from as u64);
            self.instrument.process(left, right);
        }
        let last = (0..to - from)
            .rev()
            .find(|&index| left[index] != 0.0 || right[index] != 0.0);
        if let Some(last) = last {
            self.last_sound = base + (from + last) as u64 + 1;
        }
    }

    /// Hands over what a MIDI output channel has made, oldest first. Fixed
    /// storage; nothing for any other instrument.
    pub fn drain_midi(&mut self, out: &mut dyn FnMut(windfall_dsp::MidiOutEvent)) {
        if self.external.is_none() {
            self.instrument.drain_midi(out);
        }
    }

    fn release_slot(&mut self, slot: usize) {
        if let Some(envelopes) = &mut self.notes[slot].channel_envelopes { envelopes.gate_off(); }
        let note = self.notes[slot];
        if !note.active { return; }
        self.notes[slot].active = false;
        self.held -= 1;
        let other_on_key = self.notes.iter().any(|other| other.active && other.key == note.key);
        if let Some(external) = &mut self.external {
            if let Some(unit) = external {
                let other_on_key = if unit.supports_note_channels() {
                    self.notes.iter().any(|other| other.active && other.key == note.key
                        && other.expression.color_group.unwrap_or(0) == note.expression.color_group.unwrap_or(0))
                } else { other_on_key };
                // A key-only provider cannot address the shorter overlap.
                // Keep that key down until its final logical owner ends.
                if unit.supports_note_instances() || !other_on_key {
                    unit.note_off_on_channel(note.id, note.key, note.expression.color_group.unwrap_or(0));
                }
            }
        } else {
            if self.instrument.supports_note_instances() || !other_on_key {
                self.instrument.note_off_instance(note.id, note.key);
            }
        }
    }

    pub fn set_note_expression(&mut self, id: NoteInstanceId, pan: f32, expression: NoteExpression) {
        let expression = expression.clamped();
        let pan = if pan.is_finite() { pan.clamp(-1.0, 1.0) } else { 0.0 };
        if let Some(note) = self.notes.iter_mut().find(|note| note.active && note.id == id) {
            let moved = f64::from(expression.fine_pitch_cents - note.expression.fine_pitch_cents) / 100.0;
            note.pitch += moved;
            if let Some(glide) = &mut note.glide {
                glide.from += moved;
                glide.to += moved;
            }
            note.pan = pan;
            note.expression = expression;
        }
        if let Some(Some(unit)) = &mut self.external { unit.set_note_expression(id, pan, expression); }
        else if self.external.is_none() { self.instrument.set_note_expression(id, pan, expression); }
    }

    pub fn retarget_instance(&mut self, id: NoteInstanceId, note: crate::voice::Note, at: f64, glide_end: f64) -> bool {
        let Some(held) = self.notes.iter_mut().find(|held| held.active && held.id == id) else { return false; };
        held.key = note.key; held.end = note.end; held.source = note.source; held.origin = note.origin;
        held.pitch = f64::from(note.key) + f64::from(note.expression.fine_pitch_cents) / 100.0;
        let target = held.pitch;
        held.glide = if note.expression.articulation == NoteArticulation::Portamento {
            Some(NoteGlide::new(at, glide_end, held.channel_pitch, target))
        } else { None };
        held.pitch = held.glide.map_or(target, |glide| glide.from);
        let pitch = held.channel_pitch;
        held.expression = note.expression.clamped();
        self.set_note_expression(id, note.pan, note.expression);
        self.set_pitch(id, pitch);
        true
    }

    pub fn reset_channel_modulation(&mut self) {
        for note in self.notes.iter_mut() {
            if let Some(envelopes) = &mut note.channel_envelopes { envelopes.reset(); }
        }
    }

    /// Schedule a release without touching any sibling occurrence on its key.
    pub fn release_instance(&mut self, id: NoteInstanceId) {
        if let Some(note) = self.notes.iter_mut().find(|note| note.active && note.id == id) {
            note.end = f64::NEG_INFINITY;
        }
    }

    /// The first `frames` frames of the block just rendered.
    pub fn output(&self, frames: usize) -> (&[f32], &[f32]) {
        (&self.left[..frames], &self.right[..frames])
    }
}

/// A stereo delay that lines one path up with a slower one.
///
/// The delay is a whole number of frames. A change of length crossfades
/// from the old tap to the new one in a straight line, which is also how
/// the limiter changes its own look-ahead, so a path that is compensated
/// for a limiter stays lined up with it while its look-ahead moves. The
/// crossfade can be told to wait first, which is how the delay stays in
/// step with an effect that is coming into a chain: that effect runs
/// unheard for as long as its latency before it fades in.
struct CompensationLine {
    ring: Box<[Frame]>,
    mask: usize,
    /// Where the next frame goes.
    write: usize,
    delay: usize,
    /// The current tap mixture, also used by matrix wet/dry delay edits.
    fade: TapCrossfade,
    /// Frames to go before that fade starts.
    wait: u32,
    /// Recent input frames actually retained in this ring.
    valid: usize,
    /// Latest unapplied tap edit, coalesced while its input history fills.
    pending: Option<(usize, u32, bool)>,
    readiness: usize,
}

impl CompensationLine {
    /// Allocates a line that can delay by up to `max_delay` frames, set to
    /// `delay`.
    pub fn new(max_delay: usize, delay: usize) -> Self {
        let length = (max_delay + 1).next_power_of_two();
        Self {
            ring: vec![[0.0; 2]; length].into_boxed_slice(),
            mask: length - 1,
            write: 0,
            delay: delay.min(length - 1),
            fade: TapCrossfade::new(length - 1, delay),
            wait: 0,
            valid: 0,
            pending: None,
            readiness: 0,
        }
    }

    /// The longest delay the line can give.
    pub fn capacity(&self) -> usize {
        self.mask
    }

    /// Moves to a new delay: after `wait` more frames at the old one, a
    /// crossfade of `fade_frames` frames.
    pub fn retarget(&mut self, delay: usize, fade_frames: u32, wait: u32) {
        self.retarget_ready(delay, fade_frames, wait, delay.min(self.capacity()), false);
    }

    fn retarget_ready(
        &mut self,
        delay: usize,
        fade_frames: u32,
        wait: u32,
        readiness: usize,
        joining: bool,
    ) {
        let delay = delay.min(self.capacity());
        self.readiness = readiness;
        if delay == self.delay {
            return;
        }
        self.delay = delay;
        self.pending = Some((delay, fade_frames.max(1), joining));
        self.wait = wait;
        self.begin_ready();
    }

    fn begin_ready(&mut self) {
        if self.valid >= self.readiness
            && let Some((delay, frames, joining)) = self.pending.take()
        {
            if joining {
                self.fade.retarget_joining(delay, frames);
            } else {
                self.fade.retarget(delay, frames);
            }
        }
    }

    /// Sets the delay at once.
    pub fn snap(&mut self, delay: usize) {
        self.delay = delay.min(self.capacity());
        self.fade.snap(self.delay);
        self.wait = 0;
        self.pending = None;
        self.readiness = 0;
    }

    /// Carries on from another line: its recent past, as much as fits, and
    /// the delay it was giving.
    pub fn take_history(&mut self, other: &Self) {
        let frames = self.ring.len().min(other.ring.len());
        for back in 1..=frames {
            let frame = other.ring[other.write.wrapping_sub(back) & other.mask];
            self.ring[self.write.wrapping_sub(back) & self.mask] = frame;
        }
        self.delay = other.delay.min(self.capacity());
        self.fade.take_history(&other.fade);
        self.wait = other.wait;
        self.valid = other.valid.min(self.capacity());
        self.pending = other.pending;
        self.readiness = other.readiness;
    }

    fn take_input_history(&mut self, other: &Self, prefix: usize) {
        self.valid = self.capacity().min(other.valid.saturating_sub(prefix));
        for back in 1..=self.valid {
            self.ring[self.write.wrapping_sub(back) & self.mask] =
                other.ring[other.write.wrapping_sub(back + prefix) & other.mask];
        }
    }

    /// Reconstruct a newly introduced stage's input from the preceding
    /// stage's retained input and current tap mixture. The coefficients are
    /// a bounded snapshot; reference changes do not promise phase inversion.
    fn take_output_history(&mut self, other: &Self) {
        self.valid = self
            .capacity()
            .min(other.valid.saturating_sub(other.fade.longest_delay()));
        for back in 1..=self.valid {
            let frame = std::array::from_fn(|side| {
                other.fade.read(|delay| {
                    other.ring[other.write.wrapping_sub(back + delay) & other.mask][side]
                })
            });
            self.ring[self.write.wrapping_sub(back) & self.mask] = frame;
        }
    }

    fn advance(&mut self) {
        self.valid = (self.valid + 1).min(self.capacity());
        if self.wait > 0 {
            self.wait -= 1;
        } else {
            self.fade.advance();
        }
        self.write = (self.write + 1) & self.mask;
    }

    /// Delays a block in place.
    pub fn process(&mut self, block: &mut [Frame]) {
        for frame in block {
            self.begin_ready();
            self.ring[self.write] = *frame;
            let out = std::array::from_fn(|side| {
                self.fade
                    .read(|delay| self.ring[self.write.wrapping_sub(delay) & self.mask][side])
            });
            self.advance();
            *frame = out;
        }
    }
}

/// One causal stage of the reference route. Zero-delay matrices remain in
/// this description so their history is already primed before the first edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DelayStage {
    pub key: windfall_project::PluginTarget,
    pub generation: u64,
    pub delay: usize,
    pub maximum: usize,
    pub readiness: usize,
    /// Remaining priming/departure wait of this exact rack generation.
    pub wait: u32,
    pub matrix: bool,
    pub leaving: bool,
}

impl DelayStage {
    pub fn same_line(&self, other: &Self) -> bool {
        self.key == other.key
            && self.generation == other.generation
            && self.maximum == other.maximum
            && self.matrix == other.matrix
    }
}

struct CompensationStage {
    spec: DelayStage,
    line: CompensationLine,
}

/// Prepared routing compensation. A matrix reference path is mirrored as
/// serial delay stages, rather than flattening its time-varying transfer to
/// one tap fade. Downstream stages then delay both audio and upstream fade
/// weights, including during overlapping edits. The aggregate line keeps
/// raw input history for changes of route shape and the legacy scalar policy.
pub(crate) struct Compensation {
    line: CompensationLine,
    stages: Box<[CompensationStage]>,
    selected: usize,
}

impl Compensation {
    pub fn reserved_payload(capacity: usize, stages: usize) -> Option<usize> {
        let length = capacity.checked_add(1)?.checked_next_power_of_two()?;
        // TapCrossfade's private Tap has the same usize/f32 storage and
        // alignment as this pair; its reserved universe is length taps.
        let samples = length.checked_mul(std::mem::size_of::<Frame>())?;
        let taps = length.checked_mul(std::mem::size_of::<(usize, f32)>())?;
        let line = samples
            .checked_add(taps)?
            .checked_add(std::mem::size_of::<CompensationLine>())?;
        line.checked_mul(stages.checked_add(1)?)?
            .checked_add(
                stages.checked_mul(
                    std::mem::size_of::<CompensationStage>()
                        .checked_sub(std::mem::size_of::<CompensationLine>())?,
                )?,
            )?
            .checked_add(std::mem::size_of::<Self>())
    }
    pub fn new(max_delay: usize, delay: usize) -> Self {
        Self::with_path(max_delay, delay, &[], false)
    }

    pub fn with_path(max_delay: usize, delay: usize, path: &[DelayStage], sounding: bool) -> Self {
        Self {
            line: CompensationLine::new(max_delay, delay),
            stages: path
                .iter()
                .map(|spec| CompensationStage {
                    spec: *spec,
                    line: CompensationLine::new(
                        spec.maximum,
                        if sounding { 0 } else { spec.delay },
                    ),
                })
                .collect(),
            selected: path.len(),
        }
    }

    /// Every stage bank can inherit the aggregate or any selected predecessor
    /// stage, including its complete tap universe. Inactive bank entries never
    /// participate in processing, history, suffix or scalar promotion.
    pub fn reserved(maximum: usize, path: &[DelayStage], stages: usize) -> Self {
        let empty = DelayStage {
            key: windfall_project::PluginTarget::Effect {
                effect: windfall_project::EffectId(0),
            },
            generation: 0,
            delay: 0,
            maximum: 0,
            readiness: 0,
            wait: 0,
            matrix: false,
            leaving: false,
        };
        Self {
            line: CompensationLine::new(maximum, 0),
            stages: (0..stages)
                .map(|index| CompensationStage {
                    spec: path.get(index).copied().unwrap_or(empty),
                    line: CompensationLine::new(maximum, 0),
                })
                .collect(),
            selected: path.len(),
        }
    }

    pub fn history_capacity(&self) -> usize {
        self.stages.iter().fold(self.capacity(), |capacity, stage| {
            capacity.max(stage.line.capacity())
        })
    }

    pub fn select_path(&mut self, path: &[DelayStage]) {
        assert!(
            path.len() <= self.stages.len(),
            "prepared stage bank is complete"
        );
        self.selected = path.len();
        for (stage, spec) in self.stages.iter_mut().zip(path) {
            stage.spec = *spec;
        }
    }

    pub fn capacity(&self) -> usize {
        self.line.capacity()
    }

    pub fn with_history_capacity(
        maximum: usize,
        delay: usize,
        path: &[DelayStage],
        sounding: bool,
    ) -> Self {
        Self {
            line: CompensationLine::new(maximum, delay),
            stages: path
                .iter()
                .map(|spec| CompensationStage {
                    spec: *spec,
                    line: CompensationLine::new(
                        maximum.max(spec.maximum),
                        if sounding { 0 } else { spec.delay },
                    ),
                })
                .collect(),
            selected: path.len(),
        }
    }

    pub fn retarget(&mut self, delay: usize, fade_frames: u32, wait: u32) {
        self.line.retarget(delay, fade_frames, wait);
    }

    pub fn retarget_path(
        &mut self,
        delay: usize,
        path: &[DelayStage],
        fade_frames: u32,
        wait: u32,
    ) {
        self.retarget(delay, fade_frames, wait);
        for (stage, spec) in self.stages.iter_mut().zip(path) {
            stage.line.retarget_ready(
                spec.delay,
                fade_frames,
                spec.wait,
                if spec.matrix { spec.readiness } else { 0 },
                !spec.matrix && !spec.leaving,
            );
            stage.spec = *spec;
        }
    }

    pub fn snap(&mut self, delay: usize, path: &[DelayStage]) {
        self.line.snap(delay);
        for (stage, spec) in self.stages.iter_mut().zip(path) {
            stage.line.snap(spec.delay);
            stage.spec = *spec;
        }
    }

    pub fn take_history(&mut self, other: &Self) {
        self.line.take_history(&other.line);
        // Promote settled scalar compensation when a matrix joins an
        // existing fixed-delay route. Reconstruct each fixed stage's input
        // from raw history, including the delay of preceding fixed stages.
        let fixed: usize = self.stages[..self.selected]
            .iter()
            .filter(|stage| !stage.spec.matrix)
            .map(|stage| stage.spec.delay)
            .sum();
        let promote =
            other.selected == 0 && other.line.fade.remaining() == 0 && fixed == other.line.delay;
        let mut prefix = 0;
        for index in 0..self.selected {
            let (prefix_stages, rest) = self.stages[..self.selected].split_at_mut(index);
            let (stage, following) = rest.split_first_mut().unwrap();
            let retained = || {
                other.stages[..other.selected].iter().filter(|before| {
                    !(before.spec.leaving && before.line.fade.longest_delay() == 0)
                })
            };
            let inserted_first = index == 0
                && !following.is_empty()
                && retained().count() == following.len()
                && retained()
                    .zip(following.iter())
                    .all(|(before, after)| before.spec.same_line(&after.spec));
            if let Some(before) = other.stages[..other.selected]
                .iter()
                .find(|before| stage.spec.same_line(&before.spec))
            {
                stage.line.take_history(&before.line);
            } else if promote {
                stage.line.take_input_history(&other.line, prefix);
                if !stage.spec.matrix {
                    stage.line.snap(stage.spec.delay);
                    prefix += stage.spec.delay;
                }
            } else if let Some(previous) = prefix_stages.last() {
                stage.line.take_output_history(&previous.line);
            } else if inserted_first {
                // The existing suffix already supplies its complete transfer.
                // A fresh leading insertion starts at identity, with raw
                // history only; adopting the aggregate tap would count that
                // suffix twice. Completed departures are identity too.
                stage.line.take_input_history(&other.line, 0);
            } else {
                // A change from one staged reference to another also has
                // usable raw history. Preserve the previous aggregate tap
                // while starting the first replacement stage's transition.
                stage.line.take_history(&other.line);
                if stage.line.fade.longest_delay() > stage.line.valid {
                    // Aggregate metadata can be ahead of a queued matrix
                    // edit. Never adopt a tap with unavailable raw history.
                    let audible = other.stages[..other.selected]
                        .iter()
                        .map(|stage| stage.line.fade.longest_delay())
                        .sum::<usize>();
                    stage.line.snap(audible.min(stage.line.valid));
                }
            }
        }
    }

    pub fn process(&mut self, block: &mut [Frame]) {
        if self.selected == 0 {
            self.line.process(block);
        } else {
            // Keep the aggregate raw history without replacing stage input.
            for frame in block.iter() {
                self.line.ring[self.line.write] = *frame;
                self.line.advance();
            }
            for stage in &mut self.stages[..self.selected] {
                stage.line.process(block);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(frames: usize) -> Vec<Frame> {
        (0..frames).map(|n| [n as f32, -(n as f32)]).collect()
    }

    fn stage(generation: u64, maximum: usize, delay: usize) -> DelayStage {
        DelayStage {
            key: windfall_project::PluginTarget::Effect {
                effect: windfall_project::EffectId(generation as u32),
            },
            generation,
            maximum,
            delay,
            readiness: delay,
            wait: 0,
            matrix: true,
            leaving: false,
        }
    }

    #[test]
    fn p1_selected_history_preserves_inherited_taps_and_ignores_inactive_rows() {
        let path = [stage(1, 255, 127), stage(2, 255, 63)];
        let mut before = Compensation::with_history_capacity(255, 190, &path, false);
        before.process(&mut ramp(1000));
        let changed = [stage(1, 255, 191), stage(2, 255, 111)];
        before.retarget_path(302, &changed, 240, 0);
        before.process(&mut ramp(7));
        before.retarget_path(50, &[stage(1, 255, 33), stage(2, 255, 17)], 240, 0);
        before.process(&mut ramp(13));
        let inherited = before.history_capacity();
        let mut after = Compensation::reserved(inherited, &changed, 8);
        after.select_path(&changed);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| after.take_history(&before)),
            0
        );
        for index in 0..2 {
            assert_eq!(
                after.stages[index].line.fade.longest_delay(),
                before.stages[index].line.fade.longest_delay()
            );
            assert_eq!(
                after.stages[index].line.fade.remaining(),
                before.stages[index].line.fade.remaining()
            );
            assert_eq!(
                after.stages[index]
                    .line
                    .fade
                    .read(|delay| (delay * delay) as f32),
                before.stages[index]
                    .line
                    .fade
                    .read(|delay| (delay * delay) as f32)
            );
            assert!(after.stages[index].line.capacity() >= before.stages[index].line.capacity());
        }
        // A physical bank with no selected stages behaves exactly like scalar
        // compensation, including promotion and an unfinished inherited fade.
        after.select_path(&[]);
        let mut scalar = Compensation::new(inherited, 0);
        scalar.take_history(&before);
        after.take_history(&before);
        let mut actual = ramp(137);
        let mut expected = actual.clone();
        assert_eq!(
            crate::test_alloc::allocator_calls(|| after.process(&mut actual)),
            0
        );
        scalar.process(&mut expected);
        assert_eq!(actual, expected);
        assert_eq!(after.selected, 0);
        assert_eq!(after.stages.len(), 8);
    }

    #[test]
    fn p1_reserved_payload_has_visible_size_overflow() {
        assert_eq!(Compensation::reserved_payload(usize::MAX, 1), None);
        assert_eq!(Compensation::reserved_payload(16, usize::MAX), None);
    }

    #[test]
    fn a_compensation_delays_by_whole_frames_and_passes_zero_delay_untouched() {
        let mut none = Compensation::new(16, 0);
        let mut block = ramp(40);
        none.process(&mut block);
        assert_eq!(block, ramp(40));

        let mut line = Compensation::new(16, 5);
        assert!(line.capacity() >= 16);
        let mut block = ramp(40);
        // In uneven pieces, which must not matter.
        let (first, rest) = block.split_at_mut(7);
        line.process(first);
        line.process(rest);
        for (index, frame) in block.iter().enumerate() {
            let expected = index.checked_sub(5).map_or(0.0, |n| n as f32);
            assert_eq!(*frame, [expected, -expected], "frame {index}");
        }
    }

    #[test]
    fn a_change_of_delay_crossfades_between_the_two_taps() {
        let mut line = Compensation::new(64, 2);
        let mut block = ramp(100);
        line.process(&mut block);
        line.retarget(10, 4, 0);
        let mut block: Vec<Frame> = (100..110).map(|n| [n as f32, 0.0]).collect();
        line.process(&mut block);
        let left: Vec<f32> = block.iter().map(|frame| frame[0]).collect();
        // Frame 100: all of the old tap, 98. Then a quarter more of the new
        // tap, eight frames further back, on each frame.
        assert_eq!(left[..5], [98.0, 97.0, 96.0, 95.0, 94.0]);
        assert_eq!(left[5..], [95.0, 96.0, 97.0, 98.0, 99.0]);

        // Told to wait, it stays on the old delay that much longer.
        line.retarget(2, 2, 3);
        let mut block: Vec<Frame> = (110..117).map(|n| [n as f32, 0.0]).collect();
        line.process(&mut block);
        let left: Vec<f32> = block.iter().map(|frame| frame[0]).collect();
        assert_eq!(left, [100.0, 101.0, 102.0, 103.0, 108.0, 113.0, 114.0]);

        // A longer delay than the line holds is held to what it holds.
        line.retarget(1_000, 1, 0);
        assert_eq!(line.line.delay, line.capacity());
    }

    #[test]
    fn a_line_carries_on_from_the_one_it_replaces() {
        let mut small = Compensation::new(7, 3);
        let mut block = ramp(50);
        small.process(&mut block);
        let mut large = Compensation::new(100, 0);
        large.take_history(&small);
        large.retarget(6, 2, 0);
        let mut block: Vec<Frame> = (50..54).map(|n| [n as f32, 0.0]).collect();
        large.process(&mut block);
        let left: Vec<f32> = block.iter().map(|frame| frame[0]).collect();
        // The old delay of 3 on the first frame, half way on the second,
        // and from then on the new delay of 6, reading what the small line
        // was given.
        assert_eq!(left, [47.0, 46.5, 46.0, 47.0]);
    }
}
