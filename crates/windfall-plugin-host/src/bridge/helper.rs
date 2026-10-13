//! Native owner inside the helper. No mapped pointer is handed to a plugin.

use super::{
    adapter::ParameterSpec,
    control::{ControlParameter, Decoder, DescribedParameter, Message, Packet, ReadStep},
    mapping::Mapping,
    protocol::*,
    slots::{InputBlock, OutputBlock, Region},
};
use crate::{
    HostEvent, PluginFormat, PluginHost, PluginInstance, PluginKind, PluginNotification,
    PluginProcessor, PluginState, ProcessStatus,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    net::{SocketAddr, TcpStream},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

fn error(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}
fn captured_state(state: PluginState) -> io::Result<Vec<u8>> {
    // Native save has its own raw cap. The transport cap includes WFPS bytes;
    // refusal belongs to this recoverable control operation, never reply IO.
    super::control::checked_state_size(state.as_bytes().len())?;
    state.content().map_err(error)?;
    Ok(state.into_bytes())
}
fn reply(socket: &mut TcpStream, packet: &Packet) -> io::Result<()> {
    socket.set_nonblocking(false)?;
    socket.set_write_timeout(Some(Duration::from_secs(10)))?;
    let result = packet.write(socket);
    socket.set_nonblocking(true)?;
    result
}

struct Native {
    wrapper: crate::wrapper::WrapperOptions,
    processor: Option<PluginProcessor>,
    instance: PluginInstance,
    region: Region,
    parameters: Vec<ParameterSpec>,
    host_values: BTreeMap<u32, f64>,
    uncertain: BTreeSet<u32>,
    notes_uncertain: bool,
    sequence: Option<u64>,
    epoch: u64,
    processed_generation: u64,
    processed_epoch: u64,
    offline: bool,
    dirty: bool,
}
impl Drop for Native {
    fn drop(&mut self) {
        // Same creating owner. Refused/hung teardown is contained by the
        // supervisor's failure latch/deadline and confirmed process exit.
        let _ = self.deactivate();
    }
}
impl Native {
    fn describe(&mut self) -> io::Result<(Vec<u8>, Vec<DescribedParameter>, crate::PluginLayout)> {
        // Discovery must not stop notes or disturb a playback instance. The
        // desktop creates a separate fresh helper specifically for discovery.
        if self.sequence.is_some() || self.processed_epoch != 0 {
            return Err(error(
                "native discovery requires a fresh unprocessed helper",
            ));
        }
        self.deactivate()?;
        let result = (|| {
            self.instance.idle(&mut |_| {});
            let state = captured_state(self.instance.save_state().map_err(error)?)?;
            let parameters = self
                .instance
                .params()
                .to_vec()
                .into_iter()
                .filter(|param| !param.hidden)
                .map(|param| DescribedParameter {
                    spec: ControlParameter {
                        id: param.id,
                        min: param.min,
                        max: param.max,
                        value: self.instance.param_value(param.id).unwrap_or(param.default),
                        stepped: param.stepped,
                        read_only: param.read_only,
                    },
                    name: param.name.clone(),
                    automatable: param.automatable,
                })
                .collect();
            Ok((state, parameters, self.instance.layout().clone()))
        })();
        // A refused metadata/state request must retain a usable helper owner.
        self.activate()?;
        result
    }
    fn adopt_timeline(&mut self) -> io::Result<u64> {
        let epoch = self.region.timeline_epoch().map_err(error)?;
        if epoch < self.epoch {
            return Err(error("mapped timeline authority regressed"));
        }
        if epoch != self.epoch {
            self.epoch = epoch;
            self.sequence = None;
            self.notes_uncertain = true;
        }
        Ok(epoch)
    }
    /// This check admits an owner turn, not native controls or DSP proof. A
    /// subsequent host reset cannot revoke a native turn already admitted here.
    fn admit_input(&mut self, epoch: u64) -> io::Result<bool> {
        let current = self.adopt_timeline()?;
        if epoch == 0 || epoch > u64::from(u32::MAX) || epoch > current {
            return Err(error("invalid input timeline ownership"));
        }
        Ok(epoch == current)
    }
    fn check_capture_epoch(&mut self, epoch: u64) -> io::Result<()> {
        if self.adopt_timeline()? != epoch {
            return Err(error("capture timeline ownership changed"));
        }
        Ok(())
    }
    fn activate(&mut self) -> io::Result<()> {
        let mut processor = self
            .instance
            .activate_with_options(
                f64::from(self.region.config.sample_rate),
                self.region
                    .config
                    .block
                    .max(self.wrapper.fixed_block.unwrap_or(0)),
                &self.wrapper,
            )
            .map_err(error)?;
        processor.set_realtime(!self.offline);
        let latency = processor.latency_samples() as usize;
        if latency != self.region.config.native_latency {
            let _ = self.instance.deactivate(processor);
            return Err(error(
                "native latency changed; prepare a new bridge instance",
            ));
        }
        self.processor = Some(processor);
        Ok(())
    }
    fn deactivate(&mut self) -> io::Result<()> {
        if let Some(processor) = self.processor.take()
            && let Err(refused) = self.instance.deactivate(processor)
        {
            self.processor = Some(refused.returned);
            return Err(error(refused.error));
        }
        Ok(())
    }
    fn validate_parameter(&self, parameter: Parameter) -> io::Result<()> {
        let spec = self
            .parameters
            .iter()
            .find(|spec| spec.id == parameter.id)
            .ok_or_else(|| error("unknown native parameter ID"))?;
        if spec.read_only
            || !parameter.value.is_finite()
            || !(spec.min..=spec.max).contains(&parameter.value)
            || (spec.stepped && parameter.value != parameter.value.round())
        {
            return Err(error("invalid native parameter range/value"));
        }
        Ok(())
    }
    fn process(
        &mut self,
        sequence: u64,
        input: &InputBlock,
        output: &mut OutputBlock,
    ) -> io::Result<()> {
        let snapshot = &input.parameters[..input.parameter_count];
        if snapshot.len() != self.parameters.len()
            || snapshot
                .iter()
                .zip(self.parameters.iter())
                .any(|(value, spec)| value.id != spec.id)
        {
            return Err(error(
                "native parameter snapshot differs from negotiated table",
            ));
        }
        for parameter in snapshot {
            self.validate_parameter(*parameter)?;
        }
        for event in &input.events[..input.event_count] {
            if let HostEvent::Param { id, value, .. } = event {
                self.validate_parameter(Parameter {
                    id: *id,
                    value: *value,
                })?;
            }
        }
        let discontinuity = self.epoch != input.epoch
            || self
                .sequence
                .is_none_or(|previous| previous.checked_add(1) != Some(sequence));
        let needs_parameters = snapshot.iter().any(|value| {
            self.host_values.get(&value.id) != Some(&value.value)
                || self.uncertain.contains(&value.id)
        });
        if needs_parameters {
            self.deactivate()?;
            // Only changed or unproven host intent is reconciled. Equal old
            // host values must not overwrite a newer native preset/automation.
            for value in snapshot {
                if self.host_values.get(&value.id) != Some(&value.value)
                    || self.uncertain.contains(&value.id)
                {
                    if !self.instance.set_param(value.id, value.value) {
                        return Err(error("native inactive parameter admission failed"));
                    }
                    self.host_values.insert(value.id, value.value);
                    self.uncertain.remove(&value.id);
                }
            }
            self.activate()?;
        }
        let processor = self
            .processor
            .as_mut()
            .ok_or_else(|| error("native processor unavailable"))?;
        processor.set_transport(input.transport);
        processor.set_sidechain_input(input.sidechain_input);
        let mut admitted = true;
        output.native_drops = 0;
        if discontinuity || needs_parameters || self.notes_uncertain {
            processor.reset();
            for (key, velocity) in input.notes.iter().enumerate() {
                if *velocity > 0.0 {
                    let accepted = processor.push_recovery_event(HostEvent::NoteOn {
                        time: 0,
                        key: (key % 128) as u8,
                        channel: (key / 128) as u8,
                        velocity: *velocity,
                    });
                    admitted &= accepted;
                    if !accepted {
                        output.native_drops = output.native_drops.saturating_add(1);
                    }
                }
            }
        }
        if discontinuity || needs_parameters || self.notes_uncertain {
            let masks =
                std::array::from_fn(|channel| processor.note_expression_mask(channel as u8));
            for note in &input.instances[..input.instance_count] {
                if !processor.supports_note_instances() {
                    return Err(error("instance snapshot for key-only native plugin"));
                }
                let mut events = [HostEvent::AllNotesOff { time: 0 }; 6];
                let count = note.events(&masks, &mut events);
                for event in &events[..count] {
                    let accepted = processor.push_recovery_event(*event);
                    admitted &= accepted;
                    if !accepted {
                        output.native_drops = output.native_drops.saturating_add(1);
                    }
                }
            }
        }
        output.left.copy_from_slice(&input.left);
        output.right.copy_from_slice(&input.right);
        let mut at = 0;
        let mut event_at = 0;
        let mut no_drop = true;
        let mut good_audio = true;
        while at < self.region.config.block {
            while event_at < input.event_count && input.events[event_at].time() as usize == at {
                let mut event = input.events[event_at];
                event.set_time(0);
                match event {
                    HostEvent::NoteOff { key, channel, .. } => {
                        processor.release_note_on_channel(key, channel)
                    }
                    HostEvent::NoteOffInstance {
                        id,
                        key,
                        channel,
                        velocity,
                        ..
                    } => {
                        admitted &= processor.release_note_instance(id, key, channel, velocity);
                    }
                    HostEvent::AllNotesOff { .. } => processor.release_all_notes(),
                    _ => {
                        let accepted = processor.push_event(event);
                        admitted &= accepted;
                        if !accepted {
                            output.native_drops = output.native_drops.saturating_add(1);
                        }
                    }
                }
                if let HostEvent::Param { id, value, .. } = event {
                    self.host_values.insert(id, value);
                    self.uncertain.insert(id);
                }
                event_at += 1;
            }
            let end = if event_at < input.event_count {
                input.events[event_at].time() as usize
            } else {
                self.region.config.block
            };
            if end <= at {
                return Err(error("unordered bridge event times"));
            }
            // Admission failures remain in `admitted`; this baseline does not
            // erase them. A nonempty successful native call is mandatory.
            let before = processor.health();
            dropped_evidence(before.dropped_events, before.dropped_events)?;
            let status = processor.process_sidechain(
                &mut output.left[at..end],
                &mut output.right[at..end],
                Some(&input.key[at..end]),
            );
            let after = processor.health();
            output.native_drops = output
                .native_drops
                .saturating_add(after.dropped_events.saturating_sub(before.dropped_events));
            no_drop &= dropped_evidence(before.dropped_events, after.dropped_events)?;
            good_audio &= before.scrubbed_samples < u64::MAX - 2048
                && before.scrubbed_samples == after.scrubbed_samples;
            if status == ProcessStatus::Failed || after.failed {
                return Err(error("native plugin processing failed"));
            }
            at = end;
        }
        if processor.latency_samples() as usize != self.region.config.native_latency {
            return Err(error("native latency changed during bridge processing"));
        }
        output.epoch = input.epoch;
        output.processed_generation = 0;
        if admitted && no_drop && good_audio && input.controls_complete {
            self.processed_generation = input.control_end;
            self.processed_epoch = input.epoch;
            output.processed_generation = input.control_end;
            self.uncertain.clear();
            self.notes_uncertain = false;
        } else {
            // Admission may have lost a note without changing parameters or
            // sequence continuity. Recover the next whole held snapshot before
            // acknowledging its generation, including desired releases.
            self.notes_uncertain = true;
        }
        if self.adopt_timeline()? == input.epoch {
            self.sequence = Some(sequence);
        }
        if !good_audio {
            return Err(error("native plugin emitted nonfinite samples"));
        }
        Ok(())
    }
    fn values(&mut self) -> Vec<ControlParameter> {
        let parameters = self.parameters.clone();
        parameters
            .into_iter()
            .map(|mut spec| {
                spec.value = self.instance.param_value(spec.id).unwrap_or(spec.value);
                spec.into()
            })
            .collect()
    }
    fn capture(
        &mut self,
        epoch: u64,
        generation: u64,
        pending: &[ControlParameter],
    ) -> io::Result<(Vec<u8>, Vec<ControlParameter>, u64, u64)> {
        self.capture_with_checkpoint(epoch, generation, pending, || {})
    }
    fn capture_with_checkpoint(
        &mut self,
        epoch: u64,
        generation: u64,
        pending: &[ControlParameter],
        checkpoint: impl FnOnce(),
    ) -> io::Result<(Vec<u8>, Vec<ControlParameter>, u64, u64)> {
        self.check_capture_epoch(epoch)?;
        if generation == 0 {
            return Err(error("capture timeline ownership changed"));
        }
        let result = self.capture_current(epoch, generation, pending, checkpoint);
        // This check is after VST3 recovery, including native save refusal.
        // A request can never establish timeline ownership by itself.
        self.check_capture_epoch(epoch)?;
        result
    }
    fn capture_current(
        &mut self,
        epoch: u64,
        generation: u64,
        pending: &[ControlParameter],
        checkpoint: impl FnOnce(),
    ) -> io::Result<(Vec<u8>, Vec<ControlParameter>, u64, u64)> {
        // Sequence continuity can be cleared by capture; DSP proof remains
        // bound to the epoch of the last actual native block.
        let processed = if epoch == self.processed_epoch {
            self.processed_generation
        } else {
            0
        };
        if pending.len() != self.parameters.len()
            || pending.iter().zip(&self.parameters).any(|(value, spec)| {
                value.id != spec.id
                    || value.min != spec.min
                    || value.max != spec.max
                    || value.read_only != spec.read_only
                    || value.stepped != spec.stepped
            })
        {
            return Err(error(
                "capture parameter table differs from negotiated metadata",
            ));
        }
        for parameter in pending {
            self.validate_parameter(Parameter {
                id: parameter.id,
                value: parameter.value,
            })?;
        }
        // Capture runs on the same native owner between nonempty audio blocks.
        // It cannot call a live/speculative instance in the desktop process.
        if self.instance.descriptor().format == PluginFormat::Clap {
            // Accepted CLAP semantics keep the active instance and notes intact.
            // Unprocessed intent accompanies opaque state; it is not DSP readback.
            self.instance.idle(&mut |_| {});
            let state = captured_state(self.instance.save_state().map_err(error)?)?;
            let mut values = self.values();
            if generation > processed {
                for (value, intent) in values.iter_mut().zip(pending) {
                    if self.host_values.get(&intent.id) != Some(&intent.value)
                        || self.uncertain.contains(&intent.id)
                    {
                        value.value = intent.value;
                    }
                }
            }
            if values
                .iter()
                .any(|value| !ParameterSpec::from(*value).valid())
            {
                return Err(error("native capture parameter values are malformed"));
            }
            checkpoint();
            return Ok((state, values, processed, 0));
        }
        self.deactivate()?;
        let result = (|| {
            self.instance.idle(&mut |_| {});
            // A document edit can arrive at capture before the next callback
            // admits it and advances generation. Reconcile differing intent
            // independently of DSP proof; equal intent still preserves native
            // edits, including edits made by deactivation itself.
            let mut reconciled = generation > processed;
            for parameter in pending {
                let value = Parameter {
                    id: parameter.id,
                    value: parameter.value,
                };
                self.validate_parameter(value)?;
                if self.host_values.get(&value.id) != Some(&value.value)
                    || self.uncertain.contains(&value.id)
                {
                    // Preserve a genuinely newer deactivation preset.
                    if self.instance.deactivation_param_value(value.id).is_none()
                        && !self.instance.set_param(value.id, value.value)
                    {
                        return Err(error("capture parameter reconciliation refused"));
                    }
                    reconciled = true;
                    self.host_values.insert(value.id, value.value);
                    self.uncertain.remove(&value.id);
                }
            }
            let state = captured_state(self.instance.save_state().map_err(error)?)?;
            let values = self.values();
            if values
                .iter()
                .any(|value| !ParameterSpec::from(*value).valid())
            {
                return Err(error("native capture parameter values are malformed"));
            }
            Ok((
                state,
                values,
                processed,
                if reconciled { generation } else { 0 },
            ))
        })();
        // Recovery is mandatory even when capture fails. Refusal leaves the
        // exact processor owned here; supervisor can terminate this process.
        checkpoint();
        let recovery = self.activate();
        self.sequence = None;
        match (result, recovery) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
}

/// Recognize the helper mode before application startup. Uses current-exe
/// discovery in production once desktop wiring is transferred by the parent.
pub fn entry() -> bool {
    let mut args = std::env::args();
    let _ = args.next();
    if args.next().as_deref() != Some("--windfall-audio-helper") {
        return false;
    }
    let result = (|| {
        let address: SocketAddr = args
            .next()
            .ok_or_else(|| error("missing helper endpoint"))?
            .parse()
            .map_err(error)?;
        if !address.ip().is_loopback() || args.next().is_some() {
            return Err(error("invalid helper arguments"));
        }
        run(address)
    })();
    if result.is_err() {
        std::process::exit(70);
    }
    true
}

fn run(address: SocketAddr) -> io::Result<()> {
    let bootstrap = super::auth::Bootstrap::read(&mut std::io::stdin().lock())?;
    let mut socket = TcpStream::connect_timeout(&address, bootstrap.remaining()?)?;
    socket.set_nodelay(true)?;
    bootstrap.hello(&mut socket)?;
    socket.set_nonblocking(true)?;
    let mut decoder = Decoder::default();
    let deadline = bootstrap.deadline;
    // Private debug-only test bootstrap: actual native owner and launch path,
    // no extra public API/env/key/argv fields. Production filename never selects it.
    #[cfg(debug_assertions)]
    if std::env::current_exe()?
        .file_stem()
        .and_then(|name| name.to_str())
        == Some("windfall-plugin-audio-backpressure")
    {
        // Tell the private fixture test when authenticated Load bytes arrive.
        // Cancellation must exercise a blocked writer, not process scheduling.
        let mut byte = [0];
        loop {
            match socket.peek(&mut byte) {
                Ok(0) => return Err(error("backpressure fixture disconnected")),
                Ok(_) => break,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                Err(error) => return Err(error),
            }
            if Instant::now() >= deadline {
                return Err(error("backpressure fixture never received Load"));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        std::fs::write(
            std::env::temp_dir().join(format!(
                "windfall-backpressure-{:016x}.ready",
                bootstrap.session
            )),
            [],
        )?;
        std::thread::sleep(Duration::from_secs(3));
    }
    let load = loop {
        if Instant::now() >= deadline {
            return Err(error("helper load request timed out"));
        }
        let step = decoder.poll_step(&mut socket)?;
        if Instant::now() >= deadline {
            return Err(error("helper load request timed out"));
        }
        match step {
            ReadStep::Packet(packet) => break *packet,
            ReadStep::Progress => {}
            ReadStep::Idle => std::thread::sleep(Duration::from_millis(1)),
        }
    };
    if load.owner.session != bootstrap.session {
        return Err(error("helper session mismatch"));
    }
    drop(bootstrap); // Clear the launch key before any native module load.
    let Message::Load {
        wrapper,
        mapping,
        settings,
        path,
        id,
        format,
        approved_binary,
        parameters,
        offline,
    } = &load.body
    else {
        return Err(error("first helper request must load"));
    };
    if path.len() > 32_768
        || id.is_empty()
        || id.len() > 4096
        || path.contains('\0')
        || id.contains('\0')
        || parameters.len() > PARAM_CAPACITY
        || settings.owner != load.owner
    {
        return Err(error("invalid helper binding"));
    }
    let config = settings.config().map_err(error)?;
    wrapper
        .validate(config.block.max(wrapper.fixed_block.unwrap_or(0)))
        .map_err(error)?;
    if wrapper.processing != crate::wrapper::ProcessingMode::InProcess {
        return Err(error(
            "helper DSP policy must use its already selected owner",
        ));
    }
    if config.native_latency != 0 {
        return Err(error("native latency must be negotiated after load"));
    }
    // Platform + exact mapping/identity/layout checks precede native loading.
    let storage: Arc<dyn super::slots::WordStorage> = Mapping::open(mapping)?;
    let mut region = Region::attach(storage, config).map_err(error)?;
    let declared = match format.as_str() {
        "clap" => PluginFormat::Clap,
        "vst3" => PluginFormat::Vst3,
        _ => return Err(error("unsupported bridge plugin format")),
    };
    if PluginFormat::of(Path::new(path)) != Some(declared) {
        return Err(error("declared plugin format differs from binding path"));
    }
    if crate::paths::plugin_file_identity(Path::new(path))? != *approved_binary {
        return Err(error(
            "approved native plugin binary changed before helper load",
        ));
    }
    let host = PluginHost::windfall();
    let module = host.load(Path::new(path)).map_err(error)?;
    let mut instance = module.create(id).map_err(error)?;
    if crate::paths::plugin_file_identity(Path::new(path))? != *approved_binary {
        return Err(error(
            "approved native plugin binary changed during helper load",
        ));
    }
    let kind = if config.kind == Kind::Instrument {
        PluginKind::Instrument
    } else {
        PluginKind::Effect
    };
    if instance.descriptor().format != declared || instance.descriptor().kind != kind {
        return Err(error("native plugin binding role/format mismatch"));
    }
    if !load.state.is_empty() {
        instance
            .load_state(&PluginState::from_bytes(load.state.clone()))
            .map_err(error)?;
    }
    let mut specs = Vec::new();
    for parameter in parameters {
        let spec = ParameterSpec::from(*parameter);
        let native = instance
            .param(spec.id)
            .ok_or_else(|| error("unknown load parameter"))?;
        if !spec.valid()
            || native.read_only
            || native.min != spec.min
            || native.max != spec.max
            || native.stepped != spec.stepped
            || spec.read_only
        {
            return Err(error("native parameter metadata changed"));
        }
        if specs
            .iter()
            .any(|existing: &ParameterSpec| existing.id == spec.id)
        {
            return Err(error("duplicate native parameter ID"));
        }
        if !instance.set_param(spec.id, spec.value) {
            return Err(error("native initial parameter admission refused"));
        }
        specs.push(spec);
    }
    let mut processor = instance
        .activate_with_options(
            f64::from(config.sample_rate),
            config.block.max(wrapper.fixed_block.unwrap_or(0)),
            wrapper,
        )
        .map_err(error)?;
    processor.set_realtime(!offline);
    let latency = processor.latency_samples() as usize;
    if let Err(error_value) = region.negotiate_latency(latency) {
        region.latch_helper_failure();
        let message = format!(
            "unsupported native latency {latency} at {} Hz: {error_value}",
            config.sample_rate
        );
        reply(
            &mut socket,
            &Packet::new(
                load.request,
                load.owner,
                Message::Error {
                    message: message.clone(),
                },
            ),
        )?;
        let _ = instance.deactivate(processor);
        return Err(error(message));
    }
    let tail = u64::from(processor.tail_samples().unwrap_or(config.sample_rate * 10));
    let note_instances = processor.supports_note_instances();
    let expression_masks =
        std::array::from_fn(|channel| processor.note_expression_mask(channel as u8));
    let mut native = Native {
        wrapper: (**wrapper).clone(),
        instance,
        processor: Some(processor),
        region,
        parameters: specs,
        host_values: parameters
            .iter()
            .map(|value| (value.id, value.value))
            .collect(),
        uncertain: BTreeSet::new(),
        notes_uncertain: false,
        sequence: None,
        epoch: 1,
        processed_generation: 0,
        processed_epoch: 0,
        offline: *offline,
        dirty: false,
    };
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let mut editor_refusals = crate::gui::bridge_editor::Refusals::default();
    let ready = Message::Ready {
        note_instances,
        expression_masks,
        native_latency: latency as u32,
        tail,
        parameters: parameters.clone(),
    };
    reply(&mut socket, &Packet::new(load.request, load.owner, ready))?;
    let mut input = InputBlock::new();
    let mut output = OutputBlock::silent();
    loop {
        native.adopt_timeline()?;
        if let Some(packet) = decoder.poll(&mut socket)? {
            if packet.owner != load.owner {
                return Err(error("stale helper control owner"));
            }
            let mut response = Packet::new(packet.request, packet.owner, Message::Stopped);
            match packet.body {
                Message::Describe => match native.describe() {
                    Ok((state, parameters, layout)) => {
                        response.state = state;
                        response.body = Message::Described { parameters, layout };
                        if let Err(error_value) = response.check_encoding() {
                            response.state.clear();
                            response.body = Message::Error {
                                message: error_value.to_string(),
                            };
                        }
                    }
                    Err(error_value) => {
                        response.body = Message::Error {
                            message: error_value.to_string(),
                        };
                    }
                },
                Message::Shutdown => {
                    native.deactivate()?;
                    reply(&mut socket, &response)?;
                    return Ok(());
                }
                Message::Editor { open, options } => {
                    #[cfg(any(target_os = "macos", target_os = "linux"))]
                    let options = crate::gui::bridge_editor::helper_options(
                        options,
                        cfg!(target_os = "macos"),
                    );
                    if native.offline {
                        response.body = Message::Error {
                            message: "render helpers cannot open editors".into(),
                        };
                    } else if open {
                        if let Err(error) = native.instance.open_editor(&options) {
                            response.body = Message::Error {
                                message: error.to_string(),
                            };
                        }
                    } else {
                        native.instance.close_editor();
                        #[cfg(any(target_os = "macos", target_os = "linux"))]
                        editor_refusals.clear();
                    }
                }
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                Message::EditorScale { application } => {
                    if native.instance.editor().is_some() {
                        match native.instance.set_editor_scale(application) {
                            Ok(_) => editor_refusals.clear(),
                            Err(error) => {
                                response.body = Message::Error {
                                    message: error.to_string(),
                                }
                            }
                        }
                    }
                }
                #[cfg(any(target_os = "macos", target_os = "linux"))]
                Message::EditorStatus => {
                    response.body = Message::EditorReport {
                        open: native.instance.editor().is_some(),
                        refused: editor_refusals.take(),
                    };
                }
                Message::Capture {
                    epoch,
                    desired_generation,
                    pending,
                } => match native.capture(epoch, desired_generation, &pending) {
                    Ok((state, parameters, processed_generation, reconciled_generation)) => {
                        response.state = state;
                        response.body = Message::Captured {
                            epoch,
                            processed_generation,
                            reconciled_generation,
                            parameters,
                        };
                    }
                    Err(error_value) => {
                        response.body = Message::Error {
                            message: error_value.to_string(),
                        }
                    }
                },
                _ => return Err(error("unsupported helper control request")),
            }
            reply(&mut socket, &response)?;
            native.adopt_timeline()?;
        }
        let mut worked = false;
        if let Some((slot, sequence)) = native.region.take_input(&mut input).map_err(error)? {
            worked = true;
            let result = native.admit_input(input.epoch).and_then(|admitted| {
                if admitted {
                    native.process(sequence, &input, &mut output).map(|()| true)
                } else {
                    Ok(false)
                }
            });
            match result {
                Ok(false) => native.region.retire_input(slot).map_err(error)?,
                Ok(true) => native.region.complete(slot, sequence, &output, OUTPUT_OK),
                Err(error_value) => {
                    native.region.latch_helper_failure();
                    native
                        .region
                        .complete(slot, sequence, &output, OUTPUT_FAILED);
                    return Err(error_value);
                }
            }
        }
        native.adopt_timeline()?;
        crate::gui::pump_events(Some(Duration::ZERO));
        let mut changed = false;
        native.instance.idle(&mut |notification| {
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            match &notification {
                PluginNotification::EditorScaleRefused {
                    application,
                    monitor,
                } => editor_refusals.record(*application, *monitor),
                PluginNotification::EditorClosed => editor_refusals.clear(),
                _ => {}
            }
            if matches!(
                notification,
                PluginNotification::ParamChanged { .. }
                    | PluginNotification::StateChanged
                    | PluginNotification::RestartRequested
                    | PluginNotification::ParamsRescanned
            ) {
                native.dirty = true;
                changed = true;
            }
        });
        native.adopt_timeline()?;
        if changed {
            native.region.publish_native_change().map_err(error)?;
        }
        native.region.owner_completed().map_err(error)?;
        if !worked {
            std::thread::sleep(Duration::from_micros(200));
        }
    }
}

// A delta is conservative unknown; saturation/rollover ends this helper rather
// than letting a later wrapped counter manufacture an affirmative no-drop gate.
fn dropped_evidence(before: u32, after: u32) -> io::Result<bool> {
    if before >= u32::MAX - 65_536 || after >= u32::MAX - 65_536 || after < before {
        return Err(error("native drop counter exhausted or rolled over"));
    }
    Ok(before == after)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exhausted_or_wrapped_native_loss_counters_never_become_affirmative_proof() {
        assert!(dropped_evidence(100, 100).unwrap());
        assert!(!dropped_evidence(100, 101).unwrap());
        assert!(dropped_evidence(u32::MAX - 65_536, u32::MAX - 65_536).is_err());
        assert!(dropped_evidence(u32::MAX - 65_537, u32::MAX - 65_536).is_err());
        assert!(dropped_evidence(100, 0).is_err());
    }

    fn with_native(
        format: &str,
        test: impl FnOnce(&mut Native, &mut super::super::adapter::Audio, ParameterSpec),
    ) {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let folder = std::env::temp_dir().join(format!(
            "windfall-reset-owner-turn-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(format!("fixture.{format}"));
        // Like the explicit native sticky-writer unit, these owner-turn tests
        // require a separately built fixture and never start nested Cargo.
        let source =
            std::env::var_os("WINDFALL_BRIDGE_FIXTURE").expect("explicit native fixture path");
        std::fs::copy(source, &path).unwrap();
        let host = PluginHost::windfall();
        let module = host.load(&path).unwrap();
        let mut instance = module
            .create(if format == "clap" {
                "org.windfall.test.gain"
            } else {
                "00000000000000000000000000000001"
            })
            .unwrap();
        assert!(instance.set_param(7, 0.5));
        let mut processor = instance.activate(48_000.0, 64).unwrap();
        processor.set_realtime(false);
        let region = Region::initialize(
            super::super::slots::LocalWords::new(),
            Config {
                identity: Identity {
                    session: 1,
                    token: 2,
                    revision: 3,
                    binding: 4,
                },
                sample_rate: 48_000,
                block: 64,
                native_latency: processor.latency_samples() as usize,
                kind: Kind::Effect,
            },
        )
        .unwrap();
        let spec = ParameterSpec {
            id: 7,
            min: 0.0,
            max: if format == "clap" { 2.0 } else { 1.0 },
            value: 0.5,
            read_only: false,
            stepped: false,
        };
        let mut native = Native {
            wrapper: Default::default(),
            processor: Some(processor),
            instance,
            region: region.clone(),
            parameters: vec![spec],
            host_values: BTreeMap::from([(7, 0.5)]),
            uncertain: BTreeSet::new(),
            notes_uncertain: false,
            sequence: None,
            epoch: 1,
            processed_generation: 0,
            processed_epoch: 0,
            offline: true,
            dirty: false,
        };
        let mut audio = super::super::adapter::Audio::new(
            region.clone(),
            Arc::new(super::super::adapter::Signals::default()),
            &[spec],
        )
        .unwrap();
        test(&mut native, &mut audio, spec);
        drop(audio);
        drop(native);
        drop(module);
        drop(host);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(folder).unwrap();
    }
    #[test]
    #[ignore = "requires explicitly built WINDFALL_BRIDGE_FIXTURE; runs no nested Cargo"]
    fn reset_capture_accepts_current_authority_after_an_old_native_owner_turn() {
        with_native("vst3", |native, audio, spec| {
            let region = native.region.clone();
            let mut input = InputBlock::new();
            let mut output = OutputBlock::silent();
            for _ in 0..3 {
                audio.process(&mut [1.0; 64], &mut [1.0; 64]);
                let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
                assert!(native.admit_input(input.epoch).unwrap());
                native.process(sequence, &input, &mut output).unwrap();
                region.complete(slot, sequence, &output, OUTPUT_OK);
            }
            assert_eq!(audio.health().acknowledged_generation, 1);
            audio.process(&mut [1.0; 64], &mut [1.0; 64]);
            assert_eq!(native.capture(1, 1, &[spec.into()]).unwrap().2, 1);
            let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
            assert!(native.admit_input(input.epoch).unwrap());
            // This owner turn is admitted before reset, then really calls native
            // processing after it. No scheduler sleeps or fixture changes are used.
            audio.reset_timeline();
            native.process(sequence, &input, &mut output).unwrap();
            region.complete(slot, sequence, &output, OUTPUT_OK);
            assert_eq!(native.processed_epoch, 1);
            let second = native.capture(2, 1, &[spec.into()]).unwrap();
            assert_eq!(second.2, 0);
            assert_eq!(audio.health().acknowledged_generation, 0);
            assert_eq!(native.epoch, 2);
            assert_eq!(native.sequence, None);
            assert_eq!(region.timeline_epoch(), Ok(2));
            assert_eq!(region.owner_completions(), 0);
        });
    }
    #[test]
    #[ignore = "requires explicitly built WINDFALL_BRIDGE_FIXTURE; runs no nested Cargo"]
    fn repeated_resets_fence_old_turns_and_stale_capture_without_new_dsp() {
        for format in ["clap", "vst3"] {
            with_native(format, |native, audio, spec| {
                let region = native.region.clone();
                let mut input = InputBlock::new();
                let mut output = OutputBlock::silent();
                audio.process(&mut [1.0; 64], &mut [1.0; 64]);
                let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
                assert!(native.admit_input(input.epoch).unwrap());
                audio.reset_timeline();
                audio.reset_timeline();
                native.process(sequence, &input, &mut output).unwrap();
                region.complete(slot, sequence, &output, OUTPUT_OK);
                assert_eq!(output.epoch, 1);
                assert_eq!(native.processed_epoch, 1);
                assert_eq!(native.epoch, 3);
                for epoch in [0, 1, 2, 4, u64::MAX] {
                    assert!(native.capture(epoch, 1, &[spec.into()]).is_err());
                }
                assert_eq!(native.capture(3, 1, &[spec.into()]).unwrap().2, 0);
                assert_eq!(region.timeline_epoch(), Ok(3));
                assert_eq!(region.owner_completions(), 0);
                for _ in 0..4 {
                    audio.process(&mut [0.0; 64], &mut [0.0; 64]);
                }
                assert_eq!(audio.health().acknowledged_generation, 0);
                assert_eq!(audio.completed_proof(), None);
                assert_eq!(audio.desired_generation(), 1);
            });
        }
    }
    #[test]
    #[ignore = "requires explicitly built WINDFALL_BRIDGE_FIXTURE; runs no nested Cargo"]
    fn stale_claim_retirement_is_not_native_processing_or_liveness() {
        for format in ["clap", "vst3"] {
            with_native(format, |native, audio, spec| {
                let region = native.region.clone();
                audio.process(&mut [1.0; 64], &mut [1.0; 64]);
                let mut input = InputBlock::new();
                let (slot, sequence) = region.take_input(&mut input).unwrap().unwrap();
                audio.reset_timeline(); // Claimed, but not yet owner-turn admitted.
                assert!(!native.admit_input(input.epoch).unwrap());
                region.retire_input(slot).unwrap();
                assert_eq!(region.busy_sequence(), None);
                assert!(!region.output_finished(sequence));
                assert_eq!(region.owner_completions(), 0);
                assert_eq!(native.processed_epoch, 0);
                assert_eq!(native.processed_generation, 0);
                assert_eq!(native.capture(2, 1, &[spec.into()]).unwrap().2, 0);
                assert_eq!(audio.desired_generation(), 1);
                assert!(!native.region.helper_failed());
            });
        }
    }
    #[test]
    #[ignore = "requires explicitly built WINDFALL_BRIDGE_FIXTURE; runs no nested Cargo"]
    fn reset_during_native_capture_recovers_then_refuses_and_later_processing_works() {
        for format in ["clap", "vst3"] {
            with_native(format, |native, audio, spec| {
                let result =
                    native.capture_with_checkpoint(1, 1, &[spec.into()], || audio.reset_timeline());
                assert_eq!(
                    result.unwrap_err().to_string(),
                    "capture timeline ownership changed"
                );
                assert!(native.processor.is_some(), "{format} recovery was skipped");
                assert_eq!(native.epoch, 2);
                assert_eq!(native.processed_epoch, 0);
                assert_eq!(native.capture(2, 1, &[spec.into()]).unwrap().2, 0);
                let mut input = InputBlock::new();
                let mut output = OutputBlock::silent();
                audio.process(&mut [1.0; 64], &mut [1.0; 64]);
                let (slot, sequence) = native.region.take_input(&mut input).unwrap().unwrap();
                assert!(native.admit_input(input.epoch).unwrap());
                input.controls_complete = false;
                native.process(sequence, &input, &mut output).unwrap();
                native.region.complete(slot, sequence, &output, OUTPUT_OK);
                assert_eq!(native.capture(2, 1, &[spec.into()]).unwrap().2, 0);
                audio.process(&mut [1.0; 64], &mut [1.0; 64]);
                let (slot, sequence) = native.region.take_input(&mut input).unwrap().unwrap();
                assert!(native.admit_input(input.epoch).unwrap());
                native.process(sequence, &input, &mut output).unwrap();
                native.region.complete(slot, sequence, &output, OUTPUT_OK);
                assert_eq!(native.capture(2, 1, &[spec.into()]).unwrap().2, 1);
                assert_eq!(audio.health().acknowledged_generation, 0); // Not collected yet.
            });
        }
    }
    #[test]
    #[ignore = "requires explicitly built WINDFALL_BRIDGE_FIXTURE; runs no nested Cargo"]
    fn native_authority_regression_and_future_inputs_fail_closed() {
        with_native("vst3", |native, audio, _| {
            assert!(native.admit_input(0).is_err());
            assert!(native.admit_input(2).is_err());
            assert!(native.admit_input(u64::MAX).is_err());
            native.epoch = 2; // Last observation was newer than the corrupted header.
            assert!(native.adopt_timeline().is_err());
            assert_eq!(native.processed_generation, 0);
            assert_eq!(audio.health().acknowledged_generation, 0);
        });
    }
}
