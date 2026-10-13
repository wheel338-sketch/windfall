//! The main-thread half of a CLAP plugin.

use std::collections::HashSet;
use std::ffi::CString;
use std::io::Cursor;
use std::sync::atomic::Ordering;
use std::time::Instant;

use clack_extensions::audio_ports::{AudioPortFlags, AudioPortInfoBuffer, PluginAudioPorts};
use clack_extensions::gui::{GuiApiType, GuiConfiguration, PluginGui};
use clack_extensions::latency::PluginLatency;
use clack_extensions::note_ports::{NoteDialects, NotePortInfoBuffer, PluginNotePorts};
use clack_extensions::params::{ParamInfoBuffer, ParamInfoFlags, PluginParams};
use clack_extensions::state::PluginState;
use clack_extensions::tail::PluginTail;
use clack_extensions::timer::{PluginTimer, TimerId};
use clack_host::plugin::PluginInstanceError;
use clack_host::prelude::*;

use super::editor::Editor;
use super::events::{Dialect, EventList, Outgoing};
use super::handlers::{AudioThread, MainThread, Shared, WindfallHost, flag};
use super::processor::{ClapProcessor, PortDesc};
use crate::descriptor::{AudioPort, MAX_PARAMETERS, MAX_PORT_CHANNELS, MAX_PORTS, PluginLayout};
use crate::error::PluginError;
use crate::events::{HostEvent, PluginEvent};
use crate::gui::{EditorError, EditorInfo, EditorOptions};
use crate::host::LogSink;
use crate::instance::{InstanceBackend, PluginNotification};
use crate::params::PluginParam;
use crate::processor::{AudioShape, ProcessorBackend, ProcessorShared};
use crate::state::LimitedWriter;

/// The plugin's side of the extensions the host uses.
#[derive(Clone, Copy)]
struct Extensions {
    audio_ports: Option<PluginAudioPorts>,
    note_ports: Option<PluginNotePorts>,
    params: Option<PluginParams>,
    state: Option<PluginState>,
    latency: Option<PluginLatency>,
    tail: Option<PluginTail>,
    gui: Option<PluginGui>,
    timer: Option<PluginTimer>,
}

