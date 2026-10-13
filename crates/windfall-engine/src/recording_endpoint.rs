//! Where recording input comes from: a capture endpoint, or the loopback of a
//! render endpoint.
//!
//! WASAPI can open a capture stream on an output device. It then delivers what
//! the system mixer sends to that device, stamped with the same device clock
//! as the output. Windfall lists those as inputs named `Loopback: <device>`,
//! so "what you hear" can be recorded through the ordinary timestamped capture
//! path with no cable. The name is the whole contract: a saved mixer input
//! route or a recording source keeps host and device strings as before.
//!
//! A loopback stream has exactly one format, the device's shared mix format.
//! It delivers no packets while nothing renders to the device, so a loopback
//! of an idle endpoint ends a take with the ordinary missing-audio error
//! instead of inventing silence at the wrong time.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{ErrorKind, SupportedStreamConfig, SupportedStreamConfigRange};

/// Why a capture stopped being trustworthy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FaultReason {
    /// The driver said samples were lost between two packets.
    Dropout = 1,
    /// The device went away or the system invalidated its stream.
    DeviceLost = 2,
    /// The writer fell so far behind that the capture queue filled up.
    Overflow = 3,
    /// The driver delivered a ragged buffer or a sample that is no number.
    InvalidAudio = 4,
    /// The driver reported an error of another kind.
    Driver = 5,
    /// The worker gave up for a reason it reports itself.
    Stopped = 6,
}

impl FaultReason {
    fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::Dropout,
            2 => Self::DeviceLost,
            3 => Self::Overflow,
            4 => Self::InvalidAudio,
            5 => Self::Driver,
            6 => Self::Stopped,
            _ => return None,
        })
    }

    /// One sentence for the person who was recording.
    pub fn sentence(self) -> &'static str {
        match self {
            Self::Dropout => "The input device reported a dropout: samples were lost.",
            Self::DeviceLost => {
                "The input device was disconnected or its stream was closed by the system."
            }
            Self::Overflow => "Recording could not keep up and the capture queue overflowed.",
            Self::InvalidAudio => "The input device delivered audio data that cannot be used.",
            Self::Driver => "The input driver reported an error.",
            Self::Stopped => "Recording input failed.",
        }
    }
}

/// What a driver error means for a capture, or `None` for one the stream
/// carries on from with every sample intact.
pub fn fault_of(kind: ErrorKind) -> Option<FaultReason> {
    match kind {
        // Neither loses audio: the thread runs at normal priority, or the
        // default device changed while this stream keeps its own.
        ErrorKind::RealtimeDenied | ErrorKind::DeviceChanged => None,
        ErrorKind::Xrun => Some(FaultReason::Dropout),
        ErrorKind::DeviceNotAvailable
        | ErrorKind::StreamInvalidated
        | ErrorKind::DeviceBusy
        | ErrorKind::HostUnavailable => Some(FaultReason::DeviceLost),
        _ => Some(FaultReason::Driver),
    }
}

/// The flag a capture's callbacks raise and its worker watches. It keeps the
/// first reason, so the take's error can say what actually happened. Raising
/// it is one atomic exchange: safe from the driver's callback.
#[derive(Debug, Default)]
pub struct InputFault(AtomicU8);

/// WASAPI's first packet can carry a discontinuity flag at a nonzero device
/// position (especially loopback). CPAL 0.18 treats that as an xrun. There is
/// no previous packet to lose at this boundary; every later xrun remains fatal.
/// This state belongs to one stream, not a whole multitrack capture group.
pub(crate) struct InputStartup {
    wasapi: bool,
    received: AtomicBool,
}

impl InputStartup {
    pub fn new(host: &str) -> Self {
        Self {
            wasapi: host == "WASAPI",
            received: AtomicBool::new(false),
        }
    }

    pub fn received(&self) {
        self.received.store(true, Ordering::Release);
    }

    pub fn report(&self, fault: &InputFault, error: cpal::Error) {
        if self.wasapi && !self.received.load(Ordering::Acquire) && error.kind() == ErrorKind::Xrun
        {
            std::mem::forget(error);
        } else {
            fault.report(error);
        }
    }
}

impl InputFault {
    pub fn new() -> Self {
        Self(AtomicU8::new(0))
    }

    /// Whether the capture has failed.
    pub fn load(&self, order: Ordering) -> bool {
        self.0.load(order) != 0
    }

