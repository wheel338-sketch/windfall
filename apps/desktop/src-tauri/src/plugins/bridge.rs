//! Desktop control/facade boundary for separate-process audio/state containment.
//! Streaming never waits. Offline waits belong exclusively to render threads.
use super::runtime::{ParameterControls, binding_identity};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use windfall_engine::plugins::{HostedEffect, HostedInstrument, PluginTransport};
use windfall_plugin_host::bridge::{
    adapter::{Audio as ProcessAudio, ParameterSpec},
    protocol::{Config, DEFAULT_BLOCK, Kind},
    supervisor::{self, Control, Launch, Status},
};
use windfall_project::{PluginBinding, PluginParameter};

pub(super) type RenderError = Arc<Mutex<Option<String>>>;
type CapturedBinding = (Vec<u8>, Vec<(u32, f32)>);

#[derive(Default)]
struct Metadata {
    version: AtomicU64,
    epoch: AtomicU64,
    generation: AtomicU64,
}
impl Metadata {
    fn publish(&self, epoch: u64, generation: u64) -> bool {
        let version = self.version.load(Ordering::Relaxed);
        let Some(next) = version.checked_add(2) else {
            return false;
        };
        // Single callback writer. Keep all payload/version operations in the
        // same atomic order so equal even versions prove one complete tuple.
        self.version.store(version + 1, Ordering::SeqCst);
        self.epoch.store(epoch, Ordering::SeqCst);
        self.generation.store(generation, Ordering::SeqCst);
        self.version.store(next, Ordering::SeqCst);
        true
    }
    fn snapshot(&self) -> Result<(u64, u64, u64), String> {
        // Owner only. Bound even a continuously changing/misbehaving callback.
        for _ in 0..4096 {
            let version = self.version.load(Ordering::SeqCst);
            let epoch = self.epoch.load(Ordering::SeqCst);
            let generation = self.generation.load(Ordering::SeqCst);
            if version & 1 == 0 && self.version.load(Ordering::SeqCst) == version {
                return Ok((version, epoch, generation));
            }
        }
        Err("Plugin control intent changed during capture; save again".into())
    }
}

