//! Running one block of a CLAP plugin.
//!
//! CLAP hands a plugin an array of buffers per port, each an array of
//! channel pointers. The host feeds one stereo signal through the plugin's
//! main ports and has to give every other port something too: a plugin
//! with a sidechain reads its second input whether the host has one or
//! not. So at activation this module builds a complete set of buffers for
//! every port the plugin declared, pointing at memory it owns. Per block it
//! changes the main pointers to the caller's audio and copies the detector
//! signal into the selected auxiliary input. Other auxiliary inputs stay silent.
//!
//! The raw pointers are why this file has `unsafe` in it. Clack's safe
//! buffer builder cannot express the same memory used as input and output,
//! which is what in-place processing is.

use std::any::Any;
use std::ptr;

use clack_extensions::tail::{PluginTail, TailLength};
use clack_host::events::EventFlags;
use clack_host::events::EventHeader;
use clack_host::events::event_types::{TransportEvent, TransportFlags};
use clack_host::prelude::*;
use clack_host::process::ProcessStatus as ClapStatus;
use clack_host::utils::{BeatTime, SecondsTime};
use clap_sys::audio_buffer::clap_audio_buffer;

use super::events::{Dialect, EventList, Outgoing};
use super::handlers::{AudioCall, WindfallHost, flag};
use crate::events::{HostEvent, PluginEvent, Transport};
use crate::processor::{
    AudioIo, BlockResult, ENDLESS_TAIL, ProcessFailed, ProcessStatus, ProcessorBackend,
};

/// One audio port as the plugin declared it.
#[derive(Debug, Clone)]
pub(crate) struct PortDesc {
    pub id: u32,
    pub name: String,
    pub channels: u32,
    /// The port the track's sound goes through.
    pub main: bool,
    /// The id of the port on the other side that may share this one's
    /// memory.
    pub in_place_pair: Option<u32>,
}

/// Where the main port's channel pointers are, and how the host's two
/// channels map onto them.
struct MainPort {
    /// Index of the port's first pointer in `PortSet::pointers`.
    first: usize,
    /// The buffer that stands in for a one-channel port, as an index into
    /// `PortSet::owned`. A port with two or more channels takes the
    /// caller's buffers directly.
    mono: Option<usize>,
}

/// The buffers of every input port, or of every output port.
struct PortSet {
    buffers: Vec<clap_audio_buffer>,
    /// The channel pointers of all ports, one port after the other. The
    /// `data32` of each buffer points into this.
    pointers: Vec<*mut f32>,
    /// The memory behind every pointer that is not the caller's.
    owned: Vec<Box<[f32]>>,
    main: Option<MainPort>,
    key_port: Option<usize>,
    key_owned: Vec<Vec<usize>>,
}

impl PortSet {
    /// Builds buffers of `frames` frames for `ports`.
    ///
    /// Auxiliary inputs own their first two channel buffers for key audio.
    /// Remaining input channels share silence. Output ports other than the main one each get memory
    /// of their own that nobody reads.
    fn new(ports: &[PortDesc], frames: usize, is_input: bool) -> Self {
        let buffer = || vec![0.0_f32; frames].into_boxed_slice();
        let mut owned = vec![buffer()];
        let mut pointers = Vec::new();
        let mut firsts = Vec::new();
        let mut main = None;
        let key_port = if is_input {
            ports
                .iter()
                .position(|port| !port.main && port.channels > 0)
        } else {
            None
        };
        let mut key_owned = Vec::with_capacity(ports.len());

        for port in ports {
            let mut port_owned = Vec::new();
            let first = pointers.len();
            firsts.push(first);
            let mut main_port = MainPort { first, mono: None };
            for channel in 0..port.channels {
                // The caller's audio goes in the first two channels of the
                // main port, or in its only one.
                let callers = port.main && port.channels >= 2 && channel < 2;
                let index = if port.main && port.channels == 1 {
                    owned.push(buffer());
                    main_port.mono = Some(owned.len() - 1);
                    owned.len() - 1
                } else if is_input && !port.main && channel < 2 {
                    owned.push(buffer());
                    port_owned.push(owned.len() - 1);
                    owned.len() - 1
                } else if callers || is_input {
                    0
                } else {
                    owned.push(buffer());
                    owned.len() - 1
                };
                pointers.push(owned[index].as_mut_ptr());
            }
            key_owned.push(port_owned);
            if port.main && port.channels > 0 && main.is_none() {
                main = Some(main_port);
            }
        }

        let buffers = ports
            .iter()
            .zip(&firsts)
            .map(|(port, &first)| {
                let silent = is_input && !port.main;
                clap_audio_buffer {
                    // SAFETY: `first` is at most the length of `pointers`.
                    // The vector is not touched again, so the address holds.
                    data32: unsafe { pointers.as_mut_ptr().add(first) },
                    data64: ptr::null_mut(),
                    channel_count: port.channels,
                    latency: 0,
                    constant_mask: if silent { u64::MAX } else { 0 },
                }
            })
            .collect();

        Self {
            buffers,
            pointers,
            owned,
            main,
            key_port,
            key_owned,
        }
    }

