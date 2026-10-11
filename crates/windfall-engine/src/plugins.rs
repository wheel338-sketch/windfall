//! Control-side factory and prepared audio-only halves of hosted plugins.

use std::collections::HashMap;
use windfall_project::{PluginBinding, PluginParameter, PluginTarget};

/// A song meter segment's first downbeat and zero-based bar index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeterAnchor {
    pub bar_origin_beats: f64,
    pub bar_origin_index: u32,
}

/// Musical position at the first frame of a processing block.
#[derive(Debug, Clone, Copy)]
pub struct PluginTransport {
    pub playing: bool,
    pub tempo_bpm: f64,
    pub position_beats: f64,
    pub position_seconds: f64,
    pub numerator: u16,
    pub denominator: u16,
    /// `None` retains the scalar signature's origin at beat zero.
    pub meter_anchor: Option<MeterAnchor>,
}

/// Prepared effects. Setup and destruction belong to the factory's owner thread.
pub trait HostedEffect: Send {
    fn set_sidechain_input(&mut self, _input: Option<u32>) {}
    /// Services an ownership exchange without processing sound. Called at
    /// callback/block boundaries for stopped, bypassed and retiring slots too.
    /// Must not allocate, lock, wait, deactivate or destroy native objects.
    fn control_boundary(&mut self) {}
    /// Observes document adoption even when cached values need no new control.
    /// This is callback-local metadata only: no allocation, wait or native call.
    /// Implementations must match the currently committed document values and
    /// acknowledge processing separately, after actual native completion.
    fn adopt_parameters(&mut self, _parameters: &[PluginParameter]) {}
    fn transport(&mut self, _transport: PluginTransport) {}
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    fn process_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        _key: Option<&[[f32; 2]]>,
    ) {
        self.process(left, right);
    }
    fn set_param(&mut self, id: u32, value: f32);
    fn set_tempo(&mut self, bpm: f32);
    fn latency(&self) -> usize;
    fn tail(&self) -> usize;
}

/// Prepared instruments, fed on the exact frame of each note boundary.
pub trait HostedInstrument: HostedEffect {
    fn note_on(&mut self, key: u8, velocity: f32);
    fn note_off(&mut self, key: u8);
    fn all_notes_off(&mut self);
    fn voices(&self) -> usize;
    /// Providers opt in only when their event transport preserves ownership.
    fn supports_note_instances(&self) -> bool {
        false
    }
    fn supports_note_channels(&self) -> bool {
        false
    }
    fn note_on_instance(
        &mut self,
        id: windfall_dsp::NoteInstanceId,
        key: u8,
        velocity: f32,
        pan: f32,
        expression: windfall_dsp::NoteExpression,
    ) {
        let _ = (id, pan, expression);
        self.note_on(key, velocity);
    }
    fn note_off_instance(&mut self, id: windfall_dsp::NoteInstanceId, key: u8) {
        let _ = id;
        self.note_off(key);
    }
    fn note_off_on_channel(&mut self, id: windfall_dsp::NoteInstanceId, key: u8, channel: u8) {
        let _ = channel;
        self.note_off_instance(id, key);
    }
    fn set_note_expression(
        &mut self,
        id: windfall_dsp::NoteInstanceId,
        pan: f32,
        expression: windfall_dsp::NoteExpression,
    ) {
        let _ = (id, pan, expression);
    }
    fn set_note_pitch(&mut self, id: windfall_dsp::NoteInstanceId, pitch: f32) {
        let _ = (id, pitch);
    }
    fn supports_note_pitch(&self) -> bool {
        false
    }
    /// What the stored controls of this channel's notes drive in the plugin.
    /// The engine passes a note's controls as they are stored and leaves the
    /// translation into plugin expressions to the provider. Called when the
    /// unit is installed and, on the audio thread, when a plan changes the
    /// map: no allocation, lock, wait or native call.
    fn set_expression_map(&mut self, map: windfall_dsp::ExpressionMap) {
        let _ = map;
    }
}

