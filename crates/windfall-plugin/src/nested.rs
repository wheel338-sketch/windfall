//! Each prepared guest owns independent helpers. No DAW executable is spawned,
//! no global desktop owner is borrowed, and speculative guests never share DSP.
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use windfall_engine::plugins::{HostedEffect, HostedInstrument, PluginFactory, PluginTransport};
use windfall_plugin_host::{
    Transport,
    bridge::{
        adapter::{Audio, ParameterSpec},
        protocol::{Config, Kind},
        supervisor::{self, Control, Launch},
    },
};
use windfall_project::{PluginBinding, PluginProcessingMode, PluginTarget};
static HELPERS: AtomicU64 = AtomicU64::new(0);
#[unsafe(no_mangle)]
pub extern "C" fn windfall_runtime_helpers() -> u64 {
    HELPERS.load(Ordering::Acquire)
}

pub(crate) fn validate(binding: &PluginBinding) -> Result<(), String> {
    binding.validate().map_err(str::to_owned)?;
    if !matches!(binding.format.as_str(), "clap" | "vst3") {
        return Err(format!(
            "Nested provider '{}' is unavailable: exported Windfall supports CLAP/VST3 process helpers, not Audio Units",
            binding.format
        ));
    }
    if matches!(
        binding.wrapper.processing,
        PluginProcessingMode::InProcess | PluginProcessingMode::Threaded
    ) {
        return Err(format!(
            "Nested {} '{}' requests an in-process/threaded owner; select Automatic or Bridged before export",
            binding.format, binding.name
        ));
    }
    if binding.id == "org.windfall.program"
        || binding
            .id
            .eq_ignore_ascii_case("57494E4446414C4C50524F4752414D01")
    {
        return Err(
            "Recursive Windfall exports are refused: helper nesting would be unbounded".into(),
        );
    }
    Ok(())
}
struct Record {
    target: PluginTarget,
    control: Control,
    frontier: Arc<Frontier>,
    specs: Vec<ParameterSpec>,
    values: Arc<[AtomicU64]>,
}
#[derive(Default)]
struct Frontier {
    version: AtomicU64,
    epoch: AtomicU64,
    generation: AtomicU64,
}
impl Frontier {
    fn store(&self, audio: &Audio) {
        let (epoch, _, generation) = audio.collection_frontier();
        self.version.fetch_add(1, Ordering::SeqCst);
        self.epoch.store(epoch, Ordering::SeqCst);
        self.generation.store(generation, Ordering::SeqCst);
        self.version.fetch_add(1, Ordering::SeqCst);
    }
    fn read(&self) -> Result<(u64, u64, u64), String> {
        for _ in 0..4096 {
            let version = self.version.load(Ordering::SeqCst);
            let epoch = self.epoch.load(Ordering::SeqCst);
            let generation = self.generation.load(Ordering::SeqCst);
            if version & 1 == 0 && self.version.load(Ordering::SeqCst) == version {
                return Ok((version, epoch, generation));
            }
        }
        Err("Nested audio changed during capture; save again".into())
    }
}
pub(crate) struct Provider {
    helper: PathBuf,
    records: Mutex<Vec<Record>>,
    error: Mutex<Option<String>>,
    next: AtomicU64,
    session: u64,
}
impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExportOwnedProvider")
            .field("helper", &self.helper)
            .finish_non_exhaustive()
    }
}
impl Provider {
    pub fn new() -> Result<Arc<Self>, String> {
        let runtime = crate::package::module_path()
            .ok_or("Cannot locate export-owned native helper directory")?;
        let parent = runtime.parent().ok_or("Export runtime has no directory")?;
        let helper = parent.join(windfall_plugin_host::export_runtime::helper_filename());
        Self::at(helper)
    }
    fn at(helper: PathBuf) -> Result<Arc<Self>, String> {
        if !helper.is_file() {
            return Err(format!(
                "Nested CLAP/VST3 audio helper is missing: {}",
                helper.display()
            ));
        }
        let session =
            windfall_plugin_host::bridge::auth::session_nonce().map_err(|e| e.to_string())?;
        Ok(Arc::new(Self {
            helper,
            records: Mutex::new(Vec::new()),
            error: Mutex::new(None),
            next: AtomicU64::new(1),
            session,
        }))
    }
    pub fn error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn make(&self, binding: &PluginBinding, rate: u32) -> Result<Unit, String> {
        let result = self.launch(binding, rate);
        if let Err(error) = &result {
            *self.error.lock().unwrap_or_else(|e| e.into_inner()) = Some(error.clone());
        }
        result
    }
    fn launch(&self, binding: &PluginBinding, rate: u32) -> Result<Unit, String> {
        validate(binding)?;
        let plugin = Path::new(&binding.path);
        let approved_binary =
            windfall_plugin_host::paths::plugin_file_identity(plugin).map_err(|e| {
                format!(
                    "Nested {} '{}' cannot load {}: {e}",
                    binding.format,
                    binding.name,
                    plugin.display()
                )
            })?;
        let mut token = self.next.load(Ordering::Acquire);
        loop {
            let next = token
                .checked_add(1)
                .ok_or("Export-owned helper tokens exhausted")?;
            match self
                .next
                .compare_exchange_weak(token, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => break,
                Err(current) => token = current,
            }
        }
        let identity = windfall_plugin_host::bridge::protocol::Identity {
            session: self.session,
            token,
            revision: 0,
            binding: fingerprint(binding),
        };
        let options = Launch {
            wrapper: windfall_plugin_host::wrapper::WrapperOptions {
                smart_disable: binding.wrapper.smart_disable,
                fixed_block: binding.wrapper.fixed_buffer.map(|n| n as usize),
                ..Default::default()
            },
            helper: self.helper.clone(),
            plugin: plugin.to_owned(),
            id: binding.id.clone(),
            format: binding.format.clone(),
            approved_binary,
            config: Config {
                identity,
                sample_rate: rate,
                block: 512,
                native_latency: 0,
                kind: if matches!(binding.target, PluginTarget::Instrument { .. }) {
                    Kind::Instrument
                } else {
                    Kind::Effect
                },
            },
            parameters: binding
                .parameters
                .iter()
                .filter(|p| !p.read_only)
                .map(|p| ParameterSpec {
                    id: p.id,
                    min: f64::from(p.min),
                    max: f64::from(p.max),
                    value: f64::from(p.value),
                    read_only: p.read_only,
                    stepped: p.stepped,
                })
                .collect(),
            state: binding.state.clone(),
            offline: false,
            startup_timeout: Duration::from_secs(5),
            audio_timeout: Duration::from_secs(2),
            cancelled: None,
        };
        let (control, mut audio, tail) = supervisor::launch(options)
            .map_err(|e| format!("Nested {} '{}' refused: {e}", binding.format, binding.name))?;
        HELPERS.fetch_add(1, Ordering::AcqRel);
        audio.set_sidechain_input(binding.sidechain_input);
        let frontier = Arc::new(Frontier::default());
        frontier.store(&audio);
        let specs = audio.parameters().to_vec();
        let values: Arc<[AtomicU64]> = specs
            .iter()
            .map(|p| AtomicU64::new(p.value.to_bits()))
            .collect::<Vec<_>>()
            .into();
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Record {
                target: binding.target,
                control,
                frontier: frontier.clone(),
                specs,
                values: values.clone(),
            });
        Ok(Unit {
            audio,
            tail,
            frontier,
            values,
            notes: vec![NoteOwner::EMPTY; windfall_plugin_host::MAX_NOTE_INSTANCES]
                .into_boxed_slice(),
        })
    }
    pub fn capture(&self, program: &crate::Program) -> Result<crate::Program, String> {
        let mut captured = program.clone();
        for record in self
            .records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
        {
            let before = record.frontier.read()?;
            let mut pending = record.specs.clone();
            for (spec, value) in pending.iter_mut().zip(record.values.iter()) {
                spec.value = f64::from_bits(value.load(Ordering::SeqCst));
            }
            let state =
                record
                    .control
                    .capture(before.1, before.2, &pending, Duration::from_secs(2))?;
            if before != record.frontier.read()? {
                return Err("Nested plugin changed during state capture; save again".into());
            }
            if let Some(binding) = captured
                .project
                .plugins
                .iter_mut()
                .find(|b| b.target == record.target)
            {
                binding.state = state.state;
                for parameter in state.parameters {
                    if let Some(p) = binding.parameters.iter_mut().find(|p| p.id == parameter.id) {
                        p.value = parameter.value as f32;
                    }
                }
            }
        }
        Ok(captured)
    }
}
impl Drop for Provider {
    fn drop(&mut self) {
        // Guest destruction and publication retirement run on control. Audio
        // facades own mappings/signals only, never a Control or child process.
        for record in self
            .records
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            let status = record.control.terminate();
            if status.reaped {
                HELPERS.fetch_sub(1, Ordering::AcqRel);
            } else {
                crate::runtime::error("Nested helper teardown did not confirm process exit");
            }
        }
    }
}
fn fingerprint(binding: &PluginBinding) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    binding.path.hash(&mut h);
    binding.id.hash(&mut h);
    binding.format.hash(&mut h);
    binding.state.hash(&mut h);
    binding.wrapper.hash(&mut h);
    for p in &binding.parameters {
        p.id.hash(&mut h);
        p.value.to_bits().hash(&mut h);
    }
    h.finish()
}
impl PluginFactory for Provider {
    fn effect(
        &self,
        binding: &PluginBinding,
        rate: u32,
        _: usize,
    ) -> Result<Box<dyn HostedEffect>, String> {
        Ok(Box::new(self.make(binding, rate)?))
    }
    fn instrument(
        &self,
        binding: &PluginBinding,
        rate: u32,
        _: usize,
    ) -> Result<Box<dyn HostedInstrument>, String> {
        Ok(Box::new(self.make(binding, rate)?))
    }
}
#[derive(Clone, Copy)]
struct NoteOwner {
    engine: windfall_dsp::NoteInstanceId,
    native: windfall_plugin_host::bridge::slots::NoteSnapshot,
    release: f32,
    fine: f32,
}
impl NoteOwner {
    const EMPTY: Self = Self {
        engine: windfall_dsp::NoteInstanceId(0),
        native: windfall_plugin_host::bridge::slots::NoteSnapshot::EMPTY,
        release: 0.0,
        fine: 0.0,
    };
}
struct Unit {
    audio: Audio,
    tail: usize,
    frontier: Arc<Frontier>,
    values: Arc<[AtomicU64]>,
    notes: Box<[NoteOwner]>,
}
impl Unit {
    fn end(&mut self, id: windfall_dsp::NoteInstanceId) {
        if let Some(slot) = self
            .notes
            .iter()
            .position(|n| n.engine == id && n.native.id != 0)
        {
            let note = self.notes[slot];
            self.notes[slot] = NoteOwner::EMPTY;
            self.audio.note_off_instance(note.native.id, note.release);
            self.frontier.store(&self.audio);
        }
    }
}
impl HostedEffect for Unit {
    fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.audio.set_sidechain_input(input);
    }
    fn transport(&mut self, t: PluginTransport) {
        self.audio.set_transport(Transport {
            playing: t.playing,
            tempo_bpm: t.tempo_bpm,
            position_beats: t.position_beats,
            position_seconds: t.position_seconds,
            numerator: t.numerator,
            denominator: t.denominator,
            meter_anchor: t.meter_anchor.map(|a| windfall_plugin_host::MeterAnchor {
                bar_origin_beats: a.bar_origin_beats,
                bar_origin_index: a.bar_origin_index,
            }),
        });
        self.frontier.store(&self.audio);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.audio.process(left, right);
        self.frontier.store(&self.audio);
    }
    fn process_sidechain(&mut self, left: &mut [f32], right: &mut [f32], key: Option<&[[f32; 2]]>) {
        self.audio.process_sidechain(left, right, key);
        self.frontier.store(&self.audio);
    }
    fn set_param(&mut self, id: u32, value: f32) {
        self.frontier.version.fetch_add(1, Ordering::SeqCst);
        self.audio.set_param(id, f64::from(value));
        for (spec, value) in self.audio.parameters().iter().zip(self.values.iter()) {
            value.store(spec.value.to_bits(), Ordering::SeqCst);
        }
        let (epoch, _, generation) = self.audio.collection_frontier();
        self.frontier.epoch.store(epoch, Ordering::SeqCst);
        self.frontier.generation.store(generation, Ordering::SeqCst);
        self.frontier.version.fetch_add(1, Ordering::SeqCst);
    }
    fn set_tempo(&mut self, bpm: f32) {
        self.audio.set_tempo(f64::from(bpm));
    }
    fn latency(&self) -> usize {
        self.audio.latency()
    }
    fn tail(&self) -> usize {
        self.tail
    }
    fn reset_hosted(&mut self) {
        self.audio.reset_hard();
        self.notes.fill(NoteOwner::EMPTY);
        self.frontier.store(&self.audio);
    }
}
impl HostedInstrument for Unit {
    fn midi_message(&mut self, bytes: &[u8]) -> bool {
        let result = self.audio.midi_message(bytes);
        self.frontier.store(&self.audio);
        result
    }
    fn note_on(&mut self, key: u8, velocity: f32) {
        self.audio.note_on(key, velocity);
        self.frontier.store(&self.audio);
    }
    fn note_off(&mut self, key: u8) {
        self.audio.note_off(key);
        self.frontier.store(&self.audio);
    }
    fn all_notes_off(&mut self) {
        self.audio.all_notes_off();
        self.notes.fill(NoteOwner::EMPTY);
        self.frontier.store(&self.audio);
    }
    fn voices(&self) -> usize {
        self.audio.voices()
    }
    fn supports_note_channels(&self) -> bool {
        true
    }
    fn supports_note_instances(&self) -> bool {
        self.audio.supports_note_instances()
    }
    fn supports_note_pitch(&self) -> bool {
        self.audio.supports_note_pitch()
    }
    fn note_on_instance(
        &mut self,
        id: windfall_dsp::NoteInstanceId,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: windfall_dsp::NoteExpression,
    ) {
        let expression = expression.clamped();
        if !self.audio.supports_note_instances() {
            self.audio
                .note_on_channel(key, expression.color_group.unwrap_or(0), velocity);
            self.frontier.store(&self.audio);
            return;
        }
        if id.0 == 0 || !velocity.is_finite() {
            return;
        }
        self.end(id);
        if velocity <= 0.0 {
            return;
        }
        let Some(slot) = self.notes.iter().position(|n| n.native.id == 0) else {
            return;
        };
        let key = key.min(127);
        let note = windfall_plugin_host::bridge::slots::NoteSnapshot {
            expression: None,
            id: windfall_plugin_host::FIRST_NOTE_INSTANCE_ID + slot as u32,
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
        self.notes[slot] = NoteOwner {
            engine: id,
            native: note,
            release: expression.release,
            fine: expression.fine_pitch_cents,
        };
        self.audio.note_on_instance(note);
        self.frontier.store(&self.audio);
    }
    fn note_off_instance(&mut self, id: windfall_dsp::NoteInstanceId, key: u8) {
        if self.audio.supports_note_instances() {
            self.end(id);
        } else {
            self.note_off(key);
        }
    }
    fn note_off_on_channel(&mut self, id: windfall_dsp::NoteInstanceId, key: u8, channel: u8) {
        if self.audio.supports_note_instances() {
            self.end(id);
        } else {
            self.audio.note_off_channel(key, channel);
            self.frontier.store(&self.audio);
        }
    }
    fn set_note_expression(
        &mut self,
        id: windfall_dsp::NoteInstanceId,
        pan: f32,
        expression: windfall_dsp::NoteExpression,
    ) {
        let Some(slot) = self
            .notes
            .iter()
            .position(|n| n.engine == id && n.native.id != 0)
        else {
            return;
        };
        let expression = expression.clamped();
        let owner = &mut self.notes[slot];
        let key = f32::from(owner.native.key);
        owner.native.controls.pitch = (owner.native.controls.pitch
            + (expression.fine_pitch_cents - owner.fine) / 100.0)
            .clamp(key - 120.0, key + 120.0);
        owner.fine = expression.fine_pitch_cents;
        owner.release = expression.release;
        owner.native.controls.pan = if pan.is_finite() {
            pan.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        owner.native.controls.modulation_x = expression.modulation_x;
        owner.native.controls.modulation_y = expression.modulation_y;
        self.audio
            .set_note_controls(owner.native.id, owner.native.controls);
        self.frontier.store(&self.audio);
    }
    fn set_note_pitch(&mut self, id: windfall_dsp::NoteInstanceId, pitch: f32) {
        if !pitch.is_finite() {
            return;
        }
        let Some(slot) = self
            .notes
            .iter()
            .position(|n| n.engine == id && n.native.id != 0)
        else {
            return;
        };
        let owner = &mut self.notes[slot];
        let key = f32::from(owner.native.key);
        owner.native.controls.pitch = pitch.clamp(key - 120.0, key + 120.0);
        self.audio
            .set_note_controls(owner.native.id, owner.native.controls);
        self.frontier.store(&self.audio);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding() -> PluginBinding {
        PluginBinding {
            capabilities: Default::default(),
            target: PluginTarget::Instrument {
                channel: windfall_project::ChannelId(1),
            },
            format: "clap".into(),
            path: "owned.clap".into(),
            id: "owned".into(),
            name: "Owned".into(),
            state: vec![],
            parameters: vec![],
            wrapper: Default::default(),
            sidechain_input: None,
            auxiliary_inputs: vec![],
        }
    }
    #[test]
    fn nested_provider_refusals_identify_format_policy_recursion_and_missing_helper() {
        let mut b = binding();
        for format in ["clap", "vst3"] {
            b.format = format.into();
            assert!(validate(&b).is_ok());
        }
        b.format = "au".into();
        assert!(validate(&b).unwrap_err().contains("Audio Units"));
        b.format = "clap".into();
        b.wrapper.processing = PluginProcessingMode::InProcess;
        assert!(
            validate(&b)
                .unwrap_err()
                .contains("select Automatic or Bridged")
        );
        b.wrapper.processing = PluginProcessingMode::Threaded;
        assert!(validate(&b).is_err());
        b.wrapper.processing = PluginProcessingMode::Bridged;
        b.id = "org.windfall.program".into();
        assert!(validate(&b).unwrap_err().contains("unbounded"));
        assert!(
            Provider::at(PathBuf::from("absent-export-owned-helper"))
                .unwrap_err()
                .contains("helper is missing")
        );
    }
    #[test]
    fn nested_note_ownership_controls_process_and_hard_reset_have_zero_callback_memory_calls() {
        use windfall_plugin_host::bridge::{
            adapter::Signals,
            protocol::Identity,
            slots::{LocalWords, Region},
        };
        let region = Region::initialize(
            LocalWords::new(),
            Config {
                identity: Identity {
                    session: 1,
                    token: 2,
                    revision: 3,
                    binding: 4,
                },
                sample_rate: 48_000,
                block: 64,
                native_latency: 0,
                kind: Kind::Instrument,
            },
        )
        .unwrap();
        let mut audio = Audio::new(region.clone(), Arc::new(Signals::default()), &[]).unwrap();
        audio.configure_note_capabilities(true, [0x7f; 16]).unwrap();
        let frontier = Arc::new(Frontier::default());
        frontier.store(&audio);
        let mut unit = Unit {
            audio,
            tail: 0,
            frontier,
            values: Vec::<AtomicU64>::new().into(),
            notes: vec![NoteOwner::EMPTY; windfall_plugin_host::MAX_NOTE_INSTANCES]
                .into_boxed_slice(),
        };
        let a = windfall_dsp::NoteInstanceId(u64::MAX);
        let b = windfall_dsp::NoteInstanceId(u64::MAX - 1);
        let (_, calls) = crate::clap::tests::realtime(|| {
            unit.note_on_instance(a, 69, 1.0, 0.0, Default::default());
            unit.note_on_instance(b, 69, 0.5, 0.0, Default::default());
            assert_eq!(unit.voices(), 2);
            assert_ne!(unit.notes[0].native.id, unit.notes[1].native.id);
            unit.set_note_pitch(a, 70.0);
            unit.set_note_expression(b, -0.5, Default::default());
            unit.note_off_instance(a, 69);
            assert_eq!(unit.voices(), 1);
            unit.process(&mut [0.0; 64], &mut [0.0; 64]);
            unit.reset_hosted();
            assert_eq!(unit.voices(), 0);
            assert!(unit.notes.iter().all(|n| n.native.id == 0));
            assert_eq!(region.timeline_epoch(), Ok(2));
            let (mut left, mut right) = ([0.0; 64], [0.0; 64]);
            unit.process(&mut left, &mut right);
            assert!(left.iter().chain(&right).all(|v| *v == 0.0));
        });
        assert_eq!(calls, 0);
    }
}