    fn key_input(&mut self, key: Option<&[[f32; 2]]>, frames: usize) {
        self.owned[0][..frames].fill(0.0);
        for (port, owned_channels) in self.key_owned.iter().enumerate() {
            if owned_channels.is_empty() {
                continue;
            }
            let selected = Some(port) == self.key_port;
            let mono = owned_channels.len() == 1;
            for (side, owned) in owned_channels.iter().copied().enumerate() {
                for (index, output) in self.owned[owned][..frames].iter_mut().enumerate() {
                    let pair = if selected {
                        key.and_then(|key| key.get(index))
                            .copied()
                            .unwrap_or([0.0; 2])
                    } else {
                        [0.0; 2]
                    };
                    let value = if mono {
                        (pair[0] + pair[1]) * 0.5
                    } else {
                        pair[side]
                    };
                    *output = if value.is_finite() {
                        value.clamp(-1.0e6, 1.0e6)
                    } else {
                        0.0
                    };
                }
            }
            self.buffers[port].constant_mask = if selected && key.is_some() {
                0
            } else {
                u64::MAX
            };
        }
    }
    fn select_key(&mut self, input: Option<u32>) {
        self.key_port = match input {
            Some(index) => self
                .key_owned
                .get(index as usize)
                .filter(|owned| !owned.is_empty())
                .map(|_| index as usize),
            None => self.key_owned.iter().position(|owned| !owned.is_empty()),
        };
    }

    /// Points the main port at the caller's two channels, if it takes them
    /// directly.
    fn aim(&mut self, left: *mut f32, right: *mut f32) {
        if let Some(main) = &self.main
            && main.mono.is_none()
        {
            self.pointers[main.first] = left;
            self.pointers[main.first + 1] = right;
        }
    }

    /// The one-channel stand-in of the main port, if it has one.
    fn mono(&mut self, frames: usize) -> Option<&mut [f32]> {
        let index = self.main.as_ref()?.mono?;
        Some(&mut self.owned[index][..frames])
    }
}

/// The audio-thread half of a CLAP plugin.
pub(crate) struct ClapProcessor {
    processor: PluginAudioProcessor<WindfallHost>,
    tail: Option<PluginTail>,
    tail_read: bool,
    events: EventList,
    dialect: Dialect,
    inputs: PortSet,
    outputs: PortSet,
}

// SAFETY: the raw pointers in the port sets point into memory the port sets
// own, or are overwritten with the caller's buffers before every use. None
// of it is shared with another thread: the processor is used by one thread
// at a time, which `&mut self` on every method enforces.
unsafe impl Send for ClapProcessor {}

impl ClapProcessor {
    pub fn new(
        processor: StoppedPluginAudioProcessor<WindfallHost>,
        tail: Option<PluginTail>,
        dialect: Dialect,
        inputs: &[PortDesc],
        outputs: &[PortDesc],
        max_block: usize,
    ) -> Self {
        Self {
            processor: processor.into(),
            tail,
            tail_read: false,
            events: EventList::new(dialect),
            dialect,
            inputs: PortSet::new(inputs, max_block, true),
            outputs: PortSet::new(outputs, max_block, false),
        }
    }

