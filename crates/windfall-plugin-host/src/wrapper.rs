//! Real DSP wrapper policies. Prepared off audio; unsupported modes are refused.
use crate::processor::{AudioIo, BlockResult, ProcessFailed, ProcessStatus, ProcessorBackend};
use crate::{HostEvent, PluginError, PluginEvent, Transport};
use serde::{Deserialize, Serialize};
use std::any::Any;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProcessingMode {
    #[default]
    InProcess,
    Threaded,
    Bridged,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct WrapperOptions {
    pub smart_disable: bool,
    /// None forwards variable blocks. Some(B) collects exactly B native frames,
    /// adding B samples to reported latency and preserving timed event offsets.
    pub fixed_block: Option<usize>,
    pub processing: ProcessingMode,
    pub editor: crate::EditorOptions,
}
impl Default for WrapperOptions {
    fn default() -> Self {
        Self {
            smart_disable: false,
            fixed_block: None,
            processing: ProcessingMode::InProcess,
            editor: crate::EditorOptions::default(),
        }
    }
}
impl WrapperOptions {
    pub fn validate(&self, max_block: usize) -> Result<(), PluginError> {
        if self
            .fixed_block
            .is_some_and(|block| block == 0 || block > max_block || block > 8192)
        {
            return Err(PluginError::Activate(
                "fixed buffer exceeds prepared native block".into(),
            ));
        }
        self.editor
            .validate()
            .map_err(|e| PluginError::Activate(e.to_string()))?;
        Ok(())
    }
}
const CAPACITY: usize = crate::processor::BLOCK_EVENT_CAPACITY;
struct Fixed {
    auxiliary: crate::outputs::OutputBank,
    input: [Box<[f32]>; 2],
    output: [Box<[f32]>; 2],
    key: Box<[[f32; 2]]>,
    events: Vec<HostEvent>,
    cursor: usize,
    transport: Transport,
    steady: u64,
}
impl Fixed {
    fn new(block: usize) -> Self {
        Self {
            auxiliary: crate::outputs::OutputBank::new(block),
            input: [
                vec![0.0; block].into_boxed_slice(),
                vec![0.0; block].into_boxed_slice(),
            ],
            output: [
                vec![0.0; block].into_boxed_slice(),
                vec![0.0; block].into_boxed_slice(),
            ],
            key: vec![[0.0; 2]; block].into_boxed_slice(),
            events: Vec::with_capacity(CAPACITY),
            cursor: 0,
            transport: Transport::default(),
            steady: 0,
        }
    }
}
struct Policy {
    auxiliary_check: Box<[[f32; 2]]>,
    rendered: crate::outputs::OutputBank,
    native: Box<dyn ProcessorBackend>,
    fixed: Option<Fixed>,
    smart: bool,
    asleep: bool,
    quiet: usize,
    tail: usize,
    rate: f64,
}
pub(crate) fn wrap(
    native: Box<dyn ProcessorBackend>,
    options: &WrapperOptions,
    rate: f64,
    tail: u32,
    max_block: usize,
) -> Box<dyn ProcessorBackend> {
    if !options.smart_disable && options.fixed_block.is_none() {
        return native;
    }
    Box::new(Policy {
        auxiliary_check: vec![[0.0; 2]; max_block].into_boxed_slice(),
        rendered: crate::outputs::OutputBank::new(max_block),
        native,
        fixed: options.fixed_block.map(Fixed::new),
        smart: options.smart_disable,
        asleep: false,
        quiet: 0,
        tail: tail as usize,
        rate,
    })
}
fn quiet(samples: &[f32]) -> bool {
    samples.iter().all(|v| v.is_finite() && v.abs() <= 1e-9)
}
impl Policy {
    fn native_process(
        &mut self,
        audio: (&mut [f32], &mut [f32], &mut [f32], &mut [f32]),
        key: Option<&[[f32; 2]]>,
        events: &[HostEvent],
        transport: &Transport,
        steady: u64,
        out: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        let (il, ir, ol, or) = audio;
        let idle = events.is_empty()
            && quiet(il)
            && quiet(ir)
            && key.is_none_or(|key| {
                key.iter()
                    .all(|frame| frame.iter().all(|v| v.is_finite() && v.abs() <= 1e-9))
            });
        if self.asleep && idle {
            ol.fill(0.0);
            or.fill(0.0);
            return Ok(BlockResult {
                status: ProcessStatus::Sleep,
                tail: Some(self.tail as u32),
                dropped_events: 0,
            });
        }
        if !idle {
            self.asleep = false;
            self.quiet = 0;
        }
        let result = self.native.process_sidechain(
            AudioIo::Separate {
                in_left: il,
                in_right: ir,
                out_left: ol,
                out_right: or,
            },
            key,
            events,
            transport,
            steady,
            out,
        )?;
        if let Some(tail) = result.tail {
            self.tail = tail as usize;
        }
        let mut auxiliary_quiet = true;
        if self.smart {
            for bus in 0..crate::MAX_PORTS {
                let output = &mut self.auxiliary_check[..ol.len()];
                self.native.copy_output(bus, output);
                auxiliary_quiet &= output
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite() && value.abs() <= 1e-9);
            }
        }
        if self.smart
            && idle
            && quiet(ol)
            && quiet(or)
            && auxiliary_quiet
            && self.tail != u32::MAX as usize
        {
            self.quiet = self.quiet.saturating_add(ol.len());
            // Finite tails plus one second of observed quiet; output readings
            // never authorize skipping a plugin that declares an endless tail.
            if self.quiet >= self.tail.saturating_add(self.rate as usize) {
                self.native.stop();
                self.asleep = true;
            }
        } else {
            self.quiet = 0;
        }
        Ok(result)
    }
}
impl ProcessorBackend for Policy {
    fn copy_output(&self, bus: u32, output: &mut [[f32; 2]]) -> bool {
        if self.fixed.is_some() {
            self.rendered.copy(bus, output)
        } else if self.asleep {
            output.fill([0.0; 2]);
            false
        } else {
            self.native.copy_output(bus, output)
        }
    }
    fn controls_processed(&self) -> bool {
        self.fixed.as_ref().is_none_or(|fixed| {
            !fixed
                .events
                .iter()
                .any(|event| matches!(event, HostEvent::Param { .. }))
        }) && self.native.controls_processed()
    }
    fn set_realtime(&mut self, realtime: bool) {
        self.native.set_realtime(realtime);
    }
    fn return_to_owner(&mut self) {
        self.native.return_to_owner();
    }
    fn resume_after_refusal(&mut self) {
        self.native.resume_after_refusal();
    }
    fn supports_midi_expression(&self) -> bool {
        self.native.supports_midi_expression()
    }
    fn midi_expression_mask(&self, channel: u8) -> u32 {
        self.native.midi_expression_mask(channel)
    }
    fn supports_note_instances(&self) -> bool {
        self.native.supports_note_instances()
    }
    fn note_expression_mask(&self, channel: u8) -> u32 {
        self.native.note_expression_mask(channel)
    }
    fn additional_latency(&self) -> u32 {
        self.fixed.as_ref().map_or(0, |f| f.input[0].len() as u32)
    }
    fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.native.set_sidechain_input(input);
    }
    fn process(
        &mut self,
        audio: AudioIo<'_>,
        events: &[HostEvent],
        transport: &Transport,
        steady: u64,
        out: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        self.process_sidechain(audio, None, events, transport, steady, out)
    }
    fn process_sidechain(
        &mut self,
        audio: AudioIo<'_>,
        key: Option<&[[f32; 2]]>,
        events: &[HostEvent],
        transport: &Transport,
        steady: u64,
        out: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        // Policy activation forces separate buffers. The outer processor owns
        // copies, including for a native plugin that permits in-place processing.
        let AudioIo::Separate {
            in_left: il,
            in_right: ir,
            out_left: ol,
            out_right: or,
        } = audio
        else {
            return Err(ProcessFailed);
        };
        let Some(mut fixed) = self.fixed.take() else {
            return self.native_process((il, ir, ol, or), key, events, transport, steady, out);
        };
        // Moving a preallocated option does not allocate or free. Always return
        // it, including native failure, before propagating the result.
        let result = (|| {
            let mut event_at = 0;
            let mut drops: u32 = 0;
            let mut status = ProcessStatus::Continue;
            for frame in 0..ol.len() {
                let cursor = fixed.cursor;
                if cursor == 0 {
                    fixed.transport = *transport;
                    fixed.transport.advance(frame, self.rate);
                    fixed.steady = steady.saturating_add(frame as u64);
                }
                while event_at < events.len() && events[event_at].time() as usize == frame {
                    let mut event = events[event_at];
                    event.set_time(cursor as u32);
                    if fixed.events.len() < fixed.events.capacity() {
                        fixed.events.push(event);
                    } else {
                        drops = drops.saturating_add(1);
                    }
                    event_at += 1;
                }
                ol[frame] = fixed.output[0][cursor];
                or[frame] = fixed.output[1][cursor];
                for (rendered, bus) in self.rendered.buses.iter_mut().zip(&fixed.auxiliary.buses) {
                    rendered[frame] = bus[cursor];
                }
                fixed.input[0][cursor] = il[frame];
                fixed.input[1][cursor] = ir[frame];
                fixed.key[cursor] = key
                    .and_then(|key| key.get(frame))
                    .copied()
                    .unwrap_or([0.0; 2]);
                fixed.cursor += 1;
                if fixed.cursor == fixed.input[0].len() {
                    let [in_left, in_right] = &mut fixed.input;
                    let [out_left, out_right] = &mut fixed.output;
                    let result = self.native_process(
                        (in_left, in_right, out_left, out_right),
                        Some(&fixed.key),
                        &fixed.events,
                        &fixed.transport,
                        fixed.steady,
                        out,
                    )?;
                    for (bus, output) in fixed.auxiliary.buses.iter_mut().enumerate() {
                        if self.asleep {
                            output.fill([0.0; 2]);
                        } else {
                            self.native.copy_output(bus as u32, output);
                        }
                    }
                    drops = drops.saturating_add(result.dropped_events);
                    status = result.status;
                    fixed.events.clear();
                    fixed.cursor = 0;
                }
            }
            Ok(BlockResult {
                status,
                tail: Some(self.tail as u32),
                dropped_events: drops,
            })
        })();
        self.fixed = Some(fixed);
        result
    }
    fn retire_params(&mut self, params: &mut Vec<HostEvent>) {
        if let Some(fixed) = &mut self.fixed {
            params.extend(
                fixed
                    .events
                    .iter()
                    .copied()
                    .filter(|event| matches!(event, HostEvent::Param { .. })),
            );
        }
        self.native.retire_params(params);
    }
    fn failure_dropped_events(&self) -> u32 {
        self.native.failure_dropped_events()
    }
    fn reset(&mut self) {
        self.native.reset();
        self.rendered.clear();
        self.asleep = false;
        self.quiet = 0;
        if let Some(f) = &mut self.fixed {
            f.auxiliary.clear();
            for buffer in f.input.iter_mut().chain(f.output.iter_mut()) {
                buffer.fill(0.0);
            }
            f.key.fill([0.0; 2]);
            f.events.clear();
            f.cursor = 0;
        }
    }
    fn stop(&mut self) {
        self.native.stop();
    }
    // Return the original native processor to its existing owner adapter.
    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        let Self { native, .. } = *self;
        native.into_any()
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self.native.as_any_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Probe {
        calls: Arc<AtomicUsize>,
        stops: Arc<AtomicUsize>,
    }
    impl ProcessorBackend for Probe {
        fn process(
            &mut self,
            audio: AudioIo<'_>,
            _: &[HostEvent],
            _: &Transport,
            _: u64,
            _: &mut dyn FnMut(PluginEvent),
        ) -> Result<BlockResult, ProcessFailed> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            if let AudioIo::Separate {
                in_left,
                in_right,
                out_left,
                out_right,
            } = audio
            {
                out_left.copy_from_slice(in_left);
                out_right.copy_from_slice(in_right);
            }
            Ok(BlockResult {
                status: ProcessStatus::Continue,
                tail: Some(0),
                dropped_events: 0,
            })
        }
        fn reset(&mut self) {}
        fn stop(&mut self) {
            self.stops.fetch_add(1, Ordering::Relaxed);
        }
        fn into_any(self: Box<Self>) -> Box<dyn Any> {
            self
        }
        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }
    #[test]
    fn smart_disable_stops_only_after_observed_quiet_and_wakes_on_input_or_control() {
        let calls = Arc::new(AtomicUsize::new(0));
        let stops = Arc::new(AtomicUsize::new(0));
        let mut unit = wrap(
            Box::new(Probe {
                calls: calls.clone(),
                stops: stops.clone(),
            }),
            &WrapperOptions {
                smart_disable: true,
                ..Default::default()
            },
            8.0,
            0,
            4,
        );
        let mut run = |input: f32, events: &[HostEvent]| {
            let mut il = [input; 4];
            let mut ir = il;
            let mut ol = [0.0; 4];
            let mut or = ol;
            unit.process(
                AudioIo::Separate {
                    in_left: &mut il,
                    in_right: &mut ir,
                    out_left: &mut ol,
                    out_right: &mut or,
                },
                events,
                &Transport::default(),
                0,
                &mut |_| {},
            )
            .ok()
            .expect("probe processing should succeed");
            ol
        };
        run(0.0, &[]);
        run(0.0, &[]);
        run(0.0, &[]);
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        assert_eq!(stops.load(Ordering::Relaxed), 1);
        assert_eq!(run(0.25, &[]), [0.25; 4]);
        assert_eq!(calls.load(Ordering::Relaxed), 3);
        run(0.0, &[]);
        run(0.0, &[]);
        run(
            0.0,
            &[HostEvent::Param {
                time: 0,
                id: 7,
                value: 0.5,
            }],
        );
        assert_eq!(calls.load(Ordering::Relaxed), 6);
    }
}
