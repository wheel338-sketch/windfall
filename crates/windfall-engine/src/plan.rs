//! The compiled form of a project that the audio thread plays.
//!
//! [`compile`] runs on the control side. It resolves every id to an index,
//! applies swing, sorts notes by time, orders the mixer tracks and works out
//! what mute and solo leave audible, so the audio thread only ever follows
//! indices. A [`Plan`] is immutable once built. Everything that moves while
//! it plays, from the gain ramps to the effects themselves, lives in a
//! [`PlanState`](crate::state::PlanState) sized to match.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU64, Ordering},
};

use windfall_core::{AudioBuffer, PPQ, TICKS_PER_STEP};
use windfall_project::{
    Channel, ChannelId, ChannelSource, ClipContent, ClipId, DEFAULT_PATTERN_STEPS, EffectId,
    EffectParams, Envelope, InstrumentParams, MAX_EFFECT_SLOTS, MAX_ENVELOPE_MS, MAX_GAIN, MAX_KEY,
    MAX_MIXER_TRACKS, MAX_TEMPO_BPM, MAX_TUNE_SEMITONES, MIN_TEMPO_BPM, Mixer, MixerTrack, Pattern,
    PatternId, Playlist, Project, SamplerSettings, TrackId,
};

use crate::automation::{self, Lane};
use crate::pool::SamplePool;
use crate::tempo::TempoMap;
use crate::voice::LoopRegion;

/// Tempo used when a project carries one that is not a number.
const FALLBACK_TEMPO_BPM: f64 = 120.0;

/// Length a pattern-mode loop falls back to when the plan has no pattern.
pub(crate) const FALLBACK_LOOP_TICKS: u32 = DEFAULT_PATTERN_STEPS * TICKS_PER_STEP;

/// Maps ids to positions in one of the plan's lists.
#[derive(Debug, Clone, Default)]
pub(crate) struct IdIndex {
    /// `(id, position)` sorted by id.
    entries: Vec<(u32, u32)>,
}

impl IdIndex {
    pub fn new(ids: impl Iterator<Item = u32>) -> Self {
        let mut entries: Vec<(u32, u32)> = ids
            .enumerate()
            .map(|(position, id)| (id, position as u32))
            .collect();
        entries.sort_unstable();
        // Ids are unique in a valid project. If one repeats, the first wins.
        entries.dedup_by_key(|entry| entry.0);
        Self { entries }
    }

