//! Bounded control framing, used only on worker/helper owner threads.

use super::{
    adapter::ParameterSpec,
    protocol::{Config, Identity, Kind, ProtocolError},
};
use crate::{MAX_STATE_BYTES, PluginState};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Read, Write},
    net::TcpStream,
};

const MAGIC: &[u8; 4] = b"WFCB";
const VERSION: u32 = 1;
const PREFIX: usize = 24;
pub const MAX_METADATA_BYTES: usize = 1 << 20;
pub const MAX_PACKET_BYTES: usize = PREFIX + MAX_METADATA_BYTES + MAX_STATE_BYTES;

/// The wire limit covers the complete WFPS container, not only native bytes.
pub(crate) fn checked_state_size(bytes: usize) -> io::Result<()> {
    if bytes > MAX_STATE_BYTES {
        return Err(invalid(
            "native state wrapper exceeds bridge control budget",
        ));
    }
    Ok(())
}
fn checked_frame_size(metadata: usize, state: usize) -> io::Result<usize> {
    checked_state_size(state)?;
    if metadata == 0 || metadata > MAX_METADATA_BYTES {
        return Err(invalid("invalid bridge control lengths"));
    }
    PREFIX
        .checked_add(metadata)
        .and_then(|size| size.checked_add(state))
        .ok_or_else(|| invalid("bridge control length overflow"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub session: u64,
    pub token: u64,
    pub revision: u64,
    pub binding: u64,
}
impl From<Identity> for Owner {
    fn from(value: Identity) -> Self {
        Self {
            session: value.session,
            token: value.token,
            revision: value.revision,
            binding: value.binding,
        }
    }
}
impl From<Owner> for Identity {
    fn from(value: Owner) -> Self {
        Self {
            session: value.session,
            token: value.token,
            revision: value.revision,
            binding: value.binding,
        }
    }
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub owner: Owner,
    pub sample_rate: u32,
    pub block: u32,
    pub native_latency: u32,
    pub instrument: bool,
}
impl From<Config> for Settings {
    fn from(config: Config) -> Self {
        Self {
            owner: config.identity.into(),
            sample_rate: config.sample_rate,
            block: config.block as u32,
            native_latency: config.native_latency as u32,
            instrument: config.kind == Kind::Instrument,
        }
    }
}
impl Settings {
    pub fn config(self) -> Result<Config, ProtocolError> {
        Config {
            identity: self.owner.into(),
            sample_rate: self.sample_rate,
            block: self.block as usize,
            native_latency: self.native_latency as usize,
            kind: if self.instrument {
                Kind::Instrument
            } else {
                Kind::Effect
            },
        }
        .validate()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ControlParameter {
    pub id: u32,
    pub min: f64,
    pub max: f64,
    pub value: f64,
    pub read_only: bool,
    pub stepped: bool,
}

/// Native inspector metadata returned by the creating helper owner.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DescribedParameter {
    pub spec: ControlParameter,
    pub name: String,
    pub automatable: bool,
}
impl From<ParameterSpec> for ControlParameter {
    fn from(value: ParameterSpec) -> Self {
        Self {
            id: value.id,
            min: value.min,
            max: value.max,
            value: value.value,
            read_only: value.read_only,
            stepped: value.stepped,
        }
    }
}
impl From<ControlParameter> for ParameterSpec {
    fn from(value: ControlParameter) -> Self {
        Self {
            id: value.id,
            min: value.min,
            max: value.max,
            value: value.value,
            read_only: value.read_only,
            stepped: value.stepped,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Message {
    Describe,
    /// Quiesce and read fresh capabilities; this helper stays inactive until retirement.
    Readmit { epoch: u64, pending: Vec<ControlParameter> },
    Described {
        parameters: Vec<DescribedParameter>,
        layout: crate::PluginLayout,
    },
    Load {
        #[serde(default)]
        wrapper: Box<crate::wrapper::WrapperOptions>,
        mapping: String,
        settings: Settings,
        path: String,
        id: String,
        format: String,
        approved_binary: crate::paths::PluginFileIdentity,
        parameters: Vec<ControlParameter>,
        offline: bool,
    },
    Ready {
        note_instances: bool,
        expression_masks: [u32; 16],
        native_latency: u32,
        tail: u64,
        parameters: Vec<ControlParameter>,
    },
    Capture {
        epoch: u64,
        desired_generation: u64,
        pending: Vec<ControlParameter>,
    },
    Captured {
        epoch: u64,
        processed_generation: u64,
        reconciled_generation: u64,
        parameters: Vec<ControlParameter>,
    },
    Editor {
        open: bool,
        options: crate::EditorOptions,
    },
    Shutdown,
    Stopped,
    Error {
        message: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    owner: Owner,
    body: Message,
}
#[derive(Debug, Clone)]
pub struct Packet {
    pub request: u64,
    pub owner: Owner,
    pub body: Message,
    pub state: Vec<u8>,
}
impl Packet {
    pub fn new(request: u64, owner: Owner, body: Message) -> Self {
        Self {
            request,
            owner,
            body,
            state: Vec::new(),
        }
    }
    pub fn validate(&self) -> io::Result<()> {
        if self.request == 0 || !Identity::from(self.owner).valid() {
            return Err(invalid("invalid bridge control identity/size"));
        }
        checked_state_size(self.state.len())?;
        if !self.state.is_empty() {
            if !matches!(
                self.body,
                Message::Load { .. } | Message::Captured { .. } | Message::Described { .. }
            ) {
                return Err(invalid("state on an unsupported control message"));
            }
            PluginState::from_bytes(self.state.clone())
                .content()
                .map_err(|_| invalid("malformed native state container"))?;
        }
        if let Message::Described { parameters, layout } = &self.body {
            let mut ids = std::collections::BTreeSet::new();
            if parameters.len() > super::protocol::PARAM_CAPACITY
                || parameters.iter().any(|parameter| {
                    !ParameterSpec::from(parameter.spec).valid()
                        || !ids.insert(parameter.spec.id)
                        || parameter.name.len() > 4096
                        || parameter.name.contains('\0')
                })
                || layout.audio_inputs.len() > crate::MAX_PORTS as usize
                || layout.audio_outputs.len() > crate::MAX_PORTS as usize
                || layout
                    .audio_inputs
                    .iter()
                    .chain(&layout.audio_outputs)
                    .any(|port| {
                        port.channels > crate::MAX_PORT_CHANNELS
                            || port.name.len() > 4096
                            || port.name.contains('\0')
                    })
                || self.state.is_empty()
            {
                return Err(invalid("malformed native discovery metadata"));
            }
        }
        if let Message::Readmit { epoch, pending } = &self.body {
            let mut ids = std::collections::BTreeSet::new();
            if *epoch == 0 || *epoch > u64::from(u32::MAX) || pending.len() > super::protocol::PARAM_CAPACITY
                || pending.iter().any(|param| !ParameterSpec::from(*param).valid() || !ids.insert(param.id)) {
                return Err(invalid("malformed capability re-admission intent"));
            }
        }
        Ok(())
    }
    pub(crate) fn check_encoding(&self) -> io::Result<()> {
        self.header().map(|_| ())
    }
    fn header(&self) -> io::Result<([u8; PREFIX], Vec<u8>)> {
        self.validate()?;
        let metadata = serde_json::to_vec(&Metadata {
            owner: self.owner,
            body: self.body.clone(),
        })
        .map_err(invalid)?;
        checked_frame_size(metadata.len(), self.state.len())?;
        let mut prefix = [0u8; PREFIX];
        prefix[..4].copy_from_slice(MAGIC);
        prefix[4..8].copy_from_slice(&VERSION.to_le_bytes());
        prefix[8..16].copy_from_slice(&self.request.to_le_bytes());
        prefix[16..20].copy_from_slice(&(metadata.len() as u32).to_le_bytes());
        prefix[20..24].copy_from_slice(&(self.state.len() as u32).to_le_bytes());
        Ok((prefix, metadata))
    }
    pub fn write(&self, socket: &mut TcpStream) -> io::Result<()> {
        let (prefix, metadata) = self.header()?;
        socket.write_all(&prefix)?;
        socket.write_all(&metadata)?;
        socket.write_all(&self.state)
    }
    /// Startup owner only. Keep framing across partial/nonblocking writes while
    /// checking the same whole-startup deadline, cancellation and child lifetime.
    #[cfg(any(windows, target_os = "macos", target_os = "linux", test))]
    pub(crate) fn write_bounded(
        &self,
        socket: &mut impl Write,
        mut check: impl FnMut() -> io::Result<()>,
    ) -> io::Result<usize> {
        check()?;
        let (prefix, metadata) = self.header()?;
        let mut backpressure = 0usize;
        for mut remaining in [&prefix[..], &metadata[..], &self.state[..]] {
            while !remaining.is_empty() {
                check()?;
                let count = remaining.len().min(64 << 10);
                match socket.write(&remaining[..count]) {
                    Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                    Ok(written) => remaining = &remaining[written..],
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        backpressure = backpressure.saturating_add(1);
                        std::thread::sleep(std::time::Duration::from_micros(200));
                    }
                    Err(error) => return Err(error),
                }
            }
        }
        check()?;
        Ok(backpressure)
    }
}
fn invalid(message: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

/// Incremental framing survives short/nonblocking/timeout reads without a
/// second reader, abandoned blocked threads or duplicated control requests.
#[derive(Default)]
pub struct Decoder {
    bytes: Vec<u8>,
}
pub(crate) enum ReadStep {
    Packet(Box<Packet>),
    /// One bounded read made progress, or an interrupted read must be retried.
    Progress,
    /// No bytes are available on the nonblocking socket (WouldBlock).
    Idle,
}
impl Decoder {
    pub fn poll(&mut self, socket: &mut TcpStream) -> io::Result<Option<Packet>> {
        Ok(match self.poll_step(socket)? {
            ReadStep::Packet(packet) => Some(*packet),
            ReadStep::Progress | ReadStep::Idle => None,
        })
    }
    pub(crate) fn poll_step(&mut self, socket: &mut impl Read) -> io::Result<ReadStep> {
        if let Some(packet) = self.parse()? {
            return Ok(ReadStep::Packet(Box::new(packet)));
        }
        let mut chunk = [0u8; 8192];
        match socket.read(&mut chunk) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "audio helper control connection closed",
                ));
            }
            Ok(length) => {
                if self
                    .bytes
                    .len()
                    .checked_add(length)
                    .is_none_or(|size| size > MAX_PACKET_BYTES)
                {
                    return Err(invalid("bridge control buffer too large"));
                }
                self.bytes.extend_from_slice(&chunk[..length]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(ReadStep::Idle),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                return Ok(ReadStep::Progress);
            }
            Err(error) => return Err(error),
        }
        Ok(match self.parse()? {
            Some(packet) => ReadStep::Packet(Box::new(packet)),
            None => ReadStep::Progress,
        })
    }
    fn parse(&mut self) -> io::Result<Option<Packet>> {
        if self.bytes.len() < PREFIX {
            return Ok(None);
        }
        let prefix = &self.bytes[..PREFIX];
        if &prefix[..4] != MAGIC
            || u32::from_le_bytes(prefix[4..8].try_into().expect("prefix")) != VERSION
        {
            return Err(invalid("unsupported bridge control version"));
        }
        let request = u64::from_le_bytes(prefix[8..16].try_into().expect("prefix"));
        let metadata_len = u32::from_le_bytes(prefix[16..20].try_into().expect("prefix")) as usize;
        let state = u32::from_le_bytes(prefix[20..24].try_into().expect("prefix")) as usize;
        let total = checked_frame_size(metadata_len, state)?;
        if self.bytes.len() < total {
            return Ok(None);
        }
        let metadata: Metadata =
            serde_json::from_slice(&self.bytes[PREFIX..PREFIX + metadata_len]).map_err(invalid)?;
        let packet = Packet {
            request,
            owner: metadata.owner,
            body: metadata.body,
            state: self.bytes[PREFIX + metadata_len..total].to_vec(),
        };
        packet.validate()?;
        self.bytes.drain(..total);
        Ok(Some(packet))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn described() -> Packet {
        let mut packet = Packet::new(
            1,
            Owner {
                session: 1,
                token: 1,
                revision: 1,
                binding: 1,
            },
            Message::Described {
                parameters: vec![DescribedParameter {
                    spec: ControlParameter {
                        id: 7,
                        min: 0.0,
                        max: 2.0,
                        value: 0.75,
                        read_only: false,
                        stepped: false,
                    },
                    name: "Native gain".into(),
                    automatable: true,
                }],
                layout: crate::PluginLayout::default(),
            },
        );
        packet.state = PluginState::native(&[1, 2, 3]).into_bytes();
        packet
    }
    #[test]
    fn discovery_metadata_roundtrips_with_native_state_and_rejects_invalid_shapes() {
        let packet = described();
        let mut encoded = Vec::new();
        packet.write_bounded(&mut encoded, || Ok(())).unwrap();
        let ReadStep::Packet(received) = Decoder::default()
            .poll_step(&mut std::io::Cursor::new(encoded))
            .unwrap()
        else {
            panic!("complete described reply must decode")
        };
        let received = *received;
        assert_eq!(received.state, packet.state);
        let Message::Described { parameters, .. } = received.body else {
            panic!("metadata type")
        };
        assert_eq!(parameters[0].name, "Native gain");
        assert_eq!(parameters[0].spec.value, 0.75);
        assert!(parameters[0].automatable);
        let mut duplicate = described();
        let Message::Described { parameters, .. } = &mut duplicate.body else {
            unreachable!()
        };
        parameters.push(parameters[0].clone());
        assert!(duplicate.validate().is_err());
        let mut invalid = described();
        let Message::Described { parameters, .. } = &mut invalid.body else {
            unreachable!()
        };
        parameters[0].spec.value = f64::NAN;
        assert!(invalid.validate().is_err());
        let mut missing = described();
        missing.state.clear();
        assert!(missing.validate().is_err());
    }
    #[test]
    fn discovery_metadata_obeys_existing_control_packet_budget() {
        let mut packet = described();
        let Message::Described { parameters, .. } = &mut packet.body else {
            unreachable!()
        };
        let prototype = parameters[0].clone();
        *parameters = (0..super::super::protocol::PARAM_CAPACITY)
            .map(|id| {
                let mut parameter = prototype.clone();
                parameter.spec.id = id as u32;
                parameter.name = "n".repeat(4096);
                parameter
            })
            .collect();
        assert!(packet.validate().is_ok());
        assert!(packet.check_encoding().is_err());
    }
    #[test]
    fn incomplete_reads_report_progress_and_eof_is_a_failure_not_idle() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&1u64.to_le_bytes());
        bytes.extend_from_slice(&10u32.to_le_bytes());
        bytes.extend_from_slice(&64u32.to_le_bytes());
        bytes.extend_from_slice(b"{");
        let mut stream = std::io::Cursor::new(bytes);
        let mut decoder = Decoder::default();
        assert!(matches!(
            decoder.poll_step(&mut stream).unwrap(),
            ReadStep::Progress
        ));
        assert_eq!(
            decoder.poll_step(&mut stream).err().unwrap().kind(),
            io::ErrorKind::UnexpectedEof
        );
        struct Empty;
        impl Read for Empty {
            fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
                assert_eq!(bytes.len(), 8192);
                Err(io::ErrorKind::WouldBlock.into())
            }
        }
        let before = decoder.bytes.len();
        assert!(matches!(
            decoder.poll_step(&mut Empty).unwrap(),
            ReadStep::Idle
        ));
        assert_eq!(decoder.bytes.len(), before);
    }
    #[test]
    fn wrapped_native_budget_and_frame_lengths_are_checked_before_payloads() {
        let overhead = PluginState::native(&[]).as_bytes().len();
        assert_eq!(overhead, 6);
        for (raw, accepted) in [
            (MAX_STATE_BYTES - overhead - 1, true),
            (MAX_STATE_BYTES - overhead, true),
            (MAX_STATE_BYTES - overhead + 1, false),
            (MAX_STATE_BYTES, false),
        ] {
            let wrapped = raw.checked_add(overhead).unwrap();
            assert_eq!(checked_state_size(wrapped).is_ok(), accepted);
            assert_eq!(checked_frame_size(1, wrapped).is_ok(), accepted);
            let mut prefix = [0u8; PREFIX];
            prefix[..4].copy_from_slice(MAGIC);
            prefix[4..8].copy_from_slice(&VERSION.to_le_bytes());
            prefix[8..16].copy_from_slice(&1u64.to_le_bytes());
            prefix[16..20].copy_from_slice(&1u32.to_le_bytes());
            prefix[20..24].copy_from_slice(&(wrapped as u32).to_le_bytes());
            let mut decoder = Decoder {
                bytes: prefix.to_vec(),
            };
            assert_eq!(decoder.parse().is_ok(), accepted);
            assert_eq!(decoder.bytes.len(), PREFIX);
        }
        assert_eq!(
            checked_frame_size(MAX_METADATA_BYTES, MAX_STATE_BYTES).unwrap(),
            MAX_PACKET_BYTES
        );
        for (metadata, state) in [
            (0, 0),
            (usize::MAX, 0),
            (1, usize::MAX),
            (MAX_METADATA_BYTES + 1, 0),
        ] {
            assert!(checked_frame_size(metadata, state).is_err());
        }
    }
    #[test]
    fn partial_control_writes_preserve_framing_and_check_every_retry() {
        struct Pressure {
            bytes: Vec<u8>,
            writes: usize,
        }
        impl Write for Pressure {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                self.writes += 1;
                if self.writes % 3 == 1 {
                    return Err(io::ErrorKind::WouldBlock.into());
                }
                if self.writes % 3 == 2 {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                let count = bytes.len().min(7);
                self.bytes.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let packet = Packet::new(
            7,
            Owner {
                session: 1,
                token: 2,
                revision: 3,
                binding: 4,
            },
            Message::Shutdown,
        );
        let mut socket = Pressure {
            bytes: Vec::new(),
            writes: 0,
        };
        let mut checks = 0;
        packet
            .write_bounded(&mut socket, || {
                checks += 1;
                Ok(())
            })
            .unwrap();
        assert!(checks > socket.writes);
        let mut decoder = Decoder {
            bytes: socket.bytes,
        };
        let decoded = decoder.parse().unwrap().unwrap();
        assert_eq!(decoded.request, packet.request);
        assert_eq!(decoded.owner, packet.owner);
        assert!(matches!(decoded.body, Message::Shutdown));
        let mut socket = Pressure {
            bytes: Vec::new(),
            writes: 0,
        };
        let mut checks = 0;
        let result = packet.write_bounded(&mut socket, || {
            checks += 1;
            if checks == 8 {
                Err(io::ErrorKind::TimedOut.into())
            } else {
                Ok(())
            }
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(socket.bytes.len() < PREFIX);
    }
    #[test]
    fn malformed_and_oversized_control_headers_fail_before_payload_allocation() {
        for length in [MAX_METADATA_BYTES + 1, u32::MAX as usize] {
            let mut decoder = Decoder::default();
            decoder.bytes.extend_from_slice(MAGIC);
            decoder.bytes.extend_from_slice(&VERSION.to_le_bytes());
            decoder.bytes.extend_from_slice(&1u64.to_le_bytes());
            decoder
                .bytes
                .extend_from_slice(&(length as u32).to_le_bytes());
            decoder.bytes.extend_from_slice(&0u32.to_le_bytes());
            assert!(decoder.parse().is_err());
            assert_eq!(decoder.bytes.len(), PREFIX);
        }
    }
}
