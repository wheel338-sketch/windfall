//! The main-thread half of a plugin: everything about it except running
//! audio.
//!
//! A [`PluginInstance`] belongs to the thread that created it and cannot be
//! sent to another. Plugins require that, and on macOS that thread has to
//! be the one the app's windows live on. The shell creates instances,
//! activates them, and hands the [`PluginProcessor`] each one gives to the
//! engine.

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;

use crate::containment::PluginHealth;
use crate::descriptor::{PluginDescriptor, PluginLayout};
use crate::error::PluginError;
use crate::events::{HostEvent, PluginEvent};
use crate::gui::{EditorError, EditorInfo, EditorOptions};
use crate::params::{PluginParam, PluginParamInfo};
use crate::processor::{
    AudioShape, PluginProcessor, ProcessorBackend, ProcessorParts, ProcessorShared,
};
use crate::state::{MAX_STATE_BYTES, PluginState, StateContent};

/// Parameter changes the main thread can have on their way to the audio
/// thread at once, and plugin events on their way back.
pub(crate) const QUEUE_CAPACITY: usize = 4096;

/// Something a plugin did that the app may want to act on. They come out of
/// [`PluginInstance::idle`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PluginNotification {
    /// The user took hold of a parameter in the plugin's editor. What
    /// follows until [`PluginNotification::ParamGestureEnd`] is one edit,
    /// which the app can record as a single undo step or as automation.
    ParamGestureBegin { id: u32 },
    /// The plugin changed a parameter itself, in its own units.
    ParamChanged { id: u32, value: f64 },
    /// The user let go of the parameter.
    ParamGestureEnd { id: u32 },
    /// A note finished sounding, release included.
    NoteEnded { key: u8, channel: u8 },
    /// The parameter list or its values changed in bulk, as when the user
    /// loads a preset. Read [`PluginInstance::params`] and the values again.
    ParamsRescanned,
    /// The plugin's latency is now this many samples.
    LatencyChanged { samples: u32 },
    /// The plugin wants to be deactivated and activated again, because
    /// something only that can change did change.
    RestartRequested,
    /// The plugin's state is no longer what was last saved or loaded.
    StateChanged,
    /// The editor is gone: the user closed its window, or the plugin did.
    EditorClosed,
    /// The editor's window now has this size.
    EditorResized { width: u32, height: u32 },
    /// A live monitor transition was refused; existing content remains usable.
    EditorScaleRefused { application: f64, monitor: f64 },
}

impl From<PluginEvent> for PluginNotification {
    fn from(event: PluginEvent) -> Self {
        match event {
            PluginEvent::GestureBegin { id } => Self::ParamGestureBegin { id },
            PluginEvent::ParamValue { id, value } => Self::ParamChanged { id, value },
            PluginEvent::GestureEnd { id } => Self::ParamGestureEnd { id },
            PluginEvent::NoteEnd { key, channel } => Self::NoteEnded { key, channel },
        }
    }
}

/// One plugin format's way of doing what a [`PluginInstance`] offers. Every
/// method is called on the thread that created the instance.
pub(crate) trait InstanceBackend {
    /// Proves inactive native state without consuming a refused processor.
    fn quiesce(&mut self, processor: &mut dyn ProcessorBackend) -> Result<(), PluginError> {
        processor.stop();
        Ok(())
    }
    fn layout(&self) -> &PluginLayout;
    fn shape(&self) -> AudioShape;
    fn params(&self) -> &[PluginParam];
    fn param_value(&mut self, id: u32) -> Option<f64>;
    fn param_text(&mut self, id: u32, value: f64) -> Option<String>;
    fn param_from_text(&mut self, id: u32, text: &str) -> Option<f64>;
    /// Hands parameter changes to a plugin that is not active. Whatever the
    /// plugin has to say in return goes to `out`.
    fn flush_params(&mut self, changes: &[HostEvent], out: &mut dyn FnMut(PluginEvent));
    fn flush_retired_params(&mut self, changes: &[HostEvent], out: &mut dyn FnMut(PluginEvent)) {
        self.flush_params(changes, out);
    }
    fn deactivation_param_value(&self, _id: u32) -> Option<f64> {
        None
    }
    /// The bytes the plugin's state extension writes, or `None` if it has
    /// no such extension.
    fn save_state(&mut self, limit: usize) -> Result<Option<Vec<u8>>, PluginError>;
    fn load_state(&mut self, bytes: &[u8]) -> Result<(), PluginError>;
    fn activate(
        &mut self,
        sample_rate: f64,
        max_block: u32,
        shared: &ProcessorShared,
    ) -> Result<Box<dyn ProcessorBackend>, PluginError>;
    /// Deactivates the plugin. `processor` is what `activate` returned, or
    /// `None` if it was lost, in which case the plugin can only be torn
    /// down.
    fn finish_deactivation(&mut self, processor: Option<Box<dyn ProcessorBackend>>);
    /// Does the plugin's main-thread chores. `active` is set while the
    /// plugin has a processor out.
    fn idle(
        &mut self,
        now: Instant,
        active: Option<&ProcessorShared>,
        notify: &mut dyn FnMut(PluginNotification),
    );
    /// When [`idle`](Self::idle) next has a timer to fire.
    fn next_deadline(&self) -> Option<Instant>;
    fn open_editor(&mut self, options: &EditorOptions) -> Result<EditorInfo, EditorError>;
    fn close_editor(&mut self);
    fn editor(&self) -> Option<EditorInfo>;
    fn set_editor_scale(&mut self, _application: f64) -> Result<EditorInfo, EditorError> {
        Err(EditorError::UnsupportedScale)
    }
}