pub(crate) struct ClapInstance {
    /// Declared before the instance so that it can never outlive it.
    editor: Option<Editor>,
    instance: PluginInstance<WindfallHost>,
    extensions: Extensions,
    layout: PluginLayout,
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
    dialect: Dialect,
    params: Vec<PluginParam>,
    /// Reused by `idle` for the timers that are due.
    due: Vec<u32>,
    /// Reused by `flush_params`.
    flush_list: EventList,
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn read_ports(
    extension: Option<PluginAudioPorts>,
    plugin: &PluginMainThreadHandle,
    is_input: bool,
) -> Result<Vec<PortDesc>, PluginError> {
    let Some(extension) = extension else {
        return Ok(Vec::new());
    };
    let side = if is_input { "inputs" } else { "outputs" };
    let count = extension.count(plugin, is_input);
    if count > MAX_PORTS {
        return Err(PluginError::Layout(format!(
            "it reports {count} audio {side}"
        )));
    }
    let mut buffer = AudioPortInfoBuffer::new();
    let mut ports = Vec::new();
    for index in 0..count {
        let Some(info) = extension.get(plugin, index, is_input, &mut buffer) else {
            return Err(PluginError::Layout(format!(
                "it does not describe port {index} of its audio {side}"
            )));
        };
        if info.channel_count > MAX_PORT_CHANNELS {
            return Err(PluginError::Layout(format!(
                "port {index} of its audio {side} has {} channels",
                info.channel_count
            )));
        }
        ports.push(PortDesc {
            id: info.id.get(),
            name: lossy(info.name),
            channels: info.channel_count,
            // Only the first port can be the main one. An output is taken
            // to be it whatever the plugin says, since the track's sound
            // has to come from somewhere.
            main: index == 0 && (!is_input || info.flags.contains(AudioPortFlags::IS_MAIN)),
            in_place_pair: info.in_place_pair.map(|id| id.get()),
        });
    }
    Ok(ports)
}

fn read_params(
    extension: Option<PluginParams>,
    plugin: &PluginMainThreadHandle,
) -> Vec<PluginParam> {
    let Some(extension) = extension else {
        return Vec::new();
    };
    let count = extension.count(plugin).min(MAX_PARAMETERS);
    let mut buffer = ParamInfoBuffer::new();
    let mut seen = HashSet::new();
    let mut params = Vec::new();
    for index in 0..count {
        let Some(info) = extension.get_info(plugin, index, &mut buffer) else {
            continue;
        };
        // An id may appear once, and a range has to be made of numbers.
        let usable = info.min_value.is_finite() && info.max_value.is_finite();
        if !usable || !seen.insert(info.id.get()) {
            continue;
        }
        let flags = info.flags;
        let stepped = flags.contains(ParamInfoFlags::IS_STEPPED);
        params.push(PluginParam {
            id: info.id.get(),
            name: lossy(info.name),
            module: lossy(info.module),
            min: info.min_value,
            max: info.max_value,
            default: if info.default_value.is_finite() {
                info.default_value
            } else {
                info.min_value
            },
            stepped,
            choice: stepped && flags.contains(ParamInfoFlags::IS_ENUM),
            automatable: flags.contains(ParamInfoFlags::IS_AUTOMATABLE),
            read_only: flags.contains(ParamInfoFlags::IS_READONLY),
            hidden: flags.contains(ParamInfoFlags::IS_HIDDEN),
            bypass: flags.contains(ParamInfoFlags::IS_BYPASS),
            periodic: flags.contains(ParamInfoFlags::IS_PERIODIC),
        });
    }
    params
}

/// Whether the plugin has an editor the host can open on this platform.
fn can_open_editor(gui: Option<PluginGui>, plugin: &PluginMainThreadHandle) -> bool {
    let (Some(gui), Some(api)) = (gui, GuiApiType::default_for_current_platform()) else {
        return false;
    };
    let supports = |is_floating| {
        let configuration = GuiConfiguration {
            api_type: api,
            is_floating,
        };
        gui.is_api_supported(plugin, configuration)
    };
    // Embedding needs a host window, which only Windows has so far.
    supports(true) || (cfg!(windows) && supports(false))
}

fn audio_port(port: &PortDesc) -> AudioPort {
    AudioPort {
        name: port.name.clone(),
        channels: port.channels,
        main: port.main,
    }
}

impl ClapInstance {
    pub fn create(
        entry: &PluginEntry,
        id: &str,
        host: &HostInfo,
        log: LogSink,
    ) -> Result<Self, PluginError> {
        let c_id = CString::new(id).map_err(|_| PluginError::NotFound(id.to_owned()))?;
        let mut instance = PluginInstance::<WindfallHost>::new(
            |_| Shared::new(log),
            |shared| MainThread::new(shared),
            entry,
            &c_id,
            host,
        )
        .map_err(|error| match error {
            PluginInstanceError::PluginNotFound => PluginError::NotFound(id.to_owned()),
            other => PluginError::Create(other.to_string()),
        })?;

        let shared = instance.plugin_shared_handle();
        let extensions = Extensions {
            audio_ports: shared.get_extension(),
            note_ports: shared.get_extension(),
            params: shared.get_extension(),
            state: shared.get_extension(),
            latency: shared.get_extension(),
            tail: shared.get_extension(),
            gui: shared.get_extension(),
            timer: shared.get_extension(),
        };

        let plugin = instance.plugin_handle();
        let inputs = read_ports(extensions.audio_ports, &plugin, true)?;
        let outputs = read_ports(extensions.audio_ports, &plugin, false)?;

        let (mut note_inputs, mut note_outputs, mut dialect) = (0, 0, Dialect::None);
        if let Some(note_ports) = extensions.note_ports {
            note_inputs = note_ports.count(&plugin, true);
            note_outputs = note_ports.count(&plugin, false);
            if note_inputs > MAX_PORTS || note_outputs > MAX_PORTS {
                return Err(PluginError::Layout(format!(
                    "it reports {note_inputs} note inputs and {note_outputs} note outputs"
                )));
            }
            let mut buffer = NotePortInfoBuffer::new();
            if let Some(port) = note_ports.get(&plugin, 0, true, &mut buffer) {
                if port.supported_dialects.contains(NoteDialects::CLAP) {
                    dialect = Dialect::Clap;
                } else if port.supported_dialects.contains(NoteDialects::MIDI) {
                    dialect = Dialect::Midi;
                }
            }
        }

        let params = read_params(extensions.params, &plugin);
        let layout = PluginLayout {
            audio_inputs: inputs.iter().map(audio_port).collect(),
            audio_outputs: outputs.iter().map(audio_port).collect(),
            note_inputs,
            note_outputs,
            has_editor: can_open_editor(extensions.gui, &plugin),
            parameter_count: params.len() as u32,
            has_state: extensions.state.is_some(),
        };

        Ok(Self {
            editor: None,
            instance,
            extensions,
            layout,
            inputs,
            outputs,
            dialect,
            params,
            due: Vec::new(),
            flush_list: EventList::new(Dialect::None),
        })
    }