    /// Marks the capture failed with no particular reason. `false` changes
    /// nothing: a failed capture stays failed.
    pub fn store(&self, failed: bool, _order: Ordering) {
        if failed {
            self.raise(FaultReason::Stopped);
        }
    }

    /// Marks the capture failed. The first reason raised is the one kept.
    pub fn raise(&self, reason: FaultReason) {
        let _ = self
            .0
            .compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Acquire);
    }

    pub fn reason(&self) -> Option<FaultReason> {
        FaultReason::from_code(self.0.load(Ordering::Acquire))
    }

    /// Takes a driver error from the stream's error callback.
    pub fn report(&self, error: cpal::Error) {
        if let Some(reason) = fault_of(error.kind()) {
            self.raise(reason);
        }
        // The callback runs on the driver's audio thread. Dropping the error
        // could free its message there, so it is leaked instead, as the
        // output stream's reporter does.
        std::mem::forget(error);
    }

    /// The reason as a sentence, followed by what became of the take.
    pub fn describe(&self, consequence: &str) -> String {
        let reason = self.reason().unwrap_or(FaultReason::Stopped);
        format!("{} {consequence}", reason.sentence())
    }
}

/// Prefix of an input name that means "the loopback of this output device".
pub const LOOPBACK_PREFIX: &str = "Loopback: ";

/// The host whose render endpoints can be opened for capture.
const LOOPBACK_HOST: &str = "WASAPI";

/// A device to capture from and how it must be opened.
pub(crate) struct InputEndpoint {
    pub device: cpal::Device,
    /// The device is a render endpoint captured through loopback.
    pub loopback: bool,
}

/// The output device a loopback input name refers to, if it is one.
pub fn loopback_target(input_name: &str) -> Option<&str> {
    input_name.strip_prefix(LOOPBACK_PREFIX)
}

/// The input name of an output device's loopback.
pub fn loopback_name(output_name: &str) -> String {
    format!("{LOOPBACK_PREFIX}{output_name}")
}

/// Whether a host can capture its own render endpoints.
pub(crate) fn host_has_loopback(host_name: &str) -> bool {
    cfg!(windows) && host_name == LOOPBACK_HOST
}

fn named(device: &cpal::Device, name: &str) -> bool {
    device.description().is_ok_and(|found| found.name() == name)
}

/// Finds the capture device, or the render device behind a loopback name.
pub(crate) fn find_input(host: &cpal::Host, name: &str) -> Result<Option<InputEndpoint>, String> {
    if let Some(target) = loopback_target(name) {
        if !host_has_loopback(host.id().name()) {
            return Ok(None);
        }
        return Ok(host
            .output_devices()
            .map_err(|error| error.to_string())?
            .find(|device| named(device, target))
            .map(|device| InputEndpoint {
                device,
                loopback: true,
            }));
    }
    Ok(host
        .input_devices()
        .map_err(|error| error.to_string())?
        .find(|device| named(device, name))
        .map(|device| InputEndpoint {
            device,
            loopback: false,
        }))
}

/// The one configuration a loopback stream accepts, as a range of one rate.
fn mix_format_range(format: &SupportedStreamConfig) -> SupportedStreamConfigRange {
    SupportedStreamConfigRange::new(
        format.channels(),
        format.sample_rate(),
        format.sample_rate(),
        *format.buffer_size(),
        format.sample_format(),
    )
}

/// The configurations capture can be opened with.
pub(crate) fn input_configs(
    endpoint: &InputEndpoint,
) -> Result<Vec<SupportedStreamConfigRange>, String> {
    if endpoint.loopback {
        let format = endpoint
            .device
            .default_output_config()
            .map_err(|error| error.to_string())?;
        return Ok(vec![mix_format_range(&format)]);
    }
    Ok(endpoint
        .device
        .supported_input_configs()
        .map_err(|error| error.to_string())?
        .collect())
}