    /// Gives back clack's processor, stopped, for deactivation.
    pub fn into_stopped(self) -> StoppedPluginAudioProcessor<WindfallHost> {
        self.processor.into_stopped()
    }
}

fn transport_event(transport: &Transport) -> Result<TransportEvent, ProcessFailed> {
    let (bar_start, bar_number) = transport.bar_position().ok_or(ProcessFailed)?;
    let mut flags = TransportFlags::HAS_TEMPO
        | TransportFlags::HAS_BEATS_TIMELINE
        | TransportFlags::HAS_SECONDS_TIMELINE
        | TransportFlags::HAS_TIME_SIGNATURE;
    if transport.playing {
        flags |= TransportFlags::IS_PLAYING;
    }
    Ok(TransportEvent {
        header: EventHeader::new_core(0, EventFlags::empty()),
        flags,
        song_pos_beats: BeatTime::from_float(transport.position_beats),
        song_pos_seconds: SecondsTime::from_float(transport.position_seconds),
        tempo: transport.tempo_bpm,
        tempo_inc: 0.0,
        loop_start_beats: BeatTime::from_int(0),
        loop_end_beats: BeatTime::from_int(0),
        loop_start_seconds: SecondsTime::from_int(0),
        loop_end_seconds: SecondsTime::from_int(0),
        bar_start: BeatTime::from_float(bar_start),
        bar_number,
        time_signature_numerator: transport.numerator,
        time_signature_denominator: transport.denominator,
    })
}