    pub fn get(&self, id: u32) -> Option<usize> {
        self.entries
            .binary_search_by_key(&id, |entry| entry.0)
            .ok()
            .map(|found| self.entries[found].1 as usize)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Plan {
    pub performance_clips: Vec<crate::performance_clips::PreparedClip>,
    pub performance_clip_ids: IdIndex,
    pub performance_tempo: bool,
    /// Callback-only musical cursor for performance curve occurrences. Source
    /// data remains immutable; this scalar has no lock or ownership exchange.
    pub performance_tick: Arc<AtomicU64>,
    pub videos: Vec<crate::video_audio::PreparedVideo>,
    /// Control-side memory admission shared across this controller's plans.
    pub echo_history: Arc<crate::effect_host::HistoryPool>,
    _asset_pins: windfall_dsp::asset::AssetPins,
    pub navigation: Vec<crate::timeline::NavigationPoint>,
    pub song_start: Option<u32>,
    pub signature: windfall_project::TimeSignature,
    pub meters: Result<
        Vec<windfall_project::timeline::MeterSegment>,
        windfall_project::timeline::MeterMapError,
    >,
    pub plugins: Vec<windfall_project::PluginBinding>,
    pub plugin_factory: Option<std::sync::Arc<dyn crate::plugins::PluginFactory>>,
    pub factory_stamp: Option<crate::project_preparation::FactoryStamp>,
    pub tempo_bpm: f64,
    /// How every pan the engine applies turns into the level of each side.
    pub pan_law: windfall_core::PanLaw,
    /// Channel rack order.
    pub channels: Vec<PlanChannel>,
    pub channel_ids: IdIndex,
    pub patterns: Vec<PlanPattern>,
    pub pattern_ids: IdIndex,
    /// Mixer order. `tracks[0]` is the master and always exists.
    pub tracks: Vec<PlanTrack>,
    pub track_ids: IdIndex,
    /// Track indices ordered so every track comes after the tracks that feed
    /// it.
    pub order: Vec<usize>,
    pub waves: crate::track_workers::GraphWaves,
    /// 0 inherits the controller preference during publication.
    pub audio_threads: usize,
    /// Number of routing edges, which is the number of edge ramps a
    /// [`PlanState`](crate::state::PlanState) holds.
    pub edge_count: usize,
    /// Finds an effect that is part of the project: an index into
    /// `effect_places`.
    pub effect_ids: IdIndex,
    /// The track of each such effect and its place in that track's chain.
    pub effect_places: Vec<(usize, usize)>,
    /// Pattern clips that sound, sorted by start.
    pub clips: Vec<PlanClip>,
    /// Audio clips that sound, sorted by start and then by id.
    pub audio_clips: Vec<PlanAudioClip>,
    /// Finds an audio clip that sounds: an index into `audio_clips`.
    pub audio_clip_ids: IdIndex,
    /// End of the last clip in the active playlist in ticks, muted clips included.
    pub song_end: u32,
    /// What automation does to each of its targets along the song.
    pub lanes: Arc<[Lane]>,
    pub control_sources: Vec<crate::internal_controllers::Source>,
    pub control_links: Vec<crate::internal_controllers::Link>,
    pub scripts: Vec<Option<windfall_project::scripting::NoteTransform>>,
    pub script_parameters: Vec<Option<(usize, usize)>>,
    /// Some pattern has an event lane, so a pattern playing by itself has
    /// automation to follow too.
    pub pattern_lanes: bool,
    /// How long the song takes to get to each of its ticks, when
    /// automation moves the tempo. `None` when the tempo is steady.
    pub tempo_map: Option<TempoMap>,
    /// Full-mix control-grid clock retained by isolated stem passes.
    pub tempo_replay: Arc<[f64]>,
}

#[derive(Debug, Clone)]
pub(crate) struct PlanChannel {
    pub sample_playback: Option<Arc<crate::sample_playback::Prepared>>,
    pub rack: Option<Arc<windfall_project::RackGraph>>,
    pub voice: windfall_project::ChannelVoiceSettings,
    /// What the stored controls of a note drive in this channel's voices.
    pub expression_map: windfall_dsp::ExpressionMap,
    pub id: ChannelId,
    /// Index of the mixer track the channel plays into.
    pub track: usize,
    /// Channel volume, or zero when mute or solo silences the channel.
    pub gain: f32,
    /// Neither mute nor solo silences the channel.
    pub audible: bool,
    pub pan: f32,
    /// What a sampler channel plays. An instrument channel has a sampler
    /// with no sample.
    pub sampler: PlanSampler,
    /// The settings of the instrument an instrument channel plays.
    pub instrument: Option<InstrumentParams>,
    pub instrument_asset: Option<Arc<windfall_dsp::InstrumentAsset>>,
    pub sample_bank: Option<Arc<windfall_dsp::sample_bank::SampleBank>>,
    /// For a layer channel, the indexes of the channels its notes play on,
    /// nested layers already followed. `None` for every other channel. A
    /// layer never sounds itself, so an empty list is silence.
    pub layer: Option<Arc<[usize]>>,
    /// The channel is no longer in the project. It is listed after the
    /// project's channels, where no note finds it, for as long as its
    /// instrument takes to fade out. See [`Plan::keep_leaving`].
    pub leaving: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct PlanSampler {
    pub sample: Option<AudioBuffer>,
    pub bank: Option<std::sync::Arc<crate::sampler_processing::SamplerBank>>,
    pub spectral: bool,
    /// First frame of the region that plays.
    pub start: usize,
    /// One past the last frame of the region. Greater than `start` whenever
    /// there is a sample.
    pub end: usize,
    pub reverse: bool,
    pub loop_region: Option<LoopRegion>,
    /// Semitones added to a note's key to get its pitch relative to the
    /// recorded pitch: the tuning minus the root key.
    pub key_offset: f32,
    pub gain: f32,
    pub envelope: Option<Envelope>,
    pub cut_self: bool,
    pub cut_group: u8,
}

#[derive(Debug, Clone)]
pub(crate) struct PlanPattern {
    /// Authored loop extent, before a channel script extends its score.
    pub source_length: u32,
    pub note_curves: Vec<windfall_project::NoteExpressionCurve>,
    pub id: PatternId,
    pub signature: windfall_project::TimeSignature,
    pub meters: Result<
        Vec<windfall_project::timeline::MeterSegment>,
        windfall_project::timeline::MeterMapError,
    >,
    /// Loop length in ticks, at least one step.
    pub length: u32,
    /// Sorted by `tick`. Every tick is below `length`.
    pub events: Vec<NoteEvent>,
}

/// A note with swing already applied.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NoteEvent {
    pub source: windfall_project::NoteId,
    /// Start in ticks from the beginning of the pattern.
    pub tick: f64,
    /// Length in ticks, greater than zero.
    pub length: f64,
    /// Index into [`Plan::channels`].
    pub channel: usize,
    pub key: u8,
    pub velocity: f32,
    pub pan: f32,
    pub expression: windfall_dsp::NoteExpression,
    pub scripted: bool,
    pub script_input: Option<windfall_project::scripting::ScriptNote>,
    pub script_tail: Option<f64>,
}

impl PlanPattern {
    pub fn curves_for(
        &self,
        note: windfall_project::NoteId,
    ) -> &[windfall_project::NoteExpressionCurve] {
        let from = self.note_curves.partition_point(|curve| curve.note < note);
        let to = self.note_curves.partition_point(|curve| curve.note <= note);
        &self.note_curves[from..to]
    }
}

impl Plan {
    /// Resolve the saved source independently of event order or repeated clips.
    /// Runtime occurrences supply their own normalized musical position.
    pub fn note_curve_controls(
        &self,
        pattern: PatternId,
        note: windfall_project::NoteId,
        position: f64,
        mut pan: f32,
        mut expression: windfall_dsp::NoteExpression,
    ) -> (f32, windfall_dsp::NoteExpression) {
        let Some(index) = self.pattern_ids.get(pattern.0) else {
            return (pan, expression);
        };
        for curve in self.patterns[index].curves_for(note) {
            let Some(value) = curve.value_at(position) else {
                continue;
            };
            match curve.parameter {
                windfall_project::NoteCurveParameter::Pan => pan = value,
                windfall_project::NoteCurveParameter::Release => expression.release = value,
                windfall_project::NoteCurveParameter::FinePitchCents => {
                    expression.fine_pitch_cents = value
                }
                windfall_project::NoteCurveParameter::ModulationX => {
                    expression.modulation_x = value
                }
                windfall_project::NoteCurveParameter::ModulationY => {
                    expression.modulation_y = value
                }
            }
        }
        (pan.clamp(-1.0, 1.0), expression.clamped())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PlanTrack {
    pub processing: windfall_dsp::TrackParams,
    pub current: bool,
    pub id: TrackId,
    /// Fader gain, or zero when mute or solo silences the track.
    pub gain: f32,
    /// Neither mute nor solo silences the track.
    pub audible: bool,
    pub pan: f32,
    /// Where the post-fader signal goes: the output first, then the sends.
    pub edges: Vec<PlanEdge>,
    /// The effects the track's input runs through before the fader, in
    /// order.
    pub effects: Vec<PlanEffect>,
    /// Indices of the channels whose instruments play into this track.
    pub instruments: Vec<usize>,
    pub external_output: Option<windfall_project::ExternalOutputRoute>,
    pub latency_offset_ms: f64,
    /// Some edge of the track leaves from an effect instead of the fader.
    pub tapped: bool,
}

impl PlanTrack {
    /// Which separately routable outputs of the effect at `place` in the
    /// chain have somewhere to go: bit `n` for output `n`.
    pub fn routed_outputs(&self, place: usize) -> u8 {
        if !self.tapped {
            return 0;
        }
        self.edges
            .iter()
            .filter_map(|edge| edge.tap)
            .filter(|tap| {
                tap.place == place && usize::from(tap.output) < windfall_dsp::MAX_AUX_OUTPUTS
            })
            .fold(0, |mask, tap| mask | (1 << tap.output))
    }
}

/// Only progress is shared: this never grants access to a plugin owner.
/// Unheard prepared slots need no departing definition; heard ones stay
/// until their actual rack splice completes. References retire with plans.
#[derive(Debug)]
pub(crate) struct EffectLife {
    progress: AtomicU8,
    pub generation: u64,
}

impl Default for EffectLife {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self {
            progress: AtomicU8::new(0),
            generation: NEXT.fetch_add(1, Ordering::Relaxed),
        }
    }
}

impl EffectLife {
    #[cfg(test)]
    pub fn heard(&self) -> bool {
        self.progress.load(Ordering::Acquire) == 1
    }
    pub fn hear(&self) {
        self.progress.store(1, Ordering::Release);
    }
    pub fn finish(&self) {
        self.progress.store(2, Ordering::Release);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PlanEffect {
    pub impulse: Option<Arc<windfall_project::ConvolverImpulse>>,
    pub id: EffectId,
    /// Every value inside its range.
    pub params: EffectParams,
    pub enabled: bool,
    pub mix: f32,
    /// The effect is no longer in the project. It keeps its place in the
    /// chain for as long as it takes to fade out. See
    /// [`Plan::keep_leaving`].
    pub leaving: bool,
    pub life: Arc<EffectLife>,
    /// Requested binding/provider/revision at compilation or installation.
    /// Running identity comes from preparation's ledger: this snapshot can
    /// predate attachment, and a live factory Arc can change revision.
    native_owner: Option<u64>,
    /// A restored id waits for this audible departure before its fresh
    /// owner joins. Progress only; never a native-owner handle or lineage.
    pub after: Option<Arc<EffectLife>>,
    pub departure: Option<Arc<DepartureReservation>>,
    pub after_departure: Option<Arc<DepartureReservation>>,
}

/// Two possible generations share one outgoing physical rack row. The choice
/// is evidence only after the audio thread has taken the actual predecessor.
#[derive(Debug)]
pub(crate) struct DepartureReservation {
    pub sources: [Option<DepartureSource>; 2],
    choice: AtomicU8,
}

#[derive(Debug, Clone)]
pub(crate) struct DepartureSource {
    pub impulse: Option<Arc<windfall_project::ConvolverImpulse>>,
    pub life: Arc<EffectLife>,
    pub params: EffectParams,
    pub enabled: bool,
    pub mix: f32,
    pub native: Option<(u64, usize)>,
}

impl DepartureReservation {
    pub fn possible(&self) -> impl Iterator<Item = &DepartureSource> {
        let choice = self.choice.load(Ordering::Acquire);
        self.sources
            .iter()
            .enumerate()
            .filter_map(move |(index, source)| {
                source
                    .as_ref()
                    .filter(|_| choice == 0 || choice == index as u8 + 2)
            })
    }

    pub fn adopted(&self, source: Option<usize>) {
        self.choice
            .store(source.map_or(1, |index| index as u8 + 2), Ordering::Release);
    }

    pub fn selected(&self) -> Option<&DepartureSource> {
        let choice = self.choice.load(Ordering::Acquire);
        choice
            .checked_sub(2)
            .and_then(|index| self.sources.get(index as usize)?.as_ref())
    }
}

impl PlanEffect {
    pub fn latency_samples(&self, rate: u32) -> usize {
        self.params.latency_samples(rate as f32)
            + self
                .impulse
                .as_ref()
                .map_or(0, |asset| asset.group_delay_at(rate))
    }
    pub fn native_identity(&self) -> Option<u64> {
        self.native_owner
    }
}

/// Where an edge leaves its track when that is not after the fader: one
/// separately routable output of one effect of the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PlanTap {
    pub effect: EffectId,
    pub output: u8,
    /// The effect's place in its track's chain, departing effects counted.
    /// [`Plan::link`] keeps it current; `usize::MAX` matches no place.
    pub place: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PlanEdge {
    /// Set when the edge carries an effect's own output from its place in
    /// the chain, before the fader. Such an edge is neither the track's
    /// output nor one of its sends.
    pub tap: Option<PlanTap>,
    pub sidechain: bool,
    /// Index of the receiving track.
    pub target: usize,
    pub target_id: TrackId,
    /// A send, as opposed to the track's output.
    pub send: bool,
    pub gain: f32,
    /// Index of this edge's ramp in [`PlanState::edges`].
    pub slot: usize,
}

impl PlanEdge {
    /// The same route in another plan: same destination, same kind, and for
    /// an effect's output the same output of the same effect.
    pub fn same_route(&self, other: &PlanEdge) -> bool {
        self.target_id == other.target_id
            && self.send == other.send
            && self.sidechain == other.sidechain
            && self.tap.map(|tap| (tap.effect, tap.output))
                == other.tap.map(|tap| (tap.effect, tap.output))
    }
}

#[derive(Debug, Clone)]
pub(crate) struct PlanClip {
    pub start: u32,
    pub end: u32,
    /// How far into the pattern the clip starts, below the pattern length.
    pub offset: u32,
    /// Index into [`Plan::patterns`].
    pub pattern: usize,
    /// On an instrument track, the only channel of the pattern that sounds:
    /// an index into [`Plan::channels`]. `None` plays every channel.
    pub only: Option<usize>,
    /// Largest `end` among this clip and all clips before it. It lets the
    /// sequencer skip, with one binary search, every clip that ended before
    /// the stretch of time it is looking at.
    pub reach: u32,
}

/// An audio clip: a sample that plays straight onto the timeline.
#[derive(Debug, Clone)]
pub(crate) struct PlanAudioClip {
    pub id: ClipId,
    pub start: u32,
    pub end: u32,
    /// How far into the audio the clip starts, in ticks at the project's
    /// tempo.
    pub offset: u32,
    pub sample: AudioBuffer,
    /// Index of the mixer track the clip plays into, and that track's id.
    pub track: usize,
    pub track_id: TrackId,
    pub direct_output: bool,
    pub gain: f32,
    pub pan: f32,
    /// Length of the fade in, in ticks from the start.
    pub fade_in: u32,
    /// Length of the fade out, in ticks before the end.
    pub fade_out: u32,
    pub reverse: bool,
    pub pitch: f32,
    /// Seconds of the sample that go by in one second of the song.
    pub speed: f64,
    /// Frames of the sample, in playing order, that `offset` skips.
    pub skip: f64,
    /// Largest `end` among this clip and all clips before it.
    pub reach: u32,
}

impl Plan {
    pub(crate) fn snapshot_native_owners(&mut self) {
        self.factory_stamp =
            crate::project_preparation::FactoryStamp::capture(&self.plugin_factory);
        let provider = self.factory_stamp.map_or(0, |stamp| stamp.identity());
        for effect in self.tracks.iter_mut().flat_map(|track| &mut track.effects) {
            if !effect.leaving {
                let key = windfall_project::PluginTarget::Effect { effect: effect.id };
                effect.native_owner = self
                    .plugins
                    .iter()
                    .find(|binding| binding.target == key)
                    .map(|binding| crate::plugins::identity(binding) ^ provider);
            }
        }
    }

    /// A fresh stream has no departing/history owners from the stopped one.
    /// Cloning compiled assets/curves and rebuilding indices happens off guards.
    pub fn for_attachment(&self) -> Self {
        let mut plan = self.clone();
        plan.channels.retain(|channel| !channel.leaving);
        for track in &mut plan.tracks {
            track.effects.retain(|effect| !effect.leaving);
            for effect in &mut track.effects {
                effect.life = Arc::new(EffectLife::default());
                effect.after = None;
                effect.departure = None;
                effect.after_departure = None;
            }
        }
        plan.snapshot_native_owners();
        plan.link();
        plan
    }

    /// What plays before a project is set: nothing, through a master track
    /// at unity so sample previews are still heard.
    pub fn empty() -> Self {
        compile(&Project::new(""), &SamplePool::new())
    }

    /// Keeps what `previous` played until its actual rack splice finishes,
    /// so intervening plans cannot stop a departing effect dead.
    ///
    /// An effect that left the project stays in its track's chain, marked
    /// as leaving, right after the effect it followed. An instrument
    /// channel that left the project is listed after the project's own
    /// channels, on the track it played into or on the master if that
    /// track is gone, at the gain and pan it had. Effect progress is shared
    /// with the rack: unheard speculative slots and completed departures
    /// are excluded here, and their storage retires on the control side.
    /// Instrument channels retain their existing one-plan departure policy.
    /// With a stream, `prepared` supplies the identities actually prepared
    /// or reused, rather than the previous plan's compilation-time snapshot.
    ///
    /// A track that is gone takes its effects with it at once: there is
    /// nowhere left for them to be heard.
    #[cfg(test)]
    pub fn keep_leaving(&mut self, previous: &Plan, prepared: Option<&crate::state::Ledger>) {
        self.echo_history = previous.echo_history.clone();
        // Refresh on installation too: precompilation may precede a retry.
        // Never query the old plan's mutable factory for its past revision.
        self.snapshot_native_owners();
        // Progress follows a retained identity across track moves as well.
        // A restored departing id gets a fresh marker and owner preparation.
        for effect in self.tracks.iter_mut().flat_map(|track| &mut track.effects) {
            let Some(before) = previous.effect_ids.get(effect.id.0) else {
                continue;
            };
            let (track, place) = previous.effect_places[before];
            let before = &previous.tracks[track].effects[place];
            let native = effect.native_owner;
            // A detached or precompiled plan can predate stream preparation.
            // Only the preparation ledger freezes the running native identity;
            // querying the previous factory now would observe its new revision.
            let before_native =
                prepared.map_or(before.native_owner, |held| held.native_identity(effect.id));
            if effect.params.kind() == before.params.kind()
                && native == before_native
                && effect.impulse == before.impulse
            {
                effect.life = before.life.clone();
                effect.after = before.after.as_ref().filter(|life| life.heard()).cloned();
            }
        }
        for track in &mut self.tracks {
            let Some(before) = previous.track_ids.get(track.id.0) else {
                continue;
            };
            // Where the next leaver goes: after the last effect that was
            // ahead of it and is still in this chain.
            let mut at = 0;
            for effect in &previous.tracks[before].effects {
                if let Some(kept) = track
                    .effects
                    .iter()
                    .position(|e| !e.leaving && e.id == effect.id)
                {
                    if effect.leaving && effect.life.heard() {
                        // The restored id owns a fresh unit. Keep the heard
                        // outgoing transfer ahead of it until its existing
                        // splice finishes; repeated restores never restart it.
                        track.effects[kept].after = Some(effect.life.clone());
                        track.effects.insert(kept, effect.clone());
                        at = at.max(kept + 2);
                    } else {
                        at = at.max(kept + 1);
                    }
                } else if self.effect_ids.get(effect.id.0).is_none() && effect.life.heard() {
                    track.effects.insert(
                        at,
                        PlanEffect {
                            leaving: true,
                            after: None,
                            ..effect.clone()
                        },
                    );
                    at += 1;
                }
            }
        }
        for channel in &previous.channels {
            let instrument = channel.instrument.filter(|_| !channel.leaving);
            if instrument.is_none() || self.channel_ids.get(channel.id.0).is_some() {
                continue;
            }
            let track = previous.tracks[channel.track].id;
            self.channels.push(PlanChannel {
                sample_playback: channel.sample_playback.clone(),
                rack: channel.rack.clone(),
                voice: channel.voice,
                expression_map: channel.expression_map,
                id: channel.id,
                track: self.track_ids.get(track.0).unwrap_or(0),
                gain: channel.gain,
                audible: channel.audible,
                pan: channel.pan,
                sampler: PlanSampler::silent(),
                instrument,
                instrument_asset: channel.instrument_asset.clone(),
                sample_bank: channel.sample_bank.clone(),
                layer: None,
                leaving: true,
            });
        }
        self.link();
    }

    /// Reserve the entire late-heard universe before native construction. No
    /// provisional selection is used as actual preparation/history evidence.
    pub fn reserve_departures(
        &mut self,
        previous: &Plan,
        held: &crate::state::Ledger,
    ) -> Result<(), crate::project_preparation::ProjectPreparationError> {
        use crate::project_preparation::ProjectPreparationError;
        self.echo_history = previous.echo_history.clone();
        for effect in self.tracks.iter_mut().flat_map(|track| &mut track.effects) {
            if let Some(index) = previous.effect_ids.get(effect.id.0) {
                let (track, place) = previous.effect_places[index];
                let before = &previous.tracks[track].effects[place];
                if held.reuses_effect(effect, before, held.sample_rate) {
                    effect.life = before.life.clone();
                }
            }
        }
        let retained: HashSet<u64> = self
            .tracks
            .iter()
            .flat_map(|track| &track.effects)
            .map(|effect| effect.life.generation)
            .collect();
        for track in &mut self.tracks {
            let Some(old_index) = previous.track_ids.get(track.id.0) else {
                continue;
            };
            let mut at = 0;
            let mut rows: Vec<(EffectId, Vec<DepartureSource>, usize)> = Vec::new();
            for before in &previous.tracks[old_index].effects {
                // Moving an active id switches its old track immediately,
                // including any unfinished departure reserved for that id.
                if self.effect_ids.get(before.id.0).is_some_and(|index| {
                    self.effect_places[index].0 != self.track_ids.get(track.id.0).unwrap()
                }) {
                    continue;
                }
                let kept = track
                    .effects
                    .iter()
                    .position(|effect| !effect.leaving && effect.id == before.id);
                let retained = retained.contains(&before.life.generation);
                let mut sources = Vec::new();
                if let Some(reservation) = &before.departure {
                    sources.extend(
                        reservation
                            .possible()
                            .filter(|source| source.life.progress.load(Ordering::Acquire) != 2)
                            .cloned(),
                    );
                } else if !retained && self.effect_ids.get(before.id.0).is_none() {
                    // Preserve existing replacement semantics: an active id
                    // whose native identity/kind changes is replaced directly,
                    // not newly promoted into the outgoing chain. Existing
                    // outgoing reservations still retain their actual owners.
                    // Even an unheard active generation may first sound before
                    // adoption. Finished departures alone can be excluded.
                    if !before.leaving || before.life.progress.load(Ordering::Acquire) != 2 {
                        sources.push(DepartureSource {
                            impulse: before.impulse.clone(),
                            life: before.life.clone(),
                            params: before.params,
                            enabled: before.enabled,
                            mix: before.mix,
                            native: held.generation_native(before.id, before.life.generation),
                        });
                    }
                }
                if !sources.is_empty() {
                    if let Some((_, existing, _)) =
                        rows.iter_mut().find(|(id, _, _)| *id == before.id)
                    {
                        for source in sources {
                            if !existing
                                .iter()
                                .any(|old| Arc::ptr_eq(&old.life, &source.life))
                            {
                                existing.push(source);
                            }
                        }
                        if existing.len() > 2 {
                            return Err(ProjectPreparationError::UnresolvedProgress);
                        }
                    } else {
                        rows.push((before.id, sources, at));
                    }
                }
                if let Some(kept) = kept {
                    at = at.max(kept + 1);
                }
            }
            // Insert in old serial order; active indexing is rebuilt afterwards.
            for (inserted, (id, sources, anchor)) in rows.into_iter().enumerate() {
                let first = &sources[0];
                let row = PlanEffect {
                    impulse: first.impulse.clone(),
                    id,
                    params: first.params,
                    enabled: first.enabled,
                    mix: first.mix,
                    leaving: true,
                    life: first.life.clone(),
                    native_owner: first.native.map(|record| record.0),
                    after: None,
                    departure: None,
                    after_departure: None,
                };
                let mut iterator = sources.into_iter();
                let reservation = Arc::new(DepartureReservation {
                    sources: [iterator.next(), iterator.next()],
                    choice: AtomicU8::new(0),
                });
                if let Some(active) = track
                    .effects
                    .iter_mut()
                    .find(|effect| !effect.leaving && effect.id == id)
                {
                    active.after_departure = Some(reservation.clone());
                }
                let index = track
                    .effects
                    .iter()
                    .position(|effect| !effect.leaving && effect.id == id)
                    .unwrap_or((anchor + inserted).min(track.effects.len()));
                track.effects.insert(
                    index,
                    PlanEffect {
                        departure: Some(reservation),
                        ..row
                    },
                );
            }
            if track.effects.len() > 2 * MAX_EFFECT_SLOTS {
                return Err(ProjectPreparationError::UnresolvedProgress);
            }
        }
        for channel in &previous.channels {
            if let Some(instrument) = channel.instrument.filter(|_| !channel.leaving)
                && self.channel_ids.get(channel.id.0).is_none()
            {
                self.channels.push(PlanChannel {
                    sample_playback: channel.sample_playback.clone(),
                    voice: channel.voice,
                    expression_map: channel.expression_map,
                    rack: channel.rack.clone(),
                    id: channel.id,
                    track: self
                        .track_ids
                        .get(previous.tracks[channel.track].id.0)
                        .unwrap_or(0),
                    gain: channel.gain,
                    audible: channel.audible,
                    pan: channel.pan,
                    sampler: PlanSampler::silent(),
                    instrument: Some(instrument),
                    instrument_asset: channel.instrument_asset.clone(),
                    sample_bank: channel.sample_bank.clone(),
                    layer: None,
                    leaving: true,
                });
            }
        }
        self.link();
        Ok(())
    }

    /// Works out the lists that follow from the channels and the chains.
    fn link(&mut self) {
        self._asset_pins = windfall_dsp::asset::AssetPins::for_effects(
            self.tracks
                .iter()
                .flat_map(|track| track.effects.iter().map(|effect| &effect.params)),
        );
        for channel in &self.channels {
            if let Some(graph) = &channel.rack {
                self._asset_pins.extend(graph.effect_asset_pins());
            }
        }
        for track in &mut self.tracks {
            track.instruments.clear();
        }
        for (index, channel) in self.channels.iter().enumerate() {
            if channel.instrument.is_some() {
                self.tracks[channel.track].instruments.push(index);
            }
        }
        for track in &mut self.tracks {
            let effects = &track.effects;
            for tap in track.edges.iter_mut().filter_map(|edge| edge.tap.as_mut()) {
                tap.place = effects
                    .iter()
                    .position(|effect| !effect.leaving && effect.id == tap.effect)
                    .unwrap_or(usize::MAX);
            }
        }
        let places = self.tracks.iter().enumerate().flat_map(|(track, entry)| {
            let effects = entry.effects.iter().enumerate();
            effects
                .filter(|(_, effect)| !effect.leaving)
                .map(move |(place, effect)| (effect.id, (track, place)))
        });
        let (ids, places): (Vec<u32>, Vec<(usize, usize)>) =
            places.map(|(id, place)| (id.0, place)).unzip();
        self.effect_ids = IdIndex::new(ids.into_iter());
        self.effect_places = places;
        self.compile_waves();
    }

    fn compile_waves(&mut self) {
        let dependencies: Vec<_> = self
            .tracks
            .iter()
            .enumerate()
            .flat_map(|(source, track)| track.edges.iter().map(move |edge| (source, edge.target)))
            .collect();
        let serial: Vec<_> = self
            .tracks
            .iter()
            .enumerate()
            .filter_map(|(index, track)| {
                (track.current
                    || track.tapped
                    || !self.control_sources.is_empty()
                    || track
                        .effects
                        .iter()
                        .any(|effect| effect.native_owner.is_some()))
                .then_some(index)
            })
            .collect();
        self.waves = crate::track_workers::GraphWaves::compile(
            &self.order,
            self.tracks.len(),
            &dependencies,
            &serial,
        )
        .expect("routing compiler supplies a topological order");
    }

    /// The channel with this id, if the project has it.
    pub fn channel(&self, id: ChannelId) -> Option<usize> {
        self.channel_ids.get(id.0)
    }

    /// Internal Tempo links select the integrated, unwarped runtime clock.
    pub fn linked_tempo(&self) -> bool {
        crate::internal_controllers::has_tempo(&self.control_links)
    }

    /// The clock domain for a song tick: static map time, or musical ticks
    /// while a bounded internal link integrates tempo on the control grid.
    pub fn warp(&self, tick: f64) -> f64 {
        if self.linked_tempo() {
            return tick;
        }
        match &self.tempo_map {
            Some(map) => map.warp(tick),
            None => tick,
        }
    }

    /// The inverse of [`warp`](Self::warp).
    pub fn unwarp(&self, warped: f64) -> f64 {
        if self.linked_tempo() {
            return warped;
        }
        match &self.tempo_map {
            Some(map) => map.unwarp(warped),
            None => warped,
        }
    }

    /// True when the plan keeps `buffer` alive, which makes dropping another
    /// handle to it free of any deallocation.
    pub fn holds(&self, buffer: &AudioBuffer) -> bool {
        let channels = self.channels.iter();
        let mut samples = channels
            .filter_map(|channel| channel.sampler.sample.as_ref())
            .chain(self.audio_clips.iter().map(|clip| &clip.sample));
        samples.any(|sample| same_audio(sample, buffer))
            || self.channels.iter().any(|channel| {
                channel
                    .sampler
                    .bank
                    .as_ref()
                    .is_some_and(|bank| bank.holds(buffer))
            })
    }

    pub fn holds_sampler_bank(
        &self,
        bank: &std::sync::Arc<crate::sampler_processing::SamplerBank>,
    ) -> bool {
        self.channels.iter().any(|channel| {
            channel
                .sampler
                .bank
                .as_ref()
                .is_some_and(|held| std::sync::Arc::ptr_eq(held, bank))
        })
    }
}

/// True when both handles share one block of sample data.
pub(crate) fn same_audio(a: &AudioBuffer, b: &AudioBuffer) -> bool {
    std::ptr::eq(a.samples(), b.samples())
}

impl PlanSampler {
    /// A sampler with nothing to play.
    fn silent() -> Self {
        compile_sampler(&SamplerSettings::default(), &SamplePool::new())
    }
}

/// Builds the plan for a project. Samples missing from `pool` leave their
/// channels silent.
///
/// The model's rules are not assumed to hold: values out of range are
/// clamped, and references to things that do not exist are dropped.
pub(crate) fn compile(project: &Project, pool: &SamplePool) -> Plan {
    // Notes and clips play on the grid of the project's timebase. At the
    // default of 960 ticks this borrows the project as it is.
    let on_grid = windfall_project::timebase::playback_project(project);
    let project: &Project = &on_grid;
    let tempo_bpm = if project.settings.tempo_bpm.is_finite() {
        project
            .settings
            .tempo_bpm
            .clamp(MIN_TEMPO_BPM, MAX_TEMPO_BPM)
    } else {
        FALLBACK_TEMPO_BPM
    };
    let swing = f64::from(unit(project.settings.swing));

    let (mut tracks, order) = compile_mixer_plugins(&project.mixer, &project.plugins);
    for effect in tracks.iter_mut().flat_map(|track| &mut track.effects) {
        if let EffectParams::Scratch(p) = &mut effect.params {
            p.runtime_source = pool
                .get(windfall_project::SampleId(p.source_id))
                .and_then(|audio| windfall_dsp::asset::publish_audio(audio).ok())
                .unwrap_or(windfall_dsp::asset::SourceRef::NONE);
        }
    }
    let track_ids = IdIndex::new(tracks.iter().map(|track| track.id.0));
    let edge_count = tracks.iter().map(|track| track.edges.len()).sum();

    // A soloed layer keeps the channels it plays audible.
    let solo_audible = windfall_project::layer::solo_audible(&project.channels);
    let channels: Vec<PlanChannel> = project
        .channels
        .iter()
        .enumerate()
        .map(|(index, channel)| {
            let mut compiled = compile_channel(channel, pool, &track_ids, solo_audible[index]);
            compiled.rack = project
                .settings
                .modular_racks
                .iter()
                .find(|rack| rack.channel == channel.id)
                .map(|rack| Arc::new(rack.graph.clone()));
            if windfall_project::layer::is_layer(channel) {
                compiled.layer =
                    Some(windfall_project::layer::targets(&project.channels, index).into());
            }
            compiled
        })
        .collect();
    let channel_ids = IdIndex::new(channels.iter().map(|channel| channel.id.0));

    let script_notes = crate::script_notes::prepare(project);
    let patterns: Vec<PlanPattern> = project
        .patterns
        .iter()
        .map(|pattern| {
            compile_pattern(
                pattern,
                &channel_ids,
                &project.channels,
                swing,
                project.settings.time_signature,
                &script_notes,
            )
        })
        .collect();
    let mut patterns: Vec<PlanPattern> = patterns;
    // Entity IDs are globally unique. Clip-backed pads use their clip ID in
    // the existing launcher protocol, without changing launch ordering.
    for pad in &project.settings.stage_media.pads {
        if let Some(clip) = pad
            .clip
            .and_then(|id| project.playlist.clips.iter().find(|clip| clip.id == id))
            && !patterns.iter().any(|pattern| pattern.id.0 == clip.id.0)
        {
            patterns.push(PlanPattern {
                id: PatternId(clip.id.0),
                source_length: clip.length.max(1),
                length: clip.length.max(1),
                signature: project.settings.time_signature,
                meters: windfall_project::MeterMap::checked(project.settings.time_signature, &[])
                    .map(|map| map.segments().to_vec()),
                events: Vec::new(),
                note_curves: Vec::new(),
            });
        }
    }
    let pattern_ids = IdIndex::new(patterns.iter().map(|pattern| pattern.id.0));

    let playlist = arrangement_playlist(&project.playlist);
    let (clips, song_end) = compile_playlist(&playlist, &pattern_ids, &patterns, &channel_ids);
    let audio_clips = compile_audio_clips(&playlist, pool, &track_ids, tempo_bpm);
    let audio_clip_ids = IdIndex::new(audio_clips.iter().map(|clip| clip.id.0));

    let waves = crate::track_workers::GraphWaves::compile(&order, tracks.len(), &[], &[])
        .expect("compiled track order");
    let script_parameters = project
        .settings
        .scripts
        .parameter_addresses
        .iter()
        .enumerate()
        .map(|(index, address)| {
            let channel = channel_ids.get(address.channel.0)?;
            let (parameter, _, _) = project.settings.scripts.parameter(
                address.channel,
                windfall_project::scripting::SCRIPT_PARAMETER_BASE + index as u32,
            )?;
            Some((channel, parameter))
        })
        .collect();
    let mut plan = Plan {
        performance_clips: Vec::new(),
        performance_clip_ids: IdIndex::default(),
        performance_tempo: false,
        performance_tick: Arc::new(AtomicU64::new(0)),
        videos: crate::video_audio::compile(project, pool, &track_ids),
        echo_history: Arc::new(crate::effect_host::HistoryPool::default()),
        _asset_pins: windfall_dsp::asset::AssetPins::default(),
        navigation: crate::timeline::compile(&project.playlist.timeline),
        song_start: project.playlist.timeline.start_tick(),
        signature: project.settings.time_signature,
        meters: windfall_project::timeline::MeterMap::checked(
            project.settings.time_signature,
            &project.playlist.timeline.meters,
        )
        .map(|map| map.segments().to_vec()),
        plugins: project.plugin_bindings().into_iter().cloned().collect(),
        plugin_factory: pool.plugin_factory.clone(),
        // T1 refuses invalid meter maps before any provider metadata access.
        // snapshot_native_owners below fills this only for a valid plan.
        factory_stamp: None,
        tempo_bpm,
        pan_law: project.settings.pan_law.core(),
        channels,
        channel_ids,
        patterns,
        pattern_ids,
        tracks,
        track_ids,
        waves,
        audio_threads: 0,
        order,
        edge_count,
        effect_ids: IdIndex::default(),
        effect_places: Vec::new(),
        clips,
        audio_clips,
        audio_clip_ids,
        song_end: project
            .settings
            .stage_media
            .videos
            .iter()
            .fold(song_end, |end, video| {
                end.max(video.start_tick.saturating_add(video.length_ticks))
            }),
        lanes: Arc::from([]),
        control_sources: Vec::new(),
        control_links: Vec::new(),
        pattern_lanes: false,
        tempo_map: None,
        tempo_replay: Arc::from([]),
        scripts: script_notes,
        script_parameters,
    };
    if plan.meters.is_ok() {
        plan.snapshot_native_owners();
    }
    plan.link();
    plan.lanes = automation::compile(project, &playlist, &plan).into();
    (plan.control_sources, plan.control_links) =
        crate::internal_controllers::compile(project, &plan);
    plan.compile_waves();
    plan.pattern_lanes = plan.lanes.iter().any(|lane| lane.in_patterns());
    let tempo_lane = plan.lanes.iter().find(|lane| lane.is_tempo());
    plan.tempo_map =
        tempo_lane.map(|lane| TempoMap::new(lane, tempo_bpm, f64::from(plan.song_end)));
    plan.performance_clips = crate::performance_clips::compile(project, pool, &plan);
    plan.performance_clip_ids = IdIndex::new(plan.performance_clips.iter().map(|clip| clip.id.0));
    plan.performance_tempo = plan.performance_clips.iter().any(|clip| clip.has_tempo());
    plan
}

pub(crate) fn compile_render(project: &Project, pool: &SamplePool) -> Plan {
    let mut pool = pool.clone();
    if let Some(factory) = &pool.plugin_factory
        && let Some(render) = factory.render_factory()
    {
        pool.plugin_factory = Some(render);
    }
    compile(project, &pool)
}

fn compile_channel(
    channel: &Channel,
    pool: &SamplePool,
    track_ids: &IdIndex,
    solo_audible: bool,
) -> PlanChannel {
    let (sampler, instrument) = match &channel.source {
        ChannelSource::Sampler(settings) => (compile_sampler(settings, pool), None),
        ChannelSource::Instrument { params, .. } => {
            (PlanSampler::silent(), Some(params.sanitized()))
        }
    };
    let audible = !channel.muted && solo_audible;
    PlanChannel {
        sample_playback: channel
            .sample_playback
            .as_ref()
            .and_then(|settings| crate::sample_playback::prepare(settings, pool).ok()),
        rack: None,
        voice: channel.voice.sanitized(),
        expression_map: channel.expression_map.sanitized(),
        id: channel.id,
        // A channel whose track is gone plays into the master.
        track: track_ids.get(channel.mixer_track.0).unwrap_or(0),
        gain: if audible { gain(channel.volume) } else { 0.0 },
        audible,
        pan: pan(channel.pan),
        sampler,
        instrument,
        instrument_asset: match &channel.source {
            ChannelSource::Instrument { asset, .. } => {
                asset.as_ref().map(|asset| Arc::new(asset.clone()))
            }
            ChannelSource::Sampler(_) => None,
        },
        sample_bank: pool.sample_instrument_bank(channel.id),
        layer: None,
        leaving: false,
    }
}

impl PlanChannel {
    pub fn latency_samples(&self, rate: f32) -> usize {
        self.rack.as_ref().map_or_else(
            || {
                self.instrument
                    .map_or(0, |params| params.latency_samples(rate))
            },
            |graph| {
                graph.latency_with_input(
                    rate,
                    self.instrument
                        .map_or(0, |params| params.latency_samples(rate)),
                )
            },
        )
    }
}

fn compile_sampler(settings: &SamplerSettings, pool: &SamplePool) -> PlanSampler {
    let spectral = matches!(
        settings.stretch,
        windfall_project::SamplerStretch::Spectral { .. }
    );
    let bank = spectral.then(|| pool.sampler_bank(settings)).flatten();
    let sample = settings
        .sample
        .and_then(|id| pool.get(id))
        .filter(|buffer| buffer.frames() > 0)
        .cloned();
    let frames = sample.as_ref().map_or(0, AudioBuffer::frames);
    let start_fraction = f64::from(unit(settings.start));
    let end_fraction = if settings.end.is_finite() {
        f64::from(settings.end.clamp(0.0, 1.0))
    } else {
        1.0
    };
    let start = ((start_fraction * frames as f64).round() as usize).min(frames.saturating_sub(1));
    let end =
        ((end_fraction * frames as f64).round() as usize).clamp((start + 1).min(frames), frames);
    let tune = if settings.tune.is_finite() {
        settings.tune
    } else {
        0.0
    };
    let region_frames = bank.as_ref().map_or(end - start, |bank| bank.frames);
    let loop_region = (settings.loop_mode != windfall_project::SamplerLoopMode::Off
        && region_frames > 0)
        .then(|| {
            let first = ((f64::from(unit(settings.loop_start)) * region_frames as f64).round()
                as usize)
                .min(region_frames - 1);
            let end_fraction = if settings.loop_end.is_finite() {
                unit(settings.loop_end)
            } else {
                1.0
            };
            let end = ((f64::from(end_fraction) * region_frames as f64).round() as usize)
                .clamp(first + 1, region_frames);
            LoopRegion {
                first: if settings.reverse {
                    region_frames - end
                } else {
                    first
                },
                frames: end - first,
                mode: settings.loop_mode,
                crossfade: settings.loop_crossfade,
            }
        });
    PlanSampler {
        bank,
        spectral,
        sample,
        start,
        end,
        reverse: settings.reverse,
        loop_region,
        key_offset: tune - f32::from(settings.root_key),
        gain: gain(settings.gain),
        envelope: settings
            .envelope
            .or_else(|| {
                loop_region.map(|_| Envelope {
                    attack_ms: 0.0,
                    decay_ms: 0.0,
                    sustain: 1.0,
                    release_ms: 4.0,
                })
            })
            .map(|envelope| Envelope {
                attack_ms: duration(envelope.attack_ms),
                decay_ms: duration(envelope.decay_ms),
                sustain: unit(envelope.sustain),
                release_ms: duration(envelope.release_ms),
            }),
        cut_self: settings.cut_self,
        cut_group: settings.cut_group,
    }
}

fn compile_pattern(
    pattern: &Pattern,
    channel_ids: &IdIndex,
    source_channels: &[Channel],
    swing: f64,
    legacy: windfall_project::TimeSignature,
    script_notes: &[Option<windfall_project::scripting::NoteTransform>],
) -> PlanPattern {
    let source_length = pattern.length_steps.max(1).saturating_mul(TICKS_PER_STEP);
    let mut length = source_length;
    // Extend the immutable score before placing notes; a relocated onset must
    // not be dropped just because it is outside the authored loop extent.
    for lane in &pattern.lanes {
        let Some(channel) = channel_ids.get(lane.channel.0) else {
            continue;
        };
        let program = script_notes.get(channel).and_then(Option::as_ref);
        if program.is_some() {
            for note in lane.notes.iter().filter(|note| note.start < source_length) {
                if let Some(program) = program.filter(|p| p.dynamic()) {
                    length = length.max(
                        program
                            .maximum_end(windfall_project::scripting::ScriptNote::from(note))
                            .unwrap_or(note.start.saturating_add(note.length)),
                    );
                }
                for output in crate::script_notes::transform(program, note).as_slice() {
                    length = length.max(output.start.saturating_add(output.length));
                }
            }
        }
    }

    let mut events = Vec::new();
    for lane in &pattern.lanes {
        let Some(channel) = channel_ids.get(lane.channel.0) else {
            continue;
        };
        let timing = source_channels[channel].timing;
        for note in lane.notes.iter().filter(|note| note.start < source_length) {
            let program = script_notes.get(channel).and_then(Option::as_ref);
            let transformed_notes = if program.is_some_and(|p| p.dynamic()) {
                windfall_project::scripting::TransformedNotes::single(
                    windfall_project::scripting::ScriptNote::from(note),
                )
            } else {
                crate::script_notes::transform(program, note)
            };
            for transformed in transformed_notes.as_slice() {
                let Some((tick, duration)) =
                    timing.place(transformed.start, transformed.length, length, swing)
                else {
                    continue;
                };
                events.push(NoteEvent {
                    source: note.id,
                    tick,
                    length: duration,
                    channel,
                    key: transformed.key.min(MAX_KEY),
                    velocity: unit(transformed.velocity),
                    pan: pan(transformed.pan),
                    expression: note.expression.clamped(),
                    scripted: program.is_some(),
                    script_input: program
                        .filter(|p| p.dynamic())
                        .map(|_| windfall_project::scripting::ScriptNote::from(note)),
                    script_tail: program
                        .filter(|p| p.dynamic())
                        .and_then(|p| {
                            p.maximum_end(windfall_project::scripting::ScriptNote::from(note))
                                .ok()
                        })
                        .map(|end| tick + f64::from(end - note.start)),
                });
            }
        }
    }
    // The sort is stable, so notes on the same tick keep lane order.
    events.sort_by(|a, b| a.tick.total_cmp(&b.tick));
    let mut note_curves = pattern.note_curves.clone();
    note_curves.sort_by_key(|curve| (curve.note, curve.parameter));
    PlanPattern {
        source_length,
        note_curves,
        id: pattern.id,
        signature: pattern.effective_signature(legacy),
        meters: windfall_project::MeterMap::checked(
            pattern.effective_signature(legacy),
            &pattern.timeline.meters,
        )
        .map(|map| map.segments().to_vec()),
        length,
        events,
    }
}

/// Moves a tick to where swing puts it.
///
/// Time is stretched inside each pair of steps: the first step grows by the
/// swing delay and the second shrinks by it, so the pair keeps its length, a
/// note on the second step lands late by exactly the delay, and notes between
/// steps keep their order. Ticks at or past `swung_length` are left alone.
#[cfg(test)]
fn swing_warp(tick: f64, swing: f64, swung_length: f64) -> f64 {
    windfall_project::note_timing::swing_warp(tick, swing, swung_length)
}

/// Resolve arrangement references on the control side, before mute, solo and
/// crossfade compilation. The stored playlist and its clip positions stay intact.
pub(crate) fn arrangement_playlist(playlist: &Playlist) -> Cow<'_, Playlist> {
    let book = &playlist.arrangement_book;
    if book.arrangements.is_empty() || book.active.is_none() {
        return Cow::Borrowed(playlist);
    }
    let arrangement = book
        .arrangements
        .iter()
        .find(|item| Some(item.id) == book.active);
    let mut selected = playlist.clone();
    selected.tracks = arrangement
        .filter(|item| !item.clips.is_empty())
        .into_iter()
        .flat_map(|item| &item.tracks)
        .filter_map(|id| playlist.tracks.iter().find(|track| track.id == *id))
        .cloned()
        .collect();
    let tracks: HashSet<_> = selected.tracks.iter().map(|track| track.id).collect();
    let clips: HashSet<_> = arrangement
        .into_iter()
        .flat_map(|item| &item.clips)
        .collect();
    selected
        .clips
        .retain(|clip| clips.contains(&clip.id) && tracks.contains(&clip.track));
    Cow::Owned(selected)
}

/// Playlist mute and solo share one rule for pattern, audio and automation clips.
pub(crate) fn silent_playlist_tracks(
    playlist: &Playlist,
) -> HashSet<windfall_project::PlaylistTrackId> {
    let any_solo = playlist.tracks.iter().any(|track| track.solo);
    playlist
        .tracks
        .iter()
        .filter(|track| track.muted || (any_solo && !track.solo))
        .map(|track| track.id)
        .collect()
}

fn compile_playlist(
    playlist: &Playlist,
    pattern_ids: &IdIndex,
    patterns: &[PlanPattern],
    channel_ids: &IdIndex,
) -> (Vec<PlanClip>, u32) {
    let silent_tracks = silent_playlist_tracks(playlist);
    let links = &playlist.arrangement_book.linked_tracks;

    let mut song_end = 0;
    let mut clips = Vec::new();
    for clip in &playlist.clips {
        let pattern = match clip.content {
            ClipContent::Pattern { pattern } => pattern_ids.get(pattern.0),
            _ => None,
        };
        let extension = pattern.map_or(0, |index| {
            let score = &patterns[index];
            clip.length
                .div_ceil(score.source_length)
                .saturating_mul(score.length.saturating_sub(score.source_length))
        });
        let end = clip
            .start
            .saturating_add(clip.length)
            .saturating_add(extension)
            .min(windfall_project::MAX_SONG_TICKS);
        // A silent clip still takes up room on the timeline, so mute and
        // solo do not shorten the song.
        song_end = song_end.max(end);
        if clip.muted || silent_tracks.contains(&clip.track) || end <= clip.start {
            continue;
        }
        let ClipContent::Pattern { pattern } = &clip.content else {
            continue;
        };
        let Some(pattern) = pattern_ids.get(pattern.0) else {
            continue;
        };
        // An instrument track plays its own channel only. A link whose
        // channel is gone plays nothing rather than everything.
        let only = match links.get(&clip.track) {
            Some(windfall_project::arrangement::TrackKind::Instrument { channel }) => {
                match channel_ids.get(channel.0) {
                    Some(index) => Some(index),
                    None => continue,
                }
            }
            _ => None,
        };
        // Short windows limit onsets; admitted scripted notes keep their ends.
        let score = &patterns[pattern];
        let offset = clip.offset % score.length;
        let origin = f64::from(clip.start) - f64::from(offset);
        let last_repeat =
            (u64::from(end - clip.start - 1) + u64::from(offset)) / u64::from(score.length);
        for repeat in [last_repeat.saturating_sub(1), last_repeat] {
            let shift = origin + repeat as f64 * f64::from(score.length);
            for event in score.events.iter().filter(|event| event.scripted) {
                let tick = shift + event.tick;
                if tick >= f64::from(clip.start)
                    && tick < f64::from(end)
                    && only.is_none_or(|only| only == event.channel)
                {
                    song_end = song_end.max(
                        (shift + event.script_tail.unwrap_or(event.tick + event.length)).ceil()
                            as u32,
                    );
                }
            }
        }
        clips.push(PlanClip {
            start: clip.start,
            end,
            offset: clip.offset % patterns[pattern].length,
            pattern,
            only,
            reach: end,
        });
    }
    clips.sort_by_key(|clip| clip.start);
    let mut reach = 0;
    for clip in &mut clips {
        reach = reach.max(clip.end);
        clip.reach = reach;
    }
    (clips, song_end)
}

/// The audio clips that sound: allowed by clip mute and playlist mute/solo,
/// with audio in the pool. A clip whose mixer track is gone plays into the master.
pub(crate) fn compile_audio_clips(
    playlist: &Playlist,
    pool: &SamplePool,
    track_ids: &IdIndex,
    tempo_bpm: f64,
) -> Vec<PlanAudioClip> {
    let silent_tracks = silent_playlist_tracks(playlist);
    let mut clips = Vec::new();
    let mut playlist_order = Vec::new();
    for clip in &playlist.clips {
        let ClipContent::Audio {
            sample,
            mixer_track,
            output,
            normalize,
            gain: clip_gain,
            pan: clip_pan,
            fade_in,
            fade_out,
            reverse,
            pitch,
            stretch,
        } = clip.content
        else {
            continue;
        };
        let end = clip.start.saturating_add(clip.length);
        let buffer = pool
            .clip_audio(sample, stretch, pitch)
            .filter(|buffer| buffer.frames() > 0);
        let Some(buffer) = buffer.filter(|_| !clip.muted && end > clip.start) else {
            continue;
        };
        if silent_tracks.contains(&clip.track) {
            continue;
        }
        let pitch = if pitch.is_finite() {
            pitch.clamp(-MAX_TUNE_SEMITONES, MAX_TUNE_SEMITONES)
        } else {
            0.0
        };
        let speed = if matches!(stretch, windfall_project::ClipStretch::Tape) {
            2.0_f64.powf(f64::from(pitch) / 12.0)
        } else {
            1.0
        };
        let offset_seconds = f64::from(clip.offset) * 60.0 / (tempo_bpm * f64::from(PPQ));
        let track = track_ids.get(mixer_track.0);
        playlist_order.push((clip.track, clips.len()));
        clips.push(PlanAudioClip {
            id: clip.id,
            start: clip.start,
            end,
            offset: clip.offset,
            sample: buffer.clone(),
            track: track.unwrap_or(0),
            track_id: track.map_or(TrackId::MASTER, |_| mixer_track),
            direct_output: output == windfall_project::ClipAudioOutput::Direct,
            gain: {
                let knob_gain = gain(clip_gain);
                if normalize {
                    // Prepared audio is scanned on the control side, never in the callback.
                    // Propagate any non-finite sample instead of letting f32::max hide NaN.
                    let peak = buffer.samples().iter().fold(0.0_f32, |peak, sample| {
                        if peak.is_finite() && sample.is_finite() {
                            peak.max(sample.abs())
                        } else {
                            f32::NAN
                        }
                    });
                    if peak.is_finite() && peak >= 1.0e-8 {
                        (knob_gain / peak).min(MAX_GAIN)
                    } else {
                        knob_gain
                    }
                } else {
                    knob_gain
                }
            },
            pan: pan(clip_pan),
            fade_in,
            fade_out,
            reverse,
            pitch,
            speed,
            skip: offset_seconds * speed * f64::from(buffer.sample_rate()),
            reach: end,
        });
    }
    // Crossfade only adjacent audible clips on the same playlist track.
    // Keep this temporary ordering on the control side; playback needs the
    // global start ordering below, regardless of playlist or mixer track.
    playlist_order.sort_by_key(|&(track, index)| (track, clips[index].start, clips[index].id));
    for pair in playlist_order.windows(2) {
        let [(previous_track, previous), (next_track, next)] = [pair[0], pair[1]];
        if previous_track != next_track || clips[next].start >= clips[previous].end {
            continue;
        }
        let overlap = clips[previous].end - clips[next].start;
        let previous_length = clips[previous].end - clips[previous].start;
        let next_length = clips[next].end - clips[next].start;
        clips[previous].fade_out = clips[previous].fade_out.max(overlap).min(previous_length);
        clips[next].fade_in = clips[next].fade_in.max(overlap).min(next_length);
    }
    // Clips that start together take their slots in the order of their
    // ids, which settles who is left out when there are too many.
    clips.sort_by_key(|clip| (clip.start, clip.id));
    let mut reach = 0;
    for clip in &mut clips {
        reach = reach.max(clip.end);
        clip.reach = reach;
    }
    clips
}

/// Builds the mixer tracks and the order to process them in.
///
/// Mute and solo are folded into each track's gain. A muted track is always
/// silent. While any track is soloed, a track is heard only if it is soloed
/// itself, feeds a soloed track through outputs and sends, or carries a
/// soloed track's signal on toward the master.
#[cfg(test)]
fn compile_mixer(mixer: &Mixer) -> (Vec<PlanTrack>, Vec<usize>) {
    compile_mixer_plugins(mixer, &[])
}
fn compile_mixer_plugins(
    mixer: &Mixer,
    plugins: &[windfall_project::PluginBinding],
) -> (Vec<PlanTrack>, Vec<usize>) {
    let fallback_master = [MixerTrack {
        dock: windfall_project::MixerDock::default(),
        external_output: None,
        processing: windfall_dsp::TrackParams::default(),
        current: false,
        latency_offset_ms: 0.0,
        id: TrackId::MASTER,
        name: String::new(),
        color: 0,
        volume: 1.0,
        pan: 0.0,
        muted: false,
        solo: false,
        output: None,
        sidechains: Vec::new(),
        sends: Vec::new(),
        effects: Vec::new(),
        recording: None,
    }];
    let source: &[MixerTrack] = if mixer.tracks.is_empty() {
        &fallback_master
    } else {
        &mixer.tracks[..mixer.tracks.len().min(MAX_MIXER_TRACKS)]
    };
    let ids = IdIndex::new(source.iter().map(|track| track.id.0));
    // An effect id is one effect. If a damaged project repeats one, the
    // first wins.
    let mut effects_seen = HashSet::new();

    let mut tracks: Vec<PlanTrack> = source
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let edge = |target_id: TrackId, send: bool, sidechain: bool, gain: f32| {
                let target = ids.get(target_id.0)?;
                // The master is the end of the chain, and nothing feeds itself.
                (index != 0 && target != index && !track.current && !source[target].current)
                    .then_some(PlanEdge {
                        tap: None,
                        target,
                        target_id,
                        send,
                        sidechain,
                        gain,
                        slot: 0,
                    })
            };
            let output = track
                .output
                .filter(|_| !track.external_output.is_some_and(|route| route.exclusive))
                .and_then(|target| edge(target, false, false, 1.0));
            let sends = track
                .sends
                .iter()
                .filter_map(|send| edge(send.target, true, false, gain(send.gain)));
            let keys = track
                .sidechains
                .iter()
                .filter_map(|send| edge(send.target, true, true, gain(send.gain)));
            let effects: Vec<PlanEffect> = track
                .effects
                .iter()
                .filter(|effect| effects_seen.insert(effect.id))
                .take(MAX_EFFECT_SLOTS)
                .map(|effect| PlanEffect {
                    impulse: effect.impulse.clone().map(Arc::new),
                    id: effect.id,
                    params: effect.params.sanitized(),
                    enabled: effect.enabled,
                    mix: if effect.mix.is_finite() {
                        effect.mix.clamp(0.0, 1.0)
                    } else {
                        1.0
                    },
                    leaving: false,
                    life: Arc::new(EffectLife::default()),
                    native_owner: None,
                    after: None,
                    departure: None,
                    after_departure: None,
                })
                .collect();
            // The routed outputs of this track's effects, one edge each. A
            // damaged project that names an output the effect lacks, or
            // routes one output twice, gets the first sound route only.
            let mut tapped = HashSet::new();
            let taps = mixer.effect_outputs.iter().filter_map(|route| {
                let place = effects
                    .iter()
                    .position(|effect| effect.id == route.effect)?;
                let outputs = effects[place].params.kind().aux_outputs().len();
                if usize::from(route.output) >= outputs.min(windfall_dsp::MAX_AUX_OUTPUTS)
                    || !tapped.insert((route.effect, route.output))
                {
                    return None;
                }
                let mut edge = edge(route.target, false, false, 1.0)?;
                edge.tap = Some(PlanTap {
                    effect: route.effect,
                    output: route.output,
                    place,
                });
                Some(edge)
            });
            let native_taps = plugins
                .iter()
                .filter_map(|binding| {
                    let windfall_project::PluginTarget::Effect { effect } = binding.target else {
                        return None;
                    };
                    let place = effects.iter().position(|slot| slot.id == effect)?;
                    Some((binding, effect, place))
                })
                .flat_map(|(binding, effect, place)| {
                    binding.capabilities.routes.iter().filter_map(move |route| {
                        if !binding
                            .capabilities
                            .outputs
                            .iter()
                            .any(|port| port.index == route.output)
                        {
                            return None;
                        }
                        let mut edge = edge(route.track, false, false, 1.0)?;
                        edge.tap = Some(PlanTap {
                            effect,
                            output: route.output as u8,
                            place,
                        });
                        Some(edge)
                    })
                });
            let edges: Vec<PlanEdge> = output
                .into_iter()
                .chain(sends)
                .chain(keys)
                .chain(taps)
                .chain(native_taps)
                .collect();
            PlanTrack {
                processing: track.processing,
                current: track.current,
                id: track.id,
                gain: gain(track.volume),
                audible: true,
                pan: pan(track.pan),
                tapped: edges.iter().any(|edge| edge.tap.is_some()),
                edges,
                effects,
                instruments: Vec::new(),
                external_output: track.external_output,
                latency_offset_ms: track.latency_offset_ms,
            }
        })
        .collect();

    let mut order = route(&mut tracks);
    order.sort_by_key(|&index| tracks[index].current);
    for track in &mut tracks {
        track.tapped = track.edges.iter().any(|edge| edge.tap.is_some());
    }

    let soloed: Vec<bool> = source
        .iter()
        .map(|track| track.solo && !track.current)
        .collect();
    let heard = solo_set(&tracks, &soloed);
    let mut slot = 0;
    for (index, track) in tracks.iter_mut().enumerate() {
        if source[index].muted || !track.current && !heard[index] {
            track.gain = 0.0;
            track.audible = false;
        }
        for edge in &mut track.edges {
            // An effect's own output leaves before the fader, so the fader
            // at zero does not silence it. Mute and solo still must.
            if edge.tap.is_some() && !track.audible {
                edge.gain = 0.0;
            }
            edge.slot = slot;
            slot += 1;
        }
    }
    (tracks, order)
}

/// Orders the tracks so each comes after everything that feeds it. The model
/// forbids routing loops; if one turns up anyway it is broken by dropping
/// the edges that close it, so the mixer can never feed back.
fn route(tracks: &mut [PlanTrack]) -> Vec<usize> {
    let count = tracks.len();
    let mut waiting_on = vec![0usize; count];
    for edge in tracks.iter().flat_map(|track| &track.edges) {
        waiting_on[edge.target] += 1;
    }
    let mut placed = vec![false; count];
    let mut order = Vec::with_capacity(count);
    while order.len() < count {
        let ready = (0..count).find(|&index| !placed[index] && waiting_on[index] == 0);
        let next = ready.unwrap_or_else(|| {
            // Every track left is part of a loop or fed by one. Cut the
            // inputs of the last one, which is never the master unless
            // nothing else is left.
            let victim = (0..count).rfind(|&index| !placed[index]).unwrap_or(0);
            for (index, track) in tracks.iter_mut().enumerate() {
                if !placed[index] {
                    track.edges.retain(|edge| edge.target != victim);
                }
            }
            waiting_on[victim] = 0;
            victim
        });
        placed[next] = true;
        order.push(next);
        for edge in &tracks[next].edges {
            waiting_on[edge.target] -= 1;
        }
    }
    order
}

/// Which tracks solo leaves audible. All of them when nothing is soloed.
fn solo_set(tracks: &[PlanTrack], soloed: &[bool]) -> Vec<bool> {
    if !soloed.contains(&true) {
        return vec![true; tracks.len()];
    }
    let mut feeders = vec![Vec::new(); tracks.len()];
    for (index, track) in tracks.iter().enumerate() {
        for edge in &track.edges {
            feeders[edge.target].push(index);
        }
    }
    let spread = |neighbours: &dyn Fn(usize) -> Vec<usize>| {
        let mut reached = soloed.to_vec();
        let mut pending: Vec<usize> = (0..tracks.len()).filter(|&index| soloed[index]).collect();
        while let Some(index) = pending.pop() {
            for next in neighbours(index) {
                if !reached[next] {
                    reached[next] = true;
                    pending.push(next);
                }
            }
        }
        reached
    };
    let downstream = spread(&|index| {
        tracks[index]
            .edges
            .iter()
            .filter(|edge| !edge.sidechain)
            .map(|edge| edge.target)
            .collect()
    });
    let upstream = spread(&|index| feeders[index].clone());
    downstream
        .iter()
        .zip(&upstream)
        .map(|(down, up)| *down || *up)
        .collect()
}

fn gain(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, MAX_GAIN)
    } else {
        0.0
    }
}