struct Active {
    additional_latency: u32,
    shared: Arc<ProcessorShared>,
    to_audio: rtrb::Producer<HostEvent>,
    from_audio: rtrb::Consumer<PluginEvent>,
}

/// One plugin, created and ready to be set up. See the [module](self).
pub struct PluginInstance {
    descriptor: PluginDescriptor,
    backend: Box<dyn InstanceBackend>,
    active: Option<Active>,
    /// What the audio thread had still to report when the plugin was
    /// deactivated, for the next `idle`.
    left_over: Vec<PluginNotification>,
    /// Plugins are tied to the thread they were created on.
    not_send: PhantomData<*const ()>,
}

impl PluginInstance {
    pub(crate) fn new(descriptor: PluginDescriptor, backend: Box<dyn InstanceBackend>) -> Self {
        Self {
            descriptor,
            backend,
            active: None,
            left_over: Vec::new(),
            not_send: PhantomData,
        }
    }

    /// What the plugin's file says about it.
    pub fn descriptor(&self) -> &PluginDescriptor {
        &self.descriptor
    }

    /// The plugin's ports and what else it reports about itself.
    pub fn layout(&self) -> &PluginLayout {
        self.backend.layout()
    }

    /// The plugin's parameters, in the plugin's order. The list can change
    /// when [`idle`](Self::idle) reports
    /// [`PluginNotification::ParamsRescanned`].
    pub fn params(&self) -> &[PluginParam] {
        self.backend.params()
    }

    /// The parameter with the given id.
    pub fn param(&self, id: u32) -> Option<&PluginParam> {
        self.params().iter().find(|param| param.id == id)
    }

    /// The current value of a parameter, in the plugin's units.
    pub fn param_value(&mut self, id: u32) -> Option<f64> {
        self.backend.param_value(id)
    }

    /// A native edit made by the last successful deactivation, in native
    /// parameter units. It supersedes controls queued before that lifecycle
    /// call. Available only while inactive; a later explicit set remains valid.
    pub fn deactivation_param_value(&self, id: u32) -> Option<f64> {
        self.active
            .is_none()
            .then(|| self.backend.deactivation_param_value(id))
            .flatten()
    }

    /// How the plugin writes `value` of a parameter, such as `-6.0 dB`.
    pub fn param_text(&mut self, id: u32, value: f64) -> Option<String> {
        self.backend.param_text(id, value)
    }

    /// The value the plugin reads from text the user typed, or `None` if it
    /// cannot make sense of it.
    pub fn param_from_text(&mut self, id: u32, text: &str) -> Option<f64> {
        self.backend.param_from_text(id, text)
    }

    /// A parameter in the shape of the app's own descriptors. For a choice
    /// parameter this asks the plugin for the name of every choice.
    pub fn param_info(&mut self, id: u32) -> Option<PluginParamInfo> {
        let param = self.param(id)?.clone();
        let mut choices = Vec::new();
        if param.choice
            && let Some(count) = param.step_count()
        {
            for step in 0..count {
                let value = param.min + step as f64;
                let label = self
                    .param_text(id, value)
                    .unwrap_or_else(|| value.to_string());
                choices.push(label);
            }
        }
        Some(param.info(choices))
    }