    fn main_input(&self) -> Option<&PortDesc> {
        self.inputs
            .first()
            .filter(|port| port.main && port.channels > 0)
    }

    fn main_output(&self) -> Option<&PortDesc> {
        self.outputs
            .first()
            .filter(|port| port.main && port.channels > 0)
    }

    fn take_flags(&self) -> u32 {
        self.instance
            .access_shared_handler(|shared| shared.take(flag::ALL & !flag::TAIL))
    }
}

impl InstanceBackend for ClapInstance {
    fn refresh_capabilities(&mut self) -> Result<(), PluginError> {
        // Finish a requested owner callback without consuming its dirty/restart
        // notifications. The installed owner will deliver those through idle.
        let callback = self
            .instance
            .access_shared_handler(|shared| shared.take(flag::CALLBACK));
        if callback != 0 {
            self.instance.call_on_main_thread_callback();
        }
        let plugin = self.instance.plugin_handle();
        let inputs = read_ports(self.extensions.audio_ports, &plugin, true)?;
        let outputs = read_ports(self.extensions.audio_ports, &plugin, false)?;
        let (mut note_inputs, mut note_outputs, mut dialect) = (0, 0, Dialect::None);
        if let Some(ports) = self.extensions.note_ports {
            note_inputs = ports.count(&plugin, true);
            note_outputs = ports.count(&plugin, false);
            if note_inputs > MAX_PORTS || note_outputs > MAX_PORTS {
                return Err(PluginError::Layout("invalid refreshed note ports".into()));
            }
            let mut buffer = NotePortInfoBuffer::new();
            if let Some(port) = ports.get(&plugin, 0, true, &mut buffer) {
                if port.supported_dialects.contains(NoteDialects::CLAP) {
                    dialect = Dialect::Clap;
                } else if port.supported_dialects.contains(NoteDialects::MIDI) {
                    dialect = Dialect::Midi;
                }
            }
        }
        let params = read_params(self.extensions.params, &plugin);
        self.layout.audio_inputs = inputs.iter().map(audio_port).collect();
        self.layout.audio_outputs = outputs.iter().map(audio_port).collect();
        self.layout.note_inputs = note_inputs;
        self.layout.note_outputs = note_outputs;
        self.layout.parameter_count = params.len() as u32;
        self.inputs = inputs;
        self.outputs = outputs;
        self.dialect = dialect;
        self.params = params;
        Ok(())
    }
    fn layout(&self) -> &PluginLayout {
        &self.layout
    }

    fn shape(&self) -> AudioShape {
        let (input, output) = (self.main_input(), self.main_output());
        let in_place = match (input, output) {
            (Some(input), Some(output)) => {
                input.channels == 2
                    && output.channels == 2
                    && input.in_place_pair == Some(output.id)
                    && output.in_place_pair == Some(input.id)
            }
            _ => false,
        };
        AudioShape {
            has_input: input.is_some(),
            in_place,
        }
    }

    fn params(&self) -> &[PluginParam] {
        &self.params
    }

    fn param_value(&mut self, id: u32) -> Option<f64> {
        let id = ClapId::from_raw(id)?;
        self.extensions
            .params?
            .get_value(&self.instance.plugin_handle(), id)
    }

    fn param_text(&mut self, id: u32, value: f64) -> Option<String> {
        let id = ClapId::from_raw(id)?;
        let mut buffer = [0_u8; 256];
        let text = self
            .extensions
            .params?
            .value_to_text(&self.instance.plugin_handle(), id, value, &mut buffer)
            .ok()?;
        Some(lossy(text))
    }

    fn param_from_text(&mut self, id: u32, text: &str) -> Option<f64> {
        let id = ClapId::from_raw(id)?;
        let text = CString::new(text).ok()?;
        self.extensions
            .params?
            .text_to_value(&self.instance.plugin_handle(), id, &text)
            .filter(|value| value.is_finite())
    }