/// What the owner and the audio side of one bridged unit say to each other
/// about a replacement: the owner asks the unit to leave, the unit says when
/// its output has faded out, and counts its blocks so a stopped stream is
/// not waited for.
#[derive(Default)]
pub(super) struct Departure {
    requested: AtomicBool,
    departed: AtomicBool,
    /// The helper gave its plugin back for re-admission, so the unit cannot
    /// play on: the request stands until a replacement takes over or the
    /// owner withdraws it.
    held: AtomicBool,
    blocks: AtomicU64,
}
impl Departure {
    /// Asks the unit to fade out and waits, for as long as audio keeps
    /// running and at most `longest`, until it has. True when it did.
    /// A unit that already faded out for a standing request has nothing
    /// more to do.
    pub fn leave(&self, longest: Duration) -> bool {
        if self.requested.load(Ordering::Acquire) && self.departed.load(Ordering::Acquire) {
            return true;
        }
        self.departed.store(false, Ordering::Release);
        self.requested.store(true, Ordering::Release);
        let deadline = std::time::Instant::now() + longest;
        let mut seen = (
            self.blocks.load(Ordering::Acquire),
            std::time::Instant::now(),
        );
        loop {
            if self.departed.load(Ordering::Acquire) {
                return true;
            }
            let now = std::time::Instant::now();
            let blocks = self.blocks.load(Ordering::Acquire);
            if blocks != seen.0 {
                seen = (blocks, now);
            }
            // No block for a while: the stream is stopped and nothing sounds.
            if now >= deadline || now.duration_since(seen.1) > Duration::from_millis(40) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    /// Withdraws the request: the unit fades back in and plays on.
    pub fn stay(&self) {
        self.held.store(false, Ordering::Release);
        self.requested.store(false, Ordering::Release);
    }
}
pub(super) struct Record {
    pub departure: Arc<Departure>,
    pub editor_open: bool,
    pub editor_parent: Option<usize>,
    pub note_capabilities: Arc<super::note_capabilities::Published>,
    pub pending_capture: Option<(Vec<PluginParameter>, super::runtime::CapturedState)>,
    pub dirty_serial: u64,
    pub acknowledged_serial: u64,
    pub capture_sent: bool,
    pub binding: PluginBinding,
    pub revision: u64,
    pub playback: bool,
    pub control: Control,
    pub alive: Arc<AtomicBool>,
    controls: ParameterControls,
    metadata: Arc<Metadata>,
    specs: Vec<ParameterSpec>,
}
impl Record {
    pub fn status(&self) -> Status {
        self.control.status()
    }
    fn check_document(&self, desired: &[PluginParameter]) -> Result<(), String> {
        for control in self.controls.iter() {
            let (document, value) = control.document();
            if document > 2
                && desired
                    .iter()
                    .find(|param| param.id == control.id)
                    .is_none_or(|param| param.value != value)
            {
                return Err("Plugin parameters changed during state capture; save again".into());
            }
        }
        Ok(())
    }
    /// Reads what the plugin offers now. The helper takes its plugin out of
    /// processing for that and a replacement helper plays on, so first the
    /// output fades out and each sounding note's ramp is left where the
    /// replacement finds it (`Runtime::bridge_audio`).
    pub fn readmit(
        &self,
        desired: &[PluginParameter],
    ) -> Result<(Vec<u8>, super::capabilities::Readmission), String> {
        self.departure.held.store(true, Ordering::Release);
        self.departure.leave(Duration::from_millis(150));
        let result = self.readmit_now(desired);
        if result.is_err() {
            // The helper may still play: it fades back in.
            self.departure.stay();
        }
        result
    }
    fn readmit_now(
        &self,
        desired: &[PluginParameter],
    ) -> Result<(Vec<u8>, super::capabilities::Readmission), String> {
        self.check_document(desired)?;
        let snapshot = self.metadata.snapshot()?;
        let mut pending = self.specs.clone();
        for spec in &mut pending {
            if let Some(parameter) = desired.iter().find(|param| param.id == spec.id) {
                spec.value = f64::from(parameter.value);
            }
        }
        let (state, parameters, layout) =
            self.control
                .readmit(snapshot.1, &pending, Duration::from_secs(2))?;
        self.check_document(desired)?;
        if self.metadata.snapshot()? != snapshot {
            return Err("Plugin controls changed during capability re-admission".into());
        }
        Ok((state, super::capabilities::described(parameters, layout)))
    }
    pub fn capture(&self, desired: &[PluginParameter]) -> Result<CapturedBinding, String> {
        self.check_document(desired)?;
        let snapshot = self.metadata.snapshot()?;
        let mut specs = self.specs.clone();
        let mut intents = Vec::new();
        for (spec, control) in specs.iter_mut().zip(self.controls.iter()) {
            let (generation, value) = control.snapshot();
            // The complete table must reflect current intent even for settled
            // controls when unrelated pending notes advance desired generation.
            spec.value = f64::from(value);
            let (document, committed) = control.document();
            let wanted = desired
                .iter()
                .find(|param| param.id == control.id)
                .map_or(value, |param| param.value);
            let unobserved_document = control.observed_document.load(Ordering::Acquire) != document;
            let pending_document = control.applied_document.load(Ordering::Acquire) != document;
            // An inactive capture may settle document intent before callback
            // adoption. Its acknowledgement does not update the live control
            // snapshot; keep the newer document value on repeated captures.
            if unobserved_document {
                spec.value = f64::from(wanted);
            }
            if !control.settled(generation) || pending_document || wanted != committed {
                spec.value = f64::from(
                    if unobserved_document || pending_document || wanted != committed {
                        wanted
                    } else {
                        value
                    },
                );
                intents.push((spec.id, generation, document, spec.value as f32));
            }
        }
        let captured =
            self.control
                .capture(snapshot.1, snapshot.2, &specs, Duration::from_secs(2))?;
        self.check_document(desired)?;
        if self.metadata.snapshot()? != snapshot || captured.epoch != snapshot.1 {
            return Err(
                "Plugin timeline or control intent changed during capture; save again".into(),
            );
        }
        let mut parameters = captured
            .parameters
            .iter()
            .map(|param| (param.id, param.value as f32))
            .collect::<Vec<_>>();
        // Retained document intent is distinct from opaque native state/proof.
        // CLAP remains active; reopening applies these values after opaque state.
        let reconciled =
            self.binding.format == "vst3" && captured.reconciled_generation == snapshot.2;
        for (id, generation, document, value) in intents {
            // Inactive reconciliation includes any newer deactivation edit.
            // Its native readback must not be replaced by the earlier intent.
            if !reconciled && let Some(param) = parameters.iter_mut().find(|param| param.0 == id) {
                param.1 = value;
            }
            if reconciled
                && let Some(control) = self.controls.iter().find(|control| control.id == id)
                && control.snapshot().0 == generation
            {
                control.applied.store(generation, Ordering::Release);
                control
                    .applied_document
                    .fetch_max(document, Ordering::Release);
            }
        }
        Ok((captured.state, parameters))
    }
}

pub(super) fn helper_path(override_path: Option<&Path>) -> Result<PathBuf, String> {
    // The installed desktop executable owns helper_entry before Tauri startup.
    // Overrides exist only on explicit private test constructors.
    let path = override_path
        .map_or_else(std::env::current_exe, |path| Ok(path.to_owned()))
        .map_err(|error| error.to_string())?;
    if !path.is_file() {
        return Err("The installed audio helper executable is missing".into());
    }
    Ok(path)
}
pub(super) fn options(
    helper: PathBuf,
    binding: &PluginBinding,
    stamp: windfall_plugin_host::paths::PluginFileIdentity,
    token: u64,
    revision: u64,
    rate: u32,
    offline: bool,
) -> Result<Launch, String> {
    let identity = supervisor::fresh_identity(token, revision, binding_identity(binding))
        .map_err(|error| error.to_string())?;
    Ok(Launch {
        wrapper: super::wrapper::native(binding)?,
        helper,
        plugin: PathBuf::from(&binding.path),
        id: binding.id.clone(),
        format: binding.format.clone(),
        approved_binary: stamp,
        config: Config {
            identity,
            sample_rate: rate,
            // Keep one complete helper scheduling interval across the desktop's
            // 480-frame callback. A 256-frame interval can be submitted and
            // expire within the same callback, before the helper gets CPU.
            // Offline rendering explicitly waits at output boundaries and can
            // keep the shorter collection/pipeline delay.
            block: if offline {
                DEFAULT_BLOCK
            } else {
                DEFAULT_BLOCK * 2
            },
            native_latency: 0,
            kind: if binding.target.is_instrument() {
                Kind::Instrument
            } else {
                Kind::Effect
            },
        },
        parameters: binding
            .parameters
            .iter()
            .filter(|param| !param.read_only && binding.parameter_available(param.id))
            .map(|param| ParameterSpec {
                id: param.id,
                min: f64::from(param.min),
                max: f64::from(param.max),
                value: f64::from(param.value),
                read_only: param.read_only,
                stepped: param.stepped,
            })
            .collect(),
        state: binding.state.clone(),
        offline,
        startup_timeout: Duration::from_secs(5),
        audio_timeout: Duration::from_secs(2),
        cancelled: None,
    })
}

// Each equivalence marker refers to existing desired intent and a collection
// frontier. Adoption never increments that intent or publishes a native point.
struct Equivalence {
    generation: u64,
    document: u64,
    epoch: u64,
    sequence: u64,
    desired: u64,
}
#[derive(Clone, Copy)]
struct NoteOwner {
    engine: windfall_dsp::NoteInstanceId,
    note: windfall_plugin_host::bridge::slots::NoteSnapshot,
    release: f32,
    fine_pitch: f32,
    gain: f32,
    expression: windfall_dsp::NoteExpression,
}
impl NoteOwner {
    const EMPTY: Self = Self {
        engine: windfall_dsp::NoteInstanceId(0),
        note: windfall_plugin_host::bridge::slots::NoteSnapshot::EMPTY,
        release: 0.0,
        fine_pitch: 0.0,
        gain: 1.0,
        expression: windfall_dsp::NoteExpression::NEUTRAL,
    };
    fn mapped(
        self,
        map: windfall_dsp::ExpressionMap,
    ) -> windfall_plugin_host::PluginNoteExpression {
        let mut controls = map.plugin_controls(
            self.note.controls.pan,
            f64::from(self.note.controls.pitch) - f64::from(self.note.key),
            self.expression,
        );
        controls.values[0] *= f64::from(self.gain);
        if self.gain != 1.0 {
            controls.mask |= windfall_plugin_host::NoteExpressionKind::Volume.mask();
        }
        windfall_plugin_host::PluginNoteExpression {
            values: controls.values,
            mask: controls.mask,
        }
    }
}
pub(super) struct Audio {
    retired: bool,
    note_capabilities: Arc<super::note_capabilities::Published>,
    notes: Box<[NoteOwner]>,
    next_note: u32,
    expression_map: windfall_dsp::ExpressionMap,
    process: ProcessAudio,
    controls: ParameterControls,
    equivalence: Box<[Equivalence]>,
    metadata: Arc<Metadata>,
    alive: Arc<AtomicBool>,
    selection: Option<Arc<AtomicU64>>,
    token: u64,
    tail: usize,
    offline: bool,
    render_error: RenderError,
    last_frontier: (u64, u64),
    departure: Arc<Departure>,
    /// The sounding notes of this plugin target, shared with whichever unit
    /// replaces this one after the plugin changes what it can do.
    carry: Option<Arc<Mutex<windfall_plugin_host::project_expression::Carry>>>,
    /// No audio has run here yet, so a replayed note may take up the ramp
    /// the replaced unit left for it.
    fresh: bool,
    leaving: bool,
    /// Frames played since the owner asked this unit to leave.
    leaving_frames: usize,
}
pub(super) fn launch(
    options: Launch,
    binding: PluginBinding,
    controls: ParameterControls,
    selection: Option<Arc<AtomicU64>>,
    render_error: RenderError,
) -> Result<(Record, Audio), String> {
    let offline = options.offline;
    let specs = options.parameters.clone();
    let revision = options.config.identity.revision;
    let token = options.config.identity.token;
    let (control, process, tail) = supervisor::launch(options)?;
    let alive = Arc::new(AtomicBool::new(true));
    let metadata = Arc::new(Metadata::default());
    let (epoch, sequence, desired) = process.collection_frontier();
    if !metadata.publish(epoch, desired) {
        return Err("Bridge metadata counter exhausted".into());
    }
    let equivalence = controls
        .iter()
        .map(|param| {
            param.applied.store(0, Ordering::Release);
            param.applied_document.store(0, Ordering::Release);
            Equivalence {
                generation: param.snapshot().0,
                document: 0,
                epoch,
                sequence,
                desired,
            }
        })
        .collect();
    let note_capabilities = Arc::new(super::note_capabilities::Published::default());
    let caps = process.native_note_capabilities();
    note_capabilities.publish(caps.0, caps.1);
    let departure = Arc::new(Departure::default());
    let record = Record {
        departure: departure.clone(),
        editor_open: false,
        editor_parent: None,
        note_capabilities: note_capabilities.clone(),
        pending_capture: None,
        dirty_serial: 0,
        acknowledged_serial: 0,
        capture_sent: false,
        binding,
        revision,
        playback: !offline,
        control,
        alive: alive.clone(),
        controls: controls.clone(),
        metadata: metadata.clone(),
        specs,
    };
    let audio = Audio {
        retired: false,
        note_capabilities,
        notes: vec![NoteOwner::EMPTY; windfall_plugin_host::MAX_NOTE_INSTANCES].into_boxed_slice(),
        next_note: windfall_plugin_host::FIRST_NOTE_INSTANCE_ID,
        expression_map: windfall_dsp::ExpressionMap::default(),
        process,
        controls,
        equivalence,
        metadata,
        alive,
        selection,
        token,
        tail,
        offline,
        render_error,
        last_frontier: (epoch, desired),
        departure,
        carry: None,
        fresh: true,
        leaving: false,
        leaving_frames: 0,
    };
    Ok((record, audio))
}
impl Audio {
    fn publish(&mut self, force: bool) {
        let (epoch, sequence, desired) = self.process.collection_frontier();
        if epoch != self.last_frontier.0 {
            for (control, marker) in self.controls.iter().zip(self.equivalence.iter_mut()) {
                control.applied.store(0, Ordering::Release);
                control.applied_document.store(0, Ordering::Release);
                marker.epoch = epoch;
                marker.sequence = sequence;
                marker.desired = desired;
            }
        }
        if force || (epoch, desired) != self.last_frontier {
            if !self.metadata.publish(epoch, desired) {
                self.process.signals().failed.store(true, Ordering::Release);
            }
            self.last_frontier = (epoch, desired);
        }
    }
    fn acknowledge(&self) {
        if let Some((epoch, sequence, desired)) = self.process.completed_proof() {
            for (control, marker) in self.controls.iter().zip(self.equivalence.iter()) {
                if marker.epoch == epoch && marker.sequence <= sequence && marker.desired <= desired
                {
                    control.applied.store(marker.generation, Ordering::Release);
                    control
                        .applied_document
                        .fetch_max(marker.document, Ordering::Release);
                }
            }
        }
    }
}
impl Drop for Audio {
    fn drop(&mut self) {
        // Engine facade destruction is control-side. No supervisor/control ref
        // lives here; the desktop owner reaps after the final facade retires.
        self.alive.store(false, Ordering::Release);
        if let Some(selection) = &self.selection {
            let _ = selection.compare_exchange(self.token, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    }
}
impl HostedEffect for Audio {
    fn retire_from_document(&mut self) {
        self.retired = true;
        if let Some(selection) = &self.selection {
            let _ = selection.compare_exchange(self.token, 0, Ordering::AcqRel, Ordering::Acquire);
        }
    }
    fn copy_output(&self, bus: u32, output: &mut [[f32; 2]]) -> bool {
        self.process.copy_output(bus, output)
    }
    fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.process.set_sidechain_input(input);
    }
    fn adopt_parameters(&mut self, parameters: &[PluginParameter]) {
        let (epoch, sequence, desired) = self.process.collection_frontier();
        let mut changed = false;
        for (control, marker) in self.controls.iter().zip(self.equivalence.iter_mut()) {
            if let Some(param) = parameters.iter().find(|param| param.id == control.id) {
                let before = control.observed_document.load(Ordering::Acquire);
                control.observe_document(param.value);
                let observed = control.observed_document.load(Ordering::Acquire);
                changed |= before != observed;
                if marker.document != observed
                    && control.value.load(Ordering::Relaxed) == param.value.to_bits()
                {
                    marker.document = observed;
                    marker.epoch = epoch;
                    marker.sequence = sequence;
                    marker.desired = desired;
                    changed = true;
                }
            }
        }
        self.publish(changed);
    }
    fn transport(&mut self, transport: PluginTransport) {
        if !self.retired
            && let Some(selection) = &self.selection
        {
            selection.store(self.token, Ordering::Release);
        }
        // ABI3 retains the checked native meter anchor at the first frame.
        self.process.set_transport(windfall_plugin_host::Transport {
            playing: transport.playing,
            tempo_bpm: transport.tempo_bpm,
            position_beats: transport.position_beats,
            position_seconds: transport.position_seconds,
            numerator: transport.numerator,
            denominator: transport.denominator,
            meter_anchor: transport
                .meter_anchor
                .map(|anchor| windfall_plugin_host::MeterAnchor {
                    bar_origin_beats: anchor.bar_origin_beats,
                    bar_origin_index: anchor.bar_origin_index,
                }),
        });
        self.publish(false);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.process_sidechain(left, right, None);
    }
    fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        // The owner wants this plugin replaced. While notes sound the helper
        // fades its output out first and holds their ramps where they are.
        let wanted = self.departure.requested.load(Ordering::Acquire);
        let sounding = self.process.voices() > 0;
        self.process.request_departure(wanted && sounding);
        self.fresh = false;
        self.render(left, right, key);
        if wanted {
            self.leaving = true;
            self.store_carry();
            self.departure
                .departed
                .store(!sounding || self.process.departed(), Ordering::Release);
            // A replacement that never took over must not leave this unit
            // silent: after two seconds it plays on. One whose plugin was
            // given back for re-admission cannot, and waits for its
            // replacement however long the document takes to prepare it.
            self.leaving_frames = self.leaving_frames.saturating_add(left.len());
            if self.leaving_frames > 2 * self.process.sample_rate() as usize
                && !self.departure.held.load(Ordering::Acquire)
            {
                self.departure.stay();
            }
        } else if self.leaving {
            // It stays after all: nothing it left may be taken up later.
            self.leaving = false;
            self.leaving_frames = 0;
            if let Some(carry) = &self.carry
                && let Ok(mut carry) = carry.try_lock()
            {
                carry.clear();
            }
        }
        self.departure.blocks.fetch_add(1, Ordering::AcqRel);
    }
    fn set_param(&mut self, id: u32, value: f32) {
        if !value.is_finite() {
            return;
        }
        if let Some(index) = self.controls.iter().position(|control| control.id == id) {
            self.controls[index].publish(value);
            self.process.set_param(id, f64::from(value));
            let (epoch, sequence, desired) = self.process.collection_frontier();
            let control = &self.controls[index];
            self.equivalence[index] = Equivalence {
                generation: control.generation.load(Ordering::Acquire),
                document: control.observed_document.load(Ordering::Acquire),
                epoch,
                sequence,
                desired,
            };
            self.publish(true);
        }
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.process.set_tempo(f64::from(bpm));
    }
    fn latency(&self) -> usize {
        self.process.latency()
    }
    fn tail(&self) -> usize {
        self.tail
    }
}
impl Audio {
    /// Leaves the sounding notes where a replacement can find them: each
    /// ramp as the helper held it after the last output that was played,
    /// under the engine's identity for the note.
    fn store_carry(&mut self) {
        let (report, stage) = self.process.departure_report();
        let (Some(stage), Some(carry)) = (stage, &self.carry) else {
            return;
        };
        if let Ok(mut carry) = carry.try_lock() {
            carry.store(
                stage,
                report.iter().filter_map(|entry| {
                    self.notes
                        .iter()
                        .find(|owner| u64::from(owner.note.id) == entry.id && entry.id != 0)
                        .map(|owner| windfall_plugin_host::project_expression::Carried {
                            id: owner.engine.0,
                            ..*entry
                        })
                }),
            );
        }
    }
    /// One carry per plugin target, set on a control thread before audio.
    pub(super) fn share_carry(
        &mut self,
        carry: Arc<Mutex<windfall_plugin_host::project_expression::Carry>>,
    ) {
        self.carry = Some(carry);
    }
    fn render(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        if self.offline {
            if let Err(error) = self.process.process_offline_sidechain(
                left,
                right,
                key,
                std::time::Instant::now() + Duration::from_secs(2),
                &AtomicBool::new(false),
            ) {
                *self
                    .render_error
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(windfall_ipc::ui_message(
                    "plugins.isolatedRenderFailed",
                    serde_json::json!({"error": format!("{:?}", error)}),
                ));
                // The owner maintenance pass reaps this helper off realtime.
                self.process.signals().failed.store(true, Ordering::Release);
            }
        } else {
            self.process.process_sidechain(left, right, key);
        }
        let caps = self.process.native_note_capabilities();
        self.note_capabilities.publish(caps.0, caps.1);
        self.publish(false);
        self.acknowledge();
    }
    fn owns(&self, id: windfall_dsp::NoteInstanceId) -> bool {
        self.notes
            .iter()
            .any(|owner| owner.engine == id && owner.note.id != 0)
    }
    fn end_note(&mut self, id: windfall_dsp::NoteInstanceId) {
        if let Some(slot) = self
            .notes
            .iter()
            .position(|n| n.engine == id && n.note.id != 0)
        {
            let note = self.notes[slot];
            self.process.note_off_instance(note.note.id, note.release);
            self.notes[slot] = NoteOwner::EMPTY;
            self.publish(false);
        }
    }
}
impl HostedInstrument for Audio {
    fn set_expression_map(&mut self, map: windfall_dsp::ExpressionMap) {
        if self.expression_map == map {
            return;
        }
        self.expression_map = map;
        for owner in self.notes.iter_mut().filter(|owner| owner.note.id != 0) {
            let controls = owner.mapped(map);
            owner.note.expression = Some(controls);
            self.process.set_note_expression(owner.note.id, controls);
        }
        self.publish(false);
    }
    fn set_note_pan_law(&mut self, law: windfall_core::PanLaw) {
        self.process.set_project_pan_law(law);
        self.publish(false);
    }
    fn set_note_gain(&mut self, id: windfall_dsp::NoteInstanceId, gain: f32) {
        if !gain.is_finite() {
            return;
        }
        let Some(owner) = self
            .notes
            .iter_mut()
            .find(|owner| owner.engine == id && owner.note.id != 0)
        else {
            return;
        };
        owner.gain = gain.clamp(0.0, 4.0);
        let controls = owner.mapped(self.expression_map);
        owner.note.expression = Some(controls);
        self.process.set_note_expression(owner.note.id, controls);
        self.publish(false);
    }
    fn midi_message(&mut self, bytes: &[u8]) -> bool {
        if let Some(status) = bytes.first() {
            self.note_capabilities.observe_channel(status & 15);
        }
        let accepted = self.process.midi_message(bytes);
        self.publish(false);
        accepted
    }
    fn supports_note_instances(&self) -> bool {
        self.process.supports_note_instances()
    }
    fn supports_note_channels(&self) -> bool {
        true
    }
    fn supports_note_pitch(&self) -> bool {
        self.process.supports_note_pitch()
    }
    fn note_on_instance(
        &mut self,
        id: windfall_dsp::NoteInstanceId,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: windfall_dsp::NoteExpression,
    ) {
        if expression.color_group.is_some_and(|channel| channel > 15) {
            self.note_capabilities.refuse_channel();
            return;
        }
        self.note_capabilities
            .observe_channel(expression.color_group.unwrap_or(0));
        let expression = expression.clamped();
        if id.0 == 0 || !velocity.is_finite() {
            // A note nobody can name again is played by key and channel.
            if !self.process.supports_note_instances() && velocity.is_finite() {
                self.process.note_on_channel(
                    key.min(127),
                    expression.color_group.unwrap_or(0),
                    velocity,
                );
                self.publish(false);
            }
            return;
        }
        self.end_note(id);
        if velocity <= 0.0 {
            return;
        }
        let Some(slot) = self.notes.iter().position(|n| n.note.id == 0) else {
            return;
        };
        // IDs remain unique among live owners, including after the bounded wrap.
        let native = loop {
            let native = self.next_note;
            self.next_note = if native == i32::MAX as u32 {
                windfall_plugin_host::FIRST_NOTE_INSTANCE_ID
            } else {
                native + 1
            };
            if self.notes.iter().all(|n| n.note.id != native) {
                break native;
            }
        };
        let key = key.min(127);
        let mut note = windfall_plugin_host::bridge::slots::NoteSnapshot {
            expression: None,
            id: native,
            key,
            channel: expression.color_group.unwrap_or(0),
            velocity: velocity.clamp(0.0, 1.0),
            controls: windfall_plugin_host::PluginNoteControls {
                volume: 1.0,
                pan: if pan.is_finite() {
                    pan.clamp(-1.0, 1.0)
                } else {
                    0.0
                },
                pitch: f32::from(key) + expression.fine_pitch_cents / 100.0,
                modulation_x: expression.modulation_x,
                modulation_y: expression.modulation_y,
            },
        };
        // A refused timed admission still owns desired intent: the next full
        // snapshot reconstructs it after the helper's uncertain-block reset.
        self.notes[slot] = NoteOwner {
            engine: id,
            note,
            release: expression.release,
            fine_pitch: expression.fine_pitch_cents,
            gain: 1.0,
            expression,
        };
        note.expression = Some(self.notes[slot].mapped(self.expression_map));
        self.notes[slot].note = note;
        // The same note in the unit that replaced its plugin: its ramp goes
        // on in the new helper from where the old one stopped.
        let carried = if self.fresh {
            self.carry
                .as_ref()
                .and_then(|carry| carry.try_lock().ok())
                .and_then(|carry| {
                    carry
                        .find(id.0, note.key, note.channel)
                        .map(|found| (found, carry.stage()))
                })
        } else {
            None
        };
        match carried {
            Some((found, stage)) => {
                self.process
                    .note_on_instance_carried(note, found.serial, found.ramp, stage)
            }
            None => self.process.note_on_instance(note),
        };
        self.publish(false);
    }
    fn note_off_instance(&mut self, id: windfall_dsp::NoteInstanceId, key: u8) {
        if self.owns(id) {
            self.end_note(id);
        } else if !self.process.supports_note_instances() {
            self.note_off(key);
        }
    }
    fn note_off_on_channel(&mut self, id: windfall_dsp::NoteInstanceId, key: u8, channel: u8) {
        if self.owns(id) {
            self.end_note(id);
        } else if !self.process.supports_note_instances() {
            self.process.note_off_channel(key, channel);
            self.publish(false);
        }
    }
    fn set_note_expression(
        &mut self,
        id: windfall_dsp::NoteInstanceId,
        pan: f32,
        expression: windfall_dsp::NoteExpression,
    ) {
        if expression.color_group.is_some_and(|channel| channel > 15) {
            self.note_capabilities.refuse_channel();
            return;
        }
        let Some(slot) = self
            .notes
            .iter()
            .position(|n| n.engine == id && n.note.id != 0)
        else {
            return;
        };
        let expression = expression.clamped();
        let owner = &mut self.notes[slot];
        owner.note.controls.pitch = (owner.note.controls.pitch
            + (expression.fine_pitch_cents - owner.fine_pitch) / 100.0)
            .clamp(
                f32::from(owner.note.key) - 120.0,
                f32::from(owner.note.key) + 120.0,
            );
        owner.fine_pitch = expression.fine_pitch_cents;
        owner.release = expression.release;
        owner.expression = expression;
        owner.note.controls.pan = if pan.is_finite() {
            pan.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        owner.note.controls.modulation_x = expression.modulation_x;
        owner.note.controls.modulation_y = expression.modulation_y;
        let controls = owner.mapped(self.expression_map);
        owner.note.expression = Some(controls);
        self.process.set_note_expression(owner.note.id, controls);
        self.publish(false);
    }
    fn set_note_pitch(&mut self, id: windfall_dsp::NoteInstanceId, pitch: f32) {
        if !pitch.is_finite() {
            return;
        }
        let Some(slot) = self
            .notes
            .iter()
            .position(|n| n.engine == id && n.note.id != 0)
        else {
            return;
        };
        let owner = &mut self.notes[slot];
        let key = f32::from(owner.note.key);
        owner.note.controls.pitch = pitch.clamp(key - 120.0, key + 120.0);
        let controls = owner.mapped(self.expression_map);
        owner.note.expression = Some(controls);
        self.process.set_note_expression(owner.note.id, controls);
        self.publish(false);
    }
    fn note_on(&mut self, key: u8, velocity: f32) {
        self.process.note_on(key, velocity);
        self.publish(false);
    }
    fn note_off(&mut self, key: u8) {
        self.process.note_off(key);
        self.publish(false);
    }
    fn all_notes_off(&mut self) {
        self.process.all_notes_off();
        self.notes.fill(NoteOwner::EMPTY);
        self.publish(false);
    }
    fn voices(&self) -> usize {
        self.process.voices()
    }
}

#[cfg(all(test, windows))]
#[path = "bridge_route_tests.rs"]
mod route_tests;

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windfall_engine::plugins::PluginFactory;
    use windfall_plugin_host::bridge::{
        adapter::Signals,
        protocol::*,
        slots::{InputBlock, LocalWords, OutputBlock, Region},
    };

    #[test]
    fn bridged_and_in_process_project_laws_and_live_expression_ramps_match_pcm() {
        use windfall_core::PanLaw;
        use windfall_dsp::{
            ExpressionMap, NoteExpression, NoteInstanceId, PluginExpressionDestination,
        };
        for vst3 in [false, true] {
            for offline in [false, true] {
                for law in [
                    PanLaw::Balance,
                    PanLaw::ConstantPower,
                    PanLaw::Compromise,
                    PanLaw::Linear,
                ] {
                    let (runtime, binding) = if vst3 {
                        super::super::runtime::ownership_tests::fixture(26)
                    } else {
                        super::super::runtime::ownership_tests::clap_expression_fixture(false)
                    };
                    let mut direct = runtime.instrument(&binding, 48_000, 64).unwrap();
                    let helper = PathBuf::from(
                        std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER")
                            .expect("explicit prebuilt current helper"),
                    );
                    let stamp =
                        windfall_plugin_host::paths::plugin_file_identity(Path::new(&binding.path))
                            .unwrap();
                    let launch_options =
                        options(helper, &binding, stamp, 8181, 1, 48_000, offline).unwrap();
                    let controls = super::super::runtime::parameter_controls(&binding);
                    let error = Arc::new(Mutex::new(None));
                    let (record, mut bridge) =
                        launch(launch_options, binding, controls, None, error.clone()).unwrap();
                    let delay = HostedEffect::latency(&bridge) - direct.latency();
                    let note = NoteExpression {
                        color_group: Some(3),
                        ..NoteExpression::NEUTRAL
                    };
                    let mut map = ExpressionMap::default();
                    map.modulation_x.plugin = PluginExpressionDestination::Volume;
                    for unit in [&mut *direct, &mut bridge as &mut dyn HostedInstrument] {
                        unit.set_expression_map(map);
                        unit.set_note_pan_law(law);
                        unit.note_on_instance(NoteInstanceId(1), 69, 0.4, 0.65, note);
                        unit.note_on_instance(
                            NoteInstanceId(2),
                            76,
                            0.2,
                            -0.4,
                            NoteExpression {
                                color_group: Some(5),
                                ..note
                            },
                        );
                    }
                    let mut native_pcm = Vec::new();
                    let mut bridge_pcm = Vec::new();
                    let total = 4096 + delay;
                    for frame in (0..total).step_by(64) {
                        if frame == 1024 {
                            for unit in [&mut *direct, &mut bridge as &mut dyn HostedInstrument] {
                                unit.set_note_pan_law(PanLaw::Linear);
                            }
                        }
                        if frame == 1536 {
                            for unit in [&mut *direct, &mut bridge as &mut dyn HostedInstrument] {
                                unit.set_note_expression(
                                    NoteInstanceId(1),
                                    -0.3,
                                    NoteExpression {
                                        modulation_x: 0.4,
                                        ..note
                                    },
                                );
                                unit.set_note_gain(NoteInstanceId(2), 0.6);
                            }
                        }
                        if frame == 2048 {
                            map.modulation_x.plugin = PluginExpressionDestination::Brightness;
                            for unit in [&mut *direct, &mut bridge as &mut dyn HostedInstrument] {
                                unit.set_expression_map(map);
                            }
                        }
                        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
                        assert_eq!(
                            crate::test_alloc::allocator_calls(
                                || direct.process(&mut left, &mut right)
                            ),
                            0
                        );
                        native_pcm.extend(
                            left.into_iter()
                                .zip(right)
                                .map(|(left, right)| [left, right]),
                        );
                        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
                        if offline {
                            HostedEffect::process(&mut bridge, &mut left, &mut right);
                        } else {
                            assert_eq!(
                                crate::test_alloc::allocator_calls(|| HostedEffect::process(
                                    &mut bridge,
                                    &mut left,
                                    &mut right
                                )),
                                0
                            );
                            // This is the test's device clock, not an audio callback wait.
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        bridge_pcm.extend(
                            left.into_iter()
                                .zip(right)
                                .map(|(left, right)| [left, right]),
                        );
                        assert!(error.lock().unwrap().is_none());
                        assert!(!record.control.status().failed);
                    }
                    assert!(native_pcm.iter().any(|frame| frame[0].abs() > 0.001));
                    for frame in 0..4096 {
                        for side in 0..2 {
                            assert!(
                                (bridge_pcm[frame + delay][side] - native_pcm[frame][side]).abs()
                                    < 2e-6,
                                "{vst3} {offline} {law:?} frame={frame} side={side}"
                            );
                        }
                    }
                    assert_eq!(
                        record.note_capabilities.snapshot().observed_channels,
                        vec![3, 5]
                    );
                    assert!(record.note_capabilities.snapshot().project_pan_law);
                }
            }
        }
    }

    #[test]
    fn bridged_and_in_process_agree_on_all_four_pan_and_volume_routes() {
        // The helper process and the in-process owner share one rule: a
        // property the plugin offers per note is applied by the plugin, one
        // it does not offer by the channel after it. Fixture class 26 offers
        // pan and volume, 23 pan only, 27 volume only, 2 neither. Each plays
        // the same law changes, per-note moves and a newer and an ended note
        // through both, and the two outputs must be the same samples.
        use windfall_core::PanLaw;
        use windfall_dsp::{NoteExpression, NoteInstanceId};
        use windfall_plugin_host::project_expression::Routes;
        let mut worst = 0.0_f32;
        for (class, pan, volume) in [
            (26, true, true),
            (23, true, false),
            (27, false, true),
            (2, false, false),
        ] {
            for offline in [false, true] {
                for law in [PanLaw::ConstantPower, PanLaw::Linear] {
                    let (runtime, binding) = super::super::runtime::ownership_tests::fixture(class);
                    let mut direct = runtime.instrument(&binding, 48_000, 64).unwrap();
                    let helper = PathBuf::from(
                        std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER")
                            .expect("explicit prebuilt current helper"),
                    );
                    let stamp =
                        windfall_plugin_host::paths::plugin_file_identity(Path::new(&binding.path))
                            .unwrap();
                    let launch_options =
                        options(helper, &binding, stamp, 8181, 1, 48_000, offline).unwrap();
                    let controls = super::super::runtime::parameter_controls(&binding);
                    let error = Arc::new(Mutex::new(None));
                    let (record, mut bridge) =
                        launch(launch_options, binding, controls, None, error.clone()).unwrap();
                    assert_eq!(
                        record.note_capabilities.snapshot().routes(3),
                        Routes { pan, volume },
                        "class {class}"
                    );
                    let delay = HostedEffect::latency(&bridge) - direct.latency();
                    let on = |channel| NoteExpression {
                        color_group: Some(channel),
                        ..NoteExpression::NEUTRAL
                    };
                    for unit in [&mut *direct, &mut bridge as &mut dyn HostedInstrument] {
                        unit.set_note_pan_law(law);
                        unit.note_on_instance(NoteInstanceId(1), 69, 0.4, 0.65, on(3));
                        unit.note_on_instance(NoteInstanceId(2), 76, 0.2, -0.4, on(5));
                    }
                    let mut native_pcm = Vec::new();
                    let mut bridge_pcm = Vec::new();
                    let total = 3584 + delay;
                    for frame in (0..total).step_by(64) {
                        for unit in [&mut *direct, &mut bridge as &mut dyn HostedInstrument] {
                            match frame {
                                1024 => unit.set_note_pan_law(PanLaw::Balance),
                                1536 => {
                                    unit.set_note_expression(NoteInstanceId(1), -0.3, on(3));
                                    unit.set_note_gain(NoteInstanceId(2), 0.6);
                                }
                                // A newer note leads the channel's share...
                                2048 => {
                                    unit.note_on_instance(NoteInstanceId(3), 81, 0.3, 0.9, on(7))
                                }
                                // ...and hands it back when it ends.
                                2560 => unit.note_off_on_channel(NoteInstanceId(3), 81, 7),
                                _ => {}
                            }
                        }
                        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
                        assert_eq!(
                            crate::test_alloc::allocator_calls(
                                || direct.process(&mut left, &mut right)
                            ),
                            0
                        );
                        native_pcm.extend(
                            left.into_iter()
                                .zip(right)
                                .map(|(left, right)| [left, right]),
                        );
                        let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
                        if offline {
                            HostedEffect::process(&mut bridge, &mut left, &mut right);
                        } else {
                            assert_eq!(
                                crate::test_alloc::allocator_calls(|| HostedEffect::process(
                                    &mut bridge,
                                    &mut left,
                                    &mut right
                                )),
                                0
                            );
                            // This is the test's device clock, not an audio callback wait.
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        bridge_pcm.extend(
                            left.into_iter()
                                .zip(right)
                                .map(|(left, right)| [left, right]),
                        );
                        assert!(error.lock().unwrap().is_none());
                        assert!(!record.control.status().failed);
                    }
                    assert!(native_pcm.iter().any(|frame| frame[0].abs() > 0.001));
                    for frame in 0..3584 {
                        for side in 0..2 {
                            let apart =
                                (bridge_pcm[frame + delay][side] - native_pcm[frame][side]).abs();
                            worst = worst.max(apart);
                            assert!(
                                apart < 2e-6,
                                "class {class} offline={offline} {law:?} frame={frame} side={side}: {} vs {}",
                                bridge_pcm[frame + delay][side],
                                native_pcm[frame][side]
                            );
                        }
                    }
                }
            }
        }
        println!("ROUTE bridged against in-process, four combinations: worst {worst:.3e}");
    }

    fn native_selected_sidechain_reaches_desktop_facade(vst3: bool) {
        for offline in [false, true] {
            let folder = tempfile::tempdir().unwrap();
            let path = folder.path().join(if vst3 {
                "detector.vst3"
            } else {
                "detector.clap"
            });
            std::fs::copy(std::env::var_os("WINDFALL_BRIDGE_FIXTURE").unwrap(), &path).unwrap();
            let binding = PluginBinding {
                capabilities: Default::default(),
                target: windfall_project::PluginTarget::Effect {
                    effect: windfall_project::EffectId(9123),
                },
                format: if vst3 { "vst3" } else { "clap" }.into(),
                id: if vst3 {
                    "00000000000000000000000000000016"
                } else {
                    "org.windfall.test.sidechain"
                }
                .into(),
                path: path.to_string_lossy().into_owned(),
                name: "Native detector".into(),
                state: Vec::new(),
                parameters: Vec::new(),
                sidechain_input: Some(1),
                wrapper: Default::default(),
                auxiliary_inputs: vec![windfall_project::PluginAuxInput {
                    index: 1,
                    name: "Detector".into(),
                    channels: 1,
                }],
            };
            let controls = super::super::runtime::parameter_controls(&binding);
            let options = options(
                PathBuf::from(std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER").unwrap()),
                &binding,
                windfall_plugin_host::paths::plugin_file_identity(&path).unwrap(),
                9123,
                1,
                48_000,
                offline,
            )
            .unwrap();
            let render_error = Arc::new(Mutex::new(None));
            let (record, mut audio) =
                launch(options, binding, controls, None, render_error.clone()).unwrap();
            let key = [[0.25, 0.75]; DEFAULT_BLOCK];
            // Mono detector folds the supplied stereo key to 0.5. A missing
            // explicitly selected native bus must remain silent; None chooses
            // the first auxiliary port. Plain process must clear prior key PCM.
            for (selected, with_key, expected) in [
                (Some(1), true, [0.6, 0.7]),
                (Some(2), true, [0.1, 0.2]),
                (None, true, [0.6, 0.7]),
                (Some(1), false, [0.1, 0.2]),
            ] {
                assert_eq!(
                    crate::test_alloc::allocator_calls(|| HostedEffect::set_sidechain_input(
                        &mut audio, selected
                    )),
                    0
                );
                let deadline = std::time::Instant::now() + Duration::from_secs(3);
                let mut calls = 0;
                loop {
                    let mut left = [0.1; DEFAULT_BLOCK];
                    let mut right = [0.2; DEFAULT_BLOCK];
                    let mut process = || {
                        if with_key {
                            HostedEffect::process_sidechain(
                                &mut audio,
                                &mut left,
                                &mut right,
                                Some(&key),
                            );
                        } else {
                            HostedEffect::process(&mut audio, &mut left, &mut right);
                        }
                    };
                    if offline {
                        process();
                    } else {
                        assert_eq!(crate::test_alloc::allocator_calls(process), 0);
                    }
                    calls += 1;
                    assert!(render_error.lock().unwrap().is_none());
                    assert!(!record.control.status().failed);
                    if calls >= 8
                        && left.iter().all(|value| (*value - expected[0]).abs() < 1e-6)
                        && right
                            .iter()
                            .all(|value| (*value - expected[1]).abs() < 1e-6)
                    {
                        assert!(audio.process.health().completed_blocks > 0);
                        break;
                    }
                    assert!(
                        std::time::Instant::now() < deadline,
                        "vst3={vst3}, offline={offline}, selected={selected:?}, key={with_key}, PCM={:?}/{:?}",
                        &left[..4],
                        &right[..4]
                    );
                    if !offline {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                }
            }
            drop(audio);
            assert!(record.control.terminate().reaped);
        }
    }

    #[test]
    fn native_clap_selected_sidechain_reaches_desktop_facade_live_and_offline() {
        native_selected_sidechain_reaches_desktop_facade(false);
    }
    #[test]
    fn native_vst3_selected_sidechain_reaches_desktop_facade_live_and_offline() {
        native_selected_sidechain_reaches_desktop_facade(true);
    }
    #[test]
    fn adoption_records_only_equivalence_and_waits_for_a_later_complete_proof() {
        let parameter = PluginParameter {
            id: 7,
            name: "Gain".into(),
            min: 0.0,
            max: 2.0,
            value: 0.5,
            stepped: false,
            read_only: false,
            automatable: true,
        };
        let binding = PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            capabilities: Default::default(),
            wrapper: Default::default(),
            target: windfall_project::PluginTarget::Effect {
                effect: windfall_project::EffectId(1),
            },
            format: "clap".into(),
            id: "fixture".into(),
            path: "fixture.clap".into(),
            name: "Gain".into(),
            state: Vec::new(),
            parameters: vec![parameter.clone()],
        };
        let controls = super::super::runtime::parameter_controls(&binding);
        controls[0].applied.store(0, Ordering::Release);
        controls[0].applied_document.store(0, Ordering::Release);
        let region = Region::initialize(
            LocalWords::new(),
            Config {
                identity: Identity {
                    session: 1,
                    token: 1,
                    revision: 1,
                    binding: 1,
                },
                sample_rate: 48_000,
                block: 64,
                native_latency: 0,
                kind: Kind::Effect,
            },
        )
        .unwrap();
        let process = ProcessAudio::new(
            region.clone(),
            Arc::new(Signals::default()),
            &[ParameterSpec {
                id: 7,
                min: 0.0,
                max: 2.0,
                value: 0.5,
                read_only: false,
                stepped: false,
            }],
        )
        .unwrap();
        let mut audio = Audio {
            retired: false,
            note_capabilities: Arc::new(super::super::note_capabilities::Published::default()),
            notes: vec![NoteOwner::EMPTY; windfall_plugin_host::MAX_NOTE_INSTANCES]
                .into_boxed_slice(),
            next_note: windfall_plugin_host::FIRST_NOTE_INSTANCE_ID,
            expression_map: windfall_dsp::ExpressionMap::default(),
            process,
            controls: controls.clone(),
            equivalence: vec![Equivalence {
                generation: 2,
                document: 0,
                epoch: 1,
                sequence: 0,
                desired: 1,
            }]
            .into_boxed_slice(),
            metadata: Arc::new(Metadata::default()),
            alive: Arc::new(AtomicBool::new(true)),
            selection: None,
            token: 1,
            tail: 0,
            departure: Arc::default(),
            carry: None,
            fresh: true,
            leaving: false,
            leaving_frames: 0,
            offline: false,
            render_error: Arc::new(Mutex::new(None)),
            last_frontier: (1, 1),
        };
        audio.adopt_parameters(std::slice::from_ref(&parameter));
        audio.set_param(7, 0.625);
        let mut input = InputBlock::new();
        let mut output = OutputBlock::silent();
        let finish = |input: &mut InputBlock, output: &mut OutputBlock| {
            while let Some((slot, sequence)) = region.take_input(input).unwrap() {
                output.epoch = input.epoch;
                output.processed_generation = input.control_end;
                region.complete(slot, sequence, output, OUTPUT_OK);
            }
        };
        for _ in 0..3 {
            audio.process(&mut [0.0; 64], &mut [0.0; 64]);
            finish(&mut input, &mut output);
        }
        assert!(controls[0].settled(controls[0].snapshot().0));
        controls[0].commit(0.625);
        let mut adopted = parameter;
        adopted.value = 0.625;
        let desired = audio.process.desired_generation();
        let frontier = audio.process.collection_frontier();
        assert_eq!(
            crate::test_alloc::allocator_calls(
                || audio.adopt_parameters(std::slice::from_ref(&adopted))
            ),
            0
        );
        assert_eq!(
            audio.process.desired_generation(),
            desired,
            "adoption cannot publish native desired intent"
        );
        assert_eq!(audio.process.collection_frontier(), frontier);
        assert_ne!(
            controls[0].applied_document.load(Ordering::Acquire),
            controls[0].document().0
        );
        for number in 0..3 {
            assert_eq!(
                crate::test_alloc::allocator_calls(|| audio.process(&mut [0.0; 64], &mut [0.0; 64])),
                0
            );
            if number < 2 {
                assert_ne!(
                    controls[0].applied_document.load(Ordering::Acquire),
                    controls[0].document().0
                );
            }
            finish(&mut input, &mut output);
            assert_eq!(
                input.event_count, 0,
                "adoption must not force an unchanged native point"
            );
        }
        assert_eq!(
            controls[0].applied_document.load(Ordering::Acquire),
            controls[0].document().0
        );
        controls[0].commit(0.75);
        adopted.value = 0.75;
        let desired = audio.process.desired_generation();
        audio.adopt_parameters(std::slice::from_ref(&adopted));
        assert_eq!(audio.process.desired_generation(), desired);
        for _ in 0..3 {
            audio.process(&mut [0.0; 64], &mut [0.0; 64]);
            finish(&mut input, &mut output);
        }
        assert_ne!(
            controls[0].applied_document.load(Ordering::Acquire),
            controls[0].document().0,
            "adoption may not equate a different committed value with old desired intent"
        );
        audio.process.reset_timeline();
        audio.publish(false);
        assert_eq!(audio.process.completed_proof(), None);
        assert_eq!(controls[0].applied_document.load(Ordering::Acquire), 0);
    }
    #[test]
    fn metadata_rollover_refuses_publication_without_wrapping() {
        let metadata = Metadata::default();
        metadata.version.store(u64::MAX - 1, Ordering::Release);
        assert!(!metadata.publish(1, 1));
        assert_eq!(metadata.version.load(Ordering::Acquire), u64::MAX - 1);
    }

    fn native_settled_gain_with_pending_note(vst3: bool) {
        let folder = tempfile::tempdir().unwrap();
        let path = folder
            .path()
            .join(if vst3 { "proof.vst3" } else { "proof.clap" });
        std::fs::copy(std::env::var_os("WINDFALL_BRIDGE_FIXTURE").unwrap(), &path).unwrap();
        let parameter = PluginParameter {
            id: if vst3 { 7 } else { 1 },
            name: "Gain".into(),
            min: 0.0,
            max: 1.0,
            value: 0.5,
            stepped: false,
            read_only: false,
            automatable: true,
        };
        let binding = PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            capabilities: Default::default(),
            wrapper: Default::default(),
            target: windfall_project::PluginTarget::Instrument {
                channel: windfall_project::ChannelId(9001),
            },
            format: if vst3 { "vst3" } else { "clap" }.into(),
            id: if vst3 {
                "00000000000000000000000000000003"
            } else {
                "org.windfall.test.sine"
            }
            .into(),
            path: path.to_string_lossy().into_owned(),
            name: "Native settled gain regression".into(),
            state: Vec::new(),
            parameters: vec![parameter.clone()],
        };
        let controls = super::super::runtime::parameter_controls(&binding);
        let options = options(
            PathBuf::from(std::env::var_os("WINDFALL_DESKTOP_BRIDGE_HELPER").unwrap()),
            &binding,
            windfall_plugin_host::paths::plugin_file_identity(&path).unwrap(),
            9001,
            1,
            48_000,
            false,
        )
        .unwrap();
        let (record, mut audio) = launch(
            options,
            binding,
            controls.clone(),
            None,
            Arc::new(Mutex::new(None)),
        )
        .unwrap();
        let mut committed = parameter;
        committed.value = 0.625;
        controls[0].commit(committed.value);
        let before_adoption = audio.process.collection_frontier();
        assert_eq!(
            crate::test_alloc::allocator_calls(
                || audio.adopt_parameters(std::slice::from_ref(&committed))
            ),
            0
        );
        assert_eq!(audio.process.collection_frontier(), before_adoption);
        assert_eq!(audio.process.completed_proof(), None);
        audio.set_param(committed.id, committed.value);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            std::thread::sleep(Duration::from_millis(8));
            assert_eq!(
                crate::test_alloc::allocator_calls(
                    || audio.process(&mut [0.0; DEFAULT_BLOCK], &mut [0.0; DEFAULT_BLOCK])
                ),
                0
            );
            if controls[0].settled(controls[0].snapshot().0)
                && controls[0].applied_document.load(Ordering::Acquire) == controls[0].document().0
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "native gain never completed"
            );
        }
        let proof = audio.process.completed_proof().unwrap();
        let settled = controls[0].snapshot();
        assert_eq!(settled.1, 0.625);
        assert_eq!(proof.2, audio.process.desired_generation());
        assert_eq!(
            crate::test_alloc::allocator_calls(|| audio.note_on(60, 0.75)),
            0
        );
        let desired = audio.process.desired_generation();
        assert_eq!(desired, proof.2 + 1, "only the actual note advances intent");
        assert_eq!(audio.process.completed_proof(), Some(proof));
        assert_eq!(controls[0].snapshot(), settled);
        let (state, parameters) = record.capture(std::slice::from_ref(&committed)).unwrap();
        assert_eq!(
            parameters
                .iter()
                .find(|param| param.0 == committed.id)
                .unwrap()
                .1,
            0.625
        );
        assert_eq!(audio.process.desired_generation(), desired);
        assert_eq!(audio.process.completed_proof(), Some(proof));
        assert_eq!(controls[0].snapshot(), settled);
        let captured = record.control.last_valid_state().unwrap();
        assert_eq!(captured.processed_generation, proof.2);
        assert!(captured.processed_generation < desired);
        // A capture no longer deactivates a VST3 plugin, so it reconciles
        // nothing into it: the pending note stays unacknowledged for both.
        let _ = vst3;
        assert_eq!(captured.reconciled_generation, 0);
        assert!(!state.is_empty());
        drop(audio);
        assert!(record.control.terminate().reaped);
    }

    #[test]
    fn native_clap_capture_keeps_settled_gain_without_acknowledging_pending_note() {
        native_settled_gain_with_pending_note(false);
    }
    #[test]
    fn native_vst3_capture_keeps_settled_gain_without_acknowledging_pending_note() {
        native_settled_gain_with_pending_note(true);
    }
}