    /// Sets a parameter, in the plugin's units. The value is forced into
    /// the parameter's range first.
    ///
    /// The change reaches the plugin as an event: at the start of the next
    /// block when the plugin is active, at once when it is not. Returns
    /// false for an unknown or read-only parameter, and when too many
    /// changes are already on their way to the audio thread.
    pub fn set_param(&mut self, id: u32, value: f64) -> bool {
        let Some(param) = self.param(id) else {
            return false;
        };
        if param.read_only {
            return false;
        }
        let event = HostEvent::Param {
            time: 0,
            id,
            value: param.clamp(value),
        };
        match &mut self.active {
            Some(active) => active.to_audio.push(event).is_ok(),
            None => {
                self.backend.flush_params(&[event], &mut |_| {});
                true
            }
        }
    }

    /// Saves everything the plugin needs to come back as it is now.
    ///
    /// A plugin without a state extension is saved as the values of its
    /// parameters.
    pub fn save_state(&mut self) -> Result<PluginState, PluginError> {
        if let Some(bytes) = self.backend.save_state(MAX_STATE_BYTES)? {
            return Ok(PluginState::native(&bytes));
        }
        let ids: Vec<u32> = self
            .params()
            .iter()
            .filter(|param| !param.read_only)
            .map(|param| param.id)
            .collect();
        let values: Vec<(u32, f64)> = ids
            .into_iter()
            .filter_map(|id| Some((id, self.param_value(id)?)))
            .collect();
        Ok(PluginState::parameters(&values))
    }

    /// Restores a state that [`save_state`](Self::save_state) returned, for
    /// this instance or any other of the same plugin.
    pub fn load_state(&mut self, state: &PluginState) -> Result<(), PluginError> {
        match state.content()? {
            StateContent::Native(bytes) => self.backend.load_state(bytes),
            StateContent::Parameters(values) => {
                for (id, value) in values {
                    self.set_param(id, value);
                }
                Ok(())
            }
        }
    }

    /// True while a processor from [`activate`](Self::activate) is out.
    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// Readies the plugin to process audio at `sample_rate` in blocks of up
    /// to `max_block` frames, and returns the half of it that does so.
    ///
    /// This is where the plugin and the host allocate. Send the processor
    /// to the audio thread, and give it back to
    /// [`deactivate`](Self::deactivate) when it is done there.
    pub fn activate(
        &mut self,
        sample_rate: f64,
        max_block: usize,
    ) -> Result<PluginProcessor, PluginError> {
        self.activate_with_options(
            sample_rate,
            max_block,
            &crate::wrapper::WrapperOptions::default(),
        )
    }
    pub fn activate_with_options(
        &mut self,
        sample_rate: f64,
        max_block: usize,
        options: &crate::wrapper::WrapperOptions,
    ) -> Result<PluginProcessor, PluginError> {
        options.validate(max_block)?;
        if options.processing != crate::wrapper::ProcessingMode::InProcess {
            return Err(PluginError::Unsupported(
                "threaded/bridged modes require a separate owner provider",
            ));
        }
        if self.active.is_some() {
            return Err(PluginError::AlreadyActive);
        }
        if !sample_rate.is_finite() || sample_rate <= 0.0 || max_block == 0 {
            return Err(PluginError::Activate(format!(
                "cannot run at {sample_rate} Hz in blocks of {max_block}"
            )));
        }
        let block = u32::try_from(max_block).unwrap_or(u32::MAX);
        let shared = Arc::new(ProcessorShared::default());
        let backend = self.backend.activate(sample_rate, block, &shared)?;
        let backend = crate::wrapper::wrap(
            backend,
            options,
            sample_rate,
            shared.tail.load(std::sync::atomic::Ordering::Relaxed),
        );
        let (to_audio, from_main) = rtrb::RingBuffer::new(QUEUE_CAPACITY);
        let (to_main, from_audio) = rtrb::RingBuffer::new(QUEUE_CAPACITY);
        self.active = Some(Active {
            additional_latency: backend.additional_latency(),
            shared: Arc::clone(&shared),
            to_audio,
            from_audio,
        });
        Ok(PluginProcessor::new(ProcessorParts {
            backend,
            shared,
            shape: AudioShape {
                in_place: self.backend.shape().in_place
                    && !options.smart_disable
                    && options.fixed_block.is_none(),
                ..self.backend.shape()
            },
            sample_rate,
            max_block,
            from_main,
            to_main,
        }))
    }