fn pan(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// Clamps to 0..1, with anything that is not a number becoming 0.
fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn duration(ms: f32) -> f32 {
    if ms.is_finite() {
        ms.clamp(0.0, MAX_ENVELOPE_MS)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use windfall_project::{Clip, Lane, Note, NoteId, Send};

    use super::*;

    fn audio_clip(id: u32, track: u32, start: u32, length: u32, fades: (u32, u32)) -> Clip {
        Clip {
            id: ClipId(id),
            track: windfall_project::PlaylistTrackId(track),
            start,
            length,
            offset: 0,
            muted: false,
            content: ClipContent::Audio {
                sample: windfall_project::SampleId(1),
                mixer_track: TrackId::MASTER,
                output: Default::default(),
                normalize: false,
                gain: 1.0,
                pan: 0.0,
                fade_in: fades.0,
                fade_out: fades.1,
                reverse: false,
                pitch: 0.0,
                stretch: Default::default(),
            },
        }
    }

    fn arrangement_fixture() -> (Project, SamplePool) {
        use windfall_project::{
            Automation, AutomationId, AutomationPoint, AutomationTarget, PlaylistTrack,
            PlaylistTrackId,
        };
        let mut project = Project::new("Arrangement playback");
        let mut pool = SamplePool::new();
        pool.insert(
            windfall_project::SampleId(1),
            AudioBuffer::from_interleaved(48_000, 1, vec![0.25; 48_000]),
        );
        for index in 0..2 {
            let track = PlaylistTrackId(50 + index);
            project.playlist.tracks.push(PlaylistTrack {
                id: track,
                name: format!("Track {index}"),
                muted: false,
                solo: false,
                color: 0,
                height: 0,
            });
            let pattern = Clip {
                id: ClipId(60 + index),
                track,
                start: index * PPQ,
                length: PPQ,
                offset: 120,
                muted: false,
                content: ClipContent::Pattern {
                    pattern: project.patterns[0].id,
                },
            };
            let automation = AutomationId(80 + index);
            project.automations.push(Automation {
                id: automation,
                name: format!("Automation {index}"),
                color: 0,
                target: if index == 0 {
                    AutomationTarget::Tempo
                } else {
                    AutomationTarget::TrackVolume {
                        track: TrackId::MASTER,
                    }
                },
                points: vec![AutomationPoint {
                    tick: 0,
                    value: 0.5,
                    curve: 0.0,
                    hold: true,
                }],
            });
            project.playlist.clips.extend([
                pattern.clone(),
                audio_clip(62 + index, track.0, index * PPQ, PPQ, (0, 0)),
                Clip {
                    id: ClipId(64 + index),
                    content: ClipContent::Automation { automation },
                    ..pattern
                },
            ]);
        }
        project.playlist.clips.sort_by_key(Clip::sort_key);
        (project, pool)
    }

    fn select_arrangement(project: &mut Project, clips: &[u32], tracks: &[u32]) {
        project.playlist.arrangement_book = windfall_project::ArrangementBook {
            active: Some(90),
            arrangements: vec![windfall_project::arrangement::Arrangement {
                id: 90,
                name: "Verse".into(),
                clips: clips.iter().copied().map(ClipId).collect(),
                tracks: tracks
                    .iter()
                    .copied()
                    .map(windfall_project::PlaylistTrackId)
                    .collect(),
            }],
            ..Default::default()
        };
    }

    #[test]
    fn arrangement_empty_book_plans_every_audible_clip() {
        let (mut project, pool) = arrangement_fixture();
        project
            .playlist
            .clips
            .iter_mut()
            .find(|clip| clip.id == ClipId(61))
            .unwrap()
            .muted = true;
        let plan = compile(&project, &pool);
        assert_eq!(plan.clips.len(), 1);
        assert_eq!(
            plan.audio_clips
                .iter()
                .map(|clip| clip.id.0)
                .collect::<Vec<_>>(),
            [62, 63]
        );
        assert_eq!(plan.lanes.len(), 2);
        assert_eq!(plan.song_end, 2 * PPQ);
    }

    #[test]
    fn arrangement_active_plans_only_its_clip_without_moving_it() {
        let (mut project, pool) = arrangement_fixture();
        let saved = project.playlist.clips.clone();
        for (clip, pattern_count, audio_count, lane_count) in
            [(61, 1, 0, 0), (63, 0, 1, 0), (65, 0, 0, 1)]
        {
            select_arrangement(&mut project, &[clip], &[51]);
            let plan = compile(&project, &pool);
            assert_eq!(plan.clips.len(), pattern_count);
            assert_eq!(plan.audio_clips.len(), audio_count);
            assert_eq!(plan.lanes.len(), lane_count);
            assert_eq!(plan.song_end, 2 * PPQ);
            if let Some(clip) = plan.clips.first() {
                assert_eq!((clip.start, clip.offset), (PPQ, 120));
            }
            assert_eq!(project.playlist.clips, saved);
        }
    }

    #[test]
    fn arrangement_clip_on_an_excluded_track_is_silent() {
        let (mut project, pool) = arrangement_fixture();
        select_arrangement(&mut project, &[61, 63, 65], &[50]);
        let plan = compile(&project, &pool);
        assert!(plan.clips.is_empty());
        assert!(plan.audio_clips.is_empty());
        assert!(plan.lanes.is_empty());
        assert_eq!(plan.song_end, 0);
    }

    #[test]
    fn arrangement_empty_reference_lists_plan_nothing() {
        let (mut project, pool) = arrangement_fixture();
        for (clips, tracks) in [(&[][..], &[50][..]), (&[60, 62, 64][..], &[][..])] {
            select_arrangement(&mut project, clips, tracks);
            let plan = compile(&project, &pool);
            assert!(plan.clips.is_empty());
            assert!(plan.audio_clips.is_empty());
            assert!(plan.lanes.is_empty());
            assert_eq!(plan.song_end, 0);
        }
    }

    #[test]
    fn arrangement_without_active_id_keeps_all_clips() {
        let (mut project, pool) = arrangement_fixture();
        select_arrangement(&mut project, &[], &[]);
        project.playlist.arrangement_book.active = None;
        let plan = compile(&project, &pool);
        assert_eq!(plan.clips.len(), 2);
        assert_eq!(plan.audio_clips.len(), 2);
        assert_eq!(plan.lanes.len(), 2);
    }

    #[test]
    fn arrangement_applies_mute_solo_and_crossfade_after_filtering() {
        let (mut project, pool) = arrangement_fixture();
        // A hidden solo track must not silence the active track.
        project.playlist.tracks[1].solo = true;
        project
            .playlist
            .clips
            .push(audio_clip(66, 50, PPQ / 2, PPQ, (0, 0)));
        select_arrangement(&mut project, &[60, 62, 64, 66], &[50]);
        let plan = compile(&project, &pool);
        assert_eq!(plan.clips.len(), 1);
        assert_eq!(plan.lanes.len(), 1);
        assert_eq!(plan.audio_clips.len(), 2);
        assert_eq!(plan.audio_clips[0].fade_out, PPQ / 2);
        assert_eq!(plan.audio_clips[1].fade_in, PPQ / 2);
        select_arrangement(&mut project, &[62], &[50]);
        assert_eq!(compile(&project, &pool).audio_clips[0].fade_out, 0);
        project.playlist.tracks[0].muted = true;
        select_arrangement(&mut project, &[60, 62, 64], &[50]);
        let plan = compile(&project, &pool);
        assert!(plan.clips.is_empty());
        assert!(plan.audio_clips.is_empty());
        assert!(plan.lanes.is_empty());
    }

    #[test]
    fn audio_clip_normalize_plans_peak_gain_without_changing_the_knob() {
        for (samples, normalize, knob, expected) in [
            (vec![0.25, -0.5, 0.125, 0.0], true, 1.0, 2.0),
            (vec![0.0; 4], true, 0.75, 0.75),
            (vec![0.5; 4], false, 0.75, 0.75),
            (vec![1.0e-9; 4], true, 0.75, 0.75),
            (vec![0.5, f32::NAN, 0.25, 0.0], true, 0.75, 0.75),
            (vec![0.5, f32::INFINITY, 0.25, 0.0], true, 0.75, 0.75),
            (vec![0.25; 4], true, 1.0, MAX_GAIN),
            (vec![-2.0; 4], true, 1.0, 0.5),
        ] {
            let mut pool = SamplePool::new();
            let buffer = AudioBuffer::from_interleaved(48_000, 2, samples.clone());
            let identity = buffer.identity();
            pool.insert(windfall_project::SampleId(1), buffer);
            let mut project = Project::new("Clip normalize");
            let mut clip = audio_clip(1, 50, 0, PPQ, (0, 0));
            let ClipContent::Audio {
                gain,
                normalize: enabled,
                ..
            } = &mut clip.content
            else {
                unreachable!();
            };
            *gain = knob;
            *enabled = normalize;
            project.playlist.clips.push(clip);
            let stored = project.playlist.clips.clone();
            let plan = compile(&project, &pool);
            assert_eq!(plan.audio_clips.len(), 1);
            assert_eq!(plan.audio_clips[0].gain, expected);
            assert_eq!(project.playlist.clips, stored);
            assert_eq!(plan.audio_clips[0].sample.identity(), identity);
            // Includes NaN cases, whose float equality would fail.
            assert_eq!(
                plan.audio_clips[0]
                    .sample
                    .samples()
                    .iter()
                    .map(|s| s.to_bits())
                    .collect::<Vec<_>>(),
                samples.iter().map(|s| s.to_bits()).collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn adjacent_audio_overlaps_extend_planned_fades_and_keep_longer_user_fades() {
        let mut pool = SamplePool::new();
        pool.insert(
            windfall_project::SampleId(1),
            AudioBuffer::from_interleaved(48_000, 1, vec![0.5; 48_000]),
        );
        for (user_fade, expected_fade) in [(20, 100), (150, 150)] {
            let mut project = Project::new("playlist crossfade");
            // Deliberately unsorted, with another playlist track routed to
            // the same mixer track between the overlapping pair's starts.
            project.playlist.clips = vec![
                audio_clip(30, 50, 100, 200, (user_fade, 7)),
                audio_clip(40, 50, 300, 100, (11, 13)),
                audio_clip(20, 51, 50, 400, (17, 19)),
                audio_clip(10, 50, 0, 200, (5, user_fade)),
            ];
            let stored_clips = project.playlist.clips.clone();
            let plan = compile(&project, &pool);
            let fades: Vec<_> = plan
                .audio_clips
                .iter()
                .map(|clip| (clip.id.0, clip.fade_in, clip.fade_out))
                .collect();
            assert_eq!(
                fades,
                vec![
                    (10, 5, expected_fade),
                    (20, 17, 19),
                    (30, expected_fade, 7),
                    (40, 11, 13),
                ]
            );
            assert_eq!(project.playlist.clips, stored_clips);
        }
    }

    #[test]
    fn adjacent_audio_overlaps_clamp_fades_skip_muted_clips_and_use_id_order() {
        let mut project = Project::new("adjacent overlaps only");
        project
            .playlist
            .tracks
            .push(windfall_project::PlaylistTrack {
                id: windfall_project::PlaylistTrackId(52),
                name: String::new(),
                muted: true,
                solo: false,
                color: 0,
                height: 0,
            });
        let mut muted = audio_clip(15, 50, 0, 50, (0, 0));
        muted.muted = true;
        project.playlist.clips = vec![
            audio_clip(30, 50, 200, 100, (7, 9)),
            audio_clip(20, 50, 0, 100, (0, 0)),
            audio_clip(10, 50, 0, 1_000, (0, 0)),
            muted,
            audio_clip(40, 52, 0, 1_000, (0, 0)),
            audio_clip(50, 52, 100, 1_000, (0, 0)),
        ];
        let mut pool = SamplePool::new();
        pool.insert(
            windfall_project::SampleId(1),
            AudioBuffer::from_interleaved(48_000, 1, vec![0.5; 48_000]),
        );
        let plan = compile(&project, &pool);
        let fades: Vec<_> = plan
            .audio_clips
            .iter()
            .map(|clip| (clip.id.0, clip.fade_in, clip.fade_out))
            .collect();
        // ID 10 precedes ID 20 at the same start. Their overlap is 1,000
        // ticks, clamped to ID 20's 100 ticks. ID 30 overlaps ID 10 only,
        // so it is unchanged: ID 20 is its immediate predecessor.
        assert_eq!(fades, vec![(10, 0, 1_000), (20, 100, 0), (30, 7, 9)]);
    }

    fn track(id: u32, output: Option<u32>, sends: &[u32]) -> MixerTrack {
        MixerTrack {
            dock: windfall_project::MixerDock::default(),
            external_output: None,
            processing: windfall_dsp::TrackParams::default(),
            current: false,
            latency_offset_ms: 0.0,
            id: TrackId(id),
            name: String::new(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            output: output.map(TrackId),
            sidechains: Vec::new(),
            sends: sends
                .iter()
                .map(|&target| Send {
                    target: TrackId(target),
                    gain: 0.5,
                })
                .collect(),
            effects: Vec::new(),
            recording: None,
        }
    }

    fn mixer_gains(tracks: Vec<MixerTrack>) -> Vec<f32> {
        let (tracks, _) = compile_mixer(&Mixer {
            tracks,
            effect_outputs: Vec::new(),
        });
        tracks.iter().map(|track| track.gain).collect()
    }

    #[test]
    fn production_waves_cover_wide_graphs_and_cycle_repair_with_fixed_dependency_order() {
        let mut project = Project::new("wide schedule");
        project
            .mixer
            .tracks
            .extend((1..=64).map(|id| track(id, Some(0), &[])));
        let plan = compile(&project, &SamplePool::new());
        assert_eq!(plan.waves.ranges(), &[0..64, 64..65]);
        project.mixer.tracks[1].output = Some(TrackId(2));
        project.mixer.tracks[2].output = Some(TrackId(1));
        project.mixer.tracks[3]
            .sidechains
            .push(windfall_project::Send {
                target: TrackId(4),
                gain: 1.0,
            });
        let plan = compile(&project, &SamplePool::new());
        let stage = |node| {
            plan.waves
                .ranges()
                .iter()
                .position(|range| plan.order[range.clone()].contains(&node))
                .unwrap()
        };
        for (source, track) in plan.tracks.iter().enumerate() {
            for edge in &track.edges {
                assert!(stage(source) < stage(edge.target));
            }
        }
        assert_eq!(
            plan.waves
                .ranges()
                .iter()
                .map(|range| range.len())
                .sum::<usize>(),
            plan.tracks.len()
        );
        project.mixer.tracks[5].current = true;
        let plan = compile(&project, &SamplePool::new());
        assert!(
            plan.waves
                .ranges()
                .iter()
                .any(|range| range.len() == 1 && plan.order[range.start] == 5)
        );
    }

    #[test]
    fn repeated_restores_keep_one_outgoing_generation_and_one_active_slot_per_id() {
        let mut project = Project::new("bounded restores");
        let slots: Vec<_> = (0..MAX_EFFECT_SLOTS)
            .map(|index| windfall_project::EffectSlot {
                impulse: None,
                id: EffectId(index as u32 + 1),
                params: windfall_project::EffectKind::StereoMatrix.default_params(),
                enabled: true,
                mix: 1.0,
            })
            .collect();
        project.mixer.tracks[0].effects = slots.clone();
        let pool = SamplePool::new();
        let mut previous = compile(&project, &pool);
        let generations: Vec<_> = previous.tracks[0]
            .effects
            .iter()
            .map(|effect| {
                effect.life.hear();
                effect.life.generation
            })
            .collect();
        for _ in 0..1000 {
            project.mixer.tracks[0].effects.clear();
            let mut removed = compile(&project, &pool);
            removed.keep_leaving(&previous, None);
            assert_eq!(removed.tracks[0].effects.len(), MAX_EFFECT_SLOTS);
            assert!(
                removed.tracks[0]
                    .effects
                    .iter()
                    .all(|effect| effect.leaving && effect.after.is_none())
            );
            project.mixer.tracks[0].effects = slots.clone();
            let mut restored = compile(&project, &pool);
            restored.keep_leaving(&removed, None);
            assert_eq!(restored.tracks[0].effects.len(), 2 * MAX_EFFECT_SLOTS);
            for (index, pair) in restored.tracks[0]
                .effects
                .as_chunks::<2>()
                .0
                .iter()
                .enumerate()
            {
                assert!(pair[0].leaving && !pair[1].leaving);
                assert_eq!(pair[0].life.generation, generations[index]);
                assert!(Arc::ptr_eq(pair[1].after.as_ref().unwrap(), &pair[0].life));
                let active = restored.effect_ids.get(pair[1].id.0).unwrap();
                assert_eq!(restored.effect_places[active], (0, index * 2 + 1));
            }
            previous = restored;
        }
        for effect in previous.tracks[0]
            .effects
            .iter()
            .filter(|effect| effect.leaving)
        {
            effect.life.finish();
        }
        let mut settled = compile(&project, &pool);
        settled.keep_leaving(&previous, None);
        assert_eq!(settled.tracks[0].effects.len(), MAX_EFFECT_SLOTS);
        assert!(
            settled.tracks[0]
                .effects
                .iter()
                .all(|effect| !effect.leaving && effect.after.is_none())
        );
    }

    #[test]
    fn swing_delays_the_second_step_of_each_pair() {
        let length = 16.0 * 240.0;
        assert_eq!(swing_warp(0.0, 1.0, length), 0.0);
        assert_eq!(swing_warp(240.0, 1.0, length), 320.0);
        assert_eq!(swing_warp(480.0, 1.0, length), 480.0);
        assert_eq!(swing_warp(720.0, 0.5, length), 760.0);
        assert_eq!(swing_warp(240.0, 0.0, length), 240.0);
    }

    #[test]
    fn swing_keeps_notes_in_order_and_inside_the_pair() {
        let length = 480.0;
        let mut last = -1.0;
        for tick in 0..480 {
            let warped = swing_warp(f64::from(tick), 1.0, length);
            assert!(warped > last && warped < 480.0, "tick {tick} -> {warped}");
            last = warped;
        }
    }

    #[test]
    fn an_unpaired_last_step_is_not_swung() {
        let mut project = Project::new("odd");
        project.channels.push(Channel {
            sample_playback: None,
            id: ChannelId(10),
            name: String::new(),
            color: 0,
            volume: 1.0,
            pan: 0.0,
            muted: false,
            solo: false,
            group: String::new(),
            voice: Default::default(),
            expression_map: Default::default(),
            sample_instrument: None,
            timing: windfall_project::ChannelTiming::default(),
            mixer_track: TrackId::MASTER,
            source: ChannelSource::Sampler(SamplerSettings::default()),
        });
        project.settings.swing = 1.0;
        project.patterns[0].length_steps = 3;
        project.patterns[0].lanes.push(Lane {
            channel: ChannelId(10),
            notes: [240, 600, 720]
                .iter()
                .enumerate()
                .map(|(index, &start)| Note {
                    id: NoteId(100 + index as u32),
                    start,
                    length: 240,
                    key: 60,
                    velocity: 1.0,
                    pan: 0.0,
                    expression: Default::default(),
                })
                .collect(),
        });
        let plan = compile(&project, &SamplePool::new());
        let ticks: Vec<f64> = plan.patterns[0].events.iter().map(|e| e.tick).collect();
        // Step 1 swings, the note inside the unpaired third step does not,
        // and the note at the pattern length is dropped.
        assert_eq!(ticks, vec![320.0, 600.0]);
    }

    #[test]
    fn tracks_are_ordered_after_what_feeds_them() {
        let (tracks, order) = compile_mixer(&Mixer {
            effect_outputs: Vec::new(),
            tracks: vec![
                track(0, None, &[]),
                track(1, Some(3), &[2]),
                track(2, Some(0), &[]),
                track(3, Some(0), &[]),
            ],
        });
        let place = |index: usize| order.iter().position(|&entry| entry == index).unwrap();
        for (index, track) in tracks.iter().enumerate() {
            for edge in &track.edges {
                assert!(place(index) < place(edge.target));
            }
        }
        assert_eq!(order.last(), Some(&0));
    }

    #[test]
    fn a_routing_loop_is_broken_instead_of_followed() {
        let (tracks, order) = compile_mixer(&Mixer {
            effect_outputs: Vec::new(),
            tracks: vec![
                track(0, None, &[]),
                track(1, Some(2), &[]),
                track(2, Some(1), &[0]),
            ],
        });
        assert_eq!(order.len(), 3);
        let place = |index: usize| order.iter().position(|&entry| entry == index).unwrap();
        for (index, track) in tracks.iter().enumerate() {
            for edge in &track.edges {
                assert!(place(index) < place(edge.target));
            }
        }
    }

    #[test]
    fn solo_keeps_feeders_and_the_path_to_the_master() {
        // 1 -> 2 -> 0, 3 -> 0, and 4 only sends into 2.
        let mut tracks = vec![
            track(0, None, &[]),
            track(1, Some(2), &[]),
            track(2, Some(0), &[]),
            track(3, Some(0), &[]),
            track(4, None, &[2]),
        ];
        tracks[2].solo = true;
        assert_eq!(mixer_gains(tracks.clone()), vec![1.0, 1.0, 1.0, 0.0, 1.0]);

        // Mute wins over solo.
        tracks[1].muted = true;
        assert_eq!(mixer_gains(tracks), vec![1.0, 0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn the_master_never_routes_anywhere() {
        let (tracks, _) = compile_mixer(&Mixer {
            effect_outputs: Vec::new(),
            tracks: vec![track(0, Some(1), &[1]), track(1, Some(0), &[])],
        });
        assert!(tracks[0].edges.is_empty());
        assert_eq!(tracks[1].edges.len(), 1);
    }

    #[test]
    fn muted_clips_do_not_play_but_still_set_the_song_length() {
        let mut project = Project::new("song");
        let pattern = project.patterns[0].id;
        let clip = |id: u32, start: u32, muted: bool| windfall_project::Clip {
            id: windfall_project::ClipId(id),
            track: windfall_project::PlaylistTrackId(50),
            start,
            length: 3840,
            offset: 3840 + 240,
            muted,
            content: ClipContent::Pattern { pattern },
        };
        project.playlist.clips = vec![clip(60, 0, false), clip(61, 3840, true)];
        let plan = compile(&project, &SamplePool::new());
        assert_eq!(plan.clips.len(), 1);
        assert_eq!(plan.clips[0].offset, 240);
        assert_eq!(plan.song_end, 7680);
    }
}