/// The output devices of a host whose loopback can be listed as an input,
/// with the configuration each would be captured at.
pub(crate) fn loopback_inputs(host: &cpal::Host) -> Vec<(String, SupportedStreamConfigRange)> {
    if !host_has_loopback(host.id().name()) {
        return Vec::new();
    }
    let Ok(devices) = host.output_devices() else {
        return Vec::new();
    };
    devices
        .filter_map(|device| {
            let name = device.description().ok()?.name().to_owned();
            let format = device.default_output_config().ok()?;
            Some((loopback_name(&name), mix_format_range(&format)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_wasapi_discontinuity_before_the_first_packet_is_a_start_boundary() {
        let startup = InputStartup::new("WASAPI");
        let fault = InputFault::new();
        startup.report(&fault, ErrorKind::Xrun.into());
        assert_eq!(fault.reason(), None);
        startup.received();
        startup.report(&fault, ErrorKind::Xrun.into());
        assert_eq!(fault.reason(), Some(FaultReason::Dropout));
        let fault = InputFault::new();
        InputStartup::new("ASIO").report(&fault, ErrorKind::Xrun.into());
        assert_eq!(fault.reason(), Some(FaultReason::Dropout));
        let fault = InputFault::new();
        InputStartup::new("WASAPI").report(&fault, ErrorKind::DeviceNotAvailable.into());
        assert_eq!(fault.reason(), Some(FaultReason::DeviceLost));
    }

    #[test]
    fn a_loopback_name_round_trips_and_an_ordinary_name_is_not_one() {
        let name = loopback_name("Speakers (Interface)");
        assert_eq!(name, "Loopback: Speakers (Interface)");
        assert_eq!(loopback_target(&name), Some("Speakers (Interface)"));
        assert_eq!(loopback_target("Microphone (Interface)"), None);
        // A capture device that merely mentions the word is still a capture
        // device: only the exact prefix selects loopback.
        assert_eq!(loopback_target("Stereo Mix Loopback: 2"), None);
    }

    #[test]
    fn a_fault_keeps_its_first_reason_and_says_it_in_words() {
        let fault = InputFault::new();
        assert!(!fault.load(Ordering::Acquire));
        assert_eq!(fault.reason(), None);
        fault.store(false, Ordering::Release);
        assert!(!fault.load(Ordering::Acquire));
        fault.raise(FaultReason::Overflow);
        fault.raise(FaultReason::DeviceLost);
        fault.store(true, Ordering::Release);
        assert!(fault.load(Ordering::Acquire));
        assert_eq!(fault.reason(), Some(FaultReason::Overflow));
        assert_eq!(
            fault.describe("The take was discarded."),
            "Recording could not keep up and the capture queue overflowed. The take was discarded."
        );
        // A worker that gave up without a driver reason still reads as a
        // failure, in general words.
        let plain = InputFault::new();
        plain.store(true, Ordering::Release);
        assert_eq!(plain.reason(), Some(FaultReason::Stopped));
    }

    #[test]
    fn driver_errors_that_lose_no_audio_do_not_end_a_capture() {
        for kind in [ErrorKind::RealtimeDenied, ErrorKind::DeviceChanged] {
            let fault = InputFault::new();
            fault.report(cpal::Error::new(kind));
            assert!(!fault.load(Ordering::Acquire), "{kind:?}");
        }
        for (kind, reason) in [
            (ErrorKind::Xrun, FaultReason::Dropout),
            (ErrorKind::DeviceNotAvailable, FaultReason::DeviceLost),
            (ErrorKind::StreamInvalidated, FaultReason::DeviceLost),
            (ErrorKind::BackendError, FaultReason::Driver),
        ] {
            let fault = InputFault::new();
            fault.report(cpal::Error::new(kind));
            assert_eq!(fault.reason(), Some(reason), "{kind:?}");
        }
    }

    #[test]
    fn only_the_wasapi_host_offers_loopback() {
        assert_eq!(host_has_loopback("WASAPI"), cfg!(windows));
        assert!(!host_has_loopback("ASIO"));
        assert!(!host_has_loopback("ALSA"));
    }

    #[test]
    fn a_mix_format_becomes_a_range_of_exactly_its_rate() {
        let format = SupportedStreamConfig::new(
            2,
            48_000,
            cpal::SupportedBufferSize::Range { min: 480, max: 480 },
            cpal::SampleFormat::F32,
        );
        let range = mix_format_range(&format);
        assert_eq!(range.channels(), 2);
        assert_eq!(range.min_sample_rate(), 48_000);
        assert_eq!(range.max_sample_rate(), 48_000);
        assert_eq!(range.sample_format(), cpal::SampleFormat::F32);
        // A request at another rate clamps to the one the device mixes at.
        assert_eq!(
            44_100_u32.clamp(range.min_sample_rate(), range.max_sample_rate()),
            48_000
        );
    }
}