impl ProcessorBackend for ClapProcessor {
    fn supports_midi_expression(&self) -> bool {
        self.dialect == Dialect::Midi
    }
    fn midi_expression_mask(&self, _channel: u8) -> u32 {
        if self.dialect == Dialect::Midi {
            0x7f
        } else {
            0
        }
    }
    fn supports_note_instances(&self) -> bool {
        self.dialect == Dialect::Clap
    }
    fn note_expression_mask(&self, _channel: u8) -> u32 {
        if self.dialect == Dialect::Clap {
            0x7f
        } else {
            0
        }
    }
    fn set_sidechain_input(&mut self, input: Option<u32>) {
        self.inputs.select_key(input);
    }
    fn process(
        &mut self,
        audio: AudioIo<'_>,
        events: &[HostEvent],
        transport: &Transport,
        steady_time: u64,
        out: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        self.process_sidechain(audio, None, events, transport, steady_time, out)
    }
    fn process_sidechain(
        &mut self,
        audio: AudioIo<'_>,
        key: Option<&[[f32; 2]]>,
        events: &[HostEvent],
        transport: &Transport,
        steady_time: u64,
        out: &mut dyn FnMut(PluginEvent),
    ) -> Result<BlockResult, ProcessFailed> {
        let transport = transport_event(transport)?;
        let frames = audio.frames();
        self.inputs.key_input(key, frames);
        self.events.fill(events, self.dialect);

        // The caller's slices become raw pointers here and are not used as
        // slices again until the plugin has returned.
        let (in_left, in_right, out_left, out_right) = match audio {
            AudioIo::InPlace { left, right } => {
                let (left, right) = (left.as_mut_ptr(), right.as_mut_ptr());
                (left, right, left, right)
            }
            AudioIo::Separate {
                in_left,
                in_right,
                out_left,
                out_right,
            } => {
                if let Some(mono) = self.inputs.mono(frames) {
                    for (index, sample) in mono.iter_mut().enumerate() {
                        *sample = (in_left[index] + in_right[index]) * 0.5;
                    }
                }
                (
                    in_left.as_mut_ptr(),
                    in_right.as_mut_ptr(),
                    out_left.as_mut_ptr(),
                    out_right.as_mut_ptr(),
                )
            }
        };
        self.inputs.aim(in_left, in_right);
        self.outputs.aim(out_left, out_right);

        let status = {
            let _audio_call = AudioCall::enter();
            let started = self
                .processor
                .ensure_processing_started()
                .map_err(|_| ProcessFailed)?;
            let input_events = InputEvents::from_buffer(&self.events);
            let mut outgoing = Outgoing { sink: out };
            let mut output_events = OutputEvents::from_buffer(&mut outgoing);
            // SAFETY: every buffer of both sets points at `channel_count`
            // channels of at least `frames` samples each. The owned ones
            // were allocated for the largest block, which `frames` does not
            // exceed, and the main ports' were just aimed at the caller's
            // slices of exactly `frames` samples, which stay borrowed until
            // this function returns.
            let (inputs, mut outputs) = unsafe {
                (
                    InputAudioBuffers::from_raw_buffers(&self.inputs.buffers, frames as u32),
                    OutputAudioBuffers::from_raw_buffers(&mut self.outputs.buffers, frames as u32),
                )
            };
            let status = started
                .process(
                    &inputs,
                    &mut outputs,
                    &input_events,
                    &mut output_events,
                    Some(steady_time),
                    Some(&transport),
                )
                .map_err(|_| ProcessFailed)?;

            // The tail is read once, and again when the plugin says it
            // changed.
            let tail_changed = started.access_shared_handler(|shared| shared.take(flag::TAIL)) != 0;
            let mut tail = None;
            if let Some(extension) = self.tail
                && (tail_changed || !self.tail_read)
            {
                self.tail_read = true;
                tail = Some(match extension.get(&started.plugin_handle()) {
                    TailLength::Finite(frames) => frames.min(ENDLESS_TAIL - 1),
                    TailLength::Infinite => ENDLESS_TAIL,
                });
            }
            (status, tail)
        };

        if let Some(mono) = self.outputs.mono(frames) {
            // SAFETY: the output pointers are the caller's two slices of
            // `frames` samples, and the plugin is done with them.
            let (left, right) = unsafe {
                (
                    std::slice::from_raw_parts_mut(out_left, frames),
                    std::slice::from_raw_parts_mut(out_right, frames),
                )
            };
            left.copy_from_slice(mono);
            right.copy_from_slice(mono);
        }

        let (status, tail) = status;
        Ok(BlockResult {
            status: match status {
                ClapStatus::Continue => ProcessStatus::Continue,
                ClapStatus::ContinueIfNotQuiet => ProcessStatus::ContinueIfNotQuiet,
                ClapStatus::Tail => ProcessStatus::Tail,
                ClapStatus::Sleep => ProcessStatus::Sleep,
            },
            tail,
            dropped_events: self.events.dropped(),
        })
    }

    fn reset(&mut self) {
        let _audio_call = AudioCall::enter();
        self.processor.reset();
    }

    fn stop(&mut self) {
        let _audio_call = AudioCall::enter();
        self.processor.ensure_processing_stopped();
    }

    fn into_any(self: Box<Self>) -> Box<dyn Any> {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cfg(test)]
mod meter_tests {
    use super::*;
    use crate::MeterAnchor;

    #[test]
    fn independently_converted_ticks_report_the_exact_native_downbeat() {
        let transport = Transport {
            playing: true,
            tempo_bpm: 137.0,
            position_beats: 3845.0 / 960.0,
            position_seconds: 9.25,
            numerator: 7,
            denominator: 8,
            meter_anchor: Some(MeterAnchor {
                bar_origin_beats: 485.0 / 960.0,
                bar_origin_index: 1,
            }),
        };
        let event = transport_event(&transport).unwrap_or_else(|_| panic!("valid transport"));
        assert_eq!(event.bar_start, BeatTime::from_float(3845.0 / 960.0));
        assert_eq!(event.bar_number, 2);
        assert_eq!(event.song_pos_beats, BeatTime::from_float(3845.0 / 960.0));
        assert_eq!(event.song_pos_seconds, SecondsTime::from_float(9.25));
        assert_eq!(event.tempo, 137.0);
        assert_eq!(event.time_signature_numerator, 7);
        assert_eq!(event.time_signature_denominator, 8);
        assert!(event.flags.contains(TransportFlags::IS_PLAYING));
    }