    /// Takes a processor back and deactivates the plugin, which can then be
    /// activated again with other settings. What the plugin still had to
    /// say is kept for the next [`idle`](Self::idle).
    ///
    /// Refusal returns the exact processor; neither state nor ownership is lost.
    // The error deliberately returns the exact processor without another
    // allocation. This lifecycle API belongs on its creating owner.
    #[allow(clippy::result_large_err)]
    pub fn deactivate(
        &mut self,
        mut processor: PluginProcessor,
    ) -> Result<(), crate::DeactivationError<PluginProcessor>> {
        let Some(active) = &self.active else {
            return Err(crate::DeactivationError {
                error: PluginError::Deactivate("instance is not active".into()),
                returned: processor,
            });
        };
        if !Arc::ptr_eq(&active.shared, processor.shared()) {
            return Err(crate::DeactivationError {
                error: PluginError::Deactivate("processor belongs to another instance".into()),
                returned: processor,
            });
        }
        if let Err(error) = self.backend.quiesce(processor.backend_mut()) {
            return Err(crate::DeactivationError {
                error,
                returned: processor,
            });
        }
        let (backend, pending) = processor.into_backend();
        self.backend.finish_deactivation(Some(backend));
        if let Some(mut active) = self.active.take() {
            while let Ok(event) = active.from_audio.pop() {
                self.left_over.push(event.into());
            }
        }
        // Inactive owner-thread flush, after the audio half has returned and
        // native processing has stopped. No callback allocation is introduced.
        self.backend
            .flush_retired_params(&pending, &mut |event| self.left_over.push(event.into()));
        Ok(())
    }

    /// Samples by which the plugin's output lags its input. Plugins report
    /// this once they are active, so it is 0 before.
    pub fn latency_samples(&self) -> u32 {
        self.active.as_ref().map_or(0, |active| {
            active
                .shared
                .latency
                .load(std::sync::atomic::Ordering::Relaxed)
                .saturating_add(active.additional_latency)
        })
    }

    /// How the plugin has behaved on the audio thread since it was
    /// activated.
    pub fn health(&self) -> PluginHealth {
        self.active
            .as_ref()
            .map_or_else(PluginHealth::default, |active| active.shared.health.read())
    }

    /// Does the plugin's main-thread chores and reports what it has done
    /// since the last call. Call it from the app's event loop, about 60
    /// times a second while an editor is open.
    ///
    /// This runs the callbacks the plugin asked for, fires its timers,
    /// follows its editor window, and passes on the parameter changes it
    /// made on the audio thread.
    pub fn idle(&mut self, notify: &mut dyn FnMut(PluginNotification)) {
        let shared = self
            .active
            .as_ref()
            .map(|active| Arc::clone(&active.shared));
        self.backend.idle(Instant::now(), shared.as_deref(), notify);
        for notification in self.left_over.drain(..) {
            notify(notification);
        }
        if let Some(active) = &mut self.active {
            while let Ok(event) = active.from_audio.pop() {
                notify(event.into());
            }
        }
    }

    /// When [`idle`](Self::idle) next has a timer of the plugin's to fire,
    /// so an event loop knows how long it may sleep.
    pub fn next_deadline(&self) -> Option<Instant> {
        self.backend.next_deadline()
    }

    /// True if the plugin has an editor this host can open on this
    /// platform.
    pub fn has_editor(&self) -> bool {
        self.layout().has_editor
    }

    /// Opens the plugin's editor in a window of its own.
    pub fn open_editor(&mut self, options: &EditorOptions) -> Result<EditorInfo, EditorError> {
        options.validate()?;
        self.backend.open_editor(options)
    }

    /// Closes the editor, if it is open.
    pub fn close_editor(&mut self) {
        self.backend.close_editor();
    }

    /// The open editor's window, if there is one.
    pub fn editor(&self) -> Option<EditorInfo> {
        self.backend.editor()
    }

    /// Main-thread native content scaling; no musical state or audio command.
    /// Refusal keeps the current editor alive at its previous scale.
    pub fn set_editor_scale(&mut self, application: f64) -> Result<EditorInfo, EditorError> {
        crate::gui::scaling::content_scale(application, 1.0)?;
        if self.editor().is_none() {
            return Err(EditorError::NoEditor);
        }
        self.backend.set_editor_scale(application)
    }
}

impl Drop for PluginInstance {
    fn drop(&mut self) {
        self.backend.close_editor();
        if self.active.is_some() {
            // The processor is still out. The backend decides what is safe:
            // for CLAP that is to leave the plugin alive and unreachable.
            self.backend.finish_deactivation(None);
        }
    }
}