    fn flush_params(&mut self, changes: &[HostEvent], out: &mut dyn FnMut(PluginEvent)) {
        let Some(params) = self.extensions.params else {
            return;
        };
        let Some(mut plugin) = self.instance.inactive_plugin_handle() else {
            return;
        };
        let mut outgoing = Outgoing { sink: out };
        let mut remaining = changes;
        loop {
            let (chunk, rest) =
                remaining.split_at(remaining.len().min(crate::processor::EVENT_CAPACITY));
            self.flush_list.fill(chunk, Dialect::None);
            params.flush(
                &mut plugin,
                &InputEvents::from_buffer(&self.flush_list),
                &mut OutputEvents::from_buffer(&mut outgoing),
            );
            remaining = rest;
            if remaining.is_empty() {
                break;
            }
        }
    }

    fn save_state(&mut self, limit: usize) -> Result<Option<Vec<u8>>, PluginError> {
        let Some(state) = self.extensions.state else {
            return Ok(None);
        };
        let mut writer = LimitedWriter::new(limit);
        state
            .save(&self.instance.plugin_handle(), &mut writer)
            .map_err(|_| PluginError::State("saved"))?;
        // Native code may ignore a refused stream write and report success.
        // Never turn that partial payload into a valid state/cached capture.
        if writer.failed() {
            return Err(PluginError::State("saved"));
        }
        Ok(Some(writer.bytes))
    }

    fn load_state(&mut self, bytes: &[u8]) -> Result<(), PluginError> {
        let state = self.extensions.state.ok_or(PluginError::State("loaded"))?;
        state
            .load(&self.instance.plugin_handle(), &mut Cursor::new(bytes))
            .map_err(|_| PluginError::State("loaded"))
    }

    fn activate(
        &mut self,
        sample_rate: f64,
        max_block: u32,
        shared: &ProcessorShared,
    ) -> Result<Box<dyn ProcessorBackend>, PluginError> {
        if self.main_output().is_none() {
            return Err(PluginError::Layout("it has no audio output".to_owned()));
        }
        let configuration = PluginAudioConfiguration {
            sample_rate,
            min_frames_count: 1,
            max_frames_count: max_block,
        };
        let processor = self
            .instance
            .activate(|shared, _| AudioThread::new(shared), configuration)
            .map_err(|error| PluginError::Activate(error.to_string()))?;

        // CLAP lets a plugin report its latency only once it is active.
        let latency = self
            .extensions
            .latency
            .map_or(0, |latency| latency.get(&self.instance.plugin_handle()));
        shared.latency.store(latency, Ordering::Relaxed);
        self.instance
            .access_shared_handler(|shared| shared.take(flag::LATENCY));

        Ok(Box::new(ClapProcessor::new(
            processor,
            self.extensions.tail,
            self.dialect,
            &self.inputs,
            &self.outputs,
            max_block as usize,
        )))
    }

    fn finish_deactivation(&mut self, processor: Option<Box<dyn ProcessorBackend>>) {
        let processor = processor.and_then(|processor| {
            processor
                .into_any()
                .downcast::<ClapProcessor>()
                .ok()
                .map(|processor| processor.into_stopped())
        });
        match processor {
            Some(processor) if processor.matches(&self.instance) => {
                self.instance.deactivate(processor);
            }
            // Without its processor the plugin can be deactivated only if
            // the processor no longer exists. If it does, clack keeps the
            // plugin alive rather than pull it out from under the audio
            // thread.
            _ => {
                let _ = self.instance.try_deactivate();
            }
        }
    }