/// Creates independent instances for playback and offline render, off the callback.
pub trait PluginFactory: Send + Sync + std::fmt::Debug {
    /// Stable provider identity; clones sharing one native owner should override
    /// this with the identity of that shared owner. Plans retain their factory.
    /// Publication reads this metadata under its short admission guard: it must
    /// not perform IO, allocate, lock, wait, service native code or read a clock.
    fn provider_identity(&self) -> u64 {
        std::ptr::from_ref(self) as *const () as usize as u64
    }
    /// An independent provider whose instances cannot become playback/editor owners.
    fn render_factory(&self) -> Option<std::sync::Arc<dyn PluginFactory>> {
        None
    }
    /// Control/offline caller only; never poll this from a live callback.
    fn render_error(&self) -> Option<String> {
        None
    }
    /// Changes when an explicit retry or plugin restart requires fresh instances.
    /// Like provider_identity, this is a bounded metadata read. Staged factories
    /// report their preparation revision, separately from installed revision.
    fn revision(&self) -> u64 {
        0
    }
    fn effect(
        &self,
        binding: &PluginBinding,
        sample_rate: u32,
        max_block: usize,
    ) -> Result<Box<dyn HostedEffect>, String>;
    fn instrument(
        &self,
        binding: &PluginBinding,
        sample_rate: u32,
        max_block: usize,
    ) -> Result<Box<dyn HostedInstrument>, String>;
}

pub(crate) enum PreparedPlugin {
    Effect(Option<Box<dyn HostedEffect>>),
    Instrument(Option<Box<dyn HostedInstrument>>),
}

impl PreparedPlugin {
    pub fn latency(&self) -> usize {
        match self {
            Self::Effect(unit) => unit.as_ref().map_or(0, |unit| unit.latency()),
            Self::Instrument(unit) => unit.as_ref().map_or(0, |unit| unit.latency()),
        }
    }
}

/// Identity excludes parameter values, so edits keep sounding instances alive.
pub(crate) fn identity(binding: &PluginBinding) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    binding.path.hash(&mut hash);
    binding.id.hash(&mut hash);
    binding.format.hash(&mut hash);
    binding.state.hash(&mut hash);
    for parameter in &binding.parameters {
        parameter.id.hash(&mut hash);
    }
    hash.finish()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativePreparationError {
    pub target: PluginTarget,
    pub reason: String,
}

type PreparedPlugins = (
    HashMap<PluginTarget, PreparedPlugin>,
    HashMap<PluginTarget, (u64, usize)>,
);

pub(crate) fn prepare(
    plan: &crate::plan::Plan,
    sample_rate: u32,
    known: &HashMap<PluginTarget, (u64, usize)>,
) -> Result<PreparedPlugins, NativePreparationError> {
    let mut units = HashMap::new();
    let mut records = HashMap::new();
    for binding in &plan.plugins {
        let identity = identity(binding) ^ plan.factory_stamp.map_or(0, |stamp| stamp.identity());
        if let Some(&(before, latency)) = known.get(&binding.target)
            && before == identity
        {
            records.insert(binding.target, (identity, latency));
            continue;
        }
        let unit = match binding.target {
            PluginTarget::Effect { .. } => PreparedPlugin::Effect(
                plan.plugin_factory
                    .as_ref()
                    .map(|factory| factory.effect(binding, sample_rate, crate::mixer::MAX_BLOCK))
                    .transpose()
                    .map_err(|reason| NativePreparationError {
                        target: binding.target,
                        reason,
                    })?,
            ),
            PluginTarget::Instrument { .. } => PreparedPlugin::Instrument(
                plan.plugin_factory
                    .as_ref()
                    .map(|factory| {
                        factory.instrument(binding, sample_rate, crate::mixer::MAX_BLOCK)
                    })
                    .transpose()
                    .map_err(|reason| NativePreparationError {
                        target: binding.target,
                        reason,
                    })?,
            ),
        };
        // The native unit must report its actual negotiated latency. Refuse an
        // unrepresentable/unbounded dry-history allocation before constructing
        // its engine facade; never clamp latency or call it Unavailable.
        let latency = unit.latency();
        if crate::rack::Compensation::reserved_payload(latency, 0)
            .is_none_or(|bytes| bytes > 256 * 1024 * 1024)
        {
            return Err(NativePreparationError {
                target: binding.target,
                reason: format!("Native latency {latency} exceeds prepared dry-history capacity."),
            });
        }
        if plan.plugin_factory.is_some() {
            records.insert(binding.target, (identity, latency));
        }
        units.insert(binding.target, unit);
    }
    Ok((units, records))
}