    #[test]
    fn native_bars_keep_positions_immediately_before_the_downbeat_in_the_prior_bar() {
        let downbeat = 3845.0_f64 / 960.0;
        for (beats, start, number) in [
            (3844.0 / 960.0, 485.0 / 960.0, 1),
            (3844.5 / 960.0, 485.0 / 960.0, 1),
            (downbeat.next_down(), 485.0 / 960.0, 1),
            (downbeat, 3845.0 / 960.0, 2),
            (downbeat.next_up(), 3845.0 / 960.0, 2),
            (3845.5 / 960.0, 3845.0 / 960.0, 2),
            (7205.0 / 960.0, 7205.0 / 960.0, 3),
        ] {
            let event = transport_event(&Transport {
                position_beats: beats,
                numerator: 7,
                denominator: 8,
                meter_anchor: Some(MeterAnchor {
                    bar_origin_beats: 485.0 / 960.0,
                    bar_origin_index: 1,
                }),
                ..Transport::default()
            })
            .unwrap_or_else(|_| panic!("valid transport"));
            assert_eq!(event.bar_start, BeatTime::from_float(start), "{beats}");
            assert_eq!(event.bar_number, number, "{beats}");
            assert_eq!(event.song_pos_beats, BeatTime::from_float(beats));
        }
    }

    #[test]
    fn native_bar_fields_follow_shortened_song_bars() {
        let origin = 4001.0 / 960.0;
        for (beats, bar_start, bar_number) in [
            (origin, origin, 2),
            (origin + 3.499, origin, 2),
            (origin + 3.5, origin + 3.5, 3),
            (origin + 7.125, origin + 7.0, 4),
        ] {
            let transport = Transport {
                playing: true,
                tempo_bpm: 137.0,
                position_beats: beats,
                position_seconds: 9.25,
                numerator: 7,
                denominator: 8,
                meter_anchor: Some(MeterAnchor {
                    bar_origin_beats: origin,
                    bar_origin_index: 2,
                }),
            };
            let event = transport_event(&transport).unwrap_or_else(|_| panic!("valid transport"));
            assert_eq!(event.bar_start, BeatTime::from_float(bar_start));
            assert_eq!(event.bar_number, bar_number);
            assert_eq!(event.song_pos_beats, BeatTime::from_float(beats));
            assert_eq!(event.song_pos_seconds, SecondsTime::from_float(9.25));
            assert_eq!(event.tempo, 137.0);
            assert_eq!(event.time_signature_numerator, 7);
            assert_eq!(event.time_signature_denominator, 8);
            assert!(event.flags.contains(TransportFlags::IS_PLAYING));
        }
    }

    #[test]
    fn native_scalar_pattern_transport_retains_beat_zero_origin() {
        let event = transport_event(&Transport {
            position_beats: 8.125,
            ..Transport::default()
        })
        .unwrap_or_else(|_| panic!("valid scalar transport"));
        assert_eq!(event.bar_start, BeatTime::from_int(8));
        assert_eq!(event.bar_number, 2);
        assert!(!event.flags.contains(TransportFlags::IS_PLAYING));
    }

    #[test]
    fn invalid_anchor_refuses_native_transport_before_processing() {
        for anchor in [
            MeterAnchor {
                bar_origin_beats: f64::NAN,
                bar_origin_index: 0,
            },
            MeterAnchor {
                bar_origin_beats: -1.0,
                bar_origin_index: 0,
            },
            MeterAnchor {
                bar_origin_beats: 1.0,
                bar_origin_index: 0,
            },
            MeterAnchor {
                bar_origin_beats: 0.0,
                bar_origin_index: u32::MAX,
            },
        ] {
            assert!(
                transport_event(&Transport {
                    meter_anchor: Some(anchor),
                    ..Transport::default()
                })
                .is_err()
            );
        }
    }
}