    fn idle(
        &mut self,
        now: Instant,
        active: Option<&ProcessorShared>,
        notify: &mut dyn FnMut(PluginNotification),
    ) {
        let flags = self.take_flags();
        if flags & flag::CALLBACK != 0 {
            self.instance.call_on_main_thread_callback();
        }

        self.due.clear();
        let due = &mut self.due;
        self.instance
            .access_handler(|main| main.collect_due(now, due));
        if let Some(timer) = self.extensions.timer {
            for &id in &self.due {
                timer.on_timer(&self.instance.plugin_handle(), TimerId(id));
            }
        }

        if flags & flag::LATENCY != 0
            && let (Some(latency), Some(active)) = (self.extensions.latency, active)
        {
            let samples = latency.get(&self.instance.plugin_handle());
            if active.latency.load(Ordering::Relaxed) != samples {
                notify(PluginNotification::LatencyChanged { samples });
            }
        }
        if flags & flag::RESTART != 0 {
            notify(PluginNotification::RestartRequested);
        }
        if flags & flag::PARAMS != 0 {
            self.params = read_params(self.extensions.params, &self.instance.plugin_handle());
            self.layout.parameter_count = self.params.len() as u32;
            notify(PluginNotification::ParamsRescanned);
        }
        // An active plugin gets its flush with the next block.
        if flags & flag::FLUSH != 0 && active.is_none() {
            self.flush_params(&[], &mut |event| notify(event.into()));
        }
        if flags & flag::STATE_DIRTY != 0 {
            notify(PluginNotification::StateChanged);
        }

        let Some(editor) = &mut self.editor else {
            return;
        };
        if flags & flag::GUI_CLOSED != 0 || editor.take_close_request() {
            self.close_editor();
            notify(PluginNotification::EditorClosed);
            return;
        }
        let mut resized = None;
        #[cfg(any(windows, target_os = "macos", target_os = "linux"))]
        if let Some((application, monitor, scaled)) = editor.poll_monitor_scale(&mut self.instance)
        {
            match scaled {
                Ok(info) => resized = Some((info.width, info.height)),
                Err(_) => notify(PluginNotification::EditorScaleRefused {
                    application,
                    monitor,
                }),
            }
        }
        if flags & flag::GUI_RESIZE != 0 {
            let (width, height) = self
                .instance
                .access_shared_handler(|shared| shared.requested_size());
            // An idempotent queued request must not erase the DPI notification
            // already negotiated on this same idle turn. A changed request
            // still takes precedence and reports the final actual dimensions.
            resized = editor.apply_requested_size(width, height).or(resized);
        }
        if let Some((width, height)) = resized.or_else(|| editor.poll_size()) {
            notify(PluginNotification::EditorResized { width, height });
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.instance.access_handler(|main| main.next_due())
    }

    fn open_editor(&mut self, options: &EditorOptions) -> Result<EditorInfo, EditorError> {
        if self.editor.is_some() {
            return Err(EditorError::AlreadyOpen);
        }
        let gui = self.extensions.gui.ok_or(EditorError::NoEditor)?;
        let editor = Editor::open(&mut self.instance, gui, options)?;
        let info = editor.info();
        self.editor = Some(editor);
        Ok(info)
    }

    fn close_editor(&mut self) {
        if let Some(editor) = self.editor.take() {
            editor.close(&mut self.instance);
        }
    }

    fn editor(&self) -> Option<EditorInfo> {
        self.editor.as_ref().map(Editor::info)
    }

    fn set_editor_scale(&mut self, application: f64) -> Result<EditorInfo, EditorError> {
        self.editor
            .as_mut()
            .ok_or(EditorError::NoEditor)?
            .set_scale(&mut self.instance, application)
    }
}

impl Drop for ClapInstance {
    fn drop(&mut self) {
        self.close_editor();
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;

    #[test]
    #[ignore = "requires explicitly built WINDFALL_BRIDGE_FIXTURE; runs no nested Cargo"]
    fn native_success_after_a_refused_stream_write_is_not_valid_state() {
        let path = std::env::var_os("WINDFALL_BRIDGE_FIXTURE").expect("native fixture path");
        // SAFETY: explicitly built owned synthetic native fixture.
        let entry = unsafe { PluginEntry::load(std::path::Path::new(&path)) }.unwrap();
        let info = HostInfo::new(
            "Windfall state regression",
            "Windfall",
            "https://windfall.local",
            "1",
        )
        .unwrap();
        let mut native = ClapInstance::create(
            &entry,
            "org.windfall.test.bridge-ignored-stream-error",
            &info,
            super::super::silent_log(),
        )
        .unwrap();
        native.flush_params(
            &[HostEvent::Param {
                time: 0,
                id: 7,
                value: 0.5,
            }],
            &mut |_| {},
        );
        let cached = native.save_state(32).unwrap().unwrap();
        native.flush_params(
            &[HostEvent::Param {
                time: 0,
                id: 7,
                value: 0.75,
            }],
            &mut |_| {},
        );
        assert!(
            native.save_state(32).is_err(),
            "plugin ignored refused write and relabeled partial bytes as success"
        );
        native.load_state(&cached).unwrap();
        assert_eq!(native.param_value(7), Some(0.5));
        assert_eq!(native.save_state(32).unwrap().unwrap(), cached);
    }
}