/// A plugin slot with a latency-matched dry path and smoothed wet mix.
pub(crate) struct ExternalEffect {
    pub unit: Option<Box<dyn HostedEffect>>,
    pub params: Box<[(u32, f32)]>,
    dry: crate::rack::Compensation,
    scratch: Box<[[f32; 2]]>,
    wet: f32,
    target: f32,
    step: f32,
    remaining: usize,
    latency: usize,
    sample_rate: u32,
}

impl ExternalEffect {
    pub fn new(
        unit: Option<Box<dyn HostedEffect>>,
        binding: &PluginBinding,
        rate: u32,
        enabled: bool,
        mix: f32,
    ) -> Self {
        let latency = unit.as_ref().map_or(0, |unit| unit.latency());
        let wet = if enabled && unit.is_some() { mix } else { 0.0 };
        let mut slot = Self {
            unit,
            params: binding
                .parameters
                .iter()
                .map(|p| (p.id, f32::NAN))
                .collect(),
            dry: crate::rack::Compensation::new(latency, latency),
            scratch: vec![[0.0; 2]; crate::mixer::MAX_BLOCK].into_boxed_slice(),
            wet,
            target: wet,
            step: 0.0,
            remaining: 0,
            latency,
            sample_rate: rate,
        };
        slot.apply(binding);
        slot
    }
    pub fn apply(&mut self, binding: &PluginBinding) {
        if let Some(unit) = &mut self.unit {
            unit.set_sidechain_input(binding.sidechain_input);
            unit.adopt_parameters(&binding.parameters);
        }
        for (cached, param) in self.params.iter_mut().zip(&binding.parameters) {
            if cached.0 == param.id && cached.1 != param.value {
                if let Some(unit) = &mut self.unit {
                    unit.set_param(param.id, param.value);
                }
                cached.1 = param.value;
            }
        }
    }
    pub fn automate(&mut self, index: usize, value: f32) {
        if let Some(param) = self.params.get_mut(index)
            && param.1 != value
        {
            if let Some(unit) = &mut self.unit {
                unit.set_param(param.0, value);
            }
            param.1 = value;
        }
    }
    pub fn set_mix(&mut self, enabled: bool, mix: f32) {
        let target = if enabled && self.unit.is_some() {
            mix
        } else {
            0.0
        };
        if target == self.target {
            return;
        }
        self.target = target;
        self.remaining = (self.sample_rate as usize / 200).max(1);
        self.step = (target - self.wet) / self.remaining as f32;
    }
    pub fn process_sidechain(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        key: Option<&[[f32; 2]]>,
    ) {
        let dry = &mut self.scratch[..left.len()];
        for ((frame, left), right) in dry.iter_mut().zip(left.iter()).zip(right.iter()) {
            *frame = [*left, *right];
        }
        self.dry.process(dry);
        if let Some(unit) = &mut self.unit {
            unit.process_sidechain(left, right, key);
        }
        for index in 0..left.len() {
            if self.remaining > 0 {
                self.wet += self.step;
                self.remaining -= 1;
                if self.remaining == 0 {
                    self.wet = self.target;
                }
            }
            left[index] = dry[index][0] + (left[index] - dry[index][0]) * self.wet;
            right[index] = dry[index][1] + (right[index] - dry[index][1]) * self.wet;
        }
    }
    pub fn latency(&self) -> usize {
        self.latency
    }
    pub fn tail(&self) -> usize {
        self.unit.as_ref().map_or(0, |unit| unit.tail())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use windfall_project::*;

    #[derive(Debug, Default)]
    struct FixtureFactory {
        made: AtomicUsize,
        boundaries: Arc<AtomicUsize>,
        latency: usize,
    }
    struct Fixture {
        boundaries: Arc<AtomicUsize>,
        level: f32,
        held: bool,
        instrument: bool,
        delay: Box<[[f32; 2]]>,
        cursor: usize,
    }
    impl HostedEffect for Fixture {
        fn control_boundary(&mut self) {
            self.boundaries.fetch_add(1, Ordering::Relaxed);
        }
        fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
            for (left, right) in left.iter_mut().zip(right) {
                if self.instrument {
                    *left = if self.held { self.level } else { 0.0 };
                    *right = *left;
                } else {
                    *left *= self.level;
                    *right *= self.level;
                }
                if !self.delay.is_empty() {
                    let previous = self.delay[self.cursor];
                    self.delay[self.cursor] = [*left, *right];
                    self.cursor = (self.cursor + 1) % self.delay.len();
                    *left = previous[0];
                    *right = previous[1];
                }
            }
        }
        fn set_param(&mut self, id: u32, value: f32) {
            assert_eq!(id, 43);
            self.level = value;
        }
        fn set_tempo(&mut self, _: f32) {}
        fn latency(&self) -> usize {
            self.delay.len()
        }
        fn tail(&self) -> usize {
            0
        }
    }
    impl HostedInstrument for Fixture {
        fn note_on(&mut self, _: u8, _: f32) {
            self.held = true;
        }
        fn note_off(&mut self, _: u8) {
            self.held = false;
        }
        fn all_notes_off(&mut self) {
            self.held = false;
        }
        fn voices(&self) -> usize {
            usize::from(self.held)
        }
    }
    impl PluginFactory for FixtureFactory {
        fn effect(
            &self,
            _: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedEffect>, String> {
            self.made.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(Fixture {
                boundaries: self.boundaries.clone(),
                level: 1.0,
                held: false,
                instrument: false,
                delay: vec![[0.0; 2]; self.latency].into_boxed_slice(),
                cursor: 0,
            }))
        }
        fn instrument(
            &self,
            _: &PluginBinding,
            _: u32,
            _: usize,
        ) -> Result<Box<dyn HostedInstrument>, String> {
            self.made.fetch_add(1, Ordering::Relaxed);
            Ok(Box::new(Fixture {
                boundaries: self.boundaries.clone(),
                level: 1.0,
                held: false,
                instrument: true,
                delay: Box::new([]),
                cursor: 0,
            }))
        }
    }
    fn binding() -> PluginBinding {
        PluginBinding {
            sidechain_input: None,
            auxiliary_inputs: Vec::new(),
            target: PluginTarget::Instrument {
                channel: ChannelId(0),
            },
            format: "clap".into(),
            path: "fixture.clap".into(),
            id: "fixture".into(),
            name: "Fixture".into(),
            state: vec![],
            parameters: vec![PluginParameter {
                id: 43,
                name: "Level".into(),
                min: 0.0,
                max: 2.0,
                value: 0.5,
                stepped: false,
                read_only: false,
                automatable: true,
            }],
        }
    }
    fn project() -> Project {
        let mut doc = Document::new(Project::new("Fixture"));
        doc.dispatch(Command::AddPluginInstrument { plugin: binding() }, None)
            .unwrap();
        let channel = doc.project().channels[0].id;
        let mut project = doc.project().clone();
        let id = NoteId(project.next_id);
        project.next_id += 1;
        project.patterns[0].lanes.push(Lane {
            channel,
            notes: vec![Note {
                id,
                start: 0,
                length: 96,
                key: 60,
                velocity: 1.0,
                pan: 0.0,
                expression: Default::default(),
            }],
        });
        project.check().unwrap();
        project
    }
    #[test]
    fn hardware_hosted_fixture_notes_and_panic_do_not_allocate_or_replace_plugin_owners() {
        let project = project();
        let factory = Arc::new(FixtureFactory::default());
        let mut pool = crate::SamplePool::new();
        pool.set_plugin_factory(factory.clone());
        let (mut processor, control) = crate::Processor::new(48_000);
        control.set_project(&project, &pool);
        let channel = project.channels[0].id;
        let epoch = control.hardware_epoch();
        assert!(control.hardware_note(epoch, channel, 60, 100));
        let mut out = [0.0; 2048];
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        assert!(out.iter().any(|sample| *sample > 0.1));
        assert!(control.hardware_note(epoch, channel, 60, 0));
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        assert!(out.iter().all(|sample| *sample == 0.0));
        assert!(control.hardware_note(epoch, channel, 64, 100));
        processor.process(&mut out);
        control.panic_hardware();
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        assert!(out.iter().all(|sample| *sample == 0.0));
        assert_eq!(factory.made.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn fixture_instrument_and_effect_render_and_missing_plugins_are_silent() {
        let mut project = project();
        let factory = Arc::new(FixtureFactory::default());
        let mut pool = crate::SamplePool::new();
        pool.set_plugin_factory(factory.clone());
        let options = crate::RenderOptions::default();
        let audio = crate::render(&project, &pool, &options, &mut |_| true);
        assert!(audio.samples().iter().any(|sample| *sample > 0.1));
        assert!(
            crate::render(&project, &crate::SamplePool::new(), &options, &mut |_| true)
                .samples()
                .iter()
                .all(|sample| *sample == 0.0)
        );
        let mut doc = Document::new(project);
        doc.dispatch(
            Command::AddPluginEffect {
                track: TrackId(0),
                plugin: binding(),
            },
            None,
        )
        .unwrap();
        project = doc.project().clone();
        let effected = crate::render(&project, &pool, &options, &mut |_| true);
        assert!(effected.samples().iter().any(|sample| *sample > 0.05));
        let peak = |audio: &windfall_core::AudioBuffer| {
            audio.samples().iter().copied().fold(0.0_f32, f32::max)
        };
        assert!((peak(&effected) / peak(&audio) - 0.5).abs() < 0.001);
    }
    #[test]
    fn stopped_and_empty_callbacks_service_plugin_control_without_allocating() {
        let project = project();
        let factory = Arc::new(FixtureFactory::default());
        let mut pool = crate::SamplePool::new();
        pool.set_plugin_factory(factory.clone());
        let (mut processor, control) = crate::Processor::new(48_000);
        control.set_project(&project, &pool);
        processor.process(&mut [0.0; 128]);
        let before = factory.boundaries.load(Ordering::Relaxed);
        assert!(before > 0);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut [])),
            0
        );
        assert_eq!(factory.boundaries.load(Ordering::Relaxed), before + 1);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut [0.0; 128])),
            0
        );
        assert!(factory.boundaries.load(Ordering::Relaxed) > before + 1);
    }

    #[test]
    fn bypassed_and_departing_effects_service_ownership_before_plan_replacement() {
        let mut document = Document::new(Project::new("Bypassed ownership"));
        document
            .dispatch(
                Command::AddPluginEffect {
                    track: TrackId(0),
                    plugin: binding(),
                },
                None,
            )
            .unwrap();
        let effect = match document.project().plugins[0].target {
            PluginTarget::Effect { effect } => effect,
            _ => unreachable!(),
        };
        document
            .dispatch(
                Command::UpdateEffect {
                    track: TrackId(0),
                    effect,
                    patch: EffectSlotPatch {
                        enabled: Some(false),
                        ..Default::default()
                    },
                },
                None,
            )
            .unwrap();
        let factory = Arc::new(FixtureFactory::default());
        let mut pool = crate::SamplePool::new();
        pool.set_plugin_factory(factory.clone());
        let (mut processor, control) = crate::Processor::new(48_000);
        control.set_project(document.project(), &pool);
        processor.process(&mut [0.0; 128]);
        let before = factory.boundaries.load(Ordering::Relaxed);
        assert!(
            before > 0,
            "a bypassed effect still services its ownership exchange"
        );
        document
            .dispatch(
                Command::RemoveEffect {
                    track: TrackId(0),
                    effect,
                },
                None,
            )
            .unwrap();
        control.set_project(document.project(), &pool);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut [])),
            0
        );
        assert!(
            factory.boundaries.load(Ordering::Relaxed) > before,
            "the old effect returns ownership before its plan can be retired"
        );
    }

    #[test]
    fn parameter_updates_reuse_instances_and_never_allocate_on_audio_thread() {
        let mut project = project();
        let factory = Arc::new(FixtureFactory::default());
        let mut pool = crate::SamplePool::new();
        pool.set_plugin_factory(factory.clone());
        let (mut processor, control) = crate::Processor::new(48_000);
        control.set_project(&project, &pool);
        control.play();
        let mut out = [0.0; 512];
        processor.process(&mut out);
        assert_eq!(factory.made.load(Ordering::Relaxed), 1);
        project.plugins[0].parameters[0].value = 0.75;
        control.set_project(&project, &pool);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        assert_eq!(factory.made.load(Ordering::Relaxed), 1);
        assert!(out.iter().any(|sample| *sample > 0.5));
        let replacement = Arc::new(FixtureFactory::default());
        pool.set_plugin_factory(replacement.clone());
        control.set_project(&project, &pool);
        assert_eq!(
            crate::test_alloc::allocator_calls(|| processor.process(&mut out)),
            0
        );
        assert_eq!(
            replacement.made.load(Ordering::Relaxed),
            1,
            "replacing a provider with the same revision must prepare its own native units"
        );
    }
    #[test]
    fn reordered_native_ids_invalidate_cached_parameter_routing() {
        let mut plugin = binding();
        let mut other = plugin.parameters[0].clone();
        other.id = 99;
        plugin.parameters.push(other);
        let before = identity(&plugin);
        plugin.parameters[0].value = 0.9;
        assert_eq!(identity(&plugin), before, "value edits reuse the instance");
        plugin.parameters.swap(0, 1);
        assert_ne!(
            identity(&plugin),
            before,
            "reordering native IDs must rebuild index-to-ID routing"
        );
    }
    #[test]
    fn native_latency_is_compensated_and_notes_keep_their_sample_boundaries() {
        let mut project = project();
        let note = &mut project.patterns[0].lanes[0].notes[0];
        note.start = 48;
        note.length = 48;
        let mut doc = Document::new(project);
        doc.dispatch(
            Command::AddPluginEffect {
                track: TrackId(0),
                plugin: binding(),
            },
            None,
        )
        .unwrap();
        let project = doc.project();
        let mut plain = crate::SamplePool::new();
        plain.set_plugin_factory(Arc::new(FixtureFactory::default()));
        let mut delayed = crate::SamplePool::new();
        delayed.set_plugin_factory(Arc::new(FixtureFactory {
            latency: 32,
            ..Default::default()
        }));
        let options = crate::RenderOptions::default();
        let reference = crate::render(project, &plain, &options, &mut |_| true);
        let compensated = crate::render(project, &delayed, &options, &mut |_| true);
        assert_eq!(
            reference.samples(),
            compensated.samples(),
            "offline PDC must remove native latency without moving note boundaries"
        );
        let first = reference
            .samples()
            .as_chunks::<2>()
            .0
            .iter()
            .position(|frame| frame[0] != 0.0)
            .unwrap();
        let expected = (48.0
            * windfall_core::samples_per_tick(
                project.settings.tempo_bpm,
                f64::from(options.sample_rate),
            ))
        .ceil() as usize;
        assert!(
            first.abs_diff(expected) <= 1,
            "native note starts on its scheduled frame: {first} vs {expected}"
        );
    }
    #[test]
    fn native_automation_uses_discovered_ranges_and_stable_ids() {
        let mut doc = Document::new(project());
        doc.dispatch(
            Command::AddPlaylistTrack {
                name: None,
                index: None,
            },
            None,
        )
        .unwrap();
        let mut project = doc.project().clone();
        let channel = project.channels[0].id;
        let track = project.playlist.tracks[0].id;
        let mut id = || {
            let id = project.next_id;
            project.next_id += 1;
            id
        };
        let pattern_clip = ClipId(id());
        let automation = AutomationId(id());
        let automation_clip = ClipId(id());
        project.playlist.clips.push(Clip {
            id: pattern_clip,
            track,
            start: 0,
            length: 192,
            offset: 0,
            muted: false,
            content: ClipContent::Pattern {
                pattern: project.patterns[0].id,
            },
        });
        let mut pool = crate::SamplePool::new();
        pool.set_plugin_factory(Arc::new(FixtureFactory::default()));
        let options = crate::RenderOptions {
            mode: windfall_ipc::PlayMode::Song,
            ..Default::default()
        };
        let baseline = crate::render(&project, &pool, &options, &mut |_| true);
        project.automations.push(Automation {
            id: automation,
            name: "Native level".into(),
            color: 0,
            target: AutomationTarget::InstrumentParam { channel, param: 0 },
            points: vec![AutomationPoint {
                tick: 0,
                value: 0.4,
                curve: 0.0,
                hold: true,
            }],
        });
        project.playlist.clips.push(Clip {
            id: automation_clip,
            track,
            start: 0,
            length: 192,
            offset: 0,
            muted: false,
            content: ClipContent::Automation { automation },
        });
        project.check().unwrap();
        let automated = crate::render(&project, &pool, &options, &mut |_| true);
        let at = baseline
            .samples()
            .iter()
            .position(|sample| *sample > 0.1)
            .unwrap();
        assert!(
            (automated.samples()[at] / baseline.samples()[at] - 1.6).abs() < 0.001,
            "normalized 0.4 must map to native 0.8 in the discovered 0..2 range, using native ID 43"
        );
    }
}
